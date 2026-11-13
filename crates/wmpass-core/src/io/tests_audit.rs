//! Each test pins one importer or export to what the app on the other side really writes or
//! reads. Fixtures are small and inline, following each app's own layout.

use std::io::Write;

use super::*;
use crate::clock::Hlc;
use crate::model::{Field, FieldKind, Kind, Lww, Passkey};
use crate::otp::{OtpConfig, OtpKind};
use crate::vault::login_entry;
use crate::vault::test_support::fast_vault;

const AT: Hlc = Hlc {
    ms: 1000,
    counter: 0,
    device: 1,
};

fn run(data: &[u8]) -> (Format, Imported) {
    let format = detect(data).expect("detected");
    (format, import(format, data, None, AT).unwrap())
}

fn titled<'a>(imported: &'a Imported, title: &str) -> &'a ImportedEntry {
    imported
        .entries
        .iter()
        .find(|item| item.entry.common.title.v == title)
        .unwrap_or_else(|| panic!("no {title} among {:?}", imported.entries.iter().map(|i| &i.entry.common.title.v).collect::<Vec<_>>()))
}

fn field<'a>(item: &'a ImportedEntry, name: &str) -> Option<&'a Field> {
    item.entry.common.custom_fields.v.iter().find(|f| f.name == name)
}

fn value<'a>(item: &'a ImportedEntry, name: &str) -> &'a str {
    field(item, name).map_or("", |f| f.value.expose())
}

fn zip_of(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(&mut buffer);
    for (name, bytes) in members {
        writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
    buffer.into_inner()
}

#[test]
fn lastpass_groupings_nest_by_backslash_and_none_is_no_folder() {
    let csv = "url,username,password,totp,extra,name,grouping,fav\n\
        https://a.example,me,pw1,,,A,Work\\Mail,0\n\
        https://b.example,me,pw2,,,B,(none),0\n\
        http://sn,,,,secret text,Door,Home,1\n";
    let (format, imported) = run(csv.as_bytes());
    assert_eq!(format, Format::LastPassCsv);
    assert_eq!(titled(&imported, "A").folder.as_deref(), Some("Work/Mail"));
    assert_eq!(titled(&imported, "B").folder, None);
    let door = titled(&imported, "Door");
    assert!(door.entry.common.favourite.v, "a favourite note stays one");
    assert_eq!(door.folder.as_deref(), Some("Home"));
}

#[test]
fn lastpass_credit_card_notes_become_cards() {
    let extra = "NoteType:Credit Card\nLanguage:en-US\nName on Card:Ann Lee\nType:Visa\nNumber:4111111111111111\nSecurity Code:123\nStart Date:,\nExpiration Date:June,2027\nNotes:first line\nsecond line";
    let csv = format!("url,username,password,totp,extra,name,grouping,fav\nhttp://sn,,,,\"{extra}\",My Visa,,0\n");
    let (_, imported) = run(csv.as_bytes());
    let item = titled(&imported, "My Visa");
    let Kind::Card(card) = &item.entry.kind else { panic!("a card") };
    assert_eq!(card.holder.v, "Ann Lee");
    assert_eq!(card.number.v.expose(), "4111111111111111");
    assert_eq!(card.cvv.v.expose(), "123");
    assert_eq!(card.expiry.v, "06/2027");
    assert_eq!(card.issuer.v, "Visa");
    assert_eq!(item.entry.common.notes.v.expose(), "first line\nsecond line");
    assert!(item.entry.common.custom_fields.v.is_empty(), "{:?}", item.entry.common.custom_fields.v);
}

#[test]
fn bitwarden_csv_reads_collections_and_keeps_a_notes_fields() {
    let csv = "collections,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
        \"Team,Other\",login,Router,,,0,https://r.example,admin,pw,\n\
        Team,note,Wi-Fi,the body,\"Floor: 3\",0,,,,\n";
    let (format, imported) = run(csv.as_bytes());
    assert_eq!(format, Format::BitwardenCsv);
    assert_eq!(titled(&imported, "Router").folder.as_deref(), Some("Team"));
    let note = titled(&imported, "Wi-Fi");
    assert_eq!(note.folder.as_deref(), Some("Team"));
    assert_eq!(value(note, "Floor"), "3");
}

#[test]
fn buttercup_group_rows_make_no_entries_and_nest() {
    let csv = "!type,!group_id,!group_name,!group_parent,id,title,username,password,URL\n\
        group,1,General,0,,,,,\n\
        group,2,Mail,1,,,,,\n\
        entry,2,Mail,,e1,Inbox,me,pw,https://m.example\n";
    let (format, imported) = run(csv.as_bytes());
    assert_eq!(format, Format::ButtercupCsv);
    assert_eq!(imported.entries.len(), 1, "group rows are no entries");
    let inbox = titled(&imported, "Inbox");
    assert_eq!(inbox.folder.as_deref(), Some("General/Mail"));
    assert!(field(inbox, "!group_parent").is_none());
}

#[test]
fn proton_pass_csv_notes_are_notes() {
    let csv = "type,name,url,email,username,password,note,totp,createTime,modifyTime,vault\n\
        note,Door code,,,,,1234,,1700000000,1700000000,Personal\n\
        login,Mail,https://m.example,me@example.com,,pw,,,1700000000,1700000000,Personal\n";
    let (_, imported) = run(csv.as_bytes());
    let door = titled(&imported, "Door code");
    let Kind::Note(note) = &door.entry.kind else { panic!("a note") };
    assert_eq!(note.body.v.expose(), "1234");
    assert_eq!(door.folder.as_deref(), Some("Personal"));
    assert!(titled(&imported, "Mail").entry.login().is_some());
}

#[test]
fn wmpass_csv_keeps_favourites() {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut login = login_entry("Mail", "me", "pw", at);
    login.common.favourite = Lww::new(true, at);
    vault.add(login, 2000).unwrap();
    let csv = export::to_csv(&vault).unwrap();
    let (_, imported) = run(csv.expose().as_bytes());
    assert!(titled(&imported, "Mail").entry.common.favourite.v);
}

#[test]
fn bitwarden_identity_keeps_its_title_company_and_username() {
    let json = r#"{"encrypted":false,"folders":[],"items":[{"type":4,"name":"Me","identity":{
        "title":"Dr","firstName":"Ann","lastName":"Lee","company":"Acme","username":"alee",
        "email":"a@example.com"}}]}"#;
    let (_, imported) = run(json.as_bytes());
    let me = titled(&imported, "Me");
    assert_eq!(value(me, "Title"), "Dr");
    assert_eq!(value(me, "Company"), "Acme");
    assert_eq!(value(me, "Username"), "alee");
}

