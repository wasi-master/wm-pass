//! Disposable mailboxes, for signups that only need a verification mail.
//! Anyone could read these services' mail in principle, so screens must not offer them
//! for accounts the owner cares about keeping.
//!
//! Three kinds of service: the Mail.tm API (mail.tm, mail.gw, and any server that speaks it),
//! Guerrilla Mail, and Inbucket, which people run themselves and which takes mail for any name.

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
    /// When it arrived, in Unix milliseconds. Zero when the service did not say.
    pub at_ms: u64,
}

/// One mail in full. HTML is handed over as the sender wrote it; whoever shows it decides what
/// it may load.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Body {
    /// Sender.
    pub from: String,
    /// Recipients, as the service lists them.
    pub to: String,
    /// Subject.
    pub subject: String,
    /// The plain text part, or the HTML's text when there is none.
    pub text: String,
    /// The HTML part, if any.
    pub html: Option<String>,
    /// When it arrived, in Unix milliseconds.
    pub at_ms: u64,
}

/// mail.tm's API.
pub const MAIL_TM: &str = "https://api.mail.tm";
/// mail.gw, which runs the same API.
pub const MAIL_GW: &str = "https://api.mail.gw";

/// A Mail.tm-style service: the domains it currently offers.
pub fn mailtm_domains_at(base: &str) -> Call<Vec<String>> {
    Call::new(Request::get(format!("{base}/domains")), |response| {
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

/// mail.tm: the domains it currently offers.
pub fn mailtm_domains() -> Call<Vec<String>> {
    mailtm_domains_at(MAIL_TM)
}

/// mail.tm: creates a mailbox.
pub fn mailtm_create(address: &str, password: &Secret) -> Call<String> {
    mailtm_create_at(MAIL_TM, address, password)
}

/// A Mail.tm-style service: creates a mailbox and returns its id.
pub fn mailtm_create_at(base: &str, address: &str, password: &Secret) -> Call<String> {
    let body = json!({"address": address, "password": password.expose()});
    Call::new(
        Request::json("POST", format!("{base}/accounts"), &body),
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
    mailtm_token_at(MAIL_TM, address, password)
}

/// A Mail.tm-style service: signs in and returns a bearer token. Tokens run out, so a kept
/// mailbox signs in again with its own password.
pub fn mailtm_token_at(base: &str, address: &str, password: &Secret) -> Call<Secret> {
    let body = json!({"address": address, "password": password.expose()});
    Call::new(
        Request::json("POST", format!("{base}/token"), &body),
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
    mailtm_messages_at(MAIL_TM, token)
}

/// A Mail.tm-style service: the mail received so far, newest first.
pub fn mailtm_messages_at(base: &str, token: &Secret) -> Call<Vec<Message>> {
    let request = Request::get(format!("{base}/messages"))
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
                at_ms: rfc3339_ms(&field(m, "createdAt")),
            })
            .collect())
    })
}

/// A Mail.tm-style service: one mail in full.
pub fn mailtm_read_at(base: &str, token: &Secret, id: &str) -> Call<Body> {
    let url = format!("{base}/messages/{}", percent_encode(id));
    let request = Request::get(url).header("Authorization", format!("Bearer {}", token.expose()));
    Call::new(request, |response| {
        let root = response.ok()?.json()?;
        // The HTML comes as a list of parts.
        let html = match root.get("html") {
            Some(Value::Array(parts)) => {
                let joined: String = parts.iter().filter_map(Value::as_str).collect();
                (!joined.is_empty()).then_some(joined)
            }
            Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        };
        let to = root
            .get("to")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|r| r.get("address").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let text = field(&root, "text");
        Ok(Body {
            from: root
                .pointer("/from/address")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            to,
            subject: field(&root, "subject"),
            text: if text.is_empty() {
                html.as_deref().map(html_text).unwrap_or_default()
            } else {
                text
            },
            html,
            at_ms: rfc3339_ms(&field(&root, "createdAt")),
        })
    })
}

