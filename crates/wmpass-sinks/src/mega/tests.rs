//! MEGA's crypto against values go-mega produces for the same inputs (the vectors WM
//! Keyboard's CloudCryptoTest checks), then the sink and the sign-in against a scripted
//! MEGA that stores what it is sent and serves it back.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use num_bigint::BigUint;
use serde_json::{Value, json};
use wmpass_net::http::{Http, NetError, Request, Response};
use wmpass_net::NetPolicy;

use super::*;
use crate::http::{Blocking, block_on};

// ---- Vectors ----

#[test]
fn password_key_user_hash_and_name_match_go_mega() {
    let key = prepare_key("correct horse battery");
    assert_eq!(b64encode(&key), "zIL6_CnFYrZf2lHSZcCBew");
    assert_eq!(string_hash("someone@example.com", &key).unwrap(), "tJF4Tp-IoVg");
    let name = encrypt_name(b"0123456789abcdef", "wmkeyboard-auto-x.json").unwrap();
    assert_eq!(name, "rMxvZWwJjY9RcwNZk0gk5bhZqr-nKk1YQCQhJc8VpxEB3ztoww2tBLqQLZF0klYF");
    assert_eq!(decrypt_name(b"0123456789abcdef", &name).unwrap(), "wmkeyboard-auto-x.json");
    assert_eq!(decrypt_name(b"fedcba9876543210", &name), None, "the wrong key reads no MEGA prefix");
    assert_eq!(decrypt_name(b"0123456789abcdef", "AAAA"), None, "not a whole block");
}

#[test]
fn file_key_and_mac_match_go_mega_and_a_changed_byte_fails() {
    let data: Vec<u8> = (0..1_500_000u32).map(|i| (i * 13) as u8).collect();
    let sealed = seal(&data, [1, 2, 3, 4, 5, 6]).unwrap();
    assert_eq!(b64encode(&to_bytes(&sealed.node_key)), "AAAABAAAAARBp6oiaXbN2wAAAAUAAAAGQaeqIWl2zd8");
    assert_eq!(open(&sealed.ciphertext, &sealed.node_key).unwrap(), data);
    let mut changed = sealed.ciphertext.clone();
    changed[100] = changed[100].wrapping_add(1);
    assert_eq!(open(&changed, &sealed.node_key), None);
    assert_eq!(open(&sealed.ciphertext, &sealed.node_key[..6]), None, "a folder-sized key");
}

#[test]
fn chunk_boundaries_grow_to_a_megabyte() {
    let list = chunks(6_000_000);
    assert_eq!(list[0], (0, 131_072));
    assert_eq!(list[1].1, 262_144);
    assert_eq!(list[8].1, 1_048_576);
    assert_eq!(list.iter().map(|c| c.1).sum::<u64>(), 6_000_000);
    assert!(chunks(0).is_empty());
}

#[test]
fn words_pad_short_tails_and_base64_takes_either_padding() {
    assert_eq!(to_words(&[1, 2, 3, 4, 5]), vec![0x0102_0304, 0x0500_0000]);
    assert_eq!(b64decode("AQI="), Some(vec![1, 2]));
    assert_eq!(b64decode("AQI"), Some(vec![1, 2]));
    assert_eq!(b64encode(&[0xfb, 0xff]), "-_8");
}

/// A toy RSA key on two Mersenne primes, wrapped the way MEGA wraps an account's key.
struct Rsa {
    privk: String,
    csid: String,
    sid: String,
}

fn mpi_bytes(n: &BigUint) -> Vec<u8> {
    [&(n.bits() as u16).to_be_bytes()[..], &n.to_bytes_be()].concat()
}

fn rsa(master: &[u8]) -> Rsa {
    let one = BigUint::from(1u8);
    let p = (BigUint::from(1u8) << 521u32) - &one;
    let q = (BigUint::from(1u8) << 607u32) - &one;
    let e = BigUint::from(65_537u32);
    let d = e.modinv(&((&p - &one) * (&q - &one))).unwrap();
    let message: Vec<u8> = (0..100u8).map(|i| i.wrapping_mul(7).wrapping_add(0x5a)).collect();
    let c = BigUint::from_bytes_be(&message).modpow(&e, &(&p * &q));
    let plain = [mpi_bytes(&p), mpi_bytes(&q), mpi_bytes(&d), mpi_bytes(&one)].concat();
    Rsa {
        privk: b64encode(&ecb_encrypt(master, &pad16(&plain)).unwrap()),
        csid: b64encode(&mpi_bytes(&c)),
        sid: b64encode(&message[..43]),
    }
}

