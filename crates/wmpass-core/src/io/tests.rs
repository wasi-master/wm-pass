//! Fixtures are written by hand from each format's documented layout. Real exports
//! from each app should be added under `data/fixtures/<app>/` as they become available.

use std::io::Write;

use super::*;
use crate::clock::Hlc;
use crate::model::{Kind, Lww};
use crate::otp::{OtpConfig, OtpKind};
use crate::vault::login_entry;
use crate::vault::test_support::fast_vault;

const AT: Hlc = Hlc {
    ms: 1000,
    counter: 0,
    device: 1,
};

fn run(name: &str, data: &[u8]) -> (Format, Imported) {
    let format = detect(data).unwrap_or_else(|| panic!("{name} not detected"));
    let imported = import(format, data, None, AT).unwrap();
    (format, imported)
}

fn only_login(imported: &Imported) -> (&ImportedEntry, &Login) {
    assert_eq!(imported.entries.len(), 1);
    let item = &imported.entries[0];
    (item, item.entry.login().expect("a login"))
}

#[test]
fn chrome() {
    let csv = "name,url,username,password,note\nGitHub,https://github.com/,octo,hunter2,my note\n";
    let (format, imported) = run("c.csv", csv.as_bytes());
    assert_eq!(format, Format::ChromeCsv);
    let (item, login) = only_login(&imported);
    assert_eq!(item.entry.common.title.v, "GitHub");
    assert_eq!(login.username.v, "octo");
    assert_eq!(login.password.v.expose(), "hunter2");
    assert_eq!(item.entry.common.notes.v.expose(), "my note");
    assert_eq!(
        login.site_matchers.v,
        vec![SiteMatcher::Url("https://github.com/".into())]
    );
}

#[test]
fn chrome_columns_with_a_code_and_an_email_keep_both() {
    // Proton Pass writes Chrome's columns and more.
    let csv = "type,name,url,email,username,password,note,totp,createTime,modifyTime,vault\nlogin,Mail,https://mail.example,me@example.com,,pw,,otpauth://totp/x?secret=JBSWY3DPEHPK3PXP,1,2,Personal\n";
    let (format, imported) = run("p.csv", csv.as_bytes());
    assert_eq!(format, Format::ChromeCsv);
    let (item, login) = only_login(&imported);
    assert_eq!(login.username.v, "me@example.com");
    assert!(login.otp.v.is_some());
    assert_eq!(item.folder.as_deref(), Some("Personal"));
}

#[test]
fn firefox_names_entries_after_their_host() {
    let csv = "\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\",\"timeCreated\",\"timeLastUsed\",\"timePasswordChanged\"\n\"https://www.example.com\",\"me\",\"pw\",,\"https://www.example.com\",\"{x}\",\"1\",\"2\",\"3\"\n";
    let (format, imported) = run("f.csv", csv.as_bytes());
    assert_eq!(format, Format::FirefoxCsv);
    assert_eq!(
        only_login(&imported).0.entry.common.title.v,
        "www.example.com"
    );
}

#[test]
fn keepassxc_csv_keeps_group_and_totp() {
    let csv = "\"Group\",\"Title\",\"Username\",\"Password\",\"URL\",\"Notes\",\"TOTP\",\"Icon\",\"Last Modified\",\"Created\"\n\"Root/Work\",\"Mail\",\"me\",\"pw\",\"https://mail.example\",\"n\",\"otpauth://totp/x?secret=JBSWY3DPEHPK3PXP\",\"0\",\"\",\"\"\n";
    let (format, imported) = run("k.csv", csv.as_bytes());
    assert_eq!(format, Format::KeePassXcCsv);
    let (item, login) = only_login(&imported);
    assert_eq!(item.folder.as_deref(), Some("Work"));
    assert!(login.otp.v.is_some());
}

#[test]
fn lastpass_tells_notes_from_logins() {
    let csv = "url,username,password,totp,extra,name,grouping,fav\nhttps://a.example,me,pw,JBSWY3DPEHPK3PXP,x,A,Team,1\nhttp://sn,,,,secret text,My note,Team,0\n";
    let (format, imported) = run("l.csv", csv.as_bytes());
    assert_eq!(format, Format::LastPassCsv);
    assert_eq!(imported.entries.len(), 2);
    let login = imported.entries[0].entry.login().unwrap();
    assert_eq!(
        login.otp.v.as_ref().unwrap().secret.expose(),
        "JBSWY3DPEHPK3PXP"
    );
    assert!(imported.entries[0].entry.common.favourite.v);
    let Kind::Note(note) = &imported.entries[1].entry.kind else {
        panic!("expected a note")
    };
    assert_eq!(note.body.v.expose(), "secret text");
}

#[test]
fn bitwarden_csv() {
    let csv = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\nWork,1,login,Mail,n,\"pin: 1234\",0,\"https://a.example,https://b.example\",me,pw,\nWork,0,note,Plan,body,,0,,,,\n";
    let (format, imported) = run("b.csv", csv.as_bytes());
    assert_eq!(format, Format::BitwardenCsv);
    let login = imported.entries[0].entry.login().unwrap();
    assert_eq!(login.site_matchers.v.len(), 2);
    assert_eq!(
        imported.entries[0].entry.common.custom_fields.v[0]
            .value
            .expose(),
        "1234"
    );
    assert!(matches!(imported.entries[1].entry.kind, Kind::Note(_)));
}

#[test]
fn onepassword_csv_keeps_favourites_tags_and_archived_items() {
    let csv = "Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes\n\
Mail,https://m.example,me,pw,otpauth://totp/x?secret=JBSWY3DPEHPK3PXP,true,false,\"work,mail\",n\n\
Old,https://o.example,me,pw-old,,false,true,,\n";
    let (format, imported) = run("1p.csv", csv.as_bytes());
    assert_eq!(format, Format::OnePasswordCsv);
    let mail = &imported.entries[0].entry;
    assert!(mail.common.favourite.v);
    assert_eq!(mail.common.tags.v, vec!["work".to_owned(), "mail".to_owned()]);
    assert!(mail.login().unwrap().otp.v.is_some());
    assert_eq!(mail.common.notes.v.expose(), "n");
    let old = &imported.entries[1].entry;
    assert!(!old.common.favourite.v);
    assert_eq!(old.common.tags.v, vec!["Archived".to_owned()]);
}

