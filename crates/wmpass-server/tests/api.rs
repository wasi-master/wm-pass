//! The real server on a real socket, driven by WM Pass's own client and by raw requests.
#![allow(missing_docs, clippy::unwrap_used)]

use std::net::SocketAddr;

use serde_json::{Value, json};
use wmpass_bw::account::{Account, register};
use wmpass_bw::api::{self, Device, Endpoints, Saved};
use wmpass_bw::crypto::{Kdf, MasterKey, SymKey, generate_key_pair, unwrap_with_private, wrap_to_public};
use wmpass_core::backend::Backend;
use wmpass_core::clock::Hlc;
use wmpass_core::model::Lww;
use wmpass_core::vault::login_entry;
use wmpass_net::http::NativeHttp;
use wmpass_net::{NetPolicy, Request};
use wmpass_server::{Config, Signups};

const KDF: Kdf = Kdf::Pbkdf2 { iterations: 100_000 };
const POLICY: NetPolicy = NetPolicy::Any;

struct Server {
    address: SocketAddr,
    endpoints: Endpoints,
    http: NativeHttp,
    _dir: tempfile::TempDir,
}

fn start(signups: Signups) -> Server {
    start_with(signups, None)
}

fn start_with(signups: Signups, public_url: Option<&str>) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let config = Config { data_dir: dir.path().to_owned(), database_url: None, signups, public_url: public_url.map(str::to_owned) };
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        runtime.block_on(async move {
            let app = wmpass_server::build(config).await.unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            sender.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    let address = receiver.recv().unwrap();
    Server { address, endpoints: Endpoints::self_hosted(&format!("http://{address}")), http: NativeHttp::default(), _dir: dir }
}

fn device(id: &str) -> Device {
    Device { identifier: id.into(), name: "Test".into(), kind: 8 }
}

impl Server {
    fn signup(&self, email: &str, password: &str) {
        register(&self.http, POLICY, &self.endpoints, email, "Name", password.as_bytes(), KDF).unwrap();
    }

    fn login(&self, email: &str, password: &str, id: &str) -> Account {
        Account::login(&self.http, POLICY, self.endpoints.clone(), email, password.as_bytes(), &device(id), None).unwrap()
    }

    /// A token and the account's keys, for raw requests.
    fn session(&self, email: &str, password: &str, id: &str) -> (String, SymKey) {
        let master = MasterKey::derive(password.as_bytes(), email, KDF).unwrap();
        let hash = master.login_hash(password.as_bytes());
        let tokens = api::login(&self.endpoints, email, &hash, &device(id), None).run(&self.http, POLICY).unwrap().unwrap();
        let key = master.stretch().unwrap().unwrap_key(tokens.key.as_deref().unwrap()).unwrap();
        (tokens.access.expose().to_owned(), key)
    }

    fn raw(&self, method: &'static str, path: &str, token: Option<&str>, body: Option<&Value>) -> (u16, Value) {
        let url = format!("http://{}{path}", self.address);
        let mut request = match body {
            Some(body) => Request::json(method, url, body),
            None => Request::verb(method, url),
        };
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = wmpass_net::Http::send(&self.http, &request).unwrap();
        (response.status, serde_json::from_slice(&response.body).unwrap_or(Value::Null))
    }
}

const AT: Hlc = Hlc { ms: 2_000_000_000_000, counter: 0, device: 1 };

#[test]
fn register_log_in_store_and_read_back_through_two_devices() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "correct horse");
    let mut phone = server.login("me@example.com", "correct horse", "phone");
    let id = phone.upsert(login_entry("Mail", "me", "s3cret", AT), AT.ms).unwrap();
    phone.push(&server.http, POLICY).unwrap();
    assert!(phone.pending().is_empty());

    let mut laptop = server.login("Me@Example.com ", "correct horse", "laptop");
    let got = laptop.entries(0);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].login().unwrap().password.v.expose(), "s3cret");
    assert_ne!(got[0].id, id, "the server assigns the real id");

    // An edit on one device reaches the other, and the old password stays in history.
    let mut edited = got[0].clone();
    let later = Hlc { ms: AT.ms + 5000, counter: 0, device: 2 };
    edited.set_password("n3wer".into(), later);
    laptop.upsert(edited, later.ms).unwrap();
    laptop.push(&server.http, POLICY).unwrap();
    phone.pull(&server.http, POLICY).unwrap();
    let seen = &phone.entries(0)[0];
    assert_eq!(seen.login().unwrap().password.v.expose(), "n3wer");
    assert!(seen.history_of("password").any(|r| r.value.expose() == "s3cret"));

    // Trash takes it out of both.
    let id = phone.entries(0)[0].id.clone();
    phone.remove(&id, AT.ms + 6000).unwrap();
    phone.push(&server.http, POLICY).unwrap();
    laptop.pull(&server.http, POLICY).unwrap();
    assert!(laptop.entries(0).is_empty());
}

#[test]
fn a_conflict_merges_instead_of_losing_either_side() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let mut a = server.login("me@example.com", "pw-pw-pw", "a");
    a.upsert(login_entry("Site", "me", "first", AT), AT.ms).unwrap();
    a.push(&server.http, POLICY).unwrap();
    let mut b = server.login("me@example.com", "pw-pw-pw", "b");

    // Both edit from the same copy, different fields, with the second a while later.
    let mut from_a = a.entries(0)[0].clone();
    let at_a = Hlc { ms: wmpass_core::clock::parse_date("2099-01-01T00:00:00Z").unwrap(), counter: 0, device: 1 };
    from_a.common.title = Lww::new("Title from A".into(), at_a);
    let mut from_b = b.entries(0)[0].clone();
    let at_b = Hlc { ms: at_a.ms + 1000, counter: 0, device: 2 };
    from_b.set_password("password from B".into(), at_b);

    a.upsert(from_a, at_a.ms).unwrap();
    a.push(&server.http, POLICY).unwrap();
    b.upsert(from_b, at_b.ms).unwrap();
    b.push(&server.http, POLICY).unwrap();

    a.pull(&server.http, POLICY).unwrap();
    let merged = &a.entries(0)[0];
    assert_eq!(merged.common.title.v, "Title from A");
    assert_eq!(merged.login().unwrap().password.v.expose(), "password from B");
}

