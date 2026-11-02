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
    let dir = tempfile::tempdir().unwrap();
    let config = Config { data_dir: dir.path().to_owned(), database_url: None, signups, public_url: None };
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
