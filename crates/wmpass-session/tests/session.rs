//! The contract the shells rely on: secrets stay behind the handle unless asked for.
// The helpers below are not `#[test]` functions, so clippy's test exemption misses them.
#![allow(clippy::unwrap_used)]

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

/// A browser's quick unlock: the key is sealed under a passkey's answer and comes back only
/// with that same answer. The page never holds the key itself.
#[test]
fn a_passkey_answer_seals_and_reopens_the_key() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    let answer = [7u8; 32];
    let sealed = secret_call(&mut s, "vault.quick_seal", h, json!({}), &answer);
    assert!(sealed.secret.is_none(), "the key does not come out");
    let sealed = sealed.blobs[0].clone();
    s.lock_all();

    let blobs: [&[u8]; 2] = [&sealed, &file];
    let wrong = s.call("vault.quick_open", Call { secret: &[8u8; 32], blobs: &blobs, ..Call::default() });
    assert_eq!(wrong.err().map(|e| e.code), Some("wrong_password"));
    let open = s.call("vault.quick_open", Call { secret: &answer, blobs: &blobs, ..Call::default() }).unwrap();
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
fn a_health_search_leaves_out_what_the_health_screen_ignores() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let mut weak = Vec::new();
    for title in ["One", "Two"] {
        let id = json_of(&call(&mut s, "entry.create", h, json!({"kind": "login", "title": title})))["id"].as_str().unwrap().to_owned();
        secret_call(&mut s, "entry.set_secret", h, json!({"id": id, "field": "password"}), b"password");
        weak.push(id);
    }
    let listed = |s: &mut Service| -> Vec<String> {
        json_of(&call(s, "entry.list", h, json!({"query": "weak"})))
            .as_array().unwrap().iter().map(|row| row["id"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(listed(&mut s).len(), 2);
    call(&mut s, "health.report", h, json!({"ignored": {"weak": [weak[0]]}}));
    assert_eq!(listed(&mut s), vec![weak[1].clone()]);
    // An edit throws the report away; the next one keeps the same settings.
    call(&mut s, "folder.add", h, json!({"name": "Any"}));
    assert_eq!(listed(&mut s), vec![weak[1].clone()]);
}

#[test]
fn a_code_is_renamed_without_its_seed() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = json_of(&call(&mut s, "entry.create", h, json!({"kind": "otp", "title": "Acme"})))["id"].as_str().unwrap().to_owned();
    secret_call(&mut s, "entry.otp_set", h, json!({"id": id, "issuer": "Acme", "account": "me"}), b"JBSWY3DPEHPK3PXP");
    let issuer = |s: &mut Service| json_of(&call(s, "otp.codes", h, json!({})))[0]["otp"]["issuer"].clone();
    let code = |s: &mut Service| json_of(&call(s, "otp.codes", h, json!({})))[0]["code"].clone();
    let before = code(&mut s);
    // Renaming the item renames the issuer that was its name.
    call(&mut s, "entry.common", h, json!({"id": id, "title": "Acme Corp"}));
    assert_eq!(issuer(&mut s), "Acme Corp");
    // The names alone can change, with no seed sent.
    call(&mut s, "entry.otp_set", h, json!({"id": id, "issuer": "Acme Inc"}));
    assert_eq!(issuer(&mut s), "Acme Inc");
    assert_eq!(code(&mut s), before);
    // An issuer of its own is left alone by a rename.
    call(&mut s, "entry.common", h, json!({"id": id, "title": "Work"}));
    assert_eq!(issuer(&mut s), "Acme Inc");
}

#[test]
fn removing_a_folder_moves_the_folders_inside_it_up() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = |reply: Reply| json_of(&reply)["id"].as_str().unwrap().to_owned();
    let work = id(call(&mut s, "folder.add", h, json!({"name": "Work"})));
    let mail = id(call(&mut s, "folder.add", h, json!({"name": "Mail", "parent": work})));
    let old = id(call(&mut s, "folder.add", h, json!({"name": "Old", "parent": mail})));
    call(&mut s, "folder.remove", h, json!({"id": mail}));
    let folders = json_of(&call(&mut s, "folder.list", h, json!({})));
    let parent_of = |wanted: &str| folders.as_array().unwrap().iter().find(|f| f["id"] == wanted).map(|f| f["parent"].clone());
    assert_eq!(parent_of(&old), Some(json!(work)));
    assert_eq!(parent_of(&mail), None);
}

#[test]
fn emptying_a_stale_trash_list_changes_nothing() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let kept = login(&mut s, h);
    let gone = login(&mut s, h);
    call(&mut s, "entries.bulk", h, json!({"ids": [kept, gone], "op": "trash"}));
    call(&mut s, "entry.purge", h, json!({"id": gone}));
    // The list still names the one already purged, as another device's emptying leaves it.
    let args = json!({"ids": [kept, gone], "op": "purge"}).to_string();
    assert!(s.call("entries.bulk", Call { handle: h, args: &args, ..Call::default() }).is_err());
    let trash = json_of(&call(&mut s, "entry.list", h, json!({"query": "trashed"})));
    assert_eq!(trash.as_array().unwrap().len(), 1, "the first item is still in the trash");
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
    let err = s.call("entry.otp_set", Call { handle: h, args: &args, secret: b"   ", ..Call::default() });
    assert!(err.is_err(), "an empty seed is no code");
    // As sites show it, in groups and lower case.
    secret_call(&mut s, "entry.otp_set", h, json!({"id": id}), b"jbsw y3dp ehpk 3pxp");
    let again = json_of(&call(&mut s, "otp.codes", h, json!({})));
    assert_eq!(again[0]["code"], codes[0]["code"]);
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
fn changing_a_second_password_leaves_the_first_alone() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let added = json_of(&secret_call(&mut s, "slot.add", h, json!({"label": "Second", "action": "open"}), b"second"))["index"].as_u64().unwrap();
    assert_ne!(added, 0);
    let wrong = json_of(&secret_call(&mut s, "vault.verify", h, json!({}), b"wrong"));
    assert_eq!(wrong["ok"], false);
    assert!(wrong["slot"].is_null());
    let found = json_of(&secret_call(&mut s, "vault.verify", h, json!({}), b"second"));
    assert_eq!(found["ok"], true);
    assert_eq!(found["slot"].as_u64(), Some(added));
    secret_call(&mut s, "vault.change_password", h, json!({"index": added}), b"renewed");
    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    s.lock_all();
    let only: [&[u8]; 1] = [&file];
    let outcome = |s: &mut Service, password: &[u8]| json_of(&s.call("vault.open", Call { secret: password, blobs: &only, ..Call::default() }).unwrap())["outcome"].clone();
    assert_eq!(outcome(&mut s, b"master"), "opened", "the first password still opens");
    assert_eq!(outcome(&mut s, b"renewed"), "opened");
    assert_ne!(outcome(&mut s, b"second"), "opened");
    s.lock_all();
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