#[test]
fn dashlane_apple_nordpass_roboform() {
    let cases: [(&str, Format, &str); 4] = [
        (
            "username,username2,username3,title,password,note,url,category,otpSecret\nme,alt,,Mail,pw,n,https://m.example,Work,\n",
            Format::DashlaneCsv,
            "pw",
        ),
        (
            "Title,URL,Username,Password,Notes,OTPAuth\nMail,https://m.example,me,pw2,,\n",
            Format::AppleCsv,
            "pw2",
        ),
        (
            "name,url,additional_urls,username,password,note,cardholdername,cardnumber,cvc,pin,expirydate,zipcode,folder,type\nMail,https://m.example,,me,pw3,,,,,,,,Work,password\n",
            Format::NordPassCsv,
            "pw3",
        ),
        (
            "Name,Url,MatchUrl,Login,Pwd,Note,Folder,RfFieldsV2\nMail,https://m.example,https://m.example/login,me,pw4,,/Work,\n",
            Format::RoboFormCsv,
            "pw4",
        ),
    ];
    for (csv, want, password) in cases {
        let (format, imported) = run("x.csv", csv.as_bytes());
        assert_eq!(format, want);
        assert_eq!(only_login(&imported).1.password.v.expose(), password);
    }
}

#[test]
fn mapped_csv() {
    let text = "a,b,c\nMail,me,pw\n";
    let map = ColumnMap {
        title: Some(0),
        username: Some(1),
        password: Some(2),
        ..ColumnMap::default()
    };
    let imported = import_mapped_csv(text, &map, true, AT).unwrap();
    assert_eq!(only_login(&imported).1.password.v.expose(), "pw");
    assert_eq!(preview_csv(text, 1).unwrap(), vec![vec!["a", "b", "c"]]);
}

#[test]
fn bitwarden_json_all_kinds() {
    let json = r#"{"encrypted":false,"folders":[{"id":"f1","name":"Work"}],"items":[
      {"type":1,"name":"Mail","notes":"n","favorite":true,"folderId":"f1",
       "fields":[{"name":"pin","value":"1","type":1}],
       "passwordHistory":[{"lastUsedDate":"2020","password":"older"}],
       "login":{"username":"me","password":"pw","totp":"otpauth://totp/x?secret=JBSWY3DPEHPK3PXP",
         "uris":[{"uri":"https://a.example"}],
         "fido2Credentials":[{"credentialId":"cid","rpId":"a.example","rpName":"A","userHandle":"uh","userName":"me","keyValue":"pk","counter":"3"}]}},
      {"type":2,"name":"Plan","notes":"body","secureNote":{"type":0}},
      {"type":3,"name":"Visa","card":{"cardholderName":"Me","brand":"Visa","number":"4111","expMonth":"1","expYear":"2030","code":"123"}},
      {"type":4,"name":"Me","identity":{"firstName":"A","lastName":"B","passportNumber":"P1","city":"Dhaka"}},
      {"type":5,"name":"Key","sshKey":{"privateKey":"priv","publicKey":"pub","keyFingerprint":"fp"}}
    ]}"#;
    let (format, imported) = run("b.json", json.as_bytes());
    assert_eq!(format, Format::BitwardenJson);
    assert_eq!(imported.entries.len(), 5);
    let mail = &imported.entries[0];
    assert_eq!(mail.folder.as_deref(), Some("Work"));
    let login = mail.entry.login().unwrap();
    assert_eq!(login.passkeys.v[0].rp_id, "a.example");
    assert_eq!(login.passkeys.v[0].counter, 3);
    assert_eq!(mail.entry.history[0].value.expose(), "older");
    assert!(matches!(
        mail.entry.common.custom_fields.v[0].kind,
        crate::model::FieldKind::Hidden
    ));
    let Kind::Note(note) = &imported.entries[1].entry.kind else {
        panic!()
    };
    assert_eq!(note.body.v.expose(), "body");
    let Kind::Card(card) = &imported.entries[2].entry.kind else {
        panic!()
    };
    assert_eq!(
        (card.number.v.expose(), card.expiry.v.as_str()),
        ("4111", "1/2030")
    );
    let Kind::Identity(person) = &imported.entries[3].entry.kind else {
        panic!()
    };
    assert_eq!(person.full_name.v, "A B");
    assert_eq!(person.document_numbers.v[0].value.expose(), "P1");
    assert!(matches!(imported.entries[4].entry.kind, Kind::SshKey(_)));
}

#[test]
fn encrypted_exports_say_what_to_do() {
    let bw = r#"{"encrypted":true,"data":"x"}"#;
    assert_eq!(detect(bw.as_bytes()), Some(Format::BitwardenJson));
    assert!(
        import(Format::BitwardenJson, bw.as_bytes(), None, AT)
            .unwrap_err()
            .to_string()
            .contains("unencrypted")
    );
    let aegis = r#"{"version":1,"header":{"slots":[]},"db":"cipher"}"#;
    assert!(
        import(Format::Aegis, aegis.as_bytes(), None, AT)
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
}

#[test]
fn enpass_keeper_proton() {
    let enpass = r#"{"folders":[{"uuid":"f","title":"Work"}],"items":[{"title":"Mail","note":"n","folders":["f"],"trashed":0,
      "fields":[{"label":"User","type":"username","value":"me"},{"label":"Pass","type":"password","value":"pw"},
      {"label":"Site","type":"url","value":"https://m.example"},{"label":"PIN","type":"pin","value":"1","sensitive":1}]},
      {"title":"Gone","trashed":1,"fields":[]}]}"#;
    let (format, imported) = run("e.json", enpass.as_bytes());
    assert_eq!(format, Format::EnpassJson);
    let (item, login) = only_login(&imported);
    assert_eq!(
        (login.username.v.as_str(), item.folder.as_deref()),
        ("me", Some("Work"))
    );

    let keeper = r#"{"records":[{"title":"Mail","login":"me","password":"pw","login_url":"https://m.example","notes":"n",
      "custom_fields":{"$oneTimeCode":"otpauth://totp/x?secret=JBSWY3DPEHPK3PXP","Team":"A"},"folders":[{"folder":"Work"}]}]}"#;
    let (format, imported) = run("k.json", keeper.as_bytes());
    assert_eq!(format, Format::KeeperJson);
    let (item, login) = only_login(&imported);
    assert!(login.otp.v.is_some());
    assert_eq!(item.entry.common.custom_fields.v.len(), 1);
    let shared = r#"{"records":[{"title":"Wiki","login":"me","password":"pw2","folders":[{"shared_folder":"Team","can_edit":true}]}]}"#;
    let (_, imported) = run("k.json", shared.as_bytes());
    assert_eq!(only_login(&imported).0.folder.as_deref(), Some("Team"));

    let proton = r#"{"encrypted":false,"vaults":{"v":{"name":"Personal","items":[
      {"state":1,"data":{"type":"login","metadata":{"name":"Mail","note":"n"},"extraFields":[{"fieldName":"pin","type":"hidden","data":{"content":"1"}}],
       "content":{"itemEmail":"me@x","itemUsername":"","password":"pw","urls":["https://m.example"],"totpUri":""}}},
      {"state":1,"data":{"type":"creditCard","metadata":{"name":"Visa","note":""},"content":{"cardholderName":"Me","number":"4111","verificationNumber":"123","expirationDate":"2030-01","pin":""}}},
      {"state":2,"data":{"type":"login","metadata":{"name":"Trashed"},"content":{}}},
      {"state":1,"data":{"type":"alias","metadata":{"name":"An alias"},"content":{}}}
    ]}}}"#;
    let (format, imported) = run("p.json", proton.as_bytes());
    assert_eq!(format, Format::ProtonPass);
    assert_eq!(imported.entries.len(), 2);
    assert_eq!(
        imported.entries[0].entry.login().unwrap().username.v,
        "me@x"
    );
    assert_eq!(
        imported.skipped.len(),
        1,
        "the alias must be reported, not silently dropped"
    );
}

