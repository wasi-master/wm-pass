//! The app driven by keys and drawn into a buffer, as a person would see it.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

use crate::app::{App, Overlay, Screen};
use crate::link::{LinkUi, Role, Stage};
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
    for tab in ['1', '2', '3', '4', '5'] {
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
fn the_palette_sets_how_long_the_clipboard_keeps_a_secret() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    assert_eq!(app.prefs.clip_seconds, 60);
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "clear the clipboard");
    press(&mut app, KeyCode::Enter);
    assert!(screen(&mut app, 120, 30).contains("Clear the clipboard after"));
    for _ in 0..2 {
        press(&mut app, KeyCode::Backspace);
    }
    typed(&mut app, "45");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.prefs.clip_seconds, 45);
    assert!(screen(&mut app, 120, 30).contains("cleared after 45 seconds"));
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
    press(&mut app, KeyCode::Char('5'));
    std::fs::write(out.join("settings.html"), html(&mut app, 150, 40)).expect("write");
    std::fs::write(out.join("settings-narrow.html"), html(&mut app, 80, 30)).expect("write");
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Char('e'));
    std::fs::write(out.join("editor.html"), html(&mut app, 150, 40)).expect("write");
    ctrl(&mut app, 'n');
    typed(&mut app, "Recovery email");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "backup@example.com");
    std::fs::write(out.join("editor-custom.html"), html(&mut app, 150, 40)).expect("write");
    {
        let Some(Overlay::Editor(editor)) = &mut app.overlay else { panic!("no editor") };
        editor.at = editor.rows.iter().position(|r| r.key == "password").expect("password");
    }
    typed(&mut app, "x");
    std::fs::write(out.join("editor-password.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Esc);
    std::fs::write(out.join("editor-discard.html"), html(&mut app, 150, 40)).expect("write");
    press(&mut app, KeyCode::Char('y'));
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

fn click(app: &mut App, x: u16, y: u16) {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    app.mouse_for_test(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE });
}

/// Where a piece of text was drawn.
fn find(shown: &str, text: &str) -> (u16, u16) {
    for (y, line) in shown.lines().enumerate() {
        if let Some(at) = line.find(text) {
            return (line[..at].chars().count() as u16, y as u16);
        }
    }
    panic!("{text} is not on screen:\n{shown}");
}

#[test]
fn the_join_code_draws_in_solid_cells_that_scan() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let code = "wmpass-pair:1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:1800000000000:MTkyLjE2OC4xLjQ6NTEyMzQ".to_owned();
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(
        Role::Receive,
        Stage::Waiting { code: Some(code.clone()), address: Some("192.168.1.4:51234".into()) },
    ))));
    let mut terminal = Terminal::new(TestBackend::new(200, 60)).expect("terminal");
    terminal.draw(|frame| crate::draw::draw(frame, &mut app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let (black, white) = (ratatui::style::Color::Indexed(16), ratatui::style::Color::Indexed(231));
    let mut painted = 0;
    for cell in buffer.content() {
        if cell.bg == black || cell.bg == white {
            painted += 1;
            assert_eq!(cell.symbol(), " ", "a big code paints whole cells, with no half blocks to leave seams");
        }
    }
    let qr = crate::join::Qr::new(&code).expect("qr");
    let side = qr.width + 2 * crate::join::QUIET as usize;
    assert_eq!(painted, side * side * 2);
    let shown = screen(&mut app, 200, 60);
    assert!(shown.contains("Find it on this network"), "the network way is offered too");
    assert!(shown.contains("Paste its code"));
    // Small terminals get the half-block code instead, which still fits.
    let shown = screen(&mut app, 100, 40);
    assert!(shown.contains("▀"));
}

#[test]
fn digits_are_answered_with_a_click() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(
        Role::Send,
        Stage::Digits { code: "123456".into(), peer: "Pixel".into() },
    ))));
    let shown = screen(&mut app, 120, 40);
    assert!(shown.contains("Pixel"));
    let (x, y) = find(&shown, "They differ");
    // With nothing on the network behind it the answer goes nowhere, but the screen moves on.
    click(&mut app, x, y);
    let Some(Overlay::Link(link)) = &app.overlay else { panic!("the screen closed") };
    assert_eq!(link.stage, Stage::Digits { code: "123456".into(), peer: "Pixel".into() });
    // Esc in the hint bar closes it.
    let shown = screen(&mut app, 120, 40);
    let (x, y) = find(&shown, "Esc stop");
    click(&mut app, x, y);
    assert!(app.overlay.is_none());
}

#[test]
fn the_mouse_picks_tabs_rows_and_palette_commands() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let shown = screen(&mut app, 140, 40);
    let (x, y) = find(&shown, "Wi-Fi at home");
    click(&mut app, x, y);
    let Screen::Vault(ui) = &app.screen else { panic!() };
    assert_eq!(ui.current().map(|r| r.title.as_str()), Some("Wi-Fi at home"));
    let (x, y) = find(&shown, "Generator");
    click(&mut app, x, y);
    let Screen::Vault(ui) = &app.screen else { panic!() };
    assert_eq!(ui.tab, crate::vault::Tab::Generator);
    // The hint bar's keys are buttons.
    press(&mut app, KeyCode::Char(':'));
    typed(&mut app, "go to");
    let shown = screen(&mut app, 140, 40);
    let (x, y) = find(&shown, "Go to Health");
    click(&mut app, x, y);
    assert!(app.overlay.is_none());
    let Screen::Vault(ui) = &app.screen else { panic!() };
    assert_eq!(ui.tab, crate::vault::Tab::Health);
    // A click outside the palette closes it.
    press(&mut app, KeyCode::Char(':'));
    screen(&mut app, 140, 40);
    click(&mut app, 1, 39);
    assert!(app.overlay.is_none());
}

