//! A folder in a mail account, over IMAP.
//!
//! Each file is one message in [`ImapConfig::mailbox`], whose body is the file in base64
//! and whose `X-WMPass-Backup` header carries its name. Listing reads that header and
//! nothing else; reading fetches one body. Deleting flags a message and expunges it.
//!
//! An append is atomic: a message is in the folder whole or not at all, so there is no
//! temporary name. Writing a name again adds a second message, then removes the older ones
//! of that name; listing shows only the newest of each name.
//!
//! TLS certificates are checked against the public roots and the host name.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use rustls::ClientConfig;
use wmpass_core::Secret;
use wmpass_net::NetPolicy;

use crate::ftp::{Conn, Lines, connect, handshake, io_fault, tls_config};
use crate::{Fault, Result, Sink, Stored};

/// The header a file's message is found by. Some servers answer it in lower case.
pub const HEADER: &str = "X-WMPass-Backup";

const READ_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LITERAL: usize = 64 * 1024 * 1024;
const MAX_LINE: usize = 1024 * 1024;
const LINE: usize = 76;

/// How an IMAP connection is protected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImapSecurity {
    /// TLS from the first byte, port 993.
    #[default]
    Tls,
    /// Plain first, upgraded by `STARTTLS` before the login. Port 143.
    StartTls,
    /// Nothing. The password crosses as text.
    None,
}

impl ImapSecurity {
    /// The usual port.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Tls => 993,
            Self::StartTls | Self::None => 143,
        }
    }
}

/// A mailbox on an IMAP server.
#[derive(Clone, PartialEq, Eq)]
pub struct ImapConfig {
    /// Host name or address.
    pub host: String,
    /// Port.
    pub port: u16,
    /// TLS, and which kind.
    pub security: ImapSecurity,
    /// Account name.
    pub user: String,
    /// Most services want an app password here, not the account password.
    pub password: Secret,
    /// Made on first use. Its own folder, because deleting expunges it.
    pub mailbox: String,
}

/// The mailbox used when none is named.
pub const DEFAULT_MAILBOX: &str = "WM Pass backups";

impl Default for ImapConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: ImapSecurity::Tls.default_port(),
            security: ImapSecurity::Tls,
            user: String::new(),
            password: Secret::default(),
            mailbox: DEFAULT_MAILBOX.to_owned(),
        }
    }
}

impl ImapConfig {
    /// Whether there is enough to sign in.
    pub fn configured(&self) -> bool {
        !self.host.trim().is_empty() && !self.user.is_empty() && !self.password.is_empty()
    }
}

impl std::fmt::Debug for ImapConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapConfig").field("security", &self.security).finish_non_exhaustive()
    }
}

// ---- The client ----

/// A tagged `NO` or `BAD`, an untagged `BYE`, a refused login, or the connection failing.
/// The server's words are kept to read response codes from, and never printed.
pub(crate) enum ImapError {
    Refused { kind: String, text: String },
    Auth,
    Io(Fault),
}

impl std::fmt::Debug for ImapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { kind, .. } => write!(f, "Refused({kind})"),
            Self::Auth => f.write_str("Auth"),
            Self::Io(fault) => write!(f, "Io({fault:?})"),
        }
    }
}

impl From<Fault> for ImapError {
    fn from(fault: Fault) -> Self {
        Self::Io(fault)
    }
}

/// What a failure means.
pub(crate) fn fault_of(error: &ImapError) -> Fault {
    match error {
        ImapError::Io(fault) => *fault,
        ImapError::Auth => Fault::PermissionLost,
        ImapError::Refused { text, .. } => {
            let upper = text.to_ascii_uppercase();
            if upper.contains("[AUTHENTICATIONFAILED]") || upper.contains("[AUTHORIZATIONFAILED]") {
                Fault::PermissionLost
            } else if upper.contains("[OVERQUOTA]") || upper.contains("QUOTA") {
                Fault::OutOfSpace
            } else if upper.contains("[NONEXISTENT]") {
                Fault::TargetMissing
            } else {
                Fault::Io
            }
        }
    }
}

/// One server line, with the bytes of any literals it carried, in order. The text keeps
/// each `{n}` marker.
pub(crate) struct Reply {
    pub(crate) text: String,
    pub(crate) literals: Vec<Vec<u8>>,
}

/// A command argument: sent as is, a string quoted or sent as a literal, or bytes.
pub(crate) enum Arg<'a> {
    Raw(&'a str),
    Str(&'a str),
    Bytes(&'a [u8]),
}

/// A quoted string, or None when it needs a literal: a line break, a NUL, or anything past
/// ASCII.
pub(crate) fn quote(text: &str) -> Option<String> {
    if text.chars().any(|c| c == '\r' || c == '\n' || c == '\0' || u32::from(c) > 0x7E) {
        return None;
    }
    Some(format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"")))
}

/// Modified UTF-7 (RFC 3501 section 5.1.3), what IMAP folder names are written in. Plain
/// ASCII stays itself and `&` becomes `&-`; anything else goes into `&...-` as base64 of
/// UTF-16 with `,` for `/`.
pub fn modified_utf7(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";
    let printable = |c: char| (0x20..=0x7E).contains(&u32::from(c));
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if c == '&' {
            out.push_str("&-");
            at += 1;
        } else if printable(c) {
            out.push(c);
            at += 1;
        } else {
            let mut end = at;
            while end < chars.len() && !printable(chars[end]) {
                end += 1;
            }
            let mut units = [0u16; 2];
            let bytes: Vec<u8> = chars[at..end]
                .iter()
                .flat_map(|c| c.encode_utf16(&mut units).iter().flat_map(|unit| unit.to_be_bytes()).collect::<Vec<_>>())
                .collect();
            out.push('&');
            let (mut buffer, mut bits) = (0u32, 0u32);
            for byte in bytes {
                buffer = (buffer << 8) | u32::from(byte);
                bits += 8;
                while bits >= 6 {
                    bits -= 6;
                    out.push(char::from(ALPHABET[((buffer >> bits) & 0x3F) as usize]));
                }
            }
            if bits > 0 {
                out.push(char::from(ALPHABET[((buffer << (6 - bits)) & 0x3F) as usize]));
            }
            out.push('-');
            at = end;
        }
    }
    out
}

