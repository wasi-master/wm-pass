#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use super::*;
use crate::http::block_on;
use crate::ssh::fake::{self, Options};

/// What the fake server holds, shared by every connection to it.
#[derive(Default)]
struct Fs {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
    /// Paths a write to is refused.
    denied: BTreeSet<String>,
    /// Longest READ answer; real servers answer short at will.
    max_read: usize,
    posix_rename: bool,
    /// Requests by name, in order.
    log: Vec<&'static str>,
}

fn shared(posix_rename: bool, max_read: usize) -> Arc<Mutex<Fs>> {
    Arc::new(Mutex::new(Fs { posix_rename, max_read, ..Fs::default() }))
}

fn clean(path: &str) -> String {
    let path = path.strip_prefix("./").unwrap_or(path);
    if path == "." { String::new() } else { path.to_owned() }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// An SFTP v3 server over an in-memory tree, byte for byte.
struct FakeSftp {
    fs: Arc<Mutex<Fs>>,
    input: Vec<u8>,
    handles: HashMap<Vec<u8>, (String, bool)>,
    next: u32,
}

impl FakeSftp {
    fn new(fs: Arc<Mutex<Fs>>) -> Self {
        Self { fs, input: Vec::new(), handles: HashMap::new(), next: 0 }
    }

    fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        self.input.extend_from_slice(data);
        let mut out = Vec::new();
        while self.input.len() >= 4 {
            let length = u32::from_be_bytes(self.input[..4].try_into().unwrap()) as usize;
            if self.input.len() < 4 + length {
                break;
            }
            let packet: Vec<u8> = self.input.drain(..4 + length).skip(4).collect();
            let reply = self.handle(&packet);
            out.extend_from_slice(&(reply.len() as u32).to_be_bytes());
            out.extend_from_slice(&reply);
        }
        out
    }

    fn handle(&mut self, packet: &[u8]) -> Vec<u8> {
        let kind = packet[0];
        let mut p = Parse::new(&packet[1..]);
        if kind == FXP_INIT {
            assert_eq!(p.u32().unwrap(), 3);
            let mut reply = Wire::msg(FXP_VERSION).u32(3);
            if self.fs.lock().unwrap().posix_rename {
                reply = reply.text(POSIX_RENAME).text("1");
            }
            return reply.0;
        }
        let id = p.u32().unwrap();
        let status = |code: u32| Wire::msg(FXP_STATUS).u32(id).u32(code).text("").text("").0;
        let mut fs = self.fs.lock().unwrap();
        let attrs = |w: Wire, size: u64, dir: bool| {
            w.u32(ATTR_SIZE | ATTR_PERMISSIONS | ATTR_ACMODTIME).u64(size).u32(if dir { 0o40755 } else { 0o100644 }).u32(0).u32(0)
        };
        match kind {
            FXP_STAT => {
                fs.log.push("stat");
                let path = clean(p.text().unwrap());
                if path.is_empty() || fs.dirs.contains(&path) {
                    attrs(Wire::msg(FXP_ATTRS).u32(id), 0, true).0
                } else if let Some(data) = fs.files.get(&path) {
                    attrs(Wire::msg(FXP_ATTRS).u32(id), data.len() as u64, false).0
                } else {
                    status(FX_NO_SUCH_FILE)
                }
            }
            FXP_MKDIR => {
                fs.log.push("mkdir");
                let path = clean(p.text().unwrap());
                let up = parent(&path).to_owned();
                if !up.is_empty() && !fs.dirs.contains(&up) {
                    return status(FX_NO_SUCH_FILE);
                }
                fs.dirs.insert(path);
                status(FX_OK)
            }
            FXP_OPEN => {
                fs.log.push("open");
                let path = clean(p.text().unwrap());
                let flags = p.u32().unwrap();
                if flags & FXF_WRITE != 0 {
                    if fs.denied.contains(&path) {
                        return status(FX_PERMISSION_DENIED);
                    }
                    let up = parent(&path).to_owned();
                    if !up.is_empty() && !fs.dirs.contains(&up) {
                        return status(FX_NO_SUCH_FILE);
                    }
                    fs.files.insert(path.clone(), Vec::new());
                } else if !fs.files.contains_key(&path) {
                    return status(FX_NO_SUCH_FILE);
                }
                self.next += 1;
                let handle = format!("h{}", self.next).into_bytes();
                self.handles.insert(handle.clone(), (path, false));
                Wire::msg(FXP_HANDLE).u32(id).bytes(&handle).0
            }
            FXP_WRITE => {
                fs.log.push("write");
                let (path, _) = self.handles[p.bytes().unwrap()].clone();
                let offset = p.u64().unwrap() as usize;
                let data = p.bytes().unwrap();
                let file = fs.files.get_mut(&path).unwrap();
                if file.len() < offset + data.len() {
                    file.resize(offset + data.len(), 0);
                }
                file[offset..offset + data.len()].copy_from_slice(data);
                status(FX_OK)
            }
            FXP_READ => {
                fs.log.push("read");
                let (path, _) = self.handles[p.bytes().unwrap()].clone();
                let offset = p.u64().unwrap() as usize;
                let wanted = (p.u32().unwrap() as usize).min(fs.max_read);
                let file = &fs.files[&path];
                if offset >= file.len() {
                    return status(FX_EOF);
                }
                let end = file.len().min(offset + wanted);
                Wire::msg(FXP_DATA).u32(id).bytes(&file[offset..end]).0
            }
            FXP_CLOSE => {
                fs.log.push("close");
                self.handles.remove(p.bytes().unwrap());
                status(FX_OK)
            }
            FXP_OPENDIR => {
                fs.log.push("opendir");
                let path = clean(p.text().unwrap());
                if !path.is_empty() && !fs.dirs.contains(&path) {
                    return status(FX_NO_SUCH_FILE);
                }
                self.next += 1;
                let handle = format!("d{}", self.next).into_bytes();
                self.handles.insert(handle.clone(), (path, false));
                Wire::msg(FXP_HANDLE).u32(id).bytes(&handle).0
            }
            FXP_READDIR => {
                fs.log.push("readdir");
                let key = p.bytes().unwrap().to_vec();
                let (dir, listed) = self.handles[&key].clone();
                if listed {
                    return status(FX_EOF);
                }
                self.handles.insert(key, (dir.clone(), true));
                let files: Vec<(String, u64)> = fs
                    .files
                    .iter()
                    .filter(|(path, _)| parent(path) == dir)
                    .map(|(path, data)| (path.rsplit('/').next().unwrap().to_owned(), data.len() as u64))
                    .collect();
                let dirs: Vec<String> = fs
                    .dirs
                    .iter()
                    .filter(|path| parent(path) == dir)
                    .map(|path| path.rsplit('/').next().unwrap().to_owned())
                    .collect();
                let mut reply = Wire::msg(FXP_NAME).u32(id).u32((files.len() + dirs.len() + 2) as u32);
                for name in [".", ".."] {
                    reply = attrs(reply.text(name).text(name), 0, true);
                }
                for (name, size) in files {
                    reply = attrs(reply.text(&name).text("-rw-r--r-- long name"), size, false);
                }
                for name in dirs {
                    reply = attrs(reply.text(&name).text("drwxr-xr-x"), 0, true);
                }
                reply.0
            }
            FXP_REMOVE => {
                fs.log.push("remove");
                let path = clean(p.text().unwrap());
                if fs.files.remove(&path).is_some() { status(FX_OK) } else { status(FX_NO_SUCH_FILE) }
            }
            FXP_RENAME | FXP_EXTENDED => {
                let posix = kind == FXP_EXTENDED;
                if posix {
                    assert_eq!(p.text().unwrap(), POSIX_RENAME);
                }
                fs.log.push(if posix { "posix-rename" } else { "rename" });
                let from = clean(p.text().unwrap());
                let to = clean(p.text().unwrap());
                if !posix && fs.files.contains_key(&to) {
                    // Plain v3 refuses to replace.
                    return status(4);
                }
                match fs.files.remove(&from) {
                    Some(data) => {
                        fs.files.insert(to, data);
                        status(FX_OK)
                    }
                    None => status(FX_NO_SUCH_FILE),
                }
            }
            _ => status(8),
        }
    }
}

