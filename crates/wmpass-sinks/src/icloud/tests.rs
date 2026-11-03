//! The SRP against rclone's numbers, then sign-in and the sink against an in-process
//! Apple that runs the server half of SRP, hands out cookies and session headers, and
//! keeps a folder tree.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use wmpass_net::{NetError, NetPolicy};

use super::*;
use crate::http::block_on;

const NOW_MS: u64 = 1_762_000_000_000;
const DRIVE: &str = "https://drive.example";
const DOCS: &str = "https://docs.example";
const USER: &str = "me@icloud.com";
const PASSWORD: &str = "correct horse";
const SESSION_COOKIE: &str = "X-APPLE-WEBAUTH-TOKEN";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(text: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(text).unwrap()
}

#[test]
fn password_stretching_matches_rclone() {
    let salt = b"test_salt_value!";
    assert_eq!(b64(&ICloudSrp::derive_password("test_password", salt, 1000, "s2k").unwrap()), "i/2cgo7v6v1Ugs0abOEzUVQ0izEN2gCSvZNoiZkxh8M=");
    assert_eq!(b64(&ICloudSrp::derive_password("test_password", salt, 1000, "s2k_fo").unwrap()), "ihYILE9JmatxDYSaoUjSA/J3DhTkb8khfNS1waSMLcY=");
    assert!(ICloudSrp::derive_password("p", salt, 1, "unknown").is_none());
}

/// rclone's `TestProcessChallenge` inputs: secret 0x42, derived key 0..31, server secret
/// 0x99. The expected values are what rclone's srp.go (v1.75.2) computes for them.
#[test]
fn proofs_match_rclone() {
    let mut secret = [0u8; 32];
    secret[31] = 0x42;
    let client = ICloudSrp::with_secret(&secret);
    let derived: Vec<u8> = (0..32).collect();
    let salt = b"fixed_salt_for_test!";
    let (n, g) = group();
    let x = BigUint::from_bytes_be(&sha(&[salt, &sha(&[b":", &derived])]));
    let k = BigUint::from_bytes_be(&sha(&[&n.to_bytes_be(), &pad(&g)]));
    let b = BigUint::from(0x99u32);
    let big_b = (&k * g.modpow(&x, &n) + g.modpow(&b, &n)) % &n;
    assert_eq!(b64(&pad(&big_b)), "LXMP4cVHMYwQoxdHBfUQS0OM+PSwo/jPgOW57mQ2BVRQl06Iv0ZcXLK1GRVqhMfFRMWQJO8VuhlgVAvJnJQ40NEClDOlRvRjZoN1TX5jYV8XoPpJXj70qTy6XZmKiqRVHUtrdyHuLb9t3cAbmo+JGN7gCMrcQaftNDKonzFSRRbLulOx9GG39SrJIrBHB72zpZ1VTujvvzipRXFgMCV5HOpxggnoaPf7/WMrkW2Pb2XAA0X4++laorE3L7QbNZXD0nNk6JSBkUxUQ5V1lIySWzR1dfM+r5Q2VOgWcQjwxdLNkqz6GH4iDBzqs92JvYoqABTfTa+8wFKIgU+lUrgyOA==");
    let (m1, m2) = client.proofs(b"testuser@apple.com", &derived, salt, &pad(&big_b)).unwrap();
    assert_eq!(b64(&m1), "jPnfiP5mnYhgoST17FXPEZHY8IFgQfSduJaktDZ+mkg=");
    assert_eq!(b64(&m2), "YWMnK3lfo1Mz/M/nWiQbaTv2d+/oCkanVWYwQU/Hj9g=");
    assert_eq!(client.public_a().len(), 256);
    assert!(client.proofs(b"u", &derived, salt, &pad(&BigUint::ZERO)).is_none());
    assert!(client.proofs(b"u", &derived, salt, &pad(&n)).is_none());
}

#[test]
fn cookies_are_replaced_dropped_and_expired() {
    let session = ICloudSession::new(USER, "", &"a=1; b=2".into(), &Secret::default(), NOW_MS).unwrap();
    let response = Response {
        status: 200,
        headers: vec![
            ("set-cookie".into(), "a=3; Path=/; Secure".into()),
            ("set-cookie".into(), "b=gone; Max-Age=0".into()),
            ("set-cookie".into(), "c=old; Expires=Thu, 01-Jan-1970 00:00:01 GMT".into()),
            ("set-cookie".into(), "d=new; Expires=Wed, 01 Jan 2031 00:00:00 GMT; Max-Age=60".into()),
            ("set-cookie".into(), "e=; Path=/".into()),
            ("scnt".into(), "s1".into()),
            ("x-apple-twosv-trust-token".into(), "t9".into()),
        ],
        ..Response::default()
    };
    session.absorb(&response);
    assert_eq!(session.cookie_string().expose(), "a=3; d=new");
    assert_eq!(session.trust_token().expose(), "t9");
    assert_eq!(lock(&session.state).scnt, "s1");
    assert_eq!(cookie_date("Wed, 01 Jan 2031 00:00:00 GMT"), Some(1_924_992_000_000));
    assert_eq!(cookie_date("Thu, 01-Jan-1970 00:00:01 GMT"), Some(1000));
    assert!(format!("{session:?}").find("a=3").is_none());
}

