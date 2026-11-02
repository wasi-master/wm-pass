//! Properties that must hold for any input. Merge has to converge whatever the order
//! devices sync in, and no bytes from the outside may cause a panic.
#![allow(missing_docs)]

use proptest::prelude::*;
use wmpass_core::clock::Hlc;
use wmpass_core::model::{Entry, Kind, Login, Lww, VaultDoc};
use wmpass_core::{Secret, format, io, otp, search};

fn hlc() -> impl Strategy<Value = Hlc> {
    (0u64..50, 0u32..3, 0u32..3).prop_map(|(ms, counter, device)| Hlc {
        ms,
        counter,
        device,
    })
}

fn value() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_owned()),
        Just("b".to_owned()),
        Just("c".to_owned()),
        Just(String::new())
    ]
}

prop_compose! {
    fn entry()(id in 0u8..4, title in value(), title_at in hlc(), user in value(), user_at in hlc(),
               password in value(), password_at in hlc()) -> Entry {
        let mut entry = Entry { id: format!("e{id}"), ..Entry::default() };
        // A stamp identifies one write, so it must imply the value: two devices
        // cannot write different values at the very same moment.
        let unique = |text: &str, at: Hlc| (text.to_owned(), Hlc { counter: at.counter * 8 + text.len() as u32 + text.bytes().next().map_or(0, u32::from), ..at });
        let (title, at) = unique(&title, title_at);
        entry.common.title = Lww::new(title, at);
        let (user, user_stamp) = unique(&user, user_at);
        let (password, password_stamp) = unique(&password, password_at);
        entry.kind = Kind::Login(Login {
            username: Lww::new(user, user_stamp),
            password: Lww::new(Secret::from(password), password_stamp),
            ..Login::default()
        });
        entry
    }
}

prop_compose! {
    fn doc()(entries in prop::collection::vec(entry(), 0..5),
             tombstones in prop::collection::vec((0u8..4, hlc()), 0..2)) -> VaultDoc {
        let mut doc = VaultDoc::default();
        for entry in entries {
            match doc.entries.get_mut(&entry.id) {
                Some(mine) => mine.merge_from(&entry),
                None => { doc.entries.insert(entry.id.clone(), entry); }
            }
        }
        for (id, at) in tombstones {
            doc.tombstones.insert(format!("e{id}"), at);
        }
        doc
    }
}

fn merged(a: &VaultDoc, b: &VaultDoc) -> VaultDoc {
    let mut out = a.clone();
    out.merge_from(b);
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn merge_is_commutative(a in doc(), b in doc()) {
        let (left, right) = (merged(&a, &b), merged(&b, &a));
        prop_assert_eq!(left.entries, right.entries);
        prop_assert_eq!(merged(&a, &b).tombstones, merged(&b, &a).tombstones);
    }

    #[test]
    fn merge_is_idempotent(a in doc(), b in doc()) {
        let once = merged(&a, &b);
        let twice = merged(&once, &b);
        prop_assert_eq!(once.entries, twice.entries);
    }

    #[test]
    fn merge_is_associative(a in doc(), b in doc(), c in doc()) {
        let left = merged(&merged(&a, &b), &c);
        let right = merged(&a, &merged(&b, &c));
        prop_assert_eq!(left.entries, right.entries);
    }

    #[test]
    fn no_password_is_ever_lost_unless_deleted(a in doc(), b in doc()) {
        let out = merged(&a, &b);
        for source in [&a, &b] {
            for (id, entry) in &source.entries {
                let Some(result) = out.entries.get(id) else { continue };
                let Some(login) = entry.login() else { continue };
                let password = login.password.v.clone();
                if password.is_empty() { continue; }
                // A delete after this write legitimately removes it.
                let deleted = out.tombstones.get(id).is_some_and(|at| login.password.at <= *at);
                if deleted { continue; }
                let kept = result.login().is_some_and(|l| l.password.v == password)
                    || result.history.iter().any(|r| r.value == password);
                prop_assert!(kept, "a password disappeared from {}", id);
            }
        }
    }

    #[test]
    fn hostile_bytes_never_panic(data in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = format::Header::parse(&data);
        let _ = io::detect(&data);
        for format in [io::Format::BitwardenJson, io::Format::ChromeCsv, io::Format::SafeInCloudXml,
                       io::Format::OnePux, io::Format::Aegis, io::Format::ProtonPass, io::Format::FreeOtpPlus] {
            let _ = io::import(format, &data, None, Hlc::default());
        }
    }

    #[test]
    fn hostile_text_never_panics(text in ".{0,200}") {
        let _ = otp::OtpConfig::from_uri(&text);
        let _ = otp::OtpConfig::from_uri(&format!("otpauth://totp/{text}"));
        let _ = otp::base32_decode(&text);
        let _ = otp::percent_decode(&text);
        let _ = search::Query::parse(&text);
        let _ = wmpass_core::matching::host_of(&text);
        let _ = wmpass_core::recovery::check_phrase(&text);
        let _ = wmpass_core::generator::parse_site_rules(&text);
        let _ = io::detect(text.as_bytes());
    }

    #[test]
    fn hostile_headers_never_panic(mut bytes in prop::collection::vec(any::<u8>(), format::HEADER_LEN..format::HEADER_LEN + 200)) {
        bytes[..4].copy_from_slice(format::MAGIC);
        bytes[4..6].copy_from_slice(&format::VERSION.to_le_bytes());
        // Valid-looking cost settings, so parsing reaches the records.
        bytes[8..12].copy_from_slice(&(64u32 * 1024).to_le_bytes());
        bytes[12..16].copy_from_slice(&3u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        let header = format::Header::parse(&bytes).expect("shape is valid");
        let key = format::VaultKey::from_bytes([7; 32]);
        prop_assert!(header.verify(&key).is_err());
        prop_assert!(format::open_file(&bytes, &key).is_err());
    }

    #[test]
    fn padding_round_trips_through_real_files(plain in prop::collection::vec(any::<u8>(), 0..200_000)) {
        let config = format::KdfConfig::new(wmpass_core::crypto::KdfParams { m_kib: 64 * 1024, t: 3, p: 1 }).unwrap();
        let header = format::Header::new(&config).unwrap();
        let key = format::VaultKey::generate().unwrap();
        let file = format::seal_file(&header, &key, &plain).unwrap();
        let (_, opened) = format::open_file(&file, &key).unwrap();
        prop_assert_eq!(opened.as_slice(), plain.as_slice());
    }
}
