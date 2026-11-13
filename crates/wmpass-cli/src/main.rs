//! `wmpass`: scripting access to vault files, and the harness for integration tests.
//! With no subcommand, in a terminal, it opens the full-screen app instead.
//!
//! The password is read from the terminal. `WMPASS_PASSWORD` exists for tests and
//! scripts, and anything else running as the same user can read a process's
//! environment, so it is not for everyday use.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use wmpass_core::crypto::KdfParams;
use wmpass_core::format::{KdfConfig, SlotAction, key_file_factor};
use wmpass_core::generator::{self, PassphraseSpec, PasswordSpec, Recipe};
use wmpass_core::model::{Kind, Lww, SiteMatcher};
use wmpass_core::otp::OtpConfig;
use wmpass_core::vault::{
    Freshness, OpenOutcome, Vault, check_freshness, hex, login_entry, open_set,
};
use wmpass_core::{Error, Secret, categorise, emergency, health, io, search, strength};

/// A line on stdout. A reader that has gone, as `wmpass ls … | head -1` leaves, ends the
/// program quietly, as it does other command-line tools, where `println!` would panic.
macro_rules! say {
    ($($arg:tt)*) => {
        say_line(format_args!($($arg)*))
    };
}

/// What `wmpass --help` adds under the commands: how scripts answer the questions, and what
/// the exit status means.
const AFTER_HELP: &str = "\
Passwords are asked for on the terminal. With none, as in a script, these answer instead;
anything else running as you can read them, so keep them for scripts and tests:
  WMPASS_PASSWORD         the vault's password, and a new vault's
  WMPASS_NEW_PASSWORD     a password being chosen: a duress password, a KeePass export's
  WMPASS_KEY_FILE         the vault's key file, when it has one
  WMPASS_IMPORT_PASSWORD  the password of a file being imported, if not the vault's
  WMPASS_PLACE_PASSWORD   the password or secret key of a place backups are kept
WMPASS_STATE_DIR is where revisions already seen are kept, to warn of an older copy.

Exit status: 0 done, 1 failed (the reason on stderr), 2 the command was not understood.";

#[derive(Parser)]
#[command(
    name = "wmpass",
    version,
    about = "WM Pass vaults from the command line",
    after_long_help = AFTER_HELP
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the full-screen app. Running `wmpass` alone in a terminal does the same.
    #[cfg(feature = "tui")]
    Tui {
        /// Vault file. The last one opened if left out.
        vault: Option<PathBuf>,
    },
    /// Get a vault you already have from another device on this network. Shows a QR code for a
    /// phone to scan, or compares six digits with a computer.
    #[cfg(feature = "tui")]
    Join {
        /// Where to keep it. Must not exist yet.
        vault: PathBuf,
    },
    /// Bring a vault onto this computer from a place its backups are kept: WebDAV, S3, SFTP or
    /// a folder. Lists the copies there, newest first, and restores the one picked. Passwords
    /// and keys are asked for, never taken as flags.
    #[cfg(feature = "tui")]
    Restore {
        #[command(subcommand)]
        from: Source,
        /// Folder to keep it in, which must not hold a vault yet. The WM Pass app's folder on
        /// this computer if left out.
        #[arg(long, global = true)]
        into: Option<PathBuf>,
        /// Take the newest copy without asking.
        #[arg(long, global = true)]
        latest: bool,
        /// Only list the copies.
        #[arg(long, global = true)]
        list: bool,
    },
    /// Send a vault to a new device on this network that is waiting to join.
    #[cfg(feature = "tui")]
    AddDevice {
        /// Vault file.
        vault: PathBuf,
        /// The text under the new device's QR code. Without it, the device is found on the
        /// network and six digits are compared.
        #[arg(long)]
        code: Option<String>,
    },
    /// Create a vault file.
    Create {
        /// Path of the new file.
        vault: PathBuf,
        /// Display name.
        #[arg(long, default_value = "Personal")]
        name: String,
    },
    /// List entries, optionally filtered by a search query.
    Ls {
        /// Vault file.
        vault: PathBuf,
        /// Search query, such as `tag:work has:totp`. A leading minus, as in `-weak`, is part
        /// of the query, not an option.
        #[arg(allow_hyphen_values = true)]
        query: Option<String>,
    },
    /// Show one entry. Secrets stay hidden unless asked for.
    Show {
        /// Vault file.
        vault: PathBuf,
        /// Entry id, the start of it as `ls` shows it, or title.
        entry: String,
        /// Print the password as well.
        #[arg(long)]
        reveal: bool,
    },
    /// Add a login. The password comes from the generator unless given on stdin.
    Add {
        /// Vault file.
        vault: PathBuf,
        /// Title.
        title: String,
        /// Username.
        #[arg(long, default_value = "")]
        username: String,
        /// Address.
        #[arg(long)]
        url: Option<String>,
        /// Read the password from stdin rather than generating one.
        #[arg(long)]
        password_stdin: bool,
        /// Attach a one-time code from an otpauth link.
        #[arg(long)]
        otp: Option<String>,
    },
    /// Move an entry to the trash.
    Rm {
        /// Vault file.
        vault: PathBuf,
        /// Entry id, the start of it as `ls` shows it, or title.
        entry: String,
    },
    /// Print the current one-time code of an entry.
    Otp {
        /// Vault file.
        vault: PathBuf,
        /// Entry id, the start of it as `ls` shows it, or title.
        entry: String,
    },
    /// Generate a password without touching any vault.
    Gen {
        /// Length, or word count with `--words`: 20 characters, or 6 words, if left out.
        #[arg(long)]
        length: Option<u32>,
        /// Use random words.
        #[arg(long)]
        words: bool,
        /// Leave out symbols.
        #[arg(long)]
        no_symbols: bool,
    },
    /// Rate a password read from stdin.
    Strength,
    /// Print the health report.
    Health {
        /// Vault file.
        vault: PathBuf,
    },
    /// Import a file from another app. The format is detected.
    Import {
        /// Vault file.
        vault: PathBuf,
        /// File to read.
        file: PathBuf,
        /// A KeePass database's key file. Such a database may have no password, so an
        /// empty one is then accepted.
        #[arg(long)]
        key_file: Option<PathBuf>,
    },
    /// Export. These formats hold every secret in the clear, except KDBX.
    Export {
        /// Vault file.
        vault: PathBuf,
        /// `csv`, `bitwarden`, `html` or `kdbx`.
        format: String,
        /// Where to write. Must not already exist.
        out: PathBuf,
    },
    /// Add a duress password. It does nothing visible but take the chosen action.
    Duress {
        /// Vault file.
        vault: PathBuf,
        /// `wipe` or `lock`.
        action: String,
    },
    /// Make Shamir shares for emergency access.
    Shares {
        /// Vault file.
        vault: PathBuf,
        /// Shares needed to open.
        #[arg(long)]
        threshold: u8,
        /// Shares to make.
        #[arg(long)]
        total: u8,
    },
    /// Open a vault with Shamir shares and list what it holds.
    Recover {
        /// Vault file.
        vault: PathBuf,
        /// The shares.
        shares: Vec<String>,
    },
    /// Merge another copy of the same vault into this one.
    Merge {
        /// Vault file to update.
        vault: PathBuf,
        /// Other copy.
        other: PathBuf,
    },
}

