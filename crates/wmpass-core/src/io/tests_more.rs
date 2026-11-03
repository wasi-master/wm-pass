//! The importers added for SafeInCloud's database and the apps after it, and the
//! fixes to the earlier ones. Samples follow each format's published or observed
//! layout; `tests/fixtures/safeincloud/sample.db` is a real SafeInCloud database.

use std::io::Write;

use super::*;
use crate::clock::Hlc;
use crate::model::Kind;
use crate::otp::{OtpAlgorithm, OtpKind};

const AT: Hlc = Hlc {
    ms: 1000,
    counter: 0,
    device: 1,
};

fn run(data: &[u8]) -> (Format, Imported) {
    let format = detect(data).expect("format not detected");
    let imported = import(format, data, None, AT).unwrap();
    (format, imported)
}

fn titled<'a>(imported: &'a Imported, title: &str) -> &'a ImportedEntry {
    imported
        .entries
        .iter()
        .find(|e| e.entry.common.title.v == title)
        .unwrap_or_else(|| panic!("no entry called {title}"))
}

fn field<'a>(item: &'a ImportedEntry, name: &str) -> &'a str {
    item.entry
        .common
        .custom_fields
        .v
        .iter()
        .find(|f| f.name == name)
        .map_or("", |f| f.value.expose())
}

const SAMPLE_DB: &[u8] = include_bytes!("../../tests/fixtures/safeincloud/sample.db");

#[test]
fn safeincloud_database_opens_with_its_password() {
    assert_eq!(detect(SAMPLE_DB), Some(Format::SafeInCloudDb));
    assert!(needs_password(SAMPLE_DB));
    let missing = import(Format::SafeInCloudDb, SAMPLE_DB, None, AT).unwrap_err();
    assert!(missing.to_string().contains("password"));
    let wrong = import(Format::SafeInCloudDb, SAMPLE_DB, Some("not it"), AT).unwrap_err();
    assert!(wrong.to_string().contains("password"));

    let imported = import(Format::SafeInCloudDb, SAMPLE_DB, Some("foobar"), AT).unwrap();
    // Seven sample cards; the fourteen templates stay out.
    assert_eq!(imported.entries.len(), 7);
    let facebook = titled(&imported, "Facebook (Beispiel)");
    let login = facebook.entry.login().unwrap();
    assert_eq!(login.username.v, "john555@gmail.com");
    assert_eq!(login.password.v.expose(), "early91*Fail*");
    assert_eq!(facebook.entry.common.tags.v, vec!["Beispiele".to_owned()]);
    assert!(matches!(
        titled(&imported, "Notiz (Beispiel)").entry.kind,
        Kind::Note(_)
    ));
    // The Visa sample predates autofill hints, so its number stays a field.
    let visa = titled(&imported, "Visa Kreditkarte (Beispiel)");
    assert_eq!(field(visa, "Nummer"), "5555123456789000");
    assert_eq!(field(visa, "CVV"), "555");
}

#[test]
fn safeincloud_database_rejects_damage() {
    let mut cut = SAMPLE_DB.to_vec();
    cut.truncate(40);
    assert!(import(Format::SafeInCloudDb, &cut, Some("foobar"), AT).is_err());
    let mut flipped = SAMPLE_DB.to_vec();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xff;
    assert!(import(Format::SafeInCloudDb, &flipped, Some("foobar"), AT).is_err());
}

#[test]
fn safeincloud_xml_keeps_every_password_and_the_rest() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<database>
  <label name="Home" id="1" />
  <label name="Web Accounts" id="4" type="web_accounts" />
  <card title="Router" id="300" star="true">
    <field name="Network" type="text">attic</field>
    <field name="Admin password" type="password">admin-pw</field>
    <field name="Wi-Fi password" type="password">wifi-pw</field>
    <field name="Login" type="login">root</field>
    <field name="Other login" type="login">guest</field>
    <field name="Code" type="one_time_password">JBSWY3DPEHPK3PXP</field>
    <field name="URL" type="website">http://192.168.1.1</field>
    <label_id>1</label_id>
    <label_id>4</label_id>
    <file name="manual.txt">aGVsbG8=</file>
    <notes>in the attic</notes>
  </card>
  <card title="Visa" id="301">
    <field name="Number" type="number" autofill="cc-number">4111111111111111</field>
    <field name="Owner" type="text" autofill="cc-name">Jo Doe</field>
    <field name="Expires" type="expiry" autofill="cc-exp">05/29</field>
    <field name="CVV" type="pin" autofill="cc-csc">123</field>
    <field name="PIN" type="pin" autofill="off">9876</field>
    <field name="Blocking" type="phone" autofill="off">555-0100</field>
  </card>
  <card title="Plan" id="302" type="note"><notes>step one</notes></card>
  <card title="Gone" id="303" deleted="true"><field name="Login" type="login">x</field></card>
  <ghost id="12" time_stamp="1" />