/// The fake as a [`Pipe`], with no SSH under it.
struct Direct {
    server: FakeSftp,
    out: std::collections::VecDeque<u8>,
}

impl Pipe for Direct {
    fn send(&mut self, data: &[u8]) -> Result<()> {
        let reply = self.server.feed(data);
        self.out.extend(reply);
        Ok(())
    }

    fn recv(&mut self, buf: &mut [u8]) -> Result<()> {
        if self.out.len() < buf.len() {
            return Err(Fault::Io);
        }
        let wanted = buf.len();
        for (slot, byte) in buf.iter_mut().zip(self.out.drain(..wanted)) {
            *slot = byte;
        }
        Ok(())
    }
}

#[test]
fn client_pipelines_and_copes_with_short_reads() {
    let fs = shared(false, 10_000);
    let mut client = SftpClient::new(Direct { server: FakeSftp::new(fs.clone()), out: Default::default() });
    client.init().unwrap();
    assert!(client.extensions.is_empty());
    let data: Vec<u8> = (0..(CHUNK * 20 + 123)).map(|i| (i * 7 % 256) as u8).collect();
    client.put("big", &data).unwrap();
    assert_eq!(client.get("big").unwrap(), data);
    assert_eq!(client.get("missing"), Err(Failure::Status(FX_NO_SUCH_FILE)));
    client.put("empty", b"").unwrap();
    assert_eq!(client.get("empty").unwrap(), b"");
    // Without posix-rename the target goes first, then a plain rename.
    client.put("big.part", b"new").unwrap();
    client.rename_replacing("big.part", "big").unwrap();
    assert_eq!(client.get("big").unwrap(), b"new");
    let log = fs.lock().unwrap().log.clone();
    assert!(log.windows(2).any(|pair| pair == ["remove", "rename"]), "{log:?}");
    let writes = log.iter().filter(|e| **e == "write").count();
    assert_eq!(writes, 21 + 1);
    let names: Vec<String> = client.list(".").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["big", "empty"], "dot entries are dropped");
    assert_eq!(client.stat("nowhere").unwrap(), None);
    assert_eq!(client.remove("nowhere"), Err(Failure::Status(FX_NO_SUCH_FILE)));
}