#[test]
fn offline_unlock_works_from_the_cache_and_nothing_readable_is_in_it() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let mut account = server.login("me@example.com", "pw-pw-pw", "a");
    account.upsert(login_entry("Bank visible-title", "visible-user", "visible-pass", AT), AT.ms).unwrap();
    account.push(&server.http, POLICY).unwrap();
    account.upsert(login_entry("Queued visible-queued", "u", "visible-queued-pass", AT), AT.ms).unwrap();
    let cached = account.save().unwrap();
    let text = String::from_utf8_lossy(&cached);
    assert!(!text.contains("visible"), "plaintext reached the cache");

    let offline = Account::unlock_offline(server.endpoints.clone(), &cached, b"pw-pw-pw").unwrap();
    assert_eq!(offline.entries(0).len(), 2);
    assert_eq!(offline.pending().len(), 1, "the unsent edit survives a restart");
    assert_eq!(Account::unlock_offline(server.endpoints.clone(), &cached, b"wrong").err(), Some(wmpass_bw::BwError::BadLogin));
}

#[test]
fn one_user_cannot_touch_anothers_items_or_folders() {
    let server = start(Signups::Open);
    server.signup("alice@example.com", "alice-pass");
    server.signup("bob@example.com", "bob-pass-1");
    let (alice, key) = server.session("alice@example.com", "alice-pass", "a");
    let (bob, _) = server.session("bob@example.com", "bob-pass-1", "b");
    let cipher = json!({"type": 2, "name": key.encrypt(b"Private").unwrap(), "secureNote": {"type": 0}});
    let (status, created) = server.raw("POST", "/api/ciphers", Some(&alice), Some(&cipher));
    assert_eq!(status, 200);
    let id = created["id"].as_str().unwrap();
    let (_, folder) = server.raw("POST", "/api/folders", Some(&alice), Some(&json!({"name": key.encrypt(b"F").unwrap()})));
    let folder = folder["id"].as_str().unwrap();

    for (method, path, body) in [
        ("GET", format!("/api/ciphers/{id}"), None),
        ("PUT", format!("/api/ciphers/{id}"), Some(cipher.clone())),
        ("DELETE", format!("/api/ciphers/{id}"), None),
        ("PUT", format!("/api/ciphers/{id}/delete"), None),
        ("PUT", format!("/api/ciphers/{id}/restore"), None),
        ("PUT", format!("/api/ciphers/{id}/partial"), Some(json!({"favorite": true}))),
        ("GET", format!("/api/folders/{folder}"), None),
        ("PUT", format!("/api/folders/{folder}"), Some(json!({"name": "x"}))),
        ("DELETE", format!("/api/folders/{folder}"), None),
    ] {
        let (status, _) = server.raw(method, &path, Some(&bob), body.as_ref());
        assert_eq!(status, 404, "{method} {path}");
    }
    let (_, sync) = server.raw("GET", "/api/sync", Some(&bob), None);
    assert_eq!(sync["ciphers"].as_array().unwrap().len(), 0);
    assert_eq!(sync["folders"].as_array().unwrap().len(), 0);
    let (_, sync) = server.raw("GET", "/api/sync", Some(&alice), None);
    assert_eq!(sync["ciphers"].as_array().unwrap().len(), 1);
}

#[test]
fn a_client_cannot_claim_someone_elses_ownership_in_the_body() {
    let server = start(Signups::Open);
    server.signup("alice@example.com", "alice-pass");
    server.signup("bob@example.com", "bob-pass-1");
    let (alice, key) = server.session("alice@example.com", "alice-pass", "a");
    let (bob, _) = server.session("bob@example.com", "bob-pass-1", "b");
    let hostile = json!({"type": 2, "name": key.encrypt(b"x").unwrap(), "id": "11111111-1111-1111-1111-111111111111", "userId": "someone", "organizationId": null});
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&alice), Some(&hostile));
    assert_ne!(created["id"], "11111111-1111-1111-1111-111111111111");
    assert!(created.get("userId").is_none());
    // Joining an organisation by naming it fails for anyone who is not a member.
    let body = json!({"cipher": {"type": 2, "name": "2.x", "organizationId": "00000000-0000-0000-0000-000000000000"}, "collectionIds": ["c"]});
    let (status, _) = server.raw("POST", "/api/ciphers/create", Some(&bob), Some(&body));
    assert_eq!(status, 404);
}

#[test]
fn tokens_are_checked_and_die_with_the_password() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "first-pass");
    let (token, key) = server.session("me@example.com", "first-pass", "a");
    let (other, _) = server.session("me@example.com", "first-pass", "b");
    assert_eq!(server.raw("GET", "/api/sync", None, None).0, 401);
    assert_eq!(server.raw("GET", "/api/sync", Some("nonsense"), None).0, 401);
    let mut forged = token.clone();
    forged.pop();
    forged.push(if token.ends_with('A') { 'B' } else { 'A' });
    assert_eq!(server.raw("GET", "/api/sync", Some(&forged), None).0, 401);
    assert_eq!(server.raw("GET", "/api/sync", Some(&token), None).0, 200);

    let old = MasterKey::derive(b"first-pass", "me@example.com", KDF).unwrap();
    let new = MasterKey::derive(b"second-pass", "me@example.com", KDF).unwrap();
    let change = json!({
        "masterPasswordHash": new.login_hash(b"nope"),
        "newMasterPasswordHash": new.login_hash(b"second-pass"),
        "key": new.stretch().unwrap().encrypt(&key.to_bytes()).unwrap(),
    });
    assert_eq!(server.raw("POST", "/api/accounts/password", Some(&token), Some(&change)).0, 400, "wrong current password");
    let mut change = change;
    change["masterPasswordHash"] = json!(old.login_hash(b"first-pass"));
    assert_eq!(server.raw("POST", "/api/accounts/password", Some(&token), Some(&change)).0, 200);
    assert_eq!(server.raw("GET", "/api/sync", Some(&token), None).0, 401, "the stamp changed");
    assert_eq!(server.raw("GET", "/api/sync", Some(&other), None).0, 401);

    let login = api::login(&server.endpoints, "me@example.com", &old.login_hash(b"first-pass"), &device("c"), None);
    assert_eq!(login.run(&server.http, POLICY).unwrap().err(), Some(wmpass_bw::BwError::BadLogin));
    server.login_with_kdf_check("me@example.com", "second-pass");
}

