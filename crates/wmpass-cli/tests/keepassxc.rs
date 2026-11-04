//! Round trips against the real `keepassxc-cli`. Skipped when it is not installed,
//! and CI installs it, so these always run there.
#![allow(missing_docs, clippy::unwrap_used)]

use std::io::Write;
use std::process::{Command, Stdio};

use assert_cmd::Command as AssertCommand;
use wmpass_core::backend::{Backend, KdbxBackend};
use wmpass_core::clock::Hlc;
use wmpass_core::model::{Lww, SiteMatcher};
use wmpass_core::vault::login_entry;
use wmpass_core::{Secret, io};

fn available() -> bool {
    Command::new("keepassxc-cli")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn keepassxc(args: &[&str], stdin: &str) -> (bool, String) {
    let mut child = Command::new("keepassxc-cli")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn keepassxc_reads_what_we_export() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let at = Hlc {
        ms: 1000,
        ..Hlc::default()
    };
    let mut entry = login_entry("Mail", "me@example.com", "pa\"ss<w>ord&", at);
    entry.login_mut().unwrap().site_matchers =
        Lww::new(vec![SiteMatcher::Url("https://m.example".into())], at);
    let bytes = io::kdbx::export(&[&entry], "export-pw").unwrap();
    let path = dir.path().join("out.kdbx");
    std::fs::write(&path, bytes).unwrap();
    let file = path.to_str().unwrap();

    let (ok, shown) = keepassxc(
        &[
            "show", "-q", "-s", "-a", "UserName", "-a", "Password", "-a", "URL", file, "Mail",
        ],
        "export-pw\n",
    );
    assert!(ok, "keepassxc-cli could not open our export");
    let lines: Vec<&str> = shown.lines().collect();
    assert_eq!(
        lines,
        vec!["me@example.com", "pa\"ss<w>ord&", "https://m.example"]
    );
    let (ok, _) = keepassxc(&["db-info", "-q", file], "wrong\n");
    assert!(!ok, "a wrong password must fail");
}

fn edit_round_trip(expect_upgrade: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.kdbx");
    let file = path.to_str().unwrap();
    if expect_upgrade {
        // keepassxc-cli only ever creates KDBX 3.
        let (ok, _) = keepassxc(&["db-create", "-q", "-p", file], "db-pw\ndb-pw\n");
        assert!(ok, "db-create failed");
    } else {
        // A KDBX 4 file that KeePassXC then modifies and keeps as KDBX 4.
        std::fs::write(&path, io::kdbx::export(&[], "db-pw").unwrap()).unwrap();
    }
    let (ok, _) = keepassxc(&["mkdir", "-q", file, "Team"], "db-pw\n");
    assert!(ok, "mkdir failed");
    let (ok, _) = keepassxc(
        &[
            "add",
            "-q",
            "-u",
            "me",
            "--url",
            "https://t.example",
            "-p",
            file,
            "Team/Mail",
        ],
        "db-pw\nfirst-pass\n",
    );
    assert!(ok, "add failed");

    // Import keeps the group as a folder.
    let data = std::fs::read(&path).unwrap();
    assert_eq!(io::detect(&data), Some(io::Format::Kdbx));
    let imported = io::import(io::Format::Kdbx, &data, Some("db-pw"), Hlc::default()).unwrap();
    let item = imported
        .entries
        .iter()
        .find(|i| i.entry.common.title.v == "Mail")
        .unwrap();
    assert_eq!(
        item.folder.as_deref().map(|f| f.ends_with("Team")),
        Some(true)
    );
    assert_eq!(
        item.entry.login().unwrap().password.v.expose(),
        "first-pass"
    );

    // Edit in place, and KeePassXC sees the change.
    let mut backend = KdbxBackend::open(&data, Secret::new("db-pw"), None).unwrap();
    assert_eq!(backend.will_upgrade(), expect_upgrade);
    let mut stored = backend
        .entries(5000)
        .into_iter()
        .find(|e| e.common.title.v == "Mail")
        .unwrap();
    stored.set_password(
        "second-pass".into(),
        Hlc {
            ms: 6000,
            ..Hlc::default()
        },
    );
    backend.upsert(stored, 6000).unwrap();
    std::fs::write(&path, backend.save().unwrap()).unwrap();

    let (ok, shown) = keepassxc(
        &[
            "show",
            "-q",
            "-s",
            "-a",
            "Password",
            "-a",
            "UserName",
            file,
            "Team/Mail",
        ],
        "db-pw\n",
    );
    assert!(ok, "keepassxc-cli could not open the file after our edit");
    assert_eq!(shown.lines().collect::<Vec<_>>(), vec!["second-pass", "me"]);
    let (ok, listing) = keepassxc(&["ls", "-q", file], "db-pw\n");
    assert!(
        ok && listing.contains("Team/"),
        "the group must survive: {listing}"
    );
    let (ok, info) = keepassxc(&["db-info", "-q", file], "db-pw\n");
    assert!(ok, "db-info failed");
    if expect_upgrade {
        assert!(
            info.contains("Argon2"),
            "the upgrade should bring Argon2: {info}"
        );
    }
}

#[test]
fn we_edit_a_kdbx3_database_and_upgrade_it() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    edit_round_trip(true);
}