#[test]
#[ignore = "prints a screen to look at"]
fn print_join_screens() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let code = "wmpass-pair:1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:1800000000000:MTkyLjE2OC4xLjQ6NTEyMzQ".to_owned();
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(Role::Receive, Stage::Waiting { code: Some(code), address: Some("192.168.1.4:51234".into()) }))));
    println!("{}", screen(&mut app, 150, 45));
    let mut link = LinkUi::at_stage(Role::Send, Stage::Searching);
    link.found = vec!["192.168.1.20:5123".parse().unwrap()];
    app.overlay = Some(Overlay::Link(Box::new(link)));
    println!("{}", screen(&mut app, 110, 30));
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(Role::Send, Stage::Digits { code: "482913".into(), peer: "Pixel 9".into() }))));
    println!("{}", screen(&mut app, 110, 30));
}

#[test]
fn health_groups_reused_passwords_ignores_per_check_and_leaves_out_local_logins() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let mut weak = |title: &str, site: &str| {
        let id = engine.create("login", title, &serde_json::Map::new(), &[json!({"Url": site})]).expect("login");
        engine.set_secret(&id, "password", b"password1").expect("password");
        id
    };
    let mail = weak("Mail", "https://mail.example.com");
    weak("Shop", "https://shop.example.org");
    weak("Router", "http://192.168.1.1");
    let mut app = App::over(engine);
    press(&mut app, KeyCode::Char('4'));

    // Local addresses are left out by default; the reused ones come as one group per password.
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert_eq!(ui.health_ids(0).len(), 2, "the router's login is left out");
    assert!(matches!(&ui.health_lines(1)[0], crate::vault::HealthLine::Heading(text) if text == "2 items share one password"));
    assert_eq!(ui.health_groups(), crate::vault::IGNORED_GROUP, "no Ignored group until something is");
    ui.health_group = 1;
    let reused = screen(&mut app, 140, 30);
    assert!(reused.contains("2 items share one password"), "{reused}");

    // Ignoring one leaves it out of that check only, and lists it under Ignored.
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    ui.health_group = 0;
    ui.health_at = ui.health_ids(0).iter().position(|id| *id == mail).expect("mail is weak");
    press(&mut app, KeyCode::Char('i'));
    assert_eq!(app.prefs.health_ignored, vec![format!("weak:{mail}")]);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(!ui.health_ids(0).contains(&mail));
    assert!(ui.health_ids(1).contains(&mail), "still reused");
    assert_eq!(ui.health_groups(), crate::vault::HEALTH_GROUPS.len());
    ui.health_group = crate::vault::IGNORED_GROUP;
    ui.health_at = 0;
    let ignored = screen(&mut app, 140, 30);
    assert!(ignored.contains("Mail") && ignored.contains("Weak passwords"), "{ignored}");

    // And it counts again from there.
    press(&mut app, KeyCode::Char('i'));
    assert!(app.prefs.health_ignored.is_empty());
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.health_ids(0).contains(&mail));
    assert!(ui.health_group < crate::vault::IGNORED_GROUP, "the empty Ignored group goes");

    app.command(crate::app::Command::SkipLocal);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.health_ids(0).len(), 3, "the router's login counts again");
}

#[test]
fn a_login_shows_where_to_change_its_password_only_for_a_listed_site() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    engine.create("login", "23andMe", &serde_json::Map::new(), &[json!({"Domain": "23andme.com"})]).expect("login");
    let mut app = App::over(engine);
    let change = |app: &App| {
        let Screen::Vault(ui) = &app.screen else { panic!("locked") };
        ui.lines.iter().find_map(|line| match line {
            crate::vault::Line::Value { label, text, .. } if label == "Change password" => text.clone(),
            _ => None,
        })
    };
    // GitHub is not on the list, and the terminal asks no site, so no guessed address.
    pick(&mut app, "GitHub");
    let shown = screen(&mut app, 160, 40);
    assert_eq!(change(&app), None, "{shown}");
    pick(&mut app, "23andMe");
    let shown = screen(&mut app, 160, 40);
    assert_eq!(change(&app).as_deref(), Some("https://you.23andme.com/user/edit/password/"), "{shown}");
    assert!(shown.contains("Change password"), "{shown}");
}

/// Picks the row with this title in the list.
fn pick(app: &mut App, title: &str) {
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let at = ui.rows.iter().position(|r| r.title == title).expect("row");
    ui.list.select(Some(at));
    ui.load_detail();
}

#[test]
fn apps_show_by_name_and_wifi_shows_its_code() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    engine
        .create("login", "Chat", &serde_json::Map::new(), &[json!({"AndroidApp": {"package": "com.facebook.orca", "cert_sha256": null}})])
        .expect("login");
    let mut plain = serde_json::Map::new();
    plain.insert("ssid".into(), json!("Home"));
    plain.insert("security".into(), json!("wpa"));
    let wifi = engine.create("wifi", "Home network", &plain, &[]).expect("wifi");
    engine.set_secret(&wifi, "password", b"hunter22").expect("password");
    let mut app = App::over(engine);

    pick(&mut app, "Chat");
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("Messenger (com.facebook.orca)"), "{shown}");
    press(&mut app, KeyCode::Char('w'));
    assert!(app.overlay.is_none(), "only a Wi-Fi network has a code");

    pick(&mut app, "Home network");
    press(&mut app, KeyCode::Char('w'));
    let Some(Overlay::WifiCode { ssid, qr }) = &app.overlay else { panic!("no code") };
    assert_eq!(ssid, "Home");
    assert_eq!(qr.width, qrcode::QrCode::new(b"WIFI:T:WPA;S:Home;P:hunter22;;").expect("qr").width());
    let shown = screen(&mut app, 160, 60);
    assert!(shown.contains("join the network"), "{shown}");
    assert!(!shown.contains("hunter22"), "the password is only in the code");
    press(&mut app, KeyCode::Esc);
    assert!(app.overlay.is_none());
}

