//! Everything drawn. No state changes here except where things were drawn, for the mouse,
//! and the list's scroll position, which ratatui keeps in its state.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::app::{App, COMMANDS, Level, Overlay, Screen, palette_matches};
use crate::door::{self, Door, Field};
use crate::editor::{Editor, Slot, date_text};
use crate::engine::{Code, Secret};
use crate::generator::Knob;
use crate::glyphs::Glyph;
use crate::input::Input;
use crate::theme::Theme;
use crate::vault::{self, HEALTH_GROUPS, Line as DLine, Pane, Tab, VaultUi};

const MASK: &str = "••••••••••••";

/// Draws a frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let theme = app.theme;
    let area = frame.area();
    frame.render_widget(Block::new().style(theme.base()), area);
    if area.width < 40 || area.height < 12 {
        let text = Paragraph::new("Make the window a little bigger").style(theme.dim()).alignment(Alignment::Center);
        frame.render_widget(text, centered(area, area.width, 1));
        return;
    }
    let lock_in = app.lock_in();
    let clip_left = app.clip.seconds_left();
    match &mut app.screen {
        Screen::Door(door) => draw_door(frame, area, door, &theme, &app.prefs.recent),
        Screen::Vault(ui) => draw_vault(frame, area, ui, &theme, lock_in, clip_left, app.toast.as_ref()),
    }
    if let Screen::Door(_) = app.screen
        && let Some(toast) = &app.toast
    {
        let bottom = Rect { y: area.bottom() - 1, height: 1, ..area };
        frame.render_widget(Paragraph::new(toast_line(toast, &theme)).alignment(Alignment::Center), bottom);
    }
    if let Some(overlay) = &app.overlay {
        let ui = match &app.screen {
            Screen::Vault(ui) => Some(&**ui),
            Screen::Door(_) => None,
        };
        draw_overlay(frame, area, overlay, &theme, ui);
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn panel<'a>(theme: &Theme, title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border(focused))
        .title(title)
        .style(theme.panel())
}

fn key_hint<'a>(theme: &Theme, keys: &[(&'a str, &'a str)]) -> Line<'a> {
    let mut spans = Vec::new();
    for (n, (key, what)) in keys.iter().enumerate() {
        if n > 0 {
            spans.push(Span::styled("  ", theme.faint()));
        }
        spans.push(Span::styled(*key, theme.key()));
        spans.push(Span::styled(format!(" {what}"), theme.faint()));
    }
    Line::from(spans)
}

/// A bar of `width` cells, `ratio` of it filled.
fn bar<'a>(width: u16, ratio: f64, filled: Color, theme: &Theme) -> Vec<Span<'a>> {
    let width = usize::from(width);
    let full = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    vec![
        Span::styled("━".repeat(full), Style::new().fg(filled)),
        Span::styled("─".repeat(width - full), Style::new().fg(theme.line)),
    ]
}

/// A strength meter of five segments.
fn meter<'a>(score: u8, theme: &Theme) -> Vec<Span<'a>> {
    let colour = theme.score(score);
    (0..5u8)
        .map(|n| {
            let on = n <= score;
            Span::styled(if n == 4 { "▰" } else { "▰ " }, Style::new().fg(if on { colour } else { theme.line }))
        })
        .collect()
}

fn score_word(score: u8) -> &'static str {
    match score {
        0 => "Very weak",
        1 => "Weak",
        2 => "Fair",
        3 => "Strong",
        _ => "Very strong",
    }
}

fn toast_line<'a>(toast: &crate::app::Toast, theme: &Theme) -> Line<'a> {
    let (icon, colour) = match toast.level {
        Level::Info => ("●", theme.info),
        Level::Good => ("✓", theme.ok),
        Level::Warn => ("!", theme.warn),
        Level::Bad => ("✕", theme.danger),
    };
    Line::from(vec![
        Span::styled(format!(" {icon} "), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
        Span::styled(toast.text.clone(), Style::new().fg(theme.text)),
    ])
}

fn shield(theme: &Theme) -> Vec<Line<'static>> {
    const ART: [&str; 6] = ["▗▄▄▄▄▄▄▄▄▄▄▖", "▐██████████▌", "▐████▀▀████▌", "▐████  ████▌", " ▜████████▛ ", "   ▀▜██▛▀   "];
    ART.iter()
        .enumerate()
        .map(|(n, row)| {
            let t = n as f32 / (ART.len() - 1) as f32;
            Line::from(Span::styled(*row, Style::new().fg(theme.gradient(t)))).centered()
        })
        .collect()
}

fn brand_word(theme: &Theme) -> Line<'static> {
    let word = "W M   P A S S";
    let len = word.chars().count().max(2) - 1;
    let spans: Vec<Span> = word
        .chars()
        .enumerate()
        .map(|(n, ch)| Span::styled(ch.to_string(), Style::new().fg(theme.gradient(n as f32 / len as f32)).add_modifier(Modifier::BOLD)))
        .collect();
    Line::from(spans).centered()
}

/// How a text box looks.
struct BoxLook<'a> {
    label: &'a str,
    secret: bool,
    focused: bool,
    placeholder: &'a str,
}