#[test]
fn the_session_id_is_the_rsa_decrypted_csid() {
    let master = [7u8; 16];
    let toy = rsa(&master);
    assert_eq!(decrypt_session_id(&toy.privk, &toy.csid, &master).unwrap(), toy.sid);
    assert_eq!(decrypt_session_id(&toy.privk, &toy.csid, &[8u8; 16]), None, "the wrong master key reads no MPIs");
    assert_eq!(decrypt_session_id("", &toy.csid, &master), None);
}

#[test]
fn hashcash_finds_a_prefix_under_the_threshold() {
    let token = b64encode(&[9u8; 48]);
    // Easiness 255 puts the threshold near half of all hashes, so this takes a few tries.
    let answer = hashcash(&format!("1:255:1700000000:{token}")).unwrap();
    let (rest, prefix) = answer.rsplit_once(':').unwrap();
    assert_eq!(rest, format!("1:{token}"));
    let mut buffer = b64decode(prefix).unwrap();
    for _ in 0..262_144 {
        buffer.extend_from_slice(&[9u8; 48]);
    }
    let hash = Sha256::digest(&buffer);
    assert!(u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) <= 127 << 24);
    for bad in ["", "2:255:1:AAAA", "1:256:1:AAAA", "1:x:1:AAAA", "1:255:1"] {
        assert_eq!(hashcash(bad), None, "{bad}");
    }
}

// ---- A scripted MEGA ----

const OWNER: &str = "me";

#[derive(Default)]
struct State {
    nodes: Vec<Value>,
    uploads: HashMap<String, (u64, Vec<u8>)>,
    blobs: HashMap<String, Vec<u8>>,
    next: u32,
    /// Answers served before anything else, in order.
    script: VecDeque<Response>,
    asked: Vec<Request>,
    commands: Vec<Value>,
    account: Option<Account>,
}

/// The one account the scripted MEGA knows.
#[derive(Clone)]
struct Account {
    email: String,
    handle: String,
    code: Option<String>,
    answer: Value,
    version: i64,
    salt: Option<String>,
}

struct FakeMega(Mutex<State>);

fn ok(body: Value) -> Response {
    Response { status: 200, body: body.to_string().into_bytes(), ..Response::default() }
}

fn text(body: &str) -> Response {
    Response { status: 200, body: body.as_bytes().to_vec(), ..Response::default() }
}

impl FakeMega {
    fn new() -> Self {
        let state = State {
            nodes: vec![
                json!({"h": "ROOT", "t": 2, "p": ""}),
                json!({"h": "TRASH", "t": 4, "p": ""}),
                // Shared by someone else: its key is not ours to unwrap.
                json!({"h": "FOREIGN", "t": 1, "p": "ROOT", "u": "other", "k": "other:AAAAAAAAAAAAAAAAAAAAAA", "a": "x"}),
            ],
            ..State::default()
        };
        Self(Mutex::new(state))
    }

    fn handle(&self, state: &mut State) -> String {
        state.next += 1;
        format!("H{:07}", state.next)
    }