#[test]
fn a_keepass_file_opens_through_a_mirror_and_saves_back() {
    let mut s = Service::new(9);
    let handle = new_vault(&mut s);
    let _ = login(&mut s, handle);
    let exported = secret_call(&mut s, "export.file", handle, json!({"format": "kdbx"}), b"kdbx-pass");
    let file = exported.blobs[0].clone();

    let args = r#"{"name":"Work"}"#;
    let blobs: [&[u8]; 1] = [&file];
    let opened = s
        .call("kdbx.open", Call { args, secret: b"kdbx-pass", blobs: &blobs, now_ms: 2000, ..Call::default() })
        .unwrap();
    let mirror = json_of(&opened)["handle"].as_u64().unwrap();
    let kind = call(&mut s, "remote.kind", mirror, json!({}));
    assert_eq!(json_of(&kind)["kind"], "kdbx");

    // Edits go through the same calls as any vault.
    let list = call(&mut s, "entry.list", mirror, json!({"sort": "title"}));
    let rows = json_of(&list);
    let id = rows[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(rows[0]["title"], "Mail");
    secret_call(&mut s, "entry.set_secret", mirror, json!({"id": id, "field": "password"}), b"changed-here");
    let made = call(&mut s, "entry.create", mirror, json!({"kind": "login", "title": "New one", "fields": {"username": "u"}}));
    let new_id = json_of(&made)["id"].as_str().unwrap().to_owned();
    secret_call(&mut s, "entry.set_secret", mirror, json!({"id": new_id, "field": "password"}), b"brand-new");

    let saved = call(&mut s, "remote.save", mirror, json!({}));
    let saved_file = saved.blobs[0].clone();

    // A fresh open of what was saved holds both changes.
    let blobs: [&[u8]; 1] = [&saved_file];
    let again = s
        .call("kdbx.open", Call { args, secret: b"kdbx-pass", blobs: &blobs, now_ms: 3000, ..Call::default() })
        .unwrap();
    let again = json_of(&again)["handle"].as_u64().unwrap();
    let list = json_of(&call(&mut s, "entry.list", again, json!({"sort": "title"})));
    let titles: Vec<&str> = list.as_array().unwrap().iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Mail", "New one"]);
    let mail = list.as_array().unwrap().iter().find(|r| r["title"] == "Mail").unwrap()["id"].as_str().unwrap().to_owned();
    let revealed = call(&mut s, "entry.reveal", again, json!({"id": mail, "field": "password"}));
    assert_eq!(revealed.secret.as_ref().unwrap().as_slice(), b"changed-here");

    // A plain vault has no account behind it, and says so.
    assert!(s.call("remote.save", Call { handle, ..Call::default() }).is_err());
}

#[cfg(feature = "net")]
#[test]
fn proxynova_cannot_be_asked_about_someone_else() {
    let mut s = Service::new(9);
    let handle = new_vault(&mut s);
    let id = login(&mut s, handle);
    let (email, secrets, ids) = s.comb_inputs(handle, "ME@example.com").unwrap();
    assert_eq!(email.as_str(), "me@example.com");
    assert_eq!(secrets.len(), 1);
    assert_eq!(ids, [id]);
    assert!(s.comb_inputs(handle, "stranger@example.com").is_err());
}

#[test]
fn a_shared_vault_opens_for_members_and_stops_opening_for_someone_removed() {
    let mut s = Service::new(9);
    let owner = new_vault(&mut s);
    let sibling = new_vault(&mut s);
    let stranger = new_vault(&mut s);

    let made = call(&mut s, "shared.create", owner, json!({"name": "Family", "my_name": "Owner"}));
    let file = json_of(&made)["file"].as_str().unwrap().to_owned();
    let (mut sealed, mut manifest) = (made.blobs[0].clone(), made.blobs[1].clone());

    // The sibling sends a code, the owner adds them, and the invitation comes back.
    let join = json_of(&call(&mut s, "shared.join_code", sibling, json!({"my_name": "Sibling"})));
    let code = join["code"].as_str().unwrap();
    let read = json_of(&call(&mut s, "shared.read_code", owner, json!({"code": code})));
    assert_eq!(read["name"], "Sibling");
    assert_eq!(read["fingerprint"], join["fingerprint"]);
    let args = json!({"file": file, "code": code}).to_string();
    let blobs: [&[u8]; 1] = [&manifest];
    let added = s.call("shared.add_member", Call { handle: owner, args: &args, blobs: &blobs, now_ms: 2000, ..Call::default() }).unwrap();
    let invite = json_of(&added)["invite"].as_str().unwrap().to_owned();
    manifest = added.blobs[0].clone();
    let pending = join["file"].as_str().unwrap();
    let accepted = json_of(&call(&mut s, "shared.accept", sibling, json!({"file": pending, "code": invite})));
    assert_eq!(accepted["file"], file.as_str());

    let open = |s: &mut Service, handle: u64, manifest: &[u8], sealed: &[u8]| {
        let args = json!({"file": file}).to_string();
        let blobs: [&[u8]; 2] = [manifest, sealed];
        s.call("shared.open", Call { handle, args: &args, blobs: &blobs, now_ms: 3000, ..Call::default() })
    };
    let opened = json_of(&open(&mut s, sibling, &manifest, &sealed).unwrap());
    assert_eq!(opened["name"], "Family");
    assert!(open(&mut s, stranger, &manifest, &sealed).is_err());

    // Listing shows both, and the owner marked.
    let args = json!({"file": file}).to_string();
    let blobs: [&[u8]; 1] = [&manifest];
    let listed = s.call("shared.members", Call { handle: owner, args: &args, blobs: &blobs, now_ms: 3000, ..Call::default() }).unwrap();
    let members = json_of(&listed);
    assert_eq!(members.as_array().unwrap().len(), 2);
    let removed_key = members.as_array().unwrap().iter().find(|m| m["owner"] == false).unwrap()["key"].as_str().unwrap().to_owned();

    // Removing them moves the vault to a new key. The owner follows and they cannot.
    let args = json!({"file": file, "key": removed_key}).to_string();
    let blobs: [&[u8]; 2] = [&manifest, &sealed];
    let gone = s.call("shared.remove_member", Call { handle: owner, args: &args, blobs: &blobs, now_ms: 4000, ..Call::default() }).unwrap();
    manifest = gone.blobs[0].clone();
    sealed = gone.blobs[1].clone();
    assert_eq!(json_of(&open(&mut s, owner, &manifest, &sealed).unwrap())["name"], "Family");
    assert!(open(&mut s, sibling, &manifest, &sealed).is_err());
}

#[test]
fn a_tag_icon_is_kept_in_the_vault_and_can_be_cleared() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    call(&mut s, "tags.set_icon", h, json!({"tag": "Work", "icon": "badge"}));
    call(&mut s, "tags.set_icon", h, json!({"tag": "Home", "icon": "home_storage"}));
    let icons = json_of(&call(&mut s, "tags.icons", h, json!({})));
    assert_eq!(icons, json!({"Home": "home_storage", "Work": "badge"}));
    call(&mut s, "tags.set_icon", h, json!({"tag": "Work", "icon": null}));
    assert_eq!(json_of(&call(&mut s, "tags.icons", h, json!({}))), json!({"Home": "home_storage"}));
}