#[test]
fn we_edit_a_kdbx4_database_in_place() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    edit_round_trip(false);
}

#[test]
fn the_cli_imports_a_keepassxc_database() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("db.kdbx");
    let file = database.to_str().unwrap();
    keepassxc(&["db-create", "-q", "-p", file], "pw\npw\n");
    keepassxc(
        &["add", "-q", "-u", "me", "-p", file, "Mail"],
        "pw\nsecret-1\n",
    );
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    let run = |args: &[&str]| {
        let output = AssertCommand::cargo_bin("wmpass")
            .unwrap()
            .env("WMPASS_PASSWORD", "pw")
            .env("WMPASS_STATE_DIR", dir.path().join("state"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    run(&["create", v]);
    assert!(run(&["import", v, file]).contains("1 added"));
    assert!(run(&["show", v, "Mail", "--reveal"]).contains("secret-1"));
}

#[test]
fn keepassxc_opens_our_edit_of_a_database_locked_by_a_key_file_alone() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.kdbx");
    let key = dir.path().join("db.keyx");
    let (file, key_path) = (path.to_str().unwrap(), key.to_str().unwrap());
    let (ok, _) = keepassxc(&["db-create", "-q", "--set-key-file", key_path, file], "");
    assert!(ok, "db-create failed");
    let (ok, _) = keepassxc(
        &[
            "add",
            "-q",
            "--no-password",
            "-k",
            key_path,
            "-u",
            "me",
            "-p",
            file,
            "Mail",
        ],
        "first-pass\n",
    );
    assert!(ok, "add failed");

    let data = std::fs::read(&path).unwrap();
    let key_bytes = std::fs::read(&key).unwrap();
    let imported = io::import_with_key_file(
        io::Format::Kdbx,
        &data,
        None,
        Some(&key_bytes),
        Hlc::default(),
    )
    .unwrap();
    assert_eq!(imported.entries.len(), 1);

    let mut backend = KdbxBackend::open(&data, Secret::new(""), Some(key_bytes)).unwrap();
    let mut stored = backend.entries(5000).pop().unwrap();
    stored.set_password(
        "second-pass".into(),
        Hlc {
            ms: 6000,
            ..Hlc::default()
        },
    );
    backend.upsert(stored, 6000).unwrap();
    std::fs::write(&path, backend.save().unwrap()).unwrap();
    let (ok, shown) = keepassxc(
        &[
            "show",
            "-q",
            "--no-password",
            "-k",
            key_path,
            "-s",
            "-a",
            "Password",
            file,
            "Mail",
        ],
        "",
    );
    assert!(ok, "KeePassXC could not open the file after our edit");
    assert_eq!(shown.trim(), "second-pass");
}

#[test]
fn keepassxc_finds_what_we_delete_in_its_recycle_bin() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.kdbx");
    let file = path.to_str().unwrap();
    keepassxc(&["db-create", "-q", "-p", file], "pw\npw\n");
    keepassxc(
        &["add", "-q", "-u", "me", "-p", file, "Keep"],
        "pw\nkeep-pass\n",
    );
    keepassxc(
        &["add", "-q", "-u", "me", "-p", file, "Bin"],
        "pw\nbin-pass\n",
    );
    let data = std::fs::read(&path).unwrap();
    let mut backend = KdbxBackend::open(&data, Secret::new("pw"), None).unwrap();
    let gone = backend
        .entries(5000)
        .into_iter()
        .find(|e| e.common.title.v == "Bin")
        .unwrap();
    backend.remove(&gone.id, 6000).unwrap();
    std::fs::write(&path, backend.save().unwrap()).unwrap();
    let (ok, listing) = keepassxc(&["ls", "-R", "-q", file], "pw\n");
    assert!(ok, "ls failed");
    let lines: Vec<&str> = listing.lines().map(str::trim).collect();
    let bin = lines
        .iter()
        .position(|l| *l == "Recycle Bin/")
        .expect(&listing);
    assert_eq!(lines.get(bin + 1), Some(&"Bin"), "{listing}");
    assert!(lines.contains(&"Keep"), "{listing}");
}