/// Draws a text box; returns where the cursor goes.
fn text_box(frame: &mut Frame, area: Rect, theme: &Theme, input: &Input, look: &BoxLook<'_>) -> Option<Position> {
    let BoxLook { label, secret, focused, placeholder } = *look;
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.border(focused))
        .title(Span::styled(format!(" {label} "), if focused { theme.brand() } else { theme.dim() }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let shown: String = if secret { "•".repeat(input.text().chars().count()) } else { input.text().to_owned() };
    let width = usize::from(inner.width.saturating_sub(1));
    let (line, col) = input.cursor_line_col();
    let skip = col.saturating_sub(width);
    let text = if input.is_empty() {
        Line::from(Span::styled(placeholder.to_owned(), theme.faint()))
    } else {
        let row = shown.lines().nth(line).unwrap_or("").chars().skip(skip).collect::<String>();
        Line::from(Span::styled(row, Style::new().fg(theme.text)))
    };
    frame.render_widget(Paragraph::new(text), inner);
    focused.then(|| Position::new(inner.x + (col - skip) as u16, inner.y))
}

fn draw_door(frame: &mut Frame, area: Rect, door: &Door, theme: &Theme, recent: &[std::path::PathBuf]) {
    let fields = door.fields();
    let extra = if door.mode == door::Mode::Create { 2 } else { 0 };
    let height = (8 + 3 * fields.len() as u16 + 6 + extra).min(area.height);
    let card = centered(area, 64, height);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.line))
        .style(theme.panel());
    let inner = block.inner(card).inner(Margin::new(2, 0));
    frame.render_widget(block, card);

    let mut y = inner.y;
    let logo = shield(theme);
    let logo_h = (logo.len() as u16).min(inner.height);
    frame.render_widget(Paragraph::new(logo), Rect { y, height: logo_h, ..inner });
    y += logo_h;
    frame.render_widget(Paragraph::new(brand_word(theme)), Rect { y, height: 1, ..inner });
    y += 1;
    let subtitle = match door.mode {
        door::Mode::Unlock => {
            let name = std::path::Path::new(door.path.text())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "your vault".into());
            format!("Unlock {name}")
        }
        door::Mode::Create => "A new vault, on this device only".to_owned(),
    };
    frame.render_widget(Paragraph::new(Line::from(Span::styled(subtitle, theme.dim())).centered()), Rect { y, height: 1, ..inner });
    y += 2;

    let mut cursor = None;
    for field in &fields {
        if y + 3 > inner.bottom() {
            break;
        }
        let rect = Rect { y, height: 3, ..inner };
        let focused = door.focus == *field && door.recent.is_none() && door.busy.is_none();
        let (label, input, secret, hint) = match field {
            Field::Path => ("Vault file", &door.path, false, door::default_new_path().to_string_lossy().into_owned()),
            Field::Name => ("Name", &door.name, false, "Personal".to_owned()),
            Field::Password => ("Password", &door.password, true, String::new()),
            Field::Confirm => ("Password again", &door.confirm, true, String::new()),
            Field::KeyFile => ("Key file", &door.key_file, false, "Path to the key file".to_owned()),
        };
        if let Some(at) = text_box(frame, rect, theme, input, &BoxLook { label, secret, focused, placeholder: &hint }) {
            cursor = Some(at);
        }
        y += 3;
        if *field == Field::Password
            && door.mode == door::Mode::Create
            && let Some((score, time)) = &door.strength
        {
            let mut spans = meter(*score, theme);
            spans.push(Span::styled(format!("  {}", score_word(*score)), Style::new().fg(theme.score(*score))));
            spans.push(Span::styled(format!(" · {time} to guess"), theme.faint()));
            frame.render_widget(Paragraph::new(Line::from(spans)), Rect { y, height: 1, x: inner.x + 1, width: inner.width - 1 });
            y += 1;
        }
    }
    y += 1;
    let status: Line = if let Some(busy) = door.busy {
        Line::from(Span::styled(format!("◐ {busy}"), theme.brand())).centered()
    } else if let Some(error) = &door.error {
        Line::from(Span::styled(format!("✕ {error}"), Style::new().fg(theme.danger))).centered()
    } else if let Some(notice) = &door.notice {
        Line::from(Span::styled(notice.clone(), Style::new().fg(theme.info))).centered()
    } else {
        Line::default()
    };
    if y < inner.bottom() {
        frame.render_widget(Paragraph::new(status).wrap(Wrap { trim: true }), Rect { y, height: 2.min(inner.bottom() - y), ..inner });
    }
    let hints = match door.mode {
        door::Mode::Unlock => {
            let mut keys = vec![("Enter", "unlock"), ("Tab", "next"), ("^N", "new vault")];
            if !recent.is_empty() {
                keys.push(("^R", "recent"));
            }
            keys.push(("^F", "key file"));
            keys.push(("Esc", "quit"));
            keys
        }
        door::Mode::Create => vec![("Enter", "next"), ("Tab", "move"), ("Esc", "back")],
    };
    let below = Rect { y: card.bottom().min(area.bottom() - 1), height: 1, ..area };
    if below.y < area.bottom() {
        frame.render_widget(Paragraph::new(key_hint(theme, &hints).centered()), below);
    }
    if let Some(at) = door.recent {
        let height = (recent.len() as u16 + 2).min(area.height - 2);
        let popup = centered(area, 70, height);
        frame.render_widget(Clear, popup);
        let items: Vec<ListItem> = recent
            .iter()
            .map(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(format!("◆ {name}"), Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("  {dir}"), theme.faint()),
                ]))
            })
            .collect();
        let mut state = ratatui::widgets::ListState::default().with_selected(Some(at));
        let list = List::new(items)
            .block(panel(theme, " Recent vaults ", true))
            .highlight_style(theme.selected(true))
            .highlight_symbol("▌");
        frame.render_stateful_widget(list, popup, &mut state);
    } else if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
}

fn draw_vault(
    frame: &mut Frame,
    area: Rect,
    ui: &mut VaultUi,
    theme: &Theme,
    lock_in: Option<std::time::Duration>,
    clip_left: Option<u64>,
    toast: Option<&crate::app::Toast>,
) {
    let [top, body, bottom] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(area);
    draw_top(frame, top, ui, theme, lock_in, clip_left);
    let cursor = match ui.tab {
        Tab::Items => draw_items(frame, body, ui, theme),
        Tab::Codes => draw_codes(frame, body, ui, theme),
        Tab::Generator => {
            draw_generator(frame, body, ui, theme);
            None
        }
        Tab::Health => {
            draw_health(frame, body, ui, theme);
            None
        }
    };
    draw_bottom(frame, bottom, ui, theme, toast);
    if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
}

fn draw_top(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme, lock_in: Option<std::time::Duration>, clip_left: Option<u64>) {
    frame.render_widget(Block::new().style(Style::new().bg(theme.surface)), area);
    let mut x = area.x + 1;
    let brand = Line::from(vec![
        Span::styled(theme.g.get(Glyph::Brand), Style::new().fg(theme.primary)),
        Span::styled(" WM", Style::new().fg(theme.gradient(0.2)).add_modifier(Modifier::BOLD)),
        Span::styled(" Pass", Style::new().fg(theme.gradient(0.9)).add_modifier(Modifier::BOLD)),
    ]);
    let width = brand.width() as u16;
    frame.render_widget(Paragraph::new(brand), Rect { x, width, ..area });
    x += width + 3;
    for (n, tab) in Tab::ALL.iter().enumerate() {
        let icon = theme.g.get(Glyph::Tab(n));
        let label = if icon.is_empty() { format!(" {} {} ", n + 1, tab.name()) } else { format!(" {} {icon} {} ", n + 1, tab.name()) };
        let w = label.chars().count() as u16;
        if x + w > area.right() {
            break;
        }
        let rect = Rect { x, width: w, ..area };
        let style = if *tab == ui.tab { theme.chip() } else { theme.dim() };
        frame.render_widget(Paragraph::new(Span::styled(label, style)), rect);
        ui.areas.tabs[n] = rect;
        x += w + 1;
    }
    let mut right = vec![Span::styled(ui.engine.name.clone(), Style::new().fg(theme.text).add_modifier(Modifier::BOLD))];
    if ui.engine.read_only {
        right.push(Span::styled(" read-only", Style::new().fg(theme.warn)));
    }
    if ui.shelf_open {
        right.push(Span::styled(format!("  {} shelf open", theme.g.get(Glyph::Hidden)), Style::new().fg(theme.warn)));
    }
    if let Some(left) = clip_left {
        right.push(Span::styled(format!("  {} {left}s", theme.g.get(Glyph::Clipboard)), Style::new().fg(theme.accent)));
    }
    if let Some(left) = lock_in {
        let secs = left.as_secs();
        let colour = if secs < 60 { theme.warn } else { theme.low };
        right.push(Span::styled(format!("  {} {}:{:02}", theme.g.get(Glyph::Timer), secs / 60, secs % 60), Style::new().fg(colour)));
    }
    right.push(Span::raw(" "));
    let line = Line::from(right).right_aligned();
    let width = (line.width() as u16).min(area.width.saturating_sub(x - area.x));
    frame.render_widget(Paragraph::new(line), Rect { x: area.right() - width, width, ..area });
}

