//! Disposable mailboxes, for signups that only need a verification mail.
//! Anyone could read these services' mail in principle, so screens must not offer them
//! for accounts the owner cares about keeping.

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_core::otp::percent_encode;

use crate::http::{Call, NetError, Request};

/// A mail in a temporary inbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    /// The service's id.
    pub id: String,
    /// Sender.
    pub from: String,
    /// Subject.
    pub subject: String,
    /// Opening lines.
    pub preview: String,
}

const MAIL_TM: &str = "https://api.mail.tm";

/// mail.tm: the domains it currently offers.
pub fn mailtm_domains() -> Call<Vec<String>> {
    Call::new(Request::get(format!("{MAIL_TM}/domains")), |response| {
        let root = response.ok()?.json()?;
        let list = root
            .get("hydra:member")
            .or(Some(&root))
            .and_then(Value::as_array)
            .ok_or(NetError::BadAnswer)?;
        Ok(list
            .iter()
            .filter_map(|d| d.get("domain").and_then(Value::as_str).map(str::to_owned))
            .collect())
    })
}

/// mail.tm: creates a mailbox.
pub fn mailtm_create(address: &str, password: &Secret) -> Call<String> {
    let body = json!({"address": address, "password": password.expose()});
    Call::new(
        Request::json("POST", format!("{MAIL_TM}/accounts"), &body),
        |response| {
            let root = response.ok()?.json()?;
            root.get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(NetError::BadAnswer)
        },
    )
}

/// mail.tm: signs in and returns a bearer token.
pub fn mailtm_token(address: &str, password: &Secret) -> Call<Secret> {
    let body = json!({"address": address, "password": password.expose()});
    Call::new(
        Request::json("POST", format!("{MAIL_TM}/token"), &body),
        |response| {
            let root = response.ok()?.json()?;
            root.get("token")
                .and_then(Value::as_str)
                .map(Secret::new)
                .ok_or(NetError::BadAnswer)
        },
    )
}

/// mail.tm: the mail received so far.
pub fn mailtm_messages(token: &Secret) -> Call<Vec<Message>> {
    let request = Request::get(format!("{MAIL_TM}/messages"))
        .header("Authorization", format!("Bearer {}", token.expose()));
    Call::new(request, |response| {
        let root = response.ok()?.json()?;
        let list = root
            .get("hydra:member")
            .or(Some(&root))
            .and_then(Value::as_array)
            .ok_or(NetError::BadAnswer)?;
        Ok(list
            .iter()
            .map(|m| Message {
                id: field(m, "id"),
                from: m
                    .pointer("/from/address")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                subject: field(m, "subject"),
                preview: field(m, "intro"),
            })
            .collect())
    })
}

/// mail.tm: one mail's text. Plain text only: HTML mail is never rendered here, because
/// tracking pixels and scripts have no place near a password manager.
pub fn mailtm_body(token: &Secret, id: &str) -> Call<String> {
    let url = format!("{MAIL_TM}/messages/{}", percent_encode(id));
    let request = Request::get(url).header("Authorization", format!("Bearer {}", token.expose()));
    Call::new(request, |response| {
        Ok(field(&response.ok()?.json()?, "text"))
    })
}

/// mail.tm: deletes the mailbox.
pub fn mailtm_delete(token: &Secret, account_id: &str) -> Call<()> {
    let url = format!("{MAIL_TM}/accounts/{}", percent_encode(account_id));
    let request =
        Request::verb("DELETE", url).header("Authorization", format!("Bearer {}", token.expose()));
    Call::new(request, |response| response.ok().map(|_| ()))
}

fn field(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// A Guerrilla Mail session. Its token stands in for the session cookie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuerrillaSession {
    /// The address.
    pub address: String,
    /// Session token.
    pub token: Secret,
}

const GUERRILLA: &str = "https://api.guerrillamail.com/ajax.php";
// The API requires these two. A fixed loopback value keeps the real address out of the query.
const GUERRILLA_ID: &str = "ip=127.0.0.1&agent=WM-Pass";

