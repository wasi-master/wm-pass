//! FTP and FTPS over a raw connection.
//!
//! Written against sockets rather than a library: the control channel is line-based text.
//! What that costs is that every awkward corner is handled here: passive mode, TLS on the
//! data channel, and the fact that `LIST` output has no defined format, which the
//! `parse_*` functions below handle and the tests pin down.
//!
//! **On plain FTP.** With [`FtpSecurity::None`] the password crosses the network as text.
//! WebDAV and S3 refuse that outright; FTP allows it only because a lot of old NAS boxes
//! offer nothing else. What crosses is the encrypted vault either way.
//!
//! The TLS plumbing and the line reader here are shared with IMAP.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use wmpass_core::Secret;
use wmpass_net::NetPolicy;

use crate::{Fault, Result, Sink, Stored};

// ---- Sockets and TLS, shared with IMAP ----

pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// A TLS client over a TCP socket.
pub(crate) type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// Checks certificates against the public roots and the host name, as a browser does.
/// One config per session, so the data channel can resume the control channel's TLS
/// session, which many servers insist on.
pub(crate) fn tls_config() -> Result<Arc<ClientConfig>> {
    let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|_| Fault::Io)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Whether an I/O failure was the certificate being refused: not transient, and not a
/// password problem either.
pub(crate) fn io_fault(error: &io::Error) -> Fault {
    let refused = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .is_some_and(|inner| matches!(inner, rustls::Error::InvalidCertificate(_)));
    if refused { Fault::Unsafe } else { Fault::Io }
}

/// A TCP connection with time limits on every step.
pub(crate) fn connect(host: &str, port: u16, read_timeout: Duration) -> Result<TcpStream> {
    let addresses = (host, port).to_socket_addrs().map_err(|_| Fault::Io)?;
    let mut last = Fault::Io;
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(socket) => {
                socket.set_read_timeout(Some(read_timeout)).map_err(|_| Fault::Io)?;
                socket.set_write_timeout(Some(read_timeout)).map_err(|_| Fault::Io)?;
                socket.set_nodelay(true).map_err(|_| Fault::Io)?;
                return Ok(socket);
            }
            Err(_) => last = Fault::Io,
        }
    }
    Err(last)
}

/// TLS over a connected socket, the handshake done before this returns.
pub(crate) fn handshake(config: &Arc<ClientConfig>, host: &str, mut socket: TcpStream) -> Result<TlsStream> {
    let name = ServerName::try_from(host.to_owned()).map_err(|_| Fault::NotConfigured)?;
    let mut connection = ClientConnection::new(config.clone(), name).map_err(|_| Fault::Io)?;
    while connection.is_handshaking() {
        connection.complete_io(&mut socket).map_err(|error| io_fault(&error))?;
    }
    Ok(StreamOwned::new(connection, socket))
}

/// A connection, encrypted or not.
pub(crate) enum Conn {
    Plain(TcpStream),
    Tls(Box<TlsStream>),
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(socket) => socket.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(socket) => socket.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(socket) => socket.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

/// Reads lines and counted bytes off a stream, keeping what arrived early for the next read.
pub(crate) struct Lines<S> {
    stream: S,
    buffer: Vec<u8>,
}

impl<S: Read + Write> Lines<S> {
    pub(crate) fn new(stream: S) -> Self {
        Self { stream, buffer: Vec::new() }
    }

    fn fill(&mut self) -> io::Result<usize> {
        let mut chunk = [0u8; 8192];
        let read = self.stream.read(&mut chunk)?;
        self.buffer.extend_from_slice(&chunk[..read]);
        Ok(read)
    }

    /// One line without its line break, None at a clean end. Longer than `max` is an error.
    pub(crate) fn read_line(&mut self, max: usize) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(end) = self.buffer.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
            if self.buffer.len() > max {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
            }
            if self.fill()? == 0 {
                return if self.buffer.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(std::mem::take(&mut self.buffer)))
                };
            }
        }
    }

    /// Exactly `count` bytes.
    pub(crate) fn read_exact(&mut self, count: usize) -> io::Result<Vec<u8>> {
        while self.buffer.len() < count {
            if self.fill()? == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed"));
            }
        }
        Ok(self.buffer.drain(..count).collect())
    }

    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stream.write_all(bytes)
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }

    /// The stream back, to be wrapped in TLS. None when the server already sent more:
    /// bytes that arrived before the handshake would be read as if they came after it,
    /// which is how a STARTTLS injection works.
    pub(crate) fn into_inner(self) -> Option<S> {
        self.buffer.is_empty().then_some(self.stream)
    }
}

// ---- Listings ----

/// One line of a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    /// File name.
    pub name: String,
    /// Bytes, when the server said.
    pub size: Option<u64>,
    /// Milliseconds since 1970, 0 when not known.
    pub modified_ms: u64,
    /// A folder.
    pub is_dir: bool,
}

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const YEAR_MS: u64 = 365 * DAY_MS;

/// Days since 1970-01-01 for a civil date, after Howard Hinnant.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_ms(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    u64::try_from(seconds).ok().map(|s| s * 1000)
}

fn year_of(ms: u64) -> i64 {
    // The year whose first day is at or before the moment.
    let days = i64::try_from(ms / DAY_MS).unwrap_or(0);
    let mut year = 1970 + days / 366;
    while days_from_civil(year + 1, 1, 1) <= days {
        year += 1;
    }
    year
}

fn month_number(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let lower = name.to_ascii_lowercase();
    MONTHS.iter().position(|month| *month == lower).and_then(|index| i64::try_from(index + 1).ok())
}

