//! The menu bar. On a Mac it is what carries the shortcuts, and it has to exist for copy and paste
//! to reach the page at all. Elsewhere shortcuts stay in the page, so nothing fires twice.

use std::collections::HashMap;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Runtime};

fn accelerator(chord: &str) -> Option<String> {
    if !cfg!(target_os = "macos") || chord.is_empty() {
        return None;
    }
    Some(
        chord
            .split('+')
            .map(|part| match part {
                "Mod" => "Cmd",
                other => other,
            })
            .collect::<Vec<_>>()
            .join("+"),
    )
}

pub fn build<R: Runtime>(app: &AppHandle<R>, shortcuts: &HashMap<String, String>, unlocked: bool) -> tauri::Result<Menu<R>> {
    let item = |id: &str, label: &str, enabled: bool| -> tauri::Result<MenuItem<R>> {
        let chord = shortcuts.get(id).and_then(|chord| accelerator(chord));
        MenuItem::with_id(app, format!("cmd:{id}"), label, enabled, chord)
    };
    let separator = || PredefinedMenuItem::separator(app);

    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &item("add", "New Item", unlocked)?,
            &separator()?,
            &item("lock", "Lock", unlocked)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &separator()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let entry = Submenu::with_items(
        app,
        "Item",
        unlocked,
        &[
            &item("edit", "Edit", unlocked)?,
            &separator()?,
            &item("copy_password", "Copy Password", unlocked)?,
            &item("copy_username", "Copy Username", unlocked)?,
            &item("copy_code", "Copy Code", unlocked)?,
            &item("open_site", "Open Site", unlocked)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        unlocked,
        &[
            &item("search", "Search", unlocked)?,
            &separator()?,
            &item("codes", "Codes", unlocked)?,
            &item("generator", "Password Generator", unlocked)?,
            &item("health", "Health", unlocked)?,
            &separator()?,
            &item("toggle_sidebar", "Show or Hide Sidebar", unlocked)?,
        ],
    )?;
    let window = Submenu::with_items(app, "Window", true, &[&PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::fullscreen(app, None)?])?;
    let help = Submenu::with_items(app, "Help", true, &[&MenuItem::with_id(app, "source", "Source Code", true, None::<&str>)?])?;

    #[cfg(target_os = "macos")]
    {
        let settings = item("settings", "Settings…", unlocked)?;
        let about = PredefinedMenuItem::about(app, Some("About WM Pass"), None)?;
        let application = Submenu::with_items(
            app,
            "WM Pass",
            true,
            &[
                &about,
                &separator()?,
                &settings,
                &separator()?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &separator()?,
                &MenuItem::with_id(app, "quit", "Quit WM Pass", true, Some("Cmd+Q"))?,
            ],
        )?;
        Menu::with_items(app, &[&application, &file, &edit, &entry, &view, &window, &help])
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.append(&item("settings", "Settings", unlocked)?)?;
        file.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
        Menu::with_items(app, &[&file, &edit, &entry, &view, &window, &help])
    }
}

/// What a menu item, wherever it is, asks for.
pub fn handle<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if let Some(command) = id.strip_prefix("cmd:") {
        let _ = app.emit("command", command);
    } else if let Some(choice) = id.strip_prefix("ctx:") {
        let _ = app.emit("menu-choice", choice);
    } else if id == "quit" {
        app.exit(0);
    } else if id == "source" {
        crate::open_in_browser("https://github.com/wasi-master/wm-pass");
    }
}