#[test]
fn bitwarden_passkey_ids_written_as_guids_become_their_bytes() {
    let json = r#"{"encrypted":false,"folders":[],"items":[{"type":1,"name":"Site","login":{
        "username":"me","password":"pw","uris":[],"fido2Credentials":[
        {"credentialId":"b9d84a0c-1e2f-4a3b-8c4d-5e6f708192a3","keyType":"public-key","keyAlgorithm":"ECDSA",
         "keyCurve":"P-256","keyValue":"AAEC","rpId":"site.example","userHandle":"dXNlcg","userName":"me",
         "counter":"3","rpName":"Site","creationDate":"2024-04-08T13:25:14.123Z"},
        {"credentialId":"b64.AQID","keyValue":"AAEC","rpId":"other.example"}]}}]}"#;
    let (_, imported) = run(json.as_bytes());
    let login = titled(&imported, "Site").entry.login().unwrap();
    let keys = &login.passkeys.v;
    assert_eq!(keys[0].credential_id, "udhKDB4vSjuMTV5vcIGSow", "the GUID's 16 bytes");
    assert_eq!(keys[0].counter, 3);
    assert_eq!(keys[0].created_ms, 1_712_582_714_123);
    assert_eq!(keys[1].credential_id, "AQID");
}

#[test]
fn a_bitwarden_login_with_only_a_code_is_a_code_both_ways() {
    // As Bitwarden Authenticator exports, and as a code item goes out to Bitwarden.
    let json = r#"{"encrypted":false,"items":[{"type":1,"name":"GitHub","favorite":true,"folderId":null,
        "login":{"totp":"otpauth://totp/GitHub:me?secret=JBSWY3DPEHPK3PXP&issuer=GitHub"}}]}"#;
    let (format, imported) = run(json.as_bytes());
    assert_eq!(format, Format::BitwardenJson);
    let item = titled(&imported, "GitHub");
    let Kind::Otp(otp) = &item.entry.kind else { panic!("a code item") };
    assert_eq!(otp.config.v.as_ref().unwrap().secret.expose(), "JBSWY3DPEHPK3PXP");
    assert!(item.entry.common.favourite.v);
    // Read twice, it is not doubled.
    let mut vault = fast_vault(1);
    apply(&mut vault, imported.clone(), 2000).unwrap();
    assert_eq!(apply(&mut vault, imported, 3000).unwrap().duplicates, 1);
    // Out to Bitwarden it is a login with that code, the only place Bitwarden makes codes.
    let out = export::to_bitwarden_json(&vault).unwrap();
    let root: serde_json::Value = serde_json::from_str(out.expose()).unwrap();
    assert_eq!(root["items"][0]["type"], 1);
    assert!(root["items"][0]["login"]["totp"].as_str().unwrap().starts_with("otpauth://totp/"));
}

#[test]
fn keeper_nested_and_shared_folders_and_typed_fields() {
    let json = r#"{"records":[{"title":"A","login":"me","password":"pw","login_url":"https://a.example",
        "custom_fields":{"$oneTimeCode":"otpauth://totp/A:me?secret=JBSWY3DPEHPK3PXP","$text:Hint":"blue",
            "$phone:Mobile":{"number":"555-1234","type":"Mobile"},"$pinCode":"9999"},
        "folders":[{"shared_folder":"Team","folder":"Ops\\Prod"}]},
        {"title":"B","login":"me","password":"pw2","folders":[{"folder":"Home\\Bills"}]}]}"#;
    let (format, imported) = run(json.as_bytes());
    assert_eq!(format, Format::KeeperJson);
    let a = titled(&imported, "A");
    assert_eq!(a.folder.as_deref(), Some("Team/Ops/Prod"));
    assert!(a.entry.login().unwrap().otp.v.is_some());
    assert_eq!(value(a, "Hint"), "blue");
    assert!(value(a, "Mobile").contains("555-1234"), "an object value is kept");
    assert_eq!(field(a, "pinCode").unwrap().kind, FieldKind::Hidden);
    assert_eq!(titled(&imported, "B").folder.as_deref(), Some("Home/Bills"));
}