#[test]
fn synced_settings_are_kept_in_the_vault_and_only_real_changes_count() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let set = json_of(&call(&mut s, "prefs.sync", h, json!({"values": {"layout": "grid", "gen_length": 32, "row_icons": false}})));
    assert_eq!(set["changed"], 3);
    let again = json_of(&call(&mut s, "prefs.sync", h, json!({"values": {"layout": "grid"}})));
    assert_eq!(again["changed"], 0);
    call(&mut s, "prefs.sync", h, json!({"values": {"row_icons": null}}));
    assert_eq!(json_of(&call(&mut s, "prefs.synced", h, json!({}))), json!({"gen_length": 32, "layout": "grid"}));
    let bad = json!({"values": {"Not A Key": 1}}).to_string();
    assert!(s.call("prefs.sync", Call { handle: h, args: &bad, ..Call::default() }).is_err());
}

#[test]
fn a_tag_renamed_keeps_its_place_and_merges_with_a_twin() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    call(&mut s, "entry.common", h, json!({"id": id, "tags": ["Work", "Mail", "Office"]}));
    call(&mut s, "entries.bulk", h, json!({"ids": [id], "op": "tag_rename", "tag": "Work", "to": "Job"}));
    assert_eq!(json_of(&call(&mut s, "entry.get", h, json!({"id": id})))["tags"], json!(["Job", "Mail", "Office"]));
    call(&mut s, "entries.bulk", h, json!({"ids": [id], "op": "tag_rename", "tag": "Office", "to": "Job"}));
    assert_eq!(json_of(&call(&mut s, "entry.get", h, json!({"id": id})))["tags"], json!(["Job", "Mail"]));
}

#[test]
fn a_colour_icon_or_folder_can_be_cleared_with_null() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    call(&mut s, "entry.common", h, json!({"id": id, "color": "120", "icon": "glyph:key", "destroy_at_ms": 2_000_000_000_000u64}));
    let set = json_of(&call(&mut s, "entry.get", h, json!({"id": id})));
    assert_eq!(set["color"], "120");
    call(&mut s, "entry.common", h, json!({"id": id, "color": null, "icon": null, "destroy_at_ms": null}));
    let cleared = json_of(&call(&mut s, "entry.get", h, json!({"id": id})));
    assert!(cleared["color"].is_null(), "{cleared}");
    assert!(cleared["icon"].is_null(), "{cleared}");
    assert!(cleared["destroy_at_ms"].is_null(), "{cleared}");
    // Left out, a field stays as it was.
    call(&mut s, "entry.common", h, json!({"id": id, "color": "200"}));
    call(&mut s, "entry.common", h, json!({"id": id, "pinned": true}));
    assert_eq!(json_of(&call(&mut s, "entry.get", h, json!({"id": id})))["color"], "200");
}

#[test]
fn a_code_becomes_a_login_and_keeps_its_code() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let reply = call(&mut s, "entry.create", h, json!({"kind": "otp", "title": "GitHub"}));
    let id = json_of(&reply)["id"].as_str().unwrap().to_owned();
    secret_call(&mut s, "entry.otp_set", h, json!({"id": id, "issuer": "GitHub"}), b"JBSWY3DPEHPK3PXP");
    let suggested = json_of(&call(&mut s, "categorise.suggest", h, json!({"id": id})));
    assert_eq!(suggested["categories"], json!(["Developer", "Authenticator"]));

    call(&mut s, "entry.convert", h, json!({"id": id, "kind": "login"}));
    call(&mut s, "entry.set", h, json!({"id": id, "fields": {"username": "me@example.com"}}));
    let entry = json_of(&call(&mut s, "entry.get", h, json!({"id": id})));
    assert_eq!(entry["kind"], "login");
    assert!(!entry["otp"].is_null(), "{entry}");
    assert_eq!(entry["title"], "GitHub");
    let code = call(&mut s, "otp.code", h, json!({"id": id}));
    assert_eq!(code.secret.as_ref().map(|s| s.len()), Some(6));
    let args = json!({"id": id, "kind": "login"}).to_string();
    assert!(s.call("entry.convert", Call { handle: h, args: &args, ..Call::default() }).is_err(), "a login is not converted again");
}

