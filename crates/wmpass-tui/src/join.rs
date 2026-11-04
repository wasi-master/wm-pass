//! Bringing a vault onto this computer from another device, or sending it to one, over the local
//! network.
//!
//! The network side runs on a thread of its own and reports what happens as [`Note`]s, so the
//! full-screen app can draw it while it waits, and `wmpass join` and `wmpass add-device` can
//! print the same steps as plain lines when there is no terminal to draw on.
//!
//! The terminal has no camera, so it shows the QR code when it is the one receiving, and finds
//! the new device on the network or takes its code's text when it is the one sending. Either
//! way the file that moves is the sealed vault: it opens only with its master password. The
//! other device keeps this one's key as a paired device.

use std::io::{BufRead, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use wmpass_lan::discovery;
use wmpass_lan::join::{self, Arrival};
use wmpass_lan::pair::{PairingQr, QR_TTL_MS};
use wmpass_lan::sync::BlobStore;
use wmpass_lan::{Identity, LanError};

use crate::engine::now_ms;
use crate::store;

/// How long a new device waits for the one with the vault: as long as its code lasts.
const WAIT: Duration = Duration::from_millis(QR_TTL_MS);

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
pub fn device_name() -> String {
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

/// A QR code's modules, row by row, `true` for dark. Low error correction keeps the code as
/// small as it can be: a screen is not a crumpled label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Qr {
    pub width: usize,
    dark: Vec<bool>,
}

impl Qr {
    pub fn new(text: &str) -> Option<Self> {
        let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::L).ok()?;
        let width = code.width();
        let dark = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
        Some(Self { width, dark })
    }

    /// A code the engine drew, as rows of `1` for dark and `0` for light.
    pub fn from_rows(width: usize, rows: &str) -> Option<Self> {
        let dark: Vec<bool> = rows.bytes().map(|b| b == b'1').collect();
        (width > 0 && dark.len() == width * width).then_some(Self { width, dark })
    }

    /// Whether the module at `x`, `y` is dark; anything outside the code is the light border.
    pub fn dark(&self, x: isize, y: isize) -> bool {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else { return false };
        x < self.width && y < self.width && self.dark[y * self.width + x]
    }
}

/// Light modules around a code. Two is what phone scanners need in practice, and it lets the
/// big code fit a terminal about 45 lines tall.
pub const QUIET: isize = 2;

/// A QR code as plain terminal lines, two rows of modules to a line. A cell whose halves match
/// is a space painted with its colour, so a filled area has no seam where the font's half block
/// stops short of the line spacing; only real edges use a half block. The colours are the
/// 256-colour palette's fixed black and white, which no terminal theme remaps.
pub fn qr_lines(text: &str) -> Option<Vec<String>> {
    let qr = Qr::new(text)?;
    let span = isize::try_from(qr.width).ok()? + QUIET;
    let mut lines = Vec::new();
    let mut y = -QUIET;
    while y < span {
        let mut line = String::new();
        for x in -QUIET..span {
            let (top, bottom) = (qr.dark(x, y), qr.dark(x, y + 1));
            let colour = |dark: bool| if dark { 16 } else { 231 };
            if top == bottom {
                line.push_str(&format!("\x1b[48;5;{}m ", colour(top)));
            } else {
                line.push_str(&format!("\x1b[38;5;{};48;5;{}m▀", colour(top), colour(bottom)));
            }
        }
        line.push_str("\x1b[0m");
        lines.push(line);
        y += 2;
    }
    Some(lines)
}

/// What the network side says.
#[derive(Debug)]
pub enum Note {
    /// The new device is waiting. `code` is the QR code's text, or `None` with no address to put
    /// in one, when only finding it on the network works.
    Waiting { code: Option<String>, address: Option<String> },
    /// Devices waiting to join, found on this network. Sent again as the search goes on.
    Found(Vec<SocketAddr>),
    /// Connected; vouching and sending under way.
    Busy(String),
    /// Six digits to compare. Answer with [`Say::Answer`].
    Digits { code: String, peer: String },
    /// The vault arrived and is written to `path`.
    Received { path: PathBuf, from: String },
    /// The vault reached the other device.
    Sent { to: String },
    /// It stopped. Nothing more comes.
    Failed(String),
}

