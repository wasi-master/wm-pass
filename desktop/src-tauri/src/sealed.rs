//! Small things that are not vault data but are not for anyone else either: the logins for backup
//! places and this computer's network identity. Sealed under a key kept in the system's keychain.
//! Where a session has no keychain, the key sits in a file only this user can read, like an
//! SSH key with no passphrase.

use keyring::Entry;
use wmpass_core::crypto;
use zeroize::Zeroizing;

use crate::store::Store;

const SERVICE: &str = "com.wasimaster.wmpass.desktop";
const KEY_USER: &str = "sealing-key";
const AAD: &[u8] = b"wmpass-desktop-sealed-v1";

fn keychain_allowed() -> bool {
    // A debug build under test must not make the system ask the owner anything.
    !(cfg!(debug_assertions) && std::env::var_os("WMPASS_NO_KEYCHAIN").is_some())
}

fn key(store: &Store) -> Result<Zeroizing<[u8; 32]>, String> {
    if keychain_allowed()
        && let Ok(entry) = Entry::new(SERVICE, KEY_USER)
    {
        match entry.get_secret() {
            Ok(raw) => {
                if let Ok(found) = <[u8; 32]>::try_from(raw.as_slice()) {
                    return Ok(Zeroizing::new(found));
                }
            }
            Err(keyring::Error::NoEntry) => {
                let made: [u8; 32] = crypto::random_array().map_err(|e| e.to_string())?;
                if entry.set_secret(&made).is_ok() {
                    return Ok(Zeroizing::new(made));
                }
            }
            Err(_) => {}
        }
    }
    let path = "sealed/key";
    if let Ok(raw) = store.read(path)
        && let Ok(found) = <[u8; 32]>::try_from(raw.as_slice())
    {
        return Ok(Zeroizing::new(found));
    }
    let made: [u8; 32] = crypto::random_array().map_err(|e| e.to_string())?;
    store.write(path, &made)?;
    Ok(Zeroizing::new(made))
}

pub fn load(store: &Store, name: &str) -> Result<Option<String>, String> {
    let Ok(sealed) = store.read(&format!("sealed/{name}.bin")) else {
        return Ok(None);
    };
    let key = key(store)?;
    let plain = crypto::open(&key, &sealed, AAD).ok_or("that could not be unsealed")?;
    Ok(Some(String::from_utf8_lossy(&plain).into_owned()))
}

pub fn save(store: &Store, name: &str, text: &str) -> Result<(), String> {
    let key = key(store)?;
    let sealed = crypto::seal(&key, text.as_bytes(), AAD).map_err(|e| e.to_string())?;
    store.write(&format!("sealed/{name}.bin"), &sealed)
}

fn quick_entry(file: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, &format!("vault-{file}")).map_err(|e| e.to_string())
}

/// Linux only, and off by default: any program in the session can ask the keyring for this.
pub fn quick_available() -> bool {
    cfg!(target_os = "linux") && Entry::new(SERVICE, "probe").is_ok_and(|e| !matches!(e.get_secret(), Err(keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_))))
}

pub fn quick_enrolled(file: &str) -> bool {
    quick_available() && quick_entry(file).is_ok_and(|e| e.get_secret().is_ok())
}

pub fn quick_store(file: &str, key: &[u8]) -> Result<(), String> {
    quick_entry(file)?.set_secret(key).map_err(|e| e.to_string())
}

pub fn quick_read(file: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    quick_entry(file)?.get_secret().map(Zeroizing::new).map_err(|e| e.to_string())
}

pub fn quick_forget(file: &str) -> Result<(), String> {
    match quick_entry(file)?.delete_credential() {
        Err(keyring::Error::NoEntry) | Ok(()) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real keychain, so it is run by hand: `cargo test -p wmpass-desktop -- --ignored`.
    #[test]
    #[ignore = "uses the system keychain"]
    fn the_keychain_keeps_a_secret() {
        let entry = Entry::new(SERVICE, "wmpass-test-entry").expect("entry");
        entry.set_secret(&[1, 2, 3]).expect("set");
        assert_eq!(entry.get_secret().expect("get"), vec![1, 2, 3]);
        entry.delete_credential().expect("delete");
        assert!(matches!(entry.get_secret(), Err(keyring::Error::NoEntry)));
    }
}