#[test]
fn qr_codes_are_read_from_a_picture() {
    let text = "otpauth://totp/GitHub:octo?secret=JBSWY3DPEHPK3PXP&issuer=GitHub";
    let code = qrcode::QrCode::new(text.as_bytes()).expect("qr");
    let width = code.width();
    // Four pixels a module, with a light border, in RGBA as a camera frame comes.
    let (scale, border) = (4, 4);
    let side = (width + 2 * border) * scale;
    let colours = code.to_colors();
    let mut rgba = vec![255u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = ((x / scale).wrapping_sub(border), (y / scale).wrapping_sub(border));
            if mx < width && my < width && colours[my * width + mx] == qrcode::Color::Dark {
                rgba[(y * side + x) * 4..][..3].fill(0);
            }
        }
    }
    let mut s = Service::new(1);
    let args = json!({"width": side, "height": side, "rgba": true}).to_string();
    let reply = s
        .call("qr.read", Call { args: &args, blobs: &[&rgba], ..Call::default() })
        .expect("read");
    assert_eq!(json_of(&reply)["found"], 1);
    assert!(!reply.json.contains("JBSWY3DP"), "the code's text never rides in the view");
    let found: Vec<String> = serde_json::from_slice(reply.secret.as_deref().expect("secret")).expect("list");
    assert_eq!(found, vec![text.to_owned()]);
    // A picture with no code finds nothing, and a wrong size is refused.
    let blank = vec![200u8; 64 * 64];
    let args = json!({"width": 64, "height": 64}).to_string();
    let reply = s.call("qr.read", Call { args: &args, blobs: &[&blank], ..Call::default() }).expect("blank");
    assert_eq!(json_of(&reply)["found"], 0);
    let args = json!({"width": 65, "height": 64}).to_string();
    assert!(s.call("qr.read", Call { args: &args, blobs: &[&blank], ..Call::default() }).is_err());
}

#[test]
fn a_login_says_where_to_change_its_password() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    let detail = json_of(&call(&mut s, "entry.get", h, json!({"id": id})));
    // example.com is not on the list: no guessed link, only a domain for the app to ask.
    assert!(detail["change_password"].is_null());
    let reply = json_of(&call(&mut s, "site.change_password", 0, json!({"site": "https://login.example.com/in"})));
    assert!(reply["url"].is_null());
    assert_eq!(reply["ask"], "example.com");
    let reply = json_of(&call(&mut s, "site.change_password", 0, json!({"site": "https://www.23andme.com/login"})));
    assert_eq!(reply["url"], "https://you.23andme.com/user/edit/password/");
    assert!(reply["ask"].is_null());
    let reply = json_of(&call(&mut s, "site.change_password", 0, json!({"site": "http://192.168.1.1"})));
    assert!(reply["url"].is_null());
    assert!(reply["ask"].is_null());
}

#[test]
fn health_leaves_out_what_was_ignored_and_local_logins() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let weak = |s: &mut Service, title: &str, site: &str| {
        let reply = call(s, "entry.create", h, json!({"kind": "login", "title": title, "sites": [{"Url": site}]}));
        let id = json_of(&reply)["id"].as_str().unwrap().to_owned();
        secret_call(s, "entry.set_secret", h, json!({"id": id, "field": "password"}), b"password1");
        id
    };
    let mail = weak(&mut s, "Mail", "https://mail.example.com");
    let router = weak(&mut s, "Router", "http://192.168.1.1");
    let all = json_of(&call(&mut s, "health.report", h, json!({})));
    assert_eq!(all["weak"].as_array().unwrap().len(), 2);
    // Reused passwords come as one group per password.
    let groups = all["reused"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].as_array().unwrap().len(), 2);
    assert!(groups[0].as_array().unwrap().contains(&json!(router)));
    let skipped = json_of(&call(&mut s, "health.report", h, json!({"skip_local": true})));
    assert_eq!(skipped["weak"], json!([mail.clone()]));
    let ignored = json_of(&call(&mut s, "health.report", h, json!({"skip_local": true, "ignored": {"weak": [mail]}})));
    assert_eq!(ignored["weak"], json!([]));
}

#[test]
fn a_wifi_network_comes_in_from_its_text_and_goes_out_as_a_qr_code() {
    let mut s = Service::new(1);
    let reply = s
        .call("wifi.parse", Call { secret: b"WIFI:T:WPA;S:Home;P:hunter22;;", ..Call::default() })
        .expect("parse");
    assert_eq!(json_of(&reply), json!({"ssid": "Home", "security": "wpa", "hidden": false}));
    assert_eq!(reply.secret.as_ref().map(|s| s.to_vec()), Some(b"hunter22".to_vec()), "the password comes back as the secret");
    assert!(s.call("wifi.parse", Call { secret: b"hello", ..Call::default() }).is_err());

    let h = new_vault(&mut s);
    let made = call(&mut s, "entry.create", h, json!({"kind": "wifi", "title": "Home", "fields": {"ssid": "Home", "security": "wpa"}}));
    let id = json_of(&made)["id"].as_str().unwrap().to_owned();
    secret_call(&mut s, "entry.set_secret", h, json!({"id": id, "field": "password"}), b"hunter22");
    let view = json_of(&call(&mut s, "wifi.qr", h, json!({"id": id})));
    let want = qrcode::QrCode::new(b"WIFI:T:WPA;S:Home;P:hunter22;;").unwrap();
    assert_eq!(view["size"], want.width());
    let rows: String = want.to_colors().iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect();
    assert_eq!(view["rows"], rows);
    call(&mut s, "entry.set", h, json!({"id": id, "fields": {"security": "eap"}}));
    let args = json!({"id": id}).to_string();
    assert!(s.call("wifi.qr", Call { handle: h, args: &args, ..Call::default() }).is_err(), "an enterprise network has no code");
}