fn zip_with(name: &str, content: &str) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    writer
        .start_file(name, zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(content.as_bytes()).unwrap();
    writer.finish().unwrap();
    buffer.into_inner()
}

#[test]
fn one_password_1pux() {
    let export = r#"{"accounts":[{"vaults":[{"attrs":{"name":"Private"},"items":[
      {"categoryUuid":"001","state":"active","favIndex":1,
       "overview":{"title":"Mail","url":"https://m.example","urls":[{"url":"https://m.example"},{"url":"https://alt.example"}],"tags":["t"]},
       "details":{"notesPlain":"n","passwordHistory":[{"value":"old","time":100}],
         "loginFields":[{"designation":"username","value":"me"},{"designation":"password","value":"pw"}],
         "sections":[{"fields":[{"id":"x","title":"one-time password","value":{"totp":"otpauth://totp/x?secret=JBSWY3DPEHPK3PXP"}},
                                {"id":"y","title":"Recovery","value":{"concealed":"code"}}]}]}},
      {"categoryUuid":"003","state":"active","overview":{"title":"Note"},"details":{"notesPlain":"body"}},
      {"categoryUuid":"002","state":"active","overview":{"title":"Visa"},"details":{"sections":[{"fields":[
        {"id":"cardholder","title":"n","value":{"string":"Me"}},{"id":"ccnum","title":"c","value":{"creditCardNumber":"4111"}},{"id":"cvv","title":"v","value":{"concealed":"123"}},
        {"id":"type","title":"type","value":{"cctype":"visa"}},{"id":"bank","title":"issuing bank","value":{"string":"Big Bank"}}]}]}},
      {"categoryUuid":"110","state":"active","overview":{"title":"Server"},"details":{"sections":[{"fields":[{"id":"u","title":"URL","value":{"string":"x"}}]}]}},
      {"categoryUuid":"001","state":"archived","overview":{"title":"Old"},"details":{}}
    ]}]}]}"#;
    let (format, imported) = run("e.1pux", &zip_with("export.data", export));
    assert_eq!(format, Format::OnePux);
    assert_eq!(imported.entries.len(), 5);
    let mail = &imported.entries[0];
    let login = mail.entry.login().unwrap();
    assert_eq!(login.password.v.expose(), "pw");
    assert_eq!(login.site_matchers.v.len(), 2);
    assert!(login.otp.v.is_some());
    assert_eq!(mail.entry.history[0].at.ms, 100_000);
    let Kind::Card(card) = &imported.entries[2].entry.kind else {
        panic!()
    };
    assert_eq!(card.number.v.expose(), "4111");
    assert_eq!(card.issuer.v, "visa");
    let card_fields = &imported.entries[2].entry.common.custom_fields.v;
    assert_eq!(card_fields.len(), 1, "only the field with no place of its own");
    assert_eq!(card_fields[0].value.expose(), "Big Bank");
    assert_eq!(imported.entries[3].entry.common.custom_fields.v.len(), 1);
    // The 110 item is kept as a note, and says so; the archived one comes in tagged.
    assert_eq!(imported.skipped.len(), 1, "{:?}", imported.skipped);
    let old = imported.entries.iter().find(|i| i.entry.common.title.v == "Old").unwrap();
    assert_eq!(old.entry.common.tags.v, vec!["Archived".to_owned()]);
}

#[test]
fn proton_zip() {
    let json = r#"{"vaults":{"v":{"name":"P","items":[{"state":1,"data":{"type":"login","metadata":{"name":"Mail","note":""},"content":{"itemUsername":"me","password":"pw","urls":[]}}}]}}}"#;
    let (format, imported) = run("p.zip", &zip_with("Proton Pass/data.json", json));
    assert_eq!(format, Format::ProtonPass);
    assert_eq!(only_login(&imported).1.password.v.expose(), "pw");
}

#[test]
fn safeincloud_xml_decodes_entities_and_skips_templates() {
    let xml = r#"<?xml version="1.0"?><database><card title="Tom &amp; Co" template="false"><field name="Login" type="login">me</field><field name="Password" type="password">a&lt;b&amp;c</field><field name="Web" type="website">https://x.example</field><field name="PIN" type="pin">1</field><notes>hi</notes></card><card title="T" template="true"><field name="Login" type="login">no</field></card></database>"#;
    let (format, imported) = run("s.xml", xml.as_bytes());
    assert_eq!(format, Format::SafeInCloudXml);
    let (item, login) = only_login(&imported);
    assert_eq!(item.entry.common.title.v, "Tom & Co");
    assert_eq!(login.password.v.expose(), "a<b&c");
    assert_eq!(item.entry.common.notes.v.expose(), "hi");
}

#[test]
fn xml_bombs_do_not_expand() {
    let xml = r#"<?xml version="1.0"?><!DOCTYPE d [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">]><database><card title="t"><field name="L" type="login">&b;</field></card></database>"#;
    let imported = import(Format::SafeInCloudXml, xml.as_bytes(), None, AT).unwrap();
    assert_eq!(only_login(&imported).1.username.v, "");
}

