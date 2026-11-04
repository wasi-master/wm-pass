//! Backup copies at a place: how every app names them, which files in a listing are copies, and
//! what is inside one.
//!
//! A copy is one zip of stored entries: `vaults/<name>` for each vault file and decoy,
//! `linked/<name>` for linked vaults and `blobs/<vault>/<name>` for sealed attachments. All of
//! it is ciphertext, so only the master password decides what a copy opens to. This is the
//! same format the shared web code (`ui/src/engine/bundle.ts`) and the phone app write.

use crate::Stored;

/// What every copy's name starts with.
pub const PREFIX: &str = "wmpass-backup-";
/// What every copy's name ends with.
pub const EXTENSION: &str = ".wmbackup";

/// The first bytes of a lone vault file.
const VAULT_MAGIC: &[u8; 4] = b"WMPV";

/// Whether a file at a place is a backup copy.
pub fn is_copy(name: &str) -> bool {
    name.starts_with(PREFIX) && name.ends_with(EXTENSION)
}

/// The copies in a listing, newest first. Names carry a UTC time stamp, so they sort by name.
pub fn copies(files: Vec<Stored>) -> Vec<Stored> {
    let mut out: Vec<Stored> = files.into_iter().filter(|f| is_copy(&f.name)).collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// When a copy was made, read from its name, as `2026-11-04 09:30 UTC`.
pub fn made_at(name: &str) -> Option<String> {
    let stamp = name.strip_prefix(PREFIX)?.strip_suffix(EXTENSION)?;
    let digits: Vec<char> = stamp.chars().collect();
    let ok = digits.len() == 15 && digits[8] == '-' && digits.iter().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit());
    if !ok {
        return None;
    }
    let part = |from: usize, to: usize| digits[from..to].iter().collect::<String>();
    Some(format!("{}-{}-{} {}:{} UTC", part(0, 4), part(4, 6), part(6, 8), part(9, 11), part(11, 13)))
}

/// A name for a copy made at `unix_seconds`.
pub fn copy_name(unix_seconds: u64) -> String {
    let days = unix_seconds / 86_400;
    let secs = unix_seconds % 86_400;
    let (y, m, d) = civil(i64::try_from(days).unwrap_or(0));
    format!("{PREFIX}{y:04}{m:02}{d:02}-{:02}{:02}{:02}{EXTENSION}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Days since 1970 to a calendar date (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Whether `bytes` are a lone vault file rather than a copy.
pub fn is_vault_file(bytes: &[u8]) -> bool {
    bytes.len() > 4 && bytes.starts_with(VAULT_MAGIC)
}

/// One file inside a copy.
#[derive(Clone, PartialEq, Eq)]
pub struct BundleFile {
    /// Where it goes, such as `vaults/0a1b…`.
    pub name: String,
    /// Its bytes, all ciphertext.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for BundleFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BundleFile").field("name", &self.name).field("len", &self.bytes.len()).finish()
    }
}

/// Whether a name is one a copy may hold. An entry called `../../x` must go nowhere.
pub fn name_is_fine(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == ".." || p.starts_with('.') || p.contains('\\') || p.contains(':')) {
        return false;
    }
    match parts[0] {
        "vaults" | "linked" => parts.len() == 2,
        "blobs" => parts.len() == 3,
        _ => false,
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?)))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()
}

/// The largest single entry a copy may hold.
const MAX_ENTRY: usize = 1 << 31;

/// The files in a copy, with hostile names left out. `None` when it is not a zip, or holds no
/// vault file, so is not a WM Pass backup.
pub fn unbundle(bytes: &[u8]) -> Option<Vec<BundleFile>> {
    let lowest = bytes.len().saturating_sub(66_000);
    let end = (lowest..=bytes.len().checked_sub(22)?).rev().find(|&i| u32_at(bytes, i) == Some(0x0605_4b50))?;
    let count = u16_at(bytes, end + 10)?;
    let mut at = u32_at(bytes, end + 16)?;
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(bytes, at) != Some(0x0201_4b50) {
            break;
        }
        let method = u16_at(bytes, at + 10)?;
        let packed = u32_at(bytes, at + 20)?;
        let plain = u32_at(bytes, at + 24)?;
        let name_len = u16_at(bytes, at + 28)?;
        let extra = u16_at(bytes, at + 30)?;
        let comment = u16_at(bytes, at + 32)?;
        let offset = u32_at(bytes, at + 42)?;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len)?).into_owned();
        at += 46 + name_len + extra + comment;
        if !name_is_fine(&name) || plain > MAX_ENTRY {
            continue;
        }
        let Some(start) = (|| Some(offset + 30 + u16_at(bytes, offset + 26)? + u16_at(bytes, offset + 28)?))() else { continue };
        let Some(stored) = bytes.get(start..start.checked_add(packed)?) else { continue };
        let file = match method {
            0 => stored.to_vec(),
            8 => match miniz_oxide::inflate::decompress_to_vec_with_limit(stored, plain) {
                Ok(inflated) => inflated,
                Err(_) => continue,
            },
            _ => continue,
        };
        out.push(BundleFile { name, bytes: file });
    }
    out.iter().any(|f| f.name.starts_with("vaults/")).then_some(out)
}

