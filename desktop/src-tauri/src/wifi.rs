//! The Wi-Fi networks this computer has saved, and their passwords, from the system's own store.
//!
//! Listing reads names and security only, and asks nothing of the person. A password is read one
//! network at a time, when the person picks or saves that network, because a Mac asks for an
//! administrator's password for each one. Nothing here is kept.
//!
//! - macOS: the preferred networks from `networksetup`, their passwords from the System keychain.
//! - Windows: the WLAN service's profiles. Windows hands out a key in plain text only to an
//!   administrator unless its own settings allow more.
//! - Linux: NetworkManager through `nmcli`, which asks the session's agent for secrets.

use serde::Serialize;
use zeroize::Zeroizing;

/// A saved network. `id` is what [`password`] takes back. `security` is one of the Wi-Fi item's
/// choices: open, wep, wpa, sae or eap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Network {
    pub id: String,
    pub ssid: String,
    pub security: &'static str,
    pub hidden: bool,
}

/// Every saved network with a name, by name, one per name.
pub fn list() -> Result<Vec<Network>, String> {
    let mut found = imp::list()?;
    found.retain(|network| !network.ssid.is_empty());
    found.sort_by_key(|network| network.ssid.to_lowercase());
    found.dedup_by(|a, b| a.ssid == b.ssid);
    Ok(found)
}

