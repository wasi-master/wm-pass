//! Bringing a vault onto this computer from a place its backups are kept: WebDAV, S3, SFTP or
//! a folder. The other apps write a copy there now and then; this lists the copies, newest
//! first, reads the one picked and puts its files in place, the way the desktop app keeps
//! them. A copy is ciphertext throughout, so the vault then opens with its master password,
//! as it always did.
//!
//! The network side is plain blocking calls, shared by `wmpass restore` and the full-screen
//! app, which runs them on a thread of its own (see [`RestoreUi`]).
//!
//! Drive, Dropbox, OneDrive and Box sign in through a browser and send it back to an address
//! on this computer; a terminal has neither, so they are not offered here.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Map, Value};
use wmpass_net::http::{NativeHttp, NetPolicy};
use wmpass_sinks::backups;
use wmpass_sinks::http::{Blocking, Client, block_on};
use wmpass_sinks::places::{self, Apps, Done, Op, Place};
use zeroize::Zeroizing;

pub use wmpass_sinks::Stored;

use crate::app_folder;
use crate::input::Input;

/// The kinds of place a terminal can reach without a browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    WebDav,
    S3,
    Sftp,
    Folder,
}

impl Kind {
    pub const ALL: [Self; 4] = [Self::WebDav, Self::S3, Self::Sftp, Self::Folder];

    pub fn label(self) -> &'static str {
        match self {
            Self::WebDav => "WebDAV",
            Self::S3 => "S3",
            Self::Sftp => "SFTP",
            Self::Folder => "Folder",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::WebDav => "webdav",
            Self::S3 => "s3",
            Self::Sftp => "sftp",
            Self::Folder => "folder",
        }
    }
}

/// Where the copies are, without the secrets.
#[derive(Clone, Debug, Default)]
pub struct Spot {
    pub kind: Kind,
    /// WebDAV: the folder's address.
    pub url: String,
    /// WebDAV and SFTP.
    pub user: String,
    /// S3: empty for AWS, whose address comes from the region.
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    /// S3: a key prefix, without a leading slash.
    pub prefix: String,
    pub access_key: String,
    /// S3: the bucket as a subdomain, as AWS prefers, rather than in the path.
    pub virtual_host: bool,
    /// SFTP.
    pub host: String,
    /// SFTP: 22 when 0.
    pub port: u16,
    /// SFTP: the folder on the server. Folder: the folder itself.
    pub path: String,
    /// SFTP: the server key to expect, as `<algorithm> <base64>`. Empty takes the first one
    /// seen, which [`fingerprint`] shows for checking.
    pub host_key: String,
}

/// What signs in to a spot.
#[derive(Default)]
pub struct Secrets {
    /// WebDAV or SFTP password; S3 secret access key.
    pub password: Zeroizing<String>,
    /// SFTP: a private key in OpenSSH or PEM form.
    pub private_key: Zeroizing<String>,
    /// SFTP: what unlocks the private key, when it is encrypted.
    pub key_passphrase: Zeroizing<String>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secrets")
    }
}

impl Spot {
    /// The place as the shared sink code reads it.
    pub fn place(&self, secrets: &Secrets) -> Place {
        let mut extra = Map::new();
        let mut put = |key: &str, value: Value| {
            extra.insert(key.to_owned(), value);
        };
        match self.kind {
            Kind::S3 => put("path_style", Value::Bool(!self.virtual_host)),
            Kind::Sftp => {
                put("host", self.host.trim().into());
                put("port", if self.port == 0 { 22 } else { self.port }.into());
                put("private_key", secrets.private_key.as_str().into());
                put("key_passphrase", secrets.key_passphrase.as_str().into());
                put("host_key", self.host_key.trim().into());
            }
            Kind::WebDav | Kind::Folder => {}
        }
        let s3 = self.kind == Kind::S3;
        Place {
            kind: self.kind.key().to_owned(),
            path: crate::store::expand(self.path.trim()).to_string_lossy().into_owned(),
            url: self.url.trim().to_owned(),
            user: self.user.clone(),
            password: if s3 { String::new() } else { secrets.password.to_string() },
            endpoint: self.endpoint.trim().to_owned(),
            region: if self.region.trim().is_empty() { "us-east-1".to_owned() } else { self.region.trim().to_owned() },
            bucket: self.bucket.trim().to_owned(),
            prefix: self.prefix.trim().trim_start_matches('/').to_owned(),
            access_key: self.access_key.trim().to_owned(),
            secret_key: if s3 { secrets.password.to_string() } else { String::new() },
            extra,
        }
    }
}

