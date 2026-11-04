//! Writing vault contents out. A plain export holds every secret in the clear.

use serde_json::{Value, json};

use crate::model::{Entry, Kind, SiteMatcher};
use crate::vault::Vault;
use crate::{Error, Result, Secret};

/// Every type-specific field of a non-login kind: name, value, whether it is hidden.
pub fn kind_fields(kind: &Kind) -> Vec<(&'static str, Secret, bool)> {
    let shown = |name, value: &str| (name, Secret::new(value), false);
    let hidden = |name, value: &Secret| (name, value.clone(), true);
    let list: Vec<(&'static str, Secret, bool)> = match kind {
        Kind::Login(_) => Vec::new(),
        Kind::Card(c) => vec![
            shown("Cardholder", &c.holder.v),
            hidden("Number", &c.number.v),
            shown("Expiry", &c.expiry.v),
            hidden("CVV", &c.cvv.v),
            hidden("PIN", &c.pin.v),
            shown("Issuer", &c.issuer.v),
            shown("Billing address", &c.billing.v),
        ],
        Kind::Identity(i) => vec![
            shown("Full name", &i.full_name.v),
            shown("Address", &i.address.v),
            shown("Phone", &i.phone.v),
            shown("Email", &i.email.v),
            hidden("Birth date", &i.birth_date.v),
        ],
        Kind::Note(n) => vec![hidden("Body", &n.body.v)],
        Kind::Document(d) => vec![shown("Label", &d.label.v), hidden("Number", &d.number.v)],
        Kind::SshKey(s) => vec![
            hidden("Private key", &s.private_key.v),
            shown("Public key", &s.public_key.v),
            shown("Fingerprint", &s.fingerprint.v),
            hidden("Passphrase", &s.passphrase.v),
        ],
        Kind::ApiKey(a) => vec![
            shown("Provider", &a.provider.v),
            hidden("Key", &a.key.v),
            hidden("Secret", &a.secret.v),
            shown("Scopes", &a.scopes.v),
        ],
        Kind::Database(d) => vec![
            shown("Engine", &d.engine.v),
            shown("Host", &d.host.v),
            shown(
                "Port",
                &d.port.v.map_or_else(String::new, |p| p.to_string()),
            ),
            shown("Database", &d.name.v),
            shown("User", &d.user.v),
            hidden("Password", &d.password.v),
            hidden("URL", &d.url.v),
        ],
        Kind::Licence(l) => vec![
            shown("Product", &l.product.v),
            hidden("Key", &l.key.v),
            shown("Email", &l.email.v),
            shown("Order", &l.order_id.v),
        ],
        Kind::Recovery(r) => vec![hidden("Phrase", &r.phrase.v)],
        Kind::Wifi(w) => vec![shown("SSID", &w.ssid.v), hidden("Password", &w.password.v)],
        Kind::Otp(o) => o
            .config
            .v
            .iter()
            .map(|c| hidden("otpauth", &c.to_uri()))
            .collect(),
    };
    list.into_iter()
        .filter(|(_, value, _)| !value.is_empty())
        .collect()
}

fn urls(entry: &Entry) -> Vec<String> {
    entry.login().map_or_else(Vec::new, |login| {
        login
            .site_matchers
            .v
            .iter()
            .filter_map(|m| match m {
                SiteMatcher::Url(u) | SiteMatcher::Domain(u) | SiteMatcher::Host(u) => {
                    Some(u.clone())
                }
                SiteMatcher::AndroidApp { package, .. } => Some(format!("androidapp://{package}")),
                _ => None,
            })
            .collect()
    })
}

fn folder_name(vault: &Vault, entry: &Entry) -> String {
    entry
        .common
        .folder
        .v
        .as_ref()
        .and_then(|id| vault.doc.folders.get(id))
        .map_or_else(String::new, |f| f.name.v.clone())
}

/// An entry's folder with its parents, as `Parent/Child`. Empty when it has none.
pub fn folder_path(vault: &Vault, entry: &Entry) -> String {
    let mut parts = Vec::new();
    let mut next = entry.common.folder.v.clone();
    while let Some(id) = next {
        let Some(folder) = vault.doc.folders.get(&id) else {
            break;
        };
        // A loop in the parents would otherwise never end.
        if parts.len() > 64 {
            break;
        }
        parts.push(folder.name.v.clone());
        next = folder.parent.v.clone();
    }
    parts.reverse();
    parts.join("/")
}

fn exportable(vault: &Vault) -> impl Iterator<Item = &Entry> {
    vault.doc.entries.values().filter(|e| !e.is_trashed())
}

/// WM Pass CSV. Other kinds keep their fields in the `fields` column.
pub fn to_csv(vault: &Vault) -> Result<Secret> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    let header = [
        "wmpass_kind",
        "folder",
        "title",
        "username",
        "password",
        "urls",
        "otp",
        "notes",
        "tags",
        "fields",
    ];
    writer.write_record(header).map_err(|_| Error::Encoding)?;
    for entry in exportable(vault) {
        let (username, password, otp) = entry.login().map_or_else(
            || (String::new(), Secret::default(), Secret::default()),
            |login| {
                let otp = login
                    .otp
                    .v
                    .as_ref()
                    .map_or_else(Secret::default, crate::otp::OtpConfig::to_uri);
                (login.username.v.clone(), login.password.v.clone(), otp)
            },
        );
        let mut custom: Vec<Value> = entry
            .common
            .custom_fields
            .v
            .iter()
            .map(|f| serde_json::to_value(f).unwrap_or(Value::Null))
            .collect();
        for (name, value, hidden) in kind_fields(&entry.kind) {
            custom.push(json!({"name": name, "value": value.expose(), "kind": if hidden { "Hidden" } else { "Text" }}));
        }
        writer
            .write_record([
                entry.kind.name(),
                folder_name(vault, entry).as_str(),
                entry.common.title.v.as_str(),
                username.as_str(),
                password.expose(),
                urls(entry).join(" ").as_str(),
                otp.expose(),
                entry.common.notes.v.expose(),
                entry.common.tags.v.join(",").as_str(),
                Value::Array(custom).to_string().as_str(),
            ])
            .map_err(|_| Error::Encoding)?;
    }
    let bytes = writer.into_inner().map_err(|_| Error::Encoding)?;
    String::from_utf8(bytes)
        .map(Secret::from)
        .map_err(|_| Error::Encoding)
}