fn draw_bottom(frame: &mut Frame, area: Rect, ui: &VaultUi, theme: &Theme, toast: Option<&crate::app::Toast>) {
    frame.render_widget(Block::new().style(Style::new().bg(theme.surface)), area);
    let hints: Vec<(&str, &str)> = match ui.tab {
        Tab::Items if ui.searching => vec![("Enter", "done"), ("Esc", "clear")],
        Tab::Items if ui.in_trash() => vec![("u", "restore"), ("D", "delete for good"), ("?", "keys")],
        Tab::Items => match ui.pane {
            Pane::Detail => vec![("r", "show"), ("y", "copy line"), ("e", "edit"), (":", "commands"), ("?", "keys")],
            _ => vec![("/", "search"), ("c", "copy"), ("u", "user"), ("n", "new"), (":", "commands"), ("?", "keys")],
        },
        Tab::Codes => vec![("Enter", "copy"), ("/", "filter"), ("o", "open item"), ("?", "keys")],
        Tab::Generator => vec![("←→", "change"), ("Enter", "again"), ("c", "copy"), ("?", "keys")],
        Tab::Health => vec![("←→", "group"), ("Enter", "open"), ("r", "check again"), ("?", "keys")],
    };
    let hint = key_hint(theme, &hints).right_aligned();
    let hint_w = (hint.width() as u16 + 1).min(area.width);
    let left = match toast {
        Some(toast) => toast_line(toast, theme),
        None => {
            let shown = ui.rows.len();
            let text = if ui.tab == Tab::Items {
                if shown == ui.total { format!(" {shown} items") } else { format!(" {shown} of {} items", ui.total) }
            } else {
                String::new()
            };
            Line::from(Span::styled(text, theme.faint()))
        }
    };
    frame.render_widget(Paragraph::new(left), Rect { width: area.width.saturating_sub(hint_w), ..area });
    frame.render_widget(Paragraph::new(hint), Rect { x: area.right() - hint_w, width: hint_w - 1, ..area });
}

fn draw_items(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) -> Option<Position> {
    let narrow = area.width < 76;
    let show_sidebar = area.width >= 110 || ui.pane == Pane::Sidebar;
    let mut constraints = Vec::new();
    if show_sidebar {
        constraints.push(Constraint::Length(26));
    }
    if narrow {
        constraints.push(Constraint::Min(0));
    } else {
        constraints.push(Constraint::Percentage(42));
        constraints.push(Constraint::Min(30));
    }
    let parts = Layout::horizontal(constraints).split(area);
    let mut at = 0;
    ui.areas.sidebar = Rect::default();
    ui.areas.list = Rect::default();
    ui.areas.detail = Rect::default();
    if show_sidebar {
        draw_sidebar(frame, parts[0], ui, theme);
        at = 1;
    }
    if narrow {
        if ui.pane == Pane::Detail {
            draw_detail(frame, parts[at], ui, theme);
            None
        } else if !show_sidebar || at < parts.len() {
            draw_list(frame, parts[at], ui, theme)
        } else {
            None
        }
    } else {
        let cursor = draw_list(frame, parts[at], ui, theme);
        draw_detail(frame, parts[at + 1], ui, theme);
        cursor
    }
}

fn draw_sidebar(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    ui.areas.sidebar = area;
    let focused = ui.pane == Pane::Sidebar;
    let block = panel(theme, Line::from(Span::styled(" Vault ", theme.dim())), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = usize::from(inner.width);
    let lines: Vec<Line> = ui
        .sections
        .iter()
        .enumerate()
        .map(|(n, s)| {
            if s.heading {
                return Line::from(Span::styled(format!(" {}", s.label.to_uppercase()), theme.faint().add_modifier(Modifier::BOLD)));
            }
            let picked = n == ui.section;
            let count = s.count.to_string();
            let label: String = s.label.chars().take(width.saturating_sub(count.len() + 5)).collect();
            let pad = width.saturating_sub(label.chars().count() + count.len() + 5);
            let base = if picked { theme.selected(focused) } else { Style::new() };
            let icon_style = if picked { base.fg(theme.primary) } else { Style::new().fg(theme.low) };
            Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                Span::styled(format!("{} ", theme.g.get(s.icon)), icon_style),
                Span::styled(format!("{label}{}", " ".repeat(pad)), if picked { base } else { Style::new().fg(theme.mid) }),
                Span::styled(format!(" {count} "), if picked { base.fg(theme.accent) } else { theme.faint() }),
            ])
        })
        .collect();
    let scroll = ui.section.saturating_sub(usize::from(inner.height.saturating_sub(2))) as u16;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