#[test]
fn a_tag_is_renamed_and_deleted_from_the_sidebar() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let pick_tag = |app: &mut App, tag: &str| {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.pane = crate::vault::Pane::Sidebar;
        ui.section = ui.sections.iter().position(|s| s.label == tag && !s.heading).expect("tag in the sidebar");
    };
    pick_tag(&mut app, "dev");
    press(&mut app, KeyCode::Char('R'));
    assert!(matches!(app.overlay, Some(Overlay::Prompt { .. })), "R asks for the new name");
    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace);
    }
    typed(&mut app, "code");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let row = ui.engine.list("", "title").expect("list").into_iter().find(|r| r.title == "GitHub").expect("row");
    assert_eq!(row.tags, vec!["code".to_owned()]);
    assert!(ui.picked_tag().as_deref() == Some("code"), "the sidebar follows the new name");

    press(&mut app, KeyCode::Char('D'));
    assert!(matches!(app.overlay, Some(Overlay::Confirm { .. })), "D asks first");
    press(&mut app, KeyCode::Char('y'));
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let row = ui.engine.list("", "title").expect("list").into_iter().find(|r| r.title == "GitHub").expect("row");
    assert!(row.tags.is_empty(), "the tag came off, the item stayed");
}

#[test]
fn marked_items_are_acted_on_together() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    assert!(screen(&mut app, 140, 40).contains("2 marked"));

    let run = |app: &mut App, words: &str| {
        press(app, KeyCode::Char(':'));
        typed(app, words);
        press(app, KeyCode::Enter);
    };
    run(&mut app, "add a tag");
    typed(&mut app, "work");
    press(&mut app, KeyCode::Enter);
    run(&mut app, "favorite or");
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.iter().all(|r| r.tags.contains(&"work".to_owned())), "tagged both");
    assert!(ui.rows.iter().all(|r| r.favourite), "one was a favorite, so both became one");
    assert_eq!(ui.marked.len(), 2, "still marked");

    run(&mut app, "color");
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.iter().all(|r| r.color.as_deref() == Some("25")), "both red");
    assert!(ui.marked.is_empty(), "a color clears the marks, as on the phone");

    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    run(&mut app, "link the marked");
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let ids: Vec<String> = ui.rows.iter().map(|r| r.id.clone()).collect();
    for (n, id) in ids.iter().enumerate() {
        let other = &ids[1 - n];
        assert_eq!(ui.engine.detail(id).expect("detail").links, vec![other.clone()]);
    }

    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Esc);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.marked.is_empty(), "Esc unmarks");
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Char('y'));
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert!(ui.rows.is_empty() && ui.marked.is_empty(), "both in the trash");
}

/// Moves the editor's cursor to the first row that `wanted` picks.
fn editor_to(app: &mut App, wanted: impl Fn(&crate::editor::Row) -> bool) {
    let Some(Overlay::Editor(editor)) = &mut app.overlay else { panic!("no editor") };
    editor.at = editor.rows.iter().position(wanted).expect("row");
    editor.on_name = false;
}

fn current_detail(app: &mut App) -> engine::Detail {
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let id = ui.current_id().expect("picked");
    ui.engine.detail(&id).expect("detail")
}

#[test]
fn custom_fields_apps_and_questions_are_edited_in_place() {
    use crate::editor::Slot;
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let site = json!({"AndroidApp": {"package": "com.facebook.orca", "cert_sha256": "ab:cd"}});
    let id = engine.create("login", "Chat", &serde_json::Map::new(), &[json!({"Domain": "messenger.com"}), site]).expect("login");
    engine.set_custom(&id, None, "Account number", "text", b"12345").expect("custom");
    engine.set_custom(&id, None, "Recovery PIN", "hidden", b"9999").expect("hidden");
    engine.set_custom(&id, None, "Old", "url", b"https://old.example").expect("third");
    engine.set_question(&id, None, "First pet?", b"Rex").expect("question");
    let mut app = App::over(engine);
    pick(&mut app, "Chat");
    press(&mut app, KeyCode::Char('e'));

    let shown = screen(&mut app, 140, 50);
    assert!(shown.contains("Account number") && shown.contains("12345"), "a custom field shows its value\n{shown}");
    assert!(shown.contains("Messenger (com.facebook.orca)"), "an app shows by name\n{shown}");
    assert!(shown.contains("More fields") && shown.contains("Security questions") && shown.contains("First pet?"), "{shown}");
    assert!(!shown.contains("9999") && !shown.contains("Rex"), "hidden values stay hidden until edited");

    // A plain custom field takes typing like any other.
    editor_to(&mut app, |r| r.slot == Slot::Custom && r.name.text() == "Account number");
    typed(&mut app, "-6");
    // A hidden one is loaded by the first key, then edited.
    editor_to(&mut app, |r| r.slot == Slot::Custom && r.name.text() == "Recovery PIN");
    press(&mut app, KeyCode::Backspace);
    typed(&mut app, "8");
    // Renaming the third keeps its kind; it is then taken away.
    editor_to(&mut app, |r| r.slot == Slot::Custom && r.name.text() == "Old");
    ctrl(&mut app, 'x');
    // A new field: the cursor starts in its name, Tab goes to its value.
    ctrl(&mut app, 'n');
    typed(&mut app, "Member since");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "2019");
    // The question's answer is changed; the apps lose one and gain one.
    editor_to(&mut app, |r| r.slot == Slot::Question);
    ctrl(&mut app, 'u');
    typed(&mut app, "Max");
    editor_to(&mut app, |r| r.slot == Slot::Apps);
    ctrl(&mut app, 'e');
    typed(&mut app, ", org.example.chat");
    ctrl(&mut app, 's');
    assert!(app.overlay.is_none(), "saved");

    let detail = current_detail(&mut app);
    let customs: Vec<(String, String, Option<String>)> =
        detail.custom_fields.iter().map(|c| (c.name.clone(), c.kind.clone(), c.value.clone())).collect();
    assert_eq!(
        customs,
        vec![
            ("Account number".into(), "text".into(), Some("12345-6".into())),
            ("Recovery PIN".into(), "hidden".into(), None),
            ("Member since".into(), "text".into(), Some("2019".into())),
        ]
    );
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert_eq!(ui.engine.reveal(&engine::Secret::Custom(1), &id).expect("pin").as_str(), "9998");
    assert_eq!(ui.engine.reveal(&engine::Secret::Question(0), &id).expect("answer").as_str(), "Max");
    let apps: Vec<&serde_json::Value> = detail.sites.iter().filter(|s| s.get("AndroidApp").is_some()).collect();
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0]["AndroidApp"]["cert_sha256"], "ab:cd", "a kept app keeps its certificate");
    assert_eq!(apps[1]["AndroidApp"]["package"], "org.example.chat");
    assert!(detail.sites.iter().any(|s| s["Domain"] == "messenger.com"));
}