    fn command(&self, state: &mut State, cmd: &Value) -> Response {
        let field = |name: &str| cmd.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
        match field("a").as_str() {
            "f" => ok(json!([{"f": state.nodes}])),
            "u" => {
                let id = self.handle(state);
                let size = cmd["s"].as_u64().unwrap();
                state.uploads.insert(id.clone(), (size, Vec::new()));
                ok(json!([{"p": format!("https://up.example/ul/{id}")}]))
            }
            "p" => {
                let made = &cmd["n"][0];
                let handle = self.handle(state);
                let completion = made["h"].as_str().unwrap();
                let size = state.blobs.get(completion).map(|blob| blob.len());
                if let Some(blob) = state.blobs.remove(completion) {
                    state.blobs.insert(handle.clone(), blob);
                }
                let mut node = json!({
                    "h": handle, "p": field("t"), "t": made["t"], "u": OWNER,
                    "a": made["a"], "k": format!("{OWNER}:{}", made["k"].as_str().unwrap()), "ts": 1_700_000_000,
                });
                if let Some(size) = size {
                    node["s"] = json!(size);
                }
                state.nodes.push(node.clone());
                ok(json!([{"f": [node]}]))
            }
            "g" => match state.blobs.contains_key(&field("n")) {
                true => ok(json!([{"g": format!("https://dl.example/{}", field("n")), "s": 1}])),
                false => ok(json!([-9])),
            },
            "d" => {
                let before = state.nodes.len();
                let gone = field("n");
                state.nodes.retain(|node| node["h"] != gone.as_str());
                ok(json!([if state.nodes.len() < before { 0 } else { -9 }]))
            }
            "us0" => {
                let account = state.account.clone().unwrap();
                match account.salt {
                    Some(salt) => ok(json!([{"v": account.version, "s": salt}])),
                    None => ok(json!([{"v": account.version}])),
                }
            }
            "us" => {
                let Account { email, handle, code, answer, .. } = state.account.clone().unwrap();
                if field("user") != email || field("uh") != handle {
                    return ok(json!([-9]));
                }
                match code {
                    Some(_) if field("mfa").is_empty() => ok(json!([-26])),
                    Some(code) if field("mfa") != code => ok(json!([-11])),
                    _ => ok(json!([answer])),
                }
            }
            _ => ok(json!([-2])),
        }
    }
}

impl Http for FakeMega {
    fn send(&self, request: &Request) -> std::result::Result<Response, NetError> {
        let mut state = self.0.lock().unwrap();
        state.asked.push(request.clone());
        if let Some(scripted) = state.script.pop_front() {
            return Ok(scripted);
        }
        if request.url.starts_with(&format!("{API}/cs?id=")) {
            let body: Value = serde_json::from_slice(&request.body.as_ref().unwrap().1).unwrap();
            let cmd = body[0].clone();
            state.commands.push(cmd.clone());
            return Ok(self.command(&mut state, &cmd));
        }
        if let Some(rest) = request.url.strip_prefix("https://up.example/ul/") {
            let (id, offset) = rest.split_once('/').unwrap();
            let bytes = request.body.as_ref().unwrap().1.clone();
            let (size, data) = state.uploads.get_mut(id).unwrap();
            assert_eq!(offset.parse::<usize>().unwrap(), data.len(), "chunks arrive in order");
            data.extend_from_slice(&bytes);
            if data.len() as u64 == *size {
                let blob = std::mem::take(data);
                let completion = format!("C{id}");
                state.blobs.insert(completion.clone(), blob);
                return Ok(text(&completion));
            }
            return Ok(text(""));
        }
        if let Some(handle) = request.url.strip_prefix("https://dl.example/") {
            return Ok(Response { status: 200, body: state.blobs[handle].clone(), ..Response::default() });
        }
        Err(NetError::Unreachable)
    }
}

const MASTER: [u8; 16] = [0x42; 16];

fn config(folder: &str) -> MegaConfig {
    MegaConfig { email: "me@example.com".into(), session_id: "SID".into(), master_key: b64encode(&MASTER).into(), folder: folder.into() }
}

fn sink<'a>(fake: &'a FakeMega, folder: &str) -> Mega<Blocking<'a>> {
    Mega::new(Client::new(Blocking(fake), NetPolicy::Any), config(folder))
}