#[test]
fn proton_pass_json_brings_every_kind_and_keeps_pins_and_emails() {
    let json = r#"{"encrypted":false,"vaults":{"s1":{"name":"Personal","items":[
      {"state":1,"pinned":true,"data":{"type":"login","metadata":{"name":"Mail","note":""},"extraFields":[],
        "content":{"itemUsername":"me","itemEmail":"me@example.com","password":"pw","urls":["https://m.example"],"totpUri":"","passkeys":[]}}},
      {"state":1,"data":{"type":"creditCard","metadata":{"name":"Visa","note":""},
        "extraFields":[{"fieldName":"Bank","type":"text","data":{"content":"Big Bank"}}],
        "content":{"cardholderName":"Ann","number":"4111","verificationNumber":"123","expirationDate":"2027-12","pin":"1"}}},
      {"state":1,"data":{"type":"identity","metadata":{"name":"Me","note":""},"extraFields":[],
        "content":{"fullName":"Ann Lee","email":"a@example.com","phoneNumber":"555","streetAddress":"1 Main St","city":"Town",
          "company":"Acme","passportNumber":"P123","extraPersonalDetails":[{"fieldName":"Nickname","type":"text","data":{"content":"Annie"}}],
          "extraSections":[{"sectionName":"More","sectionFields":[{"fieldName":"Blood","type":"text","data":{"content":"O+"}}]}]}}},
      {"state":1,"data":{"type":"sshKey","metadata":{"name":"Server","note":""},"extraFields":[],
        "content":{"privateKey":"-----BEGIN OPENSSH PRIVATE KEY-----","publicKey":"ssh-ed25519 AAAA","sections":[]}}},
      {"state":1,"data":{"type":"wifi","metadata":{"name":"Home Wi-Fi","note":""},"extraFields":[],
        "content":{"ssid":"home","password":"wifipw","security":2,"sections":[]}}},
      {"state":1,"data":{"type":"custom","metadata":{"name":"Locker","note":"gym"},"extraFields":[],
        "content":{"sections":[{"sectionName":"Main","sectionFields":[{"fieldName":"Combo","type":"hidden","data":{"content":"1-2-3"}}]}]}}}
    ]}}}"#;
    let (format, imported) = run(json.as_bytes());
    assert_eq!(format, Format::ProtonPass);
    assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
    let mail = titled(&imported, "Mail");
    assert!(mail.entry.common.favourite.v, "pinned is a favourite");
    assert_eq!(mail.entry.login().unwrap().username.v, "me");
    assert_eq!(value(mail, "Email"), "me@example.com", "the email beside a username is kept");
    let visa = titled(&imported, "Visa");
    let Kind::Card(card) = &visa.entry.kind else { panic!("a card") };
    assert_eq!(card.expiry.v, "12/2027");
    assert_eq!(value(visa, "Bank"), "Big Bank");
    let me = titled(&imported, "Me");
    let Kind::Identity(identity) = &me.entry.kind else { panic!("an identity") };
    assert_eq!(identity.full_name.v, "Ann Lee");
    assert_eq!(identity.address.v, "1 Main St, Town");
    assert_eq!(identity.document_numbers.v[0].value.expose(), "P123");
    assert_eq!(value(me, "Company"), "Acme");
    assert_eq!(value(me, "Nickname"), "Annie");
    assert_eq!(value(me, "Blood"), "O+");
    let Kind::SshKey(key) = &titled(&imported, "Server").entry.kind else { panic!("a key") };
    assert_eq!(key.public_key.v, "ssh-ed25519 AAAA");
    let Kind::Wifi(wifi) = &titled(&imported, "Home Wi-Fi").entry.kind else { panic!("Wi-Fi") };
    assert_eq!((wifi.ssid.v.as_str(), wifi.password.v.expose()), ("home", "wifipw"));
    let locker = titled(&imported, "Locker");
    assert_eq!(field(locker, "Combo").unwrap().kind, FieldKind::Hidden);
}

#[test]
fn enpass_keeps_attachments_and_nested_folders() {
    let json = r#"{"folders":[{"uuid":"f1","title":"Work","parent_uuid":""},{"uuid":"f2","title":"Mail","parent_uuid":"f1"}],
      "items":[{"title":"Inbox","category":"login","folders":["f2"],"note":"","favorite":0,"trashed":0,
        "attachments":[{"name":"key.txt","kind":"text/plain","data":"aGVsbG8="}],
        "fields":[{"type":"username","label":"Username","value":"me","deleted":0,"sensitive":0},
                  {"type":"password","label":"Password","value":"pw","deleted":0,"sensitive":1}]}]}"#;
    let (format, imported) = run(json.as_bytes());
    assert_eq!(format, Format::EnpassJson);
    let inbox = titled(&imported, "Inbox");
    assert_eq!(inbox.folder.as_deref(), Some("Work/Mail"));
    assert_eq!(inbox.files, vec![("key.txt".to_owned(), b"hello".to_vec())]);
}

#[test]
fn psono_cards_and_ssh_keys() {
    let json = r#"{"folders":[],"items":[
      {"type":"credit_card","name":"Visa","credit_card_title":"Visa","credit_card_number":"4111","credit_card_cvc":"123",
       "credit_card_pin":"1","credit_card_name":"Ann","credit_card_valid_through":"12/27","credit_card_notes":"n"},
      {"type":"ssh_own_key","name":"Box","ssh_own_key_title":"Box","ssh_own_key_public":"ssh-ed25519 AAAA",
       "ssh_own_key_private":"PRIVATE","ssh_own_key_notes":""}]}"#;
    let (format, imported) = run(json.as_bytes());
    assert_eq!(format, Format::PsonoJson);
    assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
    let Kind::Card(card) = &titled(&imported, "Visa").entry.kind else { panic!("a card") };
    assert_eq!((card.number.v.expose(), card.expiry.v.as_str()), ("4111", "12/27"));
    let Kind::SshKey(key) = &titled(&imported, "Box").entry.kind else { panic!("a key") };
    assert_eq!(key.private_key.v.expose(), "PRIVATE");
}