impl Server {
    fn login_with_kdf_check(&self, email: &str, password: &str) {
        let account = self.login(email, password, "d");
        assert_eq!(account.email(), email);
    }
}

#[test]
fn a_revoked_device_loses_access_at_once() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (phone, _) = server.session("me@example.com", "pw-pw-pw", "phone");
    let (laptop, _) = server.session("me@example.com", "pw-pw-pw", "laptop");
    let (_, devices) = server.raw("GET", "/wm/devices", Some(&phone), None);
    let list = devices["data"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let other = list.iter().find(|d| d["current"] == false).unwrap()["id"].as_str().unwrap();
    assert_eq!(server.raw("GET", "/api/sync", Some(&laptop), None).0, 200);
    assert_eq!(server.raw("DELETE", &format!("/wm/devices/{other}"), Some(&phone), None).0, 200);
    assert_eq!(server.raw("GET", "/api/sync", Some(&laptop), None).0, 401);
}

#[test]
fn password_guessing_is_throttled_and_says_nothing_about_who_exists() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "right-pass");
    let wrong = MasterKey::derive(b"wrong", "me@example.com", KDF).unwrap().login_hash(b"wrong");
    let attempt = |email: &str| api::login(&server.endpoints, email, &wrong, &device("x"), None).run(&server.http, POLICY);
    for _ in 0..5 {
        assert_eq!(attempt("me@example.com").unwrap().err(), Some(wmpass_bw::BwError::BadLogin));
        assert_eq!(attempt("nobody@example.com").unwrap().err(), Some(wmpass_bw::BwError::BadLogin), "same answer for no account");
    }
    assert!(matches!(attempt("me@example.com"), Err(wmpass_net::NetError::RateLimited(_))));
    assert!(matches!(attempt("nobody@example.com"), Err(wmpass_net::NetError::RateLimited(_))));

    let (known, _) = (api::prelogin(&server.endpoints, "me@example.com").run(&server.http, POLICY).unwrap(), ());
    assert_eq!(known, KDF);
    let unknown = api::prelogin(&server.endpoints, "nobody@example.com").run(&server.http, POLICY).unwrap();
    assert!(matches!(unknown, Kdf::Pbkdf2 { .. }), "an unknown address still gets an answer");
}

#[test]
fn signups_follow_the_setting_and_weak_settings_are_refused() {
    let closed = start(Signups::Closed);
    let attempt = register(&closed.http, POLICY, &closed.endpoints, "me@example.com", "N", b"pw", KDF);
    assert!(attempt.is_err());
    let listed = start(Signups::Emails(vec!["ok@example.com".into()]));
    assert!(register(&listed.http, POLICY, &listed.endpoints, "other@example.com", "N", b"pw", KDF).is_err());
    assert!(register(&listed.http, POLICY, &listed.endpoints, "OK@example.com", "N", b"pw", KDF).is_ok());
    assert!(register(&listed.http, POLICY, &listed.endpoints, "ok@example.com", "N", b"pw", KDF).is_err(), "no second account");

    let open = start(Signups::Open);
    let weak = json!({"email": "x@example.com", "masterPasswordHash": "h", "key": "k", "kdf": 0, "kdfIterations": 1});
    assert_eq!(open.raw("POST", "/identity/accounts/register", None, Some(&weak)).0, 400);
}

#[test]
fn nothing_secret_is_in_the_database_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_owned();
    let server = {
        let config = Config { data_dir: path.clone(), database_url: None, signups: Signups::Open, public_url: None };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let app = wmpass_server::build(config).await.unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                sender.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        });
        let address = receiver.recv().unwrap();
        Server { address, endpoints: Endpoints::self_hosted(&format!("http://{address}")), http: NativeHttp::default(), _dir: tempfile::tempdir().unwrap() }
    };
    server.signup("me@example.com", "the master password");
    let mut account = server.login("me@example.com", "the master password", "a");
    account.upsert(login_entry("visible-title", "visible-user", "visible-pass", AT), AT.ms).unwrap();
    account.push(&server.http, POLICY).unwrap();
    let hash = MasterKey::derive(b"the master password", "me@example.com", KDF).unwrap().login_hash(b"the master password");
    let mut everything = Vec::new();
    for item in std::fs::read_dir(&path).unwrap() {
        everything.extend(std::fs::read(item.unwrap().path()).unwrap_or_default());
    }
    assert!(everything.len() > 1000);
    let text = String::from_utf8_lossy(&everything);
    for secret in ["visible-title", "visible-user", "visible-pass", "the master password", hash.as_str()] {
        assert!(!text.contains(secret), "{secret} reached the disk");
    }
}

