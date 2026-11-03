//! Two real ends over localhost TCP.
#![allow(missing_docs, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use wmpass_core::crypto::KdfParams;
use wmpass_core::format::{KdfConfig, SlotAction, VaultKey};
use wmpass_core::model::{Kind, Lww, Note};
use wmpass_core::vault::{Vault, login_entry};
use wmpass_core::Secret;
use wmpass_lan::channel::Channel;
use wmpass_lan::pair::{PairingQr, begin_code_pairing, short_code};
use wmpass_lan::share::{ShareInvite, ShareReceiver, seal_to_offer};
use wmpass_lan::sync::{BlobStore, NoBlobs, sync};
use wmpass_lan::{Identity, LanError, Peer, Role, prepare};

fn linked() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client = TcpStream::connect(address).unwrap();
    let (server, _) = listener.accept().unwrap();
    prepare(&client).unwrap();
    prepare(&server).unwrap();
    (client, server)
}

fn config() -> KdfConfig {
    KdfConfig::new(KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }).unwrap()
}

fn vault_pair() -> (Vault, Vault) {
    let mut a = Vault::create("V", b"pw", &config(), 1, 1000).unwrap();
    let file = a.save().unwrap();
    let key = VaultKey::from_bytes(*a.key().expose_for_hardware_wrap());
    let b = Vault::from_file(&file, key, 2).unwrap();
    (a, b)
}

fn peers() -> (Identity, Identity, Vec<Peer>, Vec<Peer>) {
    let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let trusted_by_a = vec![Peer { key: *b.public(), name: "B".into() }];
    let trusted_by_b = vec![Peer { key: *a.public(), name: "A".into() }];
    (a, b, trusted_by_a, trusted_by_b)
}

#[test]
fn code_pairing_shows_the_same_digits_and_needs_both_to_accept() {
    let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let (ka, kb) = (*a.public(), *b.public());
    let (left, right) = linked();
    let other = thread::spawn(move || {
        let pending = begin_code_pairing(right, &b, "Laptop", Role::Responder).unwrap();
        let code = pending.code().to_owned();
        assert_eq!(pending.peer().name, "Phone");
        (code, pending.finish(true).unwrap())
    });
    let pending = begin_code_pairing(left, &a, "Phone", Role::Initiator).unwrap();
    let code = pending.code().to_owned();
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|c| c.is_ascii_digit()));
    let peer = pending.finish(true).unwrap();
    let (their_code, their_peer) = other.join().unwrap();
    assert_eq!(code, their_code);
    assert_eq!(peer, Peer { key: kb, name: "Laptop".into() });
    assert_eq!(their_peer.key, ka);

    let (left, right) = linked();
    let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let other = thread::spawn(move || begin_code_pairing(right, &b, "B", Role::Responder).unwrap().finish(false));
    let mine = begin_code_pairing(left, &a, "A", Role::Initiator).unwrap().finish(true);
    assert_eq!(mine, Err(LanError::Declined));
    assert_eq!(other.join().unwrap(), Err(LanError::Declined));
}

/// A party in the middle runs two sessions, so the two screens derive their codes from
/// different transcripts. Checks the digits really depend on the transcript and on both nonces.
#[test]
fn code_depends_on_session_and_both_nonces() {
    let (na, nb, h) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let base = short_code(&na, &nb, &h).unwrap();
    assert_ne!(base, short_code(&[9u8; 32], &nb, &h).unwrap());
    assert_ne!(base, short_code(&na, &[9u8; 32], &h).unwrap());
    assert_ne!(base, short_code(&na, &nb, &[9u8; 32]).unwrap());
}

#[test]
fn a_reveal_that_does_not_match_the_commitment_is_refused() {
    let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let (left, right) = linked();
    let other = thread::spawn(move || begin_code_pairing(right, &b, "B", Role::Responder).map(|_| ()));
    // A cheating initiator: commits to one value and reveals another, as an attacker would
    // to steer the code after seeing the other side's nonce.
    let mut channel = Channel::handshake(left, &a, Role::Initiator, None).unwrap();
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Hello {
        name: String,
    }
    channel.send_value(&Hello { name: "A".into() }).unwrap();
    let _: Hello = channel.recv_value().unwrap();
    channel.send_value(&[7u8; 32]).unwrap();
    let _: [u8; 32] = channel.recv_value().unwrap();
    channel.send_value(&[8u8; 32]).unwrap();
    assert_eq!(other.join().unwrap().err(), Some(LanError::Tampered));
}