#[test]
fn related_sites_and_apps_come_from_the_engine() {
    let mut s = Service::new(1);
    let args = json!({"title": "", "sites": ["facebook.com"]}).to_string();
    let found = json_of(&s.call("related.find", Call { args: &args, ..Call::default() }).expect("find"));
    assert!(found["sites"].as_array().unwrap().contains(&json!("messenger.com")), "{found}");
    assert!(found["apps"].as_array().unwrap().contains(&json!({"package": "com.facebook.orca", "name": "Messenger"})), "{found}");
    assert_eq!(found["name"], "Facebook");
    let apps = json_of(&s.call("related.apps", Call::default()).expect("apps"));
    assert!(apps.as_array().unwrap().iter().any(|a| a["package"] == "com.facebook.katana"));
}

#[test]
fn common_values_ride_with_the_fields() {
    let mut s = Service::new(1);
    let kinds = json_of(&s.call("kinds", Call::default()).expect("kinds"));
    let field = |kind: &str, key: &str| {
        kinds.as_array().unwrap().iter().find(|k| k["kind"] == kind).unwrap()["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == key)
            .unwrap()
            .clone()
    };
    assert!(field("database", "engine")["suggestions"].as_array().unwrap().contains(&json!("PostgreSQL")));
    assert!(field("api", "provider")["suggestions"].as_array().unwrap().contains(&json!("Anthropic")));
    assert!(field("document", "label")["suggestions"].as_array().unwrap().contains(&json!("Passport")));
    assert!(field("card", "issuer")["suggestions"].as_array().unwrap().contains(&json!("Visa")));
    assert!(field("login", "username").get("suggestions").is_none(), "a field without any leaves them out");
}

#[test]
fn same_site_follows_the_public_suffix_list() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let same = |s: &mut Service, a: &str, b: &str| json_of(&call(s, "match.same_site", h, json!({"a": a, "b": b})))["same"] == json!(true);
    assert!(same(&mut s, "accounts.example.com", "www.example.com"));
    assert!(same(&mut s, "signin.aws.amazon.com", "console.aws.amazon.com"));
    assert!(!same(&mut s, "example.co.uk", "other.co.uk"));
    assert!(!same(&mut s, "example.com", "example.org"));
    assert!(!same(&mut s, "", ""));
}

#[test]
fn a_question_can_be_reworded_and_removed() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    secret_call(&mut s, "entry.question_set", h, json!({"id": id, "question": "First pet"}), b"Rex");
    secret_call(&mut s, "entry.question_set", h, json!({"id": id, "question": "First school"}), b"Hill");
    call(&mut s, "entry.question_set", h, json!({"id": id, "index": 1, "question": "First school's name", "keep_answer": true}));
    call(&mut s, "entry.question_remove", h, json!({"id": id, "index": 0}));
    let detail = json_of(&call(&mut s, "entry.get", h, json!({"id": id})));
    assert_eq!(detail["questions"], json!(["First school's name"]));
    let answer = call(&mut s, "entry.reveal_question", h, json!({"id": id, "index": 0}));
    assert_eq!(answer.secret.as_deref().map(|b| b.to_vec()), Some(b"Hill".to_vec()));
}

#[test]
fn a_health_filter_turned_around_leaves_out_what_it_names() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let add = |s: &mut Service, title: &str, password: &[u8]| {
        let reply = call(s, "entry.create", h, json!({"kind": "login", "title": title}));
        let id = json_of(&reply)["id"].as_str().unwrap().to_owned();
        secret_call(s, "entry.set_secret", h, json!({"id": id, "field": "password"}), password);
        id
    };
    let weak = add(&mut s, "Weak", b"password1");
    let strong = add(&mut s, "Strong", b"xK9#mP2$vL7@qR4!tN8&");
    let ids = |s: &mut Service, query: &str| -> Vec<String> {
        json_of(&call(s, "entry.list", h, json!({"query": query})))
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(ids(&mut s, "weak"), vec![weak.clone()]);
    assert_eq!(ids(&mut s, "\"weak\""), vec![weak]);
    assert_eq!(ids(&mut s, "-weak"), vec![strong]);
}

#[test]
fn one_seed_that_makes_no_code_leaves_the_other_codes_alone() {
    use wmpass_core::crypto::KdfParams;
    use wmpass_core::format::KdfConfig;
    use wmpass_core::model::Lww;
    use wmpass_core::otp::OtpConfig;
    use wmpass_core::vault::{Vault, login_entry};
    let config = KdfConfig::new(KdfParams { m_kib: 65536, t: 3, p: 1 }).unwrap();
    let mut vault = Vault::create("Test", b"master", &config, 1, 1000).unwrap();
    // A sync from another app can bring a seed no code comes from.
    for (title, seed) in [("Good", "JBSWY3DPEHPK3PXP"), ("Broken", "not base32!")] {
        let at = vault.tick(1000);
        let mut entry = login_entry(title, "me", "pw", at);
        entry.login_mut().unwrap().otp = Lww::new(Some(OtpConfig::totp(seed)), at);
        vault.add(entry, 1000).unwrap();
    }
    let file = vault.save().unwrap();
    let mut s = Service::new(1);
    let blobs: [&[u8]; 1] = [&file];
    let open = s.call("vault.open", Call { secret: b"master", blobs: &blobs, ..Call::default() }).unwrap();
    let h = json_of(&open)["handle"].as_u64().unwrap();
    let codes = json_of(&call(&mut s, "otp.codes", h, json!({})));
    let row = |title: &str| codes.as_array().unwrap().iter().find(|row| row["title"] == title).unwrap().clone();
    assert_eq!(row("Good")["code"].as_str().unwrap().len(), 6);
    assert!(row("Broken")["code"].is_null());
    assert!(row("Broken")["error"].as_str().is_some_and(|e| !e.is_empty()));
}

