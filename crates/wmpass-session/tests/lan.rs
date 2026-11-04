//! Two services on this machine talking over localhost, through the calls the shells make:
//! two different vaults fail to sync and say so on both ends, merge into one, and then sync.
#![cfg(feature = "net")]
#![allow(clippy::unwrap_used)]

use std::sync::Mutex;
use std::thread;

use serde_json::{Value, json};
use wmpass_session::{Call, Reply, Service, net};

const NOW: u64 = 1_700_000_000_000;

fn local(s: &Mutex<Option<Service>>, method: &str, handle: u64, args: Value, secret: &[u8], blobs: &[&[u8]]) -> Reply {
    let args = args.to_string();
    let mut guard = s.lock().unwrap();
    guard
        .as_mut()
        .unwrap()
        .call(method, Call { handle, args: &args, secret, blobs, now_ms: NOW })
        .unwrap_or_else(|e| panic!("{method}: {e}"))
}

fn lan(s: &Mutex<Option<Service>>, method: &str, handle: u64, args: &Value, secret: &[u8]) -> Reply {
    net::call(s, method, handle, &args.to_string(), secret, false, NOW).unwrap_or_else(|e| panic!("{method}: {e}"))
}

fn json_of(reply: &Reply) -> Value {
    serde_json::from_str(&reply.json).unwrap()
}

/// A service with a vault of its own and one login in it.
fn device(device: u32, password: &[u8], title: &str) -> (Mutex<Option<Service>>, u64) {
    let s = Mutex::new(Some(Service::new(device)));
    let reply = local(&s, "vault.create", 0, json!({"name": title, "kdf": {"m_kib": 65536, "t": 3, "p": 1}}), password, &[]);
    let handle = json_of(&reply)["handle"].as_u64().unwrap();
    let reply = local(&s, "entry.create", handle, json!({"kind": "login", "title": title, "fields": {"username": "me"}}), &[], &[]);
    let id = json_of(&reply)["id"].as_str().unwrap().to_owned();
    local(&s, "entry.set_secret", handle, json!({"id": id, "field": "password"}), title.as_bytes(), &[]);
    (s, handle)
}

fn titles(s: &Mutex<Option<Service>>, handle: u64) -> Vec<String> {
    let rows = json_of(&local(s, "entry.list", handle, json!({"query": ""}), &[], &[]));
    let mut out: Vec<String> = rows.as_array().unwrap().iter().map(|r| r["title"].as_str().unwrap().to_owned()).collect();
    out.sort();
    out
}

fn folder(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("wmpass-lan-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_string_lossy().into_owned()
}

#[test]
fn different_vaults_say_so_on_both_ends_then_merge_and_sync() {
    let (phone, phone_vault) = device(1, b"phone-pw", "Only on the phone");
    let (computer, computer_vault) = device(2, b"computer-pw", "Only on the computer");
    let phone_id = lan(&phone, "lan.new_identity", 0, &json!({}), &[]).secret.unwrap();
    let computer_id = lan(&computer, "lan.new_identity", 0, &json!({}), &[]).secret.unwrap();
    let key_of = |s: &Mutex<Option<Service>>, id: &[u8]| json_of(&lan(s, "lan.public", 0, &json!({}), id))["public"].as_str().unwrap().to_owned();
    let (phone_key, computer_key) = (key_of(&phone, &phone_id), key_of(&computer, &computer_id));
    let phone_peers = json!([{"key": computer_key, "name": "Computer"}]);
    let computer_peers = json!([{"key": phone_key, "name": "Phone"}]);
    let (phone_blobs, computer_blobs, new_home) = (folder("phone"), folder("computer"), folder("new-home"));

    // The computer waits in the background, as it does whenever its vault is open.
    let port = json_of(&lan(&computer, "lan.listen_background", 0, &json!({}), &[]))["port"].as_u64().unwrap();
    let address = format!("127.0.0.1:{port}");
    let serve = json!({"peers": computer_peers, "blob_dir": computer_blobs, "background": true});

    // 1. Different vaults: both ends say so, and each names the device it was talking to.
    let (synced, served) = thread::scope(|scope| {
        let waiting = scope.spawn(|| lan(&computer, "lan.serve", computer_vault, &serve, &computer_id));
        let synced = lan(&phone, "lan.sync", phone_vault, &json!({"address": address, "peers": phone_peers, "blob_dir": phone_blobs}), &phone_id);
        (json_of(&synced), json_of(&waiting.join().unwrap()))
    });
    assert_eq!(synced["failed"]["code"], "not_this_vault");
    assert_eq!(synced["failed"]["key"], computer_key.as_str());
    assert_eq!(served["failed"]["code"], "not_this_vault");
    assert_eq!(served["failed"]["key"], phone_key.as_str());

    // 2. The phone asks to merge and keep the computer's vault; the computer's person says yes.
    let (asked, answered) = thread::scope(|scope| {
        let waiting = scope.spawn(|| {
            let asked = json_of(&lan(&computer, "lan.serve", computer_vault, &serve, &computer_id));
            assert_eq!(asked["merge_asked"]["they_keep"], false);
            assert_eq!(asked["merge_asked"]["peer"]["key"], phone_key.as_str());
            lan(&computer, "lan.merge_answer", computer_vault, &json!({"accepted": true, "blob_dir": computer_blobs, "new_blob_dir": folder("unused")}), &[])
        });
        let merge = json!({"address": address, "peers": phone_peers, "keep_mine": false, "blob_dir": phone_blobs, "new_blob_dir": new_home});
        let asked = lan(&phone, "lan.merge", phone_vault, &merge, &phone_id);
        (asked, json_of(&waiting.join().unwrap()))
    });
    assert_eq!(answered["merged"], "kept");
    assert_eq!(answered["added"], 1);
    assert_eq!(titles(&computer, computer_vault), ["Only on the computer", "Only on the phone"]);
    let asked_json = json_of(&asked);
    assert_eq!(asked_json["merged"], "replaced");

    // 3. The phone takes the kept vault, opened with the computer's password, and from then on
    // the two sync as usual.
    let file = asked.blobs[0].clone();
    let opened = json_of(&local(&phone, "vault.open", 0, json!({"factors": []}), b"computer-pw", &[&file]));
    assert_eq!(opened["outcome"], "opened");
    let phone_vault = opened["handle"].as_u64().unwrap();
    assert_eq!(titles(&phone, phone_vault), ["Only on the computer", "Only on the phone"]);
    let (synced, served) = thread::scope(|scope| {
        let waiting = scope.spawn(|| lan(&computer, "lan.serve", computer_vault, &serve, &computer_id));
        let synced = lan(&phone, "lan.sync", phone_vault, &json!({"address": address, "peers": phone_peers, "blob_dir": new_home}), &phone_id);
        (json_of(&synced), json_of(&waiting.join().unwrap()))
    });
    assert!(synced.get("failed").is_none(), "{synced}");
    assert_eq!(synced["key"], computer_key.as_str());
    assert_eq!(synced["converged"], true);
    assert_eq!(served["key"], phone_key.as_str());
    lan(&computer, "lan.stop_background", 0, &json!({}), &[]);
}