#[test]
fn authenticators() {
    const SEED: &str = "JBSWY3DPEHPK3PXP";
    let aegis = r#"{"version":1,"header":{"slots":null,"params":null},"db":{"entries":[
      {"type":"totp","name":"me","issuer":"A","info":{"secret":"JBSWY3DPEHPK3PXP","algo":"SHA256","digits":8,"period":60}},
      {"type":"hotp","name":"me","issuer":"B","info":{"secret":"JBSWY3DPEHPK3PXP","algo":"SHA1","digits":6,"counter":5}},
      {"type":"motp","name":"me","issuer":"C","info":{"secret":"X"}}]}}"#;
    let (format, imported) = run("a.json", aegis.as_bytes());
    assert_eq!(format, Format::Aegis);
    assert_eq!(imported.entries.len(), 2);
    assert_eq!(imported.skipped.len(), 1);
    let Kind::Otp(first) = &imported.entries[0].entry.kind else {
        panic!()
    };
    let config = first.config.v.as_ref().unwrap();
    assert_eq!(
        (config.digits, config.kind),
        (8, OtpKind::Totp { period: 60 })
    );
    let Kind::Otp(second) = &imported.entries[1].entry.kind else {
        panic!()
    };
    assert_eq!(
        second.config.v.as_ref().unwrap().kind,
        OtpKind::Hotp { counter: 5 }
    );

    let two_fas = format!(
        r#"{{"services":[{{"name":"A","secret":"{SEED}","otp":{{"account":"me","digits":6,"period":30,"algorithm":"SHA1","tokenType":"TOTP"}}}}],"schemaVersion":4}}"#
    );
    let (format, imported) = run("t.2fas", two_fas.as_bytes());
    assert_eq!(format, Format::TwoFas);
    assert_eq!(imported.entries.len(), 1);

    let and = format!(
        r#"[{{"secret":"{SEED}","issuer":"A","label":"me","digits":6,"type":"TOTP","algorithm":"SHA1","period":30}}]"#
    );
    let (format, imported) = run("a.json", and.as_bytes());
    assert_eq!(format, Format::AndOtp);
    assert_eq!(imported.entries.len(), 1);

    let free = r#"{"tokenOrder":["a"],"tokens":[{"algo":"SHA1","counter":0,"digits":6,"issuerExt":"A","label":"me","period":30,"secret":[72,-101,109],"type":"TOTP"}]}"#;
    let (format, imported) = run("f.json", free.as_bytes());
    assert_eq!(format, Format::FreeOtpPlus);
    let Kind::Otp(token) = &imported.entries[0].entry.kind else {
        panic!()
    };
    assert_eq!(
        crate::otp::base32_decode(token.config.v.as_ref().unwrap().secret.expose())
            .unwrap()
            .as_slice(),
        &[72, 155, 109]
    );

    let raivo = format!(
        r#"[{{"issuer":"A","account":"me","secret":"{SEED}","algorithm":"SHA1","digits":"6","kind":"TOTP","timer":"30","counter":"0"}}]"#
    );
    let (format, imported) = run("r.json", raivo.as_bytes());
    assert_eq!(format, Format::Raivo);
    assert_eq!(imported.entries.len(), 1);

    let list = format!("otpauth://totp/A:me?secret={SEED}\nnot a link\n");
    let (format, imported) = run("l.txt", list.as_bytes());
    assert_eq!(format, Format::OtpAuthList);
    assert_eq!((imported.entries.len(), imported.skipped.len()), (1, 1));
    // As a QR code in its alphanumeric mode gives it.
    let shouted = format!("OTPAUTH://TOTP/A:ME?SECRET={SEED}\n");
    let (format, imported) = run("u.txt", shouted.as_bytes());
    assert_eq!(format, Format::OtpAuthList);
    assert_eq!(imported.entries.len(), 1);
}

#[test]
fn google_migration_link() {
    // Field numbers per Google's OtpParameters message: one TOTP entry.
    let mut item = Vec::new();
    item.extend_from_slice(&[0x0a, 3, 72, 101, 108]); // secret "Hel"
    item.extend_from_slice(&[0x12, 2, b'm', b'e']); // name
    item.extend_from_slice(&[0x1a, 1, b'A']); // issuer
    item.extend_from_slice(&[0x20, 1, 0x28, 1, 0x30, 2]); // SHA1, six digits, TOTP
    let mut payload = vec![0x0a, item.len() as u8];
    payload.extend_from_slice(&item);
    payload.extend_from_slice(&[0x10, 1, 0x18, 1]); // version, batch
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &payload);
    let link = format!(
        "otpauth-migration://offline?data={}",
        crate::otp::percent_encode(&encoded)
    );
    let found = authenticators::google_migration(&link).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].issuer, "A");
    assert_eq!(found[0].account, "me");
    assert_eq!(found[0].digits, 6);
    assert_eq!(
        crate::otp::base32_decode(found[0].secret.expose())
            .unwrap()
            .as_slice(),
        b"Hel"
    );
    assert!(authenticators::google_migration("otpauth-migration://offline?data=!!!").is_err());
}

#[test]
fn hostile_inputs_fail_cleanly() {
    for format in [
        Format::BitwardenJson,
        Format::OnePux,
        Format::SafeInCloudXml,
        Format::Aegis,
        Format::ProtonPass,
        Format::ChromeCsv,
    ] {
        for junk in [
            &b""[..],
            b"\xff\xfe\x00",
            b"PK\x03\x04garbage",
            b"{",
            b"<database><card",
            b"[1,2,3]",
            b"\"\n\"",
        ] {
            let _ = import(format, junk, None, AT);
        }
    }
    assert_eq!(detect(b"\xff\xfe\x00"), None);
    assert_eq!(detect(b"hello world"), None);
}

#[test]
fn importing_an_authenticator_twice_does_not_double_its_codes() {
    let mut vault = fast_vault(1);
    let list = "otpauth://totp/A:me?secret=JBSWY3DPEHPK3PXP\notpauth://totp/B:me?secret=GEZDGNBVGY3TQOJQ\n";
    let read = || import(Format::OtpAuthList, list.as_bytes(), None, AT).unwrap();
    let first = apply(&mut vault, read(), 2000).unwrap();
    assert_eq!((first.added, first.duplicates), (2, 0));
    let again = apply(&mut vault, read(), 3000).unwrap();
    assert_eq!((again.added, again.duplicates), (0, 2));
    // The same seed typed another way is the same code; another length of code is not.
    let other = "otpauth://totp/A:me?secret=jbsw%20y3dp%20ehpk%203pxp\notpauth://totp/A:me?secret=JBSWY3DPEHPK3PXP&digits=8\n";
    let third = apply(&mut vault, import(Format::OtpAuthList, other.as_bytes(), None, AT).unwrap(), 4000).unwrap();
    assert_eq!((third.added, third.duplicates), (1, 1));
}

