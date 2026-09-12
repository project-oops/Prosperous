//! A streaming session: the mode a client asked for, and the video pipeline that feeds it.
//!
//! The pipeline shape follows Moonshine (Hans Gaiser, BSD-2-Clause; see `THIRD-PARTY-LICENSES.md`):
//! read the target's encoded stream, group it into frames, packetise each frame into RTP with FEC,
//! and send the packets to the client over UDP. The bridge decodes nothing; it moves bytes the
//! target produced into the shape the client reads.

use std::io::Read;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::control;
use crate::error::Result;
use crate::host::{CONTROL_PORT, VIDEO_PORT};
use crate::nal::Frames;
use crate::rtsp;
use crate::video::{self, Packetizer};

/// How large a chunk is read off the target's stream at once.
const READ_CHUNK: usize = 32 * 1024;

/// What a client asked for when it launched a stream.
///
/// Parsed from the `launch`/`resume` query and the RTSP `ANNOUNCE`. The fields the video pipeline
/// needs are the packet size and FEC percentage; the geometry and bitrate are recorded so the
/// bridge can pass them to the target once the `PCTL` set-mode record lands (oops-apps 8c12).
#[derive(Debug, Clone)]
pub(crate) struct StreamConfig {
    /// Requested width in pixels.
    pub(crate) width: u32,
    /// Requested height in pixels.
    pub(crate) height: u32,
    /// Requested frames per second.
    pub(crate) fps: u32,
    /// The AES key for the encrypted control channel, from `rikey` (16 bytes).
    pub(crate) rikey: [u8; 16],
    /// How a frame is cut into packets.
    pub(crate) video: video::Config,
}

impl StreamConfig {
    /// Read a launch/resume query into a config. Missing fields fall back to safe defaults so a
    /// terse client still gets a stream rather than a refusal.
    pub(crate) fn from_query(query: &std::collections::HashMap<String, String>) -> Self {
        // Moonlight sends `mode=WIDTHxHEIGHTxFPS`.
        let (mut width, mut height, mut fps) = (1280_u32, 720_u32, 60_u32);
        if let Some(mode) = query.get("mode") {
            let mut parts = mode.split('x').map(|part| part.parse().ok());
            width = parts.next().flatten().unwrap_or(width);
            height = parts.next().flatten().unwrap_or(height);
            fps = parts.next().flatten().unwrap_or(fps);
        }
        let rikey = query
            .get("rikey")
            .and_then(|hex_key| hex::decode(hex_key).ok())
            .and_then(|bytes| <[u8; 16]>::try_from(bytes.as_slice()).ok())
            .unwrap_or_default();
        // `rikeyid` is also sent, but the control channel's nonce is built from the message
        // sequence, not the key id, so it is not carried here.
        Self {
            width,
            height,
            fps,
            rikey,
            video: video::Config::default(),
        }
    }
}

/// What a client launched: where it is, and how it wants the stream.
#[derive(Debug, Clone)]
struct Launch {
    /// The client's address; video and control are sent back to it.
    client: IpAddr,
    /// The mode, keys and packet parameters it asked for.
    config: StreamConfig,
}

/// The streaming state the RTSP handshake drives: what was launched, and whether it is running.
#[derive(Debug)]
pub(crate) struct Sessions {
    /// The address of the target serving Porthole's 9805/9806 (a console, or the fake target).
    target: String,
    /// The most recent launch, set by `/launch` and read on PLAY.
    current: Mutex<Option<Launch>>,
    /// Whether a stream is already running, so PLAY starts it exactly once.
    streaming: AtomicBool,
}

impl Sessions {
    /// A session manager pointed at the target that serves the video and takes the input.
    pub(crate) fn new(target: String) -> Self {
        Self {
            target,
            current: Mutex::new(None),
            streaming: AtomicBool::new(false),
        }
    }

    /// Record a launch from `client` with `config`, readying a stream that PLAY will start.
    pub(crate) fn launched(&self, client: IpAddr, config: StreamConfig) {
        if let Ok(mut current) = self.current.lock() {
            *current = Some(Launch { client, config });
        }
        self.streaming.store(false, Ordering::SeqCst);
    }

    /// Handle one RTSP connection (one request; Moonlight opens a connection per request). On PLAY,
    /// start the video pump and control channel.
    pub(crate) fn serve_rtsp(&self, stream: &mut TcpStream) {
        let ports = rtsp::Ports {
            video: VIDEO_PORT,
            control: CONTROL_PORT,
            audio: VIDEO_PORT + 2,
        };
        match rtsp::serve_one(stream, ports) {
            Ok(rtsp::Next::Play) => self.play(),
            Ok(rtsp::Next::Continue) => {}
            Err(error) => tracing::debug!(%error, "rtsp connection ended"),
        }
    }

