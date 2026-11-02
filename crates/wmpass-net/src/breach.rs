//! Breach checks. All opt-in, and each one states what leaves the device:
//!
//! - HIBP passwords: five characters of a SHA-1. The password stays here.
//! - HIBP accounts and LeakCheck: the email address, sent to that service.
//! - Proxynova: the email address. Its answers hold stolen plaintext passwords, so the
//!   code here only takes emails from the vault, keeps nothing, and reports whether a
//!   line matched one of the owner's own values.

use serde_json::Value;
use sha1::{Digest, Sha1};
use wmpass_core::Secret;
use wmpass_core::otp::percent_encode;
use zeroize::Zeroize;

use crate::http::{Call, NetError, Request, Response};

/// Pwned Passwords' address.
pub const HIBP_RANGE: &str = "https://api.pwnedpasswords.com/range/";
/// HIBP's account API.
pub const HIBP_ACCOUNT: &str = "https://haveibeenpwned.com/api/v3/breachedaccount/";
/// LeakCheck's free endpoint.
pub const LEAKCHECK_PUBLIC: &str = "https://leakcheck.io/api/public";
/// LeakCheck's keyed endpoint.
pub const LEAKCHECK_PRO: &str = "https://leakcheck.io/api/v2/query/";
/// Proxynova's COMB search.
pub const PROXYNOVA: &str = "https://api.proxynova.com/comb";

fn sha1_hex(text: &str) -> String {
    let digest = Sha1::digest(text.as_bytes());
    let mut out = String::with_capacity(40);
    for byte in digest.iter() {
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

/// How many times a password appears in Pwned Passwords. Only the first five characters
/// of its SHA-1 are sent, and padding hides how many suffixes share them.
pub fn password_count(password: &Secret) -> Call<u64> {
    let mut hash = sha1_hex(password.expose());
    let suffix = hash.split_off(5);
    let request = Request::get(format!("{HIBP_RANGE}{hash}")).header("Add-Padding", "true");
    Call::new(request, move |response| {
        response.ok()?;
        for line in response.text()?.lines() {
            let Some((candidate, count)) = line.trim().split_once(':') else {
                continue;
            };
            if candidate.eq_ignore_ascii_case(&suffix) {
                // Padding rows carry a count of zero, which reads correctly as "not found".
                return count.trim().parse().map_err(|_| NetError::BadAnswer);
            }
        }
        Ok(0)
    })
}

/// One breach an address appeared in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Breach {
    /// Name of the breached service.
    pub name: String,
    /// When it happened, as the source wrote it.
    pub date: String,
    /// What leaked, when the source says.
    pub data: Vec<String>,
}

/// Breaches for an email address, using the owner's own HIBP key. The address is sent to HIBP.
pub fn hibp_account(email: &str, api_key: &Secret) -> Call<Vec<Breach>> {
    let url = format!(
        "{HIBP_ACCOUNT}{}?truncateResponse=false",
        percent_encode(email)
    );
    let request = Request::get(url).header("hibp-api-key", api_key.expose());
    Call::new(request, |response| {
        // HIBP's way of saying the address is clean.
        if response.status == 404 {
            return Ok(Vec::new());
        }
        let root = response.ok()?.json()?;
        let list = root.as_array().ok_or(NetError::BadAnswer)?;
        Ok(list
            .iter()
            .map(|item| Breach {
                name: text(item, "Name"),
                date: text(item, "BreachDate"),
                data: strings(item, "DataClasses"),
            })
            .collect())
    })
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |list| {
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
}

fn read_leakcheck(response: &Response) -> Result<Vec<Breach>, NetError> {
    let root = response.ok()?.json()?;
    if root.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(NetError::BadAnswer);
    }
    let fields = strings(&root, "fields");
    let sources = root
        .get("sources")
        .or_else(|| root.get("result"))
        .and_then(Value::as_array)
        .ok_or(NetError::BadAnswer)?;
    Ok(sources
        .iter()
        .map(|item| {
            let source = item.get("source").unwrap_or(item);
            Breach {
                name: text(source, "name"),
                date: text(source, "date"),
                data: fields.clone(),
            }
        })
        .collect())
}

/// LeakCheck's free lookup, one request a second. Commercial use needs their link shown.
pub fn leakcheck_public(email: &str) -> Call<Vec<Breach>> {
    let request = Request::get(format!(
        "{LEAKCHECK_PUBLIC}?check={}",
        percent_encode(email)
    ));
    Call::new(request, read_leakcheck)
}

/// LeakCheck with the owner's key.
pub fn leakcheck_pro(email: &str, api_key: &Secret) -> Call<Vec<Breach>> {
    let request = Request::get(format!("{LEAKCHECK_PRO}{}", percent_encode(email)))
        .header("X-API-Key", api_key.expose());
    Call::new(request, read_leakcheck)
}

/// An address known to be in the owner's vault. Proxynova can only be asked about
/// these, so this can never become a way to look up someone else's stolen passwords.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultEmail(String);

/// The addresses in a vault's logins and identities, as the only values Proxynova may be asked about.
pub fn vault_emails<'a>(
    entries: impl IntoIterator<Item = &'a wmpass_core::model::Entry>,
) -> Vec<VaultEmail> {
    use wmpass_core::model::Kind;
    let mut found: Vec<String> = Vec::new();
    for entry in entries {
        let candidate = match &entry.kind {
            Kind::Login(login) => login.username.v.as_str(),
            Kind::Identity(identity) => identity.email.v.as_str(),
            _ => continue,
        };
        let lower = candidate.trim().to_lowercase();
        if lower.contains('@') && !found.contains(&lower) {
            found.push(lower);
        }
    }
    found.into_iter().map(VaultEmail).collect()
}

