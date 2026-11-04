//! KeePass across its versions and the ways people lock their databases. The files in
//! `tests/fixtures/keepass` were made by KeePassXC itself (`make_kdbx.sh`) or follow
//! KeePass 1's format byte for byte (`make_kdb.py`); the rest are written here with
//! every cipher, key derivation and compression a KDBX 4 file can name.

use keepass::config::{CompressionConfig, InnerCipherConfig, KdfConfig, OuterCipherConfig};
use keepass::db::fields as kp;
use keepass::{Database, DatabaseKey};

use super::*;
use crate::backend::{Backend, KdbxBackend};
use crate::clock::Hlc;
use crate::model::{Lww, SiteMatcher};
use crate::otp::{OtpAlgorithm, OtpConfig, OtpKind};
use crate::vault::login_entry;

const AT: Hlc = Hlc {
    ms: 1000,
    counter: 0,
    device: 1,
};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("../../tests/fixtures/keepass/", $name)).as_slice()
    };
}

fn titles(imported: &Imported) -> Vec<String> {
    let mut out: Vec<String> = imported
        .entries
        .iter()
        .map(|e| e.entry.common.title.v.clone())
        .collect();
    out.sort();
    out
}

fn find<'a>(imported: &'a Imported, title: &str) -> &'a ImportedEntry {
    imported
        .entries
        .iter()
        .find(|e| e.entry.common.title.v == title)
        .unwrap_or_else(|| panic!("{title} not imported"))
}

fn password_of(item: &ImportedEntry) -> &str {
    item.entry.login().unwrap().password.v.expose()
}

fn fast_kdf() -> KdfConfig {
    KdfConfig::Argon2id {
        iterations: 1,
        memory: 1024 * 1024,
        parallelism: 1,
        version: argon2_for_keepass::Version::Version13,
    }
}

fn fast_config() -> keepass::config::DatabaseConfig {
    let mut config = keepass::config::DatabaseConfig::default();
    config.kdf_config = fast_kdf();
    config
}

fn one_login(config: keepass::config::DatabaseConfig, key: DatabaseKey) -> Vec<u8> {
    let mut database = Database::with_config(config);
    let mut root = database.root_mut();
    let mut group = root.add_group();
    group.name = "Web".into();
    let mut entry = group.add_entry();
    entry.set_unprotected(kp::TITLE, "Site");
    entry.set_unprotected(kp::USERNAME, "me");
    entry.set_protected(kp::PASSWORD, "s3cret");
    entry.set_unprotected(kp::URL, "https://site.example");
    let mut bytes = Vec::new();
    database.save(&mut bytes, key).unwrap();
    bytes
}

// ---- every KDBX 4 cipher, key derivation and compression ----

