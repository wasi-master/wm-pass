//! The app driven by keys and drawn into a buffer, as a person would see it.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

use crate::app::{App, Overlay, Screen};
use crate::engine;

const PASSWORD: &str = "Tr0ub4dor-and-3-horses";

fn vault(dir: &std::path::Path) -> Box<engine::Engine> {
    let mut engine = engine::create(&dir.join("t.wmvault"), "Personal", b"master-pw", 3).expect("create");
    let mut plain = serde_json::Map::new();
    plain.insert("username".into(), json!("octo"));
    let id = engine.create("login", "GitHub", &plain, &[json!({"Domain": "github.com"})]).expect("login");
    engine.set_secret(&id, "password", PASSWORD.as_bytes()).expect("password");
    engine.set_otp(&id, Some(b"otpauth://totp/GitHub:octo?secret=JBSWY3DPEHPK3PXP&issuer=GitHub")).expect("otp");
    engine.set_common(&id, json!({"tags": ["dev"], "favourite": true})).expect("tags");
    engine.create("note", "Wi-Fi at home", &serde_json::Map::new(), &[]).expect("note");
    Box::new(engine)
}

fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal.draw(|frame| crate::draw::draw(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn press(app: &mut App, code: KeyCode) {
    app.key_for_test(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, ch: char) {
    app.key_for_test(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
}

fn typed(app: &mut App, text: &str) {
    for ch in text.chars() {
        press(app, KeyCode::Char(ch));
    }
}

#[test]
fn a_secret_shows_only_when_asked_and_hides_on_lock() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("GitHub") && shown.contains("Wi-Fi at home"), "{shown}");
    assert!(shown.contains("octo"));
    assert!(!shown.contains(PASSWORD), "a password is never drawn unasked");

    // The list is sorted by title, so GitHub is first and picked.
    press(&mut app, KeyCode::Char('r'));
    let revealed = screen(&mut app, 140, 40);
    assert!(revealed.contains(PASSWORD), "{revealed}");
    press(&mut app, KeyCode::Char('r'));
    assert!(!screen(&mut app, 140, 40).contains(PASSWORD), "r hides it again");

    press(&mut app, KeyCode::Char('r'));
    ctrl(&mut app, 'l');
    assert!(matches!(app.screen, Screen::Door(_)));
    let door = screen(&mut app, 140, 40);
    assert!(!door.contains(PASSWORD) && !door.contains("GitHub"));
    assert!(door.contains("Unlock t.wmvault"), "{door}");
}

#[test]
fn every_tab_and_size_draws() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    for tab in ['1', '2', '3', '4'] {
        press(&mut app, KeyCode::Char(tab));
        for (w, h) in [(160, 48), (100, 30), (70, 24), (44, 14), (30, 8)] {
            let shown = screen(&mut app, w, h);
            assert!(!shown.contains(PASSWORD), "tab {tab} at {w}x{h}");
        }
    }
    press(&mut app, KeyCode::Char('2'));
    let codes = screen(&mut app, 120, 30);
    assert!(codes.contains("GitHub") && codes.contains("octo"), "{codes}");
    press(&mut app, KeyCode::Char('3'));
    let generator = screen(&mut app, 120, 30);
    assert!(generator.contains("bits"), "{generator}");
    press(&mut app, KeyCode::Char('?'));
    assert!(screen(&mut app, 120, 40).contains("copy password"));
}

#[test]
fn a_new_login_from_the_keyboard() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('n'));
    assert!(matches!(app.overlay, Some(Overlay::Pick { .. })));
    press(&mut app, KeyCode::Enter);
    assert!(matches!(app.overlay, Some(Overlay::Editor(_))));
    typed(&mut app, "Mastodon");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "me@example.social");
    press(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'g');
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "mastodon.social");
    ctrl(&mut app, 's');
    assert!(app.overlay.is_none(), "saving closes the editor");

    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let row = ui.current().cloned().expect("picked");
    assert_eq!(row.title, "Mastodon");
    assert_eq!(row.subtitle, "me@example.social");
    assert!(row.has_password, "^G filled the password");
    assert_eq!(row.site.as_deref(), Some("mastodon.social"));

    // It is in the file, not just in memory.
    let path = ui.engine.path().to_owned();
    let engine::Unlock::Opened(mut again, _) = engine::unlock(&path, b"master-pw", None, 4).expect("opens") else {
        panic!("did not open");
    };
    assert_eq!(again.list("mastodon", "").expect("list").len(), 1);
}

#[test]
fn search_trash_and_restore() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('/'));
    typed(&mut app, "tag:dev");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.rows.len(), 1);

    press(&mut app, KeyCode::Char('d'));
    assert!(matches!(app.overlay, Some(Overlay::Confirm { .. })));
    press(&mut app, KeyCode::Char('y'));
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(ui.rows.is_empty(), "gone from the list");
    let trash = ui.sections.iter().position(|s| s.query == "trashed").expect("trash");
    ui.search.clear();
    ui.section = trash;
    ui.refresh_list(None);
    assert_eq!(ui.rows.len(), 1);
    press(&mut app, KeyCode::Char('u'));
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.is_empty(), "brought back out of the trash");
    assert_eq!(ui.total, 2);
}