fn now_ms() -> u64 {
    crate::engine::now_ms()
}

/// One operation against a place, with what it learned (an SFTP server's first key) kept in
/// the place for the next one.
fn run(place: &mut Place, op: Op<'_>) -> Result<Done, String> {
    let http = NativeHttp::for_places();
    let reach = places::Reach { client: || Client::new(Blocking(&http), NetPolicy::Any), policy: NetPolicy::Any };
    let ran = block_on(places::run(&reach, &Apps::default(), place, op, now_ms()));
    for (key, value) in ran.update {
        place.extra.insert(key, value);
    }
    ran.done.map_err(|fault| {
        let mut text = fault.to_string();
        if let Some(first) = text.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        text
    })
}

/// The copies at a place, newest first.
pub fn copies(place: &mut Place) -> Result<Vec<Stored>, String> {
    match run(place, Op::List)? {
        Done::Files(files) => Ok(backups::copies(files)),
        _ => Ok(Vec::new()),
    }
}

/// Reads one copy.
pub fn fetch(place: &mut Place, copy: &Stored) -> Result<Vec<u8>, String> {
    match run(place, Op::Read(copy))? {
        Done::Bytes(bytes) => Ok(bytes),
        _ => Err("The place answered with no file".to_owned()),
    }
}

/// The SFTP server's key this connection was shown, for checking against the server's own.
pub fn fingerprint(place: &Place) -> Option<String> {
    let key = place.text("host_key");
    (!key.is_empty()).then(|| wmpass_sinks::sftp::fingerprint(key)).flatten()
}

/// When a copy was made, and its size, for a list.
pub fn describe(copy: &Stored) -> String {
    let when = backups::made_at(&copy.name).unwrap_or_else(|| copy.name.clone());
    format!("{when}  {}", size(copy.size))
}

fn size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    #[allow(clippy::cast_precision_loss)]
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 { format!("{kb:.0} KB") } else { format!("{:.1} MB", kb / 1024.0) }
}

/// Where a restored vault goes when nobody says: the WM Pass app's folder, so the app opens
/// it too once it is installed here.
pub fn default_target() -> Option<PathBuf> {
    app_folder::default_root()
}

/// Puts a copy's files in `root`, the way the desktop app keeps them, and says how many vault
/// files (vaults and decoys alike) it held. Refuses a folder that already has a vault.
pub fn install(root: &Path, bytes: &[u8]) -> Result<usize, String> {
    if app_folder::names(root).is_ok_and(|names| !names.is_empty()) {
        return Err(format!("There is already a vault in {}. Open it, or restore somewhere else", root.display()));
    }
    if root.is_file() {
        return Err(format!("{} is a file; restore into a folder", root.display()));
    }
    let files = if backups::is_vault_file(bytes) {
        vec![backups::BundleFile { name: format!("vaults/{}", app_folder::new_name()?), bytes: bytes.to_vec() }]
    } else {
        backups::unbundle(bytes).ok_or("That is not a WM Pass backup")?
    };
    // Attachments first and vault files last, so a folder with a vault in it is a whole one.
    let mut ordered: Vec<&backups::BundleFile> = files.iter().filter(|f| !f.name.starts_with("vaults/")).collect();
    ordered.extend(files.iter().filter(|f| f.name.starts_with("vaults/")));
    for file in &ordered {
        let mut path = root.to_owned();
        path.extend(file.name.split('/'));
        app_folder::write(&path, &file.bytes)?;
    }
    app_folder::cover_times(root)?;
    Ok(files.iter().filter(|f| f.name.starts_with("vaults/")).count())
}

// ---- The full-screen form ----

/// A line on the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Kind,
    Url,
    User,
    Password,
    Endpoint,
    Region,
    Bucket,
    Prefix,
    AccessKey,
    SecretKey,
    Host,
    Port,
    Path,
    KeyFile,
    Passphrase,
    Into,
}

