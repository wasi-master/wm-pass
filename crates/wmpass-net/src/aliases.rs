//! Email aliases: hosted services through the owner's key, and address tricks that
//! need no service at all.

use serde_json::{Value, json};
use wmpass_core::Secret;
use wmpass_core::crypto::random_below;

use crate::http::{Call, NetError, Request, Response};

/// An alias as WM Pass tracks it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Alias {
    /// The provider's id for it.
    pub id: String,
    /// The address.
    pub address: String,
    /// Whether mail is forwarded.
    pub enabled: bool,
    /// Note saved with it.
    pub note: String,
}

/// A hosted alias service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provider {
    /// SimpleLogin, or a self-hosted instance at `base`.
    SimpleLogin {
        /// Instance address.
        base: String,
    },
    /// addy.io, or a self-hosted instance at `base`.
    Addy {
        /// Instance address.
        base: String,
        /// Domain to create aliases on.
        domain: String,
    },
    /// Firefox Relay.
    FirefoxRelay,
    /// DuckDuckGo Email Protection. It has no public API: this uses the private endpoint
    /// its own extension calls, with a token copied by hand, and may stop working.
    DuckDuckGo,
}

impl Provider {
    /// SimpleLogin's hosted service.
    pub fn simplelogin() -> Self {
        Self::SimpleLogin {
            base: "https://app.simplelogin.io".into(),
        }
    }

    /// addy.io's hosted service.
    pub fn addy() -> Self {
        Self::Addy {
            base: "https://app.addy.io".into(),
            domain: "anonaddy.me".into(),
        }
    }