/// Guerrilla Mail: a new address.
pub fn guerrilla_new() -> Call<GuerrillaSession> {
    Call::new(
        Request::get(format!("{GUERRILLA}?f=get_email_address&{GUERRILLA_ID}")),
        |response| {
            let root = response.ok()?.json()?;
            Ok(GuerrillaSession {
                address: root
                    .get("email_addr")
                    .and_then(Value::as_str)
                    .ok_or(NetError::BadAnswer)?
                    .to_owned(),
                token: Secret::new(
                    root.get("sid_token")
                        .and_then(Value::as_str)
                        .ok_or(NetError::BadAnswer)?,
                ),
            })
        },
    )
}

/// Guerrilla Mail: the list of mail.
pub fn guerrilla_messages(session: &GuerrillaSession) -> Call<Vec<Message>> {
    let url = format!(
        "{GUERRILLA}?f=check_email&seq=0&sid_token={}&{GUERRILLA_ID}",
        percent_encode(session.token.expose())
    );
    Call::new(Request::get(url), |response| {
        let root = response.ok()?.json()?;
        let list = root
            .get("list")
            .and_then(Value::as_array)
            .ok_or(NetError::BadAnswer)?;
        Ok(list
            .iter()
            .map(|m| Message {
                id: field(m, "mail_id"),
                from: field(m, "mail_from"),
                subject: field(m, "mail_subject"),
                preview: field(m, "mail_excerpt"),
            })
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::NetPolicy;
    use crate::http::fake::FakeHttp;

    const ANY: NetPolicy = NetPolicy::Any;

    #[test]
    fn mailtm_flow() {
        let http = FakeHttp::new()
            .answer("https://api.mail.tm/domains", 200, r#"{"hydra:member":[{"domain":"x.tm"}]}"#)
            .answer("https://api.mail.tm/accounts", 201, r#"{"id":"acc"}"#)
            .answer("https://api.mail.tm/token", 200, r#"{"token":"tok"}"#)
            .answer("https://api.mail.tm/messages/m1", 200, r#"{"text":"code 123","html":["<img src=x>"]}"#)
            .answer("https://api.mail.tm/messages", 200, r#"{"hydra:member":[{"id":"m1","from":{"address":"a@b"},"subject":"Hi","intro":"code"}]}"#);
        assert_eq!(mailtm_domains().run(&http, ANY).unwrap(), vec!["x.tm"]);
        let password = Secret::new("p");
        assert_eq!(
            mailtm_create("me@x.tm", &password).run(&http, ANY).unwrap(),
            "acc"
        );
        let token = mailtm_token("me@x.tm", &password).run(&http, ANY).unwrap();
        let mail = mailtm_messages(&token).run(&http, ANY).unwrap();
        assert_eq!(
            (mail[0].from.as_str(), mail[0].subject.as_str()),
            ("a@b", "Hi")
        );
        assert_eq!(
            mailtm_body(&token, "m1").run(&http, ANY).unwrap(),
            "code 123"
        );
    }

    #[test]
    fn guerrilla_flow_sends_no_real_address() {
        let http = FakeHttp::new()
            .answer(&format!("{GUERRILLA}?f=get_email_address"), 200, r#"{"email_addr":"a@sharklasers.com","sid_token":"t"}"#)
            .answer(&format!("{GUERRILLA}?f=check_email"), 200, r#"{"list":[{"mail_id":123,"mail_from":"x@y","mail_subject":"Hi","mail_excerpt":"code"}]}"#);
        let session = guerrilla_new().run(&http, ANY).unwrap();
        assert_eq!(session.address, "a@sharklasers.com");
        assert!(http.last().unwrap().url.contains("ip=127.0.0.1"));
        let mail = guerrilla_messages(&session).run(&http, ANY).unwrap();
        assert_eq!(mail[0].id, "123");
    }
}