#[test]
fn qr_pairing_needs_the_secret_and_expires() {
    let qr = PairingQr::new("192.168.1.5:9000", 1000).unwrap();
    let text = qr.text();
    let scanned = PairingQr::parse(&text, 2000).unwrap();
    assert_eq!(scanned, qr);
    assert_eq!(scanned.address, "192.168.1.5:9000");
    assert_eq!(PairingQr::parse(&text, 1000 + 120_001).err(), Some(LanError::Expired));
    assert_eq!(PairingQr::parse("nonsense", 1).err(), Some(LanError::BadCode));

    let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let key_a = *a.public();
    let (left, right) = linked();
    let shown = qr.clone();
    let other = thread::spawn(move || shown.pair(right, &b, "Laptop", Role::Responder, 2000).unwrap());
    let peer = scanned.pair(left, &a, "Phone", Role::Initiator, 2000).unwrap();
    assert_eq!(peer.name, "Laptop");
    assert_eq!(other.join().unwrap().key, key_a);

    let wrong = PairingQr::new("x", 1000).unwrap();
    let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let (left, right) = linked();
    let other = thread::spawn(move || qr.pair(right, &b, "B", Role::Responder, 2000).map(|_| ()));
    assert!(wrong.pair(left, &a, "A", Role::Initiator, 2000).is_err());
    assert!(other.join().unwrap().is_err());
}

#[test]
fn sync_sends_only_what_differs_and_both_converge() {
    let (mut a, mut b) = vault_pair();
    let (ia, ib, ta, tb) = peers();
    let at = a.tick(2000);
    for n in 0..20 {
        a.add(login_entry(&format!("Shared {n}"), "u", "p", at), 2000).unwrap();
    }
    let first = run_sync(&mut a, &mut b, &ia, &ib, &ta, &tb);
    assert_eq!((first.sent, first.received), (20, 0));
    assert!(first.converged);

    let at = a.tick(3000);
    let only_a = a.add(login_entry("New on A", "u", "pa", at), 3000).unwrap();
    let at = b.tick(3001);
    let only_b = b.add(login_entry("New on B", "u", "pb", at), 3001).unwrap();
    let folder = a.add_folder("Work", None, 3002).unwrap();
    let second = run_sync(&mut a, &mut b, &ia, &ib, &ta, &tb);
    assert_eq!((second.sent, second.received), (1, 1), "only the two new entries travel");
    assert!(second.converged);
    assert!(a.get(&only_b).is_some() && b.get(&only_a).is_some());
    assert!(b.doc.folders.contains_key(&folder));
    assert_eq!(a.doc.entries, b.doc.entries);

    a.purge(&only_b, 4000);
    let third = run_sync(&mut a, &mut b, &ia, &ib, &ta, &tb);
    assert!(third.converged);
    assert!(b.get(&only_b).is_none(), "a delete crosses over");
    assert!(a.get(&only_b).is_none(), "and the old copy does not come back");
    let quiet = run_sync(&mut a, &mut b, &ia, &ib, &ta, &tb);
    assert_eq!((quiet.sent, quiet.received), (0, 0));
}

fn run_sync(a: &mut Vault, b: &mut Vault, ia: &Identity, ib: &Identity, ta: &[Peer], tb: &[Peer]) -> wmpass_lan::sync::LanReport {
    let (left, right) = linked();
    thread::scope(|scope| {
        let other = scope.spawn(|| sync(right, ib, tb, b, &mut NoBlobs, Role::Responder).unwrap());
        let report = sync(left, ia, ta, a, &mut NoBlobs, Role::Initiator).unwrap();
        other.join().unwrap();
        report
    })
}

#[test]
fn large_entries_cross_frame_boundaries() {
    let (mut a, mut b) = vault_pair();
    let (ia, ib, ta, tb) = peers();
    let at = a.tick(2000);
    let mut entry = login_entry("Big", "u", "p", at);
    entry.kind = Kind::Note(Note { body: Lww::new(Secret::from("x".repeat(300_000)), at) });
    let id = a.add(entry, 2000).unwrap();
    run_sync(&mut a, &mut b, &ia, &ib, &ta, &tb);
    let Kind::Note(note) = &b.get(&id).unwrap().kind else { panic!() };
    assert_eq!(note.body.v.expose().len(), 300_000);
}