    /// The id stored on entries.
    pub fn id(&self) -> &'static str {
        match self {
            Self::SimpleLogin { .. } => "simplelogin",
            Self::Addy { .. } => "addy",
            Self::FirefoxRelay => "firefox-relay",
            Self::DuckDuckGo => "duckduckgo",
        }
    }

    /// Whether this rests on an unofficial endpoint, so screens can say so.
    pub fn is_experimental(&self) -> bool {
        matches!(self, Self::DuckDuckGo)
    }

    /// Whether aliases can be listed, switched off and deleted here.
    pub fn can_manage(&self) -> bool {
        !matches!(self, Self::DuckDuckGo)
    }

    fn authed(&self, request: Request, key: &Secret) -> Request {
        match self {
            Self::SimpleLogin { .. } => request.header("Authentication", key.expose()),
            Self::Addy { .. } => request
                .header("Authorization", format!("Bearer {}", key.expose()))
                .header("X-Requested-With", "XMLHttpRequest"),
            Self::FirefoxRelay => {
                request.header("Authorization", format!("Token {}", key.expose()))
            }
            Self::DuckDuckGo => request.header("Authorization", format!("Bearer {}", key.expose())),
        }
    }

    /// Creates an alias. `note` usually names the site it is for.
    pub fn create(&self, key: &Secret, note: &str) -> Call<Alias> {
        let note = note.to_owned();
        match self {
            Self::SimpleLogin { base } => {
                let request = Request::json(
                    "POST",
                    format!("{base}/api/alias/random/new?mode=word"),
                    &json!({"note": note}),
                );
                Call::new(self.authed(request, key), read_simplelogin_one)
            }
            Self::Addy { base, domain } => {
                let body = json!({"domain": domain, "description": note, "format": "random_words"});
                let request = Request::json("POST", format!("{base}/api/v1/aliases"), &body);
                Call::new(self.authed(request, key), |response| {
                    let root = response.ok()?.json()?;
                    read_addy(root.get("data").ok_or(NetError::BadAnswer)?)
                })
            }
            Self::FirefoxRelay => {
                let body = json!({"enabled": true, "description": note, "generated_for": note});
                let request = Request::json(
                    "POST",
                    "https://relay.firefox.com/api/v1/relayaddresses/",
                    &body,
                );
                Call::new(self.authed(request, key), |response| {
                    read_relay(&response.ok()?.json()?)
                })
            }
            Self::DuckDuckGo => {
                let request =
                    Request::verb("POST", "https://quack.duckduckgo.com/api/email/addresses");
                Call::new(self.authed(request, key), move |response| {
                    let root = response.ok()?.json()?;
                    let local = root
                        .get("address")
                        .and_then(Value::as_str)
                        .ok_or(NetError::BadAnswer)?;
                    Ok(Alias {
                        id: local.to_owned(),
                        address: format!("{local}@duck.com"),
                        enabled: true,
                        note,
                    })
                })
            }
        }
    }

    /// Lists aliases. DuckDuckGo cannot be listed.
    pub fn list(&self, key: &Secret) -> Result<Call<Vec<Alias>>, NetError> {
        let unsupported = || NetError::Invalid("this service cannot list aliases".to_string());
        Ok(match self {
            Self::SimpleLogin { base } => {
                let request = Request::get(format!("{base}/api/v2/aliases?page_id=0"));
                Call::new(self.authed(request, key), |response| {
                    let root = response.ok()?.json()?;
                    let list = root
                        .get("aliases")
                        .and_then(Value::as_array)
                        .ok_or(NetError::BadAnswer)?;
                    list.iter().map(read_simplelogin).collect()
                })
            }
            Self::Addy { base, .. } => {
                let request = Request::get(format!("{base}/api/v1/aliases?page[size]=100"));
                Call::new(self.authed(request, key), |response| {
                    let root = response.ok()?.json()?;
                    let list = root
                        .get("data")
                        .and_then(Value::as_array)
                        .ok_or(NetError::BadAnswer)?;
                    list.iter().map(read_addy).collect()
                })
            }
            Self::FirefoxRelay => {
                let request = Request::get("https://relay.firefox.com/api/v1/relayaddresses/");
                Call::new(self.authed(request, key), |response| {
                    let root = response.ok()?.json()?;
                    root.as_array()
                        .ok_or(NetError::BadAnswer)?
                        .iter()
                        .map(read_relay)
                        .collect()
                })
            }
            Self::DuckDuckGo => return Err(unsupported()),
        })
    }

    /// Turns forwarding on or off.
    pub fn set_enabled(&self, key: &Secret, id: &str, enabled: bool) -> Result<Call<()>, NetError> {
        let id = encode(id);
        let request = match self {
            // SimpleLogin only offers a toggle, and the caller knows the current state.
            Self::SimpleLogin { base } => {
                Request::verb("POST", format!("{base}/api/aliases/{id}/toggle"))
            }
            Self::Addy { base, .. } if enabled => Request::json(
                "POST",
                format!("{base}/api/v1/active-aliases"),
                &json!({"id": id}),
            ),
            Self::Addy { base, .. } => {
                Request::verb("DELETE", format!("{base}/api/v1/active-aliases/{id}"))
            }
            Self::FirefoxRelay => Request::json(
                "PATCH",
                format!("https://relay.firefox.com/api/v1/relayaddresses/{id}/"),
                &json!({"enabled": enabled}),
            ),
            Self::DuckDuckGo => {
                return Err(NetError::Invalid(
                    "this service cannot switch aliases".into(),
                ));
            }
        };
        Ok(Call::new(self.authed(request, key), |response| {
            response.ok().map(|_| ())
        }))
    }

    /// Deletes an alias. Mail to it stops.
    pub fn delete(&self, key: &Secret, id: &str) -> Result<Call<()>, NetError> {
        let id = encode(id);
        let url = match self {
            Self::SimpleLogin { base } => format!("{base}/api/aliases/{id}"),
            Self::Addy { base, .. } => format!("{base}/api/v1/aliases/{id}"),
            Self::FirefoxRelay => format!("https://relay.firefox.com/api/v1/relayaddresses/{id}/"),
            Self::DuckDuckGo => {
                return Err(NetError::Invalid(
                    "this service cannot delete aliases".into(),
                ));
            }
        };
        Ok(Call::new(
            self.authed(Request::verb("DELETE", url), key),
            |response| response.ok().map(|_| ()),
        ))
    }
}

fn encode(id: &str) -> String {
    wmpass_core::otp::percent_encode(id)
}

