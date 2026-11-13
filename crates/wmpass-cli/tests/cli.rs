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

    // Health words in a query filter on the report.
    stdout(cli(dir.path(), "pw").args(["add", v, "Old forum", "--password-stdin"]).write_stdin("password\n"));
    let weak = stdout(cli(dir.path(), "pw").args(["ls", v, "weak"]));
    assert!(weak.contains("Old forum") && !weak.contains("GitHub"), "{weak}");
    let strong = stdout(cli(dir.path(), "pw").args(["ls", v, "-weak"]));
    assert!(strong.contains("GitHub") && !strong.contains("Old forum"), "{strong}");

    assert!(
        dir.path().join("v.wmvault.bak").exists(),
        "the previous version must be kept"
    );
    stdout(cli(dir.path(), "pw").args(["rm", v, "GitHub"]));
    assert!(!stdout(cli(dir.path(), "pw").args(["ls", v, "git"])).contains("GitHub"));
    assert!(stdout(cli(dir.path(), "pw").args(["ls", v, "trashed"])).contains("GitHub"));
}

#[test]
fn entries_are_found_by_the_id_ls_shows_and_counter_codes_move_on() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));
    // RFC 4226's seed, whose first codes are known.
    let link = "otpauth://hotp/x?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&counter=0";
    stdout(cli(dir.path(), "pw").args(["add", v, "Bank", "--otp", link]));
    let listing = stdout(cli(dir.path(), "pw").args(["ls", v]));
    let short = listing.split_whitespace().next().unwrap().to_owned();
    assert_eq!(short.len(), 8);
    assert!(stdout(cli(dir.path(), "pw").args(["show", v, &short])).contains("Bank"));
    assert!(stdout(cli(dir.path(), "pw").args(["show", v, "bank"])).contains("Bank"));
    let first = stdout(cli(dir.path(), "pw").args(["otp", v, &short]));
    let second = stdout(cli(dir.path(), "pw").args(["otp", v, "Bank"]));
    assert_eq!((first.trim(), second.trim()), ("755224", "287082"));
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

    // The vault's own password cannot be its duress password too.
    cli(dir.path(), "pw")
        .args(["duress", v, "wipe"])
        .env("WMPASS_NEW_PASSWORD", "pw")
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

    // A script that forgot --latest gets nothing it did not pick.
    cli(dir.path(), "pw")
        .args(["restore", "folder", p, "--into", r])
        .write_stdin("")
        .assert()
        .failure()
        .stderr("no answer on stdin; --latest takes the newest without asking\n");

    let done = stdout(cli(dir.path(), "pw").args(["restore", "folder", p, "--into", r, "--latest"]));
    assert!(done.contains("Restored wmpass-backup-20261104-093000.wmbackup"), "{done}");
    assert!(stdout(cli(dir.path(), "pw").args(["ls", r])).contains("Backed up"));
    cli(dir.path(), "wrong").args(["ls", r]).assert().failure();

    let again = cli(dir.path(), "pw").args(["restore", "folder", p, "--into", r, "--latest"]).output().unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already holds a vault"));
}

#[test]
fn empty_input_and_typos_fail_before_anything_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();

    // An empty password never makes a vault.
    cli(dir.path(), "").args(["create", v]).assert().failure().stderr("the password cannot be empty\n");
    assert!(!vault.exists());
    stdout(cli(dir.path(), "pw").args(["create", v]));

    // A pipe that came up empty adds no login with an empty password.
    cli(dir.path(), "pw")
        .args(["add", v, "Empty", "--password-stdin"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr("no password on stdin\n");
    assert!(!stdout(cli(dir.path(), "pw").args(["ls", v])).contains("Empty"));
    cli(dir.path(), "pw").arg("strength").write_stdin("").assert().failure();

    // An unknown format is caught before the vault is even read.
    let out = dir.path().join("out.txt");
    cli(dir.path(), "pw")
        .args(["export", "/no/such/vault", "xml", out.to_str().unwrap()])
        .assert()
        .failure()
        .stderr("unknown format xml; use csv, bitwarden, html or kdbx\n");
}

#[cfg(unix)]
#[test]
fn an_export_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));
    let out = dir.path().join("out.csv");
    stdout(cli(dir.path(), "pw").args(["export", v, "csv", out.to_str().unwrap()]));
    let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(!dir.path().join("out.csv.tmp").exists());
    // The revisions seen go where the terminal app keeps them, not a folder below.
    assert!(dir.path().join("state/seen.json").exists());
}