#[test]
fn a_scheme_alone_is_no_address() {
    let csv = "url,username,password,totp,extra,name,grouping,fav\nhttp://,me,pw,,,Bank,,0\n";
    let (_, imported) = run("l.csv", csv.as_bytes());
    let (item, login) = only_login(&imported);
    assert!(login.site_matchers.v.is_empty(), "{:?}", login.site_matchers.v);
    assert_eq!(item.entry.common.title.v, "Bank");
}

#[test]
fn apply_folders_dedupes_and_seals_files() {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut existing = login_entry("Mail", "me", "pw", at);
    existing.login_mut().unwrap().site_matchers =
        Lww::new(vec![SiteMatcher::Url("https://m.example".into())], at);
    vault.add(existing, 2000).unwrap();

    let draft = |title: &str, password: &str, folder: &str| Draft {
        title: title.into(),
        username: "me".into(),
        password: password.into(),
        urls: vec!["https://m.example".into()],
        folder: folder.into(),
        ..Draft::default()
    };
    let mut skipped = Vec::new();
    let duplicate = draft("Mail copy", "pw", "Work").into_login(AT, &mut skipped);
    let mut fresh = draft("Other", "new", "Work").into_login(AT, &mut skipped);
    fresh.files.push(("a.txt".into(), b"hello".to_vec()));
    let also_fresh = draft("Third", "newer", "Work").into_login(AT, &mut skipped);
    let imported = Imported {
        entries: vec![duplicate, fresh, also_fresh],
        skipped,
    };

    let report = apply(&mut vault, imported, 3000).unwrap();
    assert_eq!((report.added, report.duplicates), (2, 1));
    assert_eq!(
        vault.doc.folders.len(),
        1,
        "one folder however many entries name it"
    );
    let (blob, sealed) = &report.blobs[0];
    let holder = vault
        .doc
        .entries
        .values()
        .find(|e| e.common.title.v == "Other")
        .unwrap();
    let record = &holder.common.attachments.v[0];
    assert_eq!(&record.blob, blob);
    assert_eq!(
        Vault::open_attachment(record, sealed).unwrap().as_slice(),
        b"hello"
    );
}

fn sample_vault() -> Vault {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut login = login_entry("Mail <b>", "me", "pa\"ss,w\nord", at);
    {
        let body = login.login_mut().unwrap();
        body.site_matchers = Lww::new(vec![SiteMatcher::Url("https://m.example".into())], at);
        body.otp = Lww::new(Some(OtpConfig::totp("JBSWY3DPEHPK3PXP")), at);
    }
    login.common.tags = Lww::new(vec!["work".into()], at);
    login.common.custom_fields = Lww::new(
        vec![crate::model::Field {
            name: "pin".into(),
            value: "1".into(),
            kind: crate::model::FieldKind::Hidden,
        }],
        at,
    );
    vault.add(login, 2000).unwrap();
    let mut card = crate::model::Entry::default();
    card.common.title = Lww::new("Visa".into(), at);
    card.kind = Kind::Card(crate::model::Card {
        number: Lww::new("4111".into(), at),
        expiry: Lww::new("1/2030".into(), at),
        ..crate::model::Card::default()
    });
    vault.add(card, 2000).unwrap();
    vault
}

#[test]
fn csv_export_round_trips_logins_and_reports_other_kinds() {
    let vault = sample_vault();
    let csv = export::to_csv(&vault).unwrap();
    let (format, imported) = run("e.csv", csv.expose().as_bytes());
    assert_eq!(format, Format::WmPassCsv);
    assert_eq!(imported.entries.len(), 2);
    let login = imported
        .entries
        .iter()
        .find_map(|i| i.entry.login())
        .unwrap();
    assert_eq!(
        login.password.v.expose(),
        "pa\"ss,w\nord",
        "quoting must survive"
    );
    assert!(login.otp.v.is_some());
    let card = imported
        .entries
        .iter()
        .find(|i| i.entry.common.title.v == "Visa")
        .unwrap();
    assert!(
        card.entry
            .common
            .custom_fields
            .v
            .iter()
            .any(|f| f.value.expose() == "4111"),
        "values must not be lost"
    );
    assert_eq!(imported.skipped.len(), 1);
}

#[test]
fn a_note_comes_back_from_csv_as_the_note_it_was() {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut note = crate::model::Entry::default();
    note.common.title = Lww::new("Door".into(), at);
    note.common.tags = Lww::new(vec!["home".into()], at);
    note.common.custom_fields = Lww::new(vec![crate::model::Field { name: "Floor".into(), value: "3".into(), ..Default::default() }], at);
    note.kind = Kind::Note(crate::model::Note { body: Lww::new("code 1234\nside door".into(), at) });
    vault.add(note, 2000).unwrap();
    let csv = export::to_csv(&vault).unwrap();
    let (_, imported) = run("e.csv", csv.expose().as_bytes());
    let back = &imported.entries[0].entry;
    let Kind::Note(body) = &back.kind else { panic!("a note") };
    assert_eq!(body.body.v.expose(), "code 1234\nside door");
    assert_eq!(back.common.custom_fields.v.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["Floor"]);
    assert_eq!(back.common.tags.v, vec!["home".to_owned()]);
    assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
}

#[test]
fn bitwarden_export_reads_back() {
    let vault = sample_vault();
    let json = export::to_bitwarden_json(&vault).unwrap();
    let (format, imported) = run("b.json", json.expose().as_bytes());
    assert_eq!(format, Format::BitwardenJson);
    let login = imported
        .entries
        .iter()
        .find_map(|i| i.entry.login())
        .unwrap();
    assert_eq!(login.password.v.expose(), "pa\"ss,w\nord");
    assert!(login.otp.v.is_some());
    let Some(Kind::Card(card)) = imported
        .entries
        .iter()
        .map(|i| &i.entry.kind)
        .find(|k| matches!(k, Kind::Card(_)))
    else {
        panic!("the card must stay a card");
    };
    assert_eq!(card.number.v.expose(), "4111");
}

#[test]
fn print_page_escapes_everything() {
    let vault = sample_vault();
    let html = export::to_print_html(&vault);
    assert!(html.expose().contains("Mail &lt;b&gt;"));
    assert!(!html.expose().contains("<b>"));
    assert!(html.expose().contains("4111"));
}