/// A Mail.tm-style service: deletes the mailbox.
pub fn mailtm_delete_at(base: &str, token: &Secret, account_id: &str) -> Call<()> {
    let url = format!("{base}/accounts/{}", percent_encode(account_id));
    let request =
        Request::verb("DELETE", url).header("Authorization", format!("Bearer {}", token.expose()));
    Call::new(request, |response| response.ok().map(|_| ()))
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

/// Guerrilla Mail: points a fresh session at an address it handed out before. Its sessions
/// lapse after a while of quiet, but the mailbox stays.
pub fn guerrilla_resume(token: &Secret, address: &str) -> Call<GuerrillaSession> {
    let user = address.split('@').next().unwrap_or_default();
    let url = format!(
        "{GUERRILLA}?f=set_email_user&email_user={}&lang=en&sid_token={}&{GUERRILLA_ID}",
        percent_encode(user),
        percent_encode(token.expose())
    );
    let fallback = token.clone();
    Call::new(Request::get(url), move |response| {
        let root = response.ok()?.json()?;
        Ok(GuerrillaSession {
            address: root
                .get("email_addr")
                .and_then(Value::as_str)
                .ok_or(NetError::BadAnswer)?
                .to_owned(),
            token: root
                .get("sid_token")
                .and_then(Value::as_str)
                .map(Secret::new)
                .unwrap_or(fallback),
        })
    })
}

/// Guerrilla Mail: one mail in full. It sends the body as HTML only.
pub fn guerrilla_read(session: &GuerrillaSession, id: &str) -> Call<Body> {
    let url = format!(
        "{GUERRILLA}?f=fetch_email&email_id={}&sid_token={}&{GUERRILLA_ID}",
        percent_encode(id),
        percent_encode(session.token.expose())
    );
    let to = session.address.clone();
    Call::new(Request::get(url), move |response| {
        let root = response.ok()?.json()?;
        if root.is_null() || root.as_bool() == Some(false) {
            return Err(NetError::BadAnswer);
        }
        let html = field(&root, "mail_body");
        Ok(Body {
            from: field(&root, "mail_from"),
            to,
            subject: field(&root, "mail_subject"),
            text: html_text(&html),
            html: (!html.is_empty()).then_some(html),
            at_ms: field(&root, "mail_timestamp").parse::<u64>().unwrap_or(0) * 1000,
        })
    })
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
                at_ms: field(m, "mail_timestamp").parse::<u64>().unwrap_or(0) * 1000,
            })
            .collect())
    })
}

/// Inbucket: the mail for one name. It takes mail for any name at its domain, so the mailbox
/// is the address's local part.
pub fn inbucket_messages(base: &str, mailbox: &str) -> Call<Vec<Message>> {
    let url = format!("{base}/api/v1/mailbox/{}", percent_encode(mailbox));
    Call::new(Request::get(url), |response| {
        let root = response.ok()?.json()?;
        let list = root.as_array().ok_or(NetError::BadAnswer)?;
        let mut mail: Vec<Message> = list
            .iter()
            .map(|m| Message {
                id: field(m, "id"),
                from: field(m, "from"),
                subject: field(m, "subject"),
                preview: String::new(),
                at_ms: m.get("posix-millis").and_then(Value::as_u64).unwrap_or(0),
            })
            .collect();
        mail.sort_by_key(|m| std::cmp::Reverse(m.at_ms));
        Ok(mail)
    })
}