fn draw_list(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) -> Option<Position> {
    let focused = ui.pane == Pane::List || ui.searching;
    let section = ui.sections.get(ui.section).map(|s| s.label.clone()).unwrap_or_default();
    let block = panel(theme, Line::from(vec![Span::styled(format!(" {section} "), theme.brand())]), focused)
        .title_bottom(Line::from(Span::styled(format!(" {} ", ui.sort.name()), theme.faint())).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [search, list_area] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    ui.areas.list = Rect { y: list_area.y - 1, height: list_area.height + 1, ..list_area };

    // The search box, part of the list.
    let mut cursor = None;
    let prompt = Span::styled(format!(" {} ", theme.g.get(Glyph::Search)), if ui.searching { theme.brand() } else { theme.faint() });
    let typed = if ui.search.is_empty() && !ui.searching {
        Span::styled("Search   tag:work  has:otp  weak  kind:card", theme.faint())
    } else {
        Span::styled(ui.search.text().to_owned(), Style::new().fg(theme.text))
    };
    frame.render_widget(Paragraph::new(Line::from(vec![prompt, typed])), Rect { height: 1, ..search });
    let rule = "─".repeat(usize::from(search.width));
    frame.render_widget(Paragraph::new(Span::styled(rule, Style::new().fg(theme.line))), Rect { y: search.y + 1, height: 1, ..search });
    if ui.searching {
        cursor = Some(Position::new(search.x + 3 + ui.search.cursor() as u16, search.y));
    }

    if ui.rows.is_empty() {
        let text = if ui.total == 0 {
            vec![
                Line::default(),
                Line::from(Span::styled("Nothing here yet", theme.dim())).centered(),
                Line::default(),
                key_hint(theme, &[("n", "new item"), (":", "import")]).centered(),
            ]
        } else {
            vec![Line::default(), Line::from(Span::styled("No matches", theme.dim())).centered()]
        };
        frame.render_widget(Paragraph::new(text), list_area);
        return cursor;
    }
    let width = usize::from(list_area.width.saturating_sub(2));
    let items: Vec<ListItem> = ui
        .rows
        .iter()
        .map(|row| {
            let colour = theme.label(row.color.as_deref(), &row.kind);
            let mut badges = String::new();
            if row.pinned {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Pinned));
            }
            if row.favourite {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Favourite));
            }
            if row.has_otp {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Codes));
            }
            if row.has_passkey {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Passkey));
            }
            if row.attachments > 0 {
                badges.push(' ');
                badges.push_str(theme.g.get(Glyph::Attachment));
            }
            let badge_w = badges.chars().count();
            let title_w = width.saturating_sub(badge_w + 3);
            let mut title: String = row.title.chars().take(title_w).collect();
            if row.title.chars().count() > title_w && title_w > 1 {
                title.pop();
                title.push('…');
            }
            let pad = width.saturating_sub(title.chars().count() + badge_w + 2);
            let first = Line::from(vec![
                Span::styled(format!("{} ", theme.g.kind(&row.kind)), Style::new().fg(colour)),
                Span::styled(title, Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::raw(" ".repeat(pad)),
                Span::styled(badges, Style::new().fg(theme.warn)),
            ]);
            let mut sub = row.subtitle.clone();
            if let Some(site) = &row.site
                && !sub.contains(site.as_str())
            {
                if !sub.is_empty() {
                    sub.push_str(" · ");
                }
                sub.push_str(site);
            }
            if !row.tags.is_empty() {
                let tags: Vec<String> = row.tags.iter().map(|t| format!("#{t}")).collect();
                sub.push_str("  ");
                sub.push_str(&tags.join(" "));
            }
            let sub: String = sub.chars().take(width.saturating_sub(2)).collect();
            let second = Line::from(Span::styled(format!("  {sub}"), theme.faint()));
            ListItem::new(Text::from(vec![first, second]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(theme.selected(focused))
        .highlight_symbol(Span::styled("▌", Style::new().fg(theme.primary)))
        .repeat_highlight_symbol(true);
    frame.render_stateful_widget(list, list_area, &mut ui.list);
    if ui.rows.len() * 2 > usize::from(list_area.height) {
        let mut state = ScrollbarState::new(ui.rows.len()).position(ui.list.selected().unwrap_or(0));
        let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(" "))
            .thumb_style(Style::new().fg(theme.line));
        frame.render_stateful_widget(bar, Rect { x: area.right() - 1, ..list_area }, &mut state);
    }
    cursor
}

fn chip<'a>(text: String, fg: Color, theme: &Theme) -> Span<'a> {
    if theme.colourless() {
        Span::styled(format!("[{text}]"), Style::new())
    } else {
        Span::styled(format!(" {text} "), Style::new().fg(fg).bg(theme.raised))
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn spaced_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    match chars.len() {
        6 => format!("{} {}", chars[..3].iter().collect::<String>(), chars[3..].iter().collect::<String>()),
        8 => format!("{} {}", chars[..4].iter().collect::<String>(), chars[4..].iter().collect::<String>()),
        _ => code.to_owned(),
    }
}

fn code_lines<'a>(code: &Code, theme: &Theme, width: u16) -> Vec<Line<'a>> {
    let period = code.otp.period.unwrap_or(30).max(1);
    let left = code.seconds_left.unwrap_or(0);
    let colour = if left <= 5 { theme.danger } else if left <= 10 { theme.warn } else { theme.primary };
    let mut first = vec![match &code.code {
        Some(value) => Span::styled(spaced_code(value.as_str()), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
        None => Span::styled("press Enter for the next code", theme.faint()),
    }];
    if code.code.is_some() {
        first.push(Span::styled(format!("   {left}s"), theme.faint()));
    }
    if let Some(next) = &code.next_code {
        first.push(Span::styled(format!("   next {}", spaced_code(next.as_str())), theme.faint()));
    }
    let mut out = vec![Line::from(first)];
    if code.code.is_some() {
        out.push(Line::from(bar(width, f64::from(left) / f64::from(period), colour, theme)));
    }
    out
}

fn draw_detail(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    ui.areas.detail = area;
    let focused = ui.pane == Pane::Detail;
    let Some(detail) = &ui.detail else {
        let block = panel(theme, "", focused);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let text = vec![
            Line::default(),
            Line::from(Span::styled("Pick an item to see it here", theme.faint())).centered(),
        ];
        frame.render_widget(Paragraph::new(text), inner);
        return;
    };
    let row = &detail.row;
    let colour = theme.label(row.color.as_deref(), &row.kind);
    let block = panel(theme, Line::from(Span::styled(format!(" {} ", vault::kind_name(&row.kind)), Style::new().fg(colour))), focused);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);

    // Heading: icon, title, flags; then site and tags.
    let mut head = vec![
        Span::styled(format!("{} ", theme.g.kind(&row.kind)), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
        Span::styled(row.title.clone(), Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
    ];
    if row.favourite {
        head.push(Span::styled(format!("  {}", theme.g.get(Glyph::Favourite)), Style::new().fg(theme.warn)));
    }
    if row.pinned {
        head.push(Span::styled(format!("  {} pinned", theme.g.get(Glyph::Pinned)), theme.faint()));
    }
    if row.trashed {
        head.push(Span::styled("  in the trash", Style::new().fg(theme.danger)));
    }
    let mut lines: Vec<Line> = vec![Line::from(head)];
    let mut chips = Vec::new();
    if let Some(folder) = row.folder.as_ref().and_then(|id| ui.folders.iter().find(|f| f.id == *id)) {
        chips.push(chip(format!("▸ {}", folder.name), theme.mid, theme));
        chips.push(Span::raw(" "));
    }
    for tag in &row.tags {
        chips.push(chip(format!("#{tag}"), theme.accent, theme));
        chips.push(Span::raw(" "));
    }
    if !chips.is_empty() {
        lines.push(Line::from(chips));
    }
    lines.push(Line::from(Span::styled("─".repeat(usize::from(inner.width)), Style::new().fg(theme.line))));
    let head_rows = lines.len();

    let label_w = 16usize;
    let mut starts = Vec::new();
    for (n, line) in ui.lines.iter().enumerate() {
        starts.push(lines.len());
        let picked = focused && n == ui.line;
        let base = if picked { theme.selected(true) } else { Style::new() };
        let marker = Span::styled(if picked { "› " } else { "  " }, base.fg(theme.primary));
        let pad_label = |label: &str| -> String {
            let short: String = label.chars().take(label_w - 1).collect();
            format!("{short:<label_w$}")
        };
        match line {
            DLine::Heading(title) => {
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(title.to_uppercase(), theme.faint().add_modifier(Modifier::BOLD))));
            }
            DLine::Value { label, text, secret, has_value } => {
                let revealed = secret.as_ref().and_then(|s| ui.revealed_text(s));
                let label_span = Span::styled(pad_label(label), base.fg(theme.mid));
                let value_style = base.fg(theme.text);
                let (value, hint) = match (secret, revealed, text) {
                    (Some(_), Some(shown), _) => (Some(shown.to_owned()), Some("r hide")),
                    (Some(_), None, _) if *has_value => (None, Some("r show · y copy")),
                    (Some(_), None, _) => (Some(String::new()), None),
                    (None, _, Some(text)) => (Some(text.clone()), None),
                    (None, _, None) => (Some(String::new()), None),
                };
                match value {
                    None => {
                        let mut spans = vec![marker, label_span, Span::styled(MASK, base.fg(theme.low))];
                        if picked && let Some(hint) = hint {
                            spans.push(Span::styled(format!("   {hint}"), base.fg(theme.low)));
                        }
                        lines.push(Line::from(spans));
                    }
                    Some(value) => {
                        let revealed_style = if revealed.is_some() { base.fg(theme.accent).add_modifier(Modifier::BOLD) } else { value_style };
                        let mut first = true;
                        let mut rows: Vec<&str> = value.lines().collect();
                        if rows.is_empty() {
                            rows.push("");
                        }
                        for part in rows {
                            let (m, l) = if first { (marker.clone(), label_span.clone()) } else { (Span::styled("  ", base), Span::styled(" ".repeat(label_w), base)) };
                            let mut spans = vec![m, l, Span::styled(part.to_owned(), revealed_style)];
                            if first && picked && let Some(hint) = hint {
                                spans.push(Span::styled(format!("   {hint}"), base.fg(theme.low)));
                            }
                            lines.push(Line::from(spans));
                            first = false;
                        }
                    }
                }
            }
            DLine::Code => {
                let label_span = Span::styled(pad_label("One-time code"), base.fg(theme.mid));
                match &ui.code {
                    Some(code) => {
                        let width = inner.width.saturating_sub(label_w as u16 + 4).min(30);
                        for (k, mut l) in code_lines(code, theme, width).into_iter().enumerate() {
                            let prefix = if k == 0 { vec![marker.clone(), label_span.clone()] } else { vec![Span::raw("  "), Span::raw(" ".repeat(label_w))] };
                            let mut spans = prefix;
                            spans.append(&mut l.spans);
                            lines.push(Line::from(spans).style(if k == 0 { base } else { Style::new() }));
                        }
                    }
                    None => lines.push(Line::from(vec![marker, label_span, Span::styled("…", theme.faint())])),
                }
            }
            DLine::File(file) => lines.push(Line::from(vec![
                marker,
                Span::styled(format!("{} ", theme.g.get(Glyph::Attachment)), base.fg(theme.accent)),
                Span::styled(file.name.clone(), base.fg(theme.text)),
                Span::styled(format!("  {}", human_size(file.size)), base.fg(theme.low)),
                Span::styled(if picked { "   o save" } else { "" }, base.fg(theme.low)),
            ])),
            DLine::Past { index, field, at_ms } => {
                let shown = ui.revealed_text(&Secret::History(*index));
                lines.push(Line::from(vec![
                    marker,
                    Span::styled(pad_label(&format!("{field} {}", date_text(*at_ms))), base.fg(theme.mid)),
                    match shown {
                        Some(v) => Span::styled(v.to_owned(), base.fg(theme.accent)),
                        None => Span::styled(MASK, base.fg(theme.low)),
                    },
                ]));
            }
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!("Made {} · changed {}", date_text(detail.created_ms), date_text(row.modified_ms)),
        theme.faint(),
    )));

    // Keep the picked line in sight.
    let height = usize::from(inner.height);
    let at = starts.get(ui.line).copied().unwrap_or(0);
    let scroll = (at + 2).saturating_sub(height);
    let scroll = if scroll > 0 { scroll.max(head_rows.min(scroll)) } else { 0 };
    frame.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
}

fn draw_codes(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) -> Option<Position> {
    let block = panel(theme, Line::from(Span::styled(" One-time codes ", theme.brand())), true);
    let inner = block.inner(area).inner(Margin::new(1, 0));
    frame.render_widget(block, area);
    let [filter, grid] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    let typed = if ui.code_filter.is_empty() && !ui.filtering_codes {
        Span::styled("Filter by name", theme.faint())
    } else {
        Span::styled(ui.code_filter.text().to_owned(), Style::new().fg(theme.text))
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(format!("{} ", theme.g.get(Glyph::Search)), if ui.filtering_codes { theme.brand() } else { theme.faint() }), typed])),
        Rect { height: 1, ..filter },
    );
    ui.areas.codes = grid;
    if ui.codes.is_empty() {
        let text = vec![
            Line::default(),
            Line::from(Span::styled("No one-time codes yet", theme.dim())).centered(),
            Line::from(Span::styled("Edit a login and paste its otpauth:// link or seed", theme.faint())).centered(),
        ];
        frame.render_widget(Paragraph::new(text), grid);
        return ui.filtering_codes.then(|| Position::new(filter.x + 2 + ui.code_filter.cursor() as u16, filter.y));
    }
    let card_w = 36u16;
    let cols = (grid.width / card_w).max(1);
    ui.areas.code_cols = cols;
    let card_w = grid.width / cols;
    let card_h = 5u16;
    let rows_fit = (grid.height / card_h).max(1);
    let row_of = (ui.code_at / usize::from(cols)) as u16;
    let first_row = row_of.saturating_sub(rows_fit - 1);
    for (n, code) in ui.codes.iter().enumerate() {
        let (r, c) = ((n / usize::from(cols)) as u16, (n % usize::from(cols)) as u16);
        if r < first_row || r >= first_row + rows_fit {
            continue;
        }
        let rect = Rect { x: grid.x + c * card_w, y: grid.y + (r - first_row) * card_h, width: card_w.saturating_sub(1), height: card_h };
        let picked = n == ui.code_at;
        let title = Line::from(vec![
            Span::styled(format!(" {} ", theme.g.get(Glyph::Codes)), Style::new().fg(theme.accent)),
            Span::styled(format!("{} ", code.title), Style::new().fg(theme.text).add_modifier(Modifier::BOLD)),
        ]);
        let mut block = Block::bordered()
            .border_type(if picked { BorderType::Thick } else { BorderType::Rounded })
            .border_style(theme.border(picked))
            .title(title);
        if picked {
            block = block.title_bottom(Line::from(Span::styled(" Enter copies ", theme.faint())).right_aligned());
        }
        let body = block.inner(rect).inner(Margin::new(1, 0));
        frame.render_widget(block, rect);
        let mut lines = code_lines(code, theme, body.width);
        let who = [code.otp.account.as_str(), code.site.as_deref().unwrap_or("")]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        lines.push(Line::from(Span::styled(who, theme.faint())));
        frame.render_widget(Paragraph::new(lines), body);
    }
    ui.filtering_codes.then(|| Position::new(filter.x + 2 + ui.code_filter.cursor() as u16, filter.y))
}