#[test]
fn round_trip_through_encrypted_nodes() {
    let fake = FakeMega::new();
    let mega = sink(&fake, "/Backups/ WM /");
    block_on(mega.readiness()).unwrap();
    assert!(block_on(mega.list()).unwrap().is_empty());
    {
        let state = fake.0.lock().unwrap();
        let folders: Vec<&Value> = state.nodes.iter().filter(|n| n["t"] == 1 && n["u"] == OWNER).collect();
        assert_eq!(folders.len(), 2, "made level by level, once");
        assert_eq!(folders[1]["p"], folders[0]["h"]);
        assert!(state.asked.iter().all(|r| !r.url.contains("cs?") || r.url.ends_with("&sid=SID")));
    }

    let big: Vec<u8> = (0..400_000u32).map(|i| (i % 253) as u8).collect();
    let first = block_on(mega.write("vault.wmvault", b"first")).unwrap();
    let second = block_on(mega.write("vault.wmvault", &big)).unwrap();
    let empty = block_on(mega.write("empty.wmvault", b"")).unwrap();
    assert_ne!(first.id, second.id);
    {
        let state = fake.0.lock().unwrap();
        // 400,000 bytes are three chunks: 128 KiB, 256 KiB, then the rest.
        let uploads: Vec<&str> = state.asked.iter().filter(|r| r.url.starts_with("https://up.")).map(|r| r.url.as_str()).collect();
        assert!(uploads.iter().any(|u| u.ends_with("/393216")), "{uploads:?}");
        // Nothing MEGA holds names the file in the clear.
        let stored = serde_json::to_string(&state.nodes).unwrap();
        assert!(!stored.contains("vault") && !stored.contains("Backups"));
        assert!(state.blobs.values().all(|blob| blob != &big));
    }

    let listed = block_on(mega.list()).unwrap();
    let mut names: Vec<(&str, u64)> = listed.iter().map(|f| (f.name.as_str(), f.size)).collect();
    names.sort_unstable();
    assert_eq!(names, vec![("empty.wmvault", 0), ("vault.wmvault", 400_000)], "the older copy was removed");
    assert_eq!(block_on(mega.read(&second)).unwrap(), big);
    assert_eq!(block_on(mega.read(&empty)).unwrap(), b"");
    assert_eq!(block_on(mega.read(&first)), Err(Fault::TargetMissing));

    // A damaged download fails its MAC.
    fake.0.lock().unwrap().blobs.get_mut(&second.id).unwrap()[7] ^= 1;
    assert_eq!(block_on(mega.read(&second)), Err(Fault::NotVerified));

    block_on(mega.delete(&second)).unwrap();
    block_on(mega.delete(&second)).unwrap();
    assert_eq!(block_on(mega.list()).unwrap().len(), 1);
}

#[test]
fn an_existing_folder_is_found_by_its_decrypted_name() {
    let fake = FakeMega::new();
    block_on(sink(&fake, "Shared/WM").readiness()).unwrap();
    let before = fake.0.lock().unwrap().nodes.len();
    // A second sink starts with no cached handle and must find, not make, the folders.
    let again = sink(&fake, "Shared/WM");
    block_on(again.write("v", b"x")).unwrap();
    assert_eq!(fake.0.lock().unwrap().nodes.len(), before + 1);
}

#[test]
fn errors_map_to_faults() {
    let expired = FakeMega::new();
    expired.0.lock().unwrap().script.push_back(ok(json!(-15)));
    assert_eq!(block_on(sink(&expired, "W").list()), Err(Fault::PermissionLost));

    let full = FakeMega::new();
    let mega = sink(&full, "W");
    block_on(mega.readiness()).unwrap();
    full.0.lock().unwrap().script.extend([ok(json!([{"f": []}])), ok(json!([-17]))]);
    assert_eq!(block_on(mega.write("v", b"x")), Err(Fault::OutOfSpace));

    let broken = FakeMega::new();
    broken.0.lock().unwrap().script.push_back(Response { status: 500, ..Response::default() });
    assert_eq!(block_on(sink(&broken, "W").list()), Err(Fault::Io));

    let fake = FakeMega::new();
    let no_key = Mega::new(Client::new(Blocking(&fake), NetPolicy::Any), MegaConfig { master_key: "short".into(), ..config("W") });
    assert_eq!(block_on(no_key.readiness()), Err(Fault::PermissionLost));
    let unset = Mega::new(Client::new(Blocking(&fake), NetPolicy::Any), MegaConfig::default());
    assert_eq!(block_on(unset.readiness()), Err(Fault::NotConfigured));
    let offline = Mega::new(Client::new(Blocking(&fake), NetPolicy::Offline), config("W"));
    assert_eq!(block_on(offline.list()), Err(Fault::Blocked));
    assert!(fake.0.lock().unwrap().asked.is_empty());
}

#[test]
fn busy_is_retried_and_a_proof_of_work_is_answered() {
    let fake = FakeMega::new();
    let challenge = format!("1:255:1700000000:{}", b64encode(&[3u8; 48]));
    fake.0.lock().unwrap().script.extend([
        ok(json!(-3)),
        Response { status: 402, headers: vec![("x-hashcash".into(), challenge)], ..Response::default() },
    ]);
    block_on(sink(&fake, "W").readiness()).unwrap();
    let state = fake.0.lock().unwrap();
    let solved = state.asked.iter().find_map(|r| r.headers.iter().find(|(n, _)| *n == "X-Hashcash")).unwrap();
    assert!(solved.1.starts_with("1:AwMD"));
    assert_ne!(state.asked[0].url, state.asked[1].url, "each try is a new request id");
}

