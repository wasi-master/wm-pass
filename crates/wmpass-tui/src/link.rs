//! The screen for moving a vault between devices: receiving it here from a device that has it,
//! or sending the open one to a new device. The network side is in [`crate::join`]; this keeps
//! what the screen shows.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Instant;

use crate::input::Input;
use crate::join::{self, Link, Note, Say};

/// Which way the vault goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// It comes here.
    Receive,
    /// It goes from here.
    Send,
}

/// Where things stand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Starting,
    /// Receiving: waiting, with the code to scan or paste, if there is an address for one.
    Waiting { code: Option<String>, address: Option<String> },
    /// Sending: looking for a device, or taking a code.
    Searching,
    Busy(String),
    Digits { code: String, peer: String },
    Received { path: PathBuf, from: String },
    Sent { to: String },
    Failed(String),
}

/// The screen's state.
#[derive(Debug)]
pub struct LinkUi {
    pub role: Role,
    pub path: PathBuf,
    link: Option<Link>,
    pub stage: Stage,
    /// Devices found waiting, newest search first.
    pub found: Vec<SocketAddr>,
    /// The device picked in that list.
    pub at: usize,
    /// A pasted code, when sending.
    pub code: Input,
    /// When the current attempt began, for the countdown and the spinner.
    pub started: Instant,
    /// The person's choice of QR size: `Some(true)` for big cells, `None` for what fits.
    pub big: Option<bool>,
}

impl LinkUi {
    fn new(role: Role, path: PathBuf) -> Self {
        let mut ui = Self {
            role,
            path,
            link: None,
            stage: Stage::Starting,
            found: Vec::new(),
            at: 0,
            code: Input::new(),
            started: Instant::now(),
            big: None,
        };
        ui.restart();
        ui
    }

    /// For tests: a screen at a given stage, with nothing on the network behind it.
    #[cfg(test)]
    pub fn at_stage(role: Role, stage: Stage) -> Self {
        Self {
            role,
            path: PathBuf::from("t.wmvault"),
            link: None,
            stage,
            found: Vec::new(),
            at: 0,
            code: Input::new(),
            started: Instant::now(),
            big: None,
        }
    }

    /// Waits here for the device that has the vault, to keep it in `path`.
    pub fn receive(path: PathBuf) -> Self {
        Self::new(Role::Receive, path)
    }

    /// Sends the vault in `path` to a new device.
    pub fn send(path: PathBuf) -> Self {
        Self::new(Role::Send, path)
    }

    /// Starts over, with a fresh code and a fresh search.
    pub fn restart(&mut self) {
        // The old thread stops when its link goes.
        self.link = None;
        self.found.clear();
        self.at = 0;
        self.started = Instant::now();
        self.stage = match self.role {
            Role::Receive => Stage::Starting,
            Role::Send => Stage::Searching,
        };
        self.link = Some(match self.role {
            Role::Receive => join::start_receive(self.path.clone()),
            Role::Send => join::start_send(self.path.clone()),
        });
    }

    /// Takes in what the network side said. Returns whether anything changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        while let Some(note) = self.link.as_ref().and_then(Link::poll) {
            changed = true;
            match note {
                Note::Waiting { code, address } => {
                    self.started = Instant::now();
                    self.stage = Stage::Waiting { code, address };
                }
                Note::Found(found) => {
                    let keep = self.found.get(self.at).copied();
                    self.found = found;
                    self.at = keep.and_then(|a| self.found.iter().position(|f| *f == a)).unwrap_or(0);
                }
                Note::Busy(text) => self.stage = Stage::Busy(text),
                Note::Digits { code, peer } => self.stage = Stage::Digits { code, peer },
                Note::Received { path, from } => {
                    self.link = None;
                    self.stage = Stage::Received { path, from };
                }
                Note::Sent { to } => {
                    self.link = None;
                    self.stage = Stage::Sent { to };
                }
                Note::Failed(problem) => {
                    self.link = None;
                    self.stage = Stage::Failed(problem);
                }
            }
        }
        changed
    }

    /// Answers the six digits.
    pub fn answer(&mut self, matches: bool) {
        if let Some(link) = &self.link {
            link.say(Say::Answer(matches));
            self.stage = Stage::Busy(if matches { "Going ahead…" } else { "Stopping…" }.to_owned());
        }
    }

    /// Sends to the device picked in the list.
    pub fn send_to_picked(&mut self) {
        if let (Some(link), Some(address)) = (&self.link, self.found.get(self.at)) {
            link.say(Say::Address(*address));
            self.stage = Stage::Busy(format!("Connecting to {address}…"));
        }
    }

    /// Sends to the device whose code was pasted.
    pub fn send_to_code(&mut self) {
        let text = self.code.text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        if let Some(link) = &self.link {
            link.say(Say::Code(text));
            self.code.clear();
            self.stage = Stage::Busy("Sending the vault…".to_owned());
        }
    }

    /// Seconds left on the code shown, while waiting.
    pub fn seconds_left(&self) -> u64 {
        (wmpass_lan::pair::QR_TTL_MS / 1000).saturating_sub(self.started.elapsed().as_secs())
    }

    /// A frame of a small spinner.
    pub fn spinner(&self) -> &'static str {
        const FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
        FRAMES[(self.started.elapsed().as_millis() / 250 % 4) as usize]
    }
}