impl Field {
    pub fn label(self, kind: Kind) -> &'static str {
        match self {
            Self::Kind => "Kind",
            Self::Url => "Address",
            Self::User => "User",
            Self::Password => "Password",
            Self::Endpoint => "Endpoint",
            Self::Region => "Region",
            Self::Bucket => "Bucket",
            Self::Prefix => "Prefix",
            Self::AccessKey => "Access key",
            Self::SecretKey => "Secret key",
            Self::Host => "Server",
            Self::Port => "Port",
            Self::Path if kind == Kind::Folder => "Folder",
            Self::Path => "Folder there",
            Self::KeyFile => "Private key",
            Self::Passphrase => "Passphrase",
            Self::Into => "Restore into",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Url => "https://dav.example.com/WM Pass",
            Self::User => "Empty if none",
            Self::Endpoint => "Empty for AWS",
            Self::Region => "us-east-1",
            Self::Prefix => "Empty if none",
            Self::Port => "22",
            Self::Path => "Where the copies are",
            Self::KeyFile => "Path to a key file, or empty",
            Self::Passphrase => "Empty if none",
            _ => "",
        }
    }

    pub fn secret(self) -> bool {
        matches!(self, Self::Password | Self::SecretKey | Self::Passphrase)
    }
}

/// Where the screen stands.
#[derive(Debug)]
pub enum Stage {
    Form,
    Busy(&'static str),
    Copies { list: Vec<Stored>, at: usize },
}

/// What came back from the thread.
enum Got {
    Copies(Box<Place>, Result<Vec<Stored>, String>),
    Restored(Box<Place>, Result<usize, String>),
}

/// What a key did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// The vault is in this folder now.
    Restored(PathBuf),
}

/// The restore screen's state.
pub struct RestoreUi {
    pub kind: Kind,
    pub focus: Field,
    url: Input,
    user: Input,
    password: Input,
    endpoint: Input,
    region: Input,
    bucket: Input,
    prefix: Input,
    access_key: Input,
    secret_key: Input,
    host: Input,
    port: Input,
    path: Input,
    key_file: Input,
    passphrase: Input,
    into: Input,
    pub stage: Stage,
    pub error: Option<String>,
    /// The SFTP server's key, once one connection has seen it.
    pub server_key: Option<String>,
    place: Option<Box<Place>>,
    /// The copies listed, kept while one is restored so a failure can go back to them.
    listed: Vec<Stored>,
    job: Option<Receiver<Got>>,
    started: Instant,
}

impl std::fmt::Debug for RestoreUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreUi").field("kind", &self.kind).field("stage", &self.stage).finish_non_exhaustive()
    }
}

impl RestoreUi {
    /// A blank form, restoring into `into` unless changed.
    pub fn new(into: Option<PathBuf>) -> Self {
        Self {
            kind: Kind::WebDav,
            focus: Field::Kind,
            url: Input::new(),
            user: Input::new(),
            password: Input::new(),
            endpoint: Input::new(),
            region: Input::with("us-east-1"),
            bucket: Input::new(),
            prefix: Input::new(),
            access_key: Input::new(),
            secret_key: Input::new(),
            host: Input::new(),
            port: Input::with("22"),
            path: Input::new(),
            key_file: Input::new(),
            passphrase: Input::new(),
            into: into.map(|p| Input::with(&p.to_string_lossy())).unwrap_or_default(),
            stage: Stage::Form,
            error: None,
            server_key: None,
            place: None,
            listed: Vec::new(),
            job: None,
            started: Instant::now(),
        }
    }

    /// The lines shown for the kind picked, in order.
    pub fn fields(&self) -> Vec<Field> {
        use Field::{AccessKey, Bucket, Endpoint, Host, Into, KeyFile, Passphrase, Password, Path, Port, Prefix, Region, SecretKey, Url, User};
        let mut out = vec![Field::Kind];
        out.extend(match self.kind {
            Kind::WebDav => vec![Url, User, Password],
            Kind::S3 => vec![Endpoint, Region, Bucket, Prefix, AccessKey, SecretKey],
            Kind::Sftp => vec![Host, Port, User, Path, Password, KeyFile, Passphrase],
            Kind::Folder => vec![Path],
        });
        out.push(Into);
        out
    }