#[test]
fn twenty_words_are_twenty_words() {
    let words = Command::cargo_bin("wmpass").unwrap().args(["gen", "--words", "--length", "20"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&words.stdout).trim().split('-').count(), 20);
}

#[test]
fn a_file_that_is_no_vault_says_so_before_asking() {
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("junk.wmvault");
    std::fs::write(&junk, b"not a vault at all").unwrap();
    // Not "wrong password", which would send someone hunting for the right one.
    cli(dir.path(), "pw")
        .args(["ls", junk.to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(format!("{}: this is not a readable WM Pass vault\n", junk.display()));
    cli(dir.path(), "pw")
        .args(["ls", dir.path().join("missing.wmvault").to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(format!("{}: no such file or folder\n", dir.path().join("missing.wmvault").display()));
}

#[test]
fn titles_cannot_split_lines_or_drive_the_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));
    stdout(cli(dir.path(), "pw").args(["add", v, "Evil\u{1b}]0;owned\u{7}\nsecond line", "--username", "a\tb"]));
    let listing = stdout(cli(dir.path(), "pw").args(["ls", v]));
    assert_eq!(listing.lines().count(), 1, "{listing}");
    assert!(!listing.contains('\u{1b}') && !listing.contains('\t'), "{listing}");
    assert!(listing.contains("Evil\\u{1b}]0;owned\\u{7}\\nsecond line"), "{listing}");
}

#[test]
fn usage_mistakes_exit_2_and_failures_1() {
    // No subcommand and no terminal: the help, on stderr, as clap does for a missing argument.
    let bare = Command::cargo_bin("wmpass").unwrap().output().unwrap();
    assert_eq!(bare.status.code(), Some(2));
    assert!(bare.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bare.stderr).contains("Usage: wmpass"));
    Command::cargo_bin("wmpass").unwrap().arg("ls").assert().code(2);
    Command::cargo_bin("wmpass").unwrap().args(["gen", "--length", "2"]).assert().code(1);
    let help = Command::cargo_bin("wmpass").unwrap().arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("WMPASS_PASSWORD") && help.contains("Exit status"), "{help}");
}

#[test]
fn show_prints_every_kind_with_its_secrets_masked() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("v.wmvault");
    let v = vault.to_str().unwrap();
    stdout(cli(dir.path(), "pw").args(["create", v]));
    let export = dir.path().join("bw.json");
    std::fs::write(
        &export,
        r#"{"encrypted":false,"folders":[],"items":[
{"type":3,"name":"Visa","card":{"cardholderName":"Ada","number":"4111111111111111","code":"123","expMonth":"12","expYear":"2030","brand":"Visa"}},
{"type":2,"name":"Memo","notes":"line one\nline two","secureNote":{"type":0}},
{"type":1,"name":"Bank","login":{"username":"me","password":"pw1"}}]}"#,
    )
    .unwrap();
    stdout(cli(dir.path(), "pw").args(["import", v, export.to_str().unwrap()]));

    let card = stdout(cli(dir.path(), "pw").args(["show", v, "Visa"]));
    assert!(card.contains("Name:     Ada") && card.contains("Expires:  12/2030"), "{card}");
    assert!(card.contains("Number:   (hidden") && !card.contains("4111"), "{card}");
    let card = stdout(cli(dir.path(), "pw").args(["show", v, "Visa", "--reveal"]));
    assert!(card.contains("Number:   4111111111111111") && card.contains("Security code: 123"), "{card}");

    let note = stdout(cli(dir.path(), "pw").args(["show", v, "Memo"]));
    assert!(note.contains("Text:     (hidden") && !note.contains("line one"), "{note}");
    let note = stdout(cli(dir.path(), "pw").args(["show", v, "Memo", "--reveal"]));
    assert!(note.contains("Text:     line one\n          line two\n"), "{note}");

    // Counts leave the trash apart, as `ls` does.
    stdout(cli(dir.path(), "pw").args(["rm", v, "Bank"]));
    let copy = dir.path().join("copy.wmvault");
    std::fs::copy(&vault, &copy).unwrap();
    let merged = stdout(cli(dir.path(), "pw").args(["merge", v, copy.to_str().unwrap()]));
    assert_eq!(merged, "Merged. 2 items, 1 in the trash.\n");
}