/// A place backups are kept.
#[cfg(feature = "tui")]
#[derive(Subcommand)]
enum Source {
    /// A WebDAV folder, such as Nextcloud's, a NAS's or `rclone serve webdav`.
    Webdav {
        /// The folder's address.
        url: String,
        /// User name. The password is asked for.
        #[arg(long)]
        user: Option<String>,
    },
    /// An S3 bucket: AWS, Backblaze B2, Cloudflare R2, MinIO, Garage and others.
    S3 {
        /// Bucket name.
        bucket: String,
        /// Access key id. The secret access key is asked for.
        #[arg(long)]
        access_key: String,
        /// Service address. Empty for AWS, whose address comes from the region.
        #[arg(long, default_value = "")]
        endpoint: String,
        /// Region.
        #[arg(long, default_value = "us-east-1")]
        region: String,
        /// Key prefix the copies are under.
        #[arg(long, default_value = "")]
        prefix: String,
        /// Address the bucket as a subdomain, as AWS prefers, rather than in the path.
        #[arg(long)]
        virtual_host: bool,
    },
    /// A folder on an SSH server.
    Sftp {
        /// Server name or address.
        host: String,
        /// User name.
        #[arg(long)]
        user: String,
        /// Port.
        #[arg(long, default_value_t = 22)]
        port: u16,
        /// Folder on the server the copies are in.
        #[arg(long, default_value = "")]
        path: String,
        /// Private key file to sign in with instead of a password.
        #[arg(long)]
        identity: Option<PathBuf>,
        /// The server key to expect, as `<algorithm> <base64>`, from its known_hosts line.
        /// Without it the first key seen is taken, and its fingerprint shown for checking.
        #[arg(long)]
        host_key: Option<String>,
    },
    /// A folder on this computer, or one another app syncs here.
    Folder {
        /// The folder.
        path: PathBuf,
    },
}

/// A place's password, asked for. `WMPASS_PLACE_PASSWORD` answers instead, for tests and
/// scripts, with the same warning as `WMPASS_PASSWORD`.
#[cfg(feature = "tui")]
fn place_secret(prompt: &str) -> Result<String, String> {
    if let Ok(value) = std::env::var("WMPASS_PLACE_PASSWORD") {
        return Ok(value);
    }
    ask(prompt, "WMPASS_PLACE_PASSWORD").map(|secret| secret.expose().to_owned())
}

#[cfg(feature = "tui")]
fn restore(from: Source, into: Option<PathBuf>, latest: bool, list_only: bool) -> Result<(), String> {
    use std::io::{BufRead, Write};
    use wmpass_tui::restore::{self, Kind, Secrets, Spot};
    use zeroize::Zeroizing;

    let into = match into {
        Some(path) => path,
        None if list_only => PathBuf::new(),
        None => restore::default_target().ok_or("say where to keep the vault with --into")?,
    };
    if !list_only && wmpass_tui::app_folder::names(&into).is_ok_and(|n| !n.is_empty()) {
        return Err(format!("{} already holds a vault; open it, or restore somewhere else with --into", into.display()));
    }
    let mut secrets = Secrets::default();
    let spot = match from {
        Source::Webdav { url, user } => {
            if user.is_some() {
                secrets.password = Zeroizing::new(place_secret("WebDAV password: ")?);
            }
            Spot { kind: Kind::WebDav, url, user: user.unwrap_or_default(), ..Spot::default() }
        }
        Source::S3 { bucket, access_key, endpoint, region, prefix, virtual_host } => {
            secrets.password = Zeroizing::new(place_secret("S3 secret access key: ")?);
            Spot { kind: Kind::S3, bucket, access_key, endpoint, region, prefix, virtual_host, ..Spot::default() }
        }
        Source::Sftp { host, user, port, path, identity, host_key } => {
            match identity {
                Some(file) => {
                    let key = Zeroizing::new(read_file(&file)?);
                    secrets.private_key = Zeroizing::new(String::from_utf8(key.to_vec()).map_err(|_| format!("{} is not a key file", file.display()))?);
                    secrets.key_passphrase = Zeroizing::new(place_secret("Key passphrase (Enter if none): ")?);
                }
                None => secrets.password = Zeroizing::new(place_secret("SFTP password: ")?),
            }
            Spot { kind: Kind::Sftp, host, user, port, path, host_key: host_key.unwrap_or_default(), ..Spot::default() }
        }
        Source::Folder { path } => Spot { kind: Kind::Folder, path: path.to_string_lossy().into_owned(), ..Spot::default() },
    };
    let mut place = spot.place(&secrets);
    drop(secrets);
    let copies = restore::copies(&mut place)?;
    if spot.kind == Kind::Sftp
        && spot.host_key.is_empty()
        && let Some(key) = restore::fingerprint(&place)
    {
        eprintln!("Server key {key}. Check that it matches your server's.");
    }
    if copies.is_empty() {
        return Err("no WM Pass backups there; check the address or folder".to_owned());
    }
    for (n, copy) in copies.iter().enumerate() {
        say!("{:>3}  {}", n + 1, restore::describe(copy));
    }
    if list_only {
        return Ok(());
    }
    let picked = if latest {
        0
    } else {
        let mut out = std::io::stdout();
        let _ = write!(out, "Which copy? [1] ");
        let _ = out.flush();
        let mut line = String::new();
        let read = std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
        // Nothing to read at all, as from a script, is no answer: Enter picks the newest, but
        // only someone who pressed it.
        if read == 0 {
            return Err("no answer on stdin; --latest takes the newest without asking".to_owned());
        }
        match line.trim() {
            "" => 0,
            typed => typed.parse::<usize>().ok().filter(|n| (1..=copies.len()).contains(n)).ok_or("pick a number from the list")? - 1,
        }
    };
    let bytes = restore::fetch(&mut place, &copies[picked])?;
    let files = restore::install(&into, &bytes)?;
    say!("Restored {} into {} ({files} vault {}).", copies[picked].name, into.display(), if files == 1 { "file" } else { "files, decoys among them" });
    say!("Open it with: wmpass tui {}", into.display());
    Ok(())
}

