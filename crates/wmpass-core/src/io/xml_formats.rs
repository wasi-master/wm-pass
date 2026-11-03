//! XML exports, read into a small element tree first.
//!
//! Entity expansion is limited to XML's predefined ones and character references, so
//! a hostile file cannot balloon, and DOCTYPE content is ignored rather than processed.
//! Nesting is capped, so a deep file cannot exhaust the stack of the readers below.

use std::collections::HashMap;

use base64::Engine;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{Draft, Imported, kdbx};
use crate::clock::Hlc;
use crate::model::Lww;
use crate::{Error, Result};

const MAX_DEPTH: usize = 128;

/// One element: its name, attributes, child elements and the text directly inside it.
#[derive(Debug, Default)]
pub(super) struct Node {
    pub name: String,
    attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn attr(&self, name: &str) -> &str {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map_or("", |(_, value)| value.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The text of the first child called `name`, or nothing.
    pub fn text_of(&self, name: &str) -> String {
        self.child(name).map(|c| c.text.clone()).unwrap_or_default()
    }
}

fn open(start: &BytesStart<'_>) -> Node {
    let attrs = start
        .attributes()
        .flatten()
        .map(|a| {
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_default();
            (a.key.as_ref().to_owned(), value)
        })
        .collect();
    Node {
        name: start.name().as_ref().to_owned(),
        attrs,
        ..Node::default()
    }
}

/// Reads a whole document and returns its root element.
pub(super) fn tree(xml: &str) -> Result<Node> {
    let bad = || Error::Import("the XML could not be read".to_string());
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));
    let mut stack: Vec<Node> = Vec::new();
    loop {
        match reader.read_event().map_err(|_| bad())? {
            Event::Start(start) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(Error::Import("the XML is nested too deeply".into()));
                }
                stack.push(open(&start));
            }
            Event::Empty(start) => {
                let node = open(&start);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Ok(node),
                }
            }
            Event::Text(chunk) => {
                if let Some(node) = stack.last_mut() {
                    node.text
                        .push_str(&quick_xml::escape::unescape(&chunk).map_err(|_| bad())?);
                }
            }
            Event::CData(chunk) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&chunk.into_inner());
                }
            }
            Event::GeneralRef(reference) => {
                if let Some(node) = stack.last_mut() {
                    if let Some(ch) = reference.resolve_char_ref().map_err(|_| bad())? {
                        node.text.push(ch);
                    } else if let Some(text) =
                        quick_xml::escape::resolve_predefined_entity(&reference)
                    {
                        node.text.push_str(text);
                    }
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(bad)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Ok(node),
                }
            }
            Event::Eof => return Err(bad()),
            _ => {}
        }
    }
}

/// Base64 that may be wrapped over several lines.
pub(super) fn base64_body(text: &str) -> Option<Vec<u8>> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(compact)
        .ok()
}

/// KeePass 2's own XML export: `KeePassFile/Root/Group/…/Entry/String{Key,Value}`.
/// Groups become folders, the recycle bin and each entry's history are left out, and
/// attachments stored under `Meta/Binaries` come along.
pub(super) fn keepass(xml: &str, at: Hlc) -> Result<Imported> {
    let root = tree(xml)?;
    if root.name != "KeePassFile" {
        return Err(Error::Import("this is not a KeePass XML export".into()));
    }
    let meta = root.child("Meta");
    let recycle = meta
        .map(|m| m.text_of("RecycleBinUUID"))
        .unwrap_or_default();
    let mut binaries: HashMap<String, Vec<u8>> = HashMap::new();
    if let Some(list) = meta.and_then(|m| m.child("Binaries")) {
        for binary in list.all("Binary") {
            let Some(mut bytes) = base64_body(&binary.text) else {
                continue;
            };
            if binary.attr("Compressed") == "True" {
                bytes = gunzip(&bytes).unwrap_or_default();
            }
            binaries.insert(binary.attr("ID").to_owned(), bytes);
        }
    }
    let group = root
        .child("Root")
        .and_then(|r| r.child("Group"))
        .ok_or_else(|| Error::Import("no group in the file".into()))?;
    let mut out = Imported::default();
    keepass_group(group, "", &recycle, &binaries, at, &mut out)?;
    Ok(out)
}

fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(super::MAX_INFLATED)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