#[test]
fn every_kdbx4_setting_reads_and_edits_in_place() {
    let ciphers = [
        OuterCipherConfig::AES256,
        OuterCipherConfig::Twofish,
        OuterCipherConfig::ChaCha20,
    ];
    let kdfs = [
        KdfConfig::Aes { rounds: 1000 },
        KdfConfig::Argon2 {
            iterations: 1,
            memory: 1024 * 1024,
            parallelism: 2,
            version: argon2_for_keepass::Version::Version13,
        },
        fast_kdf(),
    ];
    let compressions = [CompressionConfig::None, CompressionConfig::GZip];
    let inners = [InnerCipherConfig::ChaCha20, InnerCipherConfig::Salsa20];
    for cipher in &ciphers {
        for kdf in &kdfs {
            for compression in &compressions {
                for inner in &inners {
                    let label = format!("{cipher:?} {kdf:?} {compression:?} {inner:?}");
                    let mut config = keepass::config::DatabaseConfig::default();
                    config.outer_cipher_config = cipher.clone();
                    config.kdf_config = kdf.clone();
                    config.compression_config = compression.clone();
                    config.inner_cipher_config = inner.clone();
                    let bytes = one_login(config, DatabaseKey::new().with_password("pw"));

                    let imported = import(Format::Kdbx, &bytes, Some("pw"), AT).expect(&label);
                    let site = find(&imported, "Site");
                    assert_eq!(password_of(site), "s3cret", "{label}");
                    assert_eq!(site.folder.as_deref(), Some("Web"), "{label}");

                    let mut backend = KdbxBackend::open(&bytes, "pw".into(), None).expect(&label);
                    assert!(!backend.will_upgrade(), "{label}");
                    let mut entry = backend.entries(2000).pop().unwrap();
                    entry.set_password("changed".into(), AT);
                    backend.upsert(entry, 3000).unwrap();
                    let saved = backend.save().expect(&label);
                    let back =
                        Database::parse(&saved, DatabaseKey::new().with_password("pw")).unwrap();
                    // The settings the person chose stay as they were.
                    assert_eq!(&back.config.outer_cipher_config, cipher, "{label}");
                    assert_eq!(&back.config.kdf_config, kdf, "{label}");
                    assert_eq!(&back.config.compression_config, compression, "{label}");
                    assert_eq!(&back.config.inner_cipher_config, inner, "{label}");
                    let entry = back.iter_all_entries().next().unwrap();
                    assert_eq!(entry.get(kp::PASSWORD), Some("changed"), "{label}");
                }
            }
        }
    }
}

// ---- files KeePassXC made ----

#[test]
fn keepassxc_kdbx31_and_kdbx40_read_alike() {
    // KeePassXC's merge brings the recycle bin across as a plain group, not as the
    // bin, and KeePassXC shows its entry like any other; so does WM Pass.
    for (name, bytes, deleted) in [
        ("3.1", fixture!("kpxc-kdbx31-password.kdbx"), false),
        ("4.0", fixture!("kpxc-kdbx40-password.kdbx"), true),
    ] {
        assert_eq!(detect(bytes), Some(Format::Kdbx));
        assert!(needs_password(bytes));
        let imported = import(Format::Kdbx, bytes, Some("xc-pw"), AT).expect(name);
        let mut expected = vec!["Counter code", "KeePass 2 code", "Mail", "Shared login"];
        if deleted {
            expected.insert(1, "Deleted");
        }
        assert_eq!(titles(&imported), expected, "{name}");
        let mail = find(&imported, "Mail");
        assert_eq!(mail.folder.as_deref(), Some("Team"), "{name}");
        assert_eq!(password_of(mail), "first-pass", "{name}");
        let mut tags = mail.entry.common.tags.v.clone();
        tags.sort();
        assert_eq!(tags, vec!["mail", "work"], "{name}");
        assert_eq!(mail.entry.history.len(), 1, "{name}");
        assert_eq!(mail.entry.history[0].value.expose(), "older-pass", "{name}");
        let fields: Vec<&str> = mail
            .entry
            .common
            .custom_fields
            .v
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(fields, vec!["PIN"], "{name}");
        let otp = mail.entry.login().unwrap().otp.v.clone().expect(name);
        assert_eq!(otp.secret.expose(), "JBSWY3DPEHPK3PXP", "{name}");

        // References are followed, so autofill never types `{REF:…}`.
        let shared = find(&imported, "Shared login");
        assert_eq!(password_of(shared), "first-pass", "{name}");
        assert_eq!(shared.entry.login().unwrap().username.v, "me", "{name}");

        // KeePass 2's own fields, read and not left over as custom fields.
        let kp2 = find(&imported, "KeePass 2 code");
        let otp = kp2.entry.login().unwrap().otp.v.clone().expect(name);
        assert_eq!(otp.digits, 8, "{name}");
        assert_eq!(otp.kind, OtpKind::Totp { period: 60 }, "{name}");
        assert_eq!(otp.algorithm, OtpAlgorithm::Sha256, "{name}");
        assert!(kp2.entry.common.custom_fields.v.is_empty(), "{name}");
        let counter = find(&imported, "Counter code");
        let otp = counter.entry.login().unwrap().otp.v.clone().expect(name);
        assert_eq!(otp.kind, OtpKind::Hotp { counter: 5 }, "{name}");
        assert!(
            imported.skipped.is_empty(),
            "{name}: {:?}",
            imported.skipped
        );
    }
}

