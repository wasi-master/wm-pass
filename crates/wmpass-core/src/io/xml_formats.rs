//! XML exports.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{Draft, Imported};
use crate::clock::Hlc;
use crate::{Error, Result};

fn attr(start: &BytesStart<'_>, name: &str) -> String {
    start
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
        .unwrap_or_default()
}

/// SafeInCloud: `<database><card title="…"><field name="…" type="login">…</field>…</card></database>`.
/// Entity expansion is limited to XML's predefined ones, so a hostile file cannot
/// balloon, and DOCTYPE content is ignored rather than processed.
pub(super) fn safeincloud(xml: &str, at: Hlc) -> Result<Imported> {
    let mut reader = Reader::from_str(xml);
    let mut out = Imported::default();
    let mut card: Option<Draft> = None;
    let mut field: Option<(String, String)> = None;
    let mut body = String::new();
    let bad = || Error::Import("the XML could not be read".to_string());
    loop {
        match reader.read_event().map_err(|_| bad())? {
            Event::Start(start) => match start.name().as_ref() {
                "card" => {
                    if attr(&start, "template") == "true" || attr(&start, "deleted") == "true" {
                        reader.read_to_end(start.name()).map_err(|_| bad())?;
                        continue;
                    }
                    card = Some(Draft {
                        title: attr(&start, "title"),
                        ..Draft::default()
                    });
                }
                "field" => {
                    field = Some((attr(&start, "name"), attr(&start, "type")));
                    body.clear();
                }
                "notes" => body.clear(),
                _ => {}
            },
            Event::Text(chunk) => {
                body.push_str(&quick_xml::escape::unescape(&chunk).map_err(|_| bad())?);
            }
            Event::CData(chunk) => body.push_str(&chunk.into_inner()),
            Event::GeneralRef(reference) => {
                if let Some(ch) = reference.resolve_char_ref().map_err(|_| bad())? {
                    body.push(ch);
                } else if let Some(text) = quick_xml::escape::resolve_predefined_entity(&reference)
                {
                    body.push_str(text);
                }
            }
            Event::End(end) => match end.name().as_ref() {
                "field" => {
                    if let (Some(draft), Some((name, kind))) = (card.as_mut(), field.take()) {
                        let value = std::mem::take(&mut body);
                        match kind.as_str() {
                            "login" => draft.username = value,
                            "password" => draft.password = value,
                            "website" => draft.urls.push(value),
                            "one_time_password" => draft.otp = value,
                            "secret" | "pin" => draft.fields.push((name, value, true)),
                            _ => draft.fields.push((name, value, false)),
                        }
                    }
                }
                "notes" => {
                    if let Some(draft) = card.as_mut() {
                        draft.notes = std::mem::take(&mut body);
                    }
                }
                "card" => {
                    if let Some(draft) = card.take() {
                        let entry = draft.into_login(at, &mut out.skipped);
                        out.entries.push(entry);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}