#[test]
fn an_organisation_shares_with_a_confirmed_member_only() {
    let server = start(Signups::Open);
    server.signup("owner@example.com", "owner-pass");
    server.signup("member@example.com", "member-pass");
    server.signup("outsider@example.com", "outsider-pw");
    let (owner, owner_key) = server.session("owner@example.com", "owner-pass", "o");
    let (member, member_key) = server.session("member@example.com", "member-pass", "m");
    let (outsider, _) = server.session("outsider@example.com", "outsider-pw", "x");

    // The owner makes the organisation and its key, wrapped to their own public key.
    let (_, profile) = server.raw("GET", "/api/accounts/profile", Some(&owner), None);
    let owner_private = owner_key.decrypt(profile["privateKey"].as_str().unwrap()).unwrap();
    let owner_public = server.raw("GET", &format!("/api/users/{}/public-key", profile["id"].as_str().unwrap()), Some(&owner), None).1;
    let org_key = SymKey::generate().unwrap();
    let wrapped = wrap_to_public(owner_public["publicKey"].as_str().unwrap(), &org_key).unwrap();
    assert_eq!(unwrap_with_private(&owner_private, &wrapped).unwrap(), org_key);
    let (status, org) = server.raw("POST", "/api/organizations", Some(&owner), Some(&json!({"name": "Family", "key": wrapped})));
    assert_eq!(status, 200);
    let org = org["id"].as_str().unwrap().to_owned();
    let (_, collection) = server.raw("POST", &format!("/api/organizations/{org}/collections"), Some(&owner), Some(&json!({"name": org_key.encrypt(b"Shared").unwrap()})));
    let collection = collection["id"].as_str().unwrap().to_owned();
    let (_, secret_collection) = server.raw("POST", &format!("/api/organizations/{org}/collections"), Some(&owner), Some(&json!({"name": org_key.encrypt(b"Parents").unwrap()})));
    let secret_collection = secret_collection["id"].as_str().unwrap().to_owned();

    // An item in each.
    let item = |name: &[u8], collection: &str| json!({
        "cipher": {"type": 2, "name": org_key.encrypt(name).unwrap(), "secureNote": {"type": 0}, "organizationId": org},
        "collectionIds": [collection],
    });
    let (status, shared) = server.raw("POST", "/api/ciphers/create", Some(&owner), Some(&item(b"Wi-Fi", &collection)));
    assert_eq!(status, 200);
    let shared = shared["id"].as_str().unwrap().to_owned();
    let (_, hidden) = server.raw("POST", "/api/ciphers/create", Some(&owner), Some(&item(b"Parents only", &secret_collection)));
    let hidden = hidden["id"].as_str().unwrap().to_owned();

    // Inviting is not access. Nothing reaches the member before an owner confirms.
    let invite = json!({"emails": ["member@example.com"], "type": 2, "collections": [{"id": collection}]});
    assert_eq!(server.raw("POST", &format!("/api/organizations/{org}/users/invite"), Some(&member), Some(&invite)).0, 404, "members cannot invite");
    assert_eq!(server.raw("POST", &format!("/api/organizations/{org}/users/invite"), Some(&owner), Some(&invite)).0, 200);
    assert_eq!(server.raw("GET", &format!("/api/ciphers/{shared}"), Some(&member), None).0, 404);

    let (_, users) = server.raw("GET", &format!("/api/organizations/{org}/users"), Some(&owner), None);
    let record = users["data"].as_array().unwrap().iter().find(|u| u["email"] == "member@example.com").unwrap().clone();
    assert_eq!(record["status"], 1);
    let their_public = server.raw("GET", &format!("/api/users/{}/public-key", record["userId"].as_str().unwrap()), Some(&owner), None).1;
    let for_member = wrap_to_public(their_public["publicKey"].as_str().unwrap(), &org_key).unwrap();
    let confirm = format!("/api/organizations/{org}/users/{}/confirm", record["id"].as_str().unwrap());
    assert_eq!(server.raw("POST", &confirm, Some(&outsider), Some(&json!({"key": for_member}))).0, 404, "outsiders cannot confirm themselves");
    assert_eq!(server.raw("POST", &confirm, Some(&owner), Some(&json!({"key": for_member}))).0, 200);

    // Now the member reads the shared item and only that one, with the key from their profile.
    let (_, sync) = server.raw("GET", "/api/sync", Some(&member), None);
    let ciphers = sync["ciphers"].as_array().unwrap();
    assert_eq!(ciphers.len(), 1);
    assert_eq!(ciphers[0]["id"], shared.as_str());
    let member_private = member_key.decrypt(sync["profile"]["privateKey"].as_str().unwrap()).unwrap();
    let got_key = unwrap_with_private(&member_private, sync["profile"]["organizations"][0]["key"].as_str().unwrap()).unwrap();
    assert_eq!(&got_key.decrypt(ciphers[0]["name"].as_str().unwrap()).unwrap()[..], b"Wi-Fi");
    assert_eq!(server.raw("GET", &format!("/api/ciphers/{hidden}"), Some(&member), None).0, 404);
    assert_eq!(server.raw("GET", &format!("/api/ciphers/{shared}"), Some(&outsider), None).0, 404);
    let sneaky = item(b"x", &secret_collection);
    assert_eq!(server.raw("POST", "/api/ciphers/create", Some(&member), Some(&sneaky)).0, 400, "not assigned to that collection");

    // Roles cannot be escalated, and the last owner stays.
    let promote = json!({"emails": ["outsider@example.com"], "type": 0});
    assert_eq!(server.raw("POST", &format!("/api/organizations/{org}/users/invite"), Some(&member), Some(&promote)).0, 404);

    // WM Pass's own client reads organisation items too.
    let account = server.login("member@example.com", "member-pass", "m2");
    assert_eq!(account.organisations().len(), 1);
    assert_eq!(account.entries(0)[0].common.title.v, "Wi-Fi");
    assert_eq!(account.unreadable(), 0);

    // Removing a member cuts access on the server.
    assert_eq!(server.raw("DELETE", &format!("/api/organizations/{org}/users/{}", record["id"].as_str().unwrap()), Some(&owner), None).0, 200);
    assert_eq!(server.raw("GET", &format!("/api/ciphers/{shared}"), Some(&member), None).0, 404);
}

#[test]
fn stale_edits_are_refused_so_clients_can_merge() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let base = json!({"type": 2, "name": key.encrypt(b"x").unwrap()});
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&token), Some(&base));
    let id = created["id"].as_str().unwrap();
    let mut stale = base.clone();
    stale["lastKnownRevisionDate"] = json!("2001-01-01T00:00:00.000Z");
    let (status, answer) = server.raw("PUT", &format!("/api/ciphers/{id}"), Some(&token), Some(&stale));
    assert_eq!(status, 400);
    assert!(answer["message"].as_str().unwrap().contains("out of date"));
    let update = api::update_cipher(&server.endpoints, &token.as_str().into(), id, &stale);
    assert_eq!(update.run(&server.http, POLICY).unwrap(), Saved::Conflict);
}