#[test]
fn keepassxc_kdbx31_upgrades_on_save_and_kdbx40_stays() {
    let old =
        KdbxBackend::open(fixture!("kpxc-kdbx31-password.kdbx"), "xc-pw".into(), None).unwrap();
    assert!(old.will_upgrade());
    let new =
        KdbxBackend::open(fixture!("kpxc-kdbx40-password.kdbx"), "xc-pw".into(), None).unwrap();
    assert!(!new.will_upgrade());
}

#[test]
fn every_key_file_form_opens() {
    let cases: [(&str, &[u8], &[u8]); 5] = [
        (
            "KeePassXC XML 2.0",
            fixture!("kpxc-kdbx31-key-file-only.kdbx"),
            fixture!("kpxc.keyx"),
        ),
        (
            "XML 1.0",
            fixture!("kpxc-key-file-key-xml-v1.kdbx"),
            fixture!("key-xml-v1.key"),
        ),
        (
            "64 hex digits",
            fixture!("kpxc-key-file-key-hex64.kdbx"),
            fixture!("key-hex64.key"),
        ),
        (
            "32 bytes",
            fixture!("kpxc-key-file-key-binary32.kdbx"),
            fixture!("key-binary32.key"),
        ),
        (
            "any file",
            fixture!("kpxc-key-file-key-arbitrary.kdbx"),
            fixture!("key-arbitrary.bin"),
        ),
    ];
    for (name, database, key) in cases {
        // No password at all: KeePassXC leaves it out of the key.
        let imported = import_with_key_file(Format::Kdbx, database, None, Some(key), AT)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!imported.entries.is_empty(), "{name}");
        let imported = import_with_key_file(Format::Kdbx, database, Some(""), Some(key), AT)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!imported.entries.is_empty(), "{name}");
        let mut backend = KdbxBackend::open(database, "".into(), Some(key.to_vec()))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let saved = backend.save().unwrap();
        // Saved under the same key, so KeePassXC opens it as before.
        let again = DatabaseKey::new().with_keyfile(&mut &key[..]).unwrap();
        Database::parse(&saved, again).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn password_and_key_file_both_needed() {
    let database = fixture!("kpxc-kdbx31-password-and-key-file.kdbx");
    let key = fixture!("kpxc.keyx");
    let opened =
        import_with_key_file(Format::Kdbx, database, Some("xc-pw"), Some(key), AT).unwrap();
    assert_eq!(password_of(find(&opened, "Mail")), "first-pass");
    let message = |result: Result<Imported>| result.unwrap_err().to_string();
    assert!(
        message(import(Format::Kdbx, database, Some("xc-pw"), AT)).contains("key file"),
        "the error must say a key file may be missing"
    );
    assert!(
        message(import_with_key_file(
            Format::Kdbx,
            database,
            Some("nope"),
            Some(key),
            AT
        ))
        .contains("wrong password or key file")
    );
    assert!(
        message(import_with_key_file(
            Format::Kdbx,
            database,
            Some("xc-pw"),
            Some(fixture!("key-hex64.key")),
            AT
        ))
        .contains("wrong password or key file")
    );
    assert!(import(Format::Kdbx, database, None, AT).is_err());
}