// ---- Sign-in ----

fn account(fake: &FakeMega, version: i64, password: &str, code: Option<&str>) -> (Vec<u8>, String) {
    let master = [0x24u8; 16];
    let email = "someone@example.com";
    let (pass_key, handle, salt) = if version >= 2 {
        let salt = [5u8; 32];
        let mut derived = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha512>(password.as_bytes(), &salt, V2_ROUNDS, &mut derived);
        (derived[..16].to_vec(), b64encode(&derived[16..]), Some(b64encode(&salt)))
    } else {
        let key = prepare_key(password);
        (key.to_vec(), string_hash(email, &key).unwrap(), None)
    };
    let toy = rsa(&master);
    let answer = json!({"k": b64encode(&ecb_encrypt(&pass_key, &master).unwrap()), "privk": toy.privk, "csid": toy.csid});
    fake.0.lock().unwrap().account =
        Some(Account { email: email.into(), handle, code: code.map(str::to_owned), answer, version, salt });
    (master.to_vec(), toy.sid)
}

#[test]
fn a_v1_account_with_two_factor_signs_in_in_two_steps() {
    let fake = FakeMega::new();
    let (master, sid) = account(&fake, 1, "correct horse battery", Some("123456"));
    let client = Client::new(Blocking(&fake), NetPolicy::Any);
    let MegaSignIn::NeedsCode(pending) = block_on(sign_in(&client, "  Someone@Example.com ", &"correct horse battery".into())) else {
        panic!("expected a code request")
    };
    assert!(!format!("{pending:?}").contains("someone"));
    let MegaSignIn::Refused(fault) = block_on(sign_in_with_code(&client, clone_pending(&pending), "000000")) else { panic!() };
    assert_eq!(fault, Fault::PermissionLost);
    let MegaSignIn::SignedIn(signed) = block_on(sign_in_with_code(&client, pending, "123 456")) else { panic!("expected sign-in") };
    assert_eq!(signed.email, "someone@example.com");
    assert_eq!(signed.session_id.expose(), sid);
    assert_eq!(b64decode(signed.master_key.expose()).unwrap(), master);
    assert_eq!(signed.folder, DEFAULT_FOLDER);
    let printed = format!("{signed:?}");
    assert!(!printed.contains("someone") && !printed.contains(&sid));
    let state = fake.0.lock().unwrap();
    assert_eq!(state.commands.last().unwrap()["mfa"], "123456");
    assert!(state.commands.iter().all(|c| c.get("sek").is_none()), "v1 sends no sek");
}

fn clone_pending(pending: &MegaPending) -> MegaPending {
    MegaPending {
        email: pending.email.clone(),
        pass_key: pending.pass_key.clone(),
        user_handle: pending.user_handle.clone(),
        version: pending.version,
    }
}

#[test]
fn a_v2_account_signs_in_with_pbkdf2() {
    let fake = FakeMega::new();
    let (master, sid) = account(&fake, 2, "pässword", None);
    let client = Client::new(Blocking(&fake), NetPolicy::Any);
    let MegaSignIn::SignedIn(signed) = block_on(sign_in(&client, "someone@example.com", &"pässword".into())) else {
        panic!("expected sign-in")
    };
    assert_eq!(signed.session_id.expose(), sid);
    assert_eq!(b64decode(signed.master_key.expose()).unwrap(), master);
    assert!(fake.0.lock().unwrap().commands.last().unwrap().get("sek").is_some());
}

#[test]
fn a_wrong_password_is_refused_and_offline_is_unreachable() {
    let fake = FakeMega::new();
    account(&fake, 1, "right", None);
    let client = Client::new(Blocking(&fake), NetPolicy::Any);
    assert!(matches!(block_on(sign_in(&client, "someone@example.com", &"wrong".into())), MegaSignIn::Refused(Fault::PermissionLost)));
    fake.0.lock().unwrap().script.push_back(ok(json!(-16)));
    assert!(matches!(block_on(sign_in(&client, "someone@example.com", &"right".into())), MegaSignIn::Refused(Fault::PermissionLost)));
    let offline = Client::new(Blocking(&fake), NetPolicy::Offline);
    assert!(matches!(block_on(sign_in(&offline, "someone@example.com", &"right".into())), MegaSignIn::Unreachable(Fault::Blocked)));
}