#[test]
fn a_copy_of_a_login_leaves_its_passkey_behind() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let made = call(&mut s, "passkey.create", h, json!({
        "rp_id": "example.com",
        "rp_name": "Example",
        "user_handle": "dXNlcg",
        "user_name": "me",
        "origin_host": "example.com",
    }));
    let id = json_of(&made)["id"].as_str().unwrap().to_owned();
    let copy = json_of(&call(&mut s, "entry.duplicate", h, json!({"id": id})))["id"].as_str().unwrap().to_owned();
    assert_ne!(copy, id);
    let keys = json_of(&call(&mut s, "passkey.list", h, json!({"rp_id": "example.com", "origin_host": "example.com"})));
    let holders: Vec<&str> = keys.as_array().unwrap().iter().map(|k| k["id"].as_str().unwrap()).collect();
    assert_eq!(holders, vec![id.as_str()], "only the original offers it");
}

fn fails(s: &mut Service, method: &str, handle: u64, args: Value) -> String {
    let args = args.to_string();
    s.call(method, Call { handle, args: &args, now_ms: 1_700_000_000_000, ..Call::default() })
        .map(|_| ())
        .expect_err(method)
        .message
}

#[test]
fn folders_and_tags_are_taken_as_typed_and_never_left_dangling() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let id = login(&mut s, h);
    assert_eq!(fails(&mut s, "folder.add", h, json!({"name": "   "})), "a folder has a name");
    assert_eq!(fails(&mut s, "folder.add", h, json!({"name": "Sub", "parent": "nope"})), "that folder is gone");
    let work = json_of(&call(&mut s, "folder.add", h, json!({"name": "  Work "})))["id"].as_str().unwrap().to_owned();
    let folders = json_of(&call(&mut s, "folder.list", h, json!({})));
    assert_eq!(folders[0]["name"], "Work", "trimmed");
    assert_eq!(fails(&mut s, "folder.rename", h, json!({"id": work, "name": ""})), "a folder has a name");
    assert_eq!(fails(&mut s, "folder.remove", h, json!({"id": "nope"})), "no such folder");

    // An item is never filed in a folder the vault does not have.
    assert_eq!(fails(&mut s, "entry.common", h, json!({"id": id, "folder": "nope"})), "that folder is gone");
    assert_eq!(fails(&mut s, "entries.bulk", h, json!({"ids": [id], "op": "folder", "folder": "nope"})), "that folder is gone");
    assert_eq!(fails(&mut s, "entry.create", h, json!({"kind": "note", "title": "N", "folder": "nope"})), "that folder is gone");
    call(&mut s, "entries.bulk", h, json!({"ids": [id], "op": "folder", "folder": work}));
    call(&mut s, "entries.bulk", h, json!({"ids": [id], "op": "folder", "folder": null}));

    // Tags: trimmed, without their #, each once, never empty.
    call(&mut s, "entry.common", h, json!({"id": id, "tags": [" work", "#home", "work", "", "  "]}));
    assert_eq!(json_of(&call(&mut s, "entry.get", h, json!({"id": id})))["tags"], json!(["work", "home"]));
    assert_eq!(fails(&mut s, "entries.bulk", h, json!({"ids": [id], "op": "tag_add", "tag": " # "})), "a tag has a name");
    call(&mut s, "entries.bulk", h, json!({"ids": [id], "op": "tag_add", "tag": " #work "}));
    assert_eq!(json_of(&call(&mut s, "entry.get", h, json!({"id": id})))["tags"], json!(["work", "home"]));

    // A custom field has a name.
    let args = json!({"id": id, "name": "  ", "kind": "text"}).to_string();
    let empty = s.call("entry.custom_set", Call { handle: h, args: &args, secret: b"v", ..Call::default() });
    assert_eq!(empty.map(|_| ()).expect_err("no name").message, "a field has a name");
}

#[test]
fn taking_a_tag_off_leaves_items_without_it_untouched() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let tagged = login(&mut s, h);
    let other = login(&mut s, h);
    call(&mut s, "entry.common", h, json!({"id": tagged, "tags": ["work"]}));
    let before = json_of(&call(&mut s, "entry.get", h, json!({"id": other})))["modified_ms"].clone();
    let args = json!({"ids": [tagged, other], "op": "tag_remove", "tag": "work"}).to_string();
    s.call("entries.bulk", Call { handle: h, args: &args, now_ms: 1_800_000_000_000, ..Call::default() }).unwrap();
    assert_eq!(json_of(&call(&mut s, "entry.get", h, json!({"id": other})))["modified_ms"], before);
    assert_eq!(json_of(&call(&mut s, "entry.get", h, json!({"id": tagged})))["tags"], json!([]));
}

#[test]
fn a_list_by_name_counts_numbers_as_people_do() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    for title in ["Item 10", "item 2", "Item 1", "Écran 3"] {
        call(&mut s, "entry.create", h, json!({"kind": "note", "title": title}));
    }
    let rows = json_of(&call(&mut s, "entry.list", h, json!({"query": "", "sort": "title"})));
    let titles: Vec<&str> = rows.as_array().unwrap().iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Écran 3", "Item 1", "item 2", "Item 10"]);
}

