//! The contract the shells rely on: secrets stay behind the handle unless asked for.

use serde_json::{Value, json};
use wmpass_session::{Call, Reply, Service};

const FAST: &str = r#""kdf":{"m_kib":65536,"t":3,"p":1}"#;
const PASSWORD_MARKER: &str = "PasswordMarker-5f3a";
const NOTE_MARKER: &str = "NoteMarker-77a1";
const CUSTOM_MARKER: &str = "CustomMarker-02bc";

fn call(s: &mut Service, method: &str, handle: u64, args: Value) -> Reply {
    let args = args.to_string();
    s.call(method, Call { handle, args: &args, now_ms: 1_700_000_000_000, ..Call::default() })
        .unwrap_or_else(|e| panic!("{method}: {e}"))
}

fn secret_call(s: &mut Service, method: &str, handle: u64, args: Value, secret: &[u8]) -> Reply {
    let args = args.to_string();
    s.call(method, Call { handle, args: &args, secret, now_ms: 1_700_000_000_000, ..Call::default() })
        .unwrap_or_else(|e| panic!("{method}: {e}"))
}

fn json_of(reply: &Reply) -> Value {
    serde_json::from_str(&reply.json).unwrap()
}

fn new_vault(s: &mut Service) -> u64 {
    let args = format!(r#"{{"name":"Test",{FAST}}}"#);
    let reply = s
        .call("vault.create", Call { args: &args, secret: b"master", now_ms: 1000, ..Call::default() })
        .unwrap();
    json_of(&reply)["handle"].as_u64().unwrap()
}

fn login(s: &mut Service, handle: u64) -> String {
    let reply = call(s, "entry.create", handle, json!({
        "kind": "login",
        "title": "Mail",
        "fields": {"username": "me@example.com"},
        "sites": [{"Domain": "example.com"}],
    }));
    let id = json_of(&reply)["id"].as_str().unwrap().to_owned();
    secret_call(s, "entry.set_secret", handle, json!({"id": id, "field": "password"}), PASSWORD_MARKER.as_bytes());
    secret_call(s, "entry.set_secret", handle, json!({"id": id, "field": "notes"}), NOTE_MARKER.as_bytes());
    secret_call(s, "entry.custom_set", handle, json!({"id": id, "name": "PIN", "kind": "hidden"}), CUSTOM_MARKER.as_bytes());
    id
}

#[test]
fn no_view_carries_a_secret() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    let mut seen = String::new();
    for (method, args) in [
        ("entry.list", json!({"query": ""})),
        ("entry.get", json!({"id": id})),
        ("vault.info", json!({})),
        ("match.web", json!({"url": "https://example.com/login"})),
        ("health.report", json!({})),
        ("folder.list", json!({})),
        ("slot.list", json!({})),
        ("linked.list", json!({})),
    ] {
        seen.push_str(&call(&mut s, method, h, args).json);
    }
    for marker in [PASSWORD_MARKER, NOTE_MARKER, CUSTOM_MARKER] {
        assert!(!seen.contains(marker), "{marker} crossed in a view");
    }
    assert!(seen.contains("me@example.com"));
    assert!(seen.contains("\"has_password\":true"));
}

#[test]
fn a_secret_comes_out_only_when_asked_for_and_only_as_bytes() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    let reply = call(&mut s, "entry.reveal", h, json!({"id": id, "field": "password"}));
    assert_eq!(reply.secret.as_deref().map(Vec::as_slice), Some(PASSWORD_MARKER.as_bytes()));
    assert_eq!(reply.json, "{}");
    let reply = call(&mut s, "entry.reveal", h, json!({"id": id, "field": "notes"}));
    assert_eq!(reply.secret.as_deref().map(Vec::as_slice), Some(NOTE_MARKER.as_bytes()));
    let err = s.call("entry.reveal", Call { handle: h, args: &json!({"id": id, "field": "username"}).to_string(), ..Call::default() });
    assert!(err.is_err(), "a plain field is not revealed through the secret path");
}

#[test]
fn secret_fields_refuse_values_in_json() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    let args = json!({"id": id, "fields": {"password": "in-json"}}).to_string();
    let err = s.call("entry.set", Call { handle: h, args: &args, ..Call::default() }).unwrap_err();
    assert_eq!(err.code, "invalid");
}

#[test]
fn changing_a_secret_keeps_the_old_value_and_a_repeat_adds_nothing() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    secret_call(&mut s, "entry.set_secret", h, json!({"id": id, "field": "password"}), b"second");
    secret_call(&mut s, "entry.set_secret", h, json!({"id": id, "field": "password"}), b"second");
    let detail = json_of(&call(&mut s, "entry.get", h, json!({"id": id})));
    let history = detail["history"].as_array().unwrap();
    assert_eq!(history.len(), 1);
    let old = call(&mut s, "entry.reveal_history", h, json!({"id": id, "index": 0}));
    assert_eq!(old.secret.as_deref().map(Vec::as_slice), Some(PASSWORD_MARKER.as_bytes()));
}