#[test]
fn a_saved_password_is_edited_not_replaced_and_a_note_opens_with_its_text() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let note = engine.list("", "title").expect("list").into_iter().find(|r| r.kind == "note").expect("note").id;
    engine.set_secret(&note, "body", b"router: admin / sticker on the back").expect("body");
    let mut app = App::over(engine);
    press(&mut app, KeyCode::Char('e'));
    editor_to(&mut app, |r| r.key == "password");
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("saved") && !shown.contains(PASSWORD), "{shown}");
    typed(&mut app, "!");
    ctrl(&mut app, 's');
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let id = ui.current_id().expect("picked");
    let password = ui.engine.reveal(&engine::Secret::Field("password".into()), &id).expect("reveal");
    assert_eq!(password.as_str(), format!("{PASSWORD}!"), "typing adds to what was there");

    pick(&mut app, "Wi-Fi at home");
    press(&mut app, KeyCode::Char('e'));
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    let body = editor.rows.iter().find(|r| r.key == "body").expect("body");
    assert_eq!(body.value.text(), "router: admin / sticker on the back");
    assert!(!editor.dirty(), "loading is not a change");
}

#[test]
fn leaving_the_editor_with_changes_asks_first() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Esc);
    assert!(app.overlay.is_none(), "nothing changed, nothing to ask");
    press(&mut app, KeyCode::Char('e'));
    typed(&mut app, "!");
    press(&mut app, KeyCode::Esc);
    assert!(screen(&mut app, 140, 40).contains("Discard your changes?"));
    press(&mut app, KeyCode::Char('n'));
    assert!(matches!(app.overlay, Some(Overlay::Editor(_))), "any other key goes back to editing");
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Char('y'));
    assert!(app.overlay.is_none());
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.current().expect("row").title, "GitHub");
}

#[test]
fn settings_has_a_tab_of_its_own() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('5'));
    let shown = screen(&mut app, 140, 40);
    for words in ["Settings", "Theme", "Lock after", "Clear the clipboard after", "Vault name", "Empty the trash after", "Import from another app", "THIS VAULT"] {
        assert!(shown.contains(words), "{words}\n{shown}");
    }
    assert!(screen(&mut app, 80, 30).contains("Lock after"), "narrow too");

    let to = |app: &mut App, label: &str| {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.settings_at = crate::settings::all().iter().position(|s| s.label() == label).expect("setting");
    };
    to(&mut app, "Lock after");
    assert_eq!(app.prefs.lock_minutes, 5);
    press(&mut app, KeyCode::Right);
    assert_eq!(app.prefs.lock_minutes, 10);
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.prefs.lock_minutes, 2);

    to(&mut app, "Empty the trash after");
    press(&mut app, KeyCode::Enter);
    ctrl(&mut app, 'u');
    typed(&mut app, "45");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert_eq!(ui.engine.info().expect("info").trash_days, 45);

    to(&mut app, "Vault name");
    press(&mut app, KeyCode::Enter);
    ctrl(&mut app, 'u');
    typed(&mut app, "Family");
    press(&mut app, KeyCode::Enter);
    assert!(screen(&mut app, 140, 40).contains("Family"));

    to(&mut app, "Settings follow the vault");
    let before = app.prefs.sync_settings;
    press(&mut app, KeyCode::Enter);
    assert_ne!(app.prefs.sync_settings, before);
}

#[test]
fn the_vault_password_is_changed_from_settings() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('5'));
    {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.settings_at = crate::settings::all().iter().position(|s| *s == crate::settings::Setting::Password).expect("setting");
    }
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "not-it");
    press(&mut app, KeyCode::Enter);
    assert!(app.overlay.is_none(), "a wrong password ends it");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "master-pw");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a brand new pass");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a brand new pass");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(ui.engine.verify(b"a brand new pass").expect("verify"));
    assert!(!ui.engine.verify(b"master-pw").expect("verify"));
    let path = ui.engine.path().to_owned();
    ctrl(&mut app, 'l');
    assert!(matches!(engine::unlock(&path, b"a brand new pass", None, 3), Ok(engine::Unlock::Opened(..))));
}

#[test]
fn a_second_password_changes_itself_not_the_first() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.engine.add_password("Second", b"second-pw").expect("slot");
    }
    press(&mut app, KeyCode::Char('5'));
    {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.settings_at = crate::settings::all().iter().position(|s| *s == crate::settings::Setting::Password).expect("setting");
    }
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "second-pw");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a renewed second");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a renewed second");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(ui.engine.verify(b"master-pw").expect("verify"), "the first password still opens");
    assert!(ui.engine.verify(b"a renewed second").expect("verify"));
    assert!(!ui.engine.verify(b"second-pw").expect("verify"));
}