/// One network's password, empty for an open network.
pub fn password(id: &str) -> Result<Zeroizing<String>, String> {
    imp::password(id)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::collections::HashSet;
    use std::process::Command;

    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework::os::macos::passwords::find_generic_password;
    use zeroize::Zeroizing;

    use super::Network;

    const SYSTEM: &str = "/Library/Keychains/System.keychain";
    const SERVICE: &str = "AirPort";

    pub fn list() -> Result<Vec<Network>, String> {
        let with_password = keychain_accounts();
        let names = match preferred() {
            Some(names) => names,
            // Without a Wi-Fi port to ask about, the keychain still knows the networks with a password.
            None => with_password.iter().cloned().collect(),
        };
        // The Mac does not say which WPA a network uses, and WPA covers both for sharing.
        Ok(names
            .into_iter()
            .map(|ssid| Network {
                security: if with_password.contains(&ssid) { "wpa" } else { "open" },
                id: ssid.clone(),
                ssid,
                hidden: false,
            })
            .collect())
    }

    /// macOS asks for an administrator's name and password before it hands this over.
    pub fn password(ssid: &str) -> Result<Zeroizing<String>, String> {
        let keychain = SecKeychain::open(SYSTEM).map_err(|e| format!("The System keychain could not be opened: {e}"))?;
        match find_generic_password(Some(&[keychain]), SERVICE, ssid) {
            Ok((secret, _)) => Ok(Zeroizing::new(String::from_utf8_lossy(&secret).into_owned())),
            Err(e) if e.code() == -25300 => Ok(Zeroizing::new(String::new())),
            Err(e) if e.code() == -128 => Err("macOS was not given permission.".to_owned()),
            Err(e) => Err(format!("macOS did not hand over the password: {e}")),
        }
    }

    /// The networks this Mac joins by itself, from its Wi-Fi port's list.
    fn preferred() -> Option<Vec<String>> {
        let ports = Command::new("/usr/sbin/networksetup").arg("-listallhardwareports").output().ok()?;
        let ports = String::from_utf8_lossy(&ports.stdout).into_owned();
        let device = wifi_device(&ports)?;
        let listed = Command::new("/usr/sbin/networksetup").args(["-listpreferredwirelessnetworks", &device]).output().ok()?;
        listed.status.success().then(|| preferred_names(&String::from_utf8_lossy(&listed.stdout)))
    }

    /// Reading attributes asks nothing; only a password's data needs permission.
    fn keychain_accounts() -> HashSet<String> {
        let Ok(keychain) = SecKeychain::open(SYSTEM) else { return HashSet::new() };
        let Ok(found) = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .keychains(&[keychain])
            .service(SERVICE)
            .load_attributes(true)
            .limit(Limit::All)
            .search()
        else {
            return HashSet::new();
        };
        found.iter().filter_map(|item| item.simplify_dict()?.get("acct").cloned()).collect()
    }

    pub(super) fn wifi_device(ports: &str) -> Option<String> {
        let mut lines = ports.lines();
        while let Some(line) = lines.next() {
            let Some(port) = line.strip_prefix("Hardware Port: ") else { continue };
            if port == "Wi-Fi" || port == "AirPort" {
                return lines.next()?.strip_prefix("Device: ").map(|device| device.trim().to_owned());
            }
        }
        None
    }

    pub(super) fn preferred_names(text: &str) -> Vec<String> {
        text.lines().filter_map(|line| line.strip_prefix('\t')).map(str::to_owned).collect()
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::ptr;

    use libloading::{Library, Symbol};
    use zeroize::Zeroizing;

    use super::{Network, xml};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct InterfaceInfo {
        guid: Guid,
        description: [u16; 256],
        state: i32,
    }

    #[repr(C)]
    struct InterfaceList {
        count: u32,
        index: u32,
        items: [InterfaceInfo; 1],
    }

    #[repr(C)]
    struct ProfileInfo {
        name: [u16; 256],
        flags: u32,
    }

    #[repr(C)]
    struct ProfileList {
        count: u32,
        index: u32,
        items: [ProfileInfo; 1],
    }

    type Handle = *mut c_void;
    type OpenHandle = unsafe extern "system" fn(u32, *const c_void, *mut u32, *mut Handle) -> u32;
    type CloseHandle = unsafe extern "system" fn(Handle, *const c_void) -> u32;
    type EnumInterfaces = unsafe extern "system" fn(Handle, *const c_void, *mut *mut InterfaceList) -> u32;
    type GetProfileList = unsafe extern "system" fn(Handle, *const Guid, *const c_void, *mut *mut ProfileList) -> u32;
    type GetProfile = unsafe extern "system" fn(Handle, *const Guid, *const u16, *const c_void, *mut *mut u16, *mut u32, *mut u32) -> u32;
    type FreeMemory = unsafe extern "system" fn(*const c_void);

    const PLAINTEXT_KEY: u32 = 4;
    const SERVICE_NOT_ACTIVE: u32 = 1062;

    /// One profile on one Wi-Fi adapter.
    struct Profile {
        guid: Guid,
        name: Vec<u16>,
    }

    /// The WLAN service, opened from its library at run time: Windows without the wireless
    /// service, such as some servers, has no `wlanapi.dll` and the app must still start.
    struct Wlan {
        library: Library,
        handle: Handle,
    }

    impl Wlan {
        fn open() -> Result<Self, String> {
            // SAFETY: wlanapi.dll is a system library with no initialisation of its own.
            let library = unsafe { Library::new("wlanapi.dll") }.map_err(|_| "This computer has no Wi-Fi service.".to_owned())?;
            let mut handle = ptr::null_mut();
            let mut version = 0;
            // SAFETY: the signature is WlanOpenHandle's, and both out-pointers are live.
            let status = unsafe {
                let open: Symbol<OpenHandle> = library.get(b"WlanOpenHandle\0").map_err(|e| e.to_string())?;
                open(2, ptr::null(), &mut version, &mut handle)
            };
            match status {
                0 => Ok(Self { library, handle }),
                SERVICE_NOT_ACTIVE => Err("The WLAN AutoConfig service is not running.".to_owned()),
                code => Err(format!("Windows did not open its Wi-Fi service (error {code}).")),
            }
        }

        fn symbol<T>(&self, name: &[u8]) -> Result<Symbol<'_, T>, String> {
            // SAFETY: each caller names the type of the function it asks for.
            unsafe { self.library.get(name) }.map_err(|e| e.to_string())
        }

        fn free(&self, memory: *const c_void) {
            if let Ok(free) = self.symbol::<FreeMemory>(b"WlanFreeMemory\0") {
                // SAFETY: the memory came from the WLAN service and is freed once.
                unsafe { free(memory) }
            }
        }

        fn profiles(&self) -> Result<Vec<Profile>, String> {
            let enumerate = self.symbol::<EnumInterfaces>(b"WlanEnumInterfaces\0")?;
            let get_list = self.symbol::<GetProfileList>(b"WlanGetProfileList\0")?;
            let mut interfaces = ptr::null_mut();
            // SAFETY: the out-pointer is live; the list is freed below.
            let status = unsafe { enumerate(self.handle, ptr::null(), &mut interfaces) };
            if status != 0 || interfaces.is_null() {
                return Err(format!("Windows did not list its Wi-Fi adapters (error {status})."));
            }
            let mut found = Vec::new();
            // SAFETY: the service wrote `count` items after the header.
            let guids: Vec<Guid> = unsafe {
                let count = (*interfaces).count as usize;
                std::slice::from_raw_parts(ptr::addr_of!((*interfaces).items).cast::<InterfaceInfo>(), count).iter().map(|i| i.guid).collect()
            };
            self.free(interfaces.cast());
            for guid in guids {
                let mut list = ptr::null_mut();
                // SAFETY: as above.
                if unsafe { get_list(self.handle, &guid, ptr::null(), &mut list) } != 0 || list.is_null() {
                    continue;
                }
                // SAFETY: as above.
                unsafe {
                    let count = (*list).count as usize;
                    for info in std::slice::from_raw_parts(ptr::addr_of!((*list).items).cast::<ProfileInfo>(), count) {
                        let end = info.name.iter().position(|&c| c == 0).unwrap_or(info.name.len());
                        let mut name = info.name[..end].to_vec();
                        name.push(0);
                        found.push(Profile { guid, name });
                    }
                }
                self.free(list.cast());
            }
            Ok(found)
        }

        /// The profile's XML. With `plain`, the key comes unprotected if Windows allows it.
        fn xml(&self, profile: &Profile, plain: bool) -> Result<Zeroizing<String>, String> {
            let get = self.symbol::<GetProfile>(b"WlanGetProfile\0")?;
            let mut text = ptr::null_mut();
            let mut flags = if plain { PLAINTEXT_KEY } else { 0 };
            let mut access = 0;
            // SAFETY: the name ends in a zero; the out-pointers are live; the text is freed below.
            let status = unsafe { get(self.handle, &profile.guid, profile.name.as_ptr(), ptr::null(), &mut text, &mut flags, &mut access) };
            if status != 0 || text.is_null() {
                return Err(format!("Windows did not read that network (error {status})."));
            }
            // SAFETY: the service wrote a zero-terminated UTF-16 string.
            let xml = unsafe {
                let mut length = 0;
                while *text.add(length) != 0 {
                    length += 1;
                }
                Zeroizing::new(String::from_utf16_lossy(std::slice::from_raw_parts(text, length)))
            };
            // SAFETY: the buffer may hold the key, so it is wiped before it goes back.
            unsafe {
                let mut at = 0;
                while *text.add(at) != 0 {
                    *text.add(at) = 0;
                    at += 1;
                }
            }
            self.free(text.cast());
            Ok(xml)
        }
    }

    impl Drop for Wlan {
        fn drop(&mut self) {
            if let Ok(close) = self.symbol::<CloseHandle>(b"WlanCloseHandle\0") {
                // SAFETY: the handle was opened by this value and is closed once.
                unsafe { close(self.handle, ptr::null()) };
            }
        }
    }

    /// A profile's id: the adapter and the profile's own name, which need not be the network's.
    fn id_of(profile: &Profile) -> String {
        let g = profile.guid;
        let name = String::from_utf16_lossy(&profile.name[..profile.name.len() - 1]);
        format!("{:08x}-{:04x}-{:04x}-{}/{name}", g.data1, g.data2, g.data3, g.data4.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }

    pub fn list() -> Result<Vec<Network>, String> {
        let wlan = Wlan::open()?;
        let mut found = Vec::new();
        for profile in wlan.profiles()? {
            let Ok(text) = wlan.xml(&profile, false) else { continue };
            if let Some(mut network) = xml::network(&text) {
                network.id = id_of(&profile);
                found.push(network);
            }
        }
        Ok(found)
    }

    pub fn password(id: &str) -> Result<Zeroizing<String>, String> {
        let wlan = Wlan::open()?;
        let profile = wlan.profiles()?.into_iter().find(|p| id_of(p) == id).ok_or("That network is no longer saved.")?;
        let text = wlan.xml(&profile, true)?;
        xml::key(&text)
    }
}

/// Windows describes a profile in XML. Only a few plain elements are read, so a small reader does.
#[cfg_attr(not(windows), allow(dead_code))]
mod xml {
    use zeroize::Zeroizing;

    use super::Network;

    /// The text of the first `<name>` element inside `within`, if any.
    fn element<'a>(text: &'a str, name: &str) -> Option<&'a str> {
        let open = format!("<{name}>");
        let start = text.find(&open)? + open.len();
        let end = text[start..].find(&format!("</{name}>"))? + start;
        Some(&text[start..end])
    }

    fn unescape(text: &str) -> String {
        text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
    }

    fn from_hex(text: &str) -> Option<String> {
        let bytes: Option<Vec<u8>> = (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect();
        Some(String::from_utf8_lossy(&bytes?).into_owned())
    }

    /// The network a profile joins. An id is filled in by the caller.
    pub fn network(text: &str) -> Option<Network> {
        let ssid_config = element(text, "SSID")?;
        let ssid = match element(ssid_config, "name") {
            Some(name) => unescape(name),
            None => from_hex(element(ssid_config, "hex")?)?,
        };
        let auth = element(text, "authentication").unwrap_or("open");
        let encryption = element(text, "encryption").unwrap_or("none");
        let security = match auth {
            "WPA3SAE" => "sae",
            "WPAPSK" | "WPA2PSK" => "wpa",
            "shared" => "wep",
            "open" if encryption.eq_ignore_ascii_case("WEP") => "wep",
            "open" | "OWE" => "open",
            _ => "eap",
        };
        Some(Network { id: String::new(), ssid, security, hidden: element(text, "nonBroadcast") == Some("true") })
    }

    /// The key in a profile read with its key in plain text.
    pub fn key(text: &str) -> Result<Zeroizing<String>, String> {
        let Some(shared) = element(text, "sharedKey") else { return Ok(Zeroizing::new(String::new())) };
        if element(shared, "protected") == Some("true") {
            return Err("Windows gives saved Wi-Fi passwords only to programs run as an administrator. Run WM Pass as an administrator once to bring them over.".to_owned());
        }
        Ok(Zeroizing::new(unescape(element(shared, "keyMaterial").unwrap_or(""))))
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::process::Command;

    use zeroize::Zeroizing;

    use super::Network;

    fn nmcli(args: &[&str]) -> Result<Zeroizing<String>, String> {
        let out = Command::new("nmcli").env("LC_ALL", "C").args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "WM Pass reads saved networks from NetworkManager, which this computer does not have.".to_owned()
            } else {
                format!("nmcli did not start: {e}")
            }
        })?;
        let text = Zeroizing::new(String::from_utf8_lossy(&out.stdout).into_owned());
        let mut stdout = out.stdout;
        zeroize::Zeroize::zeroize(&mut stdout);
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr);
            return Err(format!("NetworkManager said: {}", why.trim().trim_start_matches("Error: ")));
        }
        Ok(text)
    }

    /// `field:value` lines, as `nmcli -t -m multiline` prints one connection.
    pub(super) fn field<'a>(text: &'a str, name: &str) -> &'a str {
        text.lines().find_map(|line| line.strip_prefix(name)?.strip_prefix(':')).unwrap_or("")
    }

    /// NetworkManager's key management. `none` is static WEP; no security section at all is open.
    pub(super) fn security(key_mgmt: &str) -> &'static str {
        match key_mgmt {
            "" | "owe" => "open",
            "none" => "wep",
            "wpa-psk" => "wpa",
            "sae" => "sae",
            _ => "eap",
        }
    }

    const FIELDS: &str = "802-11-wireless.ssid,802-11-wireless.hidden,802-11-wireless-security.key-mgmt";

    pub fn list() -> Result<Vec<Network>, String> {
        let all = nmcli(&["-t", "-f", "UUID,TYPE", "connection", "show"])?;
        let mut found = Vec::new();
        for line in all.lines() {
            let Some((uuid, kind)) = line.split_once(':') else { continue };
            if kind != "802-11-wireless" {
                continue;
            }
            let Ok(one) = nmcli(&["--escape", "no", "-t", "-m", "multiline", "-f", FIELDS, "connection", "show", "uuid", uuid]) else { continue };
            found.push(Network {
                id: uuid.to_owned(),
                ssid: field(&one, "802-11-wireless.ssid").to_owned(),
                security: security(field(&one, "802-11-wireless-security.key-mgmt")),
                hidden: field(&one, "802-11-wireless.hidden") == "yes",
            });
        }
        Ok(found)
    }

    pub fn password(uuid: &str) -> Result<Zeroizing<String>, String> {
        let secrets = "802-11-wireless-security.key-mgmt,802-11-wireless-security.psk,802-11-wireless-security.wep-tx-keyidx,802-11-wireless-security.wep-key0,802-11-wireless-security.wep-key1,802-11-wireless-security.wep-key2,802-11-wireless-security.wep-key3";
        let one = nmcli(&["--show-secrets", "--escape", "no", "-t", "-m", "multiline", "-f", secrets, "connection", "show", "uuid", uuid])?;
        let key = match field(&one, "802-11-wireless-security.key-mgmt") {
            "wpa-psk" | "sae" => field(&one, "802-11-wireless-security.psk"),
            "none" => {
                let index = field(&one, "802-11-wireless-security.wep-tx-keyidx").parse::<u8>().unwrap_or(0).min(3);
                field(&one, &format!("802-11-wireless-security.wep-key{index}"))
            }
            _ => return Ok(Zeroizing::new(String::new())),
        };
        if key.is_empty() {
            return Err("NetworkManager did not hand over this password. It may be kept in a keyring that is locked, or need an administrator to allow it.".to_owned());
        }
        Ok(Zeroizing::new(key.to_owned()))
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod imp {
    use zeroize::Zeroizing;

    use super::Network;

    pub fn list() -> Result<Vec<Network>, String> {
        Err("WM Pass cannot read saved networks on this system.".to_owned())
    }

    pub fn password(_: &str) -> Result<Zeroizing<String>, String> {
        Err("WM Pass cannot read saved networks on this system.".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::xml;

    const PROFILE: &str = r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
	<name>Home profile</name>
	<SSIDConfig><SSID><hex>43616665</hex><name>Caf&amp;e</name></SSID><nonBroadcast>true</nonBroadcast></SSIDConfig>
	<MSM><security><authEncryption><authentication>WPA2PSK</authentication><encryption>AES</encryption></authEncryption>
	<sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>a&lt;b&amp;c</keyMaterial></sharedKey></security></MSM>
</WLANProfile>"#;

    #[test]
    fn reads_a_windows_profile() {
        let network = xml::network(PROFILE).expect("a network");
        assert_eq!(network.ssid, "Caf&e");
        assert_eq!(network.security, "wpa");
        assert!(network.hidden);
        assert_eq!(xml::key(PROFILE).expect("a key").as_str(), "a<b&c");
    }

    #[test]
    fn a_protected_key_is_refused() {
        let text = PROFILE.replace("<protected>false</protected>", "<protected>true</protected>");
        assert!(xml::key(&text).is_err());
    }

    #[test]
    fn an_ssid_only_in_hex_is_read() {
        let text = PROFILE.replace("<name>Caf&amp;e</name>", "");
        assert_eq!(xml::network(&text).expect("a network").ssid, "Cafe");
    }

    #[test]
    fn open_and_wep_and_enterprise() {
        let open = PROFILE.replace("WPA2PSK", "open").replace("AES", "none");
        assert_eq!(xml::network(&open).expect("open").security, "open");
        let wep = PROFILE.replace("WPA2PSK", "open").replace("AES", "WEP");
        assert_eq!(xml::network(&wep).expect("wep").security, "wep");
        let eap = PROFILE.replace("WPA2PSK", "WPA2");
        assert_eq!(xml::network(&eap).expect("eap").security, "eap");
        let sae = PROFILE.replace("WPA2PSK", "WPA3SAE");
        assert_eq!(xml::network(&sae).expect("sae").security, "sae");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_networksetup() {
        let ports = "\nHardware Port: Ethernet\nDevice: en3\nEthernet Address: x\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: y\n";
        assert_eq!(super::imp::wifi_device(ports).as_deref(), Some("en0"));
        let list = "Preferred networks on en0:\n\tHome\n\tCafe 5G\n";
        assert_eq!(super::imp::preferred_names(list), ["Home", "Cafe 5G"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_nmcli() {
        let text = "802-11-wireless.ssid:Cafe: upstairs\n802-11-wireless.hidden:yes\n802-11-wireless-security.key-mgmt:wpa-psk\n";
        assert_eq!(super::imp::field(text, "802-11-wireless.ssid"), "Cafe: upstairs");
        assert_eq!(super::imp::field(text, "802-11-wireless.hidden"), "yes");
        assert_eq!(super::imp::security("wpa-psk"), "wpa");
        assert_eq!(super::imp::security(""), "open");
    }
}