#[test]
fn authenticators_keep_groups_notes_favourites_and_tags() {
    let aegis = r#"{"version":1,"header":{"slots":null,"params":null},"db":{"version":3,
      "groups":[{"uuid":"g1","name":"Work"}],
      "entries":[{"type":"totp","uuid":"e1","name":"me","issuer":"GitHub","note":"recovery in safe","favorite":true,
        "groups":["g1"],"info":{"secret":"JBSWY3DPEHPK3PXP","algo":"SHA1","digits":6,"period":30}}]}}"#;
    let (_, imported) = run(aegis.as_bytes());
    let item = &imported.entries[0];
    assert_eq!(item.folder.as_deref(), Some("Work"));
    assert!(item.entry.common.favourite.v);
    assert_eq!(item.entry.common.notes.v.expose(), "recovery in safe");

    let two_fas = r#"{"schemaVersion":4,"groups":[{"id":"g1","name":"Banks"}],"services":[{"name":"Bank","secret":"JBSWY3DPEHPK3PXP",
      "groupId":"g1","otp":{"account":"me","issuer":"Bank","digits":6,"period":30,"algorithm":"SHA1","tokenType":"TOTP"}}]}"#;
    let (_, imported) = run(two_fas.as_bytes());
    assert_eq!(imported.entries[0].folder.as_deref(), Some("Banks"));

    let and_otp = r#"[{"secret":"JBSWY3DPEHPK3PXP","issuer":"Mail","label":"me","digits":6,"type":"TOTP","algorithm":"SHA1","period":30,"tags":["Work"]}]"#;
    let (_, imported) = run(and_otp.as_bytes());
    assert_eq!(imported.entries[0].entry.common.tags.v, vec!["Work".to_owned()]);

    let raivo = r#"[{"pinned":"true","issuer":"Mail","account":"me","secret":"JBSWY3DPEHPK3PXP","timer":"30","digits":"6","kind":"TOTP","counter":"0","algorithm":"SHA1"}]"#;
    let (format, imported) = run(raivo.as_bytes());
    assert_eq!(format, Format::Raivo);
    assert!(imported.entries[0].entry.common.favourite.v);

    let lastpass = r#"{"version":3,"accounts":[{"issuerName":"Mail","userName":"me","secret":"JBSWY3DPEHPK3PXP","timeStep":30,"digits":6,"algorithm":"SHA1","isFavorite":true}]}"#;
    let (_, imported) = run(lastpass.as_bytes());
    assert!(imported.entries[0].entry.common.favourite.v);

    let free_otp = r#"{"tokenOrder":["x"],"tokens":[{"algo":"SHA1","counter":0,"digits":6,"issuerExt":"","issuerInt":"Mail","label":"me","period":30,"secret":[1,2,3,4,5],"type":"TOTP"}]}"#;
    let (_, imported) = run(free_otp.as_bytes());
    assert_eq!(imported.entries[0].entry.common.title.v, "Mail");
}

#[test]
fn one_password_favourites_tags_and_codes_on_every_kind() {
    let export = r#"{"accounts":[{"vaults":[{"attrs":{"name":"Private"},"items":[
      {"categoryUuid":"003","favIndex":1,"overview":{"title":"Server note","tags":["infra"]},
       "details":{"notesPlain":"body","sections":[{"fields":[{"id":"t1","title":"Code","value":{"totp":"JBSWY3DPEHPK3PXP"}}]}]}},
      {"categoryUuid":"001","overview":{"title":"Mail","url":"https://m.example"},
       "details":{"loginFields":[{"designation":"username","value":"me"},{"designation":"password","value":"pw"}],
        "sections":[{"fields":[{"id":"a","title":"Main","value":{"totp":"JBSWY3DPEHPK3PXP"}},
                               {"id":"b","title":"Backup","value":{"totp":"GEZDGNBVGY3TQOJQ"}}]}]}}
    ]}]}]}"#;
    let data = zip_of(&[("export.data", export.as_bytes())]);
    let (format, imported) = run(&data);
    assert_eq!(format, Format::OnePux);
    let note = titled(&imported, "Server note");
    assert!(note.entry.common.favourite.v);
    assert_eq!(note.entry.common.tags.v, vec!["infra".to_owned()]);
    assert_eq!(value(note, "One-time code"), "JBSWY3DPEHPK3PXP", "a code on a note is kept");
    let mail = titled(&imported, "Mail");
    assert_eq!(mail.entry.login().unwrap().otp.v.as_ref().unwrap().secret.expose(), "JBSWY3DPEHPK3PXP");
    assert_eq!(value(mail, "Backup"), "GEZDGNBVGY3TQOJQ", "a second code is kept");
}

fn passkey() -> Passkey {
    crate::passkey::create("site.example", "Site", b"user-1", "me", true, 5000).unwrap().passkey
}