/// Inbucket: one mail in full.
pub fn inbucket_read(base: &str, mailbox: &str, id: &str) -> Call<Body> {
    let url = format!(
        "{base}/api/v1/mailbox/{}/{}",
        percent_encode(mailbox),
        percent_encode(id)
    );
    Call::new(Request::get(url), |response| {
        let root = response.ok()?.json()?;
        let html = root
            .pointer("/body/html")
            .and_then(Value::as_str)
            .filter(|h| !h.is_empty())
            .map(str::to_owned);
        let text = root
            .pointer("/body/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let to = root
            .get("to")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        Ok(Body {
            from: field(&root, "from"),
            to,
            subject: field(&root, "subject"),
            text: if text.is_empty() {
                html.as_deref().map(html_text).unwrap_or_default()
            } else {
                text
            },
            html,
            at_ms: root.get("posix-millis").and_then(Value::as_u64).unwrap_or(0),
        })
    })
}

/// Inbucket: empties a mailbox.
pub fn inbucket_purge(base: &str, mailbox: &str) -> Call<()> {
    let url = format!("{base}/api/v1/mailbox/{}", percent_encode(mailbox));
    Call::new(Request::verb("DELETE", url), |response| response.ok().map(|_| ()))
}

/// A mail's HTML as readable text: tags dropped, the usual entities read, blank runs closed up.
pub fn html_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Whatever a style or script block holds is not text.
            for skip in ["<style", "<script", "<head"] {
                if lower[i..].starts_with(skip) {
                    let close = format!("</{}", &skip[1..]);
                    i = lower[i..].find(&close).map_or(bytes.len(), |at| i + at);
                    break;
                }
            }
            if i >= bytes.len() {
                break;
            }
            let tag_end = lower[i..].find('>').map_or(bytes.len(), |at| i + at + 1);
            let tag = &lower[i..tag_end.min(lower.len())];
            if ["<br", "<p", "</p", "<div", "</div", "<tr", "<li", "<h"].iter().any(|t| tag.starts_with(t)) {
                out.push('\n');
            }
            i = tag_end;
        } else {
            let next = lower[i..].find('<').map_or(bytes.len(), |at| i + at);
            out.push_str(&html[i..next]);
            i = next;
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut lines: Vec<&str> = Vec::new();
    for line in decoded.lines().map(str::trim) {
        if line.is_empty() && lines.last().is_none_or(|last| last.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_owned()
}

/// `2024-03-01T12:30:00+00:00` as Unix milliseconds. Zero for anything else.
pub fn rfc3339_ms(text: &str) -> u64 {
    let digits = |range: std::ops::Range<usize>| text.get(range).and_then(|t| t.parse::<i64>().ok());
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        digits(0..4),
        digits(5..7),
        digits(8..10),
        digits(11..13),
        digits(14..16),
        digits(17..19),
    ) else {
        return 0;
    };
    // Days since 1970 by Howard Hinnant's civil-from-days, run backwards.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    // An offset after the seconds, fractions skipped.
    let rest = text.get(19..).unwrap_or_default();
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let (Some(sign), Some(h), Some(m)) = (
        rest.chars().next(),
        rest.get(1..3).and_then(|t| t.parse::<i64>().ok()),
        rest.get(4..6).and_then(|t| t.parse::<i64>().ok()),
    ) && (sign == '+' || sign == '-')
    {
        let offset = h * 3_600 + m * 60;
        seconds += if sign == '+' { -offset } else { offset };
    }
    u64::try_from(seconds).map_or(0, |s| s * 1000)
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
    fn a_mail_tm_style_server_reads_in_full() {
        let http = FakeHttp::new().answer(
            "https://api.mail.gw/messages/m1",
            200,
            r#"{"from":{"address":"a@b"},"to":[{"address":"me@x.gw"}],"subject":"Hi","text":"","html":["<p>Your code is <b>42</b></p>"],"createdAt":"2024-03-01T12:30:00+00:00"}"#,
        );
        let body = mailtm_read_at(MAIL_GW, &Secret::new("t"), "m1").run(&http, ANY).unwrap();
        assert_eq!(body.to, "me@x.gw");
        assert_eq!(body.text, "Your code is 42");
        assert_eq!(body.html.as_deref(), Some("<p>Your code is <b>42</b></p>"));
        assert_eq!(body.at_ms, 1_709_296_200_000);
    }

    #[test]
    fn dates_and_html_read_as_people_would() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), 0);
        assert_eq!(rfc3339_ms("2024-03-01T18:30:00+06:00"), 1_709_296_200_000);
        assert_eq!(rfc3339_ms("2024-03-01T12:30:00.123Z"), 1_709_296_200_000);
        assert_eq!(rfc3339_ms("soon"), 0);
        assert_eq!(
            html_text("<html><head><style>p{}</style></head><body><p>One&nbsp;&amp; two</p><br><div>Three</div><script>x()</script></body></html>"),
            "One & two\n\nThree"
        );
    }

    #[test]
    fn inbucket_lists_newest_first() {
        let http = FakeHttp::new()
            .answer(
                "https://mail.example.com/api/v1/mailbox/me/1",
                200,
                r#"{"from":"a@b","to":["me@example.com"],"subject":"S","posix-millis":5,"body":{"text":"T","html":""}}"#,
            )
            .answer(
                "https://mail.example.com/api/v1/mailbox/me",
                200,
                r#"[{"id":"1","from":"a@b","subject":"old","posix-millis":1},{"id":"2","from":"a@b","subject":"new","posix-millis":9}]"#,
            );
        let mail = inbucket_messages("https://mail.example.com", "me").run(&http, ANY).unwrap();
        assert_eq!(mail[0].subject, "new");
        let body = inbucket_read("https://mail.example.com", "me", "1").run(&http, ANY).unwrap();
        assert_eq!((body.text.as_str(), body.html), ("T", None));
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