    /// Start streaming for the launched session, once.
    fn play(&self) {
        if self.streaming.swap(true, Ordering::SeqCst) {
            return; // already streaming
        }
        let Some(launch) = self.current.lock().ok().and_then(|guard| guard.clone()) else {
            tracing::warn!("PLAY with no launch on record");
            return;
        };
        let target = self.target.clone();
        tracing::info!(client = %launch.client, target = %target, "streaming started");

        // Video: read the target's 9805, packetise, send to the client's video port.
        let video_target = target.clone();
        let video_config = launch.config.video;
        let client = launch.client;
        std::thread::spawn(move || {
            if let Err(error) = run_video(&video_target, client, video_config) {
                tracing::warn!(%error, "video stream ended");
            }
        });

        // Control: receive the client's input on the control port and forward it to the target.
        let rikey = launch.config.rikey;
        std::thread::spawn(move || {
            if let Err(error) = control::run(CONTROL_PORT, rikey, &target, || true) {
                tracing::warn!(%error, "control channel ended");
            }
        });
    }
}

/// Bind the video send socket and pump from the target to the client until the target closes.
fn run_video(target: &str, client: IpAddr, config: video::Config) -> Result<VideoStats> {
    let source: SocketAddr = format!("{target}:{}", crate::fake::VIDEO_PORT)
        .parse()
        .map_err(|_| crate::error::Error::Pairing("bad target address".into()))?;
    let socket = UdpSocket::bind(("0.0.0.0", VIDEO_PORT))?;
    let to = SocketAddr::new(client, VIDEO_PORT);
    pump_video(source, to, &socket, &config, || true)
}

/// Counts of what a video pipeline moved, for logs and tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VideoStats {
    /// Frames read from the target and packetised.
    pub(crate) frames: u64,
    /// RTP packets sent to the client.
    pub(crate) packets: u64,
}

/// Read the target's encoded video from `target` (its 9805), packetise it, and send the RTP
/// packets to `client` over `socket`. Runs until the target closes the stream or `keep_going`
/// returns false.
///
/// # Errors
///
/// If connecting to the target or sending on the socket fails.
pub(crate) fn pump_video(
    target: SocketAddr,
    client: SocketAddr,
    socket: &UdpSocket,
    config: &video::Config,
    keep_going: impl Fn() -> bool,
) -> Result<VideoStats> {
    let mut source = TcpStream::connect(target)?;
    let mut frames = Frames::new();
    let mut packetizer = Packetizer::new();
    let mut buffer = vec![0_u8; READ_CHUNK];
    let mut stats = VideoStats::default();
    let mut frame_index = 0_u32;

    while keep_going() {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break; // the target closed the stream
        }
        for frame in frames.feed(&buffer[..read]) {
            stats.frames += 1;
            for packet in packetizer.packetize(&frame.bytes, frame.keyframe, frame_index, config) {
                socket.send_to(&packet, client)?;
                stats.packets += 1;
            }
            frame_index = frame_index.wrapping_add(1);
        }
    }
    if let Some(frame) = frames.finish() {
        stats.frames += 1;
        for packet in packetizer.packetize(&frame.bytes, frame.keyframe, frame_index, config) {
            socket.send_to(&packet, client)?;
            stats.packets += 1;
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::{StreamConfig, pump_video};
    use crate::video;
    use std::collections::HashMap;
    use std::io::Write;
    use std::net::{Ipv4Addr, TcpListener, UdpSocket};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    #[test]
    fn a_launch_query_parses_mode_and_key() {
        let mut query = HashMap::new();
        query.insert("mode".to_owned(), "1920x1080x60".to_owned());
        query.insert("rikey".to_owned(), hex::encode([9_u8; 16]));
        let config = StreamConfig::from_query(&query);
        assert_eq!((config.width, config.height, config.fps), (1920, 1080, 60));
        assert_eq!(config.rikey, [9_u8; 16]);
    }

    #[test]
    fn video_flows_from_a_fake_target_into_rtp_packets() {
        // A stand-in target: a TCP server that sends a short Annex-B stream and closes.
        let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let target_addr = target.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = target.accept() {
                // SPS, PPS, an IDR, then a P-frame, each a NAL with a 4-byte start code.
                let mut clip = Vec::new();
                for (kind, body) in [
                    (7_u8, &b"sps"[..]),
                    (8, b"pps"),
                    (5, b"keyframe-slice"),
                    (1, b"inter"),
                ] {
                    clip.extend_from_slice(&[0, 0, 0, 1, kind]);
                    clip.extend_from_slice(body);
                }
                let _ = stream.write_all(&clip);
                // Close, which ends the pump after it flushes the last frame.
            }
        });

        // The client side: a UDP socket collecting the RTP packets.
        let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .unwrap();
        let client_addr = client.local_addr().unwrap();
        let received = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&received);
        let collector = thread::spawn(move || {
            let mut packets = Vec::new();
            let mut buf = [0_u8; 2048];
            while let Ok((n, _)) = client.recv_from(&mut buf) {
                packets.push(buf[..n].to_vec());
                flag.store(true, Ordering::SeqCst);
            }
            packets
        });

        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let stats = pump_video(
            target_addr,
            client_addr,
            &sender,
            &video::Config {
                shard_payload: 1024,
                fec_percentage: 20,
            },
            || true,
        )
        .unwrap();

        let packets = collector.join().unwrap();
        assert!(
            stats.frames >= 2,
            "the keyframe and the inter frame both went through"
        );
        assert!(stats.packets >= 2);
        assert!(!packets.is_empty(), "RTP packets reached the client socket");
        // The first packet is the start of a frame and its RTP version byte is 0x90.
        assert_eq!(packets[0][0], 0x90);
    }
}