#[test]
fn an_empty_password_in_keepass_2_and_none_in_keepassxc_both_open() {
    let key_file = fixture!("key-hex64.key");
    // KeePass 2 with "Master password" ticked and left empty, plus a key file.
    let with_empty = one_login(
        fast_config(),
        DatabaseKey::new()
            .with_password("")
            .with_keyfile(&mut &key_file[..])
            .unwrap(),
    );
    let imported =
        import_with_key_file(Format::Kdbx, &with_empty, Some(""), Some(key_file), AT).unwrap();
    assert_eq!(password_of(find(&imported, "Site")), "s3cret");
    let mut backend = KdbxBackend::open(&with_empty, "".into(), Some(key_file.to_vec())).unwrap();
    let saved = backend.save().unwrap();
    let same = DatabaseKey::new()
        .with_password("")
        .with_keyfile(&mut &key_file[..])
        .unwrap();
    Database::parse(&saved, same).expect("the empty password stays part of the key");

    // An empty master password and nothing else, which KeePass 2 allows.
    let empty = one_login(fast_config(), DatabaseKey::new().with_password(""));
    assert!(import(Format::Kdbx, &empty, Some(""), AT).is_ok());
}

// ---- KeePass 1 ----

#[test]
fn keepass_1_databases_import() {
    let database = fixture!("kdb1-password.kdb");
    assert_eq!(detect(database), Some(Format::Kdbx));
    let imported = import(Format::Kdbx, database, Some("kdb-pw"), AT).unwrap();
    // Meta-Info is KeePass 1's own settings; Backup holds old copies.
    assert_eq!(titles(&imported), vec!["Forum", "Webmail"]);
    let forum = find(&imported, "Forum");
    assert_eq!(forum.folder.as_deref(), Some("Internet"));
    assert_eq!(forum.entry.common.notes.v.expose(), "line one\nline two");
    let mail = find(&imported, "Webmail");
    assert_eq!(mail.folder.as_deref(), Some("Internet/Mail"));
    assert_eq!(password_of(mail), "mail-pass");
    assert_eq!(
        mail.files,
        vec![("note.txt".to_owned(), b"attached text".to_vec())]
    );
    assert!(
        forum.files.is_empty(),
        "KeePass 1's empty attachment stays out"
    );
    assert_eq!(imported.skipped.len(), 1, "{:?}", imported.skipped);
    assert!(imported.skipped[0].contains("Backup"));

    // A wrong password fails cleanly, many times over: KeePass 1 checks it last.
    for wrong in ["kdb-pW", "x", "a much longer wrong password"] {
        let error = import(Format::Kdbx, database, Some(wrong), AT).unwrap_err();
        assert!(error.to_string().contains("wrong password"), "{error}");
    }
}

#[test]
fn keepass_1_passwords_outside_ascii_and_key_files() {
    let imported = import(
        Format::Kdbx,
        fixture!("kdb1-ansi-password.kdb"),
        Some("Pässwörd€"),
        AT,
    )
    .unwrap();
    assert_eq!(titles(&imported), vec!["Forum"]);
    let key = fixture!("kdb1.key");
    let imported = import_with_key_file(
        Format::Kdbx,
        fixture!("kdb1-key-file.kdb"),
        None,
        Some(key),
        AT,
    )
    .unwrap();
    assert_eq!(titles(&imported), vec!["Forum"]);
    let imported = import_with_key_file(
        Format::Kdbx,
        fixture!("kdb1-password-and-key-file.kdb"),
        Some("kdb-pw"),
        Some(key),
        AT,
    )
    .unwrap();
    assert_eq!(titles(&imported), vec!["Forum"]);
}

#[test]
fn keepass_1_is_not_opened_in_place() {
    let error = KdbxBackend::open(fixture!("kdb1-password.kdb"), "kdb-pw".into(), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("KeePass 1"), "{error}");
}

#[test]
fn not_keepass_or_damaged_says_so() {
    let error = import(Format::Kdbx, b"\x03\xd9\xa2\x9a", Some("pw"), AT).unwrap_err();
    assert!(
        error.to_string().contains("not a KeePass database"),
        "{error}"
    );
    let mut cut = fixture!("kpxc-kdbx40-password.kdbx").to_vec();
    cut.truncate(cut.len() / 2);
    let error = import(Format::Kdbx, &cut, Some("xc-pw"), AT).unwrap_err();
    assert!(error.to_string().contains("damaged"), "{error}");
}