</database>"#;
    let (format, imported) = run(xml.as_bytes());
    assert_eq!(format, Format::SafeInCloudXml);
    assert_eq!(imported.entries.len(), 3);

    let router = titled(&imported, "Router");
    let login = router.entry.login().unwrap();
    assert_eq!(login.password.v.expose(), "admin-pw");
    assert_eq!(
        field(router, "Wi-Fi password"),
        "wifi-pw",
        "the second password is kept"
    );
    assert_eq!(login.username.v, "root");
    assert_eq!(field(router, "Other login"), "guest");
    assert!(login.otp.v.is_some());
    assert!(router.entry.common.favourite.v);
    assert_eq!(
        router.entry.common.tags.v,
        vec!["Home".to_owned(), "Web Accounts".to_owned()]
    );
    assert_eq!(
        router.files,
        vec![("manual.txt".to_owned(), b"hello".to_vec())]
    );
    assert_eq!(router.entry.common.notes.v.expose(), "in the attic");

    let Kind::Card(card) = &titled(&imported, "Visa").entry.kind else {
        panic!("the Visa card must be a card")
    };
    assert_eq!(card.number.v.expose(), "4111111111111111");
    assert_eq!(
        (card.holder.v.as_str(), card.expiry.v.as_str()),
        ("Jo Doe", "05/29")
    );
    assert_eq!((card.cvv.v.expose(), card.pin.v.expose()), ("123", "9876"));
    assert_eq!(field(titled(&imported, "Visa"), "Blocking"), "555-0100");

    let Kind::Note(note) = &titled(&imported, "Plan").entry.kind else {
        panic!("a note card must be a note")
    };
    assert_eq!(note.body.v.expose(), "step one");
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn keepass_xml_reads_groups_codes_and_attachments() {
    let packed = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        gzip(b"key data"),
    );
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<KeePassFile>
  <Meta>
    <RecycleBinUUID>QmluQmluQmluQmluQmluQg==</RecycleBinUUID>
    <Binaries><Binary ID="0" Compressed="True">{packed}</Binary></Binaries>
  </Meta>
  <Root>
    <Group>
      <UUID>cm9vdHJvb3Ryb290cm9vdA==</UUID>
      <Name>Root</Name>
      <Entry>
        <String><Key>Title</Key><Value>Mail</Value></String>
        <String><Key>UserName</Key><Value>me</Value></String>
        <String><Key>Password</Key><Value ProtectInMemory="True">pw</Value></String>
        <String><Key>URL</Key><Value>https://m.example</Value></String>
        <String><Key>TimeOtp-Secret-Base32</Key><Value ProtectInMemory="True">JBSWY3DPEHPK3PXP</Value></String>
        <String><Key>TimeOtp-Period</Key><Value>60</Value></String>
        <String><Key>TimeOtp-Algorithm</Key><Value>HMAC-SHA-256</Value></String>
        <String><Key>PIN</Key><Value ProtectInMemory="True">4321</Value></String>
        <Binary><Key>key.txt</Key><Value Ref="0" /></Binary>
        <Tags>work;mail</Tags>
        <History>
          <Entry>
            <String><Key>Title</Key><Value>Mail (old)</Value></String>
          </Entry>
        </History>
      </Entry>
      <Group>
        <UUID>d29ya3dvcmt3b3Jrd29yaw==</UUID>
        <Name>Work</Name>
        <Entry>
          <String><Key>Title</Key><Value>VPN</Value></String>
          <String><Key>otp</Key><Value>key=JBSWY3DPEHPK3PXP&amp;step=30&amp;size=8</Value></String>
        </Entry>
      </Group>
      <Group>
        <UUID>QmluQmluQmluQmluQmluQg==</UUID>
        <Name>Recycle Bin</Name>
        <Entry><String><Key>Title</Key><Value>Deleted</Value></String></Entry>
      </Group>
    </Group>
  </Root>