fn coloured_value<'a>(value: &str, theme: &Theme) -> Line<'a> {
    let spans: Vec<Span> = value
        .chars()
        .map(|ch| {
            let colour = if theme.colourless() {
                Color::Reset
            } else if ch.is_ascii_digit() {
                theme.info
            } else if ch.is_alphabetic() {
                theme.text
            } else if ch == ' ' || ch == '-' {
                theme.low
            } else {
                theme.warn
            };
            Span::styled(ch.to_string(), Style::new().fg(colour).add_modifier(Modifier::BOLD))
        })
        .collect();
    Line::from(spans)
}

fn draw_generator(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    let [left, right] = Layout::horizontal([Constraint::Length(34), Constraint::Min(30)]).areas(area);
    let generator = &ui.generator;
    let block = panel(theme, Line::from(Span::styled(" Settings ", theme.brand())), true);
    let inner = block.inner(left).inner(Margin::new(1, 1));
    frame.render_widget(block, left);
    let knobs = generator.knobs();
    let lines: Vec<Line> = knobs
        .iter()
        .enumerate()
        .map(|(n, knob)| {
            let (label, value) = generator.describe(*knob);
            let picked = n == generator.at;
            let base = if picked { theme.selected(true) } else { Style::new() };
            let toggle = matches!(knob, Knob::Lower | Knob::Upper | Knob::Digits | Knob::Symbols | Knob::Ambiguous | Knob::Capitalise | Knob::WordDigit);
            let shown = if toggle {
                if value == "on" { Span::styled("● on ", base.fg(theme.primary)) } else { Span::styled("○ off", base.fg(theme.low)) }
            } else {
                Span::styled(format!("‹ {value} ›"), base.fg(if picked { theme.accent } else { theme.text }))
            };
            let width = usize::from(inner.width).saturating_sub(label.chars().count() + shown.width() + 2);
            Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                Span::styled(format!("{label}{}", " ".repeat(width)), base.fg(theme.mid)),
                shown,
                Span::styled(" ", base),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);

    let block = panel(theme, Line::from(Span::styled(format!(" {} ", generator.mode.name()), theme.brand())), false);
    let inner = block.inner(right).inner(Margin::new(2, 1));
    frame.render_widget(block, right);
    let mut lines = vec![Line::default()];
    let value = generator.value.as_str();
    let width = usize::from(inner.width.max(1));
    let chars: Vec<char> = value.chars().collect();
    for chunk in chars.chunks(width) {
        let text: String = chunk.iter().collect();
        lines.push(coloured_value(&text, theme).centered());
    }
    lines.push(Line::default());
    let bits = generator.bits;
    let bar_w = inner.width.saturating_sub(18).min(40);
    let mut entropy = vec![Span::styled(format!("{bits:>5.0} bits  "), theme.dim())];
    let bits_colour = if bits < 50.0 { theme.danger } else if bits < 80.0 { theme.warn } else { theme.primary };
    entropy.extend(bar(bar_w, bits / 128.0, bits_colour, theme));
    lines.push(Line::from(entropy).centered());
    if let Some(strength) = &generator.strength {
        let mut spans = meter(strength.score, theme);
        spans.push(Span::styled(format!("  {}", score_word(strength.score)), Style::new().fg(theme.score(strength.score))));
        lines.push(Line::from(spans).centered());
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("Offline, slow hash  ", theme.faint()),
            Span::styled(strength.crack_slow_hash.clone(), Style::new().fg(theme.text)),
        ]).centered());
        lines.push(Line::from(vec![
            Span::styled("Offline, fast hash  ", theme.faint()),
            Span::styled(strength.crack_fast_hash.clone(), Style::new().fg(theme.text)),
        ]).centered());
        if let Some(warning) = &strength.warning {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(warning.clone(), Style::new().fg(theme.warn))).centered());
        }
    }
    lines.push(Line::default());
    lines.push(key_hint(theme, &[("Enter", "make another"), ("c", "copy"), ("Shift ←→", "faster")]).centered());
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_health(frame: &mut Frame, area: Rect, ui: &mut VaultUi, theme: &Theme) {
    ui.areas.health = area;
    let Some(health) = &ui.health else {
        frame.render_widget(Paragraph::new("Checking…").style(theme.dim()), area);
        return;
    };
    let [top, rest] = Layout::vertical([Constraint::Length(5), Constraint::Min(0)]).areas(area);
    let block = panel(theme, Line::from(Span::styled(" Health ", theme.brand())), false);
    let inner = block.inner(top).inner(Margin::new(2, 0));
    frame.render_widget(block, top);
    let score = health.score;
    let colour = match score {
        0..50 => theme.danger,
        50..80 => theme.warn,
        _ => theme.primary,
    };
    let verdict = match score {
        0..50 => "Your vault needs work",
        50..80 => "Your vault could be better",
        90.. => "Your vault is in great shape",
        _ => "Your vault is in good shape",
    };
    let bar_w = inner.width.saturating_sub(4);
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{score}"), Style::new().fg(colour).add_modifier(Modifier::BOLD)),
            Span::styled(" / 100  ", theme.faint()),
            Span::styled(verdict, Style::new().fg(theme.text)),
        ]),
        Line::from(bar(bar_w, f64::from(score) / 100.0, colour, theme)),
        Line::from(Span::styled("Checked on this device. Nothing is sent anywhere.", theme.faint())),
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    let [groups, entries] = Layout::horizontal([Constraint::Length(34), Constraint::Min(20)]).areas(rest);
    let block = panel(theme, Line::from(Span::styled(" Findings ", theme.dim())), true);
    let inner = block.inner(groups).inner(Margin::new(1, 0));
    frame.render_widget(block, groups);
    let lines: Vec<Line> = HEALTH_GROUPS
        .iter()
        .enumerate()
        .map(|(n, (label, _))| {
            let count = ui.health_ids(n).len();
            let picked = n == ui.health_group;
            let base = if picked { theme.selected(true) } else { Style::new() };
            let badge_colour = if count == 0 { theme.ok } else if n <= 1 { theme.danger } else { theme.warn };
            let badge = if count == 0 { "✓".to_owned() } else { count.to_string() };
            let pad = usize::from(inner.width).saturating_sub(label.chars().count() + badge.chars().count() + 3);
            Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                Span::styled(format!("{label}{}", " ".repeat(pad)), base.fg(theme.text)),
                Span::styled(format!(" {badge} "), base.fg(badge_colour).add_modifier(Modifier::BOLD)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);

    let ids = ui.health_ids(ui.health_group);
    let title = HEALTH_GROUPS.get(ui.health_group).map_or("", |g| g.0);
    let block = panel(theme, Line::from(Span::styled(format!(" {title} "), theme.dim())), false);
    let inner = block.inner(entries).inner(Margin::new(1, 0));
    frame.render_widget(block, entries);
    if ids.is_empty() {
        frame.render_widget(Paragraph::new(Line::from(Span::styled("Nothing to fix here", Style::new().fg(theme.ok)))), inner);
        return;
    }
    let skip = ui.health_at.saturating_sub(usize::from(inner.height.saturating_sub(1)));
    let lines: Vec<Line> = ids
        .iter()
        .enumerate()
        .skip(skip)
        .map(|(n, id)| {
            let (title, kind) = ui.titles.get(id).cloned().unwrap_or_else(|| ("(hidden)".into(), String::new()));
            let picked = n == ui.health_at;
            let base = if picked { theme.selected(true) } else { Style::new() };
            Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                Span::styled(format!("{} ", theme.g.kind(&kind)), base.fg(theme.label(None, &kind))),
                Span::styled(title, base.fg(theme.text)),
                Span::styled(if picked { "   Enter opens" } else { "" }, base.fg(theme.low)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn popup(frame: &mut Frame, area: Rect, width: u16, height: u16, theme: &Theme, title: &str, danger: bool) -> Rect {
    let rect = centered(area, width, height);
    frame.render_widget(Clear, rect);
    let colour = if danger { theme.danger } else { theme.primary };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(colour))
        .title(Span::styled(format!(" {title} "), Style::new().fg(colour).add_modifier(Modifier::BOLD)))
        .style(Style::new().bg(theme.raised).fg(theme.text));
    let inner = block.inner(rect).inner(Margin::new(1, 0));
    frame.render_widget(block, rect);
    inner
}

fn draw_overlay(frame: &mut Frame, area: Rect, overlay: &Overlay, theme: &Theme, ui: Option<&VaultUi>) {
    match overlay {
        Overlay::Help => draw_help(frame, area, theme),
        Overlay::Palette { input, at } => {
            let found = palette_matches(input.text());
            let height = (found.len() as u16 + 4).min(area.height - 4).max(6);
            let inner = popup(frame, area, 60, height, theme, "Commands", false);
            let line = Line::from(vec![Span::styled("› ", theme.brand()), Span::styled(input.text().to_owned(), Style::new().fg(theme.text))]);
            frame.render_widget(Paragraph::new(line), Rect { height: 1, ..inner });
            frame.set_cursor_position(Position::new(inner.x + 2 + input.cursor() as u16, inner.y));
            let list_area = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner };
            let width = usize::from(list_area.width);
            let skip = at.saturating_sub(usize::from(list_area.height.saturating_sub(1)));
            let lines: Vec<Line> = found
                .iter()
                .enumerate()
                .skip(skip)
                .map(|(n, (_, label, key))| {
                    let picked = n == *at;
                    let base = if picked { theme.selected(true) } else { Style::new() };
                    let pad = width.saturating_sub(label.chars().count() + key.chars().count() + 3);
                    Line::from(vec![
                        Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                        Span::styled(format!("{label}{}", " ".repeat(pad)), base.fg(theme.text)),
                        Span::styled(format!(" {key} "), base.fg(theme.accent)),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), list_area);
        }
        Overlay::Confirm { title, body, danger, .. } => {
            let inner = popup(frame, area, 60, 9, theme, title, *danger);
            let colour = if *danger { theme.danger } else { theme.primary };
            let lines = vec![
                Line::default(),
                Line::from(Span::styled(body.clone(), Style::new().fg(theme.text))),
            ];
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), Rect { height: inner.height - 2, ..inner });
            let buttons = Line::from(vec![
                Span::styled(" y ", Style::new().bg(colour).fg(theme.on_primary).add_modifier(Modifier::BOLD)),
                Span::styled(" Yes    ", Style::new().fg(theme.text)),
                Span::styled(" n ", Style::new().bg(theme.line).fg(theme.text).add_modifier(Modifier::BOLD)),
                Span::styled(" No", Style::new().fg(theme.text)),
            ])
            .right_aligned();
            frame.render_widget(Paragraph::new(buttons), Rect { y: inner.bottom() - 1, height: 1, ..inner });
        }
        Overlay::Prompt { title, hint, input, secret, .. } => {
            let inner = popup(frame, area, 70, 8, theme, title, false);
            frame.render_widget(Paragraph::new(Span::styled(hint.clone(), theme.dim())).wrap(Wrap { trim: true }), Rect { height: 2, ..inner });
            let look = BoxLook { label: "", secret: *secret, focused: true, placeholder: "" };
            if let Some(at) = text_box(frame, Rect { y: inner.y + 2, height: 3, ..inner }, theme, input, &look) {
                frame.set_cursor_position(at);
            }
            frame.render_widget(
                Paragraph::new(key_hint(theme, &[("Enter", "done"), ("Esc", "cancel")]).right_aligned()),
                Rect { y: inner.bottom() - 1, height: 1, ..inner },
            );
        }
        Overlay::Pick { title, items, at, .. } => {
            let height = (items.len() as u16 + 4).min(area.height - 2);
            let inner = popup(frame, area, 64, height, theme, title, false);
            let skip = at.saturating_sub(usize::from(inner.height.saturating_sub(3)));
            let lines: Vec<Line> = items
                .iter()
                .enumerate()
                .skip(skip)
                .map(|(n, (_, label))| {
                    let picked = n == *at;
                    let base = if picked { theme.selected(true) } else { Style::new() };
                    let width = usize::from(inner.width).saturating_sub(label.chars().count() + 1);
                    Line::from(vec![
                        Span::styled(if picked { "▌" } else { " " }, base.fg(theme.primary)),
                        Span::styled(format!("{label}{}", " ".repeat(width)), base.fg(theme.text)),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), Rect { y: inner.y + 1, height: inner.height.saturating_sub(2), ..inner });
            frame.render_widget(
                Paragraph::new(key_hint(theme, &[("↑↓", "move"), ("Enter", "pick"), ("Esc", "cancel")]).right_aligned()),
                Rect { y: inner.bottom() - 1, height: 1, ..inner },
            );
        }
        Overlay::Import(preview) => {
            let inner = popup(frame, area, 70, (area.height - 4).min(22), theme, "Import", false);
            let mut lines = vec![
                Line::default(),
                Line::from(vec![
                    Span::styled(format!("{} ", preview.count), theme.brand()),
                    Span::styled(format!("items found in a {} file", preview.format), Style::new().fg(theme.text)),
                ]),
                Line::default(),
            ];
            for title in preview.titles.iter().take(usize::from(inner.height.saturating_sub(9))) {
                lines.push(Line::from(vec![Span::styled("  • ", theme.faint()), Span::styled(title.clone(), Style::new().fg(theme.mid))]));
            }
            if preview.count > preview.titles.len() {
                lines.push(Line::from(Span::styled(format!("  and {} more", preview.count - preview.titles.len()), theme.faint())));
            }
            for note in preview.skipped.iter().take(3) {
                lines.push(Line::from(Span::styled(format!("! {note}"), Style::new().fg(theme.warn))));
            }
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("Items already in the vault are left as they are.", theme.faint())));
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), Rect { height: inner.height - 1, ..inner });
            frame.render_widget(
                Paragraph::new(key_hint(theme, &[("y", "import"), ("n", "cancel")]).right_aligned()),
                Rect { y: inner.bottom() - 1, height: 1, ..inner },
            );
        }
        Overlay::Editor(editor) => draw_editor(frame, area, editor, theme, ui),
    }
}

fn draw_editor(frame: &mut Frame, area: Rect, editor: &Editor, theme: &Theme, ui: Option<&VaultUi>) {
    let title = match (&editor.id, ui.and_then(|u| u.detail.as_ref())) {
        (Some(_), Some(detail)) => format!("Edit {}", detail.row.title),
        _ => format!("New {}", vault::kind_name(&editor.kind).to_lowercase()),
    };
    let width = area.width.saturating_sub(8).min(86);
    let height = area.height.saturating_sub(2);
    let inner = popup(frame, area, width, height, theme, &title, false);
    let [body, error, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]).areas(inner);

    // Each row is a label and a box; a multiline box gets more height when picked.
    let label_w = 18u16;
    let heights: Vec<u16> = editor.rows.iter().enumerate().map(|(n, r)| if r.multiline() && n == editor.at { 6 } else { 1 }).collect();
    let mut first = 0;
    loop {
        let used: u16 = heights[first..=editor.at.min(heights.len() - 1)].iter().map(|h| h + 1).sum();
        if used <= body.height || first >= editor.at {
            break;
        }
        first += 1;
    }
    let mut y = body.y + 1;
    let mut cursor = None;
    for (n, row) in editor.rows.iter().enumerate().skip(first) {
        let h = heights[n];
        if y + h > body.bottom() {
            break;
        }
        let picked = n == editor.at;
        let label_style = if picked { theme.brand() } else { theme.dim() };
        let label: String = row.label.chars().take(usize::from(label_w) - 2).collect();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if picked { "▌" } else { " " }, Style::new().fg(theme.primary)),
                Span::styled(label, label_style),
            ])),
            Rect { x: body.x, y, width: label_w, height: 1 },
        );
        let field = Rect { x: body.x + label_w, y, width: body.width.saturating_sub(label_w), height: h };
        let bg = if picked { Style::new().bg(theme.select) } else { Style::new().bg(theme.surface) };
        frame.render_widget(Block::new().style(bg), field);
        let text_area = Rect { x: field.x + 1, width: field.width.saturating_sub(2), ..field };
        let shown: Text = match row.input.as_str() {
            "toggle" => Text::from(Line::from(if row.flag {
                Span::styled("● on", Style::new().fg(theme.primary).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("○ off", Style::new().fg(theme.low))
            })),
            "choice" => Text::from(Line::from(vec![
                Span::styled("‹ ", theme.faint()),
                Span::styled(row.value.text().to_owned(), Style::new().fg(theme.text)),
                Span::styled(" ›", theme.faint()),
            ])),
            _ if row.secret && !editor.show && !row.value.is_empty() => {
                Text::from(Span::styled("•".repeat(row.value.text().chars().count().min(64)), Style::new().fg(theme.text)))
            }
            _ if row.value.is_empty() => {
                let placeholder = match (&row.slot, row.secret, row.had_value) {
                    (Slot::Otp, _, true) => "unchanged · paste an otpauth:// link or seed to replace",
                    (Slot::Otp, _, false) => "otpauth://… link or base32 seed",
                    (_, true, true) => "unchanged · type to replace, ^R to load",
                    (Slot::Sites, ..) => "github.com, https://example.com/login",
                    (Slot::Tags, ..) => "work, family",
                    _ if row.input == "date" => "2026-12-31",
                    _ if row.input == "password" => "^G makes a strong one",
                    _ => "",
                };
                Text::from(Span::styled(placeholder, theme.faint()))
            }
            _ => Text::from(row.value.text().to_owned()).style(Style::new().fg(theme.text)),
        };
        let (line, col) = row.value.cursor_line_col();
        let scroll = (line as u16).saturating_sub(h.saturating_sub(1));
        let hscroll = (col as u16).saturating_sub(text_area.width.saturating_sub(1));
        let hscroll = if row.multiline() { 0 } else { hscroll };
        frame.render_widget(Paragraph::new(shown).scroll((scroll, hscroll)), text_area);
        if picked && !matches!(row.input.as_str(), "toggle" | "choice") {
            let cx = if row.secret && !editor.show { (row.value.text().chars().count().min(64)) as u16 } else { col as u16 - hscroll };
            cursor = Some(Position::new(text_area.x + cx.min(text_area.width.saturating_sub(1)), text_area.y + (line as u16 - scroll)));
        }
        y += h + 1;
    }
    if let Some(e) = &editor.error {
        frame.render_widget(Paragraph::new(Span::styled(format!("✕ {e}"), Style::new().fg(theme.danger))), error);
    }
    let mut keys = vec![("Tab", "next"), ("^S", "save"), ("Esc", "cancel")];
    let row = &editor.rows[editor.at.min(editor.rows.len() - 1)];
    if row.input == "password" {
        keys.insert(1, ("^G", "generate"));
    }
    if row.secret {
        keys.insert(1, ("^R", if editor.show { "hide" } else { "show" }));
    }
    if matches!(row.input.as_str(), "toggle" | "choice") {
        keys.insert(1, ("Space", "change"));
    }
    if row.multiline() {
        keys.insert(1, ("Enter", "new line"));
    }
    frame.render_widget(Paragraph::new(key_hint(theme, &keys).right_aligned()), foot);
    if let Some(at) = cursor {
        frame.set_cursor_position(at);
    }
}