#[test]
fn shared_files_reach_members_only_and_refuse_stale_writes() {
    let server = start(Signups::Open);
    server.signup("owner@example.com", "owner-pass");
    server.signup("member@example.com", "member-pass");
    server.signup("outsider@example.com", "outsider-pw");
    let (owner, _) = server.session("owner@example.com", "owner-pass", "o");
    let (member, _) = server.session("member@example.com", "member-pass", "m");
    let (outsider, _) = server.session("outsider@example.com", "outsider-pw", "x");
    let (_, file) = server.raw("POST", "/wm/shared", Some(&owner), Some(&json!({"body": "c2VhbGVk"})));
    let id = file["id"].as_str().unwrap().to_owned();
    let revision = file["revision"].as_i64().unwrap();
    assert_eq!(server.raw("GET", &format!("/wm/shared/{id}"), Some(&member), None).0, 404);
    assert_eq!(server.raw("POST", &format!("/wm/shared/{id}/members"), Some(&member), Some(&json!({"email": "member@example.com"}))).0, 404);
    let (_, added) = server.raw("POST", &format!("/wm/shared/{id}/members"), Some(&owner), Some(&json!({"email": "member@example.com"})));
    assert!(added["publicKey"].is_string());
    let (_, read) = server.raw("GET", &format!("/wm/shared/{id}"), Some(&member), None);
    assert_eq!(read["body"], "c2VhbGVk");
    assert_eq!(server.raw("PUT", &format!("/wm/shared/{id}"), Some(&outsider), Some(&json!({"body": "x", "revision": revision}))).0, 404);
    assert_eq!(server.raw("PUT", &format!("/wm/shared/{id}"), Some(&member), Some(&json!({"body": "x", "revision": revision - 1}))).0, 400);
    assert_eq!(server.raw("PUT", &format!("/wm/shared/{id}"), Some(&member), Some(&json!({"body": "y", "revision": revision}))).0, 200);
    assert_eq!(server.raw("DELETE", &format!("/wm/shared/{id}"), Some(&member), None).0, 404);
    let user = added["userId"].as_str().unwrap();
    assert_eq!(server.raw("DELETE", &format!("/wm/shared/{id}/members/{user}"), Some(&owner), None).0, 200);
    assert_eq!(server.raw("GET", &format!("/wm/shared/{id}"), Some(&member), None).0, 404);
}

#[test]
fn live_pushes_reach_other_devices_in_both_protocols() {
    use futures_util_ws::*;
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (phone, key) = server.session("me@example.com", "pw-pw-pw", "phone");
    let (laptop, _) = server.session("me@example.com", "pw-pw-pw", "laptop");
    let (stranger_sent, _) = (start_ws(&server, "nonsense"), ());
    assert!(stranger_sent.is_err(), "a bad token gets no socket");

    let (mut socket, _) = start_ws(&server, &laptop).unwrap();
    send_text(&mut socket, "{\"protocol\":\"json\",\"version\":1}\u{1e}");
    assert_eq!(recv_text(&mut socket), "{}\u{1e}");
    let cipher = json!({"type": 2, "name": key.encrypt(b"x").unwrap()});
    assert_eq!(server.raw("POST", "/api/ciphers", Some(&phone), Some(&cipher)).0, 200);
    let message = recv_text_skipping_pings(&mut socket);
    let parsed: Value = serde_json::from_str(message.trim_end_matches('\u{1e}')).unwrap();
    assert_eq!(parsed["target"], "ReceiveMessage");
    assert_eq!(parsed["arguments"][0]["Type"], 5);

    let (mut socket, _) = start_ws(&server, &laptop).unwrap();
    send_text(&mut socket, "{\"protocol\":\"messagepack\",\"version\":1}\u{1e}");
    assert_eq!(recv_text(&mut socket), "{}\u{1e}");
    assert_eq!(server.raw("POST", "/api/ciphers", Some(&phone), Some(&cipher)).0, 200);
    let bytes = recv_binary_skipping_pings(&mut socket);
    // A varint length, then [1, {}, nil, "ReceiveMessage", [..]].
    let (mut length, mut start) = (0usize, 0usize);
    loop {
        length |= usize::from(bytes[start] & 0x7f) << (7 * start);
        start += 1;
        if bytes[start - 1] & 0x80 == 0 {
            break;
        }
    }
    assert_eq!(start + length, bytes.len(), "{bytes:?}");
    assert_eq!(bytes[start], 0x95);
    assert_eq!(bytes[start + 1], 1);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("ReceiveMessage") && text.contains("UserId"));
}

mod futures_util_ws {
    use super::*;
    use tokio_tungstenite::tungstenite::{self, Message};

    pub type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

    pub fn start_ws(server: &Server, token: &str) -> Result<(Socket, ()), String> {
        let url = format!("ws://{}/notifications/hub?access_token={token}", server.address);
        tungstenite::connect(url).map(|(socket, _)| (socket, ())).map_err(|e| e.to_string())
    }

    pub fn send_text(socket: &mut Socket, text: &str) {
        socket.send(Message::text(text)).unwrap();
    }

    pub fn recv_text(socket: &mut Socket) -> String {
        loop {
            if let Message::Text(text) = socket.read().unwrap() {
                return text.to_string();
            }
        }
    }

    pub fn recv_text_skipping_pings(socket: &mut Socket) -> String {
        loop {
            let text = recv_text(socket);
            if !text.starts_with("{\"type\":6}") {
                return text;
            }
        }
    }

    pub fn recv_binary_skipping_pings(socket: &mut Socket) -> Vec<u8> {
        loop {
            if let Message::Binary(bytes) = socket.read().unwrap()
                && bytes.as_ref() != [2u8, 0x91, 0x06]
            {
                return bytes.to_vec();
            }
        }
    }
}

#[test]
fn generate_key_pair_is_used_at_registration() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let (_, profile) = server.raw("GET", "/api/accounts/profile", Some(&token), None);
    assert!(key.decrypt(profile["privateKey"].as_str().unwrap()).is_ok());
    // Keys cannot be swapped once set, or a stolen session could intercept organisation keys.
    let (public_key, private_key) = generate_key_pair(&key).unwrap();
    let swap = json!({"publicKey": public_key, "encryptedPrivateKey": private_key});
    assert_eq!(server.raw("POST", "/api/accounts/keys", Some(&token), Some(&swap)).0, 400);
}