#[test]
fn statuses_mean_what_kotlin_said() {
    assert_eq!(fault_of(Failure::Status(FX_PERMISSION_DENIED)), Fault::PermissionLost);
    assert_eq!(fault_of(Failure::Status(FX_NO_SUCH_FILE)), Fault::TargetMissing);
    assert_eq!(fault_of(Failure::Status(4)), Fault::Io);
    assert_eq!(fault_of(Failure::Broken(Fault::Unsafe)), Fault::Unsafe);
}

fn sink(port: u16, path: &str, host_key: &str, seen: Arc<Mutex<Vec<String>>>) -> Sftp {
    let config = SftpConfig {
        host: "127.0.0.1".into(),
        port,
        user: "wm".into(),
        password: "right".into(),
        path: path.into(),
        host_key: host_key.into(),
        ..SftpConfig::default()
    };
    Sftp::new(NetPolicy::Any, config, Box::new(move |key| seen.lock().unwrap().push(key)))
}

fn serve(fs: &Arc<Mutex<Fs>>) -> u16 {
    let fs = fs.clone();
    fake::spawn(
        Options::default(),
        Arc::new(move || {
            let mut server = FakeSftp::new(fs.clone());
            Box::new(move |data: &[u8]| server.feed(data))
        }),
    )
}

