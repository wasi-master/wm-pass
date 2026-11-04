//! Everything that waits on a network. These calls run outside the lock on [`Service`], so a slow
//! server leaves the vault usable. They hold no vault state of their own: a shell reveals
//! what a call needs, such as an API key, and passes it in as bytes.
//!
//! The calls over HTTP are written once, against [`AsyncHttp`]. Desktop and Android run them on a
//! blocking client, whose futures are ready at once; the browser runs them over `fetch`. The local
//! network needs raw sockets, so it is native only, in `lan`.

use std::sync::Mutex;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wmpass_core::crypto;
use wmpass_core::Secret;
use wmpass_net::aliases::{self, Alias, Provider};
use wmpass_net::breach;
use wmpass_net::favicon::{self, IconSource};
use wmpass_net::http::{AsyncHttp, Call as NetCall, NetError, NetPolicy};
use wmpass_net::inbox;
use wmpass_net::usernames::{self, Availability};
use zeroize::Zeroizing;

use crate::service::{Reply, Service, SessionError};

pub(crate) fn failed(code: &'static str, message: impl Into<String>) -> SessionError {
    SessionError { code, message: message.into() }
}

fn net_error(error: NetError) -> SessionError {
    failed("network", error.to_string())
}

pub(crate) fn parse<T: DeserializeOwned>(args: &str) -> Result<T, SessionError> {
    serde_json::from_str(if args.is_empty() { "{}" } else { args })
        .map_err(|e| failed("invalid", format!("bad arguments: {e}")))
}

fn secret_text(secret: &[u8]) -> Result<Secret, SessionError> {
    Ok(Secret::new(std::str::from_utf8(secret).map_err(|_| failed("invalid", "not text"))?))
}

async fn run<T>(http: &impl AsyncHttp, call: NetCall<T>, online: bool) -> Result<T, SessionError> {
    let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
    call.run_async(http, policy).await.map_err(net_error)
}

fn answer_word(answer: Availability) -> &'static str {
    match answer {
        Availability::Available => "free",
        Availability::Taken => "taken",
        Availability::Unknown => "unknown",
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NameArgs {
    site: String,
    name: String,
}

fn page_site(name: &str) -> Result<&'static usernames::PageSite, SessionError> {
    usernames::PAGE_SITES.iter().find(|s| s.name == name).ok_or_else(|| failed("invalid", "unknown site"))
}

pub(crate) fn view(value: Value) -> Reply {
    Reply { json: value.to_string(), ..Reply::default() }
}

fn alias_view(alias: &Alias) -> Value {
    json!({"id": alias.id, "address": alias.address, "enabled": alias.enabled, "note": alias.note})
}