#[test]
fn a_password_with_a_key_file_keeps_needing_it_after_a_change() {
    let dir = tempfile::tempdir().expect("dir");
    let key_path = dir.path().join("vault.key");
    std::fs::write(&key_path, b"key file contents").expect("key file");
    let mut app = App::over(vault(dir.path()));
    {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.engine.add_password_with_key_file("With file", b"second-pw", b"key file contents").expect("slot");
    }
    press(&mut app, KeyCode::Char('5'));
    {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        ui.settings_at = crate::settings::all().iter().position(|s| *s == crate::settings::Setting::Password).expect("setting");
    }
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "second-pw");
    press(&mut app, KeyCode::Enter);
    assert!(app.overlay.is_some(), "asks for the key file rather than calling the password wrong");
    typed(&mut app, key_path.to_str().expect("path"));
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a renewed second");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "a renewed second");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let path = ui.engine.path().to_owned();
    ctrl(&mut app, 'l');
    assert!(matches!(engine::unlock(&path, b"a renewed second", None, 3), Ok(engine::Unlock::NeedsKeyFile)), "the new password alone does not open it");
    assert!(matches!(engine::unlock(&path, b"a renewed second", Some(b"key file contents"), 3), Ok(engine::Unlock::Opened(..))));
    assert!(matches!(engine::unlock(&path, b"master-pw", None, 3), Ok(engine::Unlock::Opened(..))), "the first password still opens");
}

#[test]
fn the_editor_files_colors_and_favors_an_item() {
    use crate::editor::Slot;
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let work = engine.add_folder("Work").expect("folder");
    let mut app = App::over(engine);
    pick(&mut app, "Wi-Fi at home");
    press(&mut app, KeyCode::Char('e'));
    editor_to(&mut app, |r| r.slot == Slot::Folder);
    press(&mut app, KeyCode::Right);
    editor_to(&mut app, |r| r.slot == Slot::Color);
    press(&mut app, KeyCode::Right);
    editor_to(&mut app, |r| r.slot == Slot::Favourite);
    press(&mut app, KeyCode::Char(' '));
    ctrl(&mut app, 's');
    let detail = current_detail(&mut app);
    assert_eq!(detail.row.folder.as_deref(), Some(work.as_str()));
    assert_eq!(detail.row.color.as_deref(), Some(crate::app::HUES[0].0));
    assert!(detail.row.favourite);

    // Opened again, it shows what it was saved with, and nothing counts as changed.
    press(&mut app, KeyCode::Char('e'));
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    assert_eq!(editor.rows.iter().find(|r| r.slot == Slot::Folder).expect("folder").value.text(), "Work");
    assert!(!editor.dirty());
}

#[test]
fn a_database_engine_is_offered_as_it_is_typed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('n'));
    let Some(Overlay::Pick { items, at, .. }) = &mut app.overlay else { panic!("no pick") };
    *at = items.iter().position(|i| i.0 == "database").expect("database");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "Main db");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "Postg");
    assert!(screen(&mut app, 140, 40).contains("reSQL"), "the rest is offered");
    press(&mut app, KeyCode::Right);
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    assert_eq!(editor.rows[1].value.text(), "PostgreSQL");
}

#[test]
fn linked_items_show_and_open() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let note = {
        let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
        let github = ui.current_id().expect("picked");
        let note = ui.rows.iter().find(|r| r.kind == "note").expect("note").id.clone();
        ui.engine.set_common(&github, json!({"links": [note]})).expect("link");
        ui.reload();
        ui.load_detail();
        note
    };
    let shown = screen(&mut app, 140, 40);
    assert!(shown.contains("LINKED"), "{shown}");
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    ui.pane = crate::vault::Pane::Detail;
    ui.line = ui.lines.iter().position(|l| matches!(l, crate::vault::Line::Link(_))).expect("link line");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.current_id().as_deref(), Some(note.as_str()));
}

/// Waits for the restore screen's thread, as the app's loop does between frames.
fn settle(app: &mut App) {
    for _ in 0..400 {
        let busy = matches!(&app.overlay, Some(Overlay::Restore(ui)) if ui.busy());
        app.tick();
        if !busy {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the restore never finished");
}

#[test]
fn a_vault_comes_back_from_a_backup_location_and_opens() {
    use wmpass_sinks::backups::{BundleFile, bundle};
    let dir = tempfile::tempdir().expect("dir");
    let made = vault(dir.path());
    let file = std::fs::read(made.path()).expect("vault file");
    let (older, newer) = (dir.path().join("wmpass-backup-20260101-000000.wmbackup"), dir.path().join("wmpass-backup-20261104-093000.wmbackup"));
    let copy = bundle(&[
        BundleFile { name: "vaults/0a0a".into(), bytes: file },
        BundleFile { name: "vaults/0b0b".into(), bytes: vec![7; 64] },
    ]);
    std::fs::write(&newer, &copy).expect("newer");
    std::fs::write(&older, b"not even a zip").expect("older");

    let mut app = App::over(made);
    ctrl(&mut app, 'l');
    ctrl(&mut app, 'b');
    assert!(matches!(app.overlay, Some(Overlay::Restore(_))));
    let form = screen(&mut app, 100, 30);
    assert!(form.contains("Restore from a backup location") && form.contains("WebDAV") && form.contains("SFTP"), "{form}");

    // Folder is the last kind, one step to the left of the first.
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Tab);
    typed(&mut app, &dir.path().to_string_lossy());
    press(&mut app, KeyCode::Tab);
    let root = dir.path().join("restored");
    typed(&mut app, &root.to_string_lossy());
    press(&mut app, KeyCode::Enter);
    settle(&mut app);
    let listed = screen(&mut app, 100, 30);
    assert!(listed.contains("2 copies") && listed.contains("2026-11-04 09:30 UTC"), "{listed}");
    let newest = listed.find("2026-11-04").expect("newest");
    assert!(newest < listed.find("2026-01-01").expect("older"), "newest first");

    // The older one is not a backup at all; the list stays to pick another.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    settle(&mut app);
    let refused = screen(&mut app, 100, 30);
    assert!(refused.contains("not a WM Pass backup") && refused.contains("2 copies"), "{refused}");
    assert!(!root.join("vaults").exists());

    press(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(app.overlay.is_none(), "{:?}", app.overlay);
    let Screen::Door(door) = &app.screen else { panic!("back at the door") };
    assert_eq!(door.mode, crate::door::Mode::Unlock);
    assert_eq!(door.path.text(), root.to_string_lossy());
    assert_eq!(door.focus, crate::door::Field::Password);
    assert!(screen(&mut app, 100, 30).contains("Restored"));
    assert!(matches!(engine::unlock(&root, b"master-pw", None, 9), Ok(engine::Unlock::Opened(..))), "the master password opens it");

    // A second restore into the same folder is refused, and says so.
    ctrl(&mut app, 'b');
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Tab);
    typed(&mut app, &dir.path().to_string_lossy());
    press(&mut app, KeyCode::Tab);
    typed(&mut app, &root.to_string_lossy());
    press(&mut app, KeyCode::Enter);
    settle(&mut app);
    press(&mut app, KeyCode::Enter);
    settle(&mut app);
    assert!(screen(&mut app, 100, 30).contains("already a vault"));
}

#[test]
fn a_folder_lists_only_its_own_items() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let work = engine.add_folder("Work").expect("folder");
    let workshop = engine.add_folder("Workshop").expect("folder");
    let rows = engine.list("", "title").expect("list");
    let id = |title: &str| rows.iter().find(|r| r.title == title).expect("row").id.clone();
    engine.bulk(&[id("GitHub")], "folder", json!({"folder": workshop})).expect("move");
    engine.bulk(&[id("Wi-Fi at home")], "folder", json!({"folder": work})).expect("move");
    let mut app = App::over(engine);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    ui.reload();
    // `folder:work` alone would find Workshop too.
    ui.section = ui.sections.iter().position(|s| s.label == "Work").expect("section");
    ui.refresh_list(None);
    let titles: Vec<&str> = ui.rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, vec!["Wi-Fi at home"]);
    // A reload keeps the same folder picked.
    ui.reload();
    assert_eq!(ui.sections[ui.section].label, "Work");
}