fn id_text(value: &Value) -> String {
    match value.get("id") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn read_simplelogin(value: &Value) -> Result<Alias, NetError> {
    Ok(Alias {
        id: id_text(value),
        address: value
            .get("email")
            .or_else(|| value.get("alias"))
            .and_then(Value::as_str)
            .ok_or(NetError::BadAnswer)?
            .to_owned(),
        enabled: value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        note: value
            .get("note")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn read_simplelogin_one(response: &Response) -> Result<Alias, NetError> {
    read_simplelogin(&response.ok()?.json()?)
}

fn read_addy(value: &Value) -> Result<Alias, NetError> {
    Ok(Alias {
        id: id_text(value),
        address: value
            .get("email")
            .and_then(Value::as_str)
            .ok_or(NetError::BadAnswer)?
            .to_owned(),
        enabled: value.get("active").and_then(Value::as_bool).unwrap_or(true),
        note: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn read_relay(value: &Value) -> Result<Alias, NetError> {
    Ok(Alias {
        id: id_text(value),
        address: value
            .get("full_address")
            .and_then(Value::as_str)
            .ok_or(NetError::BadAnswer)?
            .to_owned(),
        enabled: value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        note: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

/// Fastmail's masked email over JMAP. Needs the account id from [`fastmail_session`].
pub fn fastmail_masked(key: &Secret, account_id: &str, note: &str) -> Call<Alias> {
    let note = note.to_owned();
    let body = json!({
        "using": ["urn:ietf:params:jmap:core", "https://www.fastmail.com/dev/maskedemail"],
        "methodCalls": [["MaskedEmail/set", {
            "accountId": account_id,
            "create": {"new": {"state": "enabled", "description": note, "forDomain": ""}},
        }, "0"]],
    });
    let request = Request::json("POST", "https://api.fastmail.com/jmap/api/", &body)
        .header("Authorization", format!("Bearer {}", key.expose()));
    Call::new(request, move |response| {
        let root = response.ok()?.json()?;
        let created = root
            .pointer("/methodResponses/0/1/created/new")
            .ok_or(NetError::BadAnswer)?;
        Ok(Alias {
            id: id_text(created),
            address: created
                .get("email")
                .and_then(Value::as_str)
                .ok_or(NetError::BadAnswer)?
                .to_owned(),
            enabled: true,
            note,
        })
    })
}

/// Fastmail's primary account id, which masked email needs.
pub fn fastmail_session(key: &Secret) -> Call<String> {
    let request = Request::get("https://api.fastmail.com/jmap/session")
        .header("Authorization", format!("Bearer {}", key.expose()));
    Call::new(request, |response| {
        let root = response.ok()?.json()?;
        root.pointer("/primaryAccounts/https:~1~1www.fastmail.com~1dev~1maskedemail")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(NetError::BadAnswer)
    })
}

fn split(address: &str) -> Result<(&str, &str), NetError> {
    address
        .trim()
        .rsplit_once('@')
        .filter(|(local, domain)| !local.is_empty() && !domain.is_empty())
        .ok_or_else(|| NetError::Invalid("that is not an email address".into()))
}

/// `me+tag@domain`. Most mail services deliver these, though sites can strip the tag.
pub fn plus_address(address: &str, tag: &str) -> Result<String, NetError> {
    let (local, domain) = split(address)?;
    let base = local.split('+').next().unwrap_or(local);
    let tag: String = tag
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let tag = tag.trim_matches('-');
    if tag.is_empty() {
        return Err(NetError::Invalid("the tag needs a letter or digit".into()));
    }
    Ok(format!("{base}+{tag}@{domain}"))
}

/// A Gmail address with dots placed at random, which Gmail ignores on delivery. Other
/// services treat dots as part of the name, so this refuses anything else.
pub fn gmail_dots(address: &str) -> Result<String, NetError> {
    let (local, domain) = split(address)?;
    let domain_lower = domain.to_lowercase();
    if domain_lower != "gmail.com" && domain_lower != "googlemail.com" {
        return Err(NetError::Invalid("dots are only ignored by Gmail".into()));
    }
    let plain: Vec<char> = local.chars().filter(|c| *c != '.').collect();
    if plain.len() < 2 {
        return Err(NetError::Invalid("that name is too short for dots".into()));
    }
    let mut out = String::new();
    for (index, ch) in plain.iter().enumerate() {
        out.push(*ch);
        if index + 1 < plain.len() && random_below(2).map_err(|_| NetError::BadAnswer)? == 1 {
            out.push('.');
        }
    }
    Ok(format!("{out}@{domain}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::NetPolicy;
    use crate::http::fake::FakeHttp;

    const ANY: NetPolicy = NetPolicy::Any;

    fn header(http: &FakeHttp, name: &str) -> Option<String> {
        http.last()?
            .headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
    }

    #[test]
    fn simplelogin_create_list_toggle_delete() {
        let provider = Provider::simplelogin();
        let http = FakeHttp::new()
            .answer(
                "https://app.simplelogin.io/api/alias/random",
                201,
                r#"{"id":7,"email":"a.b@sl.co","enabled":true,"note":"site"}"#,
            )
            .answer(
                "https://app.simplelogin.io/api/v2/aliases",
                200,
                r#"{"aliases":[{"id":7,"email":"a.b@sl.co","enabled":false,"note":null}]}"#,
            )
            .answer("https://app.simplelogin.io/api/aliases/7", 200, "{}");
        let created = provider
            .create(&"key".into(), "site")
            .run(&http, ANY)
            .unwrap();
        assert_eq!(
            (created.id.as_str(), created.address.as_str()),
            ("7", "a.b@sl.co")
        );
        assert_eq!(header(&http, "Authentication").as_deref(), Some("key"));
        let listed = provider
            .list(&"key".into())
            .unwrap()
            .run(&http, ANY)
            .unwrap();
        assert!(!listed[0].enabled);
        provider
            .set_enabled(&"key".into(), "7", true)
            .unwrap()
            .run(&http, ANY)
            .unwrap();
        assert!(http.last().unwrap().url.ends_with("/toggle"));
        provider
            .delete(&"key".into(), "7")
            .unwrap()
            .run(&http, ANY)
            .unwrap();
        assert_eq!(http.last().unwrap().method, "DELETE");
    }

    #[test]
    fn addy_create_and_switch() {
        let provider = Provider::addy();
        let http = FakeHttp::new().answer(
            "https://app.addy.io/api/v1/aliases",
            201,
            r#"{"data":{"id":"u-1","email":"x@anonaddy.me","active":true,"description":"site"}}"#,
        ).answer("https://app.addy.io/api/v1/active-aliases", 200, "{}");
        let created = provider
            .create(&"k".into(), "site")
            .run(&http, ANY)
            .unwrap();
        assert_eq!(created.address, "x@anonaddy.me");
        assert_eq!(header(&http, "Authorization").as_deref(), Some("Bearer k"));
        assert_eq!(
            header(&http, "X-Requested-With").as_deref(),
            Some("XMLHttpRequest")
        );
        provider
            .set_enabled(&"k".into(), "u-1", false)
            .unwrap()
            .run(&http, ANY)
            .unwrap();
        assert_eq!(http.last().unwrap().method, "DELETE");
        provider
            .set_enabled(&"k".into(), "u-1", true)
            .unwrap()
            .run(&http, ANY)
            .unwrap();
        assert_eq!(http.last().unwrap().method, "POST");
    }

    #[test]
    fn firefox_relay_uses_token_auth() {
        let provider = Provider::FirefoxRelay;
        let http = FakeHttp::new().answer(
            "https://relay.firefox.com/api/v1/relayaddresses/",
            201,
            r#"{"id":3,"full_address":"r@mozmail.com","enabled":true,"description":"s"}"#,
        );
        let created = provider.create(&"k".into(), "s").run(&http, ANY).unwrap();
        assert_eq!(created.address, "r@mozmail.com");
        assert_eq!(header(&http, "Authorization").as_deref(), Some("Token k"));
    }

    #[test]
    fn duckduckgo_builds_the_address_and_says_it_is_experimental() {
        let provider = Provider::DuckDuckGo;
        assert!(provider.is_experimental() && !provider.can_manage());
        let http = FakeHttp::new().answer(
            "https://quack.duckduckgo.com",
            201,
            r#"{"address":"abc123"}"#,
        );
        assert_eq!(
            provider
                .create(&"k".into(), "s")
                .run(&http, ANY)
                .unwrap()
                .address,
            "abc123@duck.com"
        );
        assert!(provider.list(&"k".into()).is_err());
        assert!(provider.delete(&"k".into(), "x").is_err());
    }

    #[test]
    fn fastmail_masked_email() {
        let http = FakeHttp::new()
            .answer("https://api.fastmail.com/jmap/session", 200, r#"{"primaryAccounts":{"https://www.fastmail.com/dev/maskedemail":"u123"}}"#)
            .answer("https://api.fastmail.com/jmap/api", 200, r#"{"methodResponses":[["MaskedEmail/set",{"created":{"new":{"id":"m1","email":"x.y@fastmail.com"}}},"0"]]}"#);
        let account = fastmail_session(&"k".into()).run(&http, ANY).unwrap();
        assert_eq!(account, "u123");
        let alias = fastmail_masked(&"k".into(), &account, "site")
            .run(&http, ANY)
            .unwrap();
        assert_eq!(alias.address, "x.y@fastmail.com");
    }

    #[test]
    fn a_bad_key_and_rubbish_answers_are_told_apart() {
        let provider = Provider::simplelogin();
        let refused = FakeHttp::new().answer("https://app.simplelogin.io", 401, "");
        assert_eq!(
            provider.create(&"k".into(), "").run(&refused, ANY),
            Err(NetError::Unauthorised)
        );
        let rubbish = FakeHttp::new().answer("https://app.simplelogin.io", 200, "<html>");
        assert_eq!(
            provider.create(&"k".into(), "").run(&rubbish, ANY),
            Err(NetError::BadAnswer)
        );
    }

    #[test]
    fn plus_addressing() {
        assert_eq!(
            plus_address("me@gmail.com", "GitHub!").unwrap(),
            "me+github@gmail.com"
        );
        assert_eq!(plus_address("me+old@x.org", "new").unwrap(), "me+new@x.org");
        assert!(plus_address("nope", "x").is_err());
        assert!(plus_address("me@x.org", "!!").is_err());
    }

    #[test]
    fn gmail_dots_only_on_gmail_and_keep_the_letters() {
        let made = gmail_dots("first.last@gmail.com").unwrap();
        let (local, domain) = made.split_once('@').unwrap();
        assert_eq!(domain, "gmail.com");
        assert_eq!(local.replace('.', ""), "firstlast");
        assert!(!local.starts_with('.') && !local.ends_with('.'));
        assert!(gmail_dots("me@outlook.com").is_err());
    }
}
