#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::*;
use crate::http::block_on;

#[test]
fn xdr_pads_to_four_bytes_and_reads_back() {
    // The vector from WM Keyboard's CloudCryptoTest.
    let bytes = Xdr::default().int((-2i32) as u32).long(1 << 40).bool(true).string("abcde").opaque(&[1, 2]).0;
    assert_eq!(bytes.len(), 4 + 8 + 4 + (4 + 8) + (4 + 4));
    let mut r = XdrReader::new(&bytes);
    assert_eq!(r.int().unwrap() as i32, -2);
    assert_eq!(r.long().unwrap(), 1 << 40);
    assert!(r.bool().unwrap());
    assert_eq!(r.string().unwrap(), "abcde");
    assert_eq!(r.opaque().unwrap(), [1, 2]);
    assert_eq!(r.int(), Err(Fault::Io));
}

#[test]
fn failures_mean_what_kotlin_said() {
    for (failure, fault) in [
        (Failure::Denied, Fault::PermissionLost),
        (Failure::Unavailable, Fault::TargetMissing),
        (Failure::Status(NFS3ERR_ACCES), Fault::PermissionLost),
        (Failure::Status(NFS3ERR_ROFS), Fault::PermissionLost),
        (Failure::Status(NFS3ERR_STALE), Fault::TargetMissing),
        (Failure::Status(NFS3ERR_DQUOT), Fault::OutOfSpace),
        (Failure::Status(NFS3ERR_FBIG), Fault::OutOfSpace),
        (Failure::Status(10_006), Fault::Io),
        (Failure::Broken(Fault::Io), Fault::Io),
    ] {
        assert_eq!(fault_of(failure), fault);
    }
}

#[test]
fn auth_sys_says_who_we_are() {
    let credential = auth_sys(501, 20);
    let mut r = XdrReader::new(&credential);
    assert_eq!(r.int().unwrap(), AUTH_SYS);
    let body = r.opaque().unwrap();
    let mut b = XdrReader::new(body);
    b.int().unwrap();
    assert_eq!(b.string().unwrap(), "wmpass");
    assert_eq!((b.int().unwrap(), b.int().unwrap(), b.int().unwrap(), b.int().unwrap()), (501, 20, 1, 20));
}

// ---- A scripted server: portmap, MOUNT and NFS on one port ----

#[derive(Default)]
struct Server {
    /// Path to contents; `None` is a directory. The export's top is "".
    nodes: BTreeMap<String, Option<Vec<u8>>>,
    deny_mount: bool,
    /// What the portmapper says MOUNT is on; `None` is its own port.
    mount_at: Option<u32>,
    uids: Vec<u32>,
    calls: Vec<(u32, u32)>,
}

fn handle_of(path: &str) -> Vec<u8> {
    format!("fh:{path}").into_bytes()
}

fn path_of(handle: &[u8]) -> String {
    String::from_utf8(handle.to_vec()).unwrap().strip_prefix("fh:").unwrap().to_owned()
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_owned() } else { format!("{dir}/{name}") }
}

fn fattr(w: Xdr, directory: bool, size: u64) -> Xdr {
    let w = w.int(if directory { TYPE_DIR } else { 1 }).int(0o644).int(1).int(1000).int(1000).long(size).long(size);
    w.long(0).long(1).long(2).int(1).int(0).int(1_700_000_000).int(5_000_000).int(1).int(0)
}

fn serve(state: Arc<Mutex<Server>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let state = state.clone();
            std::thread::spawn(move || connection(stream, &state, port));
        }
    });
    port
}