fn draw_help(frame: &mut Frame, area: Rect, theme: &Theme) {
    let inner = popup(frame, area, 92, (area.height - 2).min(32), theme, "Keys", false);
    let groups: [(&str, &[(&str, &str)]); 6] = [
        ("Anywhere", &[("1-4", "Items, Codes, Generator, Health"), (": or ^K", "commands"), ("^L", "lock now"), ("^T", "next theme"), ("?", "this list"), ("q", "quit")]),
        ("Moving", &[("↑↓ or j k", "move"), ("←→ or h l", "pane"), ("Tab", "next pane"), ("g G", "top, bottom"), ("Enter", "open")]),
        ("Search", &[("/", "search"), ("Esc", "clear"), ("tag:work", "by tag"), ("kind:card", "by kind"), ("has:otp", "with codes"), ("weak reused old", "health")]),
        ("Item", &[("c", "copy password"), ("u", "copy username"), ("t", "copy code"), ("r", "show password"), ("e n", "edit, new"), ("d", "trash"), ("f p", "favourite, pin"), ("m + -", "folder, tags"), ("a", "attach a file")]),
        ("In an item", &[("r Space", "show or hide"), ("y Enter", "copy the line"), ("o", "save a file")]),
        ("In the trash", &[("u", "restore"), ("D", "delete for good")]),
    ];
    let half = inner.width / 2;
    let mut col = [Vec::new(), Vec::new()];
    for (n, (title, keys)) in groups.iter().enumerate() {
        let side = &mut col[usize::from(n >= 3)];
        side.push(Line::from(Span::styled(title.to_uppercase(), theme.brand())));
        for (key, what) in keys.iter() {
            side.push(Line::from(vec![Span::styled(format!("  {key:<16}"), theme.key()), Span::styled(*what, Style::new().fg(theme.text))]));
        }
        side.push(Line::default());
    }
    let [l, r] = col;
    frame.render_widget(Paragraph::new(l), Rect { width: half, ..inner });
    frame.render_widget(Paragraph::new(r), Rect { x: inner.x + half, width: inner.width - half, ..inner });
    let foot = Line::from(Span::styled(
        format!("Copied secrets clear themselves. Every change is saved at once. {} commands under :", COMMANDS.len()),
        theme.faint(),
    ))
    .centered();
    frame.render_widget(Paragraph::new(foot), Rect { y: inner.bottom() - 1, height: 1, ..inner });
}
