//! What the bridge tells a client about itself: the ports, the identity, and `serverinfo`.
//!
//! `serverinfo` is the first thing a Moonlight client reads, before pairing and before every
//! session. It has to answer in the exact shape the client parses or the client decides there is
//! no host here - so the fields below are the ones Sunshine returns, with the values a bridge in
//! front of Porthole can honestly give: H.264 only, one target, on a LAN.

use std::net::Ipv4Addr;

/// The HTTP port, where discovery, `serverinfo` and the pairing phases are served.
pub const HTTP_PORT: u16 = 47989;
/// The HTTPS port, where the paired `serverinfo`, the final pair challenge and the app list live.
pub const HTTPS_PORT: u16 = 47984;
/// The RTSP port, where a session is set up.
pub const RTSP_PORT: u16 = 48010;
/// The UDP port the bridge sends video on.
pub const VIDEO_PORT: u16 = 47998;
/// The UDP port the ENet control-and-input channel runs on.
pub const CONTROL_PORT: u16 = 47999;

/// The GameStream version the bridge reports. Clients gate behaviour on the major version
/// (7 and up choose SHA-256 pairing and the modern control protocol), so this is 7.x deliberately.
const APP_VERSION: &str = "7.1.431.0";
/// The companion version string the client also reads.
const GFE_VERSION: &str = "3.23.0.74";

/// The bridge's identity and the numbers `serverinfo` reports.
#[derive(Debug, Clone)]
pub struct Host {
    /// A stable unique id for this host, a UUID string.
    pub uuid: String,
    /// The name shown in the client's host list.
    pub hostname: String,
    /// The address the client should reach this machine on.
    pub local_ip: Ipv4Addr,
    /// A MAC-shaped identifier; the client stores it but does nothing that needs it to be real.
    pub mac: String,
}

impl Host {
    /// A host with a freshly generated id and MAC.
    #[must_use]
    pub fn new(hostname: String, local_ip: Ipv4Addr) -> Self {
        Self {
            uuid: random_uuid(),
            hostname,
            local_ip,
            mac: random_mac(),
        }
    }

    /// The `serverinfo` document, in the paired or unpaired state.
    ///
    /// `paired` flips `PairStatus`, which is how a client decides whether to offer to pair or to
    /// stream. Everything else is constant: one H.264 host on a LAN.
    #[must_use]
    pub fn serverinfo(&self, paired: bool) -> String {
        let pair_status = u8::from(paired);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
             <root status_code=\"200\">\
             <hostname>{hostname}</hostname>\
             <appversion>{APP_VERSION}</appversion>\
             <GfeVersion>{GFE_VERSION}</GfeVersion>\
             <uniqueid>{uuid}</uniqueid>\
             <MaxLumaPixelsHEVC>0</MaxLumaPixelsHEVC>\
             <ServerCodecModeSupport>3</ServerCodecModeSupport>\
             <HttpsPort>{https}</HttpsPort>\
             <ExternalPort>{http}</ExternalPort>\
             <mac>{mac}</mac>\
             <LocalIP>{ip}</LocalIP>\
             <SupportedDisplayMode>\
             <DisplayMode><Width>1920</Width><Height>1080</Height><RefreshRate>60</RefreshRate></DisplayMode>\
             <DisplayMode><Width>1280</Width><Height>720</Height><RefreshRate>60</RefreshRate></DisplayMode>\
             </SupportedDisplayMode>\
             <PairStatus>{pair_status}</PairStatus>\
             <currentgame>0</currentgame>\
             <state>SUNSHINE_SERVER_FREE</state>\
             </root>",
            hostname = self.hostname,
            uuid = self.uuid,
            https = HTTPS_PORT,
            http = HTTP_PORT,
            mac = self.mac,
            ip = self.local_ip,
        )
    }
}

/// A random UUID string (version-4 shaped). Its only job is to be stable and unique per host.
fn random_uuid() -> String {
    let b = rand::random::<[u8; 16]>();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6] & 0x0f,
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15],
    )
}

/// A random locally-administered MAC address string.
fn random_mac() -> String {
    let b = rand::random::<[u8; 6]>();
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        b[0] & 0b1111_1110 | 0b0000_0010,
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
    )
}

#[cfg(test)]
mod tests {
    use super::Host;
    use std::net::Ipv4Addr;

    #[test]
    fn serverinfo_reports_the_pair_status_and_is_well_formed() {
        let host = Host::new("prosperous-test".into(), Ipv4Addr::new(192, 168, 1, 50));
        let unpaired = host.serverinfo(false);
        assert!(unpaired.contains("<PairStatus>0</PairStatus>"));
        assert!(unpaired.contains("<hostname>prosperous-test</hostname>"));
        assert!(unpaired.contains(&format!("<uniqueid>{}</uniqueid>", host.uuid)));
        let paired = host.serverinfo(true);
        assert!(paired.contains("<PairStatus>1</PairStatus>"));
    }

    #[test]
    fn the_generated_uuid_is_uuid_shaped() {
        let host = Host::new("h".into(), Ipv4Addr::LOCALHOST);
        assert_eq!(host.uuid.len(), 36);
        assert_eq!(host.uuid.matches('-').count(), 4);
    }
}