fn say_line(line: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_fmt(line).and_then(|()| out.write_all(b"\n")) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(141);
        }
        eprintln!("could not write the output: {}", io_text(&e));
        std::process::exit(1);
    }
}

/// Text from a vault or a file as one safe line: control characters, such as a newline that
/// would split an `ls` line or an escape that would retitle or recolour the terminal, are
/// written out as `\n` and `\u{1b}` instead.
fn clean(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.chars().any(char::is_control) {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(
        text.chars()
            .map(|ch| if ch.is_control() { ch.escape_default().to_string() } else { ch.to_string() })
            .collect(),
    )
}

/// A secret as printed: exact for a pipe or a file, which may need every byte, and cleaned
/// for a terminal, which would act on an escape in it.
fn clean_secret(text: &str) -> std::borrow::Cow<'_, str> {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() { clean(text) } else { std::borrow::Cow::Borrowed(text) }
}

/// A file error as a person reads it: "no such file or folder", not "No such file or
/// directory (os error 2)".
fn io_text(error: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::NotFound => "no such file or folder".to_owned(),
        ErrorKind::PermissionDenied => "not allowed to open it".to_owned(),
        ErrorKind::IsADirectory => "that is a folder, not a file".to_owned(),
        ErrorKind::AlreadyExists => "it already exists".to_owned(),
        ErrorKind::StorageFull => "the disk is full".to_owned(),
        _ => {
            let text = error.to_string();
            match text.rfind(" (os error ") {
                Some(at) if text.ends_with(')') => text[..at].to_owned(),
                _ => text,
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn device() -> u32 {
    let mut bytes = [0u8; 4];
    let _ = getrandom::fill(&mut bytes);
    u32::from_le_bytes(bytes)
}

/// Asks on the terminal. `variable` is what a script sets instead, named when there is no
/// terminal to ask on.
fn ask(prompt: &str, variable: &str) -> Result<Secret, String> {
    #[cfg(feature = "tui")]
    {
        use wmpass_tui::ask::{NoAnswer, secret};
        match secret(prompt) {
            Ok(typed) => Ok(Secret::new(typed.as_str())),
            Err(NoAnswer::NoTerminal) => Err(format!("no terminal to ask for the password on; in a script, set {variable}")),
            Err(other) => Err(other.to_string()),
        }
    }
    #[cfg(not(feature = "tui"))]
    {
        #[cfg(unix)]
        if let Some(answer) = ask_with_stty(prompt) {
            return answer;
        }
        rpassword::prompt_password(prompt).map(Secret::from).map_err(|e| match e.raw_os_error() {
            // ENXIO: no terminal behind the process.
            Some(6) => format!("no terminal to ask for the password on; in a script, set {variable}"),
            _ => format!("could not read the password: {e}"),
        })
    }
}

/// Puts the terminal back as it was, however the question ends.
#[cfg(all(unix, not(feature = "tui")))]
struct SttyGuard {
    tty: fs::File,
    saved: String,
}

#[cfg(all(unix, not(feature = "tui")))]
impl Drop for SttyGuard {
    fn drop(&mut self) {
        if let Ok(input) = self.tty.try_clone() {
            let _ = std::process::Command::new("stty").arg(&self.saved).stdin(input).status();
        }
    }
}

/// The question asked with `stty`, for a build without the full-screen app and its terminal
/// library. The usual way turns echo off and lets Ctrl+C kill the program, echo still off;
/// here Ctrl+C is turned into a plain key for the moment of the question, so it cancels and
/// the terminal is put back. `None` when there is no `stty` or no terminal to use it on.
#[cfg(all(unix, not(feature = "tui")))]
fn ask_with_stty(prompt: &str) -> Option<Result<Secret, String>> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let mut tty = fs::OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?;
    let stty = |args: &[&str]| -> Option<String> {
        let output = Command::new("stty").args(args).stdin(tty.try_clone().ok()?).stderr(Stdio::null()).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let saved = stty(&["-g"])?;
    let guard = SttyGuard { tty: tty.try_clone().ok()?, saved };
    stty(&["-echo", "-icanon", "-isig", "min", "1", "time", "0"])?;
    let _ = write!(tty, "{prompt}");
    let _ = tty.flush();
    let mut typed: Vec<u8> = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    let answer = loop {
        match tty.read(&mut byte) {
            Ok(0) | Err(_) => break Err("cancelled".to_owned()),
            Ok(_) => {}
        }
        match byte[0] {
            b'\r' | b'\n' => break Ok(()),
            // Ctrl+C, and Ctrl+D with nothing typed.
            0x03 => break Err("cancelled".to_owned()),
            0x04 if typed.is_empty() => break Err("cancelled".to_owned()),
            // Ctrl+U clears; Backspace takes one letter, however many bytes it is.
            0x15 => {
                typed.iter_mut().for_each(|b| *b = 0);
                typed.clear();
            }
            0x7f | 0x08 => {
                while typed.last().is_some_and(|b| b & 0xC0 == 0x80) {
                    typed.pop();
                }
                typed.pop();
            }
            b if b < 0x20 => {}
            b => typed.push(b),
        }
    };
    drop(guard);
    let _ = writeln!(tty);
    let result = answer.map(|()| Secret::new(String::from_utf8_lossy(&typed).as_ref()));
    typed.iter_mut().for_each(|b| *b = 0);
    Some(result)
}

fn password(prompt: &str) -> Result<Secret, String> {
    if let Ok(value) = std::env::var("WMPASS_PASSWORD") {
        return Ok(Secret::from(value));
    }
    ask(prompt, "WMPASS_PASSWORD")
}

/// A password being chosen, asked for twice. `WMPASS_NEW_PASSWORD` answers instead; for a new
/// vault, so does `WMPASS_PASSWORD`, which is that vault's password from then on. A duress
/// password or an export's never comes from `WMPASS_PASSWORD`: that would make it the vault's
/// own without a word.
fn new_password(prompt: &str, vaults_own: bool) -> Result<Secret, String> {
    let chosen = if let Ok(value) = std::env::var("WMPASS_NEW_PASSWORD") {
        Secret::from(value)
    } else if vaults_own && std::env::var_os("WMPASS_PASSWORD").is_some() {
        password(prompt)?
    } else {
        let first = ask(prompt, "WMPASS_NEW_PASSWORD")?;
        let second = ask("Again: ", "WMPASS_NEW_PASSWORD")?;
        if first != second {
            return Err("those did not match".to_owned());
        }
        first
    };
    // Enter alone, or an empty variable, is never meant as a password.
    if chosen.expose().is_empty() {
        return Err("the password cannot be empty".to_owned());
    }
    Ok(chosen)
}

/// One line from stdin, without its line ending. Nothing at all is an error, so a script
/// whose pipe came up empty does not save or rate an empty password.
fn stdin_line(what: &str) -> Result<Secret, String> {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|e| format!("could not read {what} from stdin: {e}"))?;
    let secret = Secret::new(line.trim_end_matches(['\n', '\r']));
    if secret.expose().is_empty() {
        return Err(format!("no {what} on stdin"));
    }
    Ok(secret)
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}: {}", path.display(), io_text(&e)))
}

/// Writes through a temporary file and a rename, keeping the old file as `.bak`, so
/// a crash at any moment leaves a readable vault.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let _ = fs::remove_file(&temp);
    write_private(&temp, bytes).map_err(|e| format!("{}: {}", temp.display(), io_text(&e)))?;
    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak");
        fs::copy(path, PathBuf::from(backup))
            .map_err(|e| format!("could not keep a backup: {}", io_text(&e)))?;
    }
    fs::rename(&temp, path).map_err(|e| format!("{}: {}", path.display(), io_text(&e)))
}

