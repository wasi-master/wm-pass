//! Runs the real binary end to end. Uses the production KDF, so it is a little slow.
#![allow(missing_docs, clippy::unwrap_used)]

use assert_cmd::Command;
use std::path::Path;

fn cli(dir: &Path, password: &str) -> Command {
    let mut command = Command::cargo_bin("wmpass").unwrap();
    command
        .env("WMPASS_PASSWORD", password)
        .env("WMPASS_STATE_DIR", dir.join("state"));
    command
}

fn stdout(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_whole_session() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));

    cli(dir.path(), "wrong")
        .args(["ls", v])
        .assert()
        .failure()
        .stderr("wrong password\n");

    stdout(cli(dir.path(), "pw").args([
        "add",
        v,
        "GitHub",
        "--username",
        "octo",
        "--url",
        "https://github.com",
        "--otp",
        "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP",
    ]));
    let listing = stdout(cli(dir.path(), "pw").args(["ls", v, "git"]));
    assert!(listing.contains("GitHub") && listing.contains("octo"));
    let category = stdout(cli(dir.path(), "pw").args(["ls", v, "tag:developer"]));
    assert!(
        category.contains("GitHub"),
        "the domain list should have tagged it"
    );

    let hidden = stdout(cli(dir.path(), "pw").args(["show", v, "GitHub"]));
    assert!(hidden.contains("(hidden"));
    let shown = stdout(cli(dir.path(), "pw").args(["show", v, "GitHub", "--reveal"]));
    assert!(!shown.contains("(hidden"));

    let code = stdout(cli(dir.path(), "pw").args(["otp", v, "GitHub"]));
    assert_eq!(code.trim().len(), 6);

    let report = stdout(cli(dir.path(), "pw").args(["health", v]));
    assert!(report.contains("Score:"));

    assert!(
        dir.path().join("v.wmvault.bak").exists(),
        "the previous version must be kept"
    );
    stdout(cli(dir.path(), "pw").args(["rm", v, "GitHub"]));
    assert!(!stdout(cli(dir.path(), "pw").args(["ls", v, "git"])).contains("GitHub"));
    assert!(stdout(cli(dir.path(), "pw").args(["ls", v, "trashed"])).contains("GitHub"));
}

#[test]
fn import_export_shares_and_duress() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));

    let csv = dir.path().join("chrome.csv");
    std::fs::write(
        &csv,
        "name,url,username,password,note\nMail,https://m.example,me,hunter2,\n",
    )
    .unwrap();
    let imported = stdout(cli(dir.path(), "pw").args(["import", v, csv.to_str().unwrap()]));
    assert!(imported.contains("1 added"), "{imported}");
    let again = stdout(cli(dir.path(), "pw").args(["import", v, csv.to_str().unwrap()]));
    assert!(again.contains("1 already present"), "{again}");

    let out = dir.path().join("out.csv");
    stdout(cli(dir.path(), "pw").args(["export", v, "csv", out.to_str().unwrap()]));
    assert!(std::fs::read_to_string(&out).unwrap().contains("hunter2"));
    cli(dir.path(), "pw")
        .args(["export", v, "csv", out.to_str().unwrap()])
        .assert()
        .failure();

    let shares =
        stdout(cli(dir.path(), "pw").args(["shares", v, "--threshold", "2", "--total", "3"]));
    let lines: Vec<&str> = shares
        .lines()
        .filter_map(|l| l.split_once(": ").map(|(_, s)| s))
        .filter(|s| s.starts_with("WMPS1-"))
        .collect();
    assert_eq!(lines.len(), 3);
    let recovered = stdout(
        Command::cargo_bin("wmpass")
            .unwrap()
            .args(["recover", v, lines[0], lines[2]]),
    );
    assert!(recovered.contains("Mail"));

    cli(dir.path(), "pw")
        .args(["duress", v, "wipe"])
        .assert()
        .failure();
    stdout(
        cli(dir.path(), "pw")
            .args(["duress", v, "wipe"])
            .env("WMPASS_NEW_PASSWORD", "panic"),
    );
    // From the command line a duress password looks exactly like a wrong one.
    cli(dir.path(), "panic")
        .args(["ls", v])
        .assert()
        .failure()
        .stderr("wrong password\n");
    assert!(stdout(cli(dir.path(), "pw").args(["ls", v])).contains("Mail"));
}

#[test]
fn replaying_an_old_copy_warns() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));
    stdout(cli(dir.path(), "pw").args(["add", v, "One"]));
    let old = std::fs::read(&vault).unwrap();
    stdout(cli(dir.path(), "pw").args(["add", v, "Two"]));
    std::fs::write(&vault, old).unwrap();
    let output = cli(dir.path(), "pw").args(["ls", v]).output().unwrap();
    let warning = String::from_utf8_lossy(&output.stderr);
    assert!(warning.contains("older than one already seen"), "{warning}");
}

#[test]
fn generate_needs_no_vault() {
    let words = Command::cargo_bin("wmpass")
        .unwrap()
        .args(["gen", "--words"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&words.stdout)
            .trim()
            .split('-')
            .count(),
        6
    );
    let chars = Command::cargo_bin("wmpass")
        .unwrap()
        .args(["gen", "--length", "32"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&chars.stdout)
            .trim()
            .chars()
            .count(),
        32
    );
}

#[test]
fn a_vault_comes_back_from_a_backup_location() {
    use wmpass_sinks::backups::{BundleFile, bundle};
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));
    stdout(cli(dir.path(), "pw").args(["add", v, "Backed up", "--username", "me"]));
    let place = dir.path().join("place");
    std::fs::create_dir_all(&place).unwrap();
    let copy = bundle(&[
        BundleFile { name: "vaults/0a0a".into(), bytes: std::fs::read(&vault).unwrap() },
        BundleFile { name: "vaults/0b0b".into(), bytes: vec![1; 99] },
    ]);
    std::fs::write(place.join("wmpass-backup-20261104-093000.wmbackup"), copy).unwrap();
    std::fs::write(place.join("wmpass-backup-20250101-000000.wmbackup"), b"junk").unwrap();
    std::fs::write(place.join("phone.wmsync"), b"not a copy").unwrap();
    let p = place.to_str().unwrap();

    let listed = stdout(cli(dir.path(), "pw").args(["restore", "folder", p, "--list"]));
    let lines: Vec<&str> = listed.lines().collect();
    assert_eq!(lines.len(), 2, "{listed}");
    assert!(lines[0].contains("2026-11-04 09:30 UTC") && lines[1].contains("2025-01-01"), "{listed}");

    // The older one, picked by number, is not a backup at all.
    let root = dir.path().join("restored");
    let r = root.to_str().unwrap();
    cli(dir.path(), "pw")
        .args(["restore", "folder", p, "--into", r])
        .write_stdin("2\n")
        .assert()
        .failure()
        .stderr("That is not a WM Pass backup\n");

    let done = stdout(cli(dir.path(), "pw").args(["restore", "folder", p, "--into", r, "--latest"]));
    assert!(done.contains("Restored wmpass-backup-20261104-093000.wmbackup"), "{done}");
    assert!(stdout(cli(dir.path(), "pw").args(["ls", r])).contains("Backed up"));
    cli(dir.path(), "wrong").args(["ls", r]).assert().failure();

    let again = cli(dir.path(), "pw").args(["restore", "folder", p, "--into", r, "--latest"]).output().unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already holds a vault"));
}