#[test]
fn bitwarden_export_drops_nothing_bitwarden_has_no_place_for() {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut login = login_entry("Site", "me", "pw", at);
    let key = passkey();
    {
        let body = login.login_mut().unwrap();
        body.passkeys = Lww::new(vec![key.clone()], at);
        body.questions = Lww::new(vec![Field { name: "First pet?".into(), value: "Rex".into(), kind: FieldKind::Hidden }], at);
    }
    vault.add(login, 2000).unwrap();
    let mut card = crate::model::Entry::default();
    card.common.title = Lww::new("Visa".into(), at);
    card.kind = Kind::Card(crate::model::Card {
        number: Lww::new("4111".into(), at),
        pin: Lww::new("9876".into(), at),
        billing: Lww::new("1 Main St".into(), at),
        ..Default::default()
    });
    vault.add(card, 2000).unwrap();
    let mut ssh = crate::model::Entry::default();
    ssh.common.title = Lww::new("Box".into(), at);
    ssh.kind = Kind::SshKey(crate::model::SshKey { passphrase: Lww::new("phrase".into(), at), ..Default::default() });
    vault.add(ssh, 2000).unwrap();
    let mut person = crate::model::Entry::default();
    person.common.title = Lww::new("Me".into(), at);
    person.kind = Kind::Identity(crate::model::Identity {
        full_name: Lww::new("Ann B Lee".into(), at),
        email: Lww::new("a@example.com".into(), at),
        document_numbers: Lww::new(vec![Field { name: "Passport".into(), value: "P1".into(), kind: FieldKind::Hidden }], at),
        ..Default::default()
    });
    vault.add(person, 2000).unwrap();
    let mut note = crate::model::Entry::default();
    note.common.title = Lww::new("Note".into(), at);
    note.common.notes = Lww::new("and more".into(), at);
    note.kind = Kind::Note(crate::model::Note { body: Lww::new("body".into(), at) });
    vault.add(note, 2000).unwrap();

    let json = export::to_bitwarden_json(&vault).unwrap();
    let root: serde_json::Value = serde_json::from_str(json.expose()).unwrap();
    let person_json = root["items"].as_array().unwrap().iter().find(|i| i["name"] == "Me").unwrap();
    assert_eq!(person_json["type"], 4, "an identity is Bitwarden's identity");
    assert_eq!(person_json["identity"]["passportNumber"], "P1");

    let (_, imported) = run(json.expose().as_bytes());
    let site = titled(&imported, "Site");
    let back = &site.entry.login().unwrap().passkeys.v;
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].credential_id, key.credential_id);
    assert_eq!(back[0].private_key.expose(), key.private_key.expose());
    assert_eq!(back[0].user_handle, key.user_handle);
    assert_eq!(value(site, "First pet?"), "Rex");
    let visa = titled(&imported, "Visa");
    assert_eq!(value(visa, "PIN"), "9876");
    assert_eq!(value(visa, "Billing address"), "1 Main St");
    assert_eq!(value(titled(&imported, "Box"), "Passphrase"), "phrase");
    let me = titled(&imported, "Me");
    let Kind::Identity(identity) = &me.entry.kind else { panic!("an identity") };
    assert_eq!(identity.full_name.v, "Ann B Lee");
    assert_eq!(identity.email.v, "a@example.com");
    let Kind::Note(body) = &titled(&imported, "Note").entry.kind else { panic!("a note") };
    assert_eq!(body.body.v.expose(), "body\n\nand more");
}

#[test]
fn kdbx_export_keeps_clashing_fields_extra_addresses_and_passkeys() {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut login = login_entry("Site", "me", "pw", at);
    let key = passkey();
    {
        let body = login.login_mut().unwrap();
        body.site_matchers = Lww::new(
            vec![
                crate::model::SiteMatcher::Url("https://a.example".into()),
                crate::model::SiteMatcher::Url("https://b.example".into()),
            ],
            at,
        );
        body.passkeys = Lww::new(vec![key.clone()], at);
    }
    login.common.custom_fields = Lww::new(
        vec![
            Field { name: "Title".into(), value: "not the title".into(), kind: FieldKind::Text },
            Field { name: "Password".into(), value: "old pw".into(), kind: FieldKind::Hidden },
            Field { name: "otp".into(), value: "a note about codes".into(), kind: FieldKind::Text },
        ],
        at,
    );
    vault.add(login, 2000).unwrap();
    let mut wifi = crate::model::Entry::default();
    wifi.common.title = Lww::new("Home".into(), at);
    wifi.kind = Kind::Wifi(crate::model::Wifi {
        ssid: Lww::new("home".into(), at),
        password: Lww::new("wifipw".into(), at),
        ..Default::default()
    });
    vault.add(wifi, 2000).unwrap();
    let entries: Vec<&crate::model::Entry> = vault.doc.entries.values().collect();
    // A Wi-Fi password and a field named like KeePass's own once made the export fail.
    let bytes = kdbx::export(&entries, "pw").unwrap();
    let imported = import(Format::Kdbx, &bytes, Some("pw"), AT).unwrap();
    let site = titled(&imported, "Site");
    let login = site.entry.login().unwrap();
    assert_eq!(login.password.v.expose(), "pw");
    assert_eq!(login.site_matchers.v.len(), 2, "{:?}", login.site_matchers.v);
    assert_eq!(value(site, "Title (2)"), "not the title");
    assert_eq!(value(site, "Password (2)"), "old pw");
    assert_eq!(value(site, "otp (2)"), "a note about codes");
    assert_eq!(field(site, "Title (2)").unwrap().kind, FieldKind::Text, "a shown field stays shown");
    assert_eq!(login.passkeys.v.len(), 1);
    assert_eq!(login.passkeys.v[0].private_key.expose(), key.private_key.expose());
    assert_eq!(login.passkeys.v[0].credential_id, key.credential_id);
    assert_eq!(login.passkeys.v[0].algorithm, -7);
    assert!(site.entry.common.custom_fields.v.iter().all(|f| !f.name.starts_with("KP")), "{:?}", site.entry.common.custom_fields.v);
    let home = titled(&imported, "Home");
    assert_eq!(value(home, "Password (2)"), "wifipw");
}