#[test]
fn guesses_sent_all_at_once_are_held_to_the_same_limit() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "right-pass");
    let wrong = MasterKey::derive(b"wrong", "me@example.com", KDF).unwrap().login_hash(b"wrong");
    let answers: Vec<_> = std::thread::scope(|scope| {
        let guesses: Vec<_> = (0..12)
            .map(|_| scope.spawn(|| api::login(&server.endpoints, "me@example.com", &wrong, &device("x"), None).run(&NativeHttp::default(), POLICY)))
            .collect();
        guesses.into_iter().map(|guess| guess.join().unwrap()).collect()
    });
    let judged = answers.iter().filter(|answer| matches!(answer, Ok(Err(wmpass_bw::BwError::BadLogin)))).count();
    let refused = answers.iter().filter(|answer| matches!(answer, Err(wmpass_net::NetError::RateLimited(_)))).count();
    assert_eq!((judged, refused), (5, 7));
}

#[test]
fn of_two_edits_from_the_same_copy_only_one_lands() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let base = json!({"type": 2, "name": key.encrypt(b"x").unwrap()});
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&token), Some(&base));
    let id = created["id"].as_str().unwrap().to_owned();
    let path = format!("/api/ciphers/{id}");
    let statuses: Vec<u16> = std::thread::scope(|scope| {
        let edits: Vec<_> = (0..8)
            .map(|n| {
                let mut edit = json!({"type": 2, "name": key.encrypt(format!("edit {n}").as_bytes()).unwrap()});
                edit["lastKnownRevisionDate"] = created["revisionDate"].clone();
                let (server, token, path) = (&server, &token, &path);
                scope.spawn(move || server.raw("PUT", path, Some(token), Some(&edit)).0)
            })
            .collect();
        edits.into_iter().map(|edit| edit.join().unwrap()).collect()
    });
    assert_eq!(statuses.iter().filter(|&&s| s == 200).count(), 1, "{statuses:?}");
    assert_eq!(statuses.iter().filter(|&&s| s == 400).count(), 7, "{statuses:?}");
}

#[test]
fn moving_one_item_from_several_requests_at_once_never_fails() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&token), Some(&json!({"type": 2, "name": key.encrypt(b"x").unwrap()})));
    let path = format!("/api/ciphers/{}/partial", created["id"].as_str().unwrap());
    for _ in 0..5 {
        let statuses: Vec<u16> = std::thread::scope(|scope| {
            let moves: Vec<_> = (0..8)
                .map(|n| {
                    let (server, token, path) = (&server, &token, &path);
                    scope.spawn(move || server.raw("PUT", path, Some(token), Some(&json!({"favorite": n % 2 == 0}))).0)
                })
                .collect();
            moves.into_iter().map(|one| one.join().unwrap()).collect()
        });
        assert!(statuses.iter().all(|&s| s == 200), "{statuses:?}");
    }
}

#[test]
fn a_shared_file_write_with_an_absurd_revision_is_refused_not_crashed() {
    let server = start(Signups::Open);
    server.signup("owner@example.com", "owner-pass");
    let (owner, _) = server.session("owner@example.com", "owner-pass", "o");
    let (_, file) = server.raw("POST", "/wm/shared", Some(&owner), Some(&json!({"body": "c2VhbGVk"})));
    let id = file["id"].as_str().unwrap();
    let (status, _) = server.raw("PUT", &format!("/wm/shared/{id}"), Some(&owner), Some(&json!({"body": "x", "revision": i64::MAX})));
    assert_eq!(status, 400);
}

#[test]
fn purging_is_throttled_like_logging_in() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, _) = server.session("me@example.com", "pw-pw-pw", "a");
    let wrong = json!({"masterPasswordHash": "not the hash"});
    for _ in 0..5 {
        assert_eq!(server.raw("POST", "/api/ciphers/purge", Some(&token), Some(&wrong)).0, 400);
    }
    assert_eq!(server.raw("POST", "/api/ciphers/purge", Some(&token), Some(&wrong)).0, 429);
}

#[test]
fn revoking_a_device_closes_its_live_socket_and_no_other() {
    use futures_util_ws::*;
    use tokio_tungstenite::tungstenite::{self, Message};
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (phone, key) = server.session("me@example.com", "pw-pw-pw", "phone");
    let (laptop, _) = server.session("me@example.com", "pw-pw-pw", "laptop");
    let (tablet, _) = server.session("me@example.com", "pw-pw-pw", "tablet");
    let open = |token: &str| {
        let (mut socket, _) = start_ws(&server, token).unwrap();
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        }
        send_text(&mut socket, "{\"protocol\":\"json\",\"version\":1}\u{1e}");
        assert_eq!(recv_text(&mut socket), "{}\u{1e}");
        socket
    };
    let (mut phone_socket, mut laptop_socket) = (open(&phone), open(&laptop));
    let kind = |message: &str| serde_json::from_str::<Value>(message.trim_end_matches('\u{1e}')).unwrap()["arguments"][0]["Type"].clone();

    // Revoke the laptop from the tablet, so neither socket hears about it as its own change.
    let (_, from_tablet) = server.raw("GET", "/wm/devices", Some(&tablet), None);
    let (_, from_laptop) = server.raw("GET", "/wm/devices", Some(&laptop), None);
    let laptop_id = from_laptop["data"].as_array().unwrap().iter().find(|d| d["current"] == true).unwrap()["id"].as_str().unwrap().to_owned();
    assert_eq!(from_tablet["data"].as_array().unwrap().len(), 3);
    assert_eq!(server.raw("DELETE", &format!("/wm/devices/{laptop_id}"), Some(&tablet), None).0, 200);

    // The laptop is told to log out, then its socket ends.
    assert_eq!(kind(&recv_text_skipping_pings(&mut laptop_socket)), 11);
    loop {
        match laptop_socket.read() {
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(Message::Text(text)) => assert!(text.starts_with("{\"type\":6}"), "nothing but pings after the logout: {text}"),
            Ok(_) => {}
        }
    }
    assert!(start_ws(&server, &laptop).is_err(), "a revoked device cannot reconnect");

    // The phone hears nothing about the revoke and still gets the next change.
    let cipher = json!({"type": 2, "name": key.encrypt(b"x").unwrap()});
    assert_eq!(server.raw("POST", "/api/ciphers", Some(&tablet), Some(&cipher)).0, 200);
    assert_eq!(kind(&recv_text_skipping_pings(&mut phone_socket)), 5);
}