#[test]
fn unpaired_devices_are_refused_and_get_nothing() {
    let (mut a, mut b) = vault_pair();
    let (ia, ib, _, tb) = peers();
    let at = a.tick(2000);
    a.add(login_entry("Secret", "u", "p", at), 2000).unwrap();
    let (left, right) = linked();
    let outcome = thread::scope(|scope| {
        let other = scope.spawn(|| sync(right, &ib, &tb, &mut b, &mut NoBlobs, Role::Responder));
        // A trusts nobody.
        let mine = sync(left, &ia, &[], &mut a, &mut NoBlobs, Role::Initiator);
        (mine.err(), other.join().unwrap().is_err())
    });
    assert_eq!(outcome.0, Some(LanError::UnknownPeer));
    assert!(outcome.1);
    assert!(b.doc.entries.is_empty());
}

#[test]
fn a_paired_device_without_the_vault_gets_nothing() {
    let (mut a, _) = vault_pair();
    let mut stranger = Vault::create("Other", b"x", &config(), 3, 1).unwrap();
    let (ia, ib, ta, tb) = peers();
    let at = a.tick(2000);
    a.add(login_entry("Secret", "u", "p", at), 2000).unwrap();
    let (left, right) = linked();
    let outcome = thread::scope(|scope| {
        let other = scope.spawn(|| sync(right, &ib, &tb, &mut stranger, &mut NoBlobs, Role::Responder));
        let mine = sync(left, &ia, &ta, &mut a, &mut NoBlobs, Role::Initiator);
        (mine.err(), other.join().unwrap().err())
    });
    assert_eq!(outcome.1, Some(LanError::NotThisVault));
    assert!(outcome.0.is_some());
    assert!(stranger.doc.entries.is_empty());
}

#[derive(Default)]
struct MemoryBlobs(BTreeMap<String, Vec<u8>>);

impl BlobStore for MemoryBlobs {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.0.get(name).cloned()
    }
    fn put(&mut self, name: &str, bytes: &[u8]) -> wmpass_lan::Result<()> {
        self.0.insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }
}

#[test]
fn attachments_travel_and_nothing_else_can_be_requested() {
    let (mut a, mut b) = vault_pair();
    let (ia, ib, ta, tb) = peers();
    let (record, sealed) = Vault::seal_attachment("scan.pdf", "application/pdf", &vec![9u8; 200_000]).unwrap();
    let at = a.tick(2000);
    let mut entry = login_entry("With file", "u", "p", at);
    entry.common.attachments = Lww::new(vec![record.clone()], at);
    a.add(entry, 2000).unwrap();
    let mut store_a = MemoryBlobs::default();
    store_a.0.insert(record.blob.clone(), sealed);
    store_a.0.insert("private-unreferenced".into(), b"not for sharing".to_vec());
    let mut store_b = MemoryBlobs::default();
    let (left, right) = linked();
    let report = thread::scope(|scope| {
        let other = scope.spawn(|| sync(right, &ib, &tb, &mut b, &mut store_b, Role::Responder).unwrap());
        let mine = sync(left, &ia, &ta, &mut a, &mut store_a, Role::Initiator).unwrap();
        other.join().unwrap();
        mine
    });
    assert_eq!(report.blobs_sent, 1);
    assert!(!store_b.0.contains_key("private-unreferenced"));
    let opened = Vault::open_attachment(&record, &store_b.0[&record.blob]).unwrap();
    assert_eq!(opened.len(), 200_000);
}

#[test]
fn a_changed_password_slot_reaches_the_other_device() {
    let (mut a, mut b) = vault_pair();
    let (ia, ib, ta, tb) = peers();
    a.add_slot(b"second", "Second", SlotAction::Open, None, 5000).unwrap();
    assert!(run_sync(&mut a, &mut b, &ia, &ib, &ta, &tb).converged);
    let file = b.save().unwrap();
    let opened = wmpass_core::vault::open_set(&[&file], b"second", &[], 2).unwrap();
    assert!(matches!(opened, wmpass_core::vault::OpenOutcome::Opened { .. }));
}

/// Flips one bit of what passes through.
struct Tamper<S> {
    inner: S,
    count: usize,
    at: usize,
}

