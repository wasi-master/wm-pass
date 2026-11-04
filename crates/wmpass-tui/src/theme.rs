//! Colours. The default follows the shield, emerald into teal, on a dark ground; terminals
//! without 24-bit colour get the same roles in the sixteen colours every terminal has, and
//! `NO_COLOR` gets none at all.

use ratatui::style::{Color, Modifier, Style};

use crate::glyphs::Glyphs;

/// The roles a screen paints with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Its name, as saved.
    pub name: &'static str,
    /// The ground.
    pub bg: Color,
    /// Panels.
    pub surface: Color,
    /// A raised row or a chip.
    pub raised: Color,
    /// Borders.
    pub line: Color,
    /// Body text.
    pub text: Color,
    /// Secondary text.
    pub mid: Color,
    /// Hints.
    pub low: Color,
    /// The brand colour.
    pub primary: Color,
    /// Its partner, where the shield turns teal.
    pub accent: Color,
    /// Text on the brand colour.
    pub on_primary: Color,
    /// Good.
    pub ok: Color,
    /// Careful.
    pub warn: Color,
    /// Bad.
    pub danger: Color,
    /// Neutral news.
    pub info: Color,
    /// The selected row.
    pub select: Color,
    /// The icon set.
    pub g: Glyphs,
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Emerald into teal, dark.
pub const NIGHT: Theme = Theme {
    name: "night",
    bg: rgb(0x0E1513),
    surface: rgb(0x141D1A),
    raised: rgb(0x1C2724),
    line: rgb(0x2C3A36),
    text: rgb(0xDDE4E1),
    mid: rgb(0xA9B5B1),
    low: rgb(0x6F7D79),
    primary: rgb(0x34D399),
    accent: rgb(0x2DD4BF),
    on_primary: rgb(0x00382F),
    ok: rgb(0x4ADE80),
    warn: rgb(0xFBBF24),
    danger: rgb(0xF87171),
    info: rgb(0x60A5FA),
    select: rgb(0x173B33),
    g: Glyphs { nerd: false },
};

/// The same, light, matching the apps' standard look.
pub const DAY: Theme = Theme {
    name: "day",
    bg: rgb(0xF4FBF8),
    surface: rgb(0xEEF5F2),
    raised: rgb(0xE3EAE7),
    line: rgb(0xBFC9C5),
    text: rgb(0x171D1B),
    mid: rgb(0x3F4947),
    low: rgb(0x6F7977),
    primary: rgb(0x006A60),
    accent: rgb(0x00796B),
    on_primary: rgb(0xFFFFFF),
    ok: rgb(0x146C2E),
    warn: rgb(0x8A5300),
    danger: rgb(0xBA1A1A),
    info: rgb(0x0B5FA5),
    select: rgb(0xCDEAE2),
    g: Glyphs { nerd: false },
};

/// Deep blue-black with violet, for those who like their terminal moody.
pub const DUSK: Theme = Theme {
    name: "dusk",
    bg: rgb(0x11111B),
    surface: rgb(0x181825),
    raised: rgb(0x1E1E2E),
    line: rgb(0x313244),
    text: rgb(0xCDD6F4),
    mid: rgb(0xA6ADC8),
    low: rgb(0x6C7086),
    primary: rgb(0xA6E3A1),
    accent: rgb(0x94E2D5),
    on_primary: rgb(0x11111B),
    ok: rgb(0xA6E3A1),
    warn: rgb(0xF9E2AF),
    danger: rgb(0xF38BA8),
    info: rgb(0x89B4FA),
    select: rgb(0x2A2B3D),
    g: Glyphs { nerd: false },
};

/// The sixteen colours, on whatever ground the terminal has.
pub const ANSI: Theme = Theme {
    name: "ansi",
    bg: Color::Reset,
    surface: Color::Reset,
    raised: Color::Reset,
    line: Color::DarkGray,
    text: Color::Reset,
    mid: Color::Gray,
    low: Color::DarkGray,
    primary: Color::Green,
    accent: Color::Cyan,
    on_primary: Color::Black,
    ok: Color::Green,
    warn: Color::Yellow,
    danger: Color::Red,
    info: Color::Blue,
    select: Color::DarkGray,
    g: Glyphs { nerd: false },
};

/// No colour, for `NO_COLOR` and for screen readers that read styles aloud.
pub const PLAIN: Theme = Theme {
    name: "plain",
    bg: Color::Reset,
    surface: Color::Reset,
    raised: Color::Reset,
    line: Color::Reset,
    text: Color::Reset,
    mid: Color::Reset,
    low: Color::Reset,
    primary: Color::Reset,
    accent: Color::Reset,
    on_primary: Color::Reset,
    ok: Color::Reset,
    warn: Color::Reset,
    danger: Color::Reset,
    info: Color::Reset,
    select: Color::Reset,
    g: Glyphs { nerd: false },
};

/// Every theme, in the order the switcher cycles them.
pub const ALL: [Theme; 5] = [NIGHT, DAY, DUSK, ANSI, PLAIN];