fn literal_at_end(line: &str) -> Option<usize> {
    let inner = line.strip_suffix('}')?;
    let inner = inner.strip_suffix('+').unwrap_or(inner);
    let open = inner.rfind('{')?;
    let digits = &inner[open + 1..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Whether a refused LOGIN says the connection must be encrypted first, rather than that the
/// password is wrong.
fn wants_encryption(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    if upper.contains("[AUTHENTICATIONFAILED]") || upper.contains("[AUTHORIZATIONFAILED]") {
        return false;
    }
    ["[PRIVACYREQUIRED]", "PLAINTEXT", "SSL/TLS", "STARTTLS", "ENCRYPT"].iter().any(|word| upper.contains(word))
}

fn capabilities_in(line: &str) -> Option<BTreeSet<String>> {
    let upper = line.to_ascii_uppercase();
    let start = upper.find("CAPABILITY ")? + "CAPABILITY ".len();
    let rest = &upper[start..];
    let end = rest.find(']').unwrap_or(rest.len());
    Some(rest[..end].split_whitespace().map(str::to_owned).collect())
}

fn is_fetch(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("* ") else { return false };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && rest[digits..].to_ascii_uppercase().starts_with(" FETCH")
}

/// The client side of IMAP4rev1 (RFC 3501) a backup needs: log in, open or make a folder,
/// append a message, fetch, flag and expunge. Over whatever stream it is given, so the
/// tests drive it with a scripted server.
///
/// The only hard part of IMAP is the literal: a `{n}` at the end of a line means n raw
/// bytes follow, which is how the server sends anything with a line break in it and how a
/// client sends anything it cannot quote.
pub(crate) struct ImapClient<S> {
    lines: Lines<S>,
    tag: u32,
    pub(crate) capabilities: BTreeSet<String>,
    /// The tagged answer to the last command, for response codes such as APPENDUID.
    last_tagged: String,
}

impl<S: Read + Write> ImapClient<S> {
    pub(crate) fn new(stream: S) -> Self {
        Self { lines: Lines::new(stream), tag: 0, capabilities: BTreeSet::new(), last_tagged: String::new() }
    }

    /// The stream back, for STARTTLS. None when the server sent more than it should have.
    pub(crate) fn into_inner(self) -> Option<S> {
        self.lines.into_inner()
    }

    fn io<T>(result: std::io::Result<T>) -> std::result::Result<T, ImapError> {
        result.map_err(|error| ImapError::Io(io_fault(&error)))
    }

    /// One logical line, with each `{n}` literal's bytes read and kept aside.
    pub(crate) fn read_reply(&mut self) -> std::result::Result<Reply, ImapError> {
        let mut text = String::new();
        let mut literals = Vec::new();
        loop {
            let line = Self::io(self.lines.read_line(MAX_LINE))?.ok_or(ImapError::Io(Fault::Io))?;
            let line = String::from_utf8_lossy(&line).into_owned();
            text.push_str(&line);
            let Some(count) = literal_at_end(&line) else { break };
            if count > MAX_LITERAL {
                return Err(ImapError::Io(Fault::Io));
            }
            literals.push(Self::io(self.lines.read_exact(count))?);
        }
        Ok(Reply { text, literals })
    }

    fn note_capabilities(&mut self, line: &str) {
        if let Some(found) = capabilities_in(line) {
            self.capabilities = found;
        }
    }

    /// Reads the greeting. PREAUTH is fine; BYE is a refusal.
    pub(crate) fn greeting(&mut self) -> std::result::Result<(), ImapError> {
        let line = self.read_reply()?.text;
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("* BYE") {
            return Err(ImapError::Refused { kind: "BYE".into(), text: line });
        }
        if !upper.starts_with("* OK") && !upper.starts_with("* PREAUTH") {
            return Err(ImapError::Io(Fault::Io));
        }
        self.note_capabilities(&line);
        Ok(())
    }

    pub(crate) fn capability(&mut self) -> std::result::Result<(), ImapError> {
        for reply in self.run(&[Arg::Raw("CAPABILITY")])? {
            self.note_capabilities(&reply.text);
        }
        Ok(())
    }

    pub(crate) fn login(&mut self, user: &str, password: &Secret) -> std::result::Result<(), ImapError> {
        // RFC 3501: a server that advertises LOGINDISABLED refuses LOGIN on this connection,
        // which happens only on a plain one. The password is not sent to find that out.
        if self.capabilities.contains("LOGINDISABLED") {
            return Err(ImapError::Io(Fault::NeedsEncryption));
        }
        let replies = match self.run(&[Arg::Raw("LOGIN"), Arg::Str(user), Arg::Str(password.expose())]) {
            Ok(replies) => replies,
            // A refused LOGIN is the credentials, unless the server says it wants encryption
            // first (RFC 5530's PRIVACYREQUIRED, or Dovecot's and others' plain words).
            Err(ImapError::Refused { text, .. }) if wants_encryption(&text) => return Err(ImapError::Io(Fault::NeedsEncryption)),
            Err(ImapError::Refused { .. }) => return Err(ImapError::Auth),
            Err(other) => return Err(other),
        };
        let tagged = self.last_tagged.clone();
        for reply in replies {
            self.note_capabilities(&reply.text);
        }
        self.note_capabilities(&tagged);
        // Servers often change their capabilities after login, and only some say so.
        self.capability()
    }

    /// Opens a mailbox, making it first when it does not exist. Returns the message count.
    pub(crate) fn select_or_create(&mut self, mailbox: &str) -> std::result::Result<u64, ImapError> {
        let name = modified_utf7(mailbox);
        let replies = match self.run(&[Arg::Raw("SELECT"), Arg::Str(&name)]) {
            Ok(replies) => replies,
            Err(ImapError::Refused { kind, .. }) if kind == "NO" => {
                self.run(&[Arg::Raw("CREATE"), Arg::Str(&name)])?;
                self.run(&[Arg::Raw("SELECT"), Arg::Str(&name)])?
            }
            Err(other) => return Err(other),
        };
        Ok(replies
            .iter()
            .find_map(|reply| {
                let rest = reply.text.strip_prefix("* ")?;
                let (count, word) = rest.split_once(' ')?;
                word.to_ascii_uppercase().starts_with("EXISTS").then(|| count.parse().ok()).flatten()
            })
            .unwrap_or(0))
    }

    /// Appends a message and returns its UID when the server says (UIDPLUS).
    pub(crate) fn append(&mut self, mailbox: &str, message: &[u8]) -> std::result::Result<Option<u64>, ImapError> {
        let name = modified_utf7(mailbox);
        self.run(&[Arg::Raw("APPEND"), Arg::Str(&name), Arg::Raw("(\\Seen)"), Arg::Bytes(message)])?;
        let upper = self.last_tagged.to_ascii_uppercase();
        Ok(upper.find("[APPENDUID ").and_then(|start| {
            let mut words = upper[start + "[APPENDUID ".len()..].split([' ', ']']);
            words.next()?;
            words.next()?.parse().ok()
        }))
    }

    /// `UID FETCH <set> <items>`, returning the FETCH lines.
    pub(crate) fn uid_fetch(&mut self, set: &str, items: &str) -> std::result::Result<Vec<Reply>, ImapError> {
        let command = format!("UID FETCH {set} {items}");
        Ok(self.run(&[Arg::Raw(&command)])?.into_iter().filter(|reply| is_fetch(&reply.text)).collect())
    }

    /// The UIDs `UID SEARCH` finds.
    pub(crate) fn uid_search(&mut self, criteria: &[Arg<'_>]) -> std::result::Result<Vec<u64>, ImapError> {
        let mut args = vec![Arg::Raw("UID SEARCH")];
        args.extend(criteria.iter().map(|arg| match arg {
            Arg::Raw(text) => Arg::Raw(text),
            Arg::Str(text) => Arg::Str(text),
            Arg::Bytes(bytes) => Arg::Bytes(bytes),
        }));
        let replies = self.run(&args)?;
        Ok(replies
            .iter()
            .filter_map(|reply| reply.text.strip_prefix("* SEARCH").or_else(|| reply.text.strip_prefix("* search")))
            .flat_map(|rest| rest.split_whitespace().filter_map(|n| n.parse().ok()).collect::<Vec<_>>())
            .collect())
    }

    pub(crate) fn delete_uid(&mut self, uid: u64) -> std::result::Result<(), ImapError> {
        self.run(&[Arg::Raw(&format!("UID STORE {uid} +FLAGS.SILENT (\\Deleted)"))])?;
        if self.capabilities.contains("UIDPLUS") {
            self.run(&[Arg::Raw(&format!("UID EXPUNGE {uid}"))])?;
        } else {
            // The folder is this app's own, so expunging everything flagged in it only
            // ever removes what was chosen for deleting.
            self.run(&[Arg::Raw("EXPUNGE")])?;
        }
        Ok(())
    }

    pub(crate) fn logout(&mut self) {
        let _ = self.run(&[Arg::Raw("LOGOUT")]);
    }

    /// Sends one command and reads to its tagged answer. Every literal waits for the
    /// server's `+` first, unless the server takes `LITERAL+`.
    pub(crate) fn run(&mut self, args: &[Arg<'_>]) -> std::result::Result<Vec<Reply>, ImapError> {
        self.tag += 1;
        let me = format!("w{}", self.tag);
        let non_sync = self.capabilities.contains("LITERAL+");
        let mut pending = format!("{me} ").into_bytes();
        for (index, arg) in args.iter().enumerate() {
            if index > 0 {
                pending.push(b' ');
            }
            match arg {
                Arg::Raw(text) => {
                    if text.contains(['\r', '\n']) {
                        return Err(ImapError::Io(Fault::NotConfigured));
                    }
                    pending.extend_from_slice(text.as_bytes());
                }
                Arg::Str(text) => match quote(text) {
                    Some(quoted) => pending.extend_from_slice(quoted.as_bytes()),
                    None => self.literal(&mut pending, text.as_bytes(), non_sync)?,
                },
                Arg::Bytes(bytes) => self.literal(&mut pending, bytes, non_sync)?,
            }
        }
        pending.extend_from_slice(b"\r\n");
        Self::io(self.lines.write_all(&pending))?;
        Self::io(self.lines.flush())?;

        let logging_out = matches!(args.first(), Some(Arg::Raw("LOGOUT")));
        let mut untagged = Vec::new();
        loop {
            let reply = self.read_reply()?;
            if let Some(rest) = reply.text.strip_prefix(&format!("{me} ")) {
                let kind = rest.split(' ').next().unwrap_or_default().to_ascii_uppercase();
                if kind != "OK" {
                    return Err(ImapError::Refused { kind, text: rest.to_owned() });
                }
                self.last_tagged = rest.to_owned();
                return Ok(untagged);
            }
            if !logging_out && reply.text.to_ascii_uppercase().starts_with("* BYE") {
                return Err(ImapError::Refused { kind: "BYE".into(), text: reply.text });
            }
            untagged.push(reply);
        }
    }

    fn literal(&mut self, pending: &mut Vec<u8>, bytes: &[u8], non_sync: bool) -> std::result::Result<(), ImapError> {
        // The line break after the marker is there either way; LITERAL+ only skips
        // waiting for the server's go-ahead.
        pending.extend_from_slice(format!("{{{}{}}}\r\n", bytes.len(), if non_sync { "+" } else { "" }).as_bytes());
        if !non_sync {
            Self::io(self.lines.write_all(pending))?;
            Self::io(self.lines.flush())?;
            pending.clear();
            loop {
                let go = self.read_reply()?.text;
                if go.starts_with('+') {
                    break;
                }
                // A server may send untagged news first, such as `* 3 EXISTS` when another
                // device appended meanwhile; only a tagged answer or a BYE ends the wait.
                if !go.starts_with("* ") || go.to_ascii_uppercase().starts_with("* BYE") {
                    return Err(ImapError::Refused { kind: "NO".into(), text: go });
                }
            }
        }
        pending.extend_from_slice(bytes);
        Ok(())
    }
}

// ---- Messages ----

fn rfc2822_date(now_ms: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let stamp = crate::s3::amz_date(now_ms);
    let part = |range: std::ops::Range<usize>| stamp.get(range).unwrap_or("00");
    let month = part(4..6).parse::<usize>().unwrap_or(1).clamp(1, 12);
    let weekday = DAYS[usize::try_from(now_ms / 86_400_000 % 7).unwrap_or(0)];
    format!("{weekday}, {} {} {} {}:{}:{} +0000", part(6..8), MONTHS[month - 1], part(0..4), part(9..11), part(11..13), part(13..15))
}

/// The whole message for one file. The subject is the file name so the folder reads
/// sensibly in a mail app, and the body is an attachment of that name, so a person can
/// also save it from there by hand. `message_id` must be unique; it finds the message
/// again on servers without UIDPLUS.
pub fn compose_message(name: &str, bytes: &[u8], user: &str, now_ms: u64, message_id: &str) -> Vec<u8> {
    let from = if user.contains('@') { user.trim() } else { "wmpass@localhost" };
    let safe: String = name.chars().filter(|&c| (' '..='~').contains(&c) && c != '"').collect();
    let date = rfc2822_date(now_ms);
    let head = format!(
        "From: WM Pass <{from}>\r\nTo: <{from}>\r\nSubject: {safe}\r\nDate: {date}\r\nMessage-ID: <{message_id}@wmpass>\r\n{HEADER}: {safe}\r\nMIME-Version: 1.0\r\nContent-Type: application/octet-stream; name=\"{safe}\"\r\nContent-Disposition: attachment; filename=\"{safe}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n"
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut out = Vec::with_capacity(head.len() + encoded.len() + encoded.len() / LINE * 2 + 2);
    out.extend_from_slice(head.as_bytes());
    for line in encoded.as_bytes().chunks(LINE) {
        out.extend_from_slice(line);
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// One file from a FETCH reply, or None when the message is not one of ours.
pub(crate) fn entry_of(reply: &Reply) -> Option<Stored> {
    let upper = reply.text.to_ascii_uppercase();
    let at = upper.find("UID ")?;
    if at > 0 && upper.as_bytes()[at - 1].is_ascii_alphanumeric() {
        return None;
    }
    let uid: String = upper[at + 4..].chars().take_while(char::is_ascii_digit).collect();
    if uid.is_empty() {
        return None;
    }
    let header = String::from_utf8_lossy(reply.literals.first()?).into_owned();
    let prefix = format!("{}:", HEADER.to_ascii_lowercase());
    let name = header
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
        .map(|line| line[prefix.len()..].trim().to_owned())
        .filter(|name| !name.is_empty())?;
    Some(Stored { id: uid, name, size: 0 })
}

/// The body a `UID FETCH (BODY.PEEK[TEXT])` brought: the FETCH that carries a literal.
/// Others may be the server's news about other messages, such as a flag another device
/// changed. No FETCH at all means the message is gone.
fn body_of(replies: Vec<Reply>) -> std::result::Result<Vec<u8>, ImapError> {
    if replies.is_empty() {
        return Err(ImapError::Io(Fault::TargetMissing));
    }
    replies.into_iter().find_map(|reply| reply.literals.into_iter().next()).ok_or(ImapError::Io(Fault::Io))
}

/// One file per name, the message with the highest UID, which is the one appended last.
/// Older copies of a name are what a write left behind before it could remove them.
fn newest_of_each(files: impl Iterator<Item = Stored>) -> Vec<Stored> {
    let mut newest: Vec<(u64, Stored)> = Vec::new();
    for file in files {
        let uid: u64 = file.id.parse().unwrap_or(0);
        match newest.iter_mut().find(|(_, kept)| kept.name == file.name) {
            Some(slot) if slot.0 < uid => *slot = (uid, file),
            Some(_) => {}
            None => newest.push((uid, file)),
        }
    }
    newest.into_iter().map(|(_, file)| file).collect()
}

// ---- The sink ----

/// A mailbox used as a folder.
pub struct Imap {
    policy: NetPolicy,
    config: ImapConfig,
    tls: Option<Arc<ClientConfig>>,
}

impl std::fmt::Debug for Imap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Imap").field("config", &self.config).finish_non_exhaustive()
    }
}

impl Imap {
    /// The mailbox `config` names.
    pub fn new(policy: NetPolicy, config: ImapConfig) -> Self {
        Self { policy, config, tls: None }
    }

    #[cfg(test)]
    fn trusting(mut self, config: Arc<ClientConfig>) -> Self {
        self.tls = Some(config);
        self
    }

    fn mailbox(&self) -> &str {
        let name = self.config.mailbox.trim();
        if name.is_empty() { DEFAULT_MAILBOX } else { name }
    }

    /// Connected, encrypted as configured, and signed in.
    fn open(&self, tls: &Arc<ClientConfig>, host: &str) -> std::result::Result<ImapClient<Conn>, ImapError> {
        let socket = connect(host, self.config.port, READ_TIMEOUT)?;
        let conn = match self.config.security {
            ImapSecurity::Tls => Conn::Tls(Box::new(handshake(tls, host, socket)?)),
            ImapSecurity::StartTls | ImapSecurity::None => Conn::Plain(socket),
        };
        let mut client = ImapClient::new(conn);
        client.greeting()?;
        if self.config.security == ImapSecurity::StartTls {
            client.run(&[Arg::Raw("STARTTLS")])?;
            let Some(Conn::Plain(socket)) = client.into_inner() else {
                return Err(ImapError::Io(Fault::Io));
            };
            client = ImapClient::new(Conn::Tls(Box::new(handshake(tls, host, socket)?)));
            client.capability()?;
        }
        client.login(&self.config.user, &self.config.password)?;
        Ok(client)
    }

    fn session<T>(&self, work: impl FnOnce(&mut ImapClient<Conn>) -> std::result::Result<T, ImapError>) -> Result<T> {
        if !self.config.configured() {
            return Err(Fault::NotConfigured);
        }
        wmpass_net::http::check_socket(self.policy).map_err(|_| Fault::Blocked)?;
        let host = self.config.host.trim();
        let tls = match &self.tls {
            Some(config) => config.clone(),
            None => tls_config()?,
        };
        let mut client = self.open(&tls, host).map_err(|error| fault_of(&error))?;
        let result = work(&mut client).map_err(|error| fault_of(&error));
        client.logout();
        result
    }
}

fn random_id() -> Result<String> {
    let mut random = [0u8; 12];
    getrandom::fill(&mut random).map_err(|_| Fault::Io)?;
    Ok(random.iter().map(|b| format!("{b:02x}")).collect())
}

impl Sink for Imap {
    async fn readiness(&self) -> Result<()> {
        self.session(|client| client.select_or_create(self.mailbox()).map(|_| ()))
    }

    async fn write(&self, name: &str, bytes: &[u8]) -> Result<Stored> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0));
        let id = random_id()?;
        let message = compose_message(name, bytes, &self.config.user, now, &id);
        let uid = self.session(|client| {
            let before = client.select_or_create(self.mailbox())?;
            let uid = match client.append(self.mailbox(), &message)? {
                Some(uid) => uid,
                None => {
                    // Without UIDPLUS the server does not say where the message went; its
                    // Message-ID, made here and unique, finds it.
                    client.run(&[Arg::Raw("NOOP")])?;
                    let found = client.uid_search(&[Arg::Raw("HEADER Message-ID"), Arg::Str(&format!("<{id}@wmpass>"))])?;
                    found.into_iter().max().ok_or(ImapError::Io(Fault::NotVerified))?
                }
            };
            // A write replaces, as at every other place: older messages of this name go once
            // the new one is in. Only older ones, so a newer copy another device appended
            // meanwhile stays. Best effort: a leftover is hidden by `list` and goes next time.
            if before > 0
                && let Ok(replies) = client.uid_fetch("1:*", &format!("(UID BODY.PEEK[HEADER.FIELDS ({HEADER})])"))
            {
                let older: Vec<u64> = replies
                    .iter()
                    .filter_map(entry_of)
                    .filter(|file| file.name == name)
                    .filter_map(|file| file.id.parse().ok())
                    .filter(|old| *old < uid)
                    .collect();
                for old in older {
                    let _ = client.delete_uid(old);
                }
            }
            Ok(uid)
        })?;
        Ok(Stored { id: uid.to_string(), name: name.to_owned(), size: bytes.len() as u64 })
    }

    async fn list(&self) -> Result<Vec<Stored>> {
        self.session(|client| {
            if client.select_or_create(self.mailbox())? == 0 {
                return Ok(Vec::new());
            }
            let replies = client.uid_fetch("1:*", &format!("(UID BODY.PEEK[HEADER.FIELDS ({HEADER})])"))?;
            Ok(newest_of_each(replies.iter().filter_map(entry_of)))
        })
    }

    async fn read(&self, file: &Stored) -> Result<Vec<u8>> {
        let uid: u64 = file.id.parse().map_err(|_| Fault::TargetMissing)?;
        let text = self.session(|client| {
            client.select_or_create(self.mailbox())?;
            body_of(client.uid_fetch(&uid.to_string(), "(BODY.PEEK[TEXT])")?)
        })?;
        let compact: Vec<u8> = text.into_iter().filter(|b| !b.is_ascii_whitespace()).collect();
        base64::engine::general_purpose::STANDARD.decode(compact).map_err(|_| Fault::Io)
    }

    async fn delete(&self, file: &Stored) -> Result<()> {
        // UID STORE on a UID that is gone succeeds with no effect, which is the
        // idempotence deleting needs.
        let Ok(uid) = file.id.parse::<u64>() else { return Ok(()) };
        self.session(|client| {
            client.select_or_create(self.mailbox())?;
            client.delete_uid(uid)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Cursor};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Mutex;

    use super::*;
    use crate::ftp::test_tls;
    use crate::http::block_on;

    /// Replies fixed in advance, and what the client sent kept for checking. IMAP has no
    /// out-of-order replies, so a script is a faithful server for one session.
    struct Scripted {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for Scripted {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn script(lines: &[&str]) -> ImapClient<Scripted> {
        ImapClient::new(Scripted { input: Cursor::new(lines.concat().into_bytes()), output: Vec::new() })
    }

    fn sent(client: ImapClient<Scripted>) -> String {
        String::from_utf8(client.lines.into_inner().unwrap().output).unwrap()
    }

    #[test]
    fn a_literal_in_a_reply_is_read_whole_and_kept_aside() {
        let header = "X-WMPass-Backup: wmpass-backup-0001-00000001.wmvault\r\n\r\n";
        let mut client = script(&[
            "* OK [CAPABILITY IMAP4rev1 UIDPLUS] ready\r\n",
            &format!("* 1 FETCH (UID 42 INTERNALDATE \"23-Sep-2026 10:15:00 +0000\" BODY[HEADER.FIELDS (X-WMPASS-BACKUP)] {{{}}}\r\n", header.len()),
            header,
            ")\r\n",
            "w1 OK done\r\n",
        ]);
        client.greeting().unwrap();
        assert!(client.capabilities.contains("UIDPLUS"));
        let replies = client.uid_fetch("1:*", "(UID)").unwrap();
        assert_eq!(replies.len(), 1);
        let entry = entry_of(&replies[0]).unwrap();
        assert_eq!(entry, Stored { id: "42".into(), name: "wmpass-backup-0001-00000001.wmvault".into(), size: 0 });
        assert_eq!(sent(client), "w1 UID FETCH 1:* (UID)\r\n");
    }

    #[test]
    fn append_waits_for_the_go_ahead_and_returns_the_appenduid() {
        let mut client = script(&["* OK hi\r\n", "+ go ahead\r\n", "w1 OK [APPENDUID 7 99] appended\r\n"]);
        client.greeting().unwrap();
        assert_eq!(client.append("Backups", b"hello").unwrap(), Some(99));
        assert_eq!(sent(client), "w1 APPEND \"Backups\" (\\Seen) {5}\r\nhello\r\n");
    }

    #[test]
    fn a_flag_change_reported_alongside_is_not_taken_for_the_body() {
        let mut client = script(&["* OK hi\r\n", "* 3 FETCH (FLAGS (\\Seen))\r\n", "* 1 FETCH (UID 7 BODY[TEXT] {4}\r\n", "aGk=", ")\r\n", "w1 OK\r\n"]);
        client.greeting().unwrap();
        assert_eq!(body_of(client.uid_fetch("7", "(BODY.PEEK[TEXT])").unwrap()).unwrap(), b"aGk=");
        assert!(matches!(body_of(Vec::new()), Err(ImapError::Io(Fault::TargetMissing))));
    }

    #[test]
    fn untagged_news_before_the_go_ahead_is_waited_past() {
        let mut client = script(&["* OK hi\r\n", "* 4 EXISTS\r\n", "+ go ahead\r\n", "w1 OK appended\r\n"]);
        client.greeting().unwrap();
        assert_eq!(client.append("B", b"hi").unwrap(), None);
        let mut refused = script(&["* OK hi\r\n", "w1 NO [OVERQUOTA] too big\r\n"]);
        refused.greeting().unwrap();
        assert_eq!(fault_of(&refused.append("B", b"hi").unwrap_err()), Fault::OutOfSpace);
    }

    #[test]
    fn literal_plus_still_sends_the_line_break_and_does_not_wait() {
        let mut client = script(&["* OK [CAPABILITY IMAP4rev1 LITERAL+] hi\r\n", "w1 OK appended\r\n"]);
        client.greeting().unwrap();
        assert_eq!(client.append("B", b"hi").unwrap(), None);
        assert_eq!(sent(client), "w1 APPEND \"B\" (\\Seen) {2+}\r\nhi\r\n");
    }

    #[test]
    fn a_password_with_a_quote_is_escaped_and_one_past_ascii_goes_as_a_literal() {
        assert_eq!(quote("a\"b\\c").as_deref(), Some("\"a\\\"b\\\\c\""));
        assert_eq!(quote("pässword"), None);
        let mut client = script(&["* OK hi\r\n", "+ ok\r\n", "w1 OK in\r\n", "w2 OK caps\r\n"]);
        client.greeting().unwrap();
        client.login("me", &"pässword".into()).unwrap();
        assert!(sent(client).starts_with("w1 LOGIN \"me\" {9}\r\npässword\r\n"));
    }

    #[test]
    fn a_refused_login_is_a_credentials_problem_whatever_the_words() {
        let mut client = script(&["* OK hi\r\n", "w1 NO nope\r\n"]);
        client.greeting().unwrap();
        let failure = client.login("me", &"bad".into()).unwrap_err();
        assert_eq!(fault_of(&failure), Fault::PermissionLost);
        assert!(!format!("{failure:?}").contains("nope"));
        let quota = ImapError::Refused { kind: "NO".into(), text: "[OVERQUOTA] full".into() };
        assert_eq!(fault_of(&quota), Fault::OutOfSpace);
    }

    #[test]
    fn a_server_that_wants_encryption_says_so_rather_than_wrong_password() {
        // LOGINDISABLED: the password never goes out on the plain connection.
        let mut disabled = script(&["* OK [CAPABILITY IMAP4rev1 STARTTLS LOGINDISABLED] hi\r\n"]);
        disabled.greeting().unwrap();
        let failure = disabled.login("me", &"secret".into()).unwrap_err();
        assert_eq!(fault_of(&failure), Fault::NeedsEncryption);
        assert_eq!(sent(disabled), "", "nothing sent, least of all the password");
        // Refused with RFC 5530's code, or with Dovecot's words.
        for refusal in [
            "w1 NO [PRIVACYREQUIRED] Plaintext authentication disallowed on non-secure (SSL/TLS) connections.\r\n",
            "w1 BAD Plaintext authentication not allowed without SSL/TLS\r\n",
        ] {
            let mut client = script(&["* OK hi\r\n", refusal]);
            client.greeting().unwrap();
            assert_eq!(fault_of(&client.login("me", &"pw".into()).unwrap_err()), Fault::NeedsEncryption, "{refusal}");
        }
        // A wrong password stays a wrong password, whatever else the words mention.
        let mut wrong = script(&["* OK hi\r\n", "w1 NO [AUTHENTICATIONFAILED] Invalid credentials (TLS session)\r\n"]);
        wrong.greeting().unwrap();
        assert_eq!(fault_of(&wrong.login("me", &"pw".into()).unwrap_err()), Fault::PermissionLost);
    }

    #[test]
    fn folder_names_are_modified_utf7() {
        assert_eq!(modified_utf7("WM Pass backups"), "WM Pass backups");
        assert_eq!(modified_utf7("Entwürfe"), "Entw&APw-rfe");
        assert_eq!(modified_utf7("A&B"), "A&-B");
        assert_eq!(modified_utf7("日本語"), "&ZeVnLIqe-");
        assert_eq!(modified_utf7("😀"), "&2D3eAA-");
    }

    #[test]
    fn the_composed_message_carries_the_name_and_the_body_decodes_back() {
        let body: Vec<u8> = (0..5000u32).map(|i| (i * 31) as u8).collect();
        let message = String::from_utf8(compose_message("x.wmvault", &body, "me@example.com", 0, "abc")).unwrap();
        assert!(message.contains("\r\nX-WMPass-Backup: x.wmvault\r\n"));
        assert!(message.contains("\r\nDate: Thu, 01 Jan 1970 00:00:00 +0000\r\n"));
        assert!(message.contains("\r\nMessage-ID: <abc@wmpass>\r\n"));
        assert!(message.lines().all(|line| line.len() <= 998));
        let text: String = message.split("\r\n\r\n").nth(1).unwrap().split_whitespace().collect();
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(text).unwrap(), body);
        assert!(rfc2822_date(1_754_575_353_000).starts_with("Thu, 07 Aug 2025 14:02:33"));
    }

    // ---- A small IMAP server on this device ----

    #[derive(Default)]
    struct Mailbox {
        exists: bool,
        next_uid: u64,
        messages: Vec<(u64, Vec<u8>, bool)>,
    }

    #[derive(Clone, Copy)]
    struct Behaviour {
        security: ImapSecurity,
        uidplus: bool,
    }

    enum Stream {
        Plain(TcpStream),
        Tls(Box<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>),
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self {
                Self::Plain(s) => s.read(buf),
                Self::Tls(s) => s.read(buf),
            }
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            match self {
                Self::Plain(s) => s.write(buf),
                Self::Tls(s) => s.write(buf),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            match self {
                Self::Plain(s) => s.flush(),
                Self::Tls(s) => s.flush(),
            }
        }
    }

    fn accept_tls(socket: TcpStream) -> Stream {
        let mut stream = rustls::StreamOwned::new(rustls::ServerConnection::new(test_tls::server()).unwrap(), socket);
        while stream.conn.is_handshaking() {
            stream.conn.complete_io(&mut stream.sock).unwrap();
        }
        Stream::Tls(Box::new(stream))
    }

    fn serve(behaviour: Behaviour, mailbox: Arc<Mutex<Mailbox>>, log: Arc<Mutex<Vec<String>>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for socket in listener.incoming().flatten() {
                let (mailbox, log) = (mailbox.clone(), log.clone());
                std::thread::spawn(move || imap_session(behaviour, socket, &mailbox, &log));
            }
        });
        port
    }

    fn header_of(message: &[u8]) -> String {
        let text = String::from_utf8_lossy(message);
        let line = text.lines().find(|l| l.starts_with("X-WMPass-Backup:")).unwrap_or_default();
        format!("{line}\r\n\r\n")
    }

    fn imap_session(behaviour: Behaviour, socket: TcpStream, mailbox: &Mutex<Mailbox>, log: &Mutex<Vec<String>>) {
        let mut stream = match behaviour.security {
            ImapSecurity::Tls => accept_tls(socket),
            _ => Stream::Plain(socket),
        };
        let caps = if behaviour.uidplus { "IMAP4rev1 UIDPLUS STARTTLS" } else { "IMAP4rev1 STARTTLS" };
        stream.write_all(format!("* OK [CAPABILITY {caps}] ready\r\n").as_bytes()).unwrap();
        loop {
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end().to_owned();
            // A synchronising literal: say go, then read it.
            let mut literal = Vec::new();
            if let Some(count) = literal_at_end(&line) {
                drop(reader);
                if !line.ends_with("+}") {
                    stream.write_all(b"+ go\r\n").unwrap();
                }
                let mut reader = BufReader::new(&mut stream);
                literal.resize(count, 0);
                reader.read_exact(&mut literal).unwrap();
                let mut rest = String::new();
                reader.read_line(&mut rest).unwrap();
            } else {
                drop(reader);
            }
            log.lock().unwrap().push(line.clone());
            let (tag, command) = line.split_once(' ').unwrap();
            let upper = command.to_ascii_uppercase();
            let mut out = String::new();
            let mut binary: Vec<u8> = Vec::new();
            let mut m = mailbox.lock().unwrap();
            if upper == "STARTTLS" {
                stream.write_all(format!("{tag} OK begin\r\n").as_bytes()).unwrap();
                drop(m);
                let Stream::Plain(socket) = stream else { return };
                stream = accept_tls(socket);
                continue;
            } else if upper == "CAPABILITY" {
                out = format!("* CAPABILITY {caps}\r\n{tag} OK\r\n");
            } else if upper.starts_with("LOGIN") {
                out = if command.ends_with("\"pw\"") { format!("{tag} OK in\r\n") } else { format!("{tag} NO [AUTHENTICATIONFAILED] no\r\n") };
            } else if upper.starts_with("SELECT") {
                out = if m.exists { format!("* {} EXISTS\r\n{tag} OK [READ-WRITE]\r\n", m.messages.len()) } else { format!("{tag} NO [NONEXISTENT] none\r\n") };
            } else if upper.starts_with("CREATE") {
                m.exists = true;
                out = format!("{tag} OK made\r\n");
            } else if upper.starts_with("APPEND") {
                m.next_uid += 1;
                let uid = m.next_uid;
                m.messages.push((uid, literal, false));
                out = if behaviour.uidplus { format!("{tag} OK [APPENDUID 1 {uid}] done\r\n") } else { format!("{tag} OK done\r\n") };
            } else if upper.starts_with("UID SEARCH HEADER MESSAGE-ID") {
                let wanted = command.rsplit('"').nth(1).unwrap_or_default().to_owned();
                let found: Vec<String> = m.messages.iter().filter(|(_, b, _)| String::from_utf8_lossy(b).contains(&format!("Message-ID: {wanted}"))).map(|(u, _, _)| u.to_string()).collect();
                out = format!("* SEARCH {}\r\n{tag} OK\r\n", found.join(" "));
            } else if upper.starts_with("UID FETCH 1:*") {
                for (index, (uid, message, _)) in m.messages.iter().enumerate() {
                    let header = header_of(message);
                    binary.extend_from_slice(format!("* {} FETCH (UID {uid} BODY[HEADER.FIELDS (X-WMPASS-BACKUP)] {{{}}}\r\n{header})\r\n", index + 1, header.len()).as_bytes());
                }
                binary.extend_from_slice(format!("{tag} OK\r\n").as_bytes());
            } else if upper.starts_with("UID FETCH") {
                let wanted: u64 = command.split(' ').nth(2).unwrap().parse().unwrap();
                if let Some((index, (uid, message, _))) = m.messages.iter().enumerate().find(|(_, (u, _, _))| *u == wanted) {
                    let text = &message[message.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..];
                    binary.extend_from_slice(format!("* {} FETCH (UID {uid} BODY[TEXT] {{{}}}\r\n", index + 1, text.len()).as_bytes());
                    binary.extend_from_slice(text);
                    binary.extend_from_slice(b")\r\n");
                }
                binary.extend_from_slice(format!("{tag} OK\r\n").as_bytes());
            } else if upper.starts_with("UID STORE") {
                let wanted: u64 = command.split(' ').nth(2).unwrap().parse().unwrap();
                m.messages.iter_mut().filter(|(u, _, _)| *u == wanted).for_each(|entry| entry.2 = true);
                out = format!("{tag} OK\r\n");
            } else if upper.starts_with("UID EXPUNGE") || upper == "EXPUNGE" {
                m.messages.retain(|(_, _, deleted)| !deleted);
                out = format!("{tag} OK\r\n");
            } else if upper == "NOOP" {
                out = format!("{tag} OK\r\n");
            } else if upper == "LOGOUT" {
                stream.write_all(format!("* BYE\r\n{tag} OK\r\n").as_bytes()).unwrap();
                return;
            } else {
                out = format!("{tag} BAD what\r\n");
            }
            drop(m);
            stream.write_all(out.as_bytes()).unwrap();
            stream.write_all(&binary).unwrap();
            stream.flush().unwrap();
        }
    }

    fn round_trip(behaviour: Behaviour) -> Vec<String> {
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let log = Arc::new(Mutex::new(Vec::new()));
        let port = serve(behaviour, mailbox.clone(), log.clone());
        let config = ImapConfig { host: "localhost".into(), port, security: behaviour.security, user: "me@example.com".into(), password: "pw".into(), mailbox: "Entwürfe".into() };
        let sink = Imap::new(NetPolicy::Any, config).trusting(test_tls::client());
        block_on(sink.readiness()).unwrap();
        assert!(block_on(sink.list()).unwrap().is_empty());
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
        let stored = block_on(sink.write("a.wmvault", &big)).unwrap();
        let second = block_on(sink.write("b.wmvault", b"hello")).unwrap();
        assert_eq!(stored.id, "1");
        assert_eq!(second.id, "2");
        let listed = block_on(sink.list()).unwrap();
        assert_eq!(listed.iter().map(|f| (f.id.as_str(), f.name.as_str())).collect::<Vec<_>>(), vec![("1", "a.wmvault"), ("2", "b.wmvault")]);
        assert_eq!(block_on(sink.read(&stored)).unwrap(), big);
        block_on(sink.delete(&second)).unwrap();
        block_on(sink.delete(&second)).unwrap();
        assert_eq!(block_on(sink.read(&second)), Err(Fault::TargetMissing));
        assert_eq!(mailbox.lock().unwrap().messages.len(), 1);
        log.lock().unwrap().clone()
    }

    #[test]
    fn over_tls_with_uidplus() {
        let log = round_trip(Behaviour { security: ImapSecurity::Tls, uidplus: true });
        assert!(log[0].starts_with("w1 LOGIN \"me@example.com\" \"pw\""));
        assert!(log.iter().any(|l| l.ends_with("SELECT \"Entw&APw-rfe\"")));
        assert!(log.iter().any(|l| l.contains("UID EXPUNGE 2")));
    }

    #[test]
    fn starttls_upgrades_before_the_password_and_finds_uids_without_uidplus() {
        let log = round_trip(Behaviour { security: ImapSecurity::StartTls, uidplus: false });
        assert_eq!(log[0], "w1 STARTTLS");
        assert_eq!(log[1], "w1 CAPABILITY");
        assert!(log[2].starts_with("w2 LOGIN"));
        assert!(log.iter().any(|l| l.contains("UID SEARCH HEADER Message-ID")));
        assert!(log.iter().any(|l| l.ends_with(" EXPUNGE")));
    }

    #[test]
    fn writing_a_name_again_replaces_it_so_readers_see_the_newest() {
        for uidplus in [true, false] {
            let mailbox = Arc::new(Mutex::new(Mailbox::default()));
            let port = serve(Behaviour { security: ImapSecurity::None, uidplus }, mailbox.clone(), Arc::default());
            let config = ImapConfig { host: "127.0.0.1".into(), port, security: ImapSecurity::None, user: "me".into(), password: "pw".into(), ..ImapConfig::default() };
            let sink = Imap::new(NetPolicy::Any, config);
            block_on(sink.write("other.wmsync", b"theirs")).unwrap();
            block_on(sink.write("mine.wmsync", b"first")).unwrap();
            let newest = block_on(sink.write("mine.wmsync", b"second")).unwrap();
            let listed = block_on(sink.list()).unwrap();
            let mine: Vec<&Stored> = listed.iter().filter(|f| f.name == "mine.wmsync").collect();
            assert_eq!(mine.len(), 1, "one file per name");
            assert_eq!(mine[0].id, newest.id);
            assert_eq!(block_on(sink.read(mine[0])).unwrap(), b"second");
            assert_eq!(mailbox.lock().unwrap().messages.len(), 2, "the older copy is gone, the other name stays");
        }
        // Copies left behind before this, or by a write that could not clean up, stay hidden.
        let fetched = |uid: u64, name: &str| Stored { id: uid.to_string(), name: name.into(), size: 0 };
        let kept = newest_of_each([fetched(3, "a"), fetched(9, "a"), fetched(4, "b"), fetched(5, "a")].into_iter());
        assert_eq!(kept, vec![fetched(9, "a"), fetched(4, "b")]);
    }

    #[test]
    fn plain_imap_works_and_a_wrong_password_is_named() {
        round_trip(Behaviour { security: ImapSecurity::None, uidplus: true });
        let port = serve(Behaviour { security: ImapSecurity::None, uidplus: true }, Arc::default(), Arc::default());
        let config = ImapConfig { host: "127.0.0.1".into(), port, security: ImapSecurity::None, user: "me".into(), password: "bad".into(), ..ImapConfig::default() };
        assert_eq!(block_on(Imap::new(NetPolicy::Any, config.clone()).readiness()), Err(Fault::PermissionLost));
        assert_eq!(block_on(Imap::new(NetPolicy::Offline, config).readiness()), Err(Fault::Blocked));
    }

    #[test]
    fn a_certificate_no_authority_vouches_for_is_unsafe() {
        let port = serve(Behaviour { security: ImapSecurity::Tls, uidplus: true }, Arc::default(), Arc::default());
        let config = ImapConfig { host: "localhost".into(), port, user: "me".into(), password: "pw".into(), ..ImapConfig::default() };
        assert_eq!(block_on(Imap::new(NetPolicy::Any, config).readiness()), Err(Fault::Unsafe));
    }

    #[test]
    fn debug_shows_no_secret_or_address() {
        let config = ImapConfig { host: "imap.example".into(), user: "me@example.com".into(), password: "pw".into(), ..ImapConfig::default() };
        let printed = format!("{:?}", Imap::new(NetPolicy::Any, config));
        assert!(!printed.contains("imap.example") && !printed.contains("me@example.com") && !printed.contains("pw"));
    }
}
