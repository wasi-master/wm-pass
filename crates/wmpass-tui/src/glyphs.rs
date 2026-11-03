//! Icons. The plain set uses symbols that ordinary terminal fonts carry. The Nerd Font set
//! draws real icons from a patched font, so it is opt-in: without such a font each one shows
//! as an empty box.

/// Something with an icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Brand,
    All,
    Favourite,
    Pinned,
    Codes,
    Passkey,
    Attachment,
    Folder,
    Tag,
    Hidden,
    Trash,
    Search,
    Clipboard,
    Timer,
    /// A kind of entry, by its name.
    Kind(&'static str),
    /// A tab, by its position.
    Tab(usize),
}

/// The icon set in use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glyphs {
    /// Whether to draw Nerd Font icons.
    pub nerd: bool,
}

/// The kinds, with their plain and Nerd Font icons.
const KINDS: [(&str, &str, &str); 12] = [
    ("login", "◉", "\u{f084}"),
    ("card", "▭", "\u{f09d}"),
    ("identity", "☺", "\u{f2c2}"),
    ("note", "≡", "\u{f249}"),
    ("otp", "◷", "\u{f017}"),
    ("ssh", "⌁", "\u{f120}"),
    ("api", "⚿", "\u{f121}"),
    ("database", "⛁", "\u{f1c0}"),
    ("licence", "✦", "\u{f0a3}"),
    ("recovery", "⟲", "\u{f1cd}"),
    ("wifi", "≋", "\u{f1eb}"),
    ("document", "▤", "\u{f15c}"),
];

impl Glyphs {
    /// The set to start with: `WMPASS_ICONS` if set, else what was saved.
    pub fn pick(saved: &str) -> Self {
        let wanted = std::env::var("WMPASS_ICONS").unwrap_or_else(|_| saved.to_owned());
        Self { nerd: wanted.eq_ignore_ascii_case("nerd") }
    }

    /// The name saved in preferences.
    pub fn name(self) -> &'static str {
        if self.nerd { "nerd" } else { "plain" }
    }

    /// A kind's icon, by the kind's name.
    pub fn kind(self, kind: &str) -> &'static str {
        let (plain, nerd) = KINDS.iter().find(|k| k.0 == kind).map_or(("•", "\u{f128}"), |k| (k.1, k.2));
        if self.nerd { nerd } else { plain }
    }

    /// An icon, in this set.
    pub fn get(self, glyph: Glyph) -> &'static str {
        let (plain, nerd) = match glyph {
            Glyph::Brand => ("◆", "\u{f132}"),
            Glyph::All => ("▣", "\u{f187}"),
            Glyph::Favourite => ("★", "\u{f005}"),
            Glyph::Pinned => ("⚲", "\u{f08d}"),
            Glyph::Codes => ("◷", "\u{f017}"),
            Glyph::Passkey => ("⚷", "\u{f21b}"),
            Glyph::Attachment => ("⎙", "\u{f0c6}"),
            Glyph::Folder => ("▸", "\u{f07b}"),
            Glyph::Tag => ("#", "\u{f02b}"),
            Glyph::Hidden => ("◌", "\u{f070}"),
            Glyph::Trash => ("⌫", "\u{f1f8}"),
            Glyph::Search => ("⌕", "\u{f002}"),
            Glyph::Clipboard => ("⎘", "\u{f0ea}"),
            Glyph::Timer => ("⏲", "\u{f023}"),
            Glyph::Kind(kind) => return self.kind(kind),
            Glyph::Tab(n) => match n {
                0 => ("", "\u{f0c9}"),
                1 => ("", "\u{f017}"),
                2 => ("", "\u{f0d0}"),
                _ => ("", "\u{f21e}"),
            },
        };
        if self.nerd { nerd } else { plain }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_both_icons() {
        for (kind, _) in crate::vault::KINDS {
            let plain = Glyphs { nerd: false }.get(Glyph::Kind(kind));
            let nerd = Glyphs { nerd: true }.get(Glyph::Kind(kind));
            assert_ne!(plain, "•", "{kind}");
            assert_ne!(nerd, "\u{f128}", "{kind}");
        }
    }
}