#[test]
fn locking_makes_a_handle_useless() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    login(&mut s, h);
    call(&mut s, "vault.lock", h, json!({}));
    let err = s.call("entry.list", Call { handle: h, ..Call::default() }).unwrap_err();
    assert_eq!(err.code, "locked");
    let h = new_vault(&mut s);
    s.lock_all();
    assert_eq!(s.call("entry.list", Call { handle: h, ..Call::default() }).unwrap_err().code, "locked");
}

#[test]
fn save_reopen_and_quick_unlock() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    let saved = call(&mut s, "vault.save", h, json!({}));
    let file = saved.blobs[0].clone();
    for marker in [PASSWORD_MARKER, NOTE_MARKER, CUSTOM_MARKER, "Mail"] {
        assert!(!file.windows(marker.len()).any(|w| w == marker.as_bytes()));
    }
    let key = call(&mut s, "vault.key_for_wrapping", h, json!({})).secret.unwrap();
    s.lock_all();

    let blobs: [&[u8]; 1] = [&file];
    let wrong = s.call("vault.open", Call { secret: b"nope", blobs: &blobs, ..Call::default() }).unwrap();
    assert_eq!(json_of(&wrong)["outcome"], "no_match");
    let open = s.call("vault.open", Call { secret: b"master", blobs: &blobs, ..Call::default() }).unwrap();
    assert_eq!(json_of(&open)["outcome"], "opened");
    s.lock_all();

    let open = s.call("vault.open_key", Call { secret: &key, blobs: &blobs, ..Call::default() }).unwrap();
    let h = json_of(&open)["handle"].as_u64().unwrap();
    let got = call(&mut s, "entry.reveal", h, json!({"id": id, "field": "password"}));
    assert_eq!(got.secret.as_deref().map(Vec::as_slice), Some(PASSWORD_MARKER.as_bytes()));
}

#[test]
fn the_shelf_stays_shut_to_a_held_id_until_a_fresh_unlock() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    call(&mut s, "entry.common", h, json!({"id": id, "hidden": true}));
    let args = json!({"id": id, "field": "password"}).to_string();
    let err = s.call("entry.reveal", Call { handle: h, args: &args, ..Call::default() }).unwrap_err();
    assert_eq!(err.code, "shelf_closed");
    let rows = json_of(&call(&mut s, "entry.list", h, json!({"query": "hidden"})));
    assert_eq!(rows.as_array().unwrap().len(), 0);
    let matched = json_of(&call(&mut s, "match.web", h, json!({"url": "https://example.com"})));
    assert_eq!(matched.as_array().unwrap().len(), 0);

    let wrong = s.call("shelf.open", Call { handle: h, secret: b"nope", ..Call::default() }).unwrap_err();
    assert_eq!(wrong.code, "wrong_password");
    secret_call(&mut s, "shelf.open", h, json!({}), b"master");
    let rows = json_of(&call(&mut s, "entry.list", h, json!({"query": "hidden"})));
    assert_eq!(rows.as_array().unwrap().len(), 1);
    call(&mut s, "shelf.close", h, json!({}));
    assert!(s.call("entry.get", Call { handle: h, args: &json!({"id": id}).to_string(), ..Call::default() }).is_err());
}

#[test]
fn every_kind_can_be_filled_in_from_its_own_table() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let kinds = json_of(&call(&mut s, "kinds", 0, json!({})));
    for row in kinds.as_array().unwrap() {
        let kind = row["kind"].as_str().unwrap();
        let id = json_of(&call(&mut s, "entry.create", h, json!({"kind": kind, "title": kind})))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for field in row["fields"].as_array().unwrap() {
            let key = field["key"].as_str().unwrap();
            if field["secret"].as_bool().unwrap() {
                secret_call(&mut s, "entry.set_secret", h, json!({"id": id, "field": key}), b"value");
                let got = call(&mut s, "entry.reveal", h, json!({"id": id, "field": key}));
                assert_eq!(got.secret.as_deref().map(Vec::as_slice), Some(&b"value"[..]), "{kind}.{key}");
            } else {
                let value = match field["input"].as_str().unwrap() {
                    "toggle" => json!(true),
                    "number" | "date" => json!(443),
                    "choice" => field["choices"][0].clone(),
                    _ => json!("value"),
                };
                call(&mut s, "entry.set", h, json!({"id": id, "fields": {key: value}}));
            }
        }
        let detail = json_of(&call(&mut s, "entry.get", h, json!({"id": id})));
        for field in detail["fields"].as_array().unwrap() {
            assert_eq!(field["has_value"], true, "{kind}.{}", field["key"]);
            if field["secret"] == true {
                assert!(field["value"].is_null());
            }
        }
    }
}