</KeePassFile>"#
    );
    let (format, imported) = run(xml.as_bytes());
    assert_eq!(format, Format::KeePassXml);
    assert_eq!(
        imported.entries.len(),
        2,
        "history and the recycle bin stay out"
    );

    let mail = titled(&imported, "Mail");
    let login = mail.entry.login().unwrap();
    let otp = login.otp.v.as_ref().unwrap();
    assert_eq!(otp.kind, OtpKind::Totp { period: 60 });
    assert_eq!(otp.algorithm, OtpAlgorithm::Sha256);
    assert_eq!(field(mail, "PIN"), "4321");
    assert_eq!(
        field(mail, "TimeOtp-Period"),
        "",
        "fields read as the code are not repeated"
    );
    assert_eq!(
        mail.entry.common.tags.v,
        vec!["work".to_owned(), "mail".to_owned()]
    );
    assert_eq!(
        mail.files,
        vec![("key.txt".to_owned(), b"key data".to_vec())]
    );
    assert_eq!(mail.folder, None);

    let vpn = titled(&imported, "VPN");
    assert_eq!(vpn.folder.as_deref(), Some("Work"));
    assert_eq!(vpn.entry.login().unwrap().otp.v.as_ref().unwrap().digits, 8);
}

#[test]
fn keepass_xml_with_encrypted_values_says_how_to_export() {
    let xml = r#"<KeePassFile><Root><Group><Name>R</Name><Entry><String><Key>Password</Key><Value Protected="True">c2VjcmV0</Value></String></Entry></Group></Root></KeePassFile>"#;
    let error = import(Format::KeePassXml, xml.as_bytes(), None, AT).unwrap_err();
    assert!(error.to_string().contains("KeePass XML (2.x)"));
}

#[test]
fn kdbx_reads_keepass_native_codes_and_keeps_unreadable_ones() {
    let mut database = keepass::Database::new();
    database.config.kdf_config = keepass::config::KdfConfig::Argon2id {
        iterations: 1,
        memory: 1024 * 1024,
        parallelism: 1,
        version: argon2_for_keepass::Version::Version13,
    };
    {
        let mut root = database.root_mut();
        let mut native = root.add_entry();
        native.set_unprotected("Title", "Native");
        native.set_protected("TimeOtp-Secret-Base32", "JBSWY3DPEHPK3PXP");
        native.set_unprotected("TimeOtp-Length", "8");
        let mut broken = root.add_entry();
        broken.set_unprotected("Title", "Broken");
        broken.set_protected("otp", "not a code");
    }
    let mut bytes = Vec::new();
    database
        .save(&mut bytes, keepass::DatabaseKey::new().with_password("pw"))
        .unwrap();
    let imported = import(Format::Kdbx, &bytes, Some("pw"), AT).unwrap();
    let native = titled(&imported, "Native");
    assert_eq!(
        native.entry.login().unwrap().otp.v.as_ref().unwrap().digits,
        8
    );
    let broken = titled(&imported, "Broken");
    assert_eq!(
        field(broken, "otp"),
        "not a code",
        "an unreadable code is kept, not dropped"
    );
    assert_eq!(imported.skipped.len(), 1);
}

#[test]
fn kaspersky_text() {
    let text = "Websites\n\nWebsite name: GitHub\nWebsite URL: https://github.com\nLogin name: work\nLogin: octo\nPassword: pw1\nComment: first line\nsecond line\n\n---\n\nApplications\n\nApplication: Steam\nLogin name: \nLogin: gamer\nPassword: pw2\nComment: \n\n---\n\nNotes\n\nName: Wi-Fi\nText: hunter2\nin the hall\n\n---\n";
    let (format, imported) = run(text.as_bytes());
    assert_eq!(format, Format::KasperskyTxt);
    assert_eq!(imported.entries.len(), 3);
    let github = titled(&imported, "GitHub (work)");
    assert_eq!(github.entry.login().unwrap().password.v.expose(), "pw1");
    assert_eq!(
        github.entry.common.notes.v.expose(),
        "first line\nsecond line"
    );
    assert_eq!(
        titled(&imported, "Steam").entry.login().unwrap().username.v,
        "gamer"
    );
    let Kind::Note(note) = &titled(&imported, "Wi-Fi").entry.kind else {
        panic!("a note")
    };
    assert_eq!(note.body.v.expose(), "hunter2\nin the hall");
}

