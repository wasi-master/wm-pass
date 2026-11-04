//! Finding devices on the network. An advert holds a random name and a port, and nothing
//! about the owner, the vault or the device, except that a device with no vault yet says it
//! is waiting to join one. Whether a found address is a paired device
//! is settled by the handshake, never by the advert. On Android, the caller holds a
//! `MulticastLock` while this runs.

use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use wmpass_core::crypto;
use wmpass_core::vault::hex;

use crate::{LanError, Result};

const SERVICE: &str = "_wmpass._tcp.local.";

/// A running advert. Dropping it withdraws the advert.
pub struct Advert {
    daemon: ServiceDaemon,
    fullname: String,
}

impl std::fmt::Debug for Advert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Advert").finish_non_exhaustive()
    }
}

impl Drop for Advert {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// What an advert from a device waiting to join carries.
const JOINING: (&str, &str) = ("k", "join");

/// Starts advertising a port, under a name that changes each time.
pub fn advertise(port: u16) -> Result<Advert> {
    advertise_with(port, &[])
}

/// Advertises a device with no vault yet, for one that has the vault to find.
pub fn advertise_joining(port: u16) -> Result<Advert> {
    advertise_with(port, &[JOINING])
}

fn advertise_with(port: u16, properties: &[(&str, &str)]) -> Result<Advert> {
    let daemon = ServiceDaemon::new().map_err(|_| LanError::Discovery)?;
    let instance = hex(&crypto::random_array::<8>()?);
    let host = format!("{instance}.local.");
    let info = ServiceInfo::new(SERVICE, &instance, &host, "", port, properties)
        .map_err(|_| LanError::Discovery)?
        .enable_addr_auto();
    let fullname = info.get_fullname().to_owned();
    daemon.register(info).map_err(|_| LanError::Discovery)?;
    Ok(Advert { daemon, fullname })
}

/// Listens for adverts for a while and returns what was found.
pub fn browse(window: Duration) -> Result<Vec<SocketAddr>> {
    browse_where(window, |_| true)
}

/// Only the devices waiting to join a vault.
pub fn browse_joining(window: Duration) -> Result<Vec<SocketAddr>> {
    browse_where(window, |kind| kind == Some(JOINING.1))
}

fn browse_where(window: Duration, wanted: impl Fn(Option<&str>) -> bool) -> Result<Vec<SocketAddr>> {
    let daemon = ServiceDaemon::new().map_err(|_| LanError::Discovery)?;
    let events = daemon.browse(SERVICE).map_err(|_| LanError::Discovery)?;
    let deadline = Instant::now() + window;
    let mut found = BTreeSet::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(service)) => {
                if !wanted(service.get_property_val_str(JOINING.0)) {
                    continue;
                }
                for address in service.get_addresses_v4() {
                    found.insert(SocketAddr::new(IpAddr::V4(address), service.get_port()));
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    Ok(found.into_iter().collect())
}