fn number(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// An `MLSD` time, `20250807140233`, always UTC by RFC 3659. A fraction is dropped; 0
/// for anything unreadable.
pub fn parse_mlsd_time(value: &str) -> u64 {
    let whole = value.split('.').next().unwrap_or_default();
    if whole.len() != 14 {
        return 0;
    }
    let field = |range: std::ops::Range<usize>| whole.get(range).and_then(number);
    (|| civil_ms(field(0..4)?, field(4..6)?, field(6..8)?, field(8..10)?, field(10..12)?, field(12..14)?))().unwrap_or(0)
}

/// `MLSD`: `type=file;size=4096;modify=20250807140233; name.bin`. Facts before the space,
/// name after it; the name may itself contain spaces, so it is everything past the first.
pub fn parse_mlsd(lines: &[&str]) -> Vec<ListEntry> {
    let mut out = Vec::new();
    for line in lines {
        let Some((facts, name)) = line.split_once(' ') else { continue };
        let name = name.trim();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let mut entry = ListEntry { name: name.to_owned(), size: None, modified_ms: 0, is_dir: false };
        for fact in facts.split(';') {
            let Some((key, value)) = fact.split_once('=') else { continue };
            match key.to_ascii_lowercase().as_str() {
                "size" => entry.size = value.parse().ok(),
                "modify" => entry.modified_ms = parse_mlsd_time(value),
                // A server may say "cdir" or "pdir" for . and .., already dropped.
                "type" => entry.is_dir = value.eq_ignore_ascii_case("dir"),
                _ => {}
            }
        }
        out.push(entry);
    }
    out
}

/// Splits off `count` whitespace-separated fields; the rest of the line is the last item.
fn fields(line: &str, count: usize) -> Option<(Vec<&str>, &str)> {
    let mut rest = line.trim_start();
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let end = rest.find(char::is_whitespace)?;
        out.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    Some((out, rest.trim()))
}

/// The Unix listing's date: `Aug 7 14:02` for anything in the last six months, `Aug 7
/// 2024` for anything older. The first has no year, so take this year, and if that lands
/// more than a day ahead, it meant last year.
fn unix_date(month: &str, day: &str, year_or_time: &str, now_ms: u64) -> u64 {
    let (Some(month), Some(day)) = (month_number(month), number(day)) else { return 0 };
    if let Some((hour, minute)) = year_or_time.split_once(':') {
        let (Some(hour), Some(minute)) = (number(hour), number(minute)) else { return 0 };
        let Some(parsed) = civil_ms(year_of(now_ms), month, day, hour, minute, 0) else { return 0 };
        if parsed > now_ms + DAY_MS { parsed.saturating_sub(YEAR_MS) } else { parsed }
    } else {
        number(year_or_time).and_then(|year| civil_ms(year, month, day, 0, 0, 0)).unwrap_or(0)
    }
}

/// `-rw-r--r-- 1 owner group 4096 Aug 7 14:02 name.bin`
fn parse_unix(line: &str, now_ms: u64) -> Option<ListEntry> {
    let kind = line.chars().next()?;
    if line.len() < 10 || !matches!(kind, '-' | 'd' | 'l') {
        return None;
    }
    let (parts, rest) = fields(line, 8)?;
    let name = rest.split(" -> ").next().unwrap_or(rest).trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(ListEntry {
        name: name.to_owned(),
        size: parts[4].parse().ok(),
        modified_ms: unix_date(parts[5], parts[6], parts[7], now_ms),
        is_dir: kind == 'd',
    })
}

fn is_dos_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && (1..=2).contains(&parts[0].len())
        && (1..=2).contains(&parts[1].len())
        && (2..=4).contains(&parts[2].len())
}

/// `08-07-25  02:02PM       4096 name.bin`
fn parse_dos(line: &str) -> Option<ListEntry> {
    let (parts, name) = fields(line, 3)?;
    // A date, not merely something with a hyphen in it: `drwxr-xr-x` has three.
    if !is_dos_date(parts[0]) {
        return None;
    }
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let is_dir = parts[2].eq_ignore_ascii_case("<DIR>");
    let modified_ms = (|| {
        let mut date = parts[0].split('-').filter_map(number);
        let (month, day, year) = (date.next()?, date.next()?, date.next()?);
        let year = match year {
            0..=69 => 2000 + year,
            70..=99 => 1900 + year,
            _ => year,
        };
        let time = parts[1].to_ascii_uppercase();
        let (clock, pm) = match (time.strip_suffix("PM"), time.strip_suffix("AM")) {
            (Some(clock), _) => (clock.to_owned(), true),
            (_, Some(clock)) => (clock.to_owned(), false),
            _ => return None,
        };
        let (hour, minute) = clock.split_once(':')?;
        let (hour, minute) = (number(hour)?, number(minute)?);
        if !(1..=12).contains(&hour) {
            return None;
        }
        let hour = hour % 12 + if pm { 12 } else { 0 };
        civil_ms(year, month, day, hour, minute, 0)
    })()
    .unwrap_or(0);
    Some(ListEntry {
        name: name.to_owned(),
        size: if is_dir { None } else { parts[2].parse().ok() },
        modified_ms,
        is_dir,
    })
}

/// `LIST`, in either of the two shapes servers actually emit: Unix `ls -l`, or DOS from
/// an old Windows server. `now_ms` resolves the Unix form's missing year.
pub fn parse_list(lines: &[&str], now_ms: u64) -> Vec<ListEntry> {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .filter_map(|line| parse_unix(line, now_ms).or_else(|| parse_dos(line)))
        .collect()
}

fn six_numbers(text: &[u8]) -> Option<[u32; 6]> {
    let mut at = 0;
    let mut out = [0u32; 6];
    for (index, slot) in out.iter_mut().enumerate() {
        if index > 0 {
            while text.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            if text.get(at) != Some(&b',') {
                return None;
            }
            at += 1;
            while text.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
        }
        let start = at;
        while text.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        *slot = std::str::from_utf8(text.get(start..at)?).ok()?.parse().ok()?;
    }
    Some(out)
}