/// What the person says back.
#[derive(Debug)]
pub enum Say {
    /// Whether the digits match.
    Answer(bool),
    /// Send to the device found at this address, comparing digits.
    Address(SocketAddr),
    /// Send to the device whose code's text this is.
    Code(String),
}

/// A transfer running on its own thread. Dropping it stops the thread.
#[derive(Debug)]
pub struct Link {
    notes: Receiver<Note>,
    say: Sender<Say>,
    stop: Arc<AtomicBool>,
}

impl Link {
    /// The next thing that happened, if anything did.
    pub fn poll(&self) -> Option<Note> {
        self.notes.try_recv().ok()
    }

    /// Waits for the next thing to happen, for up to `wait`.
    pub fn wait(&self, wait: Duration) -> Option<Note> {
        self.notes.recv_timeout(wait).ok()
    }

    pub fn say(&self, what: Say) {
        let _ = self.say.send(what);
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The thread's half.
struct Side {
    notes: Sender<Note>,
    say: Receiver<Say>,
    stop: Arc<AtomicBool>,
}

impl Side {
    fn note(&self, note: Note) {
        let _ = self.notes.send(note);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Waits for the person's answer to the digits, or for the app to give up.
    fn answer(&self) -> Result<bool, String> {
        loop {
            if self.stopped() {
                return Err("stopped".to_owned());
            }
            match self.say.recv_timeout(Duration::from_millis(200)) {
                Ok(Say::Answer(yes)) => return Ok(yes),
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("stopped".to_owned()),
            }
        }
    }
}

fn spawn(work: impl FnOnce(&Side) -> Result<Note, String> + Send + 'static) -> Link {
    let (notes_tx, notes) = mpsc::channel();
    let (say, say_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let side = Side { notes: notes_tx, say: say_rx, stop: Arc::clone(&stop) };
    std::thread::spawn(move || {
        let last = work(&side).unwrap_or_else(Note::Failed);
        if !side.stopped() {
            side.note(last);
        }
    });
    Link { notes, say, stop }
}

/// Waits for one connection, until the code runs out or the app gives up.
fn accept(listener: &TcpListener, side: &Side) -> Result<TcpStream, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    loop {
        if side.stopped() {
            return Err("stopped".to_owned());
        }
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).map_err(|e| e.to_string())?;
                wmpass_lan::prepare(&stream).map_err(lan)?;
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("Nothing came within two minutes, and the code has run out. Start again for a new one".to_owned());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn connect(address: SocketAddr) -> Result<TcpStream, String> {
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(10)).map_err(|_| "Could not reach that device".to_owned())?;
    wmpass_lan::prepare(&stream).map_err(lan)?;
    Ok(stream)
}

/// Starts waiting for the device with the vault, to write what it sends to `path`.
pub fn start_receive(path: PathBuf) -> Link {
    spawn(move |side| {
        if path.exists() {
            return Err(format!("{} is already there; choose a new file for the vault", path.display()));
        }
        let listener = TcpListener::bind(("0.0.0.0", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let _advert = discovery::advertise_joining(port).map_err(lan)?;
        let identity = Identity::generate().map_err(lan)?;
        let address = local_ip().map(|ip| SocketAddr::new(ip, port).to_string());
        let qr = match &address {
            Some(address) => Some(PairingQr::new(address, now_ms()).map_err(lan)?),
            None => None,
        };
        side.note(Note::Waiting { code: qr.as_ref().map(PairingQr::text), address });
        let stream = accept(&listener, side)?;
        side.note(Note::Busy("Connected. Checking it is your device…".to_owned()));
        let mut blobs = Folder(store::blob_dir(&path));
        let joined = match join::arrive(stream, &identity, &device_name(), qr.as_ref(), now_ms(), &mut blobs).map_err(lan)? {
            Arrival::Done(joined) => joined,
            Arrival::Code(pending) => {
                side.note(Note::Digits { code: pending.code().to_owned(), peer: pending.peer().name.clone() });
                let accepted = side.answer()?;
                if accepted {
                    side.note(Note::Busy("Receiving the vault…".to_owned()));
                }
                pending.finish(accepted, &mut blobs).map_err(|e| if accepted { lan(e) } else { "Stopped. Nothing was sent".to_owned() })?
            }
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        store::write_atomic(&path, &joined.file)?;
        Ok(Note::Received { path, from: joined.peer.name })
    })
}

/// Starts looking for a device waiting to join, to send it the vault in `path`. The search goes
/// on until the person picks a device or gives a code.
pub fn start_send(path: PathBuf) -> Link {
    spawn(move |side| {
        let file = store::read(&path)?;
        wmpass_core::format::Header::parse(&file).map_err(|_| format!("{} is not a vault file", path.display()))?;
        let folder = store::blob_dir(&path);
        let names: Vec<String> = std::fs::read_dir(&folder)
            .map(|dir| dir.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| !n.starts_with('.')).collect())
            .unwrap_or_default();
        let blobs = Folder(folder);
        let identity = Identity::generate().map_err(lan)?;
        let name = device_name();
        let target = loop {
            if side.stopped() {
                return Err("stopped".to_owned());
            }
            match side.say.try_recv() {
                Ok(Say::Address(address)) => break Ok(address),
                Ok(Say::Code(text)) => break Err(text),
                Ok(Say::Answer(_)) | Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return Err("stopped".to_owned()),
            }
            // A short window, so a pick or a code is taken up quickly.
            if let Ok(found) = discovery::browse_joining(Duration::from_millis(1500)) {
                side.note(Note::Found(found));
            }
        };
        let peer = match target {
            Err(text) => {
                let qr = PairingQr::parse(text.trim(), now_ms()).map_err(|e| match e {
                    LanError::Decode => "That is not a WM Pass code. Copy the text under the new device's QR code".to_owned(),
                    other => lan(other),
                })?;
                let address: SocketAddr = qr.address.parse().map_err(|_| "That code holds no address".to_owned())?;
                side.note(Note::Busy("Sending the vault…".to_owned()));
                join::send_by_qr(connect(address)?, &qr, &identity, &name, now_ms(), &file, &names, &blobs).map_err(lan)?
            }
            Ok(address) => {
                side.note(Note::Busy(format!("Connecting to {address}…")));
                let pending = join::begin_send_by_code(connect(address)?, &identity, &name).map_err(lan)?;
                side.note(Note::Digits { code: pending.code().to_owned(), peer: pending.peer().name.clone() });
                let accepted = side.answer()?;
                if accepted {
                    side.note(Note::Busy("Sending the vault…".to_owned()));
                }
                join::finish_send(pending, accepted, &file, &names, &blobs).map_err(|e| if accepted { lan(e) } else { "Stopped. Nothing was sent".to_owned() })?
            }
        };
        Ok(Note::Sent { to: peer.name })
    })
}

// ---- plain lines, for `wmpass join` and `wmpass add-device` with no terminal to draw on ----

fn ask_yes(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes")
}

/// Runs a transfer to its end, printing each step. With `pick`, the first device found gets the
/// vault, and a search that finds none for twenty seconds gives up.
fn print_through(link: &Link, pick: bool) -> Result<(), String> {
    let give_up = Instant::now() + Duration::from_secs(20);
    let mut searching = pick;
    if pick {
        println!("Looking for a device waiting to join…");
    }
    loop {
        if searching && Instant::now() > give_up {
            return Err("No new device is waiting on this network; on it, choose I already have a vault, then Get it from another device".to_owned());
        }
        let Some(note) = link.wait(Duration::from_millis(250)) else { continue };
        match note {
            Note::Waiting { code, .. } => {
                println!();
                println!("On the device that has your vault, open Settings, then Sync and accounts, then Add a new device.");
                match code.as_deref().and_then(|code| qr_lines(code).map(|lines| (code, lines))) {
                    Some((code, lines)) => {
                        println!("There, scan this code, or choose Find it on this network and compare six digits:");
                        println!();
                        for line in lines {
                            println!("  {line}");
                        }
                        println!();
                        println!("Or choose Paste its code and paste this:");
                        println!("{code}");
                    }
                    None => println!("There, choose Find it on this network, then compare six digits."),
                }
                println!();
                println!("Waiting for up to two minutes. Ctrl+C stops.");
            }
            Note::Found(found) => {
                if searching && let Some(first) = found.first() {
                    searching = false;
                    link.say(Say::Address(*first));
                }
            }
            Note::Busy(text) => println!("{text}"),
            Note::Digits { code, peer } => {
                println!();
                println!("    {}", spaced(&code));
                println!();
                link.say(Say::Answer(ask_yes(&format!("Does {peer} show the same six digits?"))));
            }
            Note::Received { path, from } => {
                println!();
                println!("Your vault came from {from} and is in {}.", path.display());
                println!("Open it with its master password: wmpass tui {}", path.display());
                return Ok(());
            }
            Note::Sent { to } => {
                println!("Sent to {to}. Unlock it there with the vault's master password.");
                return Ok(());
            }
            Note::Failed(problem) => return Err(problem),
        }
    }
}

/// Six digits as two groups of three, easier to read aloud.
pub fn spaced(code: &str) -> String {
    if code.len() == 6 && code.is_ascii() { format!("{} {}", &code[..3], &code[3..]) } else { code.to_owned() }
}

/// Waits for the device with the vault, and writes what it sends to `path`, in plain lines.
pub fn receive(path: &Path) -> Result<(), String> {
    print_through(&start_receive(path.to_owned()), false)
}

/// Sends the vault in `path` to a device waiting to join, in plain lines. `code` is its pasted
/// QR text; without one, the first device found on the network gets it after six digits match.
pub fn send(path: &Path, code: Option<&str>) -> Result<(), String> {
    let link = start_send(path.to_owned());
    if let Some(code) = code {
        link.say(Say::Code(code.to_owned()));
    }
    print_through(&link, code.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_has_a_light_border_and_seamless_fills() {
        let qr = Qr::new("wmpass-pair:1:abc:123:def").expect("qr");
        assert!(qr.dark(0, 0), "a finder pattern's corner is dark");
        assert!(!qr.dark(-1, 0) && !qr.dark(0, -1), "outside the code is light");
        let lines = qr_lines("wmpass-pair:1:abc:123:def").expect("lines");
        assert_eq!(lines.len(), (qr.width + 2 * QUIET as usize).div_ceil(2));
        // The finder's left edge spans whole lines with no half block in it.
        assert!(lines[2].contains("\x1b[48;5;16m "));
    }

    #[test]
    fn a_phone_can_read_the_code_drawn() {
        let text = "wmpass-pair:1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:1800000000000:MTkyLjE2OC4xLjQ6NTEyMzQ";
        let qr = Qr::new(text).expect("qr");
        // What the screen shows, three pixels a module, border included.
        let side = qr.width + 2 * QUIET as usize;
        let scale = 3;
        let px = side * scale;
        let mut image = rqrr::PreparedImage::prepare_from_greyscale(px, px, |x, y| {
            let (mx, my) = ((x / scale) as isize - QUIET, (y / scale) as isize - QUIET);
            if qr.dark(mx, my) { 0 } else { 255 }
        });
        let grids = image.detect_grids();
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].decode().expect("decode").1, text);
    }
}