#[test]
fn an_imported_passkey_counter_rises_with_each_sign_in_and_is_saved() {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let made = wmpass_core::passkey::create("example.com", "Example", b"user", "me", true, 1000).unwrap().passkey;
    let export = json!({"encrypted": false, "folders": [], "items": [{
        "type": 1,
        "name": "Example",
        "login": {"username": "me", "fido2Credentials": [{
            "credentialId": made.credential_id,
            "rpId": "example.com",
            "rpName": "Example",
            "userHandle": made.user_handle,
            "userName": "me",
            "keyValue": made.private_key.expose(),
            "counter": "7",
        }]},
    }]})
    .to_string();
    let blobs: [&[u8]; 1] = [export.as_bytes()];
    s.call("import.read", Call { handle: h, args: "{}", blobs: &blobs, now_ms: 2000, ..Call::default() }).unwrap();
    call(&mut s, "import.apply", h, json!({}));
    let keys = json_of(&call(&mut s, "passkey.list", h, json!({"rp_id": "example.com", "origin_host": "example.com"})));
    let id = keys[0]["id"].as_str().unwrap().to_owned();
    call(&mut s, "vault.save", h, json!({}));
    assert_eq!(json_of(&call(&mut s, "vault.info", h, json!({})))["dirty"], false);

    let sign = |s: &mut Service| -> u32 {
        let reply = json_of(&call(s, "passkey.sign", h, json!({
            "id": id,
            "credential_id": made.credential_id,
            "client_data_hash": B64.encode([9u8; 32]),
            "user_verified": true,
            "origin_host": "example.com",
        })));
        let data = B64.decode(reply["authenticator_data"].as_str().unwrap()).unwrap();
        u32::from_be_bytes(data[33..37].try_into().unwrap())
    };
    let first = sign(&mut s);
    assert_eq!(first, 8, "never the imported number again");
    assert_eq!(json_of(&call(&mut s, "vault.info", h, json!({})))["dirty"], true, "the new count waits to be saved");
    let second = sign(&mut s);
    assert!(second > first);

    // Saved in the vault: a reopened copy goes on from there.
    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    s.lock_all();
    let only: [&[u8]; 1] = [&file];
    let opened = json_of(&s.call("vault.open", Call { secret: b"master", blobs: &only, ..Call::default() }).unwrap());
    let h = opened["handle"].as_u64().unwrap();
    let reply = json_of(&call(&mut s, "passkey.sign", h, json!({
        "id": id,
        "credential_id": made.credential_id,
        "client_data_hash": B64.encode([9u8; 32]),
        "origin_host": "example.com",
    })));
    let data = B64.decode(reply["authenticator_data"].as_str().unwrap()).unwrap();
    assert_eq!(u32::from_be_bytes(data[33..37].try_into().unwrap()), second + 1);
}

#[test]
fn a_passkey_made_here_keeps_a_counter_of_zero() {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let made = json_of(&call(&mut s, "passkey.create", h, json!({
        "rp_id": "example.com",
        "rp_name": "Example",
        "user_handle": "dXNlcg",
        "user_name": "me",
        "origin_host": "example.com",
    })));
    call(&mut s, "vault.save", h, json!({}));
    for _ in 0..2 {
        let reply = json_of(&call(&mut s, "passkey.sign", h, json!({
            "id": made["id"],
            "credential_id": made["credential_id"],
            "client_data_hash": B64.encode([1u8; 32]),
            "origin_host": "example.com",
        })));
        let data = B64.decode(reply["authenticator_data"].as_str().unwrap()).unwrap();
        assert_eq!(&data[33..37], &[0, 0, 0, 0]);
    }
    assert_eq!(json_of(&call(&mut s, "vault.info", h, json!({})))["dirty"], false, "nothing to save");
}

#[test]
fn a_changed_password_keeps_its_key_file() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let args = json!({"label": "Key file", "factor": {"kind": "key_file"}}).to_string();
    let key_file: &[u8] = b"key file contents";
    let contents: [&[u8]; 1] = [key_file];
    let added = json_of(&s.call("slot.add", Call { handle: h, args: &args, secret: b"second", blobs: &contents, now_ms: 5 }).unwrap())["index"].as_u64().unwrap();

    // Without a check first, the session cannot tell the slot needs a key file, so it refuses.
    let index = json!({"index": added}).to_string();
    let refused = s.call("vault.change_password", Call { handle: h, args: &index, secret: b"renewed", ..Call::default() });
    assert!(refused.is_err());

    let alone = json_of(&secret_call(&mut s, "vault.verify", h, json!({}), b"second"));
    assert_eq!(alone["ok"], false);
    assert_eq!(alone["needs_factor"][0]["kind"], "key_file");
    let factors = json!({"factors": ["key_file"]}).to_string();
    let found = json_of(&s.call("vault.verify", Call { handle: h, args: &factors, secret: b"second", blobs: &contents, ..Call::default() }).unwrap());
    assert_eq!(found["ok"], true);
    assert_eq!(found["slot"].as_u64(), Some(added));
    secret_call(&mut s, "vault.change_password", h, json!({"index": added}), b"renewed");
    assert!(
        s.call("vault.change_password", Call { handle: h, args: &index, secret: b"again", ..Call::default() }).is_err(),
        "one check, one change"
    );

    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    s.lock_all();
    let only: [&[u8]; 1] = [&file];
    let reply = json_of(&s.call("vault.open", Call { secret: b"renewed", blobs: &only, ..Call::default() }).unwrap());
    assert_eq!(reply["outcome"], "needs_factor", "the new password alone does not open it");
    assert_eq!(reply["factors"][0]["kind"], "key_file");
    let both: [&[u8]; 2] = [key_file, &file];
    let reply = json_of(&s.call("vault.open", Call { args: &factors, secret: b"renewed", blobs: &both, ..Call::default() }).unwrap());
    assert_eq!(reply["outcome"], "opened");
    let reply = json_of(&s.call("vault.open", Call { args: &factors, secret: b"second", blobs: &both, ..Call::default() }).unwrap());
    assert_eq!(reply["outcome"], "no_match");
}

#[test]
fn a_recovered_vault_takes_a_new_password_without_the_old_one() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let shares = String::from_utf8(call(&mut s, "emergency.kit", h, json!({"threshold": 2, "total": 3})).secret.unwrap().to_vec()).unwrap();
    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    s.lock_all();
    let picked: Vec<&str> = shares.lines().take(2).collect();
    let only: [&[u8]; 1] = [&file];
    let args = json!({"shares": picked}).to_string();
    let h = json_of(&s.call("emergency.recover", Call { args: &args, blobs: &only, ..Call::default() }).unwrap())["handle"].as_u64().unwrap();
    secret_call(&mut s, "vault.change_password", h, json!({"index": 0}), b"taken over");
    let file = call(&mut s, "vault.save", h, json!({})).blobs[0].clone();
    s.lock_all();
    let only: [&[u8]; 1] = [&file];
    let reply = json_of(&s.call("vault.open", Call { secret: b"taken over", blobs: &only, ..Call::default() }).unwrap());
    assert_eq!(reply["outcome"], "opened");
}