/// The host and port of a `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`. The port is two
/// bytes of a big-endian number, which is the part everyone gets wrong once.
pub fn parse_pasv(reply: &str) -> Option<(String, u16)> {
    let bytes = reply.as_bytes();
    let numbers = (0..bytes.len()).filter(|&at| bytes[at].is_ascii_digit()).find_map(|at| six_numbers(&bytes[at..]))?;
    if numbers.iter().any(|&n| n > 255) {
        return None;
    }
    let host = format!("{}.{}.{}.{}", numbers[0], numbers[1], numbers[2], numbers[3]);
    Some((host, u16::try_from(numbers[4] * 256 + numbers[5]).ok()?))
}

/// The port of a `229 Entering Extended Passive Mode (|||port|)`. No address, on purpose:
/// the data connection goes back to the control connection's host.
pub fn parse_epsv(reply: &str) -> Option<u16> {
    let start = reply.find("(|||")? + 4;
    let end = start + reply[start..].find("|)")?;
    number(&reply[start..end]).and_then(|port| u16::try_from(port).ok())
}

// ---- The sink ----

/// How an FTP connection is protected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FtpSecurity {
    /// `AUTH TLS` before the login, on the ordinary port 21. The default, and worth
    /// leaving on.
    #[default]
    Explicit,
    /// TLS from the first byte, usually on port 990.
    Implicit,
    /// Nothing: the password crosses as text.
    None,
}

/// An FTP server and folder.
#[derive(Clone, PartialEq, Eq)]
pub struct FtpConfig {
    /// Host name or address.
    pub host: String,
    /// Control port.
    pub port: u16,
    /// Account name.
    pub user: String,
    /// Password.
    pub password: Secret,
    /// Folder to write into, relative to wherever the login lands.
    pub path: String,
    /// TLS, and which kind.
    pub security: FtpSecurity,
}

impl Default for FtpConfig {
    fn default() -> Self {
        Self { host: String::new(), port: 21, user: String::new(), password: Secret::default(), path: String::new(), security: FtpSecurity::Explicit }
    }
}

impl std::fmt::Debug for FtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FtpConfig").field("security", &self.security).finish_non_exhaustive()
    }
}

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LINE: usize = 64 * 1024;
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
/// Suffix while uploading; renamed over the real name once whole.
const UPLOADING: &str = ".part";

/// What an FTP reply code means.
pub fn reply_fault(code: u16) -> Fault {
    match code {
        530 | 532 => Fault::PermissionLost,
        550 | 553 => Fault::TargetMissing,
        452 | 552 => Fault::OutOfSpace,
        _ => Fault::Io,
    }
}

fn code_of(reply: &str) -> u16 {
    reply.get(..3).and_then(|code| code.parse().ok()).unwrap_or(0)
}

/// One control connection, and whatever data connections it opens. Short-lived on
/// purpose: a socket held open between runs is one that has silently died since.
struct Session<'a> {
    config: &'a FtpConfig,
    control: Lines<Conn>,
    /// Set once the control channel is encrypted; the data channel then is too.
    tls: Option<Arc<ClientConfig>>,
}

impl<'a> Session<'a> {
    fn open(config: &'a FtpConfig, tls: &Arc<ClientConfig>) -> Result<Self> {
        let host = config.host.trim();
        let socket = connect(host, config.port, READ_TIMEOUT)?;
        let (conn, encrypted) = match config.security {
            FtpSecurity::Implicit => (Conn::Tls(Box::new(handshake(tls, host, socket)?)), Some(tls.clone())),
            FtpSecurity::Explicit | FtpSecurity::None => (Conn::Plain(socket), None),
        };
        let mut session = Self { config, control: Lines::new(conn), tls: encrypted };
        session.expect_reply(220)?;
        if config.security == FtpSecurity::Explicit {
            session.expect("AUTH TLS", 234)?;
            let Some(Conn::Plain(socket)) = session.control.into_inner() else {
                return Err(Fault::Io);
            };
            session = Self { config, control: Lines::new(Conn::Tls(Box::new(handshake(tls, host, socket)?))), tls: Some(tls.clone()) };
        }
        if session.tls.is_some() {
            // Protect the data channel too. Without these two the file itself would cross
            // in the clear while only the password was encrypted. Implicit servers need
            // them as well: rclone's refuses PROT P and then sends data in the clear. A
            // server that will not protect the data is not safe to send the file to.
            for request in ["PBSZ 0", "PROT P"] {
                let reply = session.command(request)?;
                if code_of(&reply) != 200 {
                    return Err(Fault::Unsafe);
                }
            }
        }
        session.login()?;
        // Binary. The default is ASCII, which on some servers rewrites line endings
        // inside the file and quietly corrupts an encrypted one.
        session.expect("TYPE I", 200)?;
        Ok(session)
    }

    fn login(&mut self) -> Result<()> {
        let user = self.command(&format!("USER {}", self.config.user))?;
        match code_of(&user) {
            331 | 332 => {
                let password = Secret::from(format!("PASS {}", self.config.password.expose()));
                let reply = self.command(password.expose())?;
                if (200..300).contains(&code_of(&reply)) { Ok(()) } else { Err(Fault::PermissionLost) }
            }
            200..=299 => Ok(()),
            _ => Err(Fault::PermissionLost),
        }
    }

    /// One reply, including a multi-line one: `250-` opens it and a line starting `250 `
    /// ends it. Reading only the first line would leave the rest to be taken as the
    /// answer to every command after.
    fn read_reply(&mut self) -> Result<String> {
        let first = self.line()?;
        if first.len() < 4 || first.as_bytes()[3] != b'-' {
            return Ok(first);
        }
        let end = format!("{} ", &first[..3]);
        let mut all = first;
        loop {
            let line = self.line()?;
            all.push('\n');
            all.push_str(&line);
            if line.starts_with(&end) {
                return Ok(all);
            }
        }
    }