#[test]
fn two_owners_removing_each_other_at_once_leave_one_owner() {
    server_with_two_owners_racing(5);
}

fn server_with_two_owners_racing(rounds: usize) {
    let server = start(Signups::Open);
    server.signup("a@example.com", "a-pass-pass");
    server.signup("b@example.com", "b-pass-pass");
    let (a, _) = server.session("a@example.com", "a-pass-pass", "a");
    let (b, _) = server.session("b@example.com", "b-pass-pass", "b");
    for round in 0..rounds {
        let (status, org) = server.raw("POST", "/api/organizations", Some(&a), Some(&json!({"name": format!("Org {round}"), "key": "2.x|y|z"})));
        assert_eq!(status, 200);
        let org = org["id"].as_str().unwrap().to_owned();
        let invite = json!({"emails": ["b@example.com"], "type": 0});
        assert_eq!(server.raw("POST", &format!("/api/organizations/{org}/users/invite"), Some(&a), Some(&invite)).0, 200);
        let (_, users) = server.raw("GET", &format!("/api/organizations/{org}/users"), Some(&a), None);
        let users = users["data"].as_array().unwrap().clone();
        let id_of = |email: &str| users.iter().find(|u| u["email"] == email).unwrap()["id"].as_str().unwrap().to_owned();
        let (a_member, b_member) = (id_of("a@example.com"), id_of("b@example.com"));
        assert_eq!(server.raw("POST", &format!("/api/organizations/{org}/users/{b_member}/confirm"), Some(&a), Some(&json!({"key": "4.wrapped"}))).0, 200);

        let barrier = std::sync::Barrier::new(2);
        let statuses: Vec<u16> = std::thread::scope(|scope| {
            let removals: Vec<_> = [(&a, &b_member), (&b, &a_member)]
                .into_iter()
                .map(|(token, target)| {
                    let (server, barrier, org) = (&server, &barrier, &org);
                    scope.spawn(move || {
                        barrier.wait();
                        server.raw("DELETE", &format!("/api/organizations/{org}/users/{target}"), Some(token), None).0
                    })
                })
                .collect();
            removals.into_iter().map(|one| one.join().unwrap()).collect()
        });
        assert_eq!(statuses.iter().filter(|&&s| s == 200).count(), 1, "round {round}: {statuses:?}");
        let (status_a, left_a) = server.raw("GET", &format!("/api/organizations/{org}/users"), Some(&a), None);
        let (status_b, left_b) = server.raw("GET", &format!("/api/organizations/{org}/users"), Some(&b), None);
        let left = if status_a == 200 { left_a } else { left_b };
        assert_eq!(u8::from(status_a == 200) + u8::from(status_b == 200), 1, "round {round}: exactly one of them is still in");
        let owners = left["data"].as_array().unwrap().iter().filter(|u| u["type"] == 0 && u["status"] == 2).count();
        assert_eq!(owners, 1, "round {round}: {left}");
    }
}

#[test]
fn every_route_including_shared_files_is_capped_at_32_mib() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, _) = server.session("me@example.com", "pw-pw-pw", "a");
    let big = "A".repeat(31 * 1024 * 1024);
    assert_eq!(server.raw("POST", "/wm/shared", Some(&token), Some(&json!({"body": big}))).0, 200);
    // Over the cap the server answers 413, or drops the connection before the upload ends.
    let too_big = Request::json("POST", format!("http://{}/wm/shared", server.address), &json!({"body": "A".repeat(33 * 1024 * 1024)}))
        .header("Authorization", format!("Bearer {token}"));
    match wmpass_net::Http::send(&server.http, &too_big) {
        Ok(response) => assert_eq!(response.status, 413),
        Err(error) => eprintln!("the oversized upload was cut off: {error}"),
    }
    assert_eq!(server.raw("GET", "/wm/shared", Some(&token), None).1["data"].as_array().unwrap().len(), 1, "nothing over the cap is stored");
}

/// A GET with a chosen `Host` header, which the HTTP client would not let a test forge.
fn get_with_host(server: &Server, path: &str, host: &str) -> Value {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(server.address).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\nX-Forwarded-Proto: https\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    serde_json::from_str(body).unwrap()
}

#[test]
fn config_advertises_the_public_url_when_set_and_the_host_otherwise() {
    let server = start_with(Signups::Closed, Some("https://pass.example.com/"));
    let config = get_with_host(&server, "/api/config", "evil.example");
    assert_eq!(config["environment"]["vault"], "https://pass.example.com");
    assert_eq!(config["environment"]["api"], "https://pass.example.com/api");
    assert_eq!(config["environment"]["identity"], "https://pass.example.com/identity");
    assert_eq!(config["environment"]["notifications"], "https://pass.example.com/notifications");

    let server = start(Signups::Closed);
    let config = get_with_host(&server, "/api/config", "pass.local:8443");
    assert_eq!(config["environment"]["vault"], "https://pass.local:8443");
    assert_eq!(config["environment"]["api"], "https://pass.local:8443/api");
}

/// Runs one statement on the server's own database, to set up what a clock set back or
/// requests finishing out of order would leave.
fn poke(server: &Server, statement: String) {
    let url = format!("sqlite://{}?mode=rw", server._dir.path().join("wmpass.sqlite3").display());
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        sqlx::any::install_default_drivers();
        let pool = sqlx::AnyPool::connect(&url).await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement)).execute(&pool).await.unwrap();
        pool.close().await;
    });
}