#[test]
fn pasted_text_goes_only_into_a_box_and_never_runs_as_keys() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    // On the list, with no box open, a paste of "dq" must neither trash nor quit.
    app.paste_for_test("dq");
    assert!(app.overlay.is_none(), "nothing asked to trash");
    assert!(matches!(app.screen, Screen::Vault(_)), "still open");
    assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("Nothing to paste")));

    // A confirmation is not a box either: a pasted y must not answer it.
    press(&mut app, KeyCode::Char('d'));
    assert!(matches!(app.overlay, Some(Overlay::Confirm { .. })));
    app.paste_for_test("y");
    assert!(matches!(app.overlay, Some(Overlay::Confirm { .. })), "still asking");
    press(&mut app, KeyCode::Esc);

    // The search box takes it.
    press(&mut app, KeyCode::Char('/'));
    app.paste_for_test("wi-fi");
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.search.text(), "wi-fi");
    assert_eq!(ui.rows.len(), 1);
}

#[test]
fn a_new_folder_from_the_commands_starts_empty() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    app.command(crate::app::Command::NewFolder);
    typed(&mut app, "Work");
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.folders.len(), 1);
    assert!(ui.rows.iter().all(|r| r.folder.is_none()), "the picked item stayed where it was");
}

#[test]
fn opening_an_item_that_is_gone_says_so() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(!ui.show_entry("no-such-item"), "nothing else stands in for it");
    let id = ui.rows.iter().find(|r| r.title == "GitHub").map(|r| r.id.clone()).expect("github");
    ui.engine.bulk(std::slice::from_ref(&id), "trash", json!({})).expect("trash");
    ui.reload();
    assert!(ui.show_entry(&id), "a trashed item opens in the trash");
    assert_eq!(ui.current_id().as_deref(), Some(id.as_str()));
    assert!(ui.in_trash());
}

#[test]
fn every_overlay_draws_in_the_smallest_window() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let sizes = [(40, 12), (41, 13), (47, 12), (40, 31), (220, 12), (75, 12), (76, 12), (109, 13), (110, 12)];
    let draw_all = |app: &mut App, what: &str| {
        for (w, h) in sizes {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| screen(app, w, h)))
                .unwrap_or_else(|_| panic!("{what} at {w}x{h}"));
        }
    };
    for tab in ['1', '2', '3', '4', '5'] {
        press(&mut app, KeyCode::Char(tab));
        draw_all(&mut app, "tab");
    }
    press(&mut app, KeyCode::Char('1'));
    press(&mut app, KeyCode::Enter);
    draw_all(&mut app, "detail");
    press(&mut app, KeyCode::Char('h'));
    press(&mut app, KeyCode::Char('h'));
    draw_all(&mut app, "sidebar");
    press(&mut app, KeyCode::Char('l'));
    press(&mut app, KeyCode::Char('/'));
    typed(&mut app, &"漢字".repeat(40));
    draw_all(&mut app, "a long search");
    press(&mut app, KeyCode::Esc);
    for key in ['?', ':', 'd', '+', 'm', 'e', 'n'] {
        press(&mut app, KeyCode::Char(key));
        draw_all(&mut app, &format!("overlay {key}"));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Esc);
        app.overlay = None;
    }
    app.overlay = Some(Overlay::Restore(Box::new(crate::restore::RestoreUi::new(None))));
    draw_all(&mut app, "restore");
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(Role::Send, Stage::Searching))));
    draw_all(&mut app, "link");
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(Role::Receive, Stage::Digits { code: "123456".into(), peer: "x".into() }))));
    draw_all(&mut app, "digits");
    app.overlay = None;
    ctrl(&mut app, 'l');
    draw_all(&mut app, "door");
    ctrl(&mut app, 'n');
    draw_all(&mut app, "new vault");
}