    pub fn input(&self, field: Field) -> Option<&Input> {
        Some(match field {
            Field::Kind => return None,
            Field::Url => &self.url,
            Field::User => &self.user,
            Field::Password => &self.password,
            Field::Endpoint => &self.endpoint,
            Field::Region => &self.region,
            Field::Bucket => &self.bucket,
            Field::Prefix => &self.prefix,
            Field::AccessKey => &self.access_key,
            Field::SecretKey => &self.secret_key,
            Field::Host => &self.host,
            Field::Port => &self.port,
            Field::Path => &self.path,
            Field::KeyFile => &self.key_file,
            Field::Passphrase => &self.passphrase,
            Field::Into => &self.into,
        })
    }

    fn input_mut(&mut self, field: Field) -> Option<&mut Input> {
        Some(match field {
            Field::Kind => return None,
            Field::Url => &mut self.url,
            Field::User => &mut self.user,
            Field::Password => &mut self.password,
            Field::Endpoint => &mut self.endpoint,
            Field::Region => &mut self.region,
            Field::Bucket => &mut self.bucket,
            Field::Prefix => &mut self.prefix,
            Field::AccessKey => &mut self.access_key,
            Field::SecretKey => &mut self.secret_key,
            Field::Host => &mut self.host,
            Field::Port => &mut self.port,
            Field::Path => &mut self.path,
            Field::KeyFile => &mut self.key_file,
            Field::Passphrase => &mut self.passphrase,
            Field::Into => &mut self.into,
        })
    }

    fn step(&mut self, by: isize) {
        let fields = self.fields();
        let at = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        let len = fields.len().cast_signed();
        self.focus = fields[(at.cast_signed() + by).rem_euclid(len).cast_unsigned()];
    }

    fn turn(&mut self, by: isize) {
        let at = Kind::ALL.iter().position(|k| *k == self.kind).unwrap_or(0).cast_signed();
        self.kind = Kind::ALL[(at + by).rem_euclid(Kind::ALL.len().cast_signed()).cast_unsigned()];
        self.error = None;
    }