#[test]
fn trash_undo_and_purge() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    call(&mut s, "entry.trash", h, json!({"id": id}));
    assert_eq!(json_of(&call(&mut s, "entry.list", h, json!({}))).as_array().unwrap().len(), 0);
    let trash = json_of(&call(&mut s, "entry.list", h, json!({"query": "trashed"})));
    assert_eq!(trash[0]["trashed"], true);
    call(&mut s, "entry.restore", h, json!({"id": id}));
    assert_eq!(json_of(&call(&mut s, "entry.list", h, json!({}))).as_array().unwrap().len(), 1);
    call(&mut s, "entry.purge", h, json!({"id": id}));
    assert!(s.call("entry.get", Call { handle: h, args: &json!({"id": id}).to_string(), ..Call::default() }).is_err());
}

#[test]
fn otp_seed_never_shows_and_codes_do() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    let seed = "JBSWY3DPEHPK3PXP";
    secret_call(&mut s, "entry.otp_set", h, json!({"id": id, "issuer": "Mail"}), seed.as_bytes());
    let detail = call(&mut s, "entry.get", h, json!({"id": id}));
    assert!(!detail.json.contains(seed));
    let codes = call(&mut s, "otp.codes", h, json!({}));
    assert!(!codes.json.contains(seed));
    let codes = json_of(&codes);
    assert_eq!(codes[0]["code"].as_str().unwrap().len(), 6);
    let args = json!({"id": id}).to_string();
    let err = s.call("entry.otp_set", Call { handle: h, args: &args, secret: b"not base32!!", ..Call::default() });
    assert!(err.is_err(), "a mistyped seed fails when it is entered");
}

#[test]
fn import_previews_before_it_changes_anything() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let csv = b"name,url,username,password\nSite,https://site.example,me,p4ss\n";
    let blobs: [&[u8]; 1] = [csv];
    let first = s.call("import.detect", Call { blobs: &blobs, ..Call::default() }).unwrap();
    let name = json_of(&first)["format"].as_str().unwrap().to_owned();
    let args2 = json!({"format": name}).to_string();
    let preview = s.call("import.read", Call { handle: h, args: &args2, blobs: &blobs, now_ms: 2000, ..Call::default() }).unwrap();
    assert_eq!(json_of(&preview)["count"], 1);
    assert!(!preview.json.contains("p4ss"));
    assert_eq!(json_of(&call(&mut s, "entry.list", h, json!({}))).as_array().unwrap().len(), 0);
    let done = call(&mut s, "import.apply", h, json!({}));
    assert_eq!(json_of(&done)["added"], 1);
    assert_eq!(json_of(&call(&mut s, "entry.list", h, json!({}))).as_array().unwrap().len(), 1);
}

#[test]
fn duress_password_reports_without_opening() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    secret_call(&mut s, "slot.add", h, json!({"label": "Panic", "action": "wipe"}), b"panic");
    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    s.lock_all();
    let blobs: [&[u8]; 1] = [&file];
    let reply = s.call("vault.open", Call { secret: b"panic", blobs: &blobs, ..Call::default() }).unwrap();
    let reply = json_of(&reply);
    assert_eq!(reply["outcome"], "duress");
    assert_eq!(reply["action"], "wipe");
    assert!(s.open_handles().is_empty());
}

#[test]
fn key_file_slot_asks_for_its_factor() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let args = json!({"label": "Key file", "factor": {"kind": "key_file"}}).to_string();
    let contents: [&[u8]; 1] = [b"key file contents"];
    s.call("slot.add", Call { handle: h, args: &args, secret: b"second", blobs: &contents, now_ms: 5 }).unwrap();
    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    s.lock_all();
    let only: [&[u8]; 1] = [&file];
    let reply = json_of(&s.call("vault.open", Call { secret: b"second", blobs: &only, ..Call::default() }).unwrap());
    assert_eq!(reply["outcome"], "needs_factor");
    assert_eq!(reply["factors"][0]["kind"], "key_file");
    let both: [&[u8]; 2] = [b"key file contents", &file];
    let args = json!({"factors": ["key_file"]}).to_string();
    let reply = json_of(&s.call("vault.open", Call { args: &args, secret: b"second", blobs: &both, ..Call::default() }).unwrap());
    assert_eq!(reply["outcome"], "opened");
}