#[test]
fn kdbx_export_import_round_trip() {
    let vault = sample_vault();
    let entries: Vec<&crate::model::Entry> = vault.doc.entries.values().collect();
    let bytes = kdbx::export(&entries, "pw").unwrap();
    assert_eq!(detect(&bytes), Some(Format::Kdbx));
    assert!(import(Format::Kdbx, &bytes, Some("wrong"), AT).is_err());
    assert!(import(Format::Kdbx, &bytes, None, AT).is_err());
    let imported = import(Format::Kdbx, &bytes, Some("pw"), AT).unwrap();
    assert_eq!(imported.entries.len(), 2);
    let item = imported
        .entries
        .iter()
        .find(|i| i.entry.common.title.v == "Mail <b>")
        .unwrap();
    let login = item.entry.login().unwrap();
    assert_eq!(login.password.v.expose(), "pa\"ss,w\nord");
    assert_eq!(
        login.otp.v.as_ref().unwrap().secret.expose(),
        "JBSWY3DPEHPK3PXP"
    );
    assert_eq!(item.entry.common.tags.v, vec!["work".to_owned()]);
    assert_eq!(item.entry.common.custom_fields.v[0].value.expose(), "1");
    let visa = imported
        .entries
        .iter()
        .find(|i| i.entry.common.title.v == "Visa")
        .unwrap();
    assert!(
        visa.entry
            .common
            .custom_fields
            .v
            .iter()
            .any(|f| f.value.expose() == "4111")
    );
    assert!(kdbx::export(&entries, "").is_err());
}

#[test]
fn kdbx_in_place_keeps_structure_and_history() {
    let mut database = keepass::Database::new();
    database.config.kdf_config = keepass::config::KdfConfig::Argon2id {
        iterations: 1,
        memory: 1024 * 1024,
        parallelism: 1,
        version: argon2_for_keepass::Version::Version13,
    };
    {
        let mut root = database.root_mut();
        let mut group = root.add_group();
        group.name = "Team".into();
        let mut entry = group.add_entry();
        entry.set_unprotected("Title", "Mail");
        entry.set_unprotected("UserName", "me");
        entry.set_protected("Password", "first");
        entry.set_protected("Custom", "keepme");
    }
    let mut bytes = Vec::new();
    database
        .save(&mut bytes, keepass::DatabaseKey::new().with_password("pw"))
        .unwrap();

    let mut opened = kdbx::InPlace::open(&bytes, "pw", None).unwrap();
    let listed = opened.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].group.as_str(), listed[0].username.as_str()),
        ("Team", "me")
    );
    assert_eq!(opened.password(&listed[0].id).unwrap().expose(), "first");
    opened
        .set_password(&listed[0].id, &Secret::new("second"))
        .unwrap();
    let saved = opened.save("pw", None).unwrap();

    let back =
        keepass::Database::parse(&saved, keepass::DatabaseKey::new().with_password("pw")).unwrap();
    let root = back.root();
    let group = root.groups().next().unwrap();
    assert_eq!(group.name, "Team");
    let entry = group.entries().next().unwrap();
    assert_eq!(entry.get_password(), Some("second"));
    assert_eq!(entry.get("Custom"), Some("keepme"));
    let history = entry.history.as_ref().unwrap().get_entries();
    assert_eq!(history[0].get_password(), Some("first"));
    assert!(
        opened
            .set_password("no-such-id", &Secret::new("x"))
            .is_err()
    );
}

#[test]
fn keepassxc_csv_root_entries_land_in_no_folder() {
    let csv = "\"Group\",\"Title\",\"Username\",\"Password\",\"URL\",\"Notes\"\n\"Root\",\"Mail\",\"me\",\"pw\",\"https://mail.example\",\"\"\n\"Root/Root\",\"Odd\",\"me\",\"pw2\",\"https://odd.example\",\"\"\n";
    let (format, imported) = run("k.csv", csv.as_bytes());
    assert_eq!(format, Format::KeePassXcCsv);
    assert_eq!(imported.entries[0].folder, None);
    // A group the person named Root under the root stays a folder.
    assert_eq!(imported.entries[1].folder.as_deref(), Some("Root"));
}

#[test]
fn bitwarden_card_without_expiry_has_none() {
    let json = r#"{"encrypted":false,"folders":[],"items":[
      {"type":3,"name":"Gift","card":{"cardholderName":"Me","number":"4111","expMonth":null,"expYear":null}}
    ]}"#;
    let (_, imported) = run("b.json", json.as_bytes());
    let Kind::Card(card) = &imported.entries[0].entry.kind else {
        panic!()
    };
    assert_eq!(card.expiry.v, "");
}

#[test]
fn bitwarden_export_writes_steam_codes_its_own_way() {
    let mut vault = fast_vault(1);
    let at = vault.tick(1000);
    let mut entry = login_entry("Steam", "me", "pw", at);
    // Typed the way a page shows it, the seed still goes out as Bitwarden reads one.
    let config = OtpConfig::from_uri("steam://jbsw y3dp ehpk 3pxp").unwrap();
    entry.login_mut().unwrap().otp = Lww::new(Some(config), at);
    vault.add(entry, 1000).unwrap();
    let json = export::to_bitwarden_json(&vault).unwrap();
    assert!(json.expose().contains("\"totp\": \"steam://JBSWY3DPEHPK3PXP\""), "{}", json.expose());
    let (_, imported) = run("b.json", json.expose().as_bytes());
    let (_, login) = only_login(&imported);
    assert_eq!(login.otp.v.as_ref().unwrap().kind, OtpKind::Steam);
}

#[test]
fn bitwarden_match_settings_survive_both_ways() {
    use crate::model::SiteMatcher;
    let json = r#"{"encrypted":false,"folders":[],"items":[
      {"type":1,"name":"Strict","login":{"username":"me","password":"pw","uris":[
        {"uri":"https://login.example.com","match":1},
        {"uri":"^https://[a-z]+\\.corp\\.example/","match":4}]}},
      {"type":1,"name":"Never","login":{"username":"me","password":"pw2","uris":[{"uri":"https://never.example","match":5}]}},
      {"type":1,"name":"Plain","login":{"username":"me","password":"pw3","uris":[{"uri":"https://plain.example","match":null}]}}
    ]}"#;
    let (_, imported) = run("b.json", json.as_bytes());
    let matchers = |i: usize| imported.entries[i].entry.login().unwrap().site_matchers.v.clone();
    assert_eq!(
        matchers(0),
        vec![
            SiteMatcher::Host("https://login.example.com".into()),
            SiteMatcher::Regex(r"^https://[a-z]+\.corp\.example/".into())
        ]
    );
    assert!(matchers(1).contains(&SiteMatcher::Never));
    assert_eq!(matchers(2), vec![SiteMatcher::Url("https://plain.example".into())]);

    let mut vault = fast_vault(1);
    for item in imported.entries {
        vault.add(item.entry, 1000).unwrap();
    }
    let json = export::to_bitwarden_json(&vault).unwrap();
    let (_, back) = run("b.json", json.expose().as_bytes());
    let by_title = |title: &str| {
        back.entries.iter().find(|i| i.entry.common.title.v == title).unwrap().entry.login().unwrap().site_matchers.v.clone()
    };
    assert!(by_title("Strict").contains(&SiteMatcher::Host("https://login.example.com".into())));
    assert!(by_title("Never").contains(&SiteMatcher::Never));
    assert_eq!(by_title("Plain"), vec![SiteMatcher::Url("https://plain.example".into())]);
}