fn keepass_group(
    group: &Node,
    path: &str,
    recycle: &str,
    binaries: &HashMap<String, Vec<u8>>,
    at: Hlc,
    out: &mut Imported,
) -> Result<()> {
    if !recycle.is_empty() && group.text_of("UUID") == recycle {
        return Ok(());
    }
    for entry in group.all("Entry") {
        let mut strings: Vec<(String, String, bool)> = Vec::new();
        for string in entry.all("String") {
            let Some(value) = string.child("Value") else {
                continue;
            };
            if value.attr("Protected") == "True" {
                return Err(Error::Import(
                    "this XML holds encrypted values. In KeePass, export it as KeePass XML (2.x)"
                        .into(),
                ));
            }
            strings.push((
                string.text_of("Key"),
                value.text.clone(),
                value.attr("ProtectInMemory") == "True",
            ));
        }
        let get = |key: &str| {
            strings
                .iter()
                .find(|(k, _, _)| k == key)
                .map(|(_, v, _)| v.clone())
        };
        let title = get("Title").unwrap_or_default();
        let (otp, used) = kdbx::otp_from_fields(&get, &title, &mut out.skipped);
        let mut draft = Draft {
            title,
            username: get("UserName").unwrap_or_default(),
            password: get("Password").unwrap_or_default(),
            urls: vec![get("URL").unwrap_or_default()],
            notes: get("Notes").unwrap_or_default(),
            folder: path.to_owned(),
            tags: entry
                .text_of("Tags")
                .split([';', ','])
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect(),
            ..Draft::default()
        };
        for (key, value, hidden) in &strings {
            let standard = ["Title", "UserName", "Password", "URL", "Notes"];
            if !standard.contains(&key.as_str()) && !used.contains(&key.as_str()) {
                draft.fields.push((key.clone(), value.clone(), *hidden));
            }
        }
        let mut item = draft.into_login(at, &mut out.skipped);
        if let Some(config) = otp
            && let Some(login) = item.entry.login_mut()
        {
            login.otp = Lww::new(Some(config), at);
        }
        for binary in entry.all("Binary") {
            let name = binary.text_of("Key");
            let reference = binary.child("Value").map_or("", |v| v.attr("Ref"));
            match binaries.get(reference) {
                Some(bytes) => item.files.push((name, bytes.clone())),
                None => out.skipped.push(format!(
                    "{}: the attachment {name} was not in the file",
                    item.entry.common.title.v
                )),
            }
        }
        out.entries.push(item);
    }
    for child in group.all("Group") {
        let name = child.text_of("Name");
        let child_path = if path.is_empty() {
            name
        } else {
            format!("{path}/{name}")
        };
        keepass_group(child, &child_path, recycle, binaries, at, out)?;
    }
    Ok(())
}

/// Sticky Password: logins hold the username and password, accounts hold the title,
/// address and notes and link to their logins, and groups nest by parent id.
pub(super) fn sticky_password(xml: &str, at: Hlc) -> Result<Imported> {
    let root = tree(xml)?;
    let database = root
        .child("Database")
        .ok_or_else(|| Error::Import("no database in the file".into()))?;
    let groups: HashMap<&str, (&str, &str)> = database
        .child("Groups")
        .map(|g| {
            g.all("Group")
                .map(|group| {
                    (
                        group.attr("ID"),
                        (group.attr("Name"), group.attr("ParentID")),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let mut accounts: HashMap<&str, &Node> = HashMap::new();
    if let Some(list) = database.child("Accounts") {
        for account in list.all("Account") {
            if let Some(links) = account.child("LoginLinks") {
                for link in links.all("Login") {
                    accounts.insert(link.attr("SourceLoginID"), account);
                }
            }
        }
    }
    let mut out = Imported::default();
    for login in database
        .child("Logins")
        .map(|l| l.all("Login"))
        .into_iter()
        .flatten()
    {
        let id = login.attr("ID");
        if id.is_empty() {
            continue;
        }
        let account = accounts.get(id);
        let get = |name: &str| account.map_or("", |a| a.attr(name)).to_owned();
        let draft = Draft {
            title: get("Name"),
            username: login.attr("Name").to_owned(),
            password: login.attr("Password").to_owned(),
            urls: vec![get("Link")],
            // Sticky Password writes line breaks in notes as a literal "/n".
            notes: get("Comments").replace("/n", "\n"),
            folder: group_path(&groups, account.map_or("", |a| a.attr("ParentID"))),
            ..Draft::default()
        };
        out.entries.push(draft.into_login(at, &mut out.skipped));
    }
    Ok(out)
}

/// A Sticky Password group's path, from its id up through its parents.
fn group_path<'a>(groups: &HashMap<&'a str, (&'a str, &'a str)>, start: &'a str) -> String {
    let mut parts = Vec::new();
    let mut id = start;
    while let Some((name, parent)) = groups.get(id) {
        if parts.len() > MAX_DEPTH {
            break;
        }
        parts.push(*name);
        id = parent;
    }
    parts.reverse();
    parts.join("/")
}

/// Password Safe (pwsafe.org): `passwordsafe/entry` with child elements. Groups are
/// dotted paths, and notes join their lines with the root's `delimiter`.
pub(super) fn password_safe(xml: &str, at: Hlc) -> Result<Imported> {
    let root = tree(xml)?;
    if root.name != "passwordsafe" {
        return Err(Error::Import(
            "this is not a Password Safe XML export".into(),
        ));
    }
    let delimiter = root.attr("delimiter");
    let mut out = Imported::default();
    for entry in root.all("entry") {
        let mut notes = entry.text_of("notes");
        if !delimiter.is_empty() {
            notes = notes.replace(delimiter, "\n");
        }
        let email = entry.text_of("email");
        let mut draft = Draft {
            title: entry.text_of("title"),
            username: entry.text_of("username"),
            password: entry.text_of("password"),
            urls: vec![entry.text_of("url")],
            notes,
            folder: entry.text_of("group").replace('.', "/"),
            ..Draft::default()
        };
        if draft.username.is_empty() {
            draft.username = email;
        } else {
            draft.fields.push(("Email".into(), email, false));
        }
        out.entries.push(draft.into_login(at, &mut out.skipped));
    }
    Ok(out)
}