// ---------------------------------------------------------------------------------------
// A fake Apple.

struct Entry {
    parent: String,
    name: String,
    extension: String,
    folder: bool,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct State {
    salt: Vec<u8>,
    verifier: BigUint,
    server_secret: BigUint,
    a: Vec<u8>,
    trusted: Vec<String>,
    code_requested: bool,
    session_cookie: String,
    expire_once: bool,
    entries: BTreeMap<String, Entry>,
    uploads: Vec<Vec<u8>>,
    counter: usize,
    sign_ins: usize,
    validations: usize,
}

#[derive(Clone)]
struct Apple(Arc<Mutex<State>>);

fn answer(status: u16, body: Value, headers: &[(&str, &str)]) -> Response {
    Response {
        status,
        body: body.to_string().into_bytes(),
        headers: headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), (*v).to_owned())).collect(),
        ..Response::default()
    }
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

impl Apple {
    fn new() -> Self {
        let salt = b"apple salt 16 b!".to_vec();
        let derived = ICloudSrp::derive_password(PASSWORD, &salt, 20_000, "s2k").unwrap();
        let (n, g) = group();
        let x = BigUint::from_bytes_be(&sha(&[&salt, &sha(&[b":", &derived])]));
        let mut entries = BTreeMap::new();
        entries.insert(
            ROOT_ID.to_owned(),
            Entry { parent: String::new(), name: "root".into(), extension: String::new(), folder: true, bytes: Vec::new() },
        );
        Self(Arc::new(Mutex::new(State {
            salt,
            verifier: g.modpow(&x, &n),
            server_secret: BigUint::from_bytes_be(&[0x5a; 32]),
            entries,
            ..State::default()
        })))
    }

    fn services() -> Value {
        json!({ "webservices": { "drivews": { "url": DRIVE, "pcsRequired": false }, "docws": { "url": DOCS } } })
    }

    fn server_b(state: &State) -> BigUint {
        let (n, g) = group();
        let k = BigUint::from_bytes_be(&sha(&[&n.to_bytes_be(), &pad(&g)]));
        (&k * &state.verifier + g.modpow(&state.server_secret, &n)) % &n
    }

    fn signed_in(state: &State, request: &Request) -> bool {
        header(request, "Cookie").is_some_and(|c| c.split("; ").any(|p| p == format!("{SESSION_COOKIE}={}", state.session_cookie)))
            && !state.session_cookie.is_empty()
    }