fn connection(mut stream: std::net::TcpStream, state: &Mutex<Server>, own_port: u16) {
    loop {
        let mut mark = [0u8; 4];
        if stream.read_exact(&mut mark).is_err() {
            return;
        }
        let length = (u32::from_be_bytes(mark) & !LAST_FRAGMENT) as usize;
        let mut record = vec![0u8; length];
        stream.read_exact(&mut record).unwrap();
        let mut r = XdrReader::new(&record);
        let xid = r.int().unwrap();
        assert_eq!((r.int().unwrap(), r.int().unwrap()), (MSG_CALL, RPC_VERSION));
        let (program, version, procedure) = (r.int().unwrap(), r.int().unwrap(), r.int().unwrap());
        let flavour = r.int().unwrap();
        let credential = r.opaque().unwrap().to_vec();
        r.int().unwrap();
        r.opaque().unwrap();
        let mut s = state.lock().unwrap();
        s.calls.push((program, procedure));
        if flavour == AUTH_SYS {
            let mut c = XdrReader::new(&credential);
            c.int().unwrap();
            c.string().unwrap();
            let uid = c.int().unwrap();
            s.uids.push(uid);
        }
        let reply = Xdr::default().int(xid).int(MSG_REPLY);
        let accepted = |results: Xdr| reply_ok(xid, results);
        let out = match (program, procedure) {
            (PORTMAP_PROGRAM, PORTMAP_GETPORT) => {
                let wanted = r.int().unwrap();
                let port = if wanted == MOUNT_PROGRAM { s.mount_at.unwrap_or(u32::from(own_port)) } else { u32::from(own_port) };
                accepted(Xdr::default().int(port))
            }
            (MOUNT_PROGRAM, MOUNT_MNT) if s.deny_mount => reply.int(1).int(REJECT_AUTH_ERROR).int(5),
            (MOUNT_PROGRAM, MOUNT_MNT) => {
                if r.string().unwrap() == "/export" {
                    accepted(Xdr::default().int(NFS_OK).opaque(&handle_of("")).int(1).int(AUTH_SYS))
                } else {
                    accepted(Xdr::default().int(NFS3ERR_NOENT))
                }
            }
            (MOUNT_PROGRAM, MOUNT_UMNT) => accepted(Xdr::default()),
            (NFS_PROGRAM, _) if version != NFS_VERSION => reply.int(MSG_ACCEPTED).int(0).int(0).int(PROG_MISMATCH),
            (NFS_PROGRAM, procedure) => accepted(nfs(&mut s, procedure, &mut r)),
            _ => reply.int(MSG_ACCEPTED).int(0).int(0).int(PROG_UNAVAIL),
        };
        drop(s);
        let mut framed = (LAST_FRAGMENT | out.0.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(&out.0);
        stream.write_all(&framed).unwrap();
    }
}

fn reply_ok(xid: u32, results: Xdr) -> Xdr {
    Xdr::default().int(xid).int(MSG_REPLY).int(MSG_ACCEPTED).int(0).int(0).int(ACCEPT_SUCCESS).fixed(&results.0)
}

fn nfs(s: &mut Server, procedure: u32, r: &mut XdrReader<'_>) -> Xdr {
    let ok = Xdr::default().int(NFS_OK);
    match procedure {
        PROC_FSINFO => ok.bool(false).int(8192).int(4096).int(4096).int(8192).int(4096).int(4096).int(4096).long(1 << 40).int(0).int(1).int(0),
        PROC_LOOKUP => {
            let path = join(&path_of(r.opaque().unwrap()), &r.string().unwrap());
            if s.nodes.contains_key(&path) {
                ok.opaque(&handle_of(&path)).bool(false).bool(false)
            } else {
                Xdr::default().int(NFS3ERR_NOENT).bool(false)
            }
        }
        PROC_MKDIR => {
            let path = join(&path_of(r.opaque().unwrap()), &r.string().unwrap());
            s.nodes.insert(path.clone(), None);
            ok.bool(true).opaque(&handle_of(&path)).bool(false).bool(false).bool(false)
        }
        PROC_CREATE => {
            let dir = path_of(r.opaque().unwrap());
            let path = join(&dir, &r.string().unwrap());
            if dir.contains("readonly") {
                return Xdr::default().int(NFS3ERR_ROFS).bool(false).bool(false);
            }
            s.nodes.insert(path, Some(Vec::new()));
            // No handle: the client has to look it up.
            ok.bool(false).bool(false).bool(false).bool(false)
        }
        PROC_WRITE => {
            let path = path_of(r.opaque().unwrap());
            let offset = r.long().unwrap() as usize;
            r.int().unwrap();
            assert_eq!(r.int().unwrap(), FILE_SYNC);
            let data = r.opaque().unwrap();
            assert!(data.len() <= 4096, "FSINFO's preferred size is used");
            // Writes at most 3000 bytes, so the client must go on from there.
            let taken = &data[..data.len().min(3000)];
            let Some(Some(file)) = s.nodes.get_mut(&path) else { panic!("no file") };
            file.resize(file.len().max(offset + taken.len()), 0);
            file[offset..offset + taken.len()].copy_from_slice(taken);
            ok.bool(true).fixed(&[0; 24]).bool(false).int(taken.len() as u32).int(FILE_SYNC).fixed(&[0; 8])
        }
        PROC_READ => {
            let path = path_of(r.opaque().unwrap());
            let offset = r.long().unwrap() as usize;
            let count = (r.int().unwrap() as usize).min(5000);
            let Some(Some(file)) = s.nodes.get(&path) else { return Xdr::default().int(NFS3ERR_NOENT).bool(false) };
            let end = file.len().min(offset + count);
            let data = &file[offset.min(end)..end];
            ok.bool(true).fixed(&[0; FATTR_BYTES]).int(data.len() as u32).bool(end == file.len()).opaque(data)
        }
        PROC_REMOVE => {
            let path = join(&path_of(r.opaque().unwrap()), &r.string().unwrap());
            let status = if s.nodes.remove(&path).is_some() { NFS_OK } else { NFS3ERR_NOENT };
            Xdr::default().int(status).bool(false).bool(false)
        }
        PROC_RENAME => {
            let from = join(&path_of(r.opaque().unwrap()), &r.string().unwrap());
            let to = join(&path_of(r.opaque().unwrap()), &r.string().unwrap());
            let node = s.nodes.remove(&from).unwrap();
            s.nodes.insert(to, node);
            ok.bool(false).bool(false).bool(false).bool(false)
        }
        PROC_READDIRPLUS => {
            let dir = path_of(r.opaque().unwrap());
            let cookie = r.long().unwrap() as usize;
            let mut entries: Vec<(String, bool, u64)> = vec![(".".into(), true, 0), ("..".into(), true, 0)];
            for (path, node) in &s.nodes {
                if let Some((parent, name)) = path.rsplit_once('/').or(Some(("", path.as_str())))
                    && parent == dir
                    && !path.is_empty()
                {
                    entries.push((name.to_owned(), node.is_none(), node.as_ref().map_or(0, |d| d.len() as u64)));
                }
            }
            // Two at a time, so the client has to page with the cookie.
            let mut out = ok.bool(false).fixed(&[3; VERIFIER_BYTES]);
            let page: Vec<_> = entries.iter().enumerate().skip(cookie).take(2).collect();
            for (i, (name, directory, size)) in &page {
                out = out.bool(true).long(*i as u64 + 100).string(name).long(*i as u64 + 1).bool(true);
                out = fattr(out, *directory, *size).bool(true).opaque(&handle_of(name));
            }
            out.bool(false).bool(cookie + page.len() >= entries.len())
        }
        _ => Xdr::default().int(10_004),
    }
}

fn sink(port: u16, portmap: u16, path: &str, fixed_ports: bool) -> Nfs {
    let config = NfsConfig {
        host: "127.0.0.1".into(),
        export: " /export ".into(),
        path: path.into(),
        uid: 501,
        gid: 20,
        port: if fixed_ports { port } else { 0 },
        mount_port: if fixed_ports { port } else { 0 },
    };
    Nfs { policy: NetPolicy::Any, config, portmap_port: portmap }
}

#[test]
fn round_trip_with_fixed_ports() {
    let state = Arc::new(Mutex::new(Server::default()));
    let port = serve(state.clone());
    let nfs = sink(port, 1, "a\\b/", true);
    block_on(nfs.readiness()).unwrap();
    assert_eq!(state.lock().unwrap().nodes.get("a/b"), Some(&None), "made a level at a time");
    let big: Vec<u8> = (0..20_000u32).map(|i| (i % 249) as u8).collect();
    let stored = block_on(nfs.write("v.wmvault", &big)).unwrap();
    block_on(nfs.write("w.wmvault", b"small")).unwrap();
    {
        let mut s = state.lock().unwrap();
        s.nodes.insert("a/b/half.part".into(), Some(b"x".to_vec()));
        s.nodes.insert("a/b/sub".into(), None);
        assert!(!s.nodes.contains_key("a/b/v.wmvault.part"));
    }
    let listed = block_on(nfs.list()).unwrap();
    let names: Vec<(&str, u64)> = listed.iter().map(|f| (f.name.as_str(), f.size)).collect();
    assert_eq!(names, [("v.wmvault", 20_000), ("w.wmvault", 5)]);
    assert_eq!(block_on(nfs.read(&stored)).unwrap(), big);
    block_on(nfs.delete(&stored)).unwrap();
    block_on(nfs.delete(&stored)).unwrap();
    assert_eq!(block_on(nfs.read(&stored)), Err(Fault::TargetMissing));
    let s = state.lock().unwrap();
    assert!(s.uids.iter().all(|uid| *uid == 501) && !s.uids.is_empty());
    assert!(!s.calls.iter().any(|(program, _)| *program == PORTMAP_PROGRAM), "fixed ports skip the portmapper");
    assert!(s.calls.contains(&(MOUNT_PROGRAM, MOUNT_UMNT)), "every session unmounts");
}

#[test]
fn ports_come_from_the_portmapper() {
    let state = Arc::new(Mutex::new(Server::default()));
    let port = serve(state.clone());
    let nfs = sink(port, port, "", false);
    block_on(nfs.write("v", b"data")).unwrap();
    assert_eq!(block_on(nfs.list()).unwrap().len(), 1);
    assert!(state.lock().unwrap().calls.contains(&(PORTMAP_PROGRAM, PORTMAP_GETPORT)));
    // A portmapper with no mount service.
    state.lock().unwrap().mount_at = Some(0);
    assert_eq!(block_on(nfs.readiness()), Err(Fault::TargetMissing));
}

#[test]
fn refusals_are_named() {
    let state = Arc::new(Mutex::new(Server::default()));
    let port = serve(state.clone());
    let wrong_export = Nfs { config: NfsConfig { export: "/elsewhere".into(), ..sink(port, 1, "", true).config }, ..sink(port, 1, "", true) };
    assert_eq!(block_on(wrong_export.readiness()), Err(Fault::TargetMissing));
    let readonly = sink(port, 1, "readonly", true);
    assert_eq!(block_on(readonly.write("v", b"x")), Err(Fault::PermissionLost));
    for bad in ["../x", "a/b", "..", ""] {
        assert_eq!(block_on(readonly.write(bad, b"x")).err(), Some(Fault::NotConfigured), "{bad}");
    }
    state.lock().unwrap().deny_mount = true;
    assert_eq!(block_on(sink(port, 1, "", true).readiness()), Err(Fault::PermissionLost));
    let offline = Nfs::new(NetPolicy::Offline, sink(port, 1, "", true).config);
    assert_eq!(block_on(offline.readiness()), Err(Fault::Blocked));
    assert_eq!(block_on(Nfs::new(NetPolicy::Any, NfsConfig::default()).readiness()), Err(Fault::NotConfigured));
    let shown = format!("{:?}", sink(port, 1, "secret/path", true));
    assert!(!shown.contains("127.0.0.1") && !shown.contains("export") && !shown.contains("secret"), "{shown}");
}