/// Creates `path`, which must not exist, readable only by its owner from the first byte. An
/// export holds every secret in the clear, so there is no moment it is anyone else's to read,
/// and a file that appeared since it was checked for is never replaced.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = fs::remove_file(path);
    }
    written
}

/// Where the revisions seen are kept: the folder the terminal app keeps its state in, so either
/// one notices a copy rolled back behind the other's back. `WMPASS_STATE_DIR` is that folder.
fn state_file() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("WMPASS_STATE_DIR") {
        return Some(PathBuf::from(dir).join("seen.json"));
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))?;
    Some(base.join("wmpass").join("seen.json"))
}

fn seen_generations() -> HashMap<String, u64> {
    state_file()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn remember_generation(vault_file: &[u8], generation: u64) {
    let Some(path) = state_file() else { return };
    let Ok(header) = wmpass_core::format::Header::parse(vault_file) else {
        return;
    };
    let mut seen = seen_generations();
    let id = hex(header.vault_id());
    let slot = seen.entry(id).or_default();
    *slot = (*slot).max(generation);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_vec(&seen) {
        let _ = fs::write(path, text);
    }
}

fn warn_if_rolled_back(vault_file: &[u8], generation: u64) {
    let Ok(header) = wmpass_core::format::Header::parse(vault_file) else {
        return;
    };
    let seen = seen_generations()
        .get(&hex(header.vault_id()))
        .copied()
        .unwrap_or(0);
    if let Freshness::Rollback { seen, file } = check_freshness(seen, generation) {
        eprintln!(
            "Warning: this copy is older than one already seen here (revision {file}, seen {seen})."
        );
        eprintln!("Someone may have replaced it with an old version. Check your sync location.");
    }
}

struct Opened {
    path: PathBuf,
    vault: Box<Vault>,
    /// The desktop app's folder, when the vault is the one it keeps.
    #[cfg(feature = "tui")]
    root: Option<PathBuf>,
}

/// The desktop app's folder, for `@app` or the folder's own path.
#[cfg(feature = "tui")]
fn app_root(path: &Path) -> Option<PathBuf> {
    use wmpass_tui::app_folder;
    if path == Path::new("@app") {
        return app_folder::default_root();
    }
    app_folder::is_app_folder(path).then(|| path.to_owned())
}

/// Opens the vault in the desktop app's folder that the password opens, counting a wrong one
/// against the app's own limit, as the app and the terminal app do.
#[cfg(feature = "tui")]
fn open_in_app(root: &Path) -> Result<Opened, String> {
    use wmpass_tui::app_folder::{self, DoorState};
    let mut door = DoorState::load(root);
    let now = now_ms();
    if let Some(at) = door.waiting(now) {
        return Err(format!("too many wrong passwords; try again in {} min", at.saturating_sub(now).div_ceil(60_000)));
    }
    let files = app_folder::files(root)?;
    if files.is_empty() {
        return Err("the WM Pass app has no vault on this computer yet".to_owned());
    }
    let secret = password("Vault password: ")?;
    let mut factors = Vec::new();
    if let Some(key_path) = std::env::var_os("WMPASS_KEY_FILE") {
        factors.push(key_file_factor(&read_file(Path::new(&key_path))?));
    }
    let set: Vec<&[u8]> = files.iter().map(|(_, bytes)| bytes.as_slice()).collect();
    match open_set(&set, secret.expose().as_bytes(), &factors, device()).map_err(|e| e.to_string())? {
        OpenOutcome::Opened { index, vault } => {
            door.opened();
            door.save(root);
            let (name, file) = &files[index];
            warn_if_rolled_back(file, vault.doc.generation);
            remember_generation(file, vault.doc.generation);
            Ok(Opened { path: app_folder::vault_path(root, name), vault, root: Some(root.to_owned()) })
        }
        OpenOutcome::Duress { action, .. } => {
            if action == SlotAction::WipeLocal {
                app_folder::wipe(root);
            } else {
                door.hard_lock(now);
                door.save(root);
            }
            Err("wrong password".to_owned())
        }
        OpenOutcome::NoMatch => {
            door.failed(now);
            door.save(root);
            Err("wrong password".to_owned())
        }
        OpenOutcome::NeedsFactor(_) => Err("this vault also needs a second factor; set WMPASS_KEY_FILE".to_owned()),
    }
}

fn open(path: &Path) -> Result<Opened, String> {
    #[cfg(feature = "tui")]
    if let Some(root) = app_root(path).or_else(|| wmpass_tui::app_folder::root_of(path)) {
        return open_in_app(&root);
    }
    let file = read_file(path)?;
    // A file that is no vault at all, or one cut short, would otherwise ask for the password
    // and then call it wrong.
    if let Err(e) = wmpass_core::format::Header::parse(&file) {
        return Err(format!("{}: {e}", path.display()));
    }
    let secret = password("Vault password: ")?;
    let mut factors = Vec::new();
    if let Some(key_path) = std::env::var_os("WMPASS_KEY_FILE") {
        factors.push(key_file_factor(&read_file(Path::new(&key_path))?));
    }
    match open_set(&[&file], secret.expose().as_bytes(), &factors, device())
        .map_err(|e| e.to_string())?
    {
        OpenOutcome::Opened { vault, .. } => {
            warn_if_rolled_back(&file, vault.doc.generation);
            remember_generation(&file, vault.doc.generation);
            Ok(Opened {
                path: path.to_owned(),
                vault,
                #[cfg(feature = "tui")]
                root: None,
            })
        }
        // A duress password looks like a wrong one from here.
        OpenOutcome::Duress { .. } | OpenOutcome::NoMatch => Err("wrong password".to_owned()),
        OpenOutcome::NeedsFactor(_) => {
            Err("this vault also needs a second factor; set WMPASS_KEY_FILE".to_owned())
        }
    }
}

/// Where a vault's attachments are kept: beside its file, or under `blobs/` in the desktop
/// app's folder, as the terminal app keeps them.
fn blob_dir(opened: &Opened) -> PathBuf {
    #[cfg(feature = "tui")]
    if let (Some(root), Some(name)) = (&opened.root, opened.path.file_name()) {
        return root.join("blobs").join(name);
    }
    opened.path.with_extension("blobs")
}

fn save(opened: &mut Opened) -> Result<(), String> {
    let bytes = opened.vault.save().map_err(|e| e.to_string())?;
    #[cfg(feature = "tui")]
    if let Some(root) = &opened.root {
        // As the app writes its folder: no backup beside the file, the partner decoy kept the
        // same size, and every file's time the same.
        use wmpass_tui::app_folder;
        app_folder::write(&opened.path, &bytes)?;
        remember_generation(&bytes, opened.vault.doc.generation);
        let partner = opened.vault.chaff_file_name().map_err(|e| e.to_string())?;
        let config = opened.vault.kdf_config().map_err(|e| e.to_string())?;
        return app_folder::cover_tracks(root, &partner, bytes.len(), |n| wmpass_core::format::chaff_like(&config, n).map_err(|e| e.to_string()));
    }
    write_atomic(&opened.path, &bytes)?;
    remember_generation(&bytes, opened.vault.doc.generation);
    Ok(())
}

/// An entry named by its id, the start of its id as `ls` prints it, or its title: exactly as
/// written first, then in any case.
fn find_id(vault: &Vault, wanted: &str) -> Result<String, String> {
    if vault.get(wanted).is_some() {
        return Ok(wanted.to_owned());
    }
    let live = || vault.doc.entries.values().filter(|entry| !entry.is_trashed());
    let tries: [&dyn Fn(&wmpass_core::model::Entry) -> bool; 3] = [
        &|entry| entry.common.title.v == wanted,
        // `ls` shows the first eight characters of each id.
        &|entry| wanted.len() >= 4 && entry.id.starts_with(wanted),
        &|entry| entry.common.title.v.to_lowercase() == wanted.to_lowercase(),
    ];
    for matching in tries {
        let matches: Vec<_> = live().filter(|entry| matching(entry)).collect();
        match matches.as_slice() {
            [] => {}
            [one] => return Ok(one.id.clone()),
            _ => {
                return Err(format!(
                    "{} entries match {wanted}; use more of the id `wmpass ls` shows",
                    matches.len()
                ));
            }
        }
    }
    Err(format!("no entry called {wanted}; `wmpass ls` lists them"))
}

/// One `Label:   value` line of `show`, left out when there is nothing to say. A value of
/// several lines, such as an address or a key, goes on under the first, lined up with it.
fn shown_line(label: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    let head = format!("{label}:");
    let width = head.chars().count().max(9) + 1;
    for (n, line) in value.lines().enumerate() {
        if n == 0 {
            say!("{head:<width$}{}", clean(line));
        } else {
            say!("{:width$}{}", "", clean(line));
        }
    }
}

/// A secret's line: masked unless `--reveal`, and then exact for a pipe, as `Password:` is.
fn shown_secret(label: &str, value: &Secret, reveal: bool) {
    let text = value.expose();
    if text.is_empty() {
        return;
    }
    if !reveal {
        return shown_line(label, "(hidden, add --reveal)");
    }
    if text.contains('\n') {
        return shown_line(label, text);
    }
    let head = format!("{label}:");
    let width = head.chars().count().max(9) + 1;
    say!("{head:<width$}{}", clean_secret(text));
}

/// A day, as `2026-12-31`, from milliseconds since 1970.
fn day_text(ms: u64) -> String {
    // Howard Hinnant's days-to-civil, for dates from 1970 on.
    let days = i64::try_from(ms / 86_400_000).unwrap_or(i64::MAX / 2) + 719_468;
    let era = days / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn yes_no(on: bool) -> &'static str {
    if on { "yes" } else { "no" }
}

/// What `show` prints of one entry: its own fields for every kind, then what all kinds share.
/// Secrets are masked unless `reveal`.
fn show_entry(vault: &Vault, found: &wmpass_core::model::Entry, reveal: bool) {
    use wmpass_core::model::{FieldKind, RecoveryForm, WifiSecurity};
    shown_line("Title", &found.common.title.v);
    shown_line("Kind", found.kind.name());
    shown_line("Id", &found.id);
    if found.is_trashed() {
        shown_line("Trashed", "yes; `wmpass ls trashed` lists the trash");
    }
    let otp_line = |config: &Option<wmpass_core::otp::OtpConfig>| {
        if let Some(config) = config {
            let who = [config.issuer.as_str(), config.account.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
            shown_line("Code", &format!("{}`wmpass otp` prints it", if who.is_empty() { String::new() } else { format!("{who}; ") }));
        }
    };
    let fields = |list: &[wmpass_core::model::Field]| {
        for field in list {
            if field.kind == FieldKind::Hidden {
                shown_secret(&field.name, &field.value, reveal);
            } else {
                shown_line(&field.name, field.value.expose());
            }
        }
    };
    match &found.kind {
        Kind::Login(login) => {
            shown_line("Username", &login.username.v);
            if login.password.v.expose().is_empty() {
                shown_line("Password", "(none)");
            } else {
                shown_secret("Password", &login.password.v, reveal);
            }
            for matcher in &login.site_matchers.v {
                match matcher {
                    SiteMatcher::Url(url) | SiteMatcher::Domain(url) | SiteMatcher::Host(url) => shown_line("Address", url),
                    SiteMatcher::AndroidApp { package, .. } => shown_line("App", package),
                    SiteMatcher::Regex(_) | SiteMatcher::Never => {}
                }
            }
            otp_line(&login.otp.v);
            if !login.passkeys.v.is_empty() {
                shown_line("Passkeys", &login.passkeys.v.len().to_string());
            }
            for question in &login.questions.v {
                shown_secret(&question.name, &question.value, reveal);
            }
        }
        Kind::Card(card) => {
            shown_line("Name", &card.holder.v);
            shown_secret("Number", &card.number.v, reveal);
            shown_line("Expires", &card.expiry.v);
            shown_secret("Security code", &card.cvv.v, reveal);
            shown_secret("PIN", &card.pin.v, reveal);
            shown_line("Issuer", &card.issuer.v);
            shown_line("Billing", &card.billing.v);
        }
        Kind::Identity(identity) => {
            shown_line("Name", &identity.full_name.v);
            shown_line("Address", &identity.address.v);
            shown_line("Phone", &identity.phone.v);
            shown_line("Email", &identity.email.v);
            shown_secret("Born", &identity.birth_date.v, reveal);
            fields(&identity.document_numbers.v);
        }
        Kind::Note(note) => shown_secret("Text", &note.body.v, reveal),
        Kind::Document(document) => {
            shown_line("Label", &document.label.v);
            shown_secret("Number", &document.number.v, reveal);
        }
        Kind::SshKey(key) => {
            shown_line("Public key", &key.public_key.v);
            shown_line("Fingerprint", &key.fingerprint.v);
            shown_secret("Private key", &key.private_key.v, reveal);
            shown_secret("Passphrase", &key.passphrase.v, reveal);
            shown_line("SSH agent", yes_no(key.agent_allowed.v));
        }
        Kind::ApiKey(api) => {
            shown_line("Provider", &api.provider.v);
            shown_secret("Key", &api.key.v, reveal);
            shown_secret("Secret", &api.secret.v, reveal);
            shown_line("Scopes", &api.scopes.v);
            if let Some(ms) = api.expires_ms.v {
                shown_line("Expires", &day_text(ms));
            }
        }
        Kind::Database(db) => {
            shown_line("Engine", &db.engine.v);
            shown_line("Host", &db.host.v);
            if let Some(port) = db.port.v {
                shown_line("Port", &port.to_string());
            }
            shown_line("Database", &db.name.v);
            shown_line("User", &db.user.v);
            shown_secret("Password", &db.password.v, reveal);
            shown_line("TLS", yes_no(db.tls.v));
            shown_secret("URL", &db.url.v, reveal);
        }
        Kind::Licence(licence) => {
            shown_line("Product", &licence.product.v);
            shown_secret("Key", &licence.key.v, reveal);
            shown_line("Email", &licence.email.v);
            shown_line("Order", &licence.order_id.v);
            if let Some(ms) = licence.expires_ms.v {
                shown_line("Expires", &day_text(ms));
            }
        }
        Kind::Recovery(recovery) => {
            let label = match recovery.form.v {
                RecoveryForm::Phrase => "Phrase",
                RecoveryForm::Codes => "Codes",
            };
            shown_secret(label, &recovery.phrase.v, reveal);
        }
        Kind::Wifi(wifi) => {
            shown_line("Network", &wifi.ssid.v);
            let security = match wifi.security.v {
                WifiSecurity::Open => "open",
                WifiSecurity::Wep => "WEP",
                WifiSecurity::Wpa => "WPA2",
                WifiSecurity::Sae => "WPA3",
                WifiSecurity::Eap => "WPA2 Enterprise",
            };
            shown_line("Security", security);
            shown_secret("Password", &wifi.password.v, reveal);
            if wifi.hidden_ssid.v {
                shown_line("Hidden", "yes");
            }
        }
        Kind::Otp(otp) => otp_line(&otp.config.v),
    }
    fields(&found.common.custom_fields.v);
    if let Some(folder) = found.common.folder.v.as_deref().and_then(|id| vault.doc.folders.get(id)) {
        shown_line("Folder", &folder.name.v);
    }
    shown_line("Tags", &found.common.tags.v.join(", "));
    let files: Vec<&str> = found.common.attachments.v.iter().map(|a| a.name.as_str()).collect();
    shown_line("Files", &files.join(", "));
    shown_secret("Notes", &found.common.notes.v, reveal);
    match found.history.len() {
        0 => {}
        1 => shown_line("History", "1 earlier value"),
        past => shown_line("History", &format!("{past} earlier values")),
    }
}

/// How many items a vault holds, as `ls` counts them, with the trash said apart: "12 items,
/// 2 in the trash".
fn counted(vault: &Vault) -> String {
    let trashed = vault.doc.entries.values().filter(|entry| entry.is_trashed()).count();
    let live = vault.doc.entries.len() - trashed;
    let items = if live == 1 { "1 item".to_owned() } else { format!("{live} items") };
    if trashed == 0 { items } else { format!("{items}, {trashed} in the trash") }
}

/// Folder names for `folder:` in a query.
#[derive(Debug)]
struct FolderNames<'a>(&'a Vault);

impl search::FolderNames for FolderNames<'_> {
    fn folder_name(&self, id: &str) -> Option<String> {
        self.0.doc.folders.get(id).map(|folder| folder.name.v.clone())
    }
}