#[test]
fn locking_clears_the_message_and_the_door_hints_stay_whole() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('c'));
    assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("copied")));
    ctrl(&mut app, 'l');
    let door = screen(&mut app, 100, 30);
    assert!(!door.contains("copied"), "the copy is gone, and so is the word of it:\n{door}");
    assert!(door.contains("Esc quit") && !door.contains("Esc qui\n"), "{door}");
    // A narrow door drops whole hints rather than cutting one off part way.
    let narrow = screen(&mut app, 64, 30);
    let hints = narrow.lines().find(|l| l.contains("Enter unlock")).expect("hints");
    assert!(hints.trim_end().ends_with("quit") || hints.trim_end().ends_with("vault"), "{hints}");
    // "Locked" was about the vault left, not one being made.
    ctrl(&mut app, 'n');
    assert!(!screen(&mut app, 100, 30).contains("Locked"));
}

#[test]
fn a_message_in_the_bottom_bar_is_never_run_into_by_the_hints() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    app.say(crate::app::Level::Bad, "/nonexistent/file.csv: no such file or folder");
    let shown = screen(&mut app, 100, 20);
    let bottom = shown.lines().last().expect("bottom");
    assert!(bottom.contains("/nonexistent/file.csv: no such file or folder  "), "{bottom}");
}

#[test]
fn the_keys_list_fits_80_by_24_and_scrolls() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('?'));
    let top = screen(&mut app, 80, 24);
    assert!(top.contains("go to a tab") && top.contains("↓ more"), "{top}");
    for _ in 0..80 {
        press(&mut app, KeyCode::Down);
    }
    let end = screen(&mut app, 80, 24);
    assert!(end.contains("Settings: change, open") && end.contains("trash: restore, delete for good"), "{end}");
    assert!(matches!(app.overlay, Some(Overlay::Help { .. })), "moving keeps it open");
    press(&mut app, KeyCode::Esc);
    assert!(app.overlay.is_none());
    // Wide enough, both columns show whole, the sort key among them.
    press(&mut app, KeyCode::Char('?'));
    let wide = screen(&mut app, 120, 40);
    assert!(wide.contains("Items, Codes") || wide.contains("go to a tab"));
    assert!(wide.contains("sort the list") && wide.contains("a sidebar tag: rename, delete"), "{wide}");
}

#[test]
fn a_new_item_that_fails_to_save_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('n'));
    let Some(Overlay::Pick { items, at, .. }) = &mut app.overlay else { panic!("no pick") };
    *at = items.iter().position(|(kind, _)| kind == "otp").expect("authenticator");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "Thing");
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "not base32!!");
    ctrl(&mut app, 's');
    let shown = screen(&mut app, 100, 30);
    assert!(shown.contains("New authenticator") && !shown.contains("Edit GitHub"), "{shown}");
    assert!(matches!(app.overlay, Some(Overlay::Editor(_))), "the error keeps it open");
    app.overlay = None;
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(ui.engine.list("thing", "title").expect("list").is_empty(), "no half-made item");
}

#[test]
fn new_item_titles_keep_their_capitals() {
    assert_eq!(crate::vault::kind_in_text("ssh"), "SSH key");
    assert_eq!(crate::vault::kind_in_text("wifi"), "Wi-Fi");
    assert_eq!(crate::vault::kind_in_text("recovery"), "recovery codes");
    assert_eq!(crate::vault::kind_in_text("login"), "login");
}

#[test]
fn enter_never_says_yes_to_what_cannot_be_undone() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Enter);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    let trash = ui.sections.iter().position(|s| s.query == "trashed").expect("trash");
    ui.section = trash;
    ui.refresh_list(None);
    assert_eq!(ui.rows.len(), 1, "Enter moves to the trash, which can be undone");
    press(&mut app, KeyCode::Char('D'));
    assert!(matches!(app.overlay, Some(Overlay::Confirm { danger: true, .. })));
    press(&mut app, KeyCode::Enter);
    assert!(app.overlay.is_some(), "still asking");
    press(&mut app, KeyCode::Char('y'));
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    assert!(ui.rows.is_empty());
    assert!(screen(&mut app, 100, 20).contains("The trash is empty"));
}

#[test]
fn ctrl_c_in_an_edited_item_asks_before_quitting() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    typed(&mut app, "x");
    ctrl(&mut app, 'c');
    assert!(matches!(&app.overlay, Some(Overlay::Editor(editor)) if editor.confirm_discard), "asked");
    assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("^C again")));
    ctrl(&mut app, 'c');
    assert!(app.quitting());
    // Nothing typed: one ^C quits.
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    ctrl(&mut app, 'c');
    assert!(app.quitting());
}

#[test]
fn a_codes_filter_that_finds_nothing_says_so() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Char('/'));
    typed(&mut app, "zzz");
    let shown = screen(&mut app, 100, 20);
    assert!(shown.contains("No codes match") && !shown.contains("No one-time codes yet"), "{shown}");
}

#[test]
fn a_keepass_export_asks_for_its_password_twice() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    let out = dir.path().join("out.kdbx");
    app.command(crate::app::Command::Export);
    press(&mut app, KeyCode::Enter);
    let Some(Overlay::Prompt { input, .. }) = &mut app.overlay else { panic!("no path prompt") };
    input.set(&out.to_string_lossy());
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "correct horse");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "correct hrose");
    press(&mut app, KeyCode::Enter);
    assert!(!out.exists(), "a typo writes nothing");
    assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("do not match")));
    typed(&mut app, "correct horse");
    press(&mut app, KeyCode::Enter);
    typed(&mut app, "correct horse");
    press(&mut app, KeyCode::Enter);
    assert!(out.exists(), "{:?}", app.toast);
}