// ---- editing in place ----

fn opened() -> KdbxBackend {
    KdbxBackend::open(fixture!("kpxc-kdbx31-password.kdbx"), "xc-pw".into(), None).unwrap()
}

fn by_title(backend: &dyn Backend, title: &str) -> crate::model::Entry {
    backend
        .entries(2000)
        .into_iter()
        .find(|e| e.common.title.v == title)
        .unwrap_or_else(|| panic!("{title} missing"))
}

fn saved_database(backend: &mut KdbxBackend) -> Database {
    let bytes = backend.save().unwrap();
    Database::parse(&bytes, DatabaseKey::new().with_password("xc-pw")).unwrap()
}

fn raw<'a>(database: &'a Database, title: &str) -> keepass::db::EntryRef<'a> {
    database
        .iter_all_entries()
        .find(|e| e.get(kp::TITLE) == Some(title))
        .unwrap()
}

#[test]
fn in_place_shows_codes_and_followed_references() {
    let backend = opened();
    let shared = by_title(&backend, "Shared login");
    assert_eq!(shared.login().unwrap().password.v.expose(), "first-pass");
    let kp2 = by_title(&backend, "KeePass 2 code");
    assert_eq!(kp2.login().unwrap().otp.v.as_ref().unwrap().digits, 8);
    assert!(
        backend
            .entries(2000)
            .iter()
            .all(|e| e.common.title.v != "Deleted")
    );
}

#[test]
fn saving_unchanged_entries_changes_nothing() {
    let mut backend = opened();
    for entry in backend.entries(2000) {
        backend.upsert(entry, 3000).unwrap();
    }
    let database = saved_database(&mut backend);
    // No new history, and the reference is still a reference.
    let mail = raw(&database, "Mail");
    assert_eq!(mail.history.as_ref().unwrap().get_entries().len(), 1);
    let shared = raw(&database, "Shared login");
    assert!(
        shared
            .history
            .as_ref()
            .is_none_or(|h| h.get_entries().is_empty())
    );
    assert_eq!(
        shared.get(kp::PASSWORD),
        Some("{REF:P@I:11111111111111111111111111111111}")
    );
    // KeePass 2's code fields are as they were.
    assert_eq!(
        raw(&database, "KeePass 2 code").get("TimeOtp-Length"),
        Some("8")
    );
    assert!(raw(&database, "KeePass 2 code").get(kp::OTP).is_none());
}

#[test]
fn clearing_an_address_or_code_clears_it_in_the_file() {
    let mut backend = opened();
    let mut mail = by_title(&backend, "Mail");
    let login = mail.login_mut().unwrap();
    login.site_matchers = Lww::new(Vec::new(), AT);
    login.otp = Lww::new(None, AT);
    backend.upsert(mail, 3000).unwrap();
    let mut kp2 = by_title(&backend, "KeePass 2 code");
    kp2.login_mut().unwrap().otp = Lww::new(None, AT);
    backend.upsert(kp2, 3000).unwrap();
    let database = saved_database(&mut backend);
    let mail = raw(&database, "Mail");
    assert_eq!(mail.get(kp::URL), Some(""));
    assert!(mail.get(kp::OTP).is_none());
    // The custom field is untouched.
    assert_eq!(mail.get("PIN"), Some("1234"));
    let kp2 = raw(&database, "KeePass 2 code");
    assert!(kp2.fields.keys().all(|k| !k.starts_with("TimeOtp")));
}