/// The DOS time and date every entry carries, so a copy says nothing about when files changed:
/// 2020-09-13 12:26:40 UTC, the moment the apps set vault files to.
const FILE_TIME: (u16, u16) = ((12 << 11) | (26 << 5) | (40 >> 1), ((2020 - 1980) << 9) | (9 << 5) | 13);

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for b in bytes {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

/// Makes a copy of stored entries, as the other apps do.
#[allow(clippy::cast_possible_truncation)]
pub fn bundle(files: &[BundleFile]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for file in files {
        let offset = out.len() as u32;
        let crc = crc32(&file.bytes);
        let len = file.bytes.len() as u32;
        let name = file.name.as_bytes();
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&FILE_TIME.0.to_le_bytes());
        out.extend_from_slice(&FILE_TIME.1.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name);
        out.extend_from_slice(&file.bytes);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&FILE_TIME.0.to_le_bytes());
        central.extend_from_slice(&FILE_TIME.1.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let start = out.len() as u32;
    let size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, bytes: &[u8]) -> BundleFile {
        BundleFile { name: name.to_owned(), bytes: bytes.to_vec() }
    }

    fn stored(name: &str) -> Stored {
        Stored { id: name.to_owned(), name: name.to_owned(), size: 1 }
    }

    #[test]
    fn copies_are_ours_only_and_newest_first() {
        let listed = vec![
            stored("wmpass-backup-20250101-000000.wmbackup"),
            stored("notes.txt"),
            stored("wmpass-backup-20261104-093000.wmbackup"),
            stored("device.wmsync"),
            stored("wmpass-backup-20260301-120000.wmbackup"),
        ];
        let names: Vec<String> = copies(listed).into_iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            ["wmpass-backup-20261104-093000.wmbackup", "wmpass-backup-20260301-120000.wmbackup", "wmpass-backup-20250101-000000.wmbackup"]
        );
        assert_eq!(made_at(&names[0]).as_deref(), Some("2026-11-04 09:30 UTC"));
        assert_eq!(made_at("wmpass-backup-x.wmbackup"), None);
    }

    #[test]
    fn a_name_made_here_is_one_the_other_apps_make() {
        // 2026-11-04 09:30:05 UTC.
        assert_eq!(copy_name(1_793_784_605), "wmpass-backup-20261104-093005.wmbackup");
        assert_eq!(copy_name(0), "wmpass-backup-19700101-000000.wmbackup");
        assert!(is_copy(&copy_name(1_600_000_000)));
    }

    #[test]
    fn a_copy_round_trips_and_hostile_names_go_nowhere() {
        let files = vec![file("vaults/aa", b"WMPV-one"), file("vaults/bb", b"chaff"), file("blobs/aa/x1", b"sealed")];
        let mut all = files.clone();
        for hostile in ["../../etc/passwd", "vaults/../x", "vaults/.hidden", "blobs/aa", "other/x", "vaults/a\\b", "/vaults/x"] {
            all.push(file(hostile, b"evil"));
        }
        let back = unbundle(&bundle(&all)).expect("a backup");
        assert_eq!(back, files);
    }

    #[test]
    fn a_deflated_entry_reads_too() {
        // What a zip tool makes when it compresses: method 8, raw deflate.
        let plain = b"WMPV".repeat(100);
        let packed = miniz_oxide::deflate::compress_to_vec(&plain, 6);
        let mut zip = bundle(&[file("vaults/aa", &packed)]);
        // Mark it deflated and give its real size, in the central directory.
        let central = zip.windows(4).position(|w| w == 0x0201_4b50u32.to_le_bytes()).expect("central");
        zip[central + 10] = 8;
        zip[central + 24..central + 28].copy_from_slice(&u32::try_from(plain.len()).expect("len").to_le_bytes());
        assert_eq!(unbundle(&zip).expect("a backup")[0].bytes, plain);
    }

    #[test]
    fn something_else_is_not_a_backup() {
        assert!(unbundle(b"").is_none());
        assert!(unbundle(b"WMPV and then some").is_none());
        assert!(unbundle(&bundle(&[file("linked/aa", b"x")])).is_none(), "no vault file in it");
        assert!(is_vault_file(b"WMPV\x01\x02") && !is_vault_file(b"PK\x03\x04"));
    }
}