#[test]
fn bitwarden_export_keeps_a_card_expiry_in_any_form() {
    assert_eq!(export::expiry_parts("12/27"), Some(("12".into(), "2027".into())));
    assert_eq!(export::expiry_parts("2027-03"), Some(("3".into(), "2027".into())));
    assert_eq!(export::expiry_parts("202712"), Some(("12".into(), "2027".into())));
    assert_eq!(export::expiry_parts("0528"), Some(("5".into(), "2028".into())));
    assert_eq!(export::expiry_parts("1/2030"), Some(("1".into(), "2030".into())));
    assert_eq!(export::expiry_parts("soon"), None);
    assert_eq!(export::expiry_parts("12 / 27"), Some(("12".into(), "2027".into())));
    let mut vault = fast_vault(1);
    let at = vault.tick(1000);
    let mut entry = login_entry("Odd card", "", "", at);
    entry.kind = Kind::Card(crate::model::Card { expiry: Lww::new("end of 2027".into(), at), ..Default::default() });
    vault.add(entry, 1000).unwrap();
    let json = export::to_bitwarden_json(&vault).unwrap();
    assert!(json.expose().contains("end of 2027"), "{}", json.expose());
}

#[test]
fn bitwarden_organisation_collections_become_folders() {
    let json = r#"{"encrypted":false,"collections":[{"id":"c1","name":"Shared"}],"items":[
      {"type":1,"name":"Team","collectionIds":["c1"],"login":{"username":"me","password":"pw"}}]}"#;
    let (_, imported) = run("b.json", json.as_bytes());
    assert_eq!(imported.entries[0].folder.as_deref(), Some("Shared"));
}

#[test]
fn spreadsheet_csvs_lose_blank_rows_and_read_semicolons() {
    let csv = "name,url,username,password\nMail,https://mail.example,me,pw\n,,,\n\n";
    let (_, imported) = run("c.csv", csv.as_bytes());
    assert_eq!(imported.entries.len(), 1);
    let map = ColumnMap { title: Some(0), password: Some(1), ..ColumnMap::default() };
    let semi = "Title;Secret\nMail;pw\n;\n";
    let mapped = import_mapped_csv(semi, &map, true, AT).unwrap();
    assert_eq!(only_login(&mapped).1.password.v.expose(), "pw");
}

#[test]
fn an_export_saved_again_by_a_spreadsheet_still_reads() {
    // Excel in much of Europe saves with semicolons, and as "Unicode Text" with tabs.
    let semi = "name;url;username;password\nMail;https://mail.example;me;p,w\n";
    let (format, imported) = run("c.csv", semi.as_bytes());
    assert_eq!(format, Format::ChromeCsv);
    assert_eq!(only_login(&imported).1.password.v.expose(), "p,w");
    let tabs = "name\turl\tusername\tpassword\nMail\thttps://mail.example\tme\tpw\n";
    let mut data = vec![0xff, 0xfe];
    data.extend(tabs.encode_utf16().flat_map(u16::to_le_bytes));
    let (format, imported) = run("c.txt", &data);
    assert_eq!(format, Format::ChromeCsv);
    assert_eq!(only_login(&imported).1.username.v, "me");
}

#[test]
fn utf16_exports_from_windows_read() {
    let csv = "name,url,username,password\nCafé,https://cafe.example,me,pässword\n";
    let mut data = vec![0xff, 0xfe];
    data.extend(csv.encode_utf16().flat_map(u16::to_le_bytes));
    let (format, imported) = run("c.csv", &data);
    assert_eq!(format, Format::ChromeCsv);
    let (item, login) = only_login(&imported);
    assert_eq!(item.entry.common.title.v, "Café");
    assert_eq!(login.password.v.expose(), "pässword");
    let latin1 = b"name,url,username,password\nCaf\xe9,https://cafe.example,me,pw\n";
    assert!(import(Format::ChromeCsv, latin1, None, AT).unwrap_err().to_string().contains("UTF-8"));
}

#[test]
fn upper_case_otp_links_in_an_import_read() {
    let mut skipped = Vec::new();
    let config = parse_otp("OTPAUTH://TOTP/ACME:ME?SECRET=JBSWY3DPEHPK3PXP", "ACME", "me", &mut skipped).unwrap();
    assert!(skipped.is_empty());
    assert_eq!(config.account, "ME");
    let seed = parse_otp("jbsw y3dp ehpk 3pxp", "Site", "me", &mut skipped).unwrap();
    assert_eq!(seed.secret.expose(), "JBSWY3DPEHPK3PXP");
}

#[test]
fn a_login_only_in_the_trash_is_imported_again() {
    let mut vault = fast_vault(1);
    let at = vault.tick(1000);
    let mut old = login_entry("Mail", "me", "pw", at);
    old.login_mut().unwrap().site_matchers = Lww::new(vec![crate::model::SiteMatcher::Url("https://mail.example".into())], at);
    let id = vault.add(old, 1000).unwrap();
    vault.trash(&id, 2000).unwrap();
    let csv = "name,url,username,password\nMail,https://mail.example,me,pw\n";
    let (_, imported) = run("c.csv", csv.as_bytes());
    let report = apply(&mut vault, imported, 3000).unwrap();
    assert_eq!((report.added, report.duplicates), (1, 0));
}