#[test]
fn zoho_buttercup_avira_avast_csv() {
    let zoho = "\"Password Name\",\"Description\",\"Password URL\",\"SecretData\",\"Notes\",\"CustomData\",\"Tags\",\"Classification\",\"Favorite\",\"TOTP\",\"Folder Name\"\nXYZ Test,,https://abc.example/login,\"SecretType:Web Account\nUser Name:email@domain.de\nPassword:PcY_IQEXIjKGj8YW\n\",,\"Pin:1234\",,E,1,otpauth://totp?secret=JBSWY3DPEHPK3PXP&algorithm=SHA1&period=30&digits=6,folderName\n";
    let (format, imported) = run(zoho.as_bytes());
    assert_eq!(format, Format::ZohoVaultCsv);
    let item = titled(&imported, "XYZ Test");
    let login = item.entry.login().unwrap();
    assert_eq!(
        (login.username.v.as_str(), login.password.v.expose()),
        ("email@domain.de", "PcY_IQEXIjKGj8YW")
    );
    assert!(login.otp.v.is_some());
    assert!(item.entry.common.favourite.v);
    assert_eq!(item.folder.as_deref(), Some("folderName"));
    assert_eq!(field(item, "Pin"), "1234");

    let buttercup = "!group_id,!group_name,title,username,password,URL,note,custom_field,id\n1,Work/Projects,Entry,user1,pass1,https://example.com,a note,custom value,entry1\n";
    let (format, imported) = run(buttercup.as_bytes());
    assert_eq!(format, Format::ButtercupCsv);
    let item = &imported.entries[0];
    assert_eq!(item.folder.as_deref(), Some("Work/Projects"));
    assert_eq!(item.entry.common.notes.v.expose(), "a note");
    assert_eq!(field(item, "custom_field"), "custom value");
    assert_eq!(
        item.entry.common.custom_fields.v.len(),
        1,
        "official columns are not fields"
    );

    let avira = "name,website,username,secondary_username,password\nMail,https://m.example,,me@m.example,pw\n";
    let (format, imported) = run(avira.as_bytes());
    assert_eq!(format, Format::AviraCsv);
    assert_eq!(
        imported.entries[0].entry.login().unwrap().username.v,
        "me@m.example"
    );

    let named_by_site = "url,username,password\nhttps://m.example/login,me,pw\n";
    let (format, imported) = run(named_by_site.as_bytes());
    assert_eq!(format, Format::ChromeCsv);
    assert_eq!(imported.entries[0].entry.common.title.v, "m.example");

    let avast = "name,web,login,password\nMail,https://m.example,me,pw\n";
    let (format, imported) = run(avast.as_bytes());
    assert_eq!(format, Format::AvastCsv);
    assert_eq!(
        imported.entries[0]
            .entry
            .login()
            .unwrap()
            .password
            .v
            .expose(),
        "pw"
    );
}

#[test]
fn msecure_rows_by_type() {
    let csv = "myCreditCard|155089404,Credit Card,,,Card Number|12|41111111111111111,Expiration Date|11|05/2026,Security Code|9|123,Name on Card|0|John Doe,PIN|9|1234,Issuing Bank|0|Visa,Phone Number|4|,Billing Address|0|,\nExample|188987444,Login,Work,This is a note \\nline two,Website|2|example2.com,Username|7|username || lol,Password|8|this is a password,\nWifi|1401978656,Secure Note,Unassigned,the hall,\n";
    let (format, imported) = run(csv.as_bytes());
    assert_eq!(format, Format::MSecureCsv);
    assert_eq!(imported.entries.len(), 3);
    let Kind::Card(card) = &titled(&imported, "myCreditCard").entry.kind else {
        panic!("a card")
    };
    assert_eq!(card.number.v.expose(), "41111111111111111");
    assert_eq!(
        (card.cvv.v.expose(), card.holder.v.as_str()),
        ("123", "John Doe")
    );
    assert_eq!(
        field(titled(&imported, "myCreditCard"), "Issuing Bank"),
        "Visa"
    );
    let example = titled(&imported, "Example");
    let login = example.entry.login().unwrap();
    assert_eq!(login.username.v, "username || lol");
    assert_eq!(login.password.v.expose(), "this is a password");
    assert_eq!(example.folder.as_deref(), Some("Work"));
    assert_eq!(
        example.entry.common.notes.v.expose(),
        "This is a note \nline two"
    );
    let wifi = titled(&imported, "Wifi");
    assert!(matches!(wifi.entry.kind, Kind::Note(_)));
    assert_eq!(wifi.folder, None);
}

