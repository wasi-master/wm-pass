//! The same flows on a real Postgres. Starts its own throwaway cluster.
//!
//! ```bash
//! cargo test -p wmpass-server --test postgres -- --ignored
//! ```
#![allow(missing_docs, clippy::unwrap_used, clippy::zombie_processes)]

use std::process::{Command, Stdio};

use wmpass_bw::account::{Account, register};
use wmpass_bw::api::{Device, Endpoints};
use wmpass_bw::crypto::Kdf;
use wmpass_core::backend::Backend;
use wmpass_core::clock::Hlc;
use wmpass_core::vault::login_entry;
use wmpass_net::NetPolicy;
use wmpass_net::http::NativeHttp;
use wmpass_server::{Config, Signups};

struct Cluster(std::path::PathBuf);

impl Drop for Cluster {
    fn drop(&mut self) {
        let _ = Command::new("pg_ctl").arg("-D").arg(&self.0).args(["-m", "immediate", "stop"]).stdout(Stdio::null()).status();
    }
}

#[test]
#[ignore = "needs postgres installed"]
fn everything_works_on_postgres() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("pg");
    assert!(Command::new("initdb").env("LC_ALL", "C").arg("-D").arg(&data).args(["-U", "wm", "--auth=trust", "--locale=C", "-E", "UTF8"]).stdout(Stdio::null()).status().unwrap().success());
    let port = 54_329;
    let options = format!("-p {port} -c listen_addresses=127.0.0.1 -c unix_socket_directories=''");
    assert!(Command::new("pg_ctl").env("LC_ALL", "C").arg("-D").arg(&data).args(["-w", "-o", &options, "start"]).stdout(Stdio::null()).status().unwrap().success());
    let _cluster = Cluster(data);
    let url = format!("postgres://wm@127.0.0.1:{port}/postgres");

    let config = Config { data_dir: dir.path().join("server"), database_url: Some(url), signups: Signups::Open, public_url: None };
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
    let endpoints = Endpoints::self_hosted(&format!("http://{address}"));
    let http = NativeHttp::default();
    let policy = NetPolicy::Any;
    let kdf = Kdf::Pbkdf2 { iterations: 100_000 };
    register(&http, policy, &endpoints, "me@example.com", "Me", b"pw-pw-pw", kdf).unwrap();
    let device = Device { identifier: "d".into(), name: "T".into(), kind: 8 };
    let mut account = Account::login(&http, policy, endpoints.clone(), "me@example.com", b"pw-pw-pw", &device, None).unwrap();
    let at = Hlc { ms: 2_000_000_000_000, counter: 0, device: 1 };
    account.upsert(login_entry("On Postgres", "u", "p", at), at.ms).unwrap();
    account.push(&http, policy).unwrap();
    let id = account.entries(0)[0].id.clone();
    assert_eq!(account.entries(0)[0].common.title.v, "On Postgres");
    account.remove(&id, at.ms).unwrap();
    account.push(&http, policy).unwrap();
    assert!(account.entries(0).is_empty());

    let device_b = Device { identifier: "e".into(), name: "T".into(), kind: 8 };
    let second = Account::login(&http, policy, endpoints, "me@example.com", b"pw-pw-pw", &device_b, None).unwrap();
    assert_eq!(second.unreadable(), 0);
}
