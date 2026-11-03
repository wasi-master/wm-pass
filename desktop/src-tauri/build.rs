use std::path::Path;

fn main() {
    // Tauri refuses to build at all without the relay program, which `scripts/build-host.mjs`
    // makes. Checks and tests never run it, so an empty stand-in lets them go ahead, and a release
    // build refuses the stand-in.
    let target = std::env::var("TARGET").unwrap_or_default();
    let ext = if target.contains("windows") { ".exe" } else { "" };
    let relay = format!("binaries/wm-pass-host-{target}{ext}");
    let relay = Path::new(&relay);
    if !relay.exists() {
        let _ = std::fs::create_dir_all("binaries");
        let _ = std::fs::write(relay, b"");
    }
    let empty = relay.metadata().map_or(true, |m| m.len() == 0);
    assert!(
        !(empty && std::env::var("PROFILE").as_deref() == Ok("release")),
        "the relay program is missing: run `node scripts/build-host.mjs`"
    );
    println!("cargo:rerun-if-changed=binaries");
    // App ids for the cloud places' browser sign-in, read with option_env! in signin.rs.
    for name in [
        "WMPASS_DROPBOX_APP_KEY",
        "WMPASS_ONEDRIVE_CLIENT_ID",
        "WMPASS_BOX_CLIENT_ID",
        "WMPASS_BOX_CLIENT_SECRET",
        "WMPASS_DRIVE_DESKTOP_CLIENT_ID",
        "WMPASS_DRIVE_DESKTOP_SECRET",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    tauri_build::build();
}