#[test]
fn password_safe_and_sticky_password_xml() {
    let pwsafe = r#"<?xml version="1.0" encoding="UTF-8"?>
<passwordsafe delimiter="»" Database_Format="3.0">
  <entry id="1"><group>Work.Mail</group><title>Mail</title><username>me</username><password>pw</password><url>https://m.example</url><notes>one»two</notes><email>me@m.example</email></entry>
</passwordsafe>"#;
    let (format, imported) = run(pwsafe.as_bytes());
    assert_eq!(format, Format::PasswordSafeXml);
    let mail = &imported.entries[0];
    assert_eq!(mail.folder.as_deref(), Some("Work/Mail"));
    assert_eq!(mail.entry.common.notes.v.expose(), "one\ntwo");
    assert_eq!(field(mail, "Email"), "me@m.example");

    let sticky = r#"<?xml version="1.0" encoding="UTF-8"?>
<root>
  <Database>
    <Logins><Login ID="10" Name="me" Password="pw" /></Logins>
    <Accounts>
      <Account ID="20" Name="Mail" Link="https://m.example" Comments="a/nb" ParentID="2">
        <LoginLinks><Login SourceLoginID="10" /></LoginLinks>
      </Account>
    </Accounts>
    <Groups><Group ID="1" Name="Home" ParentID="0" /><Group ID="2" Name="Mail" ParentID="1" /></Groups>
  </Database>
</root>"#;
    let (format, imported) = run(sticky.as_bytes());
    assert_eq!(format, Format::StickyPasswordXml);
    let mail = &imported.entries[0];
    assert_eq!(mail.entry.common.title.v, "Mail");
    assert_eq!(mail.entry.login().unwrap().password.v.expose(), "pw");
    assert_eq!(mail.folder.as_deref(), Some("Home/Mail"));
    assert_eq!(mail.entry.common.notes.v.expose(), "a\nb");
}

#[test]
fn avast_and_psono_json() {
    let avast = r#"{"logins":[{"custName":"Mail","loginName":"me","pwd":"pw","url":"https://m.example","note":"n"}],
      "notes":[{"label":"Plan","text":"body"}],
      "cards":[{"custName":"Visa","holderName":"Jo","cardNumber":"4111","cvv":"123","expirationDate":{"month":5,"year":2029}}]}"#;
    let (format, imported) = run(avast.as_bytes());
    assert_eq!(format, Format::AvastJson);
    assert_eq!(imported.entries.len(), 3);
    let Kind::Card(card) = &titled(&imported, "Visa").entry.kind else {
        panic!("a card")
    };
    assert_eq!(card.expiry.v, "5/2029");

    let psono = r#"{"folders":[{"name":"Work","folders":[{"name":"Deep","items":[
        {"type":"website_password","name":"Mail","website_password_title":"Mail","website_password_url":"https://m.example","website_password_username":"me","website_password_password":"pw","website_password_notes":"n"}]}],
      "items":[{"type":"totp","name":"Code","totp_title":"Code","totp_period":30,"totp_algorithm":"SHA1","totp_digits":6,"totp_code":"JBSWY3DPEHPK3PXP","totp_notes":""}]}],
      "items":[{"type":"environment_variables","name":"Env","environment_variables_title":"Env","environment_variables_variables":[{"key":"TOKEN","value":"abc"}],"environment_variables_notes":""}]}"#;
    let (format, imported) = run(psono.as_bytes());
    assert_eq!(format, Format::PsonoJson);
    assert_eq!(imported.entries.len(), 3);
    assert_eq!(
        titled(&imported, "Mail").folder.as_deref(),
        Some("Work/Deep")
    );
    let code = titled(&imported, "Code");
    assert!(matches!(code.entry.kind, Kind::Otp(_)));
    assert_eq!(code.folder.as_deref(), Some("Work"));
    assert_eq!(field(titled(&imported, "Env"), "TOKEN"), "abc");
}