#[test]
fn a_long_list_draws_only_what_shows_and_follows_the_pick() {
    let dir = tempfile::tempdir().expect("dir");
    let mut engine = vault(dir.path());
    let mut csv = String::from("name,url,username,password\n");
    for n in 0..300 {
        csv.push_str(&format!("Site {n:03},https://s{n}.example.com,user{n},pw-{n}\n"));
    }
    engine.import_read(csv.as_bytes(), &[], None).expect("read");
    engine.import_apply().expect("apply");
    let mut app = App::over(engine);
    let Screen::Vault(ui) = &mut app.screen else { panic!("locked") };
    ui.reload();
    press(&mut app, KeyCode::End);
    let bottom = screen(&mut app, 120, 30);
    assert!(bottom.contains("Wi-Fi at home") && !bottom.contains("Site 000"), "{bottom}");
    press(&mut app, KeyCode::Home);
    let top = screen(&mut app, 120, 30);
    assert!(top.contains("GitHub") && !top.contains("Wi-Fi at home"), "{top}");
    // Walking down past the window scrolls it, and a click lands on the row drawn there.
    for _ in 0..40 {
        press(&mut app, KeyCode::Down);
    }
    let moved = screen(&mut app, 120, 30);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    let picked = ui.current().expect("row").title.clone();
    assert!(moved.contains(&picked), "{picked} in sight:\n{moved}");
    let (x, y) = find(&moved, "Site 030");
    click(&mut app, x, y);
    let Screen::Vault(ui) = &app.screen else { panic!("locked") };
    assert_eq!(ui.current().expect("row").title, "Site 030");
}

#[cfg(unix)]
#[test]
fn ctrl_z_hands_the_terminal_back_from_anywhere() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    app.key_for_test(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert!(app.suspending(), "even with the editor open");
    assert!(matches!(app.overlay, Some(Overlay::Editor(_))), "and the editor is kept for after");
    let Some(Overlay::Editor(editor)) = &app.overlay else { panic!("no editor") };
    assert!(!editor.dirty(), "no z was typed into it");
}

#[test]
fn earlier_values_keep_their_whole_date_and_messages_start_with_a_capital() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    press(&mut app, KeyCode::Char('e'));
    {
        let Some(Overlay::Editor(editor)) = &mut app.overlay else { panic!("no editor") };
        editor.at = editor.rows.iter().position(|r| r.key == "password").expect("password");
    }
    typed(&mut app, "x");
    ctrl(&mut app, 's');
    let shown = screen(&mut app, 150, 30);
    let today = shown.lines().find(|l| l.contains("Made ")).expect("made line");
    let date = today.split("Made ").nth(1).expect("date").get(..10).expect("ten").to_owned();
    assert!(shown.lines().any(|l| l.contains("Password") && l.contains(&date) && !l.contains("Made")), "{shown}");
    // A narrow pane leaves the date off rather than cutting it.
    let narrow = screen(&mut app, 120, 30);
    let year = &date[..4];
    for line in narrow.lines().filter(|l| l.contains("Password") && l.contains(year)) {
        assert!(line.contains(&date), "a date cut short: {line}");
    }
    app.say(crate::app::Level::Bad, "that password is not right");
    assert_eq!(app.toast.as_ref().map(|t| t.text.as_str()), Some("That password is not right"));
}

#[test]
fn adding_a_device_wraps_its_words_in_a_narrow_window() {
    let dir = tempfile::tempdir().expect("dir");
    let mut app = App::over(vault(dir.path()));
    app.overlay = Some(Overlay::Link(Box::new(LinkUi::at_stage(Role::Send, Stage::Searching))));
    let shown = screen(&mut app, 80, 24);
    // Every word shows, the last of each sentence included.
    assert!(shown.contains("another") && shown.contains("device.") && shown.contains("code."), "{shown}");
}

/// A master password changed on two devices at once is said on the status line once unlocked.
#[test]
fn a_password_changed_on_two_devices_at_once_is_said_on_the_status_line() {
    use wmpass_session::{Call, Service};
    let dir = tempfile::tempdir().expect("dir");
    let fast = r#"{"name":"Shared","kdf":{"m_kib":65536,"t":3,"p":1}}"#;
    let json = |reply: wmpass_session::Reply| serde_json::from_str::<serde_json::Value>(&reply.json).expect("json");
    let mut a = Service::new(1);
    let h = json(a.call("vault.create", Call { args: fast, secret: b"master", now_ms: 1000, ..Call::default() }).expect("create"))["handle"].as_u64().expect("handle");
    let file = a.call("vault.save", Call { handle: h, args: "{}", now_ms: 1000, ..Call::default() }).expect("save").blobs.remove(0);
    let mut b = Service::new(2);
    let blobs: [&[u8]; 1] = [&file];
    let hb = json(b.call("vault.open", Call { args: "{}", secret: b"master", blobs: &blobs, now_ms: 1500, ..Call::default() }).expect("open"))["handle"].as_u64().expect("handle");
    for (service, handle, new, now) in [(&mut a, h, &b"from-a"[..], 2000), (&mut b, hb, b"from-b", 3000)] {
        let found = json(service.call("vault.verify", Call { handle, args: "{}", secret: b"master", now_ms: now, ..Call::default() }).expect("verify"));
        let index = json!({"index": found["slot"]}).to_string();
        service.call("vault.change_password", Call { handle, args: &index, secret: new, now_ms: now, ..Call::default() }).expect("change");
    }
    let from_b = b.call("vault.save", Call { handle: hb, args: "{}", now_ms: 3000, ..Call::default() }).expect("save").blobs.remove(0);
    let blobs: [&[u8]; 1] = [&from_b];
    a.call("vault.merge", Call { handle: h, args: "{}", blobs: &blobs, now_ms: 4000, ..Call::default() }).expect("merge");
    let merged = a.call("vault.save", Call { handle: h, args: "{}", now_ms: 4000, ..Call::default() }).expect("save").blobs.remove(0);
    let path = dir.path().join("shared.wmvault");
    std::fs::write(&path, &merged).expect("write");

    let engine::Unlock::Opened(engine, _) = engine::unlock(&path, b"from-b", None, 3).expect("unlock") else {
        panic!("B's new password opens the merged vault");
    };
    let mut app = App::over(engine);
    let shown = screen(&mut app, 220, 30);
    assert!(shown.contains("Your master password was changed on two devices"), "{shown}");
}
