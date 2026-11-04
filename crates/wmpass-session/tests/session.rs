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