fn login_line(entry: &wmpass_core::model::Entry) -> String {
    let user = entry.login().map_or("", |login| login.username.v.as_str());
    // Ids are hex as this app makes them, but one from elsewhere need not be.
    let short: String = entry.id.chars().take(8).collect();
    format!(
        "{}  {:<9} {}  {}",
        clean(&short),
        entry.kind.name(),
        clean(&entry.common.title.v),
        clean(user)
    )
}

fn run(cli: Cli) -> Result<(), String> {
    let now = now_ms();
    let Some(command) = cli.command else {
        return no_subcommand();
    };
    match command {
        #[cfg(feature = "tui")]
        Command::Tui { vault } => wmpass_tui::run(vault)?,
        #[cfg(feature = "tui")]
        Command::Join { vault } => wmpass_tui::join(vault)?,
        #[cfg(feature = "tui")]
        Command::AddDevice { vault, code } => wmpass_tui::add_device(vault, code.as_deref())?,
        #[cfg(feature = "tui")]
        Command::Restore { from, into, latest, list } => restore(from, into, latest, list)?,
        Command::Create { vault, name } => {
            if vault.exists() {
                return Err(format!("{} already exists", vault.display()));
            }
            let secret = new_password("New vault password: ", true)?;
            let config = KdfConfig::new(KdfParams::default()).map_err(|e| e.to_string())?;
            let mut created =
                Vault::create(&name, secret.expose().as_bytes(), &config, device(), now)
                    .map_err(|e| e.to_string())?;
            let bytes = created.save().map_err(|e| e.to_string())?;
            write_atomic(&vault, &bytes)?;
            say!("Created {}.", vault.display());
        }
        Command::Ls { vault, query } => {
            let opened = open(&vault)?;
            let parsed = search::Query::parse(query.as_deref().unwrap_or(""));
            // `weak`, `-reused` and the like keep nothing, or leave nothing out, without the report.
            let flags = parsed.wants_health().then(|| {
                let input = health::HealthInput { now_ms: now, ..health::HealthInput::default() };
                let report = health::report(&opened.vault, &input);
                search::Flags {
                    weak: report.weak.iter().cloned().collect(),
                    reused: report.reused.iter().flatten().cloned().collect(),
                    breached: report.breached.iter().map(|(id, _)| id.clone()).collect(),
                    old: report.old.iter().cloned().collect(),
                    duplicate: report.duplicates.iter().flatten().cloned().collect(),
                }
            });
            let names = FolderNames(&opened.vault);
            let context = search::Context {
                now_ms: now,
                flags: flags.as_ref(),
                folder_name: Some(&names),
            };
            for entry in search::search(opened.vault.doc.entries.values(), &parsed, &context) {
                say!("{}", login_line(entry));
            }
        }
        Command::Show {
            vault,
            entry,
            reveal,
        } => {
            let opened = open(&vault)?;
            let id = find_id(&opened.vault, &entry)?;
            let found = opened.vault.get(&id).ok_or("no such entry")?;
            show_entry(&opened.vault, found, reveal);
        }
        Command::Add {
            vault,
            title,
            username,
            url,
            password_stdin,
            otp,
        } => {
            let mut opened = open(&vault)?;
            let generated = !password_stdin;
            let secret = if password_stdin {
                stdin_line("password")?
            } else {
                generator::password(&PasswordSpec::default()).map_err(|e| e.to_string())?
            };
            let at = opened.vault.tick(now);
            let mut entry = login_entry(&title, &username, secret.expose(), at);
            if let Some(login) = entry.login_mut() {
                if let Some(url) = url {
                    login.site_matchers = Lww::new(vec![SiteMatcher::Url(url)], at);
                }
                if let Some(uri) = otp {
                    login.otp = Lww::new(
                        Some(OtpConfig::from_uri(&uri).map_err(|e| e.to_string())?),
                        at,
                    );
                }
            }
            if let Some(found) = categorise::suggest(&entry) {
                entry.common.tags = Lww::new(vec![found.category.to_owned()], at);
            }
            let id = opened.vault.add(entry, now).map_err(|e| e.to_string())?;
            save(&mut opened)?;
            say!("Added {id}.");
            if generated {
                say!("Generated password: {}", secret.expose());
            }
        }
        Command::Rm { vault, entry } => {
            let mut opened = open(&vault)?;
            let id = find_id(&opened.vault, &entry)?;
            opened.vault.trash(&id, now).map_err(|e| e.to_string())?;
            save(&mut opened)?;
            // Zero days means the trash is never emptied on its own.
            match opened.vault.doc.prefs.trash_days {
                0 => say!("Moved to the trash. It stays until the trash is emptied."),
                1 => say!("Moved to the trash. It stays for 1 day."),
                days => say!("Moved to the trash. It stays for {days} days."),
            }
        }
        Command::Otp { vault, entry } => {
            let mut opened = open(&vault)?;
            let id = find_id(&opened.vault, &entry)?;
            let found = opened.vault.get(&id).ok_or("no such entry")?;
            let config = match &found.kind {
                Kind::Login(login) => login.otp.v.clone(),
                Kind::Otp(otp) => otp.config.v.clone(),
                _ => None,
            }
            .ok_or("that entry has no one-time code")?;
            let code = config.code(now / 1000).map_err(|e| e.to_string())?;
            // A counter code is used up once shown: the counter moves on and is saved, or the
            // next run would print the same code again.
            if matches!(config.kind, wmpass_core::otp::OtpKind::Hotp { .. }) {
                let mut next = config.clone();
                next.advance();
                opened
                    .vault
                    .edit(&id, now, |entry, at| match &mut entry.kind {
                        Kind::Login(login) => login.otp = Lww::new(Some(next), at),
                        Kind::Otp(otp) => otp.config = Lww::new(Some(next), at),
                        _ => {}
                    })
                    .map_err(|e| e.to_string())?;
                save(&mut opened)?;
            }
            say!("{}", code.expose());
            if let Some(left) = config.seconds_left(now / 1000) {
                eprintln!("{left} seconds left");
            }
        }
        Command::Gen {
            length,
            words,
            no_symbols,
        } => {
            let recipe = if words {
                Recipe::Passphrase(PassphraseSpec {
                    words: length.unwrap_or(6),
                    ..PassphraseSpec::default()
                })
            } else {
                Recipe::Password(PasswordSpec {
                    length: length.unwrap_or(20),
                    symbols: !no_symbols,
                    ..PasswordSpec::default()
                })
            };
            say!(
                "{}",
                generator::generate(&recipe)
                    .map_err(|e| e.to_string())?
                    .expose()
            );
            eprintln!("{:.0} bits of entropy", generator::entropy_bits(&recipe));
        }
        Command::Strength => {
            let secret = stdin_line("password")?;
            let found = strength::assess(secret.expose(), &[]);
            say!("Score: {}/4", found.score);
            say!("Offline, slow hash: {}", found.crack_slow_hash);
            say!("Offline, fast hash: {}", found.crack_fast_hash);
            if let Some(warning) = found.warning {
                say!("Warning: {warning}");
            }
            for tip in found.suggestions {
                say!("Tip: {tip}");
            }
        }
        Command::Health { vault } => {
            let opened = open(&vault)?;
            let input = health::HealthInput {
                now_ms: now,
                ..health::HealthInput::default()
            };
            let report = health::report(&opened.vault, &input);
            say!("Score: {}/100", report.score);
            say!("Weak: {}", report.weak.len());
            say!("Reused groups: {}", report.reused.len());
            say!("Old: {}", report.old.len());
            say!("Missing two-factor: {}", report.missing_two_factor.len());
            say!("Plain http addresses: {}", report.insecure_url.len());
            say!("Duplicate groups: {}", report.duplicates.len());
            say!("Expiring: {}", report.expiring.len());
        }
        Command::Import {
            vault,
            file,
            key_file,
        } => {
            let mut opened = open(&vault)?;
            let data = read_file(&file)?;
            let format = io::detect(&data).ok_or("the format was not recognised")?;
            let key_file = key_file.map(|path| read_file(&path)).transpose()?;
            let source_password = if io::needs_password(&data) {
                // The file's own password, which can differ from the vault's.
                Some(match std::env::var("WMPASS_IMPORT_PASSWORD") {
                    Ok(value) => Secret::from(value),
                    Err(_) => password("Password of the file being imported: ")?,
                })
            } else {
                None
            };
            let at = opened.vault.tick(now);
            let imported = io::import_with_key_file(
                format,
                &data,
                source_password.as_ref().map(Secret::expose),
                key_file.as_deref(),
                at,
            )
            .map_err(|e| e.to_string())?;
            let skipped = imported.skipped.clone();
            let report = io::apply(&mut opened.vault, imported, now).map_err(|e| e.to_string())?;
            let blob_dir = blob_dir(&opened);
            if !report.blobs.is_empty() {
                fs::create_dir_all(&blob_dir).map_err(|e| e.to_string())?;
                for (name, bytes) in &report.blobs {
                    fs::write(blob_dir.join(name), bytes).map_err(|e| e.to_string())?;
                }
            }
            save(&mut opened)?;
            say!(
                "{}: {} added, {} already present.",
                format.name(),
                report.added,
                report.duplicates
            );
            for note in skipped {
                say!("Note: {}", clean(&note));
            }
        }
        Command::Export { vault, format, out } => {
            // Before the password is asked for, so a typo costs nothing.
            if !matches!(format.as_str(), "csv" | "bitwarden" | "html" | "kdbx") {
                return Err(format!("unknown format {format}; use csv, bitwarden, html or kdbx"));
            }
            if out.exists() {
                return Err(format!("{} already exists", out.display()));
            }
            let opened = open(&vault)?;
            let bytes = match format.as_str() {
                "csv" => io::export::to_csv(&opened.vault)
                    .map_err(|e| e.to_string())?
                    .expose()
                    .as_bytes()
                    .to_vec(),
                "bitwarden" => io::export::to_bitwarden_json(&opened.vault)
                    .map_err(|e| e.to_string())?
                    .expose()
                    .as_bytes()
                    .to_vec(),
                "html" => io::export::to_print_html(&opened.vault)
                    .expose()
                    .as_bytes()
                    .to_vec(),
                "kdbx" => {
                    let secret = new_password("Password for the export: ", false)?;
                    let entries: Vec<_> = opened
                        .vault
                        .doc
                        .entries
                        .values()
                        .filter(|e| !e.is_trashed())
                        .map(|e| (e, io::export::folder_path(&opened.vault, e)))
                        .collect();
                    io::kdbx::export_in_folders(&entries, secret.expose())
                        .map_err(|e| e.to_string())?
                }
                other => return Err(format!("unknown format {other}")),
            };
            write_private(&out, &bytes).map_err(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => format!("{} already exists", out.display()),
                _ => format!("{}: {}", out.display(), io_text(&e)),
            })?;
            if format != "kdbx" {
                eprintln!(
                    "{} holds your secrets in the clear. Delete it when you are done.",
                    out.display()
                );
            }
            say!("Wrote {}.", out.display());
        }
        Command::Duress { vault, action } => {
            let action = match action.as_str() {
                "wipe" => SlotAction::WipeLocal,
                "lock" => SlotAction::HardLock,
                other => return Err(format!("unknown action {other}; use wipe or lock")),
            };
            let mut opened = open(&vault)?;
            let secret = new_password("Duress password: ", false)?;
            opened
                .vault
                .add_slot(secret.expose().as_bytes(), "Duress", action, None, now)
                .map_err(|e| e.to_string())?;
            save(&mut opened)?;
            say!("Added.");
        }
        Command::Shares {
            vault,
            threshold,
            total,
        } => {
            let mut opened = open(&vault)?;
            let shares = emergency::create_shares(&mut opened.vault, threshold, total, now)
                .map_err(|e| e.to_string())?;
            save(&mut opened)?;
            say!(
                "Any {threshold} of these {total} shares open this vault. Give each person one."
            );
            say!(
                "The waiting period is advice only: people holding enough shares can act at once."
            );
            for share in shares {
                say!("{}: {}", share.number, share.text);
            }
        }
        Command::Recover { vault, shares } => {
            let file = read_file(&vault)?;
            let texts: Vec<&str> = shares.iter().map(String::as_str).collect();
            let opened = emergency::recover(&file, &texts, device()).map_err(|e| e.to_string())?;
            say!("Opened {}. {}:", clean(&opened.doc.name), counted(&opened));
            for entry in opened.doc.entries.values().filter(|entry| !entry.is_trashed()) {
                say!("{}", login_line(entry));
            }
        }
        Command::Merge { vault, other } => {
            let mut opened = open(&vault)?;
            let data = read_file(&other)?;
            opened.vault.merge_file(&data).map_err(|e| match e {
                Error::Integrity => {
                    "that file is a different vault, or was rekeyed since".to_owned()
                }
                other => other.to_string(),
            })?;
            save(&mut opened)?;
            say!("Merged. {}.", counted(&opened.vault));
        }
    }
    Ok(())
}

/// In a terminal, the full-screen app; anywhere else, the list of subcommands, so a script
/// that forgot one fails plainly rather than hanging on a screen nobody sees.
fn no_subcommand() -> Result<(), String> {
    #[cfg(feature = "tui")]
    use std::io::IsTerminal;
    #[cfg(feature = "tui")]
    if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
        return wmpass_tui::run(None);
    }
    // A usage mistake, as clap reports a missing argument: the help on stderr, and 2.
    let help = <Cli as clap::CommandFactory>::command().render_help();
    eprint!("{help}");
    std::process::exit(2);
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