    fn line(&mut self) -> Result<String> {
        let line = self.control.read_line(MAX_LINE).map_err(|error| io_fault(&error))?.ok_or(Fault::Io)?;
        Ok(String::from_utf8_lossy(&line).into_owned())
    }

    fn expect_reply(&mut self, code: u16) -> Result<String> {
        let reply = self.read_reply()?;
        if code_of(&reply) == code { Ok(reply) } else { Err(reply_fault(code_of(&reply))) }
    }

    fn expect(&mut self, request: &str, code: u16) -> Result<String> {
        let reply = self.command(request)?;
        if code_of(&reply) == code { Ok(reply) } else { Err(reply_fault(code_of(&reply))) }
    }

    fn command(&mut self, request: &str) -> Result<String> {
        // A line break in a name or password would end the command early and start
        // another one of the server's choosing.
        if request.contains(['\r', '\n']) {
            return Err(Fault::NotConfigured);
        }
        let mut line = Vec::with_capacity(request.len() + 2);
        line.extend_from_slice(request.as_bytes());
        line.extend_from_slice(b"\r\n");
        self.control.write_all(&line).map_err(|error| io_fault(&error))?;
        self.control.flush().map_err(|error| io_fault(&error))?;
        self.read_reply()
    }

    fn change_to_folder(&mut self) -> Result<()> {
        let path = self.config.path.trim().trim_matches('/');
        if path.is_empty() {
            return Ok(());
        }
        let reply = self.command(&format!("CWD {path}"))?;
        if (200..300).contains(&code_of(&reply)) { Ok(()) } else { Err(Fault::TargetMissing) }
    }

    /// A passive-mode data socket. `EPSV` first, then `PASV`; either way the connection
    /// goes to the control connection's host. That is what works through NAT, where
    /// `PASV` hands out the server's private address, and it means a server cannot point
    /// this device at some other machine.
    fn open_data(&mut self) -> Result<TcpStream> {
        let epsv = self.command("EPSV")?;
        let port = match (code_of(&epsv), parse_epsv(&epsv)) {
            (229, Some(port)) => port,
            _ => {
                let pasv = self.command("PASV")?;
                if code_of(&pasv) != 227 {
                    return Err(reply_fault(code_of(&pasv)));
                }
                parse_pasv(&pasv).ok_or(Fault::Io)?.1
            }
        };
        connect(self.config.host.trim(), port, READ_TIMEOUT)
    }

    /// Starts a transfer: the data socket, then the command, then TLS on the data socket.
    /// The handshake comes after the command because servers such as vsftpd only start
    /// it once they know what the connection is for.
    fn start(&mut self, request: &str) -> Result<Conn> {
        let socket = self.open_data()?;
        let reply = self.command(request)?;
        if !(100..200).contains(&code_of(&reply)) {
            return Err(reply_fault(code_of(&reply)));
        }
        match &self.tls {
            Some(config) => Ok(Conn::Tls(Box::new(handshake(config, self.config.host.trim(), socket)?))),
            None => Ok(Conn::Plain(socket)),
        }
    }

    fn finish(&mut self) -> Result<()> {
        let done = self.read_reply()?;
        if (200..300).contains(&code_of(&done)) { Ok(()) } else { Err(reply_fault(code_of(&done))) }
    }

    /// Runs a command whose answer arrives on a data connection.
    fn data_in(&mut self, request: &str) -> Result<Vec<u8>> {
        let mut data = self.start(request)?;
        let mut bytes = Vec::new();
        match (&mut data).take(MAX_DOWNLOAD).read_to_end(&mut bytes) {
            Ok(_) => {}
            // Many servers close the data connection without TLS's close_notify. The 226
            // on the encrypted control channel is what says the transfer was whole.
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {}
            Err(error) => return Err(io_fault(&error)),
        }
        drop(data);
        self.finish()?;
        Ok(bytes)
    }

    fn store(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut data = self.start(&format!("STOR {name}"))?;
        data.write_all(bytes).map_err(|error| io_fault(&error))?;
        data.flush().map_err(|error| io_fault(&error))?;
        match &mut data {
            Conn::Tls(stream) => {
                stream.conn.send_close_notify();
                stream.flush().map_err(|error| io_fault(&error))?;
                let _ = stream.sock.shutdown(Shutdown::Write);
            }
            Conn::Plain(socket) => {
                let _ = socket.shutdown(Shutdown::Write);
            }
        }
        drop(data);
        self.finish()
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.expect(&format!("RNFR {from}"), 350)?;
        let reply = self.command(&format!("RNTO {to}"))?;
        if (200..300).contains(&code_of(&reply)) { Ok(()) } else { Err(reply_fault(code_of(&reply))) }
    }

    fn listing(&mut self) -> Result<Vec<ListEntry>> {
        // MLSD first: it is the only listing with a defined format. LIST is the fallback,
        // and its output is prose.
        if let Ok(machine) = self.data_in("MLSD") {
            let text = String::from_utf8_lossy(&machine).into_owned();
            let parsed = parse_mlsd(&text.lines().collect::<Vec<_>>());
            if !parsed.is_empty() {
                return Ok(parsed);
            }
        }
        let human = self.data_in("LIST")?;
        let text = String::from_utf8_lossy(&human).into_owned();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0));
        Ok(parse_list(&text.lines().collect::<Vec<_>>(), now))
    }

    fn close(mut self) {
        let _ = self.command("QUIT");
        if let Conn::Tls(stream) = &mut self.control.stream {
            stream.conn.send_close_notify();
            let _ = stream.flush();
        }
    }
}

/// An FTP folder.
pub struct Ftp {
    policy: NetPolicy,
    config: FtpConfig,
    tls: Option<Arc<ClientConfig>>,
}

impl std::fmt::Debug for Ftp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ftp").field("config", &self.config).finish_non_exhaustive()
    }
}