/// Bitwarden's unencrypted JSON, which most other managers can also read. Kinds it
/// lacks become secure notes with their values as custom fields.
pub fn to_bitwarden_json(vault: &Vault) -> Result<Secret> {
    let mut items = Vec::new();
    for entry in exportable(vault) {
        let mut fields: Vec<Value> = entry
            .common
            .custom_fields
            .v
            .iter()
            .map(|f| {
                let kind = u8::from(matches!(f.kind, crate::model::FieldKind::Hidden));
                json!({"name": f.name, "value": f.value.expose(), "type": kind})
            })
            .collect();
        let mut item = json!({
            "id": entry.id,
            "name": entry.common.title.v,
            "notes": entry.common.notes.v.expose(),
            "favorite": entry.common.favourite.v,
            "folderId": entry.common.folder.v,
        });
        match &entry.kind {
            Kind::Login(login) => {
                let uris: Vec<Value> = urls(entry)
                    .into_iter()
                    .map(|u| json!({"uri": u, "match": null}))
                    .collect();
                let history: Vec<Value> = entry
                    .history_of("password")
                    .map(|r| json!({"password": r.value.expose()}))
                    .collect();
                item["type"] = json!(1);
                item["login"] = json!({
                    "username": login.username.v,
                    "password": login.password.v.expose(),
                    "totp": login.otp.v.as_ref().map(|o| o.to_uri().expose().to_owned()),
                    "uris": uris,
                });
                item["passwordHistory"] = json!(history);
            }
            Kind::Note(note) => {
                item["type"] = json!(2);
                item["secureNote"] = json!({"type": 0});
                item["notes"] = json!(note.body.v.expose());
            }
            Kind::Card(card) => {
                let (month, year) = card.expiry.v.split_once('/').unwrap_or(("", ""));
                item["type"] = json!(3);
                item["card"] = json!({
                    "cardholderName": card.holder.v,
                    "brand": card.issuer.v,
                    "number": card.number.v.expose(),
                    "expMonth": month,
                    "expYear": year,
                    "code": card.cvv.v.expose(),
                });
            }
            Kind::SshKey(key) => {
                item["type"] = json!(5);
                item["sshKey"] = json!({
                    "privateKey": key.private_key.v.expose(),
                    "publicKey": key.public_key.v,
                    "keyFingerprint": key.fingerprint.v,
                });
            }
            other => {
                item["type"] = json!(2);
                item["secureNote"] = json!({"type": 0});
                for (name, value, hidden) in kind_fields(other) {
                    fields.push(
                        json!({"name": name, "value": value.expose(), "type": u8::from(hidden)}),
                    );
                }
            }
        }
        item["fields"] = Value::Array(fields);
        items.push(item);
    }
    let folders: Vec<Value> = vault
        .doc
        .folders
        .values()
        .map(|f| json!({"id": f.id, "name": f.name.v}))
        .collect();
    let root = json!({"encrypted": false, "folders": folders, "items": items});
    serde_json::to_string_pretty(&root)
        .map(Secret::from)
        .map_err(|_| Error::Encoding)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A page to print and lock away. Values are escaped, so a hostile title cannot
/// run anything in whatever renders it.
pub fn to_print_html(vault: &Vault) -> Secret {
    let mut body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{name}</title>\
<style>body{{font:12pt serif;margin:2cm}}h2{{font-size:13pt;margin:1.2em 0 .2em}}\
table{{border-collapse:collapse;width:100%}}td{{border-top:1px solid #999;padding:2pt 6pt;vertical-align:top}}\
td:first-child{{width:9em;color:#333}}code{{font:11pt monospace;word-break:break-all}}\
.warn{{border:2px solid #000;padding:8pt}}</style></head><body><h1>{name}</h1>\
<p class=\"warn\">Anyone holding this page can read everything on it. Keep it locked away.</p>",
        name = escape(&vault.doc.name),
    );
    for entry in exportable(vault) {
        body.push_str(&format!(
            "<h2>{}</h2><table>",
            escape(&entry.common.title.v)
        ));
        let mut row = |name: &str, value: &str| {
            if !value.is_empty() {
                body.push_str(&format!(
                    "<tr><td>{}</td><td><code>{}</code></td></tr>",
                    escape(name),
                    escape(value)
                ));
            }
        };
        if let Some(login) = entry.login() {
            row("Username", &login.username.v);
            row("Password", login.password.v.expose());
            row("Address", &urls(entry).join(" "));
            if let Some(otp) = &login.otp.v {
                row("One-time code", otp.to_uri().expose());
            }
        }
        for (name, value, _) in kind_fields(&entry.kind) {
            row(name, value.expose());
        }
        for field in &entry.common.custom_fields.v {
            row(&field.name, field.value.expose());
        }
        row("Notes", entry.common.notes.v.expose());
        body.push_str("</table>");
    }
    body.push_str("</body></html>");
    Secret::from(body)
}