    fn handle(&self, request: &Request) -> Response {
        let mut state = self.0.lock().unwrap();
        let body: Value = request.body.as_ref().and_then(|(_, b)| serde_json::from_slice(b).ok()).unwrap_or(Value::Null);
        let url = request.url.as_str();
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let auth_headers_ok = header(request, "scnt") == Some("scnt1") && header(request, "X-Apple-ID-Session-Id") == Some("sid1");
        match (request.method, path) {
            ("GET", p) if p == format!("{AUTH}/authorize/signin") => {
                assert!(query.contains("client_id=d39ba99") && query.contains("redirect_uri=https%3A%2F%2Fwww.icloud.com"));
                answer(200, json!({}), &[("Set-Cookie", "aasp=A1; Domain=.apple.com; Path=/; Secure")])
            }
            ("POST", p) if p == format!("{AUTH}/federate") => {
                assert_eq!(body["accountName"], USER);
                answer(200, json!({}), &[("scnt", "scnt1"), ("X-Apple-ID-Session-Id", "sid1")])
            }
            ("POST", p) if p == format!("{AUTH}/signin/init") => {
                assert!(auth_headers_ok, "init without the session headers");
                assert_eq!(header(request, "X-Apple-Widget-Key"), Some(CLIENT_ID));
                state.a = unb64(body["a"].as_str().unwrap());
                let b = pad(&Self::server_b(&state));
                answer(200, json!({ "salt": b64(&state.salt), "b": b64(&b), "iteration": 20_000, "protocol": "s2k", "c": "cval" }), &[])
            }
            ("POST", p) if p == format!("{AUTH}/signin/complete") => {
                state.sign_ins += 1;
                assert_eq!(body["c"], "cval");
                let (n, g) = group();
                let a = BigUint::from_bytes_be(&state.a);
                let b = Self::server_b(&state);
                let u = BigUint::from_bytes_be(&sha(&[&pad(&a), &pad(&b)]));
                let s = (&a * state.verifier.modpow(&u, &n)).modpow(&state.server_secret, &n);
                let key = sha(&[&pad(&s)]);
                let hg = sha(&[&pad(&g)]);
                let hn = sha(&[&n.to_bytes_be()]);
                let xor: Vec<u8> = hg.iter().zip(&hn).map(|(a, b)| a ^ b).collect();
                let m1 = sha(&[&xor, &sha(&[USER.as_bytes()]), &state.salt, &pad(&a), &pad(&b), &key]);
                if body["m1"].as_str().map(unb64) != Some(m1.clone()) {
                    return answer(401, json!({}), &[]);
                }
                assert_eq!(body["m2"].as_str().map(unb64), Some(sha(&[&pad(&a), &m1, &key])));
                let offered: Vec<String> = body["trustTokens"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_owned()).collect();
                if offered.iter().any(|t| state.trusted.contains(t)) {
                    answer(200, json!({}), &[("X-Apple-Session-Token", "st-trusted"), ("X-Apple-ID-Account-Country", "USA")])
                } else {
                    answer(409, json!({}), &[("X-Apple-Session-Token", "st-pending")])
                }
            }
            ("GET", AUTH) => answer(200, json!({ "noTrustedDevices": false, "trustedPhoneNumbers": [{ "id": 1 }] }), &[]),
            ("PUT", p) if p == format!("{AUTH}/verify/trusteddevice/securitycode") => {
                state.code_requested = true;
                answer(202, json!({}), &[])
            }
            ("POST", p) if p == format!("{AUTH}/verify/trusteddevice/securitycode") => {
                let good = body["securityCode"]["code"] == "123456" && state.code_requested && auth_headers_ok;
                answer(if good { 204 } else { 400 }, json!({}), &[])
            }
            ("GET", p) if p == format!("{AUTH}/2sv/trust") => {
                state.trusted.push("trust1".into());
                answer(204, json!({}), &[("X-Apple-TwoSV-Trust-Token", "trust1"), ("X-Apple-Session-Token", "st-trusted")])
            }
            ("POST", p) if p == format!("{SETUP}/accountLogin") => {
                if body["dsWebAuthToken"] != "st-trusted" {
                    return answer(421, json!({}), &[]);
                }
                state.counter += 1;
                state.session_cookie = format!("v{}", state.counter);
                let cookie = format!("{SESSION_COOKIE}={}; Domain=.icloud.com; Path=/; Max-Age=86400", state.session_cookie);
                answer(200, Self::services(), &[("Set-Cookie", &cookie), ("Set-Cookie", "X-APPLE-DS-WEB-SESSION-TOKEN=w; Path=/")])
            }
            ("POST", p) if p == format!("{SETUP}/validate") => {
                state.validations += 1;
                if Self::signed_in(&state, request) { answer(200, Self::services(), &[]) } else { answer(421, json!({}), &[]) }
            }
            _ if !Self::signed_in(&state, request) => answer(401, json!({}), &[]),
            _ if state.expire_once && path.starts_with(DRIVE) => {
                state.expire_once = false;
                state.session_cookie.clear();
                answer(421, json!({}), &[])
            }
            ("POST", p) if p == format!("{DRIVE}/retrieveItemDetailsInFolders") => {
                let folder = body[0]["drivewsid"].as_str().unwrap();
                let items: Vec<Value> = state
                    .entries
                    .iter()
                    .filter(|(_, e)| e.parent == folder)
                    .map(|(id, e)| {
                        json!({
                            "drivewsid": id, "etag": "e1", "name": e.name, "extension": e.extension,
                            "type": if e.folder { "FOLDER" } else { "FILE" }, "size": e.bytes.len(),
                        })
                    })
                    .collect();
                answer(200, json!([{ "drivewsid": folder, "items": items }]), &[])
            }
            ("POST", p) if p == format!("{DRIVE}/createFolders") => {
                state.counter += 1;
                let id = format!("FOLDER::com.apple.CloudDocs::f{}", state.counter);
                let parent = body["destinationDrivewsId"].as_str().unwrap().to_owned();
                assert!(body["folders"][0]["clientId"].as_str().unwrap().starts_with("FOLDER::UNKNOWN_ZONE::TempId-"));
                let name = body["folders"][0]["name"].as_str().unwrap().to_owned();
                state.entries.insert(id.clone(), Entry { parent, name, extension: String::new(), folder: true, bytes: Vec::new() });
                answer(200, json!({ "folders": [{ "drivewsid": id, "status": "OK" }] }), &[])
            }
            ("POST", p) if p == format!("{DRIVE}/moveItemsToTrash") => {
                let id = body["items"][0]["drivewsid"].as_str().unwrap();
                assert_eq!(body["items"][0]["etag"], "e1");
                state.entries.remove(id);
                answer(200, json!({}), &[])
            }
            ("POST", p) if p == format!("{DOCS}/ws/com.apple.CloudDocs/upload/web") => {
                assert_eq!(body["type"], "FILE");
                state.counter += 1;
                answer(200, json!([{ "url": format!("https://upload.example/{}", state.counter), "document_id": format!("doc{}", state.counter) }]), &[])
            }
            ("POST", p) if p.starts_with("https://upload.example/") => {
                let bytes = request.body.as_ref().unwrap().1.clone();
                let size = bytes.len();
                state.uploads.push(bytes);
                answer(200, json!({ "singleFile": { "receipt": format!("r{}", state.uploads.len() - 1), "fileChecksum": "fc", "referenceChecksum": "rc", "size": size, "wrappingKey": "wk" } }), &[])
            }
            ("POST", p) if p == format!("{DOCS}/ws/com.apple.CloudDocs/update/documents") => {
                assert_eq!(body["command"], "add_file");
                assert_eq!(body["data"]["signature"], "fc");
                assert_eq!(body["data"]["wrapping_key"], "wk");
                let receipt: usize = body["data"]["receipt"].as_str().unwrap()[1..].parse().unwrap();
                let id = format!("FILE::com.apple.CloudDocs::{}", body["document_id"].as_str().unwrap());
                let start = body["path"]["starting_document_id"].as_str().unwrap();
                let parent = state.entries.keys().find(|k| k.ends_with(&format!("::{start}"))).unwrap().clone();
                // iCloud splits off the extension and keeps the name in NFD.
                let full = body["path"]["path"].as_str().unwrap();
                let (name, extension) = full.rsplit_once('.').unwrap_or((full, ""));
                let entry = Entry {
                    parent,
                    name: name.nfd().collect(),
                    extension: extension.into(),
                    folder: false,
                    bytes: state.uploads[receipt].clone(),
                };
                state.entries.insert(id, entry);
                answer(200, json!({}), &[])
            }
            ("GET", p) if p == format!("{DOCS}/ws/com.apple.CloudDocs/download/by_id") => {
                let doc = query.strip_prefix("document_id=").unwrap();
                answer(200, json!({ "data_token": { "url": format!("https://download.example/{doc}") } }), &[])
            }
            ("GET", p) if p.starts_with("https://download.example/real/") => {
                let doc = &p["https://download.example/real/".len()..];
                let entry = &state.entries[&format!("FILE::com.apple.CloudDocs::{doc}")];
                Response { status: 200, body: entry.bytes.clone(), ..Response::default() }
            }
            ("GET", p) if p.starts_with("https://download.example/") => {
                let doc = &p["https://download.example/".len()..];
                answer(302, json!({}), &[("Location", &format!("/real/{doc}"))])
            }
            _ => answer(404, json!({}), &[]),
        }
    }
}