#[test]
fn bitwarden_password_history_keeps_its_dates_both_ways() {
    let json = r#"{"encrypted":false,"folders":[],"items":[
      {"type":1,"name":"Mail","login":{"username":"me","password":"new"},
       "passwordHistory":[{"lastUsedDate":"2024-04-08T13:25:14.123Z","password":"old"},{"password":"older"}]}
    ]}"#;
    let (_, imported) = run("b.json", json.as_bytes());
    let times = |entry: &crate::model::Entry| entry.history_of("password").map(|r| (r.value.expose().to_owned(), r.at.ms)).collect::<Vec<_>>();
    assert_eq!(times(&imported.entries[0].entry), vec![("old".to_owned(), 1_712_582_714_123), ("older".to_owned(), 0)]);

    let mut vault = fast_vault(1);
    for item in imported.entries {
        vault.add(item.entry, 1000).unwrap();
    }
    let json = export::to_bitwarden_json(&vault).unwrap();
    assert!(json.expose().contains(r#""lastUsedDate": "2024-04-08T13:25:14.123Z""#));
    assert!(json.expose().contains(r#""lastUsedDate": "1970-01-01T00:00:00.000Z""#));
    let (_, back) = run("b.json", json.expose().as_bytes());
    let mut kept = times(&back.entries[0].entry);
    kept.sort();
    assert_eq!(kept, vec![("old".to_owned(), 1_712_582_714_123), ("older".to_owned(), 0)]);
}

#[test]
fn wmpass_csv_keeps_every_matching_rule_and_old_readers_never_widen_one() {
    use crate::matching::{Target, host_of, match_entry, matcher_from_address};
    use crate::model::SiteMatcher;
    let cert = "ab".repeat(32);
    let strict = vec![
        SiteMatcher::Url("https://plain.example/login".into()),
        SiteMatcher::Host("https://login.example.com".into()),
        SiteMatcher::Domain("corp.example".into()),
        SiteMatcher::Regex(r"^https://[a-z]+\.corp\.example/a b%20".into()),
        SiteMatcher::AndroidApp { package: "com.example.app".into(), cert_sha256: Some(cert.clone()) },
    ];
    let mut never = strict.clone();
    never.push(SiteMatcher::Never);
    let mut vault = fast_vault(1);
    for (title, matchers) in [("Strict", strict.clone()), ("Never", never.clone())] {
        let at = vault.tick(1000);
        let mut entry = login_entry(title, "me", "pw", at);
        entry.login_mut().unwrap().site_matchers = Lww::new(matchers, at);
        vault.add(entry, 1000).unwrap();
    }
    let csv = export::to_csv(&vault).unwrap();
    let (_, back) = run("e.csv", csv.expose().as_bytes());
    let by_title = |title: &str| back.entries.iter().find(|i| i.entry.common.title.v == title).unwrap().entry.clone();
    // A domain rule goes out as a plain address, so other apps can use it, and comes back as one:
    // that matches the same registrable domain, as the rule did.
    let strict_back: Vec<SiteMatcher> = strict
        .iter()
        .map(|m| match m {
            SiteMatcher::Domain(d) => matcher_from_address(d),
            other => other.clone(),
        })
        .collect();
    assert_eq!(by_title("Strict").login().unwrap().site_matchers.v, strict_back);
    assert_eq!(by_title("Never").login().unwrap().site_matchers.v, never);

    // An older version read every token as a plain address. Each tagged one then has no host,
    // so it fills nowhere: not the subdomain a host rule kept out, and not a never-fill login.
    let mut reader = csv::Reader::from_reader(csv.expose().as_bytes());
    for record in reader.records() {
        let record = record.unwrap();
        let old: Vec<SiteMatcher> = record[5].split(' ').map(matcher_from_address).collect();
        for token in record[5].split(' ').filter(|t| t.starts_with("wmpass:")) {
            assert_eq!(host_of(token), None, "{token}");
        }
        let at = vault.tick(2000);
        let mut entry = login_entry(&record[2], "me", "pw", at);
        entry.login_mut().unwrap().site_matchers = Lww::new(old, at);
        let fills = |url: &str| match_entry(&entry, Target::Web { url }).is_some();
        assert!(!fills("https://other.example.com/"), "{}", &record[2]);
        // The domain rule, written plainly, still covers its own subdomains; nothing else does.
        assert_eq!(fills("https://a.corp.example/"), &record[2] == "Strict", "{}", &record[2]);
        assert_eq!(fills("https://plain.example/"), &record[2] == "Strict");
    }
}

#[test]
fn nested_folders_go_out_as_paths_and_come_back_nested() {
    let mut vault = fast_vault(1);
    let work = vault.add_folder("Work", None, 2000).unwrap();
    let mail = vault.add_folder("Mail", Some(work.clone()), 2000).unwrap();
    // A second folder called Mail elsewhere must not be confused with this one.
    vault.add_folder("Mail", None, 2000).unwrap();
    let at = vault.tick(2000);
    let mut entry = login_entry("Inbox", "me", "pw-one", at);
    entry.common.folder = Lww::new(Some(mail), at);
    vault.add(entry, 2000).unwrap();

    let csv = export::to_csv(&vault).unwrap();
    assert!(csv.expose().contains(",Work/Mail,"), "{}", csv.expose());
    let json = export::to_bitwarden_json(&vault).unwrap();
    let root: serde_json::Value = serde_json::from_str(json.expose()).unwrap();
    let names: Vec<&str> = root["folders"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str()).collect();
    assert!(names.contains(&"Work/Mail") && names.contains(&"Mail"), "{names:?}");

    // Read back into an empty vault, the path becomes a folder inside a folder.
    let mut fresh = fast_vault(2);
    let imported = import(Format::BitwardenJson, json.expose().as_bytes(), None, AT).unwrap();
    apply(&mut fresh, imported, 3000).unwrap();
    let folder_of = |vault: &Vault, name: &str, parent: Option<&str>| {
        vault
            .doc
            .folders
            .values()
            .find(|f| f.name.v == name && f.parent.v.as_deref() == parent)
            .map(|f| f.id.clone())
    };
    let work = folder_of(&fresh, "Work", None).expect("Work at the top");
    let mail = folder_of(&fresh, "Mail", Some(&work)).expect("Mail inside Work");
    let inbox = fresh.doc.entries.values().find(|e| e.common.title.v == "Inbox").unwrap();
    assert_eq!(inbox.common.folder.v.as_deref(), Some(mail.as_str()));
    // Bringing it in again uses the folders already there.
    let again = import(Format::KeePassXcCsv, b"Group,Title,Username,Password,URL,Notes\nRoot/Work/Mail,Other,me,pw-two,,\n", None, AT).unwrap();
    apply(&mut fresh, again, 4000).unwrap();
    assert_eq!(fresh.doc.folders.values().filter(|f| f.name.v == "Work").count(), 1);
    let other = fresh.doc.entries.values().find(|e| e.common.title.v == "Other").unwrap();
    assert_eq!(other.common.folder.v.as_deref(), Some(mail.as_str()));
}
