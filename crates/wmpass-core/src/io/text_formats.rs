//! Plain-text exports.

use super::{Draft, Imported, note_item};
use crate::Result;
use crate::clock::Hlc;

const SECTIONS: [&str; 3] = ["Websites", "Applications", "Notes"];

/// Kaspersky Password Manager's text export starts with one of its section names.
pub(super) fn is_kaspersky(text: &str) -> bool {
    text.lines()
        .next()
        .is_some_and(|first| SECTIONS.contains(&first.trim()))
        && text.contains("\n---")
}

/// Kaspersky Password Manager: sections `Websites`, `Applications` and `Notes`, items
/// split by `---` lines, each line `Key: value`. A comment or a note's text runs on to
/// the end of its item.
pub(super) fn kaspersky(text: &str, at: Hlc) -> Result<Imported> {
    let mut out = Imported::default();
    let mut section = "";
    let mut item: Vec<(String, String)> = Vec::new();
    let mut running = false;
    let flush = |section: &str, item: &mut Vec<(String, String)>, out: &mut Imported| {
        if item.is_empty() {
            return;
        }
        let get = |key: &str| {
            item.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.trim_end().to_owned())
                .unwrap_or_default()
        };
        if section == "Notes" {
            out.entries
                .push(note_item(&get("Name"), &get("Text"), Vec::new(), None, at));
        } else {
            let name = match get("Website name") {
                name if name.is_empty() => get("Application"),
                name => name,
            };
            let label = get("Login name");
            let title = if label.is_empty() || label == name {
                name
            } else {
                format!("{name} ({label})")
            };
            let draft = Draft {
                title,
                urls: vec![get("Website URL")],
                username: get("Login"),
                password: get("Password"),
                notes: get("Comment"),
                ..Draft::default()
            };
            out.entries.push(draft.into_login(at, &mut out.skipped));
        }
        item.clear();
    };
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim() == "---" {
            flush(section, &mut item, &mut out);
            running = false;
            continue;
        }
        if running {
            if let Some((_, value)) = item.last_mut() {
                value.push('\n');
                value.push_str(line);
            }
            continue;
        }
        if item.is_empty() && SECTIONS.contains(&line.trim()) {
            section = SECTIONS
                .iter()
                .find(|s| **s == line.trim())
                .copied()
                .unwrap_or_default();
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.strip_prefix(' ').unwrap_or(value);
        item.push((key.to_owned(), value.to_owned()));
        running = key == "Comment" || key == "Text";
    }
    flush(section, &mut item, &mut out);
    Ok(out)
}
