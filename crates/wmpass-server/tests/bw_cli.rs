//! The official Bitwarden CLI against this server. It shows the wire format and the
//! crypto agree with Bitwarden's own client, in both directions. The CLI refuses plain http,
//! so it connects over TLS with a certificate made for the test.
//!
//! ```bash
//! npm install --prefix /tmp/bw @bitwarden/cli
//! BW_CLI=/tmp/bw/node_modules/.bin/bw cargo test -p wmpass-server --test bw_cli -- --ignored --nocapture
//! ```
#![allow(missing_docs, clippy::unwrap_used)]

use std::process::Command;

use serde_json::{Value, json};
use wmpass_bw::account::{Account, register};
use wmpass_bw::api::{Device, Endpoints};
use wmpass_bw::crypto::Kdf;
use wmpass_core::backend::Backend;
use wmpass_core::clock::Hlc;
use wmpass_core::model::{Kind, Lww, Note};
use wmpass_core::otp::OtpConfig;
use wmpass_core::vault::login_entry;
use wmpass_net::NetPolicy;
use wmpass_net::http::NativeHttp;
use wmpass_server::{Config, Signups};

const POLICY: NetPolicy = NetPolicy::Any;

struct Bw {
    path: String,
    home: tempfile::TempDir,
    ca: std::path::PathBuf,
    session: Option<String>,
}

impl Bw {
    fn run(&self, args: &[&str], password_env: Option<&str>) -> (bool, String, String) {
        let mut command = Command::new(&self.path);
        command.args(args).env("BITWARDENCLI_APPDATA_DIR", self.home.path()).env("BW_NOINTERACTION", "true").env("NODE_EXTRA_CA_CERTS", &self.ca);
        if let Some(session) = &self.session {
            command.env("BW_SESSION", session);
        }
        if let Some(password) = password_env {
            command.env("WM_TEST_PW", password);
        }
        let output = command.output().unwrap();
        (output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
    }

    fn ok(&self, args: &[&str]) -> String {
        let (success, out, err) = self.run(args, None);
        assert!(success, "bw {args:?} failed\nstdout: {out}\nstderr: {err}");
        out
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.ok(args)).unwrap()
    }
}