#[test]
fn a_new_code_is_written_for_keepassxc_and_keepass_2() {
    let mut backend = opened();
    let mut shared = by_title(&backend, "Shared login");
    let mut otp = OtpConfig::totp("GEZDGNBVGY3TQOJQ".to_owned());
    otp.digits = 8;
    shared.login_mut().unwrap().otp = Lww::new(Some(otp), AT);
    backend.upsert(shared, 3000).unwrap();
    let database = saved_database(&mut backend);
    let shared = raw(&database, "Shared login");
    assert!(shared.get(kp::OTP).unwrap().starts_with("otpauth://totp/"));
    assert_eq!(
        shared.get("TimeOtp-Secret-Base32"),
        Some("GEZDGNBVGY3TQOJQ")
    );
    assert_eq!(shared.get("TimeOtp-Length"), Some("8"));
    // Editing the code left the reference alone.
    assert_eq!(
        shared.get(kp::PASSWORD),
        Some("{REF:P@I:11111111111111111111111111111111}")
    );
}

#[test]
fn deleting_moves_to_the_recycle_bin_then_for_good() {
    let mut backend = opened();
    let mail = by_title(&backend, "Mail");
    backend.remove(&mail.id, 3000).unwrap();
    assert!(backend.entries(4000).iter().all(|e| e.id != mail.id));
    let database = saved_database(&mut backend);
    let bin = database.recycle_bin().unwrap();
    assert!(bin.entries().any(|e| e.get(kp::TITLE) == Some("Mail")));

    // In a database whose bin is off, it goes for good and the deletion is recorded.
    let mut source = Database::with_config(fast_config());
    source.meta.recyclebin_enabled = Some(false);
    let id = source.root_mut().add_entry().id();
    let mut bytes = Vec::new();
    source
        .save(&mut bytes, DatabaseKey::new().with_password("pw"))
        .unwrap();
    let mut backend = KdbxBackend::open(&bytes, "pw".into(), None).unwrap();
    backend.remove(&id.uuid().to_string(), 3000).unwrap();
    let saved = backend.save().unwrap();
    let back = Database::parse(&saved, DatabaseKey::new().with_password("pw")).unwrap();
    assert_eq!(back.iter_all_entries().count(), 0);
    assert!(back.deleted_objects.contains_key(&id.uuid()));
}

// ---- export ----

#[test]
fn export_keeps_folders_and_writes_codes_keepass_2_reads() {
    let mut mail = login_entry("Mail", "me", "pw-1", AT);
    let mut otp = OtpConfig::totp("JBSWY3DPEHPK3PXP".to_owned());
    otp.kind = OtpKind::Totp { period: 60 };
    mail.login_mut().unwrap().otp = Lww::new(Some(otp), AT);
    mail.login_mut().unwrap().site_matchers =
        Lww::new(vec![SiteMatcher::Url("https://m.example".into())], AT);
    let bank = login_entry("Bank", "me", "pw-2", AT);
    let loose = login_entry("Loose", "me", "pw-3", AT);
    let bytes = kdbx::export_in_folders(
        &[
            (&mail, "Work/Mail".to_owned()),
            (&bank, "Money".to_owned()),
            (&loose, String::new()),
        ],
        "pw",
    )
    .unwrap();
    let database = Database::parse(&bytes, DatabaseKey::new().with_password("pw")).unwrap();
    let mail_entry = raw(&database, "Mail");
    assert_eq!(
        mail_entry.get("TimeOtp-Secret-Base32"),
        Some("JBSWY3DPEHPK3PXP")
    );
    assert_eq!(mail_entry.get("TimeOtp-Period"), Some("60"));

    // Back in, folders return and the code's KeePass 2 copy is not a stray field.
    let imported = import(Format::Kdbx, &bytes, Some("pw"), AT).unwrap();
    let mail = find(&imported, "Mail");
    assert_eq!(mail.folder.as_deref(), Some("Work/Mail"));
    assert!(mail.entry.common.custom_fields.v.is_empty());
    let otp = mail.entry.login().unwrap().otp.v.clone().unwrap();
    assert_eq!(otp.kind, OtpKind::Totp { period: 60 });
    assert_eq!(find(&imported, "Bank").folder.as_deref(), Some("Money"));
    assert_eq!(find(&imported, "Loose").folder, None);
}