impl<S: Read> Read for Tamper<S> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buffer)?;
        for byte in &mut buffer[..n] {
            if self.count == self.at {
                *byte ^= 1;
            }
            self.count += 1;
        }
        Ok(n)
    }
}

impl<S: Write> Write for Tamper<S> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buffer)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[test]
fn any_flipped_bit_stops_the_session() {
    for at in [3, 40, 90, 200, 400] {
        let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let (left, right) = linked();
        let other = thread::spawn(move || {
            let mut channel = Channel::handshake(right, &b, Role::Responder, None)?;
            channel.send(b"hello")?;
            channel.recv().map(|_| ())
        });
        let outcome = (|| {
            let mut channel = Channel::handshake(Tamper { inner: left, count: 0, at }, &a, Role::Initiator, None)?;
            let got = channel.recv()?;
            assert_eq!(&got[..], b"hello");
            Ok::<_, LanError>(())
        })();
        if at < 150 {
            assert!(outcome.is_err(), "flip at {at} went unnoticed");
        }
        drop(other);
    }
}

#[test]
fn hostile_frames_do_not_panic_or_allocate_without_limit() {
    let (a, _) = (Identity::generate().unwrap(), ());
    let (left, mut right) = linked();
    let writer = thread::spawn(move || {
        right.write_all(&[0xff, 0xff]).unwrap();
        right.write_all(&[0u8; 65_535]).unwrap();
    });
    assert!(Channel::handshake(left, &a, Role::Responder, None).is_err());
    writer.join().unwrap();
}

#[test]
fn qr_handover_only_the_scanner_can_open() {
    let at = wmpass_core::clock::Hlc { ms: 5, counter: 0, device: 1 };
    let mut entry = login_entry("Wi-Fi at home", "u", "hunter2", at);
    entry.id = "original".into();
    entry.common.folder = Lww::new(Some("private folder".into()), at);
    entry.history.push(wmpass_core::model::Revision { field: "password".into(), value: "old".into(), at });
    let receiver = ShareReceiver::new(1000).unwrap();
    let sealed = seal_to_offer(&receiver.offer_text(), &entry, 2000).unwrap();
    assert!(!sealed.contains("hunter2"));
    let got = receiver.open(&sealed, 3000).unwrap();
    assert_eq!(got.login().unwrap().password.v.expose(), "hunter2");
    assert!(got.history.is_empty(), "past passwords stay behind");
    assert!(got.id.is_empty());
    assert_eq!(got.common.folder.v, None);

    let intruder = ShareReceiver::new(1000).unwrap();
    assert_eq!(intruder.open(&sealed, 3000).err(), Some(LanError::BadCode));
    let late = ShareReceiver::new(1000).unwrap();
    let sealed = seal_to_offer(&late.offer_text(), &entry, 2000).unwrap();
    assert_eq!(late.open(&sealed, 1000 + 120_001).err(), Some(LanError::Expired));

    let big = {
        let mut e = entry.clone();
        e.kind = Kind::Note(Note { body: Lww::new(Secret::from("x".repeat(5000)), at) });
        e
    };
    let offer = ShareReceiver::new(1000).unwrap().offer_text();
    assert_eq!(seal_to_offer(&offer, &big, 2000).err(), Some(LanError::TooLarge));
}

#[test]
fn network_handover_works_once_with_the_right_code() {
    let at = wmpass_core::clock::Hlc { ms: 5, counter: 0, device: 1 };
    let entry = login_entry("Big", "u", "pw", at);
    let invite = ShareInvite::new("10.0.0.2:7000", 1000).unwrap();
    let scanned = ShareInvite::parse(&invite.text(), 2000).unwrap();
    assert_eq!(scanned.address, "10.0.0.2:7000");
    let (left, right) = linked();
    let copy = entry.clone();
    let other = thread::spawn(move || invite.serve(right, &copy, 2000));
    let got = scanned.fetch(left).unwrap();
    other.join().unwrap().unwrap();
    assert_eq!(got.login().unwrap().password.v.expose(), "pw");

    let invite = ShareInvite::new("x", 1000).unwrap();
    let wrong = ShareInvite::new("x", 1000).unwrap();
    let (left, right) = linked();
    let copy = entry.clone();
    let other = thread::spawn(move || invite.serve(right, &copy, 2000));
    assert!(wrong.fetch(left).is_err());
    assert!(other.join().unwrap().is_err());
}