/// Whether this terminal draws 24-bit colour.
fn truecolor() -> bool {
    let colorterm = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();
    colorterm.contains("truecolor")
        || colorterm.contains("24bit")
        || std::env::var_os("WT_SESSION").is_some()
        || matches!(
            std::env::var("TERM_PROGRAM").as_deref(),
            Ok("iTerm.app" | "WezTerm" | "vscode" | "ghostty" | "Hyper" | "rio" | "WarpTerminal")
        )
}

impl Theme {
    /// The theme to start with: the saved one if any, else the best this terminal can show.
    pub fn pick(saved: &str) -> Self {
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return PLAIN;
        }
        if let Some(theme) = ALL.iter().find(|t| t.name == saved) {
            return *theme;
        }
        if truecolor() { NIGHT } else { ANSI }
    }

    /// The next one along.
    pub fn next(self) -> Self {
        let at = ALL.iter().position(|t| t.name == self.name).unwrap_or(0);
        Self { g: self.g, ..ALL[(at + 1) % ALL.len()] }
    }

    /// Whether colours are drawn at all, which decides whether selection needs reverse video.
    pub fn colourless(&self) -> bool {
        self.name == "plain"
    }

    /// The ground.
    pub fn base(&self) -> Style {
        Style::new().bg(self.bg).fg(self.text)
    }

    /// A panel.
    pub fn panel(&self) -> Style {
        Style::new().bg(self.surface).fg(self.text)
    }

    /// A border, brighter when its pane has the keyboard.
    pub fn border(&self, focused: bool) -> Style {
        if focused {
            Style::new().fg(self.primary)
        } else {
            Style::new().fg(self.line)
        }
    }

    /// The selected row.
    pub fn selected(&self, focused: bool) -> Style {
        if self.colourless() || matches!(self.select, Color::DarkGray) && !focused {
            return Style::new().add_modifier(Modifier::REVERSED);
        }
        let style = Style::new().bg(self.select).fg(self.text);
        if focused { style.add_modifier(Modifier::BOLD) } else { style }
    }

    /// Secondary text.
    pub fn dim(&self) -> Style {
        Style::new().fg(self.mid)
    }

    /// Hints.
    pub fn faint(&self) -> Style {
        Style::new().fg(self.low)
    }

    /// The brand colour as text.
    pub fn brand(&self) -> Style {
        Style::new().fg(self.primary).add_modifier(Modifier::BOLD)
    }

    /// A key name in a hint.
    pub fn key(&self) -> Style {
        if self.colourless() {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
        }
    }

    /// A filled button or chip.
    pub fn chip(&self) -> Style {
        if self.colourless() {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().bg(self.primary).fg(self.on_primary).add_modifier(Modifier::BOLD)
        }
    }

    /// The colour of a strength score, 0 to 4.
    pub fn score(&self, score: u8) -> Color {
        match score {
            0 | 1 => self.danger,
            2 => self.warn,
            3 => self.ok,
            _ => self.primary,
        }
    }

    /// One step along the shield's gradient, `t` from 0 to 1. Plain colours have no steps.
    pub fn gradient(&self, t: f32) -> Color {
        match (self.primary, self.accent) {
            (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
                let mix = |a: u8, b: u8| {
                    let t = t.clamp(0.0, 1.0);
                    (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8
                };
                Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
            }
            _ if t < 0.5 => self.primary,
            _ => self.accent,
        }
    }

    /// A colour for an entry, from its colour label or its kind.
    pub fn label(&self, color: Option<&str>, kind: &str) -> Color {
        if self.colourless() {
            return Color::Reset;
        }
        // The apps keep a color as a hue in degrees; the terminal has a handful to show it with.
        if let Some(hue) = color.and_then(|c| c.parse::<u16>().ok()) {
            return match hue % 360 {
                0..=40 | 341..=359 => self.danger,
                41..=100 => self.warn,
                101..=165 => self.ok,
                166..=225 => self.accent,
                226..=260 => self.info,
                _ => Color::Magenta,
            };
        }
        match color.unwrap_or("") {
            "red" => return self.danger,
            "orange" | "yellow" => return self.warn,
            "green" => return self.ok,
            "blue" => return self.info,
            "teal" | "cyan" => return self.accent,
            "purple" | "pink" => return Color::Magenta,
            _ => {}
        }
        match kind {
            "login" => self.primary,
            "card" => self.warn,
            "identity" | "document" => self.info,
            "note" => self.mid,
            "otp" | "recovery" => self.accent,
            "ssh" | "api" | "database" => Color::Magenta,
            _ => self.mid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cycle_visits_every_theme() {
        let mut theme = NIGHT;
        let mut seen = Vec::new();
        for _ in 0..ALL.len() {
            seen.push(theme.name);
            theme = theme.next();
        }
        assert_eq!(theme, NIGHT);
        assert_eq!(seen.len(), ALL.len());
    }

    #[test]
    fn the_gradient_runs_from_one_end_to_the_other() {
        assert_eq!(NIGHT.gradient(0.0), NIGHT.primary);
        assert_eq!(NIGHT.gradient(1.0), NIGHT.accent);
        assert_eq!(ANSI.gradient(0.9), ANSI.accent);
    }
}