#[test]
fn stratum_lastpass_and_proton_authenticators() {
    let stratum = r#"{"Authenticators":[
        {"Type":2,"Issuer":"A","Username":"me","Secret":"JBSWY3DPEHPK3PXP","Algorithm":1,"Digits":8,"Period":60,"Counter":0},
        {"Type":1,"Issuer":"B","Username":"me","Secret":"JBSWY3DPEHPK3PXP","Algorithm":0,"Digits":6,"Period":30,"Counter":7},
        {"Type":5,"Issuer":"Yandex","Username":"me","Secret":"X","Algorithm":0,"Digits":8,"Period":30,"Counter":0}],
      "Categories":[{"Id":"c1","Name":"Work"}],
      "AuthenticatorCategories":[{"CategoryId":"c1","AuthenticatorSecret":"JBSWY3DPEHPK3PXP"}],
      "CustomIcons":[]}"#;
    let (format, imported) = run(stratum.as_bytes());
    assert_eq!(format, Format::Stratum);
    assert_eq!(imported.entries.len(), 2);
    assert_eq!(imported.skipped.len(), 1);
    let Kind::Otp(first) = &imported.entries[0].entry.kind else {
        panic!("a code")
    };
    let config = first.config.v.as_ref().unwrap();
    assert_eq!((config.digits, config.algorithm), (8, OtpAlgorithm::Sha256));
    assert_eq!(imported.entries[0].folder.as_deref(), Some("Work"));
    let Kind::Otp(second) = &imported.entries[1].entry.kind else {
        panic!("a code")
    };
    assert_eq!(
        second.config.v.as_ref().unwrap().kind,
        OtpKind::Hotp { counter: 7 }
    );

    let lastpass = r#"{"version":3,"accounts":[{"accountID":"1","issuerName":"GitHub","originalIssuerName":"GitHub","userName":"octo","originalUserName":"octo","pushNotification":false,"secret":"JBSWY3DPEHPK3PXP","timeStep":30,"digits":6,"creationTimestamp":1,"isFavorite":false,"algorithm":"SHA1"}]}"#;
    let (format, imported) = run(lastpass.as_bytes());
    assert_eq!(format, Format::LastPassAuthenticator);
    assert_eq!(imported.entries[0].entry.common.title.v, "GitHub");

    let proton = r#"{"version":1,"entries":[{"id":"a","content":{"id":"a","name":"GitHub","uri":"otpauth://totp/GitHub:octo?secret=JBSWY3DPEHPK3PXP&issuer=GitHub","entry_type":"Totp"},"note":"work"}]}"#;
    let (format, imported) = run(proton.as_bytes());
    assert_eq!(format, Format::ProtonAuthenticator);
    assert!(!needs_password(proton.as_bytes()));
    assert_eq!(imported.entries[0].entry.common.notes.v.expose(), "work");

    // The protected form wraps that same JSON.
    let sealed = seal_proton(proton, "hunter2");
    assert_eq!(detect(sealed.as_bytes()), Some(Format::ProtonAuthenticator));
    assert!(needs_password(sealed.as_bytes()));
    let asked = import(Format::ProtonAuthenticator, sealed.as_bytes(), None, AT).unwrap_err();
    assert!(asked.to_string().contains("password"));
    assert!(
        import(
            Format::ProtonAuthenticator,
            sealed.as_bytes(),
            Some("nope"),
            AT
        )
        .is_err()
    );
    let opened = import(
        Format::ProtonAuthenticator,
        sealed.as_bytes(),
        Some("hunter2"),
        AT,
    )
    .unwrap();
    assert_eq!(opened.entries.len(), 1);
}

fn seal_proton(json: &str, password: &str) -> String {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let salt = [7u8; 16];
    let nonce = [9u8; 12];
    let params = argon2::Params::new(19 * 1024, 2, 1, Some(32)).unwrap();
    let mut key = [0u8; 32];
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .unwrap();
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).unwrap();
    let body = cipher
        .encrypt(
            &aes_gcm::Nonce::try_from(&nonce[..]).unwrap(),
            Payload {
                msg: json.as_bytes(),
                aad: b"proton.authenticator.export.v1",
            },
        )
        .unwrap();
    let mut content = nonce.to_vec();
    content.extend(body);
    let b64 =
        |bytes: &[u8]| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    format!(
        r#"{{"version":1,"salt":"{}","content":"{}"}}"#,
        b64(&salt),
        b64(&content)
    )
}

#[test]
fn chrome_app_logins_get_their_app_and_a_title() {
    let csv = "name,url,username,password\n,android://N2H9MndUUUt3JuQSWAKexOU9oJLJeHR4nyUGac5E1TXKppkY7xtdRl6l8vKo1hQWCqAEy4gsNLUBIbVxpdmhOP==@com.xyz.example.app.android/,username@example.com,Qh6W4Wz55YGFNU\n";
    let (_, imported) = run(csv.as_bytes());
    let item = &imported.entries[0];
    assert_eq!(item.entry.common.title.v, "com.xyz.example.app.android");
    assert_eq!(
        item.entry.login().unwrap().site_matchers.v,
        vec![SiteMatcher::AndroidApp {
            package: "com.xyz.example.app.android".into(),
            cert_sha256: None
        }]
    );
}

