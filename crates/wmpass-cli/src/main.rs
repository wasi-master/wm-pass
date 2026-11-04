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

#[derive(Parser)]
#[command(
    name = "wmpass",
    version,
    about = "WM Pass vaults from the command line"
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
        /// Search query, such as `tag:work has:totp`.
        query: Option<String>,
    },
    /// Show one entry. Secrets stay hidden unless asked for.
    Show {
        /// Vault file.
        vault: PathBuf,
        /// Entry id or exact title.
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
        /// Entry id or exact title.
        entry: String,
    },
    /// Print the current one-time code of an entry.
    Otp {
        /// Vault file.
        vault: PathBuf,
        /// Entry id or exact title.
        entry: String,
    },
    /// Generate a password without touching any vault.
    Gen {
        /// Length, or word count with `--words`.
        #[arg(long, default_value_t = 20)]
        length: u32,
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
    rpassword::prompt_password(prompt).map_err(|e| format!("could not read the password: {e}"))
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
        println!("{:>3}  {}", n + 1, restore::describe(copy));
    }
    if list_only {
        return Ok(());
    }
    let picked = if latest {
        0
    } else {
        print!("Which copy? [1] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
        match line.trim() {
            "" => 0,
            typed => typed.parse::<usize>().ok().filter(|n| (1..=copies.len()).contains(n)).ok_or("pick a number from the list")? - 1,
        }
    };
    let bytes = restore::fetch(&mut place, &copies[picked])?;
    let files = restore::install(&into, &bytes)?;
    println!("Restored {} into {} ({files} vault {}).", copies[picked].name, into.display(), if files == 1 { "file" } else { "files, decoys among them" });
    println!("Open it with: wmpass tui {}", into.display());
    Ok(())
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

fn password(prompt: &str) -> Result<Secret, String> {
    if let Ok(value) = std::env::var("WMPASS_PASSWORD") {
        return Ok(Secret::from(value));
    }
    rpassword::prompt_password(prompt)
        .map(Secret::from)
        .map_err(|e| format!("could not read the password: {e}"))
}

fn new_password(prompt: &str) -> Result<Secret, String> {
    if let Ok(value) = std::env::var("WMPASS_NEW_PASSWORD") {
        return Ok(Secret::from(value));
    }
    if std::env::var_os("WMPASS_PASSWORD").is_some() {
        return password(prompt);
    }
    let first = password(prompt)?;
    let second = password("Again: ")?;
    if first == second {
        Ok(first)
    } else {
        Err("those did not match".to_owned())
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes through a temporary file and a rename, keeping the old file as `.bak`, so
/// a crash at any moment leaves a readable vault.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    restrict(&temp);
    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak");
        fs::copy(path, PathBuf::from(backup))
            .map_err(|e| format!("could not keep a backup: {e}"))?;
    }
    fs::rename(&temp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

fn state_file() -> Option<PathBuf> {
    let base = std::env::var_os("WMPASS_STATE_DIR")
        .or_else(|| std::env::var_os("XDG_STATE_HOME"))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
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

fn find_id(vault: &Vault, wanted: &str) -> Result<String, String> {
    if vault.get(wanted).is_some() {
        return Ok(wanted.to_owned());
    }
    let matches: Vec<_> = vault
        .doc
        .entries
        .values()
        .filter(|entry| !entry.is_trashed() && entry.common.title.v == wanted)
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(format!("no entry called {wanted}")),
        _ => Err(format!(
            "{} entries are called {wanted}; use an id",
            matches.len()
        )),
    }
}

fn login_line(entry: &wmpass_core::model::Entry) -> String {
    let user = entry.login().map_or("", |login| login.username.v.as_str());
    format!(
        "{}  {:<9} {}  {}",
        &entry.id[..8.min(entry.id.len())],
        entry.kind.name(),
        entry.common.title.v,
        user
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
            let secret = new_password("New vault password: ")?;
            let config = KdfConfig::new(KdfParams::default()).map_err(|e| e.to_string())?;
            let mut created =
                Vault::create(&name, secret.expose().as_bytes(), &config, device(), now)
                    .map_err(|e| e.to_string())?;
            let bytes = created.save().map_err(|e| e.to_string())?;
            write_atomic(&vault, &bytes)?;
            println!("Created {}.", vault.display());
        }
        Command::Ls { vault, query } => {
            let opened = open(&vault)?;
            let context = search::Context {
                now_ms: now,
                ..search::Context::default()
            };
            let parsed = search::Query::parse(query.as_deref().unwrap_or(""));
            for entry in search::search(opened.vault.doc.entries.values(), &parsed, &context) {
                println!("{}", login_line(entry));
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
            println!("Title:    {}", found.common.title.v);
            println!("Kind:     {}", found.kind.name());
            println!("Id:       {}", found.id);
            if let Kind::Login(login) = &found.kind {
                println!("Username: {}", login.username.v);
                if reveal {
                    println!("Password: {}", login.password.v.expose());
                } else {
                    println!("Password: (hidden, add --reveal)");
                }
                for matcher in &login.site_matchers.v {
                    if let SiteMatcher::Url(url)
                    | SiteMatcher::Domain(url)
                    | SiteMatcher::Host(url) = matcher
                    {
                        println!("Address:  {url}");
                    }
                }
            }
            let past = found.history.len();
            if past > 0 {
                println!("History:  {past} earlier value(s)");
            }
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
                let mut line = String::new();
                std::io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?;
                Secret::new(line.trim_end_matches(['\n', '\r']))
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
            println!("Added {id}.");
            if generated {
                println!("Generated password: {}", secret.expose());
            }
        }
        Command::Rm { vault, entry } => {
            let mut opened = open(&vault)?;
            let id = find_id(&opened.vault, &entry)?;
            opened.vault.trash(&id, now).map_err(|e| e.to_string())?;
            save(&mut opened)?;
            println!(
                "Moved to the trash. It stays for {} days.",
                opened.vault.doc.prefs.trash_days
            );
        }
        Command::Otp { vault, entry } => {
            let opened = open(&vault)?;
            let id = find_id(&opened.vault, &entry)?;
            let found = opened.vault.get(&id).ok_or("no such entry")?;
            let config = match &found.kind {
                Kind::Login(login) => login.otp.v.as_ref(),
                Kind::Otp(otp) => otp.config.v.as_ref(),
                _ => None,
            }
            .ok_or("that entry has no one-time code")?;
            let code = config.code(now / 1000).map_err(|e| e.to_string())?;
            println!("{}", code.expose());
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
                let count = if length == 20 { 6 } else { length };
                Recipe::Passphrase(PassphraseSpec {
                    words: count,
                    ..PassphraseSpec::default()
                })
            } else {
                Recipe::Password(PasswordSpec {
                    length,
                    symbols: !no_symbols,
                    ..PasswordSpec::default()
                })
            };
            println!(
                "{}",
                generator::generate(&recipe)
                    .map_err(|e| e.to_string())?
                    .expose()
            );
            eprintln!("{:.0} bits of entropy", generator::entropy_bits(&recipe));
        }
        Command::Strength => {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            let secret = Secret::new(line.trim_end_matches(['\n', '\r']));
            let found = strength::assess(secret.expose(), &[]);
            println!("Score: {}/4", found.score);
            println!("Offline, slow hash: {}", found.crack_slow_hash);
            println!("Offline, fast hash: {}", found.crack_fast_hash);
            if let Some(warning) = found.warning {
                println!("Warning: {warning}");
            }
            for tip in found.suggestions {
                println!("Tip: {tip}");
            }
        }
        Command::Health { vault } => {
            let opened = open(&vault)?;
            let input = health::HealthInput {
                now_ms: now,
                ..health::HealthInput::default()
            };
            let report = health::report(&opened.vault, &input);
            println!("Score: {}/100", report.score);
            println!("Weak: {}", report.weak.len());
            println!("Reused groups: {}", report.reused.len());
            println!("Old: {}", report.old.len());
            println!("Missing two-factor: {}", report.missing_two_factor.len());
            println!("Plain http addresses: {}", report.insecure_url.len());
            println!("Duplicate groups: {}", report.duplicates.len());
            println!("Expiring: {}", report.expiring.len());
        }
        Command::Import { vault, file } => {
            let mut opened = open(&vault)?;
            let data = read_file(&file)?;
            let format = io::detect(&data).ok_or("the format was not recognised")?;
            let source_password = if io::needs_password(&data) {
                Some(password("Password of the file being imported: ")?)
            } else {
                None
            };
            let at = opened.vault.tick(now);
            let imported = io::import(
                format,
                &data,
                source_password.as_ref().map(Secret::expose),
                at,
            )
            .map_err(|e| e.to_string())?;
            let skipped = imported.skipped.clone();
            let report = io::apply(&mut opened.vault, imported, now).map_err(|e| e.to_string())?;
            let blob_dir = vault.with_extension("blobs");
            if !report.blobs.is_empty() {
                fs::create_dir_all(&blob_dir).map_err(|e| e.to_string())?;
                for (name, bytes) in &report.blobs {
                    fs::write(blob_dir.join(name), bytes).map_err(|e| e.to_string())?;
                }
            }
            save(&mut opened)?;
            println!(
                "{}: {} added, {} already present.",
                format.name(),
                report.added,
                report.duplicates
            );
            for note in skipped {
                println!("Note: {note}");
            }
        }
        Command::Export { vault, format, out } => {
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
                    let secret = new_password("Password for the export: ")?;
                    let entries: Vec<_> = opened
                        .vault
                        .doc
                        .entries
                        .values()
                        .filter(|e| !e.is_trashed())
                        .collect();
                    io::kdbx::export(&entries, secret.expose()).map_err(|e| e.to_string())?
                }
                other => return Err(format!("unknown format {other}")),
            };
            write_atomic(&out, &bytes)?;
            if format != "kdbx" {
                eprintln!(
                    "{} holds your secrets in the clear. Delete it when you are done.",
                    out.display()
                );
            }
            println!("Wrote {}.", out.display());
        }
        Command::Duress { vault, action } => {
            let action = match action.as_str() {
                "wipe" => SlotAction::WipeLocal,
                "lock" => SlotAction::HardLock,
                other => return Err(format!("unknown action {other}; use wipe or lock")),
            };
            let mut opened = open(&vault)?;
            let secret = new_password("Duress password: ")?;
            opened
                .vault
                .add_slot(secret.expose().as_bytes(), "Duress", action, None, now)
                .map_err(|e| e.to_string())?;
            save(&mut opened)?;
            println!("Added.");
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
            println!(
                "Any {threshold} of these {total} shares open this vault. Give each person one."
            );
            println!(
                "The waiting period is advice only: people holding enough shares can act at once."
            );
            for share in shares {
                println!("{}: {}", share.number, share.text);
            }
        }
        Command::Recover { vault, shares } => {
            let file = read_file(&vault)?;
            let texts: Vec<&str> = shares.iter().map(String::as_str).collect();
            let opened = emergency::recover(&file, &texts, device()).map_err(|e| e.to_string())?;
            println!(
                "Opened {}. {} entries:",
                opened.doc.name,
                opened.doc.entries.len()
            );
            for entry in opened.doc.entries.values() {
                println!("{}", login_line(entry));
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
            println!("Merged. {} entries.", opened.vault.doc.entries.len());
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
    let _ = <Cli as clap::CommandFactory>::command().print_help();
    Err(String::new())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{message}");
            }
            ExitCode::FAILURE
        }
    }
}
