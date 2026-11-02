//! Against the real services. Ignored by default, because a test run must not reach out.
//!
//! ```bash
//! cargo test -p wmpass-net --features native --test live -- --ignored
//! ```
#![cfg(feature = "native")]
#![allow(missing_docs, clippy::unwrap_used)]

use wmpass_net::http::NativeHttp;
use wmpass_net::usernames::{Availability, SITES, check};
use wmpass_net::{NetPolicy, breach, inbox};

const ANY: NetPolicy = NetPolicy::Any;

#[test]
#[ignore = "reaches api.pwnedpasswords.com"]
fn pwned_passwords_knows_the_word_password() {
    let count = breach::password_count(&"password".into())
        .run(&NativeHttp::default(), ANY)
        .unwrap();
    assert!(count > 1_000_000, "{count}");
}

#[test]
#[ignore = "reaches each listed site"]
fn username_signals_still_hold() {
    let http = NativeHttp::default();
    // A name that exists on most of these, and one that should exist on none.
    for site in SITES {
        let taken = check(*site, "test").unwrap().run(&http, ANY);
        let free = check(*site, "wmpass-no-such-user-9f3k2")
            .unwrap()
            .run(&http, ANY);
        println!("{:<12} test={taken:?} random={free:?}", site.name);
        assert_ne!(
            free,
            Ok(Availability::Taken),
            "{} calls a random name taken",
            site.name
        );
    }
}

#[test]
#[ignore = "reaches api.mail.tm"]
fn mail_tm_lists_domains() {
    let domains = inbox::mailtm_domains()
        .run(&NativeHttp::default(), ANY)
        .unwrap();
    assert!(!domains.is_empty());
}

#[test]
#[ignore = "reaches leakcheck.io with a throwaway address"]
fn leakcheck_answers_in_the_documented_shape() {
    let found = breach::leakcheck_public("test@example.com").run(&NativeHttp::default(), ANY);
    println!("{found:?}");
    assert!(found.is_ok());
}