#[test]
fn dashlane_reads_the_newer_otp_column() {
    let csv = "username,username2,username3,title,password,note,url,category,otpUrl\njdoe,,,example.com,pw,,https://www.example.com,Fun,otpauth://totp/x?secret=JBSWY3DPEHPK3PXP\n";
    let (format, imported) = run(csv.as_bytes());
    assert_eq!(format, Format::DashlaneCsv);
    assert!(imported.entries[0].entry.login().unwrap().otp.v.is_some());
}

#[test]
fn nordpass_keeps_kinds_extra_addresses_and_fields() {
    let head = "name,url,additional_urls,username,password,note,cardholdername,cardnumber,cvc,expirydate,zipcode,folder,full_name,phone_number,email,address1,address2,city,country,state,type,custom_fields\n";
    let rows = [
        "notesFolder,,,,,,,,,,,,,,,,,,,,,",
        "Mail,https://example.com,\"[\"\"https://example.net\"\"]\",me,pw,n,,,,,,F,,,,,,,,,password,\"[{\"\"label\"\":\"\"textLabel\"\",\"\"type\"\":\"\"text\"\",\"\"value\"\":\"\"text value\"\"},{\"\"label\"\":\"\"hiddenLabel\"\",\"\"type\"\":\"\"hidden\"\",\"\"value\"\":\"\"hidden value\"\"}]\"",
        "Visa,,,,,,Holder,4024007103939509,123,01 / 22,12345,,,,,,,,,,credit_card,",
        "Plan,,,,,secret note,,,,,,notesFolder,,,,,,,,,note,",
        "Me,,,,,,,,,,123456,,Jo Doe,123456789,jo@example.com,Street 1,,Cologne,Germany,NRW,identity,",
    ];
    let csv = format!("{head}{}\n", rows.join("\n"));
    let (format, imported) = run(csv.as_bytes());
    assert_eq!(format, Format::NordPassCsv);
    assert_eq!(imported.entries.len(), 4, "a folder row makes no entry");
    let mail = titled(&imported, "Mail");
    assert_eq!(mail.entry.login().unwrap().site_matchers.v.len(), 2);
    assert_eq!(field(mail, "hiddenLabel"), "hidden value");
    let Kind::Card(card) = &titled(&imported, "Visa").entry.kind else {
        panic!("a card")
    };
    assert_eq!(
        (card.expiry.v.as_str(), card.cvv.v.expose()),
        ("01/22", "123")
    );
    assert!(matches!(
        titled(&imported, "Plan").entry.kind,
        Kind::Note(_)
    ));
    let Kind::Identity(person) = &titled(&imported, "Me").entry.kind else {
        panic!("an identity")
    };
    assert_eq!(person.full_name.v, "Jo Doe");
    assert!(person.address.v.contains("Cologne"));
}

#[test]
fn enpass_notes_cards_apps_and_second_passwords() {
    let json = r#"{"folders":[],"items":[
      {"title":"Amazon","category":"login","favorite":1,"note":"","fields":[
        {"type":"username","label":"Username","value":"me","deleted":0,"sensitive":0},
        {"type":"password","label":"Password","value":"pw","deleted":0,"sensitive":1},
        {"type":"password","label":"Old password","value":"old","deleted":0,"sensitive":1},
        {"type":"password","label":"Removed","value":"gone","deleted":1,"sensitive":1},
        {"type":".Android#","label":"Autofill Info","value":"com.amazon.mShop","deleted":0,"sensitive":0},
        {"type":".Android#","label":"Autofill Info 1","value":"android://hash==@com.amazon.other","deleted":0,"sensitive":0}]},
      {"title":"Note","category":"note","note":"body","fields":[]},
      {"title":"Visa","category":"creditcard","note":"","fields":[
        {"type":"ccName","label":"Name","value":"Jo","deleted":0,"sensitive":0},
        {"type":"ccNumber","label":"Number","value":"4111","deleted":0,"sensitive":0},
        {"type":"ccCvc","label":"CVC","value":"123","deleted":0,"sensitive":1}]}]}"#;
    let (format, imported) = run(json.as_bytes());
    assert_eq!(format, Format::EnpassJson);
    let amazon = titled(&imported, "Amazon");
    let login = amazon.entry.login().unwrap();
    assert_eq!(login.password.v.expose(), "pw");
    assert_eq!(field(amazon, "Old password"), "old");
    assert_eq!(field(amazon, "Removed"), "", "deleted fields stay out");
    assert_eq!(login.site_matchers.v.len(), 2);
    assert!(amazon.entry.common.favourite.v);
    assert!(matches!(
        titled(&imported, "Note").entry.kind,
        Kind::Note(_)
    ));
    assert!(matches!(
        titled(&imported, "Visa").entry.kind,
        Kind::Card(_)
    ));
}