impl Ftp {
    /// A folder on the server `config` names.
    pub fn new(policy: NetPolicy, config: FtpConfig) -> Self {
        Self { policy, config, tls: None }
    }

    #[cfg(test)]
    fn trusting(mut self, config: Arc<ClientConfig>) -> Self {
        self.tls = Some(config);
        self
    }

    fn session<T>(&self, work: impl FnOnce(&mut Session<'_>) -> Result<T>) -> Result<T> {
        if self.config.host.trim().is_empty() || self.config.user.is_empty() {
            return Err(Fault::NotConfigured);
        }
        wmpass_net::http::check_socket(self.policy).map_err(|_| Fault::Blocked)?;
        let tls = match &self.tls {
            Some(config) => config.clone(),
            None => tls_config()?,
        };
        let mut session = Session::open(&self.config, &tls)?;
        let result = session.change_to_folder().and_then(|()| work(&mut session));
        session.close();
        result
    }
}

impl Sink for Ftp {
    async fn readiness(&self) -> Result<()> {
        self.session(|_| Ok(()))
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        self.session(|session| {
            let staged = format!("{name}{UPLOADING}");
            session.store(&staged, bytes)?;
            // FTP has a real rename. A stale target would make RNTO fail on some
            // servers, so it goes first; this is the same name being replaced.
            let _ = session.command(&format!("DELE {name}"))?;
            session.rename(&staged, name)
        })?;
        Ok(Stored { id: name.to_owned(), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        let entries = self.session(|session| session.listing())?;
        Ok(entries
            .into_iter()
            .filter(|entry| !entry.is_dir && !entry.name.ends_with(UPLOADING))
            .map(|entry| Stored { id: entry.name.clone(), name: entry.name, size: entry.size.unwrap_or(0) })
            .collect())
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        self.session(|session| session.data_in(&format!("RETR {}", file.name)))
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        // Already gone answers 550, which counts as done.
        self.session(|session| session.command(&format!("DELE {}", file.name)).map(|_| ()))
    }
}

#[cfg(test)]
pub(crate) mod test_tls {
    //! A certificate authority and a server certificate for `localhost` and `127.0.0.1`,
    //! made for these tests only.

    use std::sync::Arc;

    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{ClientConfig, RootCertStore, ServerConfig};

    const CA: &[u8] = include_bytes!("../testdata/ca.der");
    const LEAF: &[u8] = include_bytes!("../testdata/leaf.der");
    const KEY: &[u8] = include_bytes!("../testdata/leaf-key.der");

    pub(crate) fn server() -> Arc<ServerConfig> {
        let config = ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(LEAF), CertificateDer::from(CA)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(KEY)))
            .unwrap();
        Arc::new(config)
    }

    pub(crate) fn client() -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(CA)).unwrap();
        let config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Arc::new(config)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Mutex;

    use super::*;
    use crate::http::block_on;

    /// 2025-08-07T14:02:33Z
    const NOW: u64 = 1_754_575_353_000;