#[test]
fn keepass_xml_reads_keepassxc_extra_addresses() {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?><KeePassFile><Root><Group><Name>Root</Name>
      <Entry><String><Key>Title</Key><Value>Site</Value></String>
        <String><Key>URL</Key><Value>https://a.example</Value></String>
        <String><Key>KP2A_URL_1</Key><Value>https://b.example</Value></String>
        <String><Key>Password</Key><Value ProtectInMemory="True">pw</Value></String></Entry>
    </Group></Root></KeePassFile>"#;
    let (format, imported) = run(xml.as_bytes());
    assert_eq!(format, Format::KeePassXml);
    let site = titled(&imported, "Site");
    assert_eq!(site.entry.login().unwrap().site_matchers.v.len(), 2);
    assert!(field(site, "KP2A_URL_1").is_none());
}

#[test]
fn a_steam_code_goes_to_bitwarden_as_steam() {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut entry = crate::model::Entry::default();
    entry.common.title = Lww::new("Steam".into(), at);
    let mut config = OtpConfig::totp("JBSWY3DPEHPK3PXP".to_owned());
    config.kind = OtpKind::Steam;
    entry.kind = Kind::Otp(crate::model::Otp { config: Lww::new(Some(config), at) });
    vault.add(entry, 2000).unwrap();
    let json = export::to_bitwarden_json(&vault).unwrap();
    let root: serde_json::Value = serde_json::from_str(json.expose()).unwrap();
    assert_eq!(root["items"][0]["login"]["totp"], "steam://JBSWY3DPEHPK3PXP");
}

#[test]
fn dashlane_zip_brings_every_csv_in_it() {
    let credentials = "username,username2,username3,title,password,note,url,category,otpSecret\nme,,,Mail,pw,,https://m.example,Work,\n";
    let notes = "title,note\nDoor,1234\n";
    let payments = "type,account_name,account_holder,cc_number,code,expiration_month,expiration_year,routing_number,account_number,country,issuing_bank\n\
        payment_card,Visa,Ann Lee,4111,123,12,2027,,,US,Big Bank\n\
        bank,Checking,Ann Lee,,,,,021000021,12345,US,Big Bank\n";
    let ids = "type,number,name,issue_date,expiration_date,place_of_issue,state\npassport,P123,Ann Lee,2020-01-01,2030-01-01,US,\n";
    let data = zip_of(&[
        ("credentials.csv", credentials.as_bytes()),
        ("securenotes.csv", notes.as_bytes()),
        ("payments.csv", payments.as_bytes()),
        ("ids.csv", ids.as_bytes()),
    ]);
    let (format, imported) = run(&data);
    assert_eq!(format, Format::DashlaneCsv);
    assert_eq!(titled(&imported, "Mail").folder.as_deref(), Some("Work"));
    let Kind::Note(door) = &titled(&imported, "Door").entry.kind else { panic!("a note") };
    assert_eq!(door.body.v.expose(), "1234");
    let Kind::Card(card) = &titled(&imported, "Visa").entry.kind else { panic!("a card") };
    assert_eq!((card.number.v.expose(), card.expiry.v.as_str(), card.cvv.v.expose()), ("4111", "12/2027", "123"));
    let bank = titled(&imported, "Checking");
    assert_eq!(value(bank, "account_number"), "12345");
    assert_eq!(field(bank, "account_number").unwrap().kind, FieldKind::Hidden);
    let passport = titled(&imported, "passport: Ann Lee");
    let Kind::Document(document) = &passport.entry.kind else { panic!("a document") };
    assert_eq!(document.number.v.expose(), "P123");
    assert_eq!(value(passport, "expiration_date"), "2030-01-01");
    // Each CSV also reads on its own.
    let (format, alone) = run(payments.as_bytes());
    assert_eq!(format, Format::DashlaneCsv);
    assert_eq!(alone.entries.len(), 2);
}

#[test]
fn creation_times_come_across() {
    let firefox = "\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\",\"timeCreated\",\"timeLastUsed\",\"timePasswordChanged\"\n\
        \"https://m.example\",\"me\",\"pw\",,\"https://m.example\",\"{1}\",\"1700000000123\",\"1700000000123\",\"1700000000123\"\n\
        \"chrome://FirefoxAccounts\",\"uid\",\"{}\",\"Firefox Accounts credentials\",,\"{2}\",\"1\",\"1\",\"1\"\n";
    let (format, imported) = run(firefox.as_bytes());
    assert_eq!(format, Format::FirefoxCsv);
    assert_eq!(imported.entries.len(), 1, "Firefox's own account entry is no login");
    assert_eq!(imported.entries[0].entry.created_ms, 1_700_000_000_123);
    assert_eq!(imported.skipped.len(), 1);

    let bitwarden = r#"{"encrypted":false,"folders":[],"items":[{"type":2,"name":"N","notes":"x","secureNote":{"type":0},
        "creationDate":"2024-04-08T13:25:14.123Z"}]}"#;
    let (_, imported) = run(bitwarden.as_bytes());
    assert_eq!(imported.entries[0].entry.created_ms, 1_712_582_714_123);

    let proton = r#"{"encrypted":false,"vaults":{"s":{"name":"V","items":[{"state":1,"createTime":1700000000,
        "data":{"type":"note","metadata":{"name":"N","note":"x"},"extraFields":[],"content":{}}}]}}}"#;
    let (_, imported) = run(proton.as_bytes());
    assert_eq!(imported.entries[0].entry.created_ms, 1_700_000_000_000);

    let enpass = r#"{"folders":[],"items":[{"title":"N","category":"note","note":"x","createdAt":1700000000,"fields":[]}]}"#;
    let (_, imported) = run(enpass.as_bytes());
    assert_eq!(imported.entries[0].entry.created_ms, 1_700_000_000_000);

    let onepux = r#"{"accounts":[{"vaults":[{"attrs":{"name":"V"},"items":[{"categoryUuid":"003","createdAt":1700000000,
        "overview":{"title":"N"},"details":{"notesPlain":"x"}}]}]}]}"#;
    let (_, imported) = run(&zip_of(&[("export.data", onepux.as_bytes())]));
    assert_eq!(imported.entries[0].entry.created_ms, 1_700_000_000_000);

    // Kept through to the vault.
    let mut vault = fast_vault(1);
    apply(&mut vault, imported, 9_000_000_000_000).unwrap();
    assert_eq!(vault.doc.entries.values().next().unwrap().created_ms, 1_700_000_000_000);
}