#[test]
fn revisions_only_move_forward_whatever_the_clock_says() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&token), Some(&json!({"type": 2, "name": key.encrypt(b"x").unwrap()})));
    let id = created["id"].as_str().unwrap().to_owned();
    // As if the last change had been stamped by a clock since set back by a day.
    let ahead = wmpass_core::clock::parse_date(created["revisionDate"].as_str().unwrap()).unwrap() + 86_400_000;
    poke(&server, format!("UPDATE ciphers SET revision = {ahead}"));
    poke(&server, format!("UPDATE users SET revision = {ahead}"));
    let stale_copy = wmpass_core::clock::format_date(ahead);

    // Trashing moves the item's revision on, so the copy from before cannot be written over it.
    assert_eq!(server.raw("PUT", &format!("/api/ciphers/{id}/delete"), Some(&token), None).0, 200);
    let mut edit = json!({"type": 2, "name": key.encrypt(b"from before the trash").unwrap()});
    edit["lastKnownRevisionDate"] = json!(stale_copy);
    let (status, _) = server.raw("PUT", &format!("/api/ciphers/{id}"), Some(&token), Some(&edit));
    assert_eq!(status, 400, "an edit from before the trash is out of date");

    // And the account's revision moved on too, so other devices know to sync.
    let (_, revision) = server.raw("GET", "/api/accounts/revision-date", Some(&token), None);
    assert!(revision.as_i64().unwrap() > i64::try_from(ahead).unwrap());
}

#[test]
fn sharing_an_old_copy_is_refused_like_editing_one() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let (_, org) = server.raw("POST", "/api/organizations", Some(&token), Some(&json!({"name": "Family", "key": "2.x|y|z"})));
    let org = org["id"].as_str().unwrap().to_owned();
    let (_, collection) = server.raw("POST", &format!("/api/organizations/{org}/collections"), Some(&token), Some(&json!({"name": "2.a|b|c"})));
    let collection = collection["id"].as_str().unwrap().to_owned();
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&token), Some(&json!({"type": 2, "name": key.encrypt(b"x").unwrap()})));
    let id = created["id"].as_str().unwrap().to_owned();
    // Another device edits it.
    let mut edit = json!({"type": 2, "name": key.encrypt(b"edited elsewhere").unwrap()});
    edit["lastKnownRevisionDate"] = created["revisionDate"].clone();
    assert_eq!(server.raw("PUT", &format!("/api/ciphers/{id}"), Some(&token), Some(&edit)).0, 200);
    // This device still holds the first copy and shares it.
    let share = |cipher_name: &str, known: &Value| {
        json!({
            "cipher": {"type": 2, "name": cipher_name, "organizationId": org, "lastKnownRevisionDate": known},
            "collectionIds": [collection],
        })
    };
    let (status, answer) = server.raw("PUT", &format!("/api/ciphers/{id}/share"), Some(&token), Some(&share("2.old|old|old", &created["revisionDate"])));
    assert_eq!(status, 400);
    assert!(answer["message"].as_str().unwrap().contains("out of date"));
    let (_, now) = server.raw("GET", &format!("/api/ciphers/{id}"), Some(&token), None);
    assert_eq!(now["organizationId"], Value::Null, "nothing changed");
    assert_eq!(server.raw("PUT", &format!("/api/ciphers/{id}/share"), Some(&token), Some(&share("2.new|new|new", &now["revisionDate"]))).0, 200);
}

#[test]
fn an_item_moved_into_a_folder_deleted_meanwhile_shows_no_folder() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let (_, folder) = server.raw("POST", "/api/folders", Some(&token), Some(&json!({"name": "2.f|f|f"})));
    let folder = folder["id"].as_str().unwrap().to_owned();
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&token), Some(&json!({"type": 2, "name": key.encrypt(b"x").unwrap()})));
    let id = created["id"].as_str().unwrap().to_owned();
    assert_eq!(server.raw("DELETE", &format!("/api/folders/{folder}"), Some(&token), None).0, 200);
    // The move checked the folder before the delete and wrote after it.
    poke(&server, format!("UPDATE cipher_user SET folder_id = '{folder}' WHERE cipher_id = '{id}'"));
    let (_, item) = server.raw("GET", &format!("/api/ciphers/{id}"), Some(&token), None);
    assert_eq!(item["folderId"], Value::Null);
    let (_, sync) = server.raw("GET", "/api/sync", Some(&token), None);
    assert_eq!(sync["ciphers"][0]["folderId"], Value::Null);
}

#[test]
fn adding_the_same_member_twice_at_once_never_fails() {
    let server = start(Signups::Open);
    server.signup("owner@example.com", "owner-pass");
    server.signup("member@example.com", "member-pass");
    let (owner, _) = server.session("owner@example.com", "owner-pass", "o");
    for _ in 0..5 {
        let (_, file) = server.raw("POST", "/wm/shared", Some(&owner), Some(&json!({"body": "c2VhbGVk"})));
        let path = format!("/wm/shared/{}/members", file["id"].as_str().unwrap());
        let statuses: Vec<u16> = std::thread::scope(|scope| {
            let adds: Vec<_> = (0..6)
                .map(|_| {
                    let (server, owner, path) = (&server, &owner, &path);
                    scope.spawn(move || server.raw("POST", path, Some(owner), Some(&json!({"email": "member@example.com"}))).0)
                })
                .collect();
            adds.into_iter().map(|add| add.join().unwrap()).collect()
        });
        assert!(statuses.iter().all(|&s| s == 200), "{statuses:?}");
    }
}

#[test]
fn moving_an_item_or_starring_it_tells_devices_that_poll() {
    let server = start(Signups::Open);
    server.signup("me@example.com", "pw-pw-pw");
    let (token, key) = server.session("me@example.com", "pw-pw-pw", "a");
    let (_, created) = server.raw("POST", "/api/ciphers", Some(&token), Some(&json!({"type": 2, "name": key.encrypt(b"x").unwrap()})));
    let id = created["id"].as_str().unwrap().to_owned();
    let revision = |server: &Server| server.raw("GET", "/api/accounts/revision-date", Some(&token), None).1.as_i64().unwrap();
    let before = revision(&server);
    assert_eq!(server.raw("PUT", &format!("/api/ciphers/{id}/partial"), Some(&token), Some(&json!({"favorite": true}))).0, 200);
    let starred = revision(&server);
    assert!(starred > before);
    assert_eq!(server.raw("PUT", "/api/ciphers/move", Some(&token), Some(&json!({"ids": [id], "folderId": null}))).0, 200);
    assert!(revision(&server) > starred);
}