    #[test]
    fn mlsd_is_the_machine_readable_one_and_says_everything() {
        let entries = parse_mlsd(&[
            "type=cdir;modify=20250807140233; .",
            "type=dir;modify=20250807140233; subfolder",
            "type=file;size=4096;modify=20250807140233; wmpass-backup-1.wmvault",
        ]);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir);
        assert_eq!(entries[1], ListEntry { name: "wmpass-backup-1.wmvault".into(), size: Some(4096), modified_ms: NOW, is_dir: false });
    }

    #[test]
    fn a_name_with_a_space_survives_mlsd() {
        assert_eq!(parse_mlsd(&["type=file;size=1; my backup.bin"])[0].name, "my backup.bin");
    }

    #[test]
    fn unix_list_output_parses() {
        let entries = parse_list(
            &["drwxr-xr-x 2 owner group 4096 Aug  7 14:02 subfolder", "-rw-r--r-- 1 owner group 4096 Aug  7 14:02 wmpass-backup-1.wmvault"],
            NOW,
        );
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "wmpass-backup-1.wmvault");
        assert_eq!(entries[1].size, Some(4096));
        assert_eq!(entries[1].modified_ms, NOW - 33_000);
    }

    #[test]
    fn the_unix_formats_missing_year_is_inferred_backwards_never_forwards() {
        // Read on 2025-01-05: taking this year would put a December file eleven months ahead.
        let read_on = 1_736_035_200_000;
        let december = parse_list(&["-rw-r--r-- 1 o g 1 Dec 25 09:00 x.bin"], read_on);
        assert!(december[0].modified_ms < read_on);
        assert_eq!(december[0].modified_ms, civil_ms(2024, 12, 25, 9, 0, 0).unwrap());
    }

    #[test]
    fn an_old_unix_entry_has_a_year_instead_of_a_time() {
        let entry = &parse_list(&["-rw-r--r-- 1 o g 7 Aug  7 2020 x.bin"], NOW)[0];
        assert_eq!(entry.size, Some(7));
        assert!((1_596_000_000_000..1_597_000_000_000).contains(&entry.modified_ms));
    }

    #[test]
    fn dos_list_output_parses() {
        let entries = parse_list(&["08-07-25  02:02PM       <DIR>          subfolder", "08-07-25  02:02PM                 4096 x.bin"], NOW);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].size, Some(4096));
        assert_eq!(entries[1].modified_ms, NOW - 33_000);
    }

    #[test]
    fn dot_entries_are_never_listed() {
        assert!(parse_mlsd(&["type=cdir; .", "type=pdir; .."]).is_empty());
        assert!(parse_list(&["drwxr-xr-x 2 o g 4096 Aug  7 14:02 .", "drwxr-xr-x 2 o g 4096 Aug  7 14:02 .."], NOW).is_empty());
    }

    #[test]
    fn rubbish_lines_are_skipped_rather_than_fatal() {
        assert!(parse_list(&["", "   ", "total 8", "garbage"], NOW).is_empty());
        assert!(parse_mlsd(&["", "no-space-here"]).is_empty());
    }

    #[test]
    fn pasv_gives_a_host_and_a_port_built_from_two_bytes() {
        assert_eq!(parse_pasv("227 Entering Passive Mode (192,168,1,5,199,56)"), Some(("192.168.1.5".into(), 51_000)));
        assert_eq!(parse_pasv("227 Passive (10,0,0,1,0,21)."), Some(("10.0.0.1".into(), 21)));
    }

    #[test]
    fn a_malformed_pasv_reply_is_refused_rather_than_guessed_at() {
        assert_eq!(parse_pasv("227 Entering Passive Mode"), None);
        assert_eq!(parse_pasv("227 (1,2,3,4,5)"), None);
        // A byte out of range is not an address.
        assert_eq!(parse_pasv("227 (999,0,0,1,0,21)"), None);
    }

    #[test]
    fn epsv_gives_a_port_and_no_address() {
        assert_eq!(parse_epsv("229 Entering Extended Passive Mode (|||51000|)"), Some(51_000));
        assert_eq!(parse_epsv("229 Entering Extended Passive Mode"), None);
    }

    #[test]
    fn an_mlsd_timestamp_is_read_as_utc() {
        assert_eq!(parse_mlsd_time("20250807140233"), NOW);
        assert_eq!(parse_mlsd_time("20250807140233.123"), NOW);
        assert_eq!(parse_mlsd_time("nonsense"), 0);
    }

    /// How the fake server behaves.
    #[derive(Clone, Copy)]
    struct Script {
        tls: Option<FtpSecurity>,
        epsv: bool,
        mlsd: bool,
        refuse_prot: bool,
    }

    enum Stream {
        Plain(TcpStream),
        Tls(Box<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>),
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Plain(s) => s.read(buf),
                Self::Tls(s) => s.read(buf),
            }
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            match self {
                Self::Plain(s) => s.write(buf),
                Self::Tls(s) => s.write(buf),
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            match self {
                Self::Plain(s) => s.flush(),
                Self::Tls(s) => s.flush(),
            }
        }
    }

    fn accept_tls(socket: TcpStream) -> Stream {
        let connection = rustls::ServerConnection::new(test_tls::server()).unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        while stream.conn.is_handshaking() {
            stream.conn.complete_io(&mut stream.sock).unwrap();
        }
        Stream::Tls(Box::new(stream))
    }

    /// A small FTP server on this device, enough of RFC 959, 2228, 2428 and 3659 to be
    /// honest about what the client sends. Records every command.
    fn serve(script: Script, files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>, log: Arc<Mutex<Vec<String>>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for socket in listener.incoming().flatten() {
                let files = files.clone();
                let log = log.clone();
                std::thread::spawn(move || session(script, socket, &files, &log));
            }
        });
        port
    }

    fn session(script: Script, socket: TcpStream, files: &Mutex<BTreeMap<String, Vec<u8>>>, log: &Mutex<Vec<String>>) {
        let mut control = match script.tls {
            Some(FtpSecurity::Implicit) => accept_tls(socket),
            _ => Stream::Plain(socket),
        };
        let mut protected = script.tls == Some(FtpSecurity::Implicit);
        let mut passive: Option<TcpListener> = None;
        let mut renaming = String::new();
        let reply = |control: &mut Stream, text: &str| {
            control.write_all(format!("{text}\r\n").as_bytes()).unwrap();
            control.flush().unwrap();
        };
        reply(&mut control, "220-Welcome\r\n220 ready");
        loop {
            let mut reader = BufReader::new(&mut control);
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            drop(reader);
            let line = line.trim_end().to_owned();
            log.lock().unwrap().push(line.clone());
            let (verb, argument) = line.split_once(' ').unwrap_or((&line, ""));
            let argument = argument.to_owned();
            let data = |passive: &mut Option<TcpListener>, protected: bool| {
                let (socket, _) = passive.take().unwrap().accept().unwrap();
                if protected { accept_tls(socket) } else { Stream::Plain(socket) }
            };
            match verb {
                "AUTH" if script.tls == Some(FtpSecurity::Explicit) => {
                    reply(&mut control, "234 go ahead");
                    let Stream::Plain(socket) = control else { unreachable!() };
                    control = accept_tls(socket);
                }
                "PBSZ" => reply(&mut control, "200 PBSZ=0"),
                "PROT" if script.refuse_prot => reply(&mut control, "550 Action not taken"),
                "PROT" => {
                    protected = argument == "P";
                    reply(&mut control, "200 ok");
                }
                "USER" => reply(&mut control, "331 password please"),
                "PASS" if argument == "pw" => reply(&mut control, "230 in"),
                "PASS" => reply(&mut control, "530 no"),
                "TYPE" => reply(&mut control, "200 binary"),
                "CWD" if argument == "backups/wm" => reply(&mut control, "250 ok"),
                "CWD" => reply(&mut control, "550 no such folder"),
                "EPSV" if script.epsv => {
                    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                    let port = listener.local_addr().unwrap().port();
                    passive = Some(listener);
                    reply(&mut control, &format!("229 Entering Extended Passive Mode (|||{port}|)"));
                }
                "PASV" => {
                    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                    let port = listener.local_addr().unwrap().port();
                    passive = Some(listener);
                    // An address that is not this server's, which the client must ignore.
                    reply(&mut control, &format!("227 Entering Passive Mode (10,9,8,7,{},{})", port / 256, port % 256));
                }
                "MLSD" if script.mlsd => {
                    reply(&mut control, "150 here");
                    let mut out = data(&mut passive, protected);
                    for (name, bytes) in files.lock().unwrap().iter() {
                        out.write_all(format!("type=file;size={};modify=20250807140233; {name}\r\n", bytes.len()).as_bytes()).unwrap();
                    }
                    out.write_all(b"type=dir;modify=20250807140233; sub\r\n").unwrap();
                    finish(out);
                    reply(&mut control, "226 done");
                }
                "LIST" => {
                    reply(&mut control, "150 here");
                    let mut out = data(&mut passive, protected);
                    for (name, bytes) in files.lock().unwrap().iter() {
                        out.write_all(format!("-rw-r--r-- 1 o g {} Aug  7 14:02 {name}\r\n", bytes.len()).as_bytes()).unwrap();
                    }
                    finish(out);
                    reply(&mut control, "226 done");
                }
                "STOR" => {
                    reply(&mut control, "150 send it");
                    let mut input = data(&mut passive, protected);
                    let mut bytes = Vec::new();
                    let _ = input.read_to_end(&mut bytes);
                    files.lock().unwrap().insert(argument, bytes);
                    reply(&mut control, "226 stored");
                }
                "RETR" => match files.lock().unwrap().get(&argument).cloned() {
                    Some(bytes) => {
                        reply(&mut control, "150 here");
                        let mut out = data(&mut passive, protected);
                        out.write_all(&bytes).unwrap();
                        finish(out);
                        reply(&mut control, "226 sent");
                    }
                    None => reply(&mut control, "550 no such file"),
                },
                "DELE" => match files.lock().unwrap().remove(&argument) {
                    Some(_) => reply(&mut control, "250 deleted"),
                    None => reply(&mut control, "550 no such file"),
                },
                "RNFR" => {
                    renaming = argument;
                    reply(&mut control, "350 ready");
                }
                "RNTO" => {
                    let moved = files.lock().unwrap().remove(&renaming).unwrap();
                    files.lock().unwrap().insert(argument, moved);
                    reply(&mut control, "250 renamed");
                }
                "QUIT" => {
                    reply(&mut control, "221 bye");
                    return;
                }
                _ => reply(&mut control, "502 not here"),
            }
        }
    }

    fn finish(mut out: Stream) {
        if let Stream::Tls(stream) = &mut out {
            stream.conn.send_close_notify();
            let _ = stream.flush();
        }
    }

    fn round_trip(script: Script, security: FtpSecurity) -> Vec<String> {
        let files = Arc::new(Mutex::new(BTreeMap::new()));
        let log = Arc::new(Mutex::new(Vec::new()));
        let port = serve(script, files.clone(), log.clone());
        let config = FtpConfig { host: "localhost".into(), port, user: "me".into(), password: "pw".into(), path: "/backups/wm/".into(), security };
        let sink = Ftp::new(NetPolicy::Any, config).trusting(test_tls::client());
        block_on(sink.readiness()).unwrap();
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let stored = block_on(sink.write("a file.bin", &big)).unwrap();
        block_on(sink.write("second.bin", b"hello")).unwrap();
        block_on(sink.write("second.bin", b"hello again")).unwrap();
        let listed = block_on(sink.list()).unwrap();
        assert_eq!(listed.iter().map(|f| (f.name.as_str(), f.size)).collect::<Vec<_>>(), vec![("a file.bin", 300_000), ("second.bin", 11)]);
        assert_eq!(block_on(sink.read(&stored)).unwrap(), big);
        block_on(sink.delete(&listed[1])).unwrap();
        block_on(sink.delete(&listed[1])).unwrap();
        assert_eq!(files.lock().unwrap().keys().cloned().collect::<Vec<_>>(), vec!["a file.bin".to_owned()]);
        log.lock().unwrap().clone()
    }

    #[test]
    fn explicit_ftps_protects_the_data_channel_before_signing_in() {
        let log = round_trip(Script { tls: Some(FtpSecurity::Explicit), epsv: true, mlsd: true, refuse_prot: false }, FtpSecurity::Explicit);
        assert_eq!(&log[..6], ["AUTH TLS", "PBSZ 0", "PROT P", "USER me", "PASS pw", "TYPE I"]);
        assert!(log.contains(&"STOR a file.bin.part".to_owned()));
        assert!(log.contains(&"RNTO a file.bin".to_owned()));
        assert!(!log.contains(&"LIST".to_owned()), "MLSD answered, so LIST is not needed");
    }

    #[test]
    fn implicit_ftps_starts_encrypted_and_still_protects_the_data_channel() {
        let log = round_trip(Script { tls: Some(FtpSecurity::Implicit), epsv: true, mlsd: true, refuse_prot: false }, FtpSecurity::Implicit);
        assert_eq!(&log[..3], ["PBSZ 0", "PROT P", "USER me"]);
    }

    #[test]
    fn plain_ftp_falls_back_to_pasv_and_list_and_ignores_the_pasv_address() {
        let log = round_trip(Script { tls: None, epsv: false, mlsd: false, refuse_prot: false }, FtpSecurity::None);
        assert_eq!(&log[..3], ["USER me", "PASS pw", "TYPE I"]);
        assert!(log.contains(&"PASV".to_owned()) && log.contains(&"LIST".to_owned()));
    }

    #[test]
    fn refusals_are_named_and_offline_connects_to_nothing() {
        let files = Arc::new(Mutex::new(BTreeMap::new()));
        let log = Arc::new(Mutex::new(Vec::new()));
        let port = serve(Script { tls: None, epsv: true, mlsd: true, refuse_prot: false }, files, log.clone());
        let config = FtpConfig { host: "127.0.0.1".into(), port, user: "me".into(), password: "bad".into(), security: FtpSecurity::None, ..FtpConfig::default() };
        assert_eq!(block_on(Ftp::new(NetPolicy::Any, config.clone()).readiness()), Err(Fault::PermissionLost));
        let elsewhere = FtpConfig { password: "pw".into(), path: "nope".into(), ..config.clone() };
        assert_eq!(block_on(Ftp::new(NetPolicy::Any, elsewhere).readiness()), Err(Fault::TargetMissing));
        let injected = FtpConfig { password: "pw\r\nDELE x".into(), ..config.clone() };
        assert_eq!(block_on(Ftp::new(NetPolicy::Any, injected).readiness()), Err(Fault::NotConfigured));
        let before = log.lock().unwrap().len();
        assert_eq!(block_on(Ftp::new(NetPolicy::Offline, config).readiness()), Err(Fault::Blocked));
        assert_eq!(log.lock().unwrap().len(), before);
    }

    #[test]
    fn a_server_that_will_not_protect_the_data_is_unsafe() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let script = Script { tls: Some(FtpSecurity::Implicit), epsv: true, mlsd: true, refuse_prot: true };
        let port = serve(script, Arc::default(), log.clone());
        let config = FtpConfig { host: "localhost".into(), port, user: "me".into(), password: "pw".into(), security: FtpSecurity::Implicit, ..FtpConfig::default() };
        assert_eq!(block_on(Ftp::new(NetPolicy::Any, config).trusting(test_tls::client()).readiness()), Err(Fault::Unsafe));
        assert!(!log.lock().unwrap().iter().any(|line| line.starts_with("PASS")), "the password is not sent either");
    }

    #[test]
    fn a_certificate_no_authority_vouches_for_is_unsafe() {
        let port = serve(Script { tls: Some(FtpSecurity::Explicit), epsv: true, mlsd: true, refuse_prot: false }, Arc::default(), Arc::default());
        let config = FtpConfig { host: "localhost".into(), port, user: "me".into(), password: "pw".into(), ..FtpConfig::default() };
        assert_eq!(block_on(Ftp::new(NetPolicy::Any, config).readiness()), Err(Fault::Unsafe));
    }

    #[test]
    fn debug_shows_no_secret_or_address() {
        let config = FtpConfig { host: "ftp.example".into(), user: "me".into(), password: "pw".into(), path: "/p".into(), ..FtpConfig::default() };
        let printed = format!("{:?}", Ftp::new(NetPolicy::Any, config));
        assert!(!printed.contains("ftp.example") && !printed.contains("pw") && !printed.contains("\"me\""));
    }

    fn pem(label: &str, der: &[u8]) -> String {
        use base64::Engine;
        let text = base64::engine::general_purpose::STANDARD.encode(der);
        let lines: Vec<&str> = text.as_bytes().chunks(64).map(|c| std::str::from_utf8(c).unwrap()).collect();
        format!("-----BEGIN {label}-----\n{}\n-----END {label}-----\n", lines.join("\n"))
    }

    /// Against `rclone serve ftp`: a plain round trip, and its implicit TLS, which will not
    /// protect the data channel and so must be refused. Run with
    /// `WMPASS_LIVE_FTP=1 cargo test -p wmpass-sinks ftp::tests::live -- --ignored`.
    fn live(security: FtpSecurity) {
        if std::env::var("WMPASS_LIVE_FTP").is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("backups/wm")).unwrap();
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let address = format!("127.0.0.1:{port}");
        let mut args = vec!["serve".to_owned(), "ftp".into(), root.to_str().unwrap().into(), "--addr".into(), address.clone(), "--user".into(), "me".into(), "--pass".into(), "pw".into()];
        // rclone with a certificate speaks implicit TLS.
        if security == FtpSecurity::Implicit {
            let cert = dir.path().join("cert.pem");
            let key = dir.path().join("key.pem");
            let chain = pem("CERTIFICATE", include_bytes!("../testdata/leaf.der")) + &pem("CERTIFICATE", include_bytes!("../testdata/ca.der"));
            std::fs::write(&cert, chain).unwrap();
            std::fs::write(&key, pem("PRIVATE KEY", include_bytes!("../testdata/leaf-key.der"))).unwrap();
            args.extend(["--cert".into(), cert.to_str().unwrap().into(), "--key".into(), key.to_str().unwrap().into()]);
        }
        let mut child = std::process::Command::new("rclone").args(&args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        for _ in 0..50 {
            if TcpStream::connect(&address).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let config = FtpConfig { host: "localhost".into(), port, user: "me".into(), password: "pw".into(), path: "backups/wm".into(), security };
        let sink = Ftp::new(NetPolicy::Any, config).trusting(test_tls::client());
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if security == FtpSecurity::Implicit {
                // rclone's implicit FTPS answers PROT P with 550 and sends data in the clear.
                assert_eq!(block_on(sink.readiness()), Err(Fault::Unsafe));
                return;
            }
            block_on(sink.readiness()).unwrap();
            let big = vec![7u8; 3 * 1024 * 1024];
            let stored = block_on(sink.write("a file.bin", &big)).unwrap();
            block_on(sink.write("second.bin", b"hello")).unwrap();
            block_on(sink.write("second.bin", b"hello again")).unwrap();
            let listed = block_on(sink.list()).unwrap();
            assert!(listed.iter().any(|f| f.name == "second.bin" && f.size == 11), "{listed:?}");
            assert_eq!(block_on(sink.read(&stored)).unwrap(), big);
            block_on(sink.delete(&stored)).unwrap();
            block_on(sink.delete(&stored)).unwrap();
            assert!(block_on(sink.list()).unwrap().iter().all(|f| f.name != "a file.bin"));
        }));
        let _ = child.kill();
        let _ = child.wait();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    #[ignore = "needs rclone and WMPASS_LIVE_FTP"]
    fn live_plain_against_rclone() {
        live(FtpSecurity::None);
    }

    #[test]
    #[ignore = "needs rclone and WMPASS_LIVE_FTP"]
    fn live_implicit_tls_against_rclone_is_refused_for_clear_data() {
        live(FtpSecurity::Implicit);
    }
}