/// What Proxynova's answer said about an address. Holds no leaked text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CombResult {
    /// Lines for exactly this address.
    pub lines: usize,
    /// Indexes into the candidates that appeared in a line.
    pub matched: Vec<usize>,
}

/// Asks whether any of the owner's passwords for an address appear in COMB. Each
/// answer line is compared and wiped, and only the verdict comes back.
pub fn proxynova_check(email: &VaultEmail, candidates: Vec<Secret>) -> Call<CombResult> {
    let address = email.0.clone();
    let url = format!(
        "{PROXYNOVA}?query={}&start=0&limit=100",
        percent_encode(&address)
    );
    Call::new(Request::get(url), move |response| {
        let mut root = response.ok()?.json()?;
        let mut result = CombResult::default();
        let lines = root
            .get_mut("lines")
            .and_then(Value::as_array_mut)
            .ok_or(NetError::BadAnswer)?;
        for line in lines.iter_mut() {
            if let Value::String(text) = line {
                // The search is by substring, so other people's lines arrive too.
                // Those are skipped unread.
                if let Some((who, leaked)) = text.split_once(':')
                    && who.trim().eq_ignore_ascii_case(&address)
                {
                    result.lines += 1;
                    for (index, candidate) in candidates.iter().enumerate() {
                        if leaked == candidate.expose() && !result.matched.contains(&index) {
                            result.matched.push(index);
                        }
                    }
                }
                text.zeroize();
            }
        }
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::NetPolicy;
    use crate::http::fake::FakeHttp;
    use wmpass_core::clock::Hlc;
    use wmpass_core::vault::login_entry;

    const ANY: NetPolicy = NetPolicy::Any;

    #[test]
    fn password_range_sends_only_five_characters_and_finds_the_suffix() {
        // SHA-1 of "password" is 5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8.
        let http = FakeHttp::new().answer(
            HIBP_RANGE,
            200,
            "0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n1E4C9B93F3F0682250B6CF8331B7EE68FD8:9545824\r\n",
        );
        let count = password_count(&"password".into()).run(&http, ANY).unwrap();
        assert_eq!(count, 9_545_824);
        let asked = http.last().unwrap();
        assert_eq!(asked.url, format!("{HIBP_RANGE}5BAA6"));
        assert!(
            asked
                .headers
                .iter()
                .any(|(name, value)| *name == "Add-Padding" && value == "true")
        );
        assert!(asked.body.is_none());
    }

    #[test]
    fn padding_rows_mean_not_found() {
        let http =
            FakeHttp::new().answer(HIBP_RANGE, 200, "1E4C9B93F3F0682250B6CF8331B7EE68FD8:0\r\n");
        assert_eq!(password_count(&"password".into()).run(&http, ANY), Ok(0));
        let empty = FakeHttp::new().answer(HIBP_RANGE, 200, "AAAA:3\r\n");
        assert_eq!(password_count(&"password".into()).run(&empty, ANY), Ok(0));
    }

    #[test]
    fn hibp_account_reads_breaches_and_treats_404_as_clean() {
        let body = r#"[{"Name":"Adobe","BreachDate":"2013-10-04","DataClasses":["Email addresses","Passwords"]}]"#;
        let http = FakeHttp::new().answer(HIBP_ACCOUNT, 200, body);
        let found = hibp_account("me+tag@example.com", &"key".into())
            .run(&http, ANY)
            .unwrap();
        assert_eq!(found[0].name, "Adobe");
        assert_eq!(found[0].data.len(), 2);
        let asked = http.last().unwrap();
        assert!(
            asked.url.contains("me%2Btag%40example.com"),
            "{}",
            asked.url
        );
        let clean = FakeHttp::new().answer(HIBP_ACCOUNT, 404, "");
        assert_eq!(
            hibp_account("a@b.c", &"key".into()).run(&clean, ANY),
            Ok(Vec::new())
        );
        let refused = FakeHttp::new().answer(HIBP_ACCOUNT, 401, "");
        assert_eq!(
            hibp_account("a@b.c", &"key".into()).run(&refused, ANY),
            Err(NetError::Unauthorised)
        );
    }

    #[test]
    fn leakcheck_public_and_pro() {
        let public = r#"{"success":true,"found":1,"fields":["password"],"sources":[{"name":"Site","date":"2020-01"}]}"#;
        let http = FakeHttp::new().answer(LEAKCHECK_PUBLIC, 200, public);
        let found = leakcheck_public("a@b.c").run(&http, ANY).unwrap();
        assert_eq!(
            (found[0].name.as_str(), found[0].data[0].as_str()),
            ("Site", "password")
        );
        let pro = r#"{"success":true,"found":1,"result":[{"email":"a@b.c","source":{"name":"Site","breach_date":"x","date":"2021"}}]}"#;
        let http = FakeHttp::new().answer(LEAKCHECK_PRO, 200, pro);
        assert_eq!(
            leakcheck_pro("a@b.c", &"k".into()).run(&http, ANY).unwrap()[0].date,
            "2021"
        );
    }

    #[test]
    fn proxynova_compares_locally_and_ignores_other_peoples_lines() {
        let body = r#"{"count":3,"lines":["me@example.com:my-old-pass","me@example.com:not-mine","notme@example.com:other-pass"]}"#;
        let http = FakeHttp::new().answer(PROXYNOVA, 200, body);
        let entry = login_entry("Mail", "Me@Example.com", "x", Hlc::default());
        let emails = vault_emails([&entry]);
        assert_eq!(emails.len(), 1);
        let candidates = vec![
            Secret::new("my-old-pass"),
            Secret::new("other-pass"),
            Secret::new("current"),
        ];
        let result = proxynova_check(&emails[0], candidates)
            .run(&http, ANY)
            .unwrap();
        assert_eq!(
            result,
            CombResult {
                lines: 2,
                matched: vec![0]
            }
        );
        let printed = format!("{result:?}");
        assert!(
            !printed.contains("pass"),
            "no leaked text may survive: {printed}"
        );
    }

    #[test]
    fn vault_emails_skip_plain_usernames_and_repeat_addresses() {
        let at = Hlc::default();
        let (a, b, c) = (
            login_entry("A", "octo", "", at),
            login_entry("B", "x@y.z", "", at),
            login_entry("C", "X@Y.z", "", at),
        );
        assert_eq!(vault_emails([&a, &b, &c]).len(), 1);
    }

    #[test]
    fn nothing_is_sent_offline() {
        let http = FakeHttp::new();
        assert_eq!(
            password_count(&"x".into()).run(&http, NetPolicy::Offline),
            Err(NetError::Blocked)
        );
        assert!(http.asked.lock().unwrap().is_empty());
    }
}