    /// A frame of a small spinner.
    pub fn spinner(&self) -> &'static str {
        const FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
        FRAMES[(self.started.elapsed().as_millis() / 250 % 4) as usize]
    }

    /// The form as a place, or what is missing.
    fn spot(&self) -> Result<(Spot, Secrets), String> {
        let text = |input: &Input| input.text().trim().to_owned();
        let need = |input: &Input, what: &str| if input.text().trim().is_empty() { Err(format!("Type the {what}")) } else { Ok(()) };
        match self.kind {
            Kind::WebDav => need(&self.url, "address")?,
            Kind::S3 => {
                need(&self.bucket, "bucket")?;
                need(&self.access_key, "access key")?;
                need(&self.secret_key, "secret key")?;
            }
            Kind::Sftp => {
                need(&self.host, "server")?;
                need(&self.user, "user")?;
                if self.password.is_empty() && self.key_file.text().trim().is_empty() {
                    return Err("Type a password, or the path to a private key".to_owned());
                }
            }
            Kind::Folder => need(&self.path, "folder")?,
        }
        need(&self.into, "folder to restore into")?;
        let port = if self.port.text().trim().is_empty() { 22 } else { self.port.text().trim().parse().map_err(|_| "The port is a number, such as 22".to_owned())? };
        let private_key = match self.key_file.text().trim() {
            "" => Zeroizing::new(String::new()),
            path => {
                let bytes = Zeroizing::new(crate::store::read(&crate::store::expand(path))?);
                Zeroizing::new(String::from_utf8(bytes.to_vec()).map_err(|_| "That key file is not text".to_owned())?)
            }
        };
        let spot = Spot {
            kind: self.kind,
            url: text(&self.url),
            user: self.user.text().to_owned(),
            endpoint: text(&self.endpoint),
            region: text(&self.region),
            bucket: text(&self.bucket),
            prefix: text(&self.prefix),
            access_key: text(&self.access_key),
            virtual_host: false,
            host: text(&self.host),
            port,
            path: text(&self.path),
            host_key: String::new(),
        };
        let password = if self.kind == Kind::S3 { &self.secret_key } else { &self.password };
        let secrets = Secrets {
            password: Zeroizing::new(password.text().to_owned()),
            private_key,
            key_passphrase: Zeroizing::new(self.passphrase.text().to_owned()),
        };
        Ok((spot, secrets))
    }

    /// Lists the copies, on a thread of its own.
    fn connect(&mut self) {
        let (spot, secrets) = match self.spot() {
            Ok(both) => both,
            Err(problem) => {
                self.error = Some(problem);
                return;
            }
        };
        let mut place = Box::new(spot.place(&secrets));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let listed = copies(&mut place);
            let _ = tx.send(Got::Copies(place, listed));
        });
        self.begin(rx, "Looking for copies…");
    }

    /// Reads the copy picked and puts it in place, on a thread of its own.
    fn restore(&mut self, list: Vec<Stored>, at: usize) {
        let Some(copy) = list.get(at).cloned() else { return };
        let Some(mut place) = self.place.take() else { return };
        self.listed = list;
        let root = crate::store::expand(self.into.text().trim());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let done = fetch(&mut place, &copy).and_then(|bytes| install(&root, &bytes));
            let _ = tx.send(Got::Restored(place, done));
        });
        self.begin(rx, "Restoring…");
    }

    fn begin(&mut self, job: Receiver<Got>, what: &'static str) {
        self.error = None;
        self.job = Some(job);
        self.started = Instant::now();
        self.stage = Stage::Busy(what);
    }

    /// Takes in what the thread said. Returns the folder once a vault is in it.
    pub fn tick(&mut self) -> Option<PathBuf> {
        let got = self.job.as_ref()?.try_recv().ok()?;
        self.job = None;
        match got {
            Got::Copies(place, listed) => {
                self.error = None;
                self.server_key = fingerprint(&place);
                self.place = Some(place);
                match listed {
                    Ok(list) if list.is_empty() => {
                        self.stage = Stage::Form;
                        self.error = Some("No WM Pass backups there. Check the address or folder".to_owned());
                    }
                    Ok(list) => self.stage = Stage::Copies { list, at: 0 },
                    Err(problem) => {
                        self.stage = Stage::Form;
                        self.error = Some(problem);
                    }
                }
                None
            }
            Got::Restored(place, done) => {
                self.place = Some(place);
                match done {
                    Ok(_) => {
                        self.wipe();
                        return Some(crate::store::expand(self.into.text().trim()));
                    }
                    Err(problem) => {
                        let list = std::mem::take(&mut self.listed);
                        self.stage = if list.is_empty() { Stage::Form } else { Stage::Copies { list, at: 0 } };
                        self.error = Some(problem);
                    }
                }
                None
            }
        }
    }

    /// Whether a text box has the keyboard, to take pasted text.
    pub fn typing(&self) -> bool {
        matches!(self.stage, Stage::Form) && self.focus != Field::Kind
    }

    /// Whether the thread is still at work.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Clears every secret on the form.
    pub fn wipe(&mut self) {
        self.password.clear();
        self.secret_key.clear();
        self.passphrase.clear();
        self.place = None;
    }

    /// Applies a key.
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        if let Stage::Copies { list, at } = &mut self.stage {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => *at = at.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => *at = (*at + 1).min(list.len().saturating_sub(1)),
                KeyCode::Enter => {
                    let (list, at) = (std::mem::take(list), *at);
                    self.restore(list, at);
                }
                KeyCode::Esc => {
                    self.error = None;
                    self.stage = Stage::Form;
                }
                _ => {}
            }
            return Outcome::Stay;
        }
        if let Stage::Busy(_) = self.stage {
            if key.code == KeyCode::Esc {
                // The thread finishes on its own; what it says is no longer listened to.
                self.job = None;
                self.stage = Stage::Form;
            }
            return Outcome::Stay;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.wipe();
                return Outcome::Close;
            }
            KeyCode::Left if self.focus == Field::Kind => self.turn(-1),
            KeyCode::Right | KeyCode::Char(' ') if self.focus == Field::Kind => self.turn(1),
            KeyCode::Tab | KeyCode::Down => self.step(1),
            KeyCode::BackTab | KeyCode::Up => self.step(-1),
            KeyCode::Char('s') if ctrl => self.connect(),
            KeyCode::Enter => {
                if self.fields().last() == Some(&self.focus) {
                    self.connect();
                } else {
                    self.step(1);
                }
            }
            _ => {
                let focus = self.focus;
                if let Some(input) = self.input_mut(focus)
                    && input.key(key) == crate::input::Edit::Changed
                {
                    self.error = None;
                }
            }
        }
        Outcome::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wmpass_sinks::backups::{BundleFile, bundle};

    fn copy(files: &[(&str, &[u8])]) -> Vec<u8> {
        bundle(&files.iter().map(|(name, bytes)| BundleFile { name: (*name).to_owned(), bytes: bytes.to_vec() }).collect::<Vec<_>>())
    }

    #[test]
    fn a_copy_lands_as_the_app_keeps_it() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("app");
        let bytes = copy(&[("vaults/aa", b"WMPV vault"), ("vaults/bb", b"WMPV decoy"), ("blobs/aa/f1", b"sealed")]);
        assert_eq!(install(&root, &bytes), Ok(2));
        assert!(app_folder::is_app_folder(&root));
        assert_eq!(app_folder::names(&root).expect("names"), ["aa", "bb"]);
        assert_eq!(std::fs::read(root.join("blobs/aa/f1")).expect("blob"), b"sealed");
        let again = install(&root, &bytes).expect_err("a second restore over the first");
        assert!(again.contains("already a vault"), "{again}");
    }

    #[test]
    fn a_lone_vault_file_or_something_else() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(install(&dir.path().join("one"), b"WMPV a lone vault"), Ok(1));
        assert_eq!(install(&dir.path().join("two"), b"PK not really"), Err("That is not a WM Pass backup".to_owned()));
        assert!(!dir.path().join("two/vaults").exists());
    }

    #[test]
    fn a_folder_place_lists_only_copies_newest_first_and_reads_one() {
        let dir = tempfile::tempdir().expect("dir");
        for name in ["wmpass-backup-20260101-000000.wmbackup", "wmpass-backup-20261104-093000.wmbackup", "phone.wmsync"] {
            std::fs::write(dir.path().join(name), name).expect("write");
        }
        let spot = Spot { kind: Kind::Folder, path: dir.path().to_string_lossy().into_owned(), ..Spot::default() };
        let mut place = spot.place(&Secrets::default());
        let found = copies(&mut place).expect("list");
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["wmpass-backup-20261104-093000.wmbackup", "wmpass-backup-20260101-000000.wmbackup"]);
        assert_eq!(fetch(&mut place, &found[0]).expect("read"), found[0].name.as_bytes());
        assert!(describe(&found[0]).starts_with("2026-11-04 09:30 UTC"));
    }

    #[test]
    fn each_kind_is_read_the_way_the_sinks_expect() {
        let secrets = Secrets { password: Zeroizing::new("pw".into()), ..Secrets::default() };
        let s3 = Spot { kind: Kind::S3, bucket: " b ".into(), prefix: "/p".into(), access_key: "AK".into(), ..Spot::default() }.place(&secrets);
        assert_eq!((s3.kind.as_str(), s3.bucket.as_str(), s3.prefix.as_str(), s3.region.as_str()), ("s3", "b", "p", "us-east-1"));
        assert_eq!((s3.secret_key.as_str(), s3.password.as_str()), ("pw", ""));
        assert!(s3.flag("path_style", false));
        let sftp = Spot { kind: Kind::Sftp, host: "nas".into(), user: "me".into(), ..Spot::default() }.place(&secrets);
        assert_eq!((sftp.text("host"), sftp.number("port"), sftp.password.as_str()), ("nas", Some(22), "pw"));
    }

    #[test]
    fn the_form_asks_for_what_each_kind_needs() {
        let mut ui = RestoreUi::new(Some(PathBuf::from("/tmp/x")));
        assert_eq!(ui.fields(), [Field::Kind, Field::Url, Field::User, Field::Password, Field::Into]);
        ui.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(ui.kind, Kind::S3);
        ui.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(ui.error.as_deref(), Some("Type the bucket"));
        assert!(matches!(ui.stage, Stage::Form));
        ui.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        ui.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(ui.kind, Kind::Folder);
        assert_eq!(ui.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)), Outcome::Close);
    }
}