#[test]
fn the_ssh_agent_offers_only_keys_it_can_sign_with() {
    let mut s = Service::new(1);
    let h = new_vault(&mut s);
    let made = |s: &mut Service, title: &str| {
        let reply = call(s, "entry.create", h, json!({"kind": "ssh", "title": title, "fields": {"agent_allowed": true}}));
        json_of(&reply)["id"].as_str().unwrap().to_owned()
    };
    let ed = made(&mut s, "Ed");
    call(&mut s, "ssh.generate", h, json!({"id": ed, "comment": "ed"}));
    let rsa = made(&mut s, "Rsa");
    let line = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAAgQC80wCoaLi59quPQ602uexTwQrvzb6dlfsqjakoMR18Dey88MeBxTNdT3/4gT/yt1/b/f42catXAEgWOYCR8bBC3Nh1TIBmItvcVtx+2gXNkWvPCEiHdE9tYIk0CTokCI2iG42Is1SXGhXad7HI6jdxqJGtCqM3nkiV3B5w6oUXDQ== me";
    call(&mut s, "entry.set", h, json!({"id": rsa, "fields": {"public_key": line}}));
    let offered = json_of(&call(&mut s, "ssh.agent_keys", h, json!({})));
    let titles: Vec<&str> = offered.as_array().unwrap().iter().map(|k| k["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Ed"], "the RSA key is not offered");
}

fn open_on(s: &mut Service, file: &[u8], password: &[u8], now_ms: u64) -> Value {
    let blobs: [&[u8]; 1] = [file];
    json_of(&s.call("vault.open", Call { secret: password, blobs: &blobs, now_ms, ..Call::default() }).unwrap())
}

fn merge_in(s: &mut Service, handle: u64, file: &[u8], now_ms: u64) -> Value {
    let blobs: [&[u8]; 1] = [file];
    json_of(&s.call("vault.merge", Call { handle, args: "{}", blobs: &blobs, now_ms, ..Call::default() }).unwrap())
}

fn change_master(s: &mut Service, handle: u64, new: &[u8], now_ms: u64) {
    let found = json_of(&s.call("vault.verify", Call { handle, args: "{}", secret: b"master", now_ms, ..Call::default() }).unwrap());
    let index = json!({"index": found["slot"]}).to_string();
    s.call("vault.change_password", Call { handle, args: &index, secret: new, now_ms, ..Call::default() }).unwrap();
}

#[test]
fn a_password_changed_on_two_devices_at_once_is_shown_until_one_is_removed() {
    let mut a = Service::new(1);
    let h = new_vault(&mut a);
    let file = call(&mut a, "vault.save", h, json!({})).blobs[0].clone();
    assert!(json_of(&call(&mut a, "vault.notices", h, json!({})))["password_conflict"].is_null());
    let mut b = Service::new(2);
    let hb = open_on(&mut b, &file, b"master", 1500)["handle"].as_u64().unwrap();
    change_master(&mut a, h, b"from-a", 2000);
    change_master(&mut b, hb, b"from-b", 3000);
    let from_b = call(&mut b, "vault.save", hb, json!({})).blobs[0].clone();
    let merged = merge_in(&mut a, h, &from_b, 4000);
    assert_eq!(merged["password_conflict"]["both_open"], true);
    assert!(merged["password_conflict"]["message"].as_str().unwrap().contains("Both new passwords open this vault"));
    let file = call(&mut a, "vault.save", h, json!({})).blobs[0].clone();
    for password in [&b"from-a"[..], b"from-b"] {
        let opened = open_on(&mut a, &file, password, 5000);
        assert_eq!(opened["outcome"], "opened");
        assert_eq!(opened["password_conflict"]["slots"].as_array().unwrap().len(), 2, "said on opening, too");
    }
    let slots = json_of(&call(&mut a, "slot.list", h, json!({})));
    let marked: Vec<u64> = slots.as_array().unwrap().iter().filter(|s| s["conflict"] == true).map(|s| s["index"].as_u64().unwrap()).collect();
    assert_eq!(marked.len(), 2);
    call(&mut a, "slot.remove", h, json!({"index": marked[1]}));
    assert!(json_of(&call(&mut a, "vault.notices", h, json!({})))["password_conflict"].is_null());
}

#[test]
fn a_device_whose_clock_runs_ahead_is_named_in_a_warning() {
    let day = 86_400_000;
    let now = 1_700_000_000_000;
    let mut a = Service::new(1);
    let h = new_vault(&mut a);
    let file = call(&mut a, "vault.save", h, json!({})).blobs[0].clone();
    let mut b = Service::new(2);
    let hb = open_on(&mut b, &file, b"master", now + 3 * day)["handle"].as_u64().unwrap();
    let args = json!({"name": "Kitchen tablet"}).to_string();
    b.call("vault.device_name", Call { handle: hb, args: &args, now_ms: now + 3 * day, ..Call::default() }).unwrap();
    let from_b = call(&mut b, "vault.save", hb, json!({})).blobs[0].clone();
    let merged = merge_in(&mut a, h, &from_b, now);
    let warning = &merged["clock_warning"];
    assert_eq!(warning["this_device"], false);
    assert_eq!(warning["ahead_device"], "Kitchen tablet");
    assert_eq!(warning["message"], "Another device's clock (Kitchen tablet) is about 3 days ahead. Fix the time on that device so changes sync in the right order.");
    assert!(json_of(&b.call("vault.notices", Call { handle: hb, args: "{}", now_ms: now + 3 * day, ..Call::default() }).unwrap())["clock_warning"].is_null());
}