#[test]
fn keepassxc_reads_the_folders_and_codes_we_export() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let at = Hlc {
        ms: 1000,
        ..Hlc::default()
    };
    let mut entry = login_entry("Mail", "me", "pw-1", at);
    entry.login_mut().unwrap().otp = Lww::new(
        Some(wmpass_core::otp::OtpConfig::totp(
            "JBSWY3DPEHPK3PXP".to_owned(),
        )),
        at,
    );
    let bytes = io::kdbx::export_in_folders(&[(&entry, "Work/Team".to_owned())], "pw").unwrap();
    let path = dir.path().join("out.kdbx");
    std::fs::write(&path, bytes).unwrap();
    let file = path.to_str().unwrap();
    let (ok, shown) = keepassxc(
        &["show", "-q", "-s", "-a", "Password", file, "Work/Team/Mail"],
        "pw\n",
    );
    assert!(ok, "the entry must be in Work/Team");
    assert_eq!(shown.trim(), "pw-1");
    let (ok, code) = keepassxc(&["show", "-q", "--totp", file, "Work/Team/Mail"], "pw\n");
    assert!(ok, "KeePassXC must read the code");
    assert!(
        code.trim().len() == 6 && code.trim().chars().all(|c| c.is_ascii_digit()),
        "{code}"
    );
}

#[test]
fn the_cli_imports_a_database_with_a_key_file() {
    if !available() {
        eprintln!("keepassxc-cli not installed, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("db.kdbx");
    let key = dir.path().join("db.keyx");
    let (file, key_path) = (database.to_str().unwrap(), key.to_str().unwrap());
    keepassxc(&["db-create", "-q", "--set-key-file", key_path, file], "");
    keepassxc(
        &[
            "add",
            "-q",
            "--no-password",
            "-k",
            key_path,
            "-u",
            "me",
            "-p",
            file,
            "Mail",
        ],
        "secret-1\n",
    );
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    let run = |password: &str, args: &[&str]| {
        let output = AssertCommand::cargo_bin("wmpass")
            .unwrap()
            .env("WMPASS_PASSWORD", password)
            .env("WMPASS_STATE_DIR", dir.path().join("state"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    run("pw", &["create", v]);
    let output = AssertCommand::cargo_bin("wmpass")
        .unwrap()
        .env("WMPASS_PASSWORD", "pw")
        .env("WMPASS_IMPORT_PASSWORD", "")
        .env("WMPASS_STATE_DIR", dir.path().join("state"))
        .args(["import", v, file, "--key-file", key_path])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 added"));
    assert!(run("pw", &["show", v, "Mail", "--reveal"]).contains("secret-1"));
}