#[test]
fn the_palette_finds_and_runs() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "health");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.tab, crate::vault::Tab::Health);
    assert!(ui.health.is_some());
    assert!(screen(&mut app, 120, 30).contains("/ 100"));
}

#[test]
fn nerd_font_icons_from_the_palette() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    assert!(screen(&mut app, 140, 40).contains('◉'), "plain icons by default");
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "nerd");
    press(&mut app, KeyCode::Enter);
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains('\u{f084}') && !shown.contains('◉'), "{shown}");
    app.command(crate::app::Command::Theme);
    assert!(app.theme.g.nerd, "a new theme keeps the icons");
}

#[test]
fn editing_leaves_secrets_alone_unless_typed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    assert!(editor.rows.iter().filter(|r| r.secret).all(|r| r.value.is_empty()), "no secret is loaded to edit a title");
    ctrl(&mut app, 'e');
    typed(&mut app, " Enterprise");
    ctrl(&mut app, 's');
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let id = ui.current_id().expect("picked");
    assert_eq!(ui.current().expect("row").title, "GitHub Enterprise");
    let password = ui.engine.reveal(&engine::Secret::Field("password".into()), &id).expect("reveal");
    assert_eq!(password.as_str(), PASSWORD);
    assert!(ui.detail.as_ref().expect("detail").history.is_empty(), "no revision for an untouched password");
}

/// Writes each screen as a coloured HTML page, for looking at the design without a terminal.
/// Run with `WMPASS_TUI_SNAPSHOTS=<dir> cargo test -p wmpass-tui snapshots -- --ignored`.
#[test]
#[ignore = "writes files; run by hand"]
fn snapshots() {
    use ratatui::style::{Color, Modifier};
    let Some(out) = std::env::var_os("WMPASS_TUI_SNAPSHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&out).expect("dir");
    let colour = |c: Color, fallback: &str| match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => fallback.to_owned(),
    };
    let html = |app: &mut App, w: u16, h: u16| -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        terminal.draw(|frame| crate::draw::draw(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut page = String::from("<html><head><meta charset=\"utf-8\"></head><body style=\"margin:0;background:#0E1513\"><pre style=\"font:14px/1.25 'FiraCode Nerd Font Mono','FiraCode Nerd Font','JetBrains Mono',Menlo,monospace;margin:12px\">");
        for y in 0..h {
            for x in 0..w {
                let cell = &buffer[(x, y)];
                let bold = if cell.modifier.contains(Modifier::BOLD) { "font-weight:bold;" } else { "" };
                let symbol = match cell.symbol() {
                    "<" => "&lt;",
                    ">" => "&gt;",
                    "&" => "&amp;",
                    other => other,
                };
                page.push_str(&format!(
                    "<span style=\"color:{};background:{};{bold}\">{symbol}</span>",
                    colour(cell.fg, "#DDE4E1"),
                    colour(cell.bg, "#0E1513"),
                ));
            }
            page.push('\n');
        }
        page.push_str("</pre></body></html>");
        page
    };
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    for (title, user, site) in [("Proton Mail", "me@proton.me", "proton.me"), ("Amazon", "shopper@example.com", "amazon.com"), ("Bank of Somewhere", "acct-1234", "bank.example")] {
        let mut plain = serde_json::Map::new();
        plain.insert("username".into(), json!(user));
        let id = engine.create("login", title, &plain, &[json!({"Domain": site})]).expect("login");
        engine.set_secret(&id, "password", b"password1").expect("weak");
    }
    let mut plain = serde_json::Map::new();
    plain.insert("holder".into(), json!("Ada Lovelace"));
    plain.insert("issuer".into(), json!("Visa"));
    let card = engine.create("card", "Travel card", &plain, &[]).expect("card");
    engine.set_secret(&card, "number", b"4111111111111111").expect("number");
    let mut app = App::over(engine);
    std::fs::write(out.join("items.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('l'));
    press(&mut app, KeyCode::Char('r'));
    std::fs::write(out.join("detail.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('2'));
    std::fs::write(out.join("codes.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('3'));
    std::fs::write(out.join("generator.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('4'));
    std::fs::write(out.join("health.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('e'));
    std::fs::write(out.join("editor.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char(':'));
    std::fs::write(out.join("palette.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Esc);
    std::fs::write(out.join("narrow.html"), html(&mut app, 70, 30)).expect("write");
    app.theme.g.nerd = true;
    std::fs::write(out.join("nerd.html"), html(&mut app, 150, 40)).expect("write");
    app.theme.g.nerd = false;
    ctrl(&mut app, 'l');
    std::fs::write(out.join("door.html"), html(&mut app, 150, 40)).expect("write");
}