#[test]
fn google_migration_names_holding_the_issuer_are_split() {
    let mut item = Vec::new();
    item.extend_from_slice(&[0x0a, 3, 72, 101, 108]); // secret
    item.extend_from_slice(&[0x12, 9]);
    item.extend_from_slice(b"Mail:me@x"); // name, as the original link's label
    item.extend_from_slice(&[0x20, 1, 0x28, 1, 0x30, 2]);
    let mut payload = vec![0x0a, item.len() as u8];
    payload.extend_from_slice(&item);
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &payload);
    let link = format!("otpauth-migration://offline?data={}", crate::otp::percent_encode(&encoded));
    let found = authenticators::google_migration(&link).unwrap();
    assert_eq!((found[0].issuer.as_str(), found[0].account.as_str()), ("Mail", "me@x"));
}

#[test]
fn keepass_creation_times_come_across() {
    let csv = "\"Group\",\"Title\",\"Username\",\"Password\",\"URL\",\"Notes\",\"TOTP\",\"Icon\",\"Last Modified\",\"Created\"\n\
        \"Root\",\"Mail\",\"me\",\"pw\",\"https://m.example\",\"\",\"\",\"0\",\"2023-11-14T22:13:20Z\",\"2023-11-14T22:13:20Z\"\n";
    let (format, imported) = run(csv.as_bytes());
    assert_eq!(format, Format::KeePassXcCsv);
    assert_eq!(imported.entries[0].entry.created_ms, 1_700_000_000_000);
    let xml = r#"<KeePassFile><Root><Group><Name>Root</Name><Entry>
        <String><Key>Title</Key><Value>Mail</Value></String>
        <Times><CreationTime>2023-11-14T22:13:20Z</CreationTime></Times></Entry></Group></Root></KeePassFile>"#;
    let (_, imported) = run(xml.as_bytes());
    assert_eq!(imported.entries[0].entry.created_ms, 1_700_000_000_000);
}

#[test]
fn kdbx_export_writes_old_passwords_to_the_history() {
    let mut vault = fast_vault(1);
    let at = vault.tick(2000);
    let mut login = login_entry("Mail", "me", "pw-3", at);
    for (value, ms) in [("pw-1", 1_600_000_000_000u64), ("pw-2", 1_650_000_000_000)] {
        login.history.push(crate::model::Revision {
            field: "password".into(),
            value: value.into(),
            at: Hlc { ms, ..Hlc::default() },
        });
    }
    vault.add(login, 2000).unwrap();
    let entries: Vec<&crate::model::Entry> = vault.doc.entries.values().collect();
    let bytes = kdbx::export(&entries, "pw").unwrap();
    let database = keepass::Database::parse(&bytes, keepass::DatabaseKey::new().with_password("pw")).unwrap();
    let saved = database.iter_all_entries().next().unwrap();
    let versions = saved.history.as_ref().unwrap().get_entries();
    assert_eq!(versions.len(), 2, "one past version per old password");
    assert_eq!(versions[0].get_password(), Some("pw-1"), "oldest first, as KeePass keeps them");
    let imported = import(Format::Kdbx, &bytes, Some("pw"), AT).unwrap();
    let back = &imported.entries[0].entry;
    let old: Vec<(&str, u64)> = back.history.iter().map(|r| (r.value.expose(), r.at.ms)).collect();
    assert_eq!(old, vec![("pw-1", 1_600_000_000_000), ("pw-2", 1_650_000_000_000)]);
}

#[test]
fn keepass_xml_reads_old_passwords_from_the_history() {
    let xml = r#"<KeePassFile><Root><Group><Name>Root</Name><Entry>
        <String><Key>Title</Key><Value>Mail</Value></String>
        <String><Key>Password</Key><Value ProtectInMemory="True">pw-3</Value></String>
        <History>
          <Entry><String><Key>Password</Key><Value ProtectInMemory="True">pw-1</Value></String>
            <Times><LastModificationTime>2020-09-13T12:26:40Z</LastModificationTime></Times></Entry>
          <Entry><String><Key>Password</Key><Value ProtectInMemory="True">pw-1</Value></String>
            <Times><LastModificationTime>2021-01-01T00:00:00Z</LastModificationTime></Times></Entry>
          <Entry><String><Key>Password</Key><Value ProtectInMemory="True">pw-2</Value></String>
            <Times><LastModificationTime>2022-04-15T23:06:40Z</LastModificationTime></Times></Entry>
          <Entry><String><Key>Password</Key><Value ProtectInMemory="True">pw-3</Value></String>
            <Times><LastModificationTime>2023-01-01T00:00:00Z</LastModificationTime></Times></Entry>
        </History></Entry></Group></Root></KeePassFile>"#;
    let (_, imported) = run(xml.as_bytes());
    let old: Vec<(&str, u64)> = imported.entries[0].entry.history.iter().map(|r| (r.value.expose(), r.at.ms)).collect();
    assert_eq!(old, vec![("pw-1", 1_600_000_000_000), ("pw-2", 1_650_064_000_000)]);
}