#[test]
fn sink_round_trip_over_ssh() {
    let fs = shared(true, usize::MAX);
    let port = serve(&fs);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sftp = sink(port, "backups/wm/", "", seen.clone());
    block_on(sftp.readiness()).unwrap();
    assert!(fs.lock().unwrap().dirs.contains("backups/wm"), "made a level at a time");
    let stored = block_on(sftp.write("v.wmvault", b"one")).unwrap();
    block_on(sftp.write("v.wmvault", b"two")).unwrap();
    fs.lock().unwrap().files.insert("backups/wm/left.wmvault.part".into(), b"half".to_vec());
    fs.lock().unwrap().dirs.insert("backups/wm/sub".into());
    let listed = block_on(sftp.list()).unwrap();
    assert_eq!(listed, vec![Stored { id: "v.wmvault".into(), name: "v.wmvault".into(), size: 3 }]);
    assert_eq!(block_on(sftp.read(&stored)).unwrap(), b"two");
    assert!(fs.lock().unwrap().log.contains(&"posix-rename"));
    block_on(sftp.delete(&stored)).unwrap();
    block_on(sftp.delete(&stored)).unwrap();
    assert!(!fs.lock().unwrap().files.contains_key("backups/wm/v.wmvault"));

    let keys = seen.lock().unwrap().clone();
    assert!(keys.len() >= 5 && keys.iter().all(|k| k == &keys[0] && k.starts_with("ssh-ed25519 ")));
    // Pinned: the callback stays quiet, and a different key is refused.
    let quiet = Arc::new(Mutex::new(Vec::new()));
    block_on(sink(port, "backups/wm", &keys[0], quiet.clone()).readiness()).unwrap();
    assert!(quiet.lock().unwrap().is_empty());
    let other = format!("ssh-ed25519 {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, fake::host_blob(&[1; 32])));
    assert_eq!(block_on(sink(port, "backups/wm", &other, quiet).readiness()), Err(Fault::Unsafe));
}

#[test]
fn sink_failures_are_named_and_leave_nothing_behind() {
    let fs = shared(true, usize::MAX);
    let port = serve(&fs);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sftp = sink(port, "/abs", "", seen.clone());
    // The folder is missing and this verb does not make it.
    assert_eq!(block_on(sftp.write("v", b"x")), Err(Fault::TargetMissing));
    block_on(sftp.readiness()).unwrap();
    fs.lock().unwrap().denied.insert("/abs/v.part".into());
    assert_eq!(block_on(sftp.write("v", b"x")), Err(Fault::PermissionLost));
    assert!(fs.lock().unwrap().files.is_empty());
    let missing = Stored { id: "gone".into(), name: "gone".into(), size: 0 };
    assert_eq!(block_on(sftp.read(&missing)), Err(Fault::TargetMissing));
    for bad in ["../x", "a/b", "..", ""] {
        assert_eq!(block_on(sftp.write(bad, b"x")).err(), Some(Fault::NotConfigured), "{bad}");
    }
    let unset = Sftp::new(NetPolicy::Any, SftpConfig::default(), Box::new(|_| {}));
    assert_eq!(block_on(unset.readiness()), Err(Fault::NotConfigured));
    let offline = Sftp::new(NetPolicy::Offline, sink(port, "", "", seen).config.clone(), Box::new(|_| {}));
    assert_eq!(block_on(offline.readiness()), Err(Fault::Blocked));
}

#[test]
fn config_debug_names_no_secret() {
    let config = SftpConfig {
        host: "nas.example".into(),
        user: "me".into(),
        password: "hunter2".into(),
        private_key: "KEY".into(),
        path: "/srv/backups".into(),
        host_key: "ssh-ed25519 AAAA".into(),
        ..SftpConfig::default()
    };
    assert!(config.configured());
    let shown = format!("{config:?} {:?}", Sftp::new(NetPolicy::Any, config.clone(), Box::new(|_| {})));
    for secret in ["hunter2", "KEY", "nas.example", "/srv/backups", "AAAA", "me"] {
        assert!(!shown.contains(secret), "{shown}");
    }
    assert!(!SftpConfig { password: Secret::default(), private_key: Secret::default(), ..config }.configured());
}

// ---- Against real servers ----

fn exercise(sink: &Sftp) {
    block_on(sink.readiness()).unwrap();
    let big: Vec<u8> = (0..3 * 1024 * 1024 + 17).map(|i: u32| (i % 253) as u8).collect();
    let stored = block_on(sink.write("a file.bin", &big)).unwrap();
    block_on(sink.write("second.bin", b"hello")).unwrap();
    block_on(sink.write("second.bin", b"hello again")).unwrap();
    let listed = block_on(sink.list()).unwrap();
    let names: Vec<&str> = listed.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"a file.bin") && names.contains(&"second.bin"), "{names:?}");
    let found = listed.iter().find(|f| f.name == "second.bin").unwrap();
    assert_eq!(found.size, 11);
    assert_eq!(block_on(sink.read(found)).unwrap(), b"hello again");
    assert_eq!(block_on(sink.read(&stored)).unwrap(), big);
    block_on(sink.delete(found)).unwrap();
    block_on(sink.delete(found)).unwrap();
    block_on(sink.delete(&stored)).unwrap();
    assert!(block_on(sink.list()).unwrap().is_empty());
}