fn zip_of(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    for (name, content) in members {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap();
    buffer.into_inner()
}

#[test]
fn one_password_identities_keys_fields_and_files() {
    let export = r#"{"accounts":[{"vaults":[{"attrs":{"name":"Private"},"items":[
      {"categoryUuid":"001","state":"active","favIndex":0,
       "overview":{"title":"Bank","url":"https://bank.example"},
       "details":{"loginFields":[
          {"value":"me","name":"user","fieldType":"T","designation":"username"},
          {"value":"pw","name":"pass","fieldType":"P","designation":"password"},
          {"value":"1234","name":"branch pin","fieldType":"P"}],
        "sections":[{"title":"","fields":[
          {"title":"Contact","id":"c","value":{"email":{"email_address":"me@bank.example","provider":null}}},
          {"title":"Since","id":"s","value":{"monthYear":202605}},
          {"title":"Scan","id":"f","value":{"file":{"fileName":"scan.pdf","documentId":"doc123","decryptedSize":3}}}]}]}},
      {"categoryUuid":"004","state":"active","overview":{"title":"Me"},
       "details":{"sections":[{"title":"","fields":[
          {"title":"first name","id":"firstname","value":{"string":"Jo"}},
          {"title":"last name","id":"lastname","value":{"string":"Doe"}},
          {"title":"address","id":"address","value":{"address":{"street":"1 Road","city":"Town","country":"uk","zip":"AB1","state":""}}}]}]}},
      {"categoryUuid":"114","state":"active","overview":{"title":"Server key"},
       "details":{"sections":[{"title":"","fields":[
          {"title":"private key","id":"pk","value":{"sshKey":{"privateKey":"raw","metadata":{"privateKey":"-----BEGIN OPENSSH PRIVATE KEY-----","publicKey":"ssh-ed25519 AAAA","fingerprint":"SHA256:x","keyType":"ed25519"}}}}]}]}}]}]}]}"#;
    let archive = zip_of(&[
        ("export.data", export.as_bytes()),
        ("files/doc123___scan.pdf", b"pdf"),
    ]);
    let (format, imported) = run(&archive);
    assert_eq!(format, Format::OnePux);
    let bank = titled(&imported, "Bank");
    assert_eq!(field(bank, "branch pin"), "1234");
    assert_eq!(field(bank, "Contact"), "me@bank.example");
    assert_eq!(field(bank, "Since"), "05/2026");
    assert_eq!(bank.files, vec![("scan.pdf".to_owned(), b"pdf".to_vec())]);
    let Kind::Identity(person) = &titled(&imported, "Me").entry.kind else {
        panic!("an identity")
    };
    assert_eq!(person.full_name.v, "Jo Doe");
    assert!(person.address.v.starts_with("1 Road, Town"));
    let Kind::SshKey(key) = &titled(&imported, "Server key").entry.kind else {
        panic!("an SSH key")
    };
    assert!(key.private_key.v.expose().starts_with("-----BEGIN OPENSSH"));
    assert_eq!(key.public_key.v, "ssh-ed25519 AAAA");
}

#[test]
fn every_format_has_a_distinct_name_and_survives_junk() {
    let mut names: Vec<&str> = Format::ALL.iter().map(|f| f.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), Format::ALL.len());
    for format in Format::ALL {
        for junk in [
            &b""[..],
            b"\x05\x05\x01\x10",
            b"<root><Database>",
            b"{\"Authenticators\":3}",
            b"Websites\n\n---\n",
        ] {
            let _ = import(format, junk, Some("pw"), AT);
            let _ = import(format, junk, None, AT);
        }
    }
}

#[test]
fn deep_xml_is_refused_not_overflowed() {
    let xml = format!(
        "<KeePassFile>{}{}</KeePassFile>",
        "<Group>".repeat(10_000),
        "</Group>".repeat(10_000)
    );
    assert!(import(Format::KeePassXml, xml.as_bytes(), None, AT).is_err());
}