#[test]
#[ignore = "uses real multicast on this machine's network"]
fn devices_find_each_other() {
    let advert = wmpass_lan::discovery::advertise(47113).unwrap();
    let found = wmpass_lan::discovery::browse(std::time::Duration::from_secs(4)).unwrap();
    drop(advert);
    assert!(found.iter().any(|a| a.port() == 47113), "{found:?}");
}

#[derive(Default)]
struct Shelf(BTreeMap<String, Vec<u8>>);

impl BlobStore for Shelf {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.0.get(name).cloned()
    }
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<(), LanError> {
        self.0.insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }
}

#[test]
fn a_new_device_joins_by_qr_and_gets_the_file_and_attachments() {
    use wmpass_lan::join::{Arrival, arrive, send_by_qr};
    let (mut vault, _) = vault_pair();
    let file = vault.save().unwrap();
    let (old, new) = (Identity::generate().unwrap(), Identity::generate().unwrap());
    let (old_key, new_key) = (*old.public(), *new.public());
    let qr = PairingQr::new("127.0.0.1:1", 1000).unwrap();
    let scanned = PairingQr::parse(&qr.text(), 1000).unwrap();
    let mut held = Shelf::default();
    held.0.insert("abc123".into(), b"sealed".to_vec());
    held.0.insert("not-referenced".into(), b"private".to_vec());
    let (left, right) = linked();
    let sent = file.clone();
    let sender = thread::spawn(move || send_by_qr(left, &scanned, &old, "Phone", 1000, &sent, &["abc123".to_owned()], &held).unwrap());
    let mut got = Shelf::default();
    let Arrival::Done(joined) = arrive(right, &new, "Mac", Some(&qr), 1000, &mut got).unwrap() else { panic!("asked for digits") };
    let peer = sender.join().unwrap();
    assert_eq!(joined.file, file);
    assert_eq!(joined.peer, Peer { key: old_key, name: "Phone".into() });
    assert_eq!(peer, Peer { key: new_key, name: "Mac".into() });
    assert_eq!(got.0.keys().collect::<Vec<_>>(), ["abc123"]);
    // The file opens with the vault's password on the new device.
    let opened = wmpass_core::vault::open_set(&[&joined.file], b"pw", &[], 3).unwrap();
    assert!(matches!(opened, wmpass_core::vault::OpenOutcome::Opened { .. }));
}

#[test]
fn a_new_device_joins_by_digits_only_when_both_accept() {
    use wmpass_lan::join::{Arrival, arrive, begin_send_by_code, finish_send};
    let (mut vault, _) = vault_pair();
    let file = vault.save().unwrap();
    for (sender_yes, joiner_yes) in [(true, true), (true, false)] {
        let (old, new) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let (left, right) = linked();
        let sent = file.clone();
        let sender = thread::spawn(move || {
            let pending = begin_send_by_code(left, &old, "Phone").unwrap();
            let code = pending.code().to_owned();
            (code, finish_send(pending, sender_yes, &sent, &[], &NoBlobs))
        });
        let Arrival::Code(pending) = arrive(right, &new, "Mac", None, 1000, &mut NoBlobs).unwrap() else { panic!("no digits") };
        let code = pending.code().to_owned();
        let joined = pending.finish(joiner_yes, &mut NoBlobs);
        let (their_code, sent) = sender.join().unwrap();
        assert_eq!(code, their_code);
        if joiner_yes {
            assert_eq!(joined.unwrap().file, file);
            assert!(sent.is_ok());
        } else {
            assert_eq!(joined.unwrap_err(), LanError::Declined);
            assert_eq!(sent.unwrap_err(), LanError::Declined);
        }
    }
}

#[test]
fn a_qr_sender_cannot_reach_a_device_showing_another_code() {
    use wmpass_lan::join::{arrive, send_by_qr};
    let (mut vault, _) = vault_pair();
    let file = vault.save().unwrap();
    let shown = PairingQr::new("127.0.0.1:1", 1000).unwrap();
    let forged = PairingQr::new("127.0.0.1:1", 1000).unwrap();
    let (left, right) = linked();
    let sender = thread::spawn(move || send_by_qr(left, &forged, &Identity::generate().unwrap(), "X", 1000, &file, &[], &NoBlobs));
    assert!(arrive(right, &Identity::generate().unwrap(), "Mac", Some(&shown), 1000, &mut NoBlobs).is_err());
    assert!(sender.join().unwrap().is_err());
}