impl AsyncHttp for Apple {
    async fn send(&self, request: &Request) -> std::result::Result<Response, NetError> {
        Ok(self.handle(request))
    }
}

fn client(apple: &Apple) -> Client<Apple> {
    Client::new(apple.clone(), NetPolicy::Any)
}

fn config(trust: &str, cookies: &str) -> ICloudConfig {
    ICloudConfig {
        apple_id: USER.into(),
        password: PASSWORD.into(),
        trust_token: trust.into(),
        cookies: cookies.into(),
        folder: "Backups/WM Pass".into(),
        ..ICloudConfig::default()
    }
}

#[test]
fn signing_in_takes_the_password_then_the_code_and_keeps_the_trust_token() {
    let apple = Apple::new();
    let client = client(&apple);
    let outcome = block_on(start(&client, " Me@iCloud.com ", &PASSWORD.into(), "", NOW_MS));
    let ICloudOutcome::NeedsCode { pending, by_sms } = outcome else { panic!("{outcome:?}") };
    assert!(!by_sms);
    let wrong = block_on(finish(&client, pending, "000000"));
    assert!(matches!(wrong, ICloudOutcome::Failed(Fault::PermissionLost)), "{wrong:?}");

    let ICloudOutcome::NeedsCode { pending, .. } = block_on(start(&client, USER, &PASSWORD.into(), "", NOW_MS)) else { panic!() };
    let outcome = block_on(finish(&client, pending, "123 456"));
    let ICloudOutcome::SignedIn { trust_token, cookies } = outcome else { panic!("{outcome:?}") };
    assert_eq!(trust_token.expose(), "trust1");
    assert!(cookies.expose().contains("aasp=A1") && cookies.expose().contains(&format!("{SESSION_COOKIE}=v")), "{cookies:?}");

    // Trusted now: the next sign-in needs no code.
    let outcome = block_on(start(&client, USER, &PASSWORD.into(), "", NOW_MS));
    assert!(matches!(outcome, ICloudOutcome::NeedsCode { .. }), "a new session offers no trust token: {outcome:?}");
}