/// The round trip, then: the key it saw lets a pinned sink in, and a different key is refused.
fn host_key_pinning(config: SftpConfig) -> String {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();
    exercise(&Sftp::new(NetPolicy::Any, config.clone(), Box::new(move |key| recorder.lock().unwrap().push(key))));
    let key = seen.lock().unwrap().first().cloned().unwrap();
    assert!(key.contains(' '), "no host key recorded");
    let pinned = SftpConfig { host_key: key.clone(), ..config.clone() };
    block_on(Sftp::new(NetPolicy::Any, pinned, Box::new(|_| panic!("pinned"))).readiness()).unwrap();
    let wrong = format!("{} AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl", key.split(' ').next().unwrap());
    let refused = SftpConfig { host_key: wrong, ..config };
    assert_eq!(block_on(Sftp::new(NetPolicy::Any, refused, Box::new(|_| {})).readiness()), Err(Fault::Unsafe));
    key
}

/// `WMPASS_LIVE_SSHD=port:user:keyfile[:passphrase]` against an OpenSSH `sshd`, which writes
/// under a fresh temporary folder. Optional `WMPASS_LIVE_SSHD_LEGACY=1`.
#[test]
#[ignore = "needs WMPASS_LIVE_SSHD"]
fn sftp_against_openssh() {
    let spec = std::env::var("WMPASS_LIVE_SSHD").expect("WMPASS_LIVE_SSHD=port:user:keyfile[:passphrase]");
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    let dir = tempfile::tempdir().unwrap();
    let config = SftpConfig {
        host: "127.0.0.1".into(),
        port: parts[0].parse().unwrap(),
        user: parts[1].into(),
        private_key: std::fs::read_to_string(parts[2]).unwrap().into(),
        key_passphrase: parts.get(3).copied().unwrap_or_default().into(),
        path: dir.path().join("backups/wm").to_str().unwrap().to_owned(),
        legacy_algorithms: std::env::var("WMPASS_LIVE_SSHD_LEGACY").is_ok(),
        ..SftpConfig::default()
    };
    let key = host_key_pinning(config);
    println!("host key {key}");
}

/// Against `rclone serve sftp` (Go's SSH server), with a password.
#[test]
#[ignore = "needs rclone"]
fn sftp_against_rclone() {
    let dir = tempfile::tempdir().unwrap();
    let port = 18_393;
    let mut child = std::process::Command::new("rclone")
        .args(["serve", "sftp", dir.path().to_str().unwrap(), "--addr", "127.0.0.1:18393", "--user", "wm", "--pass", "secret"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let config = SftpConfig {
        host: "127.0.0.1".into(),
        port,
        user: "wm".into(),
        password: "secret".into(),
        path: "backups/wm".into(),
        ..SftpConfig::default()
    };
    let result = std::panic::catch_unwind(|| {
        host_key_pinning(config.clone());
        let wrong = SftpConfig { password: "nope".into(), ..config };
        assert_eq!(block_on(Sftp::new(NetPolicy::Any, wrong, Box::new(|_| {})).readiness()), Err(Fault::PermissionLost));
    });
    let _ = child.kill();
    let _ = child.wait();
    result.unwrap();
}