fn check_with_kdf(kdf: Kdf, email: &str) {
    let path = std::env::var("BW_CLI").expect("set BW_CLI to the bw binary");
    let dir = tempfile::tempdir().unwrap();
    let config = Config { data_dir: dir.path().to_owned(), database_url: None, signups: Signups::Open, public_url: None };
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let ca = dir.path().join("test-ca.pem");
    std::fs::write(&ca, certified.cert.pem()).unwrap();
    let (cert_pem, key_pem) = (certified.cert.pem().into_bytes(), certified.signing_key.serialize_pem().into_bytes());
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        runtime.block_on(async move {
            let app = wmpass_server::build(config).await.unwrap();
            let tls = axum_server::tls_rustls::RustlsConfig::from_pem(cert_pem, key_pem).await.unwrap();
            let secure = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            secure.set_nonblocking(true).unwrap();
            let secure_address = secure.local_addr().unwrap();
            let plain = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            sender.send((plain.local_addr().unwrap(), secure_address)).unwrap();
            let served = app.clone();
            tokio::spawn(async move { axum::serve(plain, served).await.unwrap() });
            axum_server::from_tcp_rustls(secure, tls).unwrap().serve(app.into_make_service()).await.unwrap();
        });
    });
    let (address, secure) = receiver.recv().unwrap();
    let base = format!("http://{address}");
    let secure_base = format!("https://localhost:{}", secure.port());
    let endpoints = Endpoints::self_hosted(&base);
    let http = NativeHttp::default();
    let password = "correct horse battery staple";

    // Written by WM Pass.
    register(&http, POLICY, &endpoints, email, "Tester", password.as_bytes(), kdf).unwrap();
    let device = Device { identifier: "test-device".into(), name: "WM Pass test".into(), kind: 8 };
    let mut mine = Account::login(&http, POLICY, endpoints.clone(), email, password.as_bytes(), &device, None).unwrap();
    let at = Hlc { ms: 1_800_000_000_000, counter: 0, device: 1 };
    let mut login = login_entry("Written by WM Pass", "wm-user", "wm-secret", at);
    if let Kind::Login(body) = &mut login.kind {
        body.otp = Lww::new(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP")), at);
    }
    login.common.notes = Lww::new("a note".into(), at);
    mine.upsert(login, at.ms).unwrap();
    let mut note = login_entry("A secure note from WM Pass", "", "", at);
    note.kind = Kind::Note(Note { body: Lww::new("note body".into(), at) });
    mine.upsert(note, at.ms).unwrap();
    mine.push(&http, POLICY).unwrap();

    // Read by the official client.
    let mut bw = Bw { path, home: tempfile::tempdir().unwrap(), ca, session: None };
    bw.ok(&["config", "server", &secure_base]);
    let (success, out, err) = bw.run(&["login", email, "--passwordenv", "WM_TEST_PW", "--raw"], Some(password));
    assert!(success, "login failed\nstdout: {out}\nstderr: {err}");
    bw.session = Some(out.trim().to_owned());
    assert_eq!(bw.json(&["status"])["status"], "unlocked");
    bw.ok(&["sync"]);
    let items = bw.json(&["list", "items"]);
    let items = items.as_array().unwrap();
    assert_eq!(items.len(), 2, "{items:?}");
    let login = items.iter().find(|i| i["name"] == "Written by WM Pass").expect("bw decrypted the name");
    assert_eq!(login["login"]["username"], "wm-user");
    assert_eq!(login["login"]["password"], "wm-secret");
    assert_eq!(login["notes"], "a note");
    assert!(login["login"]["totp"].as_str().unwrap().contains("JBSWY3DPEHPK3PXP"));
    let code = bw.ok(&["get", "totp", login["id"].as_str().unwrap()]);
    assert_eq!(code.trim().len(), 6, "bw generates a code from what WM Pass stored");
    let note = items.iter().find(|i| i["type"] == 2).unwrap();
    assert_eq!(note["notes"], "note body");

    // Written by the official client.
    let template = json!({
        "type": 1,
        "name": "Written by bw",
        "notes": "from the official client",
        "favorite": true,
        "fields": [{"name": "Custom", "value": "field value", "type": 1}],
        "login": {"username": "bw-user", "password": "bw-secret", "uris": [{"uri": "https://example.com/", "match": null}], "totp": null},
        "reprompt": 0,
    });
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(template.to_string());
    let created: Value = serde_json::from_str(&bw.ok(&["create", "item", &encoded])).unwrap();
    let folder = base64::engine::general_purpose::STANDARD.encode(json!({"name": "Folder by bw"}).to_string());
    let folder: Value = serde_json::from_str(&bw.ok(&["create", "folder", &folder])).unwrap();

    // Read back by WM Pass, with its own decryption.
    mine.pull(&http, POLICY).unwrap();
    let entries = mine.entries(0);
    let got = entries.iter().find(|e| e.common.title.v == "Written by bw").expect("WM Pass decrypted what bw wrote");
    assert_eq!(got.login().unwrap().username.v, "bw-user");
    assert_eq!(got.login().unwrap().password.v.expose(), "bw-secret");
    assert_eq!(got.common.notes.v.expose(), "from the official client");
    assert!(got.common.favourite.v);
    assert_eq!(got.common.custom_fields.v[0].value.expose(), "field value");
    assert!(mine.folders().iter().any(|(_, name)| name == "Folder by bw"));
    assert_eq!(mine.unreadable(), 0);

    // Edited by the official client, and history comes through.
    let id = created["id"].as_str().unwrap();
    let mut edit: Value = bw.json(&["get", "item", id]);
    edit["login"]["password"] = json!("bw-secret-2");
    edit["folderId"] = folder["id"].clone();
    let encoded = base64::engine::general_purpose::STANDARD.encode(edit.to_string());
    bw.ok(&["edit", "item", id, &encoded]);
    mine.pull(&http, POLICY).unwrap();
    let got = mine.entries(0).into_iter().find(|e| e.id == id).unwrap();
    assert_eq!(got.login().unwrap().password.v.expose(), "bw-secret-2");
    assert_eq!(got.common.folder.v.as_deref(), folder["id"].as_str());
    assert!(got.history_of("password").any(|r| r.value.expose() == "bw-secret"), "{:?}", got.history);

    // Edited by WM Pass, read by the official client. Delete, restore.
    let mut mine_edit = got.clone();
    let later = Hlc { ms: wmpass_core::clock::parse_date("2099-01-01T00:00:00Z").unwrap(), counter: 0, device: 1 };
    mine_edit.common.title = Lww::new("Renamed by WM Pass".into(), later);
    mine.upsert(mine_edit, later.ms).unwrap();
    mine.push(&http, POLICY).unwrap();
    bw.ok(&["sync"]);
    let read = bw.json(&["get", "item", id]);
    assert_eq!(read["name"], "Renamed by WM Pass");
    assert_eq!(read["login"]["password"], "bw-secret-2", "a field this edit did not touch stays");

    bw.ok(&["delete", "item", id]);
    mine.pull(&http, POLICY).unwrap();
    assert!(!mine.entries(0).iter().any(|e| e.id == id), "soft delete reached WM Pass");
    bw.ok(&["restore", "item", id]);
    mine.pull(&http, POLICY).unwrap();
    assert!(mine.entries(0).iter().any(|e| e.id == id));

    // Lock and unlock locally, then log out.
    bw.ok(&["lock"]);
    let (success, out, err) = bw.run(&["unlock", "--passwordenv", "WM_TEST_PW", "--raw"], Some(password));
    assert!(success, "unlock failed\nstdout: {out}\nstderr: {err}");
    bw.session = Some(out.trim().to_owned());
    assert_eq!(bw.json(&["list", "items"]).as_array().unwrap().len(), 3);
    bw.ok(&["logout"]);
}

#[test]
#[ignore = "needs the official bw CLI"]
fn official_client_with_pbkdf2() {
    check_with_kdf(Kdf::Pbkdf2 { iterations: 600_000 }, "pbkdf2@example.com");
}

#[test]
#[ignore = "needs the official bw CLI"]
fn official_client_with_argon2() {
    check_with_kdf(Kdf::Argon2id { iterations: 3, memory_mib: 64, parallelism: 4 }, "argon2@example.com");
}
