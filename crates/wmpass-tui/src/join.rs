//! Bringing a vault onto this computer from another device, or sending it to one, over the local
//! network. Both run as plain lines in the terminal rather than inside the full-screen app, so
//! `wmpass join` and `wmpass add-device` use them as they are, and the app steps out of full
//! screen while one runs.
//!
//! The terminal has no camera, so it shows the QR code when it is the one receiving and
//! compares six digits when it is the one sending. Either way the file that moves is the sealed
//! vault: it opens only with its master password. The other device keeps this one's key as a
//! paired device, though the terminal does not sync over the network by itself.

use std::io::{BufRead, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use wmpass_lan::discovery;
use wmpass_lan::join::{self, Arrival, Joined};
use wmpass_lan::pair::PairingQr;
use wmpass_lan::sync::BlobStore;
use wmpass_lan::{Identity, LanError};

use crate::engine::now_ms;
use crate::store;

/// How long a new device waits for the one with the vault.
const WAIT: Duration = Duration::from_secs(120);

/// Attachments beside a vault file, where the terminal app keeps them.
struct Folder(PathBuf);

impl BlobStore for Folder {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        if name.contains(['/', '\\']) {
            return None;
        }
        std::fs::read(self.0.join(name)).ok()
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), LanError> {
        if name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(LanError::Decode);
        }
        std::fs::create_dir_all(&self.0)?;
        std::fs::write(self.0.join(name), bytes).map_err(LanError::from)
    }
}

fn lan(error: LanError) -> String {
    error.to_string()
}

/// This computer's name, as the other device shows it.
fn device_name() -> String {
    let host = std::env::var("HOSTNAME").ok().or_else(|| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
    });
    let host = host.map(|h| h.trim().trim_end_matches(".local").to_owned()).filter(|h| !h.is_empty());
    format!("{} (terminal)", host.unwrap_or_else(|| "Computer".to_owned()))
}

/// The address other devices on this network reach this one at. Connecting a UDP socket sends
/// nothing; it only asks the system which interface it would use.
fn local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("192.0.2.1", 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// A QR code in half blocks, two rows of modules to a line, with a quiet border. Each cell
/// names its own black and white, so the code scans whatever colours the terminal uses.
pub fn qr_lines(text: &str) -> Option<Vec<String>> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let width = code.width();
    let dark = |x: isize, y: isize| -> bool {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else { return false };
        x < width && y < width && code[(x, y)] == qrcode::Color::Dark
    };
    let edge: isize = 2;
    let span = isize::try_from(width).ok()? + edge;
    let mut lines = Vec::new();
    let mut y = -edge;
    while y < span {
        let mut line = String::new();
        for x in -edge..span {
            // The upper half is the text colour, the lower half the background.
            let top = if dark(x, y) { "30" } else { "97" };
            let bottom = if dark(x, y + 1) { "40" } else { "107" };
            line.push_str(&format!("\x1b[{top};{bottom}m▀"));
        }
        line.push_str("\x1b[0m");
        lines.push(line);
        y += 2;
    }
    Some(lines)
}

fn ask_yes(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes")
}

/// Waits for one connection, for up to two minutes.
fn accept(listener: &TcpListener) -> Result<TcpStream, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).map_err(|e| e.to_string())?;
                wmpass_lan::prepare(&stream).map_err(lan)?;
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("nothing came within two minutes".to_owned());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn connect(address: SocketAddr) -> Result<TcpStream, String> {
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(10)).map_err(|_| "could not reach that device".to_owned())?;
    wmpass_lan::prepare(&stream).map_err(lan)?;
    Ok(stream)
}

/// Waits for the device with the vault, and writes what it sends to `path`.
pub fn receive(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} is already there; choose a new file for the vault", path.display()));
    }
    let listener = TcpListener::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let _advert = discovery::advertise_joining(port).map_err(lan)?;
    let identity = Identity::generate().map_err(lan)?;
    let name = device_name();
    let qr = match local_ip() {
        Some(ip) => Some(PairingQr::new(&SocketAddr::new(ip, port).to_string(), now_ms()).map_err(lan)?),
        None => None,
    };
    println!();
    println!("On the device that has your vault, open Settings, then Sync and accounts, then Add a new device.");
    if let Some(lines) = qr.as_ref().and_then(|qr| qr_lines(&qr.text())) {
        println!("A phone scans this code:");
        println!();
        for line in lines {
            println!("  {line}");
        }
        println!();
        println!("A computer can find this one on the network instead and show six digits, or take this code pasted in:");
        println!("{}", qr.as_ref().map(PairingQr::text).unwrap_or_default());
    } else {
        println!("The other device finds this computer on the network and shows six digits to compare.");
    }
    println!();
    println!("Waiting for up to two minutes. Ctrl+C stops.");
    let stream = accept(&listener)?;
    let mut blobs = Folder(store::blob_dir(path));
    let joined: Joined = match join::arrive(stream, &identity, &name, qr.as_ref(), now_ms(), &mut blobs).map_err(lan)? {
        Arrival::Done(joined) => joined,
        Arrival::Code(pending) => {
            println!();
            println!("    {}", pending.code());
            println!();
            let accepted = ask_yes(&format!("Does {} show the same six digits?", pending.peer().name));
            pending.finish(accepted, &mut blobs).map_err(|e| if accepted { lan(e) } else { "stopped; nothing was sent".to_owned() })?
        }
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    store::write_atomic(path, &joined.file)?;
    println!();
    println!("Your vault came from {} and is in {}.", joined.peer.name, path.display());
    println!("Open it with its master password: wmpass tui {}", path.display());
    Ok(())
}

/// Sends the vault in `path` to a device waiting to join. `code` is its pasted QR text; without
/// one, the device is found on the network and six digits are compared.
pub fn send(path: &Path, code: Option<&str>) -> Result<(), String> {
    let file = store::read(path)?;
    wmpass_core::format::Header::parse(&file).map_err(|_| format!("{} is not a vault file", path.display()))?;
    let folder = store::blob_dir(path);
    let names: Vec<String> = std::fs::read_dir(&folder)
        .map(|dir| dir.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| !n.starts_with('.')).collect())
        .unwrap_or_default();
    let blobs = Folder(folder);
    let identity = Identity::generate().map_err(lan)?;
    let name = device_name();
    let peer = if let Some(text) = code {
        let qr = PairingQr::parse(text.trim(), now_ms()).map_err(lan)?;
        let address: SocketAddr = qr.address.parse().map_err(|_| "that code holds no address".to_owned())?;
        join::send_by_qr(connect(address)?, &qr, &identity, &name, now_ms(), &file, &names, &blobs).map_err(lan)?
    } else {
        println!("Looking for a device waiting to join…");
        let found = discovery::browse_joining(Duration::from_secs(4)).map_err(lan)?;
        let address = *found.first().ok_or("no new device is waiting on this network; on it, choose I already have a vault, then Get it from another device")?;
        let pending = join::begin_send_by_code(connect(address)?, &identity, &name).map_err(lan)?;
        println!();
        println!("    {}", pending.code());
        println!();
        let accepted = ask_yes(&format!("Does {} show the same six digits?", pending.peer().name));
        join::finish_send(pending, accepted, &file, &names, &blobs).map_err(|e| if accepted { lan(e) } else { "stopped; nothing was sent".to_owned() })?
    };
    println!("Sent to {}. Unlock it there with the vault's master password.", peer.name);
    Ok(())
}
