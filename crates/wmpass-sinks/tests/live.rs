//! Against real servers started by `rclone serve`. Ignored unless asked for.
//!
//! ```bash
//! cargo test -p wmpass-sinks --features native --test live -- --ignored --test-threads=1
//! ```
#![cfg(feature = "native")]
#![allow(missing_docs, clippy::unwrap_used, clippy::zombie_processes)]

use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use wmpass_net::NetPolicy;
use wmpass_net::http::NativeHttp;
use wmpass_sinks::Sink;
use wmpass_sinks::http::{Blocking, Client, block_on};
use wmpass_sinks::s3::{S3, S3Config};
use wmpass_sinks::webdav::WebDav;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve(args: &[&str], port: u16) -> Server {
    let child = Command::new("rclone").args(args).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Server(child);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("rclone did not start");
}

fn exercise<S: Sink>(sink: &S) {
    block_on(sink.readiness()).unwrap();
    let big = vec![7u8; 3 * 1024 * 1024];
    let stored = block_on(sink.write("a file.bin", &big)).unwrap();
    block_on(sink.write("second.bin", b"hello")).unwrap();
    block_on(sink.write("second.bin", b"hello again")).unwrap();
    let listed = block_on(sink.list()).unwrap();
    let names: Vec<&str> = listed.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"a file.bin") && names.contains(&"second.bin"), "{names:?}");
    let found = listed.iter().find(|f| f.name == "second.bin").unwrap();
    assert_eq!(found.size, 11);
    assert_eq!(block_on(sink.read(found)).unwrap(), b"hello again");
    assert_eq!(block_on(sink.read(&stored)).unwrap(), big);
    block_on(sink.delete(found)).unwrap();
    block_on(sink.delete(found)).unwrap();
    assert!(block_on(sink.list()).unwrap().iter().all(|f| f.name != "second.bin"));
}

#[test]
#[ignore = "needs rclone"]
fn webdav_against_rclone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let _server = serve(&["serve", "webdav", path, "--addr", "127.0.0.1:18391", "--user", "u", "--pass", "p"], 18391);
    let http = NativeHttp::default();
    let client = Client::new(Blocking(&http), NetPolicy::Any);
    exercise(&WebDav::new(client, "http://127.0.0.1:18391/wm", "u", &"p".into()));
    let wrong = WebDav::new(Client::new(Blocking(&http), NetPolicy::Any), "http://127.0.0.1:18391/wm", "u", &"nope".into());
    assert!(block_on(wrong.readiness()).is_err());
}

#[test]
#[ignore = "needs rclone"]
fn s3_against_rclone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("bucket")).unwrap();
    let path = dir.path().to_str().unwrap();
    let _server = serve(&["serve", "s3", path, "--addr", "127.0.0.1:18392", "--auth-key", "AKIAIOSFODNN7EXAMPLE,wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY", "--force-path-style"], 18392);
    let http = NativeHttp::default();
    let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap();
    let config = S3Config {
        endpoint: "http://127.0.0.1:18392".into(),
        region: "us-east-1".into(),
        bucket: "bucket".into(),
        prefix: "wm".into(),
        access_key: "AKIAIOSFODNN7EXAMPLE".into(),
        virtual_host: false,
    };
    exercise(&S3::new(Client::new(Blocking(&http), NetPolicy::Any), config.clone(), "wJalrXUtnFEMIK7MDENGbPxRfiCYEXAMPLEKEY".into(), now));
    let wrong = S3::new(Client::new(Blocking(&http), NetPolicy::Any), config, "WRONG".into(), now);
    assert!(block_on(wrong.readiness()).is_err());
}
