//! Advertising the bridge over mDNS, so a client finds it without being given an address.
//!
//! GameStream hosts announce `_nvstream._tcp` on the HTTP port. The advertisement lasts as long
//! as the daemon, so the daemon is handed back rather than dropped.

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::error::{Error, Result};
use crate::host::{HTTP_PORT, Host};

/// The GameStream mDNS service type.
const SERVICE_TYPE: &str = "_nvstream._tcp.local.";

/// A running mDNS advertisement. Keep it: dropping it withdraws the service.
pub(crate) struct Advertisement {
    /// The daemon, held so the advertisement lives as long as this does.
    _daemon: ServiceDaemon,
}

impl std::fmt::Debug for Advertisement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Advertisement")
    }
}

/// Advertise `host` as an `_nvstream._tcp` service on the HTTP port.
///
/// # Errors
///
/// If the mDNS daemon cannot start or the service will not register - most often no usable network
/// interface.
pub(crate) fn advertise(host: &Host) -> Result<Advertisement> {
    let daemon = ServiceDaemon::new().map_err(|error| Error::Io(io_other(&error)))?;
    let host_name = format!("{}.local.", host.hostname);
    let properties: [(&str, &str); 0] = [];
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        &host.hostname,
        &host_name,
        host.local_ip.to_string().as_str(),
        HTTP_PORT,
        &properties[..],
    )
    .map_err(|error| Error::Io(io_other(&error)))?;
    daemon
        .register(info)
        .map_err(|error| Error::Io(io_other(&error)))?;
    tracing::info!(service = SERVICE_TYPE, host = %host.hostname, ip = %host.local_ip, "advertising");
    Ok(Advertisement { _daemon: daemon })
}

/// Wrap an mDNS error as an I/O error.
fn io_other(error: &impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(error.to_string())
}