#[test]
fn a_wrong_password_is_refused() {
    let apple = Apple::new();
    let outcome = block_on(start(&client(&apple), USER, &"wrong".into(), "", NOW_MS));
    assert!(matches!(outcome, ICloudOutcome::Failed(Fault::PermissionLost)), "{outcome:?}");
}

#[test]
fn files_go_up_come_back_through_a_redirect_and_are_replaced_by_name() {
    let apple = Apple::new();
    apple.0.lock().unwrap().trusted.push("trust1".into());
    let updates = Arc::new(Mutex::new(Vec::new()));
    let seen = updates.clone();
    let icloud = ICloud::new(
        client(&apple),
        config("trust1", ""),
        NOW_MS,
        Box::new(move |update: &ICloudSessionUpdate| seen.lock().unwrap().push(update.cookies.expose().to_owned())),
    )
    .unwrap();
    block_on(icloud.readiness()).unwrap();
    assert_eq!(updates.lock().unwrap().len(), 1);
    assert!(updates.lock().unwrap()[0].contains(SESSION_COOKIE));

    let name = "Café vault.wmpass";
    let stored = block_on(icloud.write(name, b"first")).unwrap();
    assert!(stored.id.starts_with("FILE::com.apple.CloudDocs::doc"));
    let listed = block_on(icloud.list()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, name, "NFD from iCloud comes back as NFC");
    assert_eq!(listed[0].size, 5);
    assert_eq!(block_on(icloud.read(&listed[0])).unwrap(), b"first");

    block_on(icloud.write(name, b"second")).unwrap();
    let listed = block_on(icloud.list()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(block_on(icloud.read(&listed[0])).unwrap(), b"second");
    block_on(icloud.delete(&listed[0])).unwrap();
    block_on(icloud.delete(&listed[0])).unwrap();
    assert!(block_on(icloud.list()).unwrap().is_empty());
    // Two folders made, one inside the other.
    let state = apple.0.lock().unwrap();
    assert_eq!(state.entries.values().filter(|e| e.folder).count(), 3);
    assert_eq!(state.sign_ins, 1);
}

#[test]
fn a_dead_session_signs_in_again_once() {
    let apple = Apple::new();
    apple.0.lock().unwrap().trusted.push("trust1".into());
    // Stored cookies that no longer work: validate fails, so a sign-in follows.
    let icloud = ICloud::new(client(&apple), config("trust1", "X-APPLE-WEBAUTH-TOKEN=stale"), NOW_MS, Box::new(|_| {})).unwrap();
    block_on(icloud.readiness()).unwrap();
    {
        let state = apple.0.lock().unwrap();
        assert_eq!((state.validations, state.sign_ins), (1, 1));
    }
    apple.0.lock().unwrap().expire_once = true;
    block_on(icloud.write("v", b"x")).unwrap();
    assert_eq!(apple.0.lock().unwrap().sign_ins, 2);
}

#[test]
fn a_run_out_trust_token_needs_a_person() {
    let apple = Apple::new();
    let icloud = ICloud::new(client(&apple), config("expired", ""), NOW_MS, Box::new(|_| panic!("no session"))).unwrap();
    assert_eq!(block_on(icloud.readiness()), Err(Fault::PermissionLost));
    let unset = ICloud::new(client(&apple), ICloudConfig::default(), NOW_MS, Box::new(|_| {})).unwrap();
    assert_eq!(block_on(unset.readiness()), Err(Fault::NotConfigured));
}

#[test]
fn debug_output_hides_the_account() {
    let printed = format!("{:?}", config("trust1", "a=b"));
    assert!(!printed.contains("trust1") && !printed.contains(PASSWORD) && !printed.contains(USER) && !printed.contains("a=b"), "{printed}");
}