fn provider(args: &AliasArgs) -> Result<Provider, SessionError> {
    Ok(match args.provider.as_str() {
        "simplelogin" => match &args.base {
            Some(base) => Provider::SimpleLogin { base: base.clone() },
            None => Provider::simplelogin(),
        },
        "addy" => match (&args.base, &args.domain) {
            (None, None) => Provider::addy(),
            _ => Provider::Addy {
                base: args.base.clone().unwrap_or_else(|| "https://app.addy.io".to_owned()),
                domain: args.domain.clone().unwrap_or_else(|| "anonaddy.me".to_owned()),
            },
        },
        "firefox-relay" => Provider::FirefoxRelay,
        "duckduckgo" => Provider::DuckDuckGo,
        _ => return Err(failed("invalid", "unknown alias service")),
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AliasArgs {
    provider: String,
    base: Option<String>,
    domain: Option<String>,
    note: String,
    id: String,
    enabled: bool,
    address: String,
    tag: String,
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Where a kept mailbox's service lives. The named ones are fixed; the rest are the owner's.
fn inbox_base(provider: &str, base: &str) -> Result<String, SessionError> {
    match provider {
        "mailtm" => Ok(inbox::MAIL_TM.to_owned()),
        "mailgw" => Ok(inbox::MAIL_GW.to_owned()),
        "guerrilla" => Ok(String::new()),
        "custom" | "inbucket" => {
            let base = base.trim().trim_end_matches('/');
            let scheme = base.starts_with("https://") || base.starts_with("http://");
            if !scheme || base.contains(char::is_whitespace) || base.len() < 10 {
                return Err(failed("invalid", "give the server's address, such as https://mail.example.com"));
            }
            Ok(base.to_owned())
        }
        _ => Err(failed("invalid", "unknown inbox service")),
    }
}

/// A kept mailbox, read from the open vault.
fn kept_inbox(service: &Mutex<Option<Service>>, handle: u64, id: &str) -> Result<wmpass_core::model::Inbox, SessionError> {
    let mut guard = service.lock().map_err(|_| failed("busy", "try that again"))?;
    let service = guard.as_mut().ok_or_else(|| failed("not_started", "the vault service has not started"))?;
    service
        .with_vault(handle, |vault| vault.doc.prefs.inboxes.get(id).and_then(|kept| kept.v.clone()))?
        .ok_or_else(|| failed("invalid", "that inbox was closed"))
}

/// Writes a kept mailbox back, or closes it with `None`.
fn keep_inbox(service: &Mutex<Option<Service>>, handle: u64, id: &str, inbox: Option<wmpass_core::model::Inbox>, now_ms: u64) -> Result<(), SessionError> {
    let mut guard = service.lock().map_err(|_| failed("busy", "try that again"))?;
    let service = guard.as_mut().ok_or_else(|| failed("not_started", "the vault service has not started"))?;
    service.with_vault(handle, |vault| {
        let at = vault.tick(now_ms);
        vault.doc.prefs.inboxes.insert(id.to_owned(), wmpass_core::model::Lww::new(inbox, at));
    })?;
    service.touch(handle)
}

fn mail_rows(list: &[inbox::Message]) -> Value {
    json!(list.iter().map(|m| json!({"id": m.id, "from": m.from, "subject": m.subject, "preview": m.preview, "at_ms": m.at_ms})).collect::<Vec<_>>())
}

/// A kept mail.tm-style mailbox's list, or one message from it.
async fn mailtm_mail(http: &impl AsyncHttp, base: &str, token: &Secret, reading: bool, message: &str, online: bool) -> Result<Value, SessionError> {
    if reading {
        let body = run(http, inbox::mailtm_read_at(base, token, message), online).await?;
        Ok(json!({"from": body.from, "to": body.to, "subject": body.subject, "text": body.text, "html": body.html, "at_ms": body.at_ms}))
    } else {
        Ok(mail_rows(&run(http, inbox::mailtm_messages_at(base, token), online).await?))
    }
}

/// Whether a call belongs here rather than on the service itself.
pub fn is_net_method(method: &str) -> bool {
    method == "remote.sync" || ["breach.", "alias.", "usernames.", "inbox.", "icon.", "lan.", "share.", "bitwarden."].iter().any(|prefix| method.starts_with(prefix))
}

/// One call, on a blocking client or over the local network. `service` is locked only for the
/// moments vault state is needed.
#[cfg(feature = "net")]
pub fn call(
    service: &Mutex<Option<Service>>,
    method: &str,
    handle: u64,
    args: &str,
    secret: &[u8],
    online: bool,
    now_ms: u64,
) -> Result<Reply, SessionError> {
    if crate::lan::is_lan_method(method) {
        return crate::lan::call(service, method, handle, args, secret, now_ms);
    }
    let native = wmpass_net::http::NativeHttp::default();
    wmpass_net::http::block_on(call_over(&wmpass_net::http::Blocking(&native), service, method, handle, args, secret, online, now_ms))
}

/// One call over HTTP, on whatever client the shell has.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn call_over(
    http: &impl AsyncHttp,
    service: &Mutex<Option<Service>>,
    method: &str,
    handle: u64,
    args: &str,
    secret: &[u8],
    online: bool,
    now_ms: u64,
) -> Result<Reply, SessionError> {
    let with_service = |run: &mut dyn FnMut(&mut Service) -> Result<Reply, SessionError>| -> Result<Reply, SessionError> {
        let mut guard = service.lock().map_err(|_| failed("busy", "try that again"))?;
        let service = guard.as_mut().ok_or_else(|| failed("not_started", "the vault service has not started"))?;
        run(service)
    };
    match method {
        "breach.password" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                source: String,
            }
            let a: A = parse(args)?;
            let password = secret_text(secret)?;
            let count = match a.source.as_str() {
                "" | "hibp" => run(http, breach::password_count(&password), online).await?,
                "xon" => run(http, breach::password_count_xon(&password), online).await?,
                _ => return Err(failed("invalid", "unknown source")),
            };
            Ok(view(json!({"count": count})))
        }
        "icon.fetch" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                host: String,
                source: String,
            }
            let a: A = parse(args)?;
            let host = a.host.trim().trim_end_matches('.').to_ascii_lowercase();
            // A router's or NAS's page is nobody else's business, and often has no certificate.
            if wmpass_core::health::is_local_host(&host) {
                return Err(failed("invalid", "no icons for local addresses"));
            }
            let tries: &[IconSource] = match a.source.as_str() {
                "duckduckgo" => &[IconSource::DuckDuckGo],
                _ => &[IconSource::SiteTouch, IconSource::Site],
            };
            let mut last = failed("not_found", "no icon");
            for source in tries {
                let fetched = match favicon::fetch(*source, &host) {
                    Ok(call) => run(http, call, online).await,
                    Err(error) => Err(net_error(error)),
                };
                match fetched {
                    Ok(bytes) => return Ok(Reply { json: json!({}).to_string(), blobs: vec![bytes], ..Reply::default() }),
                    Err(error) => last = error,
                }
            }
            Err(last)
        }
        "breach.account" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                source: String,
                email: String,
            }
            let a: A = parse(args)?;
            let found = match a.source.as_str() {
                "hibp" => run(http, breach::hibp_account(&a.email, &secret_text(secret)?), online).await?,
                "leakcheck" => run(http, breach::leakcheck_public(&a.email), online).await?,
                "leakcheck_pro" => run(http, breach::leakcheck_pro(&a.email, &secret_text(secret)?), online).await?,
                _ => return Err(failed("invalid", "unknown source")),
            };
            let rows: Vec<Value> = found.iter().map(|b| json!({"name": b.name, "date": b.date, "data": b.data})).collect();
            Ok(view(json!(rows)))
        }
        "breach.comb" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                email: String,
            }
            let a: A = parse(args)?;
            // Only an address and passwords from the open vault, whatever a shell passes in.
            let mut prepared = None;
            with_service(&mut |service| {
                prepared = Some(service.comb_inputs(handle, &a.email)?);
                Ok(Reply::default())
            })?;
            let (email, candidates, ids) = prepared.ok_or_else(|| failed("invalid", "nothing to check"))?;
            let result = run(http, breach::proxynova_check(&email, candidates), online).await?;
            let matched: Vec<&String> = result.matched.iter().filter_map(|i| ids.get(*i)).collect();
            Ok(view(json!({"lines": result.lines, "matched": matched})))
        }
        // ---- aliases ----
        "alias.create" => {
            let a: AliasArgs = parse(args)?;
            let made = run(http, provider(&a)?.create(&secret_text(secret)?, &a.note), online).await?;
            Ok(view(alias_view(&made)))
        }
        "alias.list" => {
            let a: AliasArgs = parse(args)?;
            let list = run(http, provider(&a)?.list(&secret_text(secret)?).map_err(net_error)?, online).await?;
            Ok(view(json!(list.iter().map(alias_view).collect::<Vec<_>>())))
        }
        "alias.set_enabled" => {
            let a: AliasArgs = parse(args)?;
            run(http, provider(&a)?.set_enabled(&secret_text(secret)?, &a.id, a.enabled).map_err(net_error)?, online).await?;
            Ok(view(json!({})))
        }
        "alias.delete" => {
            let a: AliasArgs = parse(args)?;
            run(http, provider(&a)?.delete(&secret_text(secret)?, &a.id).map_err(net_error)?, online).await?;
            Ok(view(json!({})))
        }
        "alias.plus" => {
            let a: AliasArgs = parse(args)?;
            Ok(view(json!({"address": aliases::plus_address(&a.address, &a.tag).map_err(net_error)?})))
        }
        "alias.dots" => {
            let a: AliasArgs = parse(args)?;
            Ok(view(json!({"address": aliases::gmail_dots(&a.address).map_err(net_error)?})))
        }
        // ---- usernames ----
        "usernames.sites" => {
            let names: Vec<&str> = usernames::SITES.iter().map(|s| s.name).collect();
            Ok(view(json!(names)))
        }
        // Every site in one list: `check` is asked here, `page` is read in a hidden browser
        // view by the app, and `link` is only opened for a person to look at.
        "usernames.site_list" => {
            let mut sites: Vec<(usize, Value)> = usernames::SITES
                .iter()
                .map(|s| (usernames::rank(s.name), json!({"name": s.name, "profile": s.profile, "icon": s.icon, "kind": "check"})))
                .chain(usernames::PAGE_SITES.iter().map(|s| (usernames::rank(s.name), json!({"name": s.name, "profile": s.profile, "icon": s.icon, "kind": "page"}))))
                .chain(usernames::LINK_SITES.iter().map(|s| (usernames::rank(s.name), json!({"name": s.name, "profile": s.url, "icon": s.icon, "kind": "link"}))))
                .collect();
            // Most used first; a stable sort keeps list order among the rest.
            sites.sort_by_key(|(rank, _)| *rank);
            Ok(view(json!(sites.into_iter().map(|(_, site)| site).collect::<Vec<_>>())))
        }
        "usernames.check" => {
            let a: NameArgs = parse(args)?;
            let site = usernames::SITES.iter().find(|s| s.name == a.site).ok_or_else(|| failed("invalid", "unknown site"))?;
            let answer = run(http, usernames::check(*site, &a.name).map_err(net_error)?, online).await?;
            Ok(view(json!({"answer": answer_word(answer)})))
        }
        "usernames.pages" => {
            let names: Vec<&str> = usernames::PAGE_SITES.iter().map(|s| s.name).collect();
            Ok(view(json!(names)))
        }
        "usernames.page" => {
            let a: NameArgs = parse(args)?;
            let site = page_site(&a.site)?;
            Ok(view(json!({"url": usernames::page_url(site, &a.name).map_err(net_error)?, "domain": site.domain})))
        }
        "usernames.read_page" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                site: String,
                text: String,
            }
            let a: A = parse(args)?;
            let answer = usernames::read_page(page_site(&a.site)?, &a.text);
            Ok(view(json!({"answer": answer.map_or("waiting", answer_word)})))
        }
        "usernames.links" => {
            let a: NameArgs = parse(args)?;
            let links = usernames::LINK_SITES
                .iter()
                .map(|site| Ok(json!({"site": site.name, "url": usernames::link_url(site, &a.name).map_err(net_error)?})))
                .collect::<Result<Vec<_>, SessionError>>()?;
            Ok(view(json!(links)))
        }
        // ---- disposable inboxes ----
        "inbox.new" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                service: String,
            }
            let a: A = parse(args)?;
            match a.service.as_str() {
                "mailtm" => {
                    let domains = run(http, inbox::mailtm_domains(), online).await?;
                    let domain = domains.first().ok_or_else(|| failed("network", "the service offered no addresses"))?;
                    let local: [u8; 8] = crypto::random_array()?;
                    let address = format!("{}@{domain}", hex(&local));
                    let password: [u8; 18] = crypto::random_array()?;
                    let password = Secret::from(hex(&password));
                    let account = run(http, inbox::mailtm_create(&address, &password), online).await?;
                    let token = run(http, inbox::mailtm_token(&address, &password), online).await?;
                    Ok(Reply {
                        json: json!({"address": address, "service": "mailtm", "account": account}).to_string(),
                        secret: Some(Zeroizing::new(token.expose().as_bytes().to_vec())),
                        blobs: Vec::new(),
                    })
                }
                "guerrilla" => {
                    let session = run(http, inbox::guerrilla_new(), online).await?;
                    Ok(Reply {
                        json: json!({"address": session.address, "service": "guerrilla"}).to_string(),
                        secret: Some(Zeroizing::new(session.token.expose().as_bytes().to_vec())),
                        blobs: Vec::new(),
                    })
                }
                _ => Err(failed("invalid", "unknown inbox service")),
            }
        }
        "inbox.messages" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                service: String,
                address: String,
            }
            let a: A = parse(args)?;
            let token = secret_text(secret)?;
            let list = if a.service == "mailtm" {
                run(http, inbox::mailtm_messages(&token), online).await?
            } else {
                run(http, inbox::guerrilla_messages(&inbox::GuerrillaSession { address: a.address, token }), online).await?
            };
            let rows: Vec<Value> = list.iter().map(|m| json!({"id": m.id, "from": m.from, "subject": m.subject, "preview": m.preview})).collect();
            Ok(view(json!(rows)))
        }
        "inbox.body" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                id: String,
            }
            let a: A = parse(args)?;
            let text = run(http, inbox::mailtm_body(&secret_text(secret)?, &a.id), online).await?;
            Ok(view(json!({"text": text})))
        }
        // ---- kept inboxes: their tokens stay in the vault and never reach the app ----
        "inbox.sessions" => with_service(&mut |service| {
            let rows = service.with_vault(handle, |vault| {
                let mut kept: Vec<(&String, &wmpass_core::model::Inbox)> =
                    vault.doc.prefs.inboxes.iter().filter_map(|(id, kept)| kept.v.as_ref().map(|inbox| (id, inbox))).collect();
                kept.sort_by_key(|kept| std::cmp::Reverse(kept.1.created_ms));
                kept.iter()
                    .map(|(id, inbox)| {
                        json!({"id": id, "provider": inbox.provider, "base": inbox.base, "address": inbox.address, "label": inbox.label, "created_ms": inbox.created_ms})
                    })
                    .collect::<Vec<Value>>()
            })?;
            Ok(view(json!(rows)))
        }),
        "inbox.open" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                provider: String,
                base: String,
                domain: String,
                label: String,
            }
            let a: A = parse(args)?;
            let base = inbox_base(&a.provider, &a.base)?;
            let made = match a.provider.as_str() {
                "mailtm" | "mailgw" | "custom" => {
                    let domains = run(http, inbox::mailtm_domains_at(&base), online).await?;
                    let domain = domains.first().ok_or_else(|| failed("network", "the service offered no addresses"))?;
                    let local: [u8; 8] = crypto::random_array()?;
                    let address = format!("{}@{domain}", hex(&local));
                    let password: [u8; 18] = crypto::random_array()?;
                    let password = Secret::from(hex(&password));
                    let account = run(http, inbox::mailtm_create_at(&base, &address, &password), online).await?;
                    let token = run(http, inbox::mailtm_token_at(&base, &address, &password), online).await?;
                    wmpass_core::model::Inbox { provider: a.provider.clone(), base, address, label: a.label.clone(), token, password, account, created_ms: now_ms }
                }
                "guerrilla" => {
                    let session = run(http, inbox::guerrilla_new(), online).await?;
                    wmpass_core::model::Inbox {
                        provider: "guerrilla".to_owned(),
                        address: session.address,
                        token: session.token,
                        label: a.label.clone(),
                        created_ms: now_ms,
                        ..wmpass_core::model::Inbox::default()
                    }
                }
                "inbucket" => {
                    let domain = a.domain.trim().trim_start_matches('@').to_ascii_lowercase();
                    if domain.is_empty() || domain.contains(['/', '@', ' ']) || !domain.contains('.') {
                        return Err(failed("invalid", "name the domain your Inbucket server takes mail for"));
                    }
                    let local: [u8; 6] = crypto::random_array()?;
                    wmpass_core::model::Inbox {
                        provider: "inbucket".to_owned(),
                        base,
                        address: format!("{}@{domain}", hex(&local)),
                        label: a.label.clone(),
                        created_ms: now_ms,
                        ..wmpass_core::model::Inbox::default()
                    }
                }
                _ => return Err(failed("invalid", "unknown inbox service")),
            };
            let address = made.address.clone();
            let id_bytes: [u8; 8] = crypto::random_array()?;
            let id = hex(&id_bytes);
            keep_inbox(service, handle, &id, Some(made), now_ms)?;
            Ok(view(json!({"id": id, "address": address})))
        }
        "inbox.list" | "inbox.read" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                id: String,
                message: String,
            }
            let a: A = parse(args)?;
            let mut kept = kept_inbox(service, handle, &a.id)?;
            let reading = method == "inbox.read";
            let reply = match kept.provider.as_str() {
                "mailtm" | "mailgw" | "custom" => {
                    let base = kept.base.clone();
                    match mailtm_mail(http, &base, &kept.token, reading, &a.message, online).await {
                        Ok(reply) => reply,
                        Err(_) => {
                            // Tokens run out, so the mailbox signs in again with its own password.
                            kept.token = run(http, inbox::mailtm_token_at(&base, &kept.address, &kept.password), online).await?;
                            let reply = mailtm_mail(http, &base, &kept.token, reading, &a.message, online).await?;
                            keep_inbox(service, handle, &a.id, Some(kept.clone()), now_ms)?;
                            reply
                        }
                    }
                }
                "guerrilla" => {
                    // Guerrilla's sessions lapse when quiet, and a lapsed one hands out a new
                    // address, so each visit first points the session back at this one.
                    let resumed = run(http, inbox::guerrilla_resume(&kept.token, &kept.address), online).await?;
                    if resumed.token != kept.token {
                        kept.token = resumed.token.clone();
                        keep_inbox(service, handle, &a.id, Some(kept.clone()), now_ms)?;
                    }
                    let session = inbox::GuerrillaSession { address: kept.address.clone(), token: resumed.token };
                    if reading {
                        let body = run(http, inbox::guerrilla_read(&session, &a.message), online).await?;
                        json!({"from": body.from, "to": body.to, "subject": body.subject, "text": body.text, "html": body.html, "at_ms": body.at_ms})
                    } else {
                        mail_rows(&run(http, inbox::guerrilla_messages(&session), online).await?)
                    }
                }
                "inbucket" => {
                    let mailbox = kept.address.split('@').next().unwrap_or_default().to_owned();
                    if reading {
                        let body = run(http, inbox::inbucket_read(&kept.base, &mailbox, &a.message), online).await?;
                        json!({"from": body.from, "to": body.to, "subject": body.subject, "text": body.text, "html": body.html, "at_ms": body.at_ms})
                    } else {
                        mail_rows(&run(http, inbox::inbucket_messages(&kept.base, &mailbox), online).await?)
                    }
                }
                _ => return Err(failed("invalid", "unknown inbox service")),
            };
            Ok(view(reply))
        }
        "inbox.rename" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                id: String,
                label: String,
            }
            let a: A = parse(args)?;
            let mut kept = kept_inbox(service, handle, &a.id)?;
            kept.label = a.label.trim().to_owned();
            keep_inbox(service, handle, &a.id, Some(kept), now_ms)?;
            Ok(view(json!({})))
        }
        "inbox.close" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                id: String,
                /// Also delete the mailbox on the service, where it allows that.
                delete: bool,
            }
            let a: A = parse(args)?;
            let kept = kept_inbox(service, handle, &a.id)?;
            if a.delete && online {
                // Best effort: the mailbox is forgotten here either way.
                let _ = match kept.provider.as_str() {
                    "mailtm" | "mailgw" | "custom" => run(http, inbox::mailtm_delete_at(&kept.base, &kept.token, &kept.account), online).await,
                    "inbucket" => run(http, inbox::inbucket_purge(&kept.base, kept.address.split('@').next().unwrap_or_default()), online).await,
                    _ => Ok(()),
                };
            }
            keep_inbox(service, handle, &a.id, None, now_ms)?;
            Ok(view(json!({})))
        }
        // ---- accounts held elsewhere ----
        "bitwarden.login" | "bitwarden.open_cached" | "bitwarden.register" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct A {
                server: Option<String>,
                email: String,
                name: String,
                device: String,
                cache: String,
                code: Option<String>,
                provider: Option<u32>,
                /// The code Bitwarden emailed to confirm this device.
                device_code: Option<String>,
                region: Option<String>,
                /// Where this runs, for a shell the engine cannot tell from its build, such as
                /// `chrome-extension`. Bitwarden names the device by it in sign-in emails.
                platform: String,
            }
            use wmpass_bw::account::Account;
            use wmpass_bw::api::{Device, Endpoints, SecondFactor};
            let a: A = parse(args)?;
            let endpoints = match (&a.server, a.region.as_deref()) {
                (Some(base), _) if !base.is_empty() => Endpoints::self_hosted(base),
                (_, Some("eu")) => Endpoints::official_eu(),
                _ => Endpoints::official(),
            };
            let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
            // Only a refused password is a guess. The unlock screen counts those towards its
            // wait, so a dropped connection or a server fault must not look like one.
            let bw_error = |e: wmpass_bw::BwError| match e {
                wmpass_bw::BwError::TwoFactor(providers) => failed("two_factor", format!("{providers:?}")),
                wmpass_bw::BwError::NewDevice => failed("new_device", e.to_string()),
                wmpass_bw::BwError::BadCode => failed("bad_code", e.to_string()),
                wmpass_bw::BwError::Net(error) => net_error(error),
                wmpass_bw::BwError::BadLogin => failed("wrong_password", e.to_string()),
                // Offline, the saved key opening is the password check. Online the server has
                // already taken the password, so what will not decrypt is the account's own.
                wmpass_bw::BwError::Crypto if method == "bitwarden.open_cached" => failed("wrong_password", e.to_string()),
                wmpass_bw::BwError::Crypto => {
                    failed("bitwarden", "Bitwarden accepted the password, but part of this account is encrypted in a way WM Pass cannot read yet")
                }
                other => failed("bitwarden", other.to_string()),
            };
            if method == "bitwarden.register" {
                wmpass_bw::account::register_async(
                    http,
                    policy,
                    &endpoints,
                    &a.email,
                    &a.name,
                    secret,
                    wmpass_bw::crypto::Kdf::Argon2id { iterations: 3, memory_mib: 64, parallelism: 4 },
                )
                .await
                .map_err(bw_error)?;
                return Ok(view(json!({})));
            }
            let account = if method == "bitwarden.login" {
                let kind = wmpass_bw::api::device_kind(&a.platform);
                let device = Device { identifier: a.device.clone(), name: "WM Pass".to_owned(), kind };
                let factor = match (a.code, a.device_code) {
                    (Some(code), _) => Some(SecondFactor::Code { provider: a.provider.unwrap_or(0), code }),
                    (None, Some(code)) => Some(SecondFactor::NewDevice(code)),
                    (None, None) => None,
                };
                Account::login_async(http, policy, endpoints, &a.email, secret, &device, factor.as_ref()).await.map_err(bw_error)?
            } else {
                let cached = crate::service::base64_decode(&a.cache)?;
                Account::unlock_offline(endpoints, &cached, secret).map_err(bw_error)?
            };
            let name = account.email().to_owned();
            let unreadable = account.unreadable();
            let mut slot = Some(Box::new(account));
            with_service(&mut |service| {
                let account = slot.take().ok_or_else(|| failed("busy", "try that again"))?;
                service.adopt_bitwarden(&name, account, unreadable, now_ms)
            })
        }
        "remote.sync" => {
            let policy = if online { NetPolicy::Any } else { NetPolicy::Offline };
            let mut taken = None;
            with_service(&mut |service| {
                taken = Some(service.take_bitwarden(handle, now_ms)?);
                Ok(Reply::default())
            })?;
            let mut account = taken.ok_or_else(|| failed("invalid", "nothing to sync"))?;
            let pushed = match account.push_async(http, policy).await {
                Ok(()) => account.pull_async(http, policy).await,
                Err(error) => Err(error),
            };
            let mut account_slot = Some(account);
            with_service(&mut |service| {
                if let Some(account) = account_slot.take() {
                    service.return_bitwarden(handle, account, now_ms)?;
                }
                Ok(Reply::default())
            })?;
            pushed.map_err(|e| failed("bitwarden", e.to_string()))?;
            Ok(view(json!({})))
        }
        _ => Err(failed("invalid", format!("no such call: {method}"))),
    }
}

#[cfg(all(test, feature = "net"))]
#[cfg(test)]
mod tests {
    use super::*;

    fn login_error(online: bool) -> SessionError {
        let service = Mutex::new(None);
        let args = json!({"server": "https://127.0.0.1:9", "email": "a@example.com", "device": "test"}).to_string();
        let Err(error) = call(&service, "bitwarden.login", 0, &args, b"not the password", online, 0) else {
            panic!("a login with no server cannot succeed");
        };
        error
    }

    /// The unlock screen waits longer after each wrong password, so a sign-in that never
    /// reached a server must not come back looking like one.
    #[test]
    fn a_login_that_reaches_no_server_is_a_network_failure() {
        assert_eq!(login_error(false).code, "network");
        assert_eq!(login_error(true).code, "network");
    }
}