#[test]
fn authenticator_codes_that_cannot_make_codes_are_left_out_and_said() {
    let aegis = r#"{"version":1,"header":{"slots":null,"params":null},"db":{"version":3,"entries":[
      {"type":"totp","name":"good","issuer":"A","info":{"secret":"JBSWY3DPEHPK3PXP","algo":"SHA1","digits":6,"period":30}},
      {"type":"totp","name":"me","issuer":"Broken","info":{"secret":"not base32!","algo":"SHA1","digits":6,"period":30}},
      {"type":"totp","name":"me","issuer":"Empty","info":{"secret":"","algo":"SHA1","digits":6,"period":30}},
      {"type":"totp","name":"me","issuer":"Zero","info":{"secret":"JBSWY3DPEHPK3PXP","algo":"SHA1","digits":6,"period":0}}]}}"#;
    let (_, imported) = run(aegis.as_bytes());
    assert_eq!(imported.entries.len(), 1);
    assert_eq!(imported.skipped.len(), 3, "{:?}", imported.skipped);
    assert!(imported.skipped[0].starts_with("Broken: left out, as its setup key is not base32"), "{:?}", imported.skipped);
    assert!(imported.skipped[1].contains("no setup key"));
    assert!(imported.skipped[2].contains("period is zero"));

    let and_otp = r#"[{"secret":"JBSWY3DPEHPK3PXP","issuer":"Mail","label":"me","digits":0,"type":"TOTP","algorithm":"SHA1","period":30}]"#;
    let (_, imported) = run(and_otp.as_bytes());
    assert!(imported.entries.is_empty());
    assert!(imported.skipped[0].contains("digits"), "{:?}", imported.skipped);
}

#[test]
fn one_password_routers_and_databases_keep_their_kind() {
    let export = r#"{"accounts":[{"vaults":[{"attrs":{"name":"Home"},"items":[
      {"categoryUuid":"109","overview":{"title":"Router"},"details":{"notesPlain":"","sections":[{"fields":[
        {"id":"name","title":"base station name","value":{"string":"Box"}},
        {"id":"password","title":"base station password","value":{"concealed":"admin-pw"}},
        {"id":"network_name","title":"network name","value":{"string":"home"}},
        {"id":"wireless_security","title":"wireless security","value":{"menu":"wpa3p"}},
        {"id":"wireless_password","title":"wireless network password","value":{"concealed":"wifi-pw"}}]}]}},
      {"categoryUuid":"102","overview":{"title":"DB"},"details":{"notesPlain":"","sections":[{"fields":[
        {"id":"database_type","title":"type","value":{"menu":"postgresql"}},
        {"id":"hostname","title":"server","value":{"string":"db.example"}},
        {"id":"port","title":"port","value":{"string":"5432"}},
        {"id":"database","title":"database","value":{"string":"app"}},
        {"id":"username","title":"username","value":{"string":"app_user"}},
        {"id":"password","title":"password","value":{"concealed":"db-pw"}},
        {"id":"sid","title":"SID","value":{"string":"ORCL"}}]}]}}
    ]}]}]}"#;
    let (_, imported) = run(&zip_of(&[("export.data", export.as_bytes())]));
    assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
    let router = titled(&imported, "Router");
    let Kind::Wifi(wifi) = &router.entry.kind else { panic!("Wi-Fi") };
    assert_eq!((wifi.ssid.v.as_str(), wifi.password.v.expose()), ("home", "wifi-pw"));
    assert_eq!(wifi.security.v, crate::model::WifiSecurity::Sae);
    assert_eq!(value(router, "base station password"), "admin-pw");
    assert!(field(router, "network name").is_none());
    let db = titled(&imported, "DB");
    let Kind::Database(database) = &db.entry.kind else { panic!("a database") };
    assert_eq!(database.engine.v, "postgresql");
    assert_eq!((database.host.v.as_str(), database.port.v), ("db.example", Some(5432)));
    assert_eq!((database.name.v.as_str(), database.user.v.as_str()), ("app", "app_user"));
    assert_eq!(database.password.v.expose(), "db-pw");
    assert_eq!(value(db, "SID"), "ORCL");
    assert_eq!(db.entry.common.custom_fields.v.len(), 1);
}

#[test]
fn proton_wifi_security_is_read_only_where_certain() {
    let item = |security: &str| {
        format!(
            r#"{{"encrypted":false,"vaults":{{"s":{{"name":"V","items":[{{"state":1,"data":{{"type":"wifi",
            "metadata":{{"name":"Net","note":""}},"extraFields":[],"content":{{"ssid":"home","password":"pw","security":{security},"sections":[]}}}}}}]}}}}}}"#
        )
    };
    let read = |security: &str| {
        let (_, imported) = run(item(security).as_bytes());
        let net = imported.entries[0].clone();
        let Kind::Wifi(wifi) = &net.entry.kind else { panic!("Wi-Fi") };
        (wifi.security.v, value(&net, "Security").to_owned())
    };
    use crate::model::WifiSecurity;
    assert_eq!(read("\"WPA3\""), (WifiSecurity::Sae, String::new()));
    assert_eq!(read("\"WEP\""), (WifiSecurity::Wep, String::new()));
    assert_eq!(read("2"), (WifiSecurity::default(), "2".to_owned()), "a number is kept as written");
    assert_eq!(read("0"), (WifiSecurity::default(), String::new()), "unspecified is nothing");
}
