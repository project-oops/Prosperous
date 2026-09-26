//! The RTSP handshake that sets a stream up, on port 48010.
//!
//! Adapted from Moonshine's `rtsp.rs` (BSD-2-Clause; see `THIRD-PARTY-LICENSES.md`). After
//! `launch`, a client runs OPTIONS, DESCRIBE, SETUP (once per stream), ANNOUNCE and PLAY, one
//! request per connection. On PLAY the caller starts streaming. Only H.264 video is advertised:
//! no HEVC, AV1 or audio.

use std::io::{Read, Write};
use std::net::TcpStream;

use rtsp_types::headers::{CSEQ, SESSION};
use rtsp_types::{Method, Request, Response, StatusCode, Version};

/// What one RTSP request told the bridge to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    /// Keep handling requests on this session.
    Continue,
    /// The client sent PLAY: start streaming.
    Play,
}

/// Read one RTSP request off `stream`, answer it, and say whether it was PLAY.
///
/// # Errors
///
/// On a read/write error or a request that will not parse as RTSP.
pub(crate) fn serve_one(stream: &mut TcpStream, server_ports: Ports) -> std::io::Result<Next> {
    let mut buffer = vec![0_u8; 8192];
    let read = stream.read(&mut buffer)?;
    if read == 0 {
        return Ok(Next::Continue);
    }
    let (message, _consumed) =
        rtsp_types::Message::<Vec<u8>>::parse(&buffer[..read]).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{error}"))
        })?;
    let rtsp_types::Message::Request(request) = message else {
        return Ok(Next::Continue); // not a request; nothing to answer
    };
    let (response, next) = answer(&request, server_ports);
    let mut out = Vec::new();
    response
        .write(&mut out)
        .map_err(|error| std::io::Error::other(format!("{error}")))?;
    stream.write_all(&out)?;
    stream.flush()?;
    Ok(next)
}

/// The UDP ports the bridge tells the client to reach each stream on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ports {
    /// The video port.
    pub(crate) video: u16,
    /// The control port.
    pub(crate) control: u16,
    /// The audio port, answered in SETUP; nothing is sent on it.
    pub(crate) audio: u16,
}

/// Build the response to one request, and decide what happens next.
fn answer<B>(request: &Request<B>, ports: Ports) -> (Response<Vec<u8>>, Next) {
    let cseq = request
        .header(&CSEQ)
        .map_or("0", |value| value.as_str())
        .to_owned();
    let base = || Response::builder(Version::V1_0, StatusCode::Ok).header(CSEQ, cseq.clone());

    match request.method() {
        Method::Options => (
            base()
                .header(
                    rtsp_types::headers::PUBLIC,
                    "OPTIONS DESCRIBE SETUP PLAY ANNOUNCE",
                )
                .build(Vec::new()),
            Next::Continue,
        ),
        Method::Describe => (base().build(sdp().into_bytes()), Next::Continue),
        Method::Setup => {
            // SETUP names the stream in the URL as `streamid=<name>/...`.
            let uri = request
                .request_uri()
                .map(ToString::to_string)
                .unwrap_or_default();
            let port = if uri.contains("streamid=control") {
                ports.control
            } else if uri.contains("streamid=audio") {
                ports.audio
            } else {
                ports.video
            };
            (
                base()
                    .header(SESSION, "ProsperousSession;timeout=90")
                    .header(
                        rtsp_types::headers::TRANSPORT,
                        format!("server_port={port}"),
                    )
                    .build(Vec::new()),
                Next::Continue,
            )
        }
        Method::Announce => (base().build(Vec::new()), Next::Continue),
        Method::Play => (base().build(Vec::new()), Next::Play),
        _ => (
            Response::builder(Version::V1_0, StatusCode::MethodNotAllowed)
                .header(CSEQ, cseq)
                .build(Vec::new()),
            Next::Continue,
        ),
    }
}

/// The SDP a DESCRIBE returns: H.264 only, one video stream.
fn sdp() -> String {
    // The keys a GameStream client reads; the values say H.264 only, no HEVC, no audio.
    "v=0\r\n\
     o=android 0 14 IN IPv4 0.0.0.0\r\n\
     s=NVIDIA Streaming Server\r\n\
     a=x-nv-video[0].clientViewportWd:1280 \r\n\
     a=x-nv-video[0].clientViewportHt:720 \r\n\
     a=x-nv-video[0].maxFPS:60 \r\n\
     a=x-nv-video[0].encoderCscMode:0 \r\n\
     a=x-nv-vqos[0].bitStreamFormat:0 \r\n\
     a=x-nv-general.serverCodecSupportMode:3 \r\n\
     m=video 47998 RTP/AVP 98 \r\n\
     a=rtpmap:98 H264/90000\r\n"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{Next, Ports, answer, sdp};
    use rtsp_types::{Empty, Method, Request, Version};

    fn ports() -> Ports {
        Ports {
            video: 47998,
            control: 47999,
            audio: 48000,
        }
    }

    fn request(method: Method, uri: &str) -> Request<Empty> {
        Request::builder(method, Version::V1_0)
            .request_uri(uri.parse::<rtsp_types::Url>().unwrap())
            .header(rtsp_types::headers::CSEQ, "3")
            .empty()
    }

    /// OPTIONS lists the methods and echoes the request's `CSeq`.
    #[test]
    fn options_lists_the_methods_and_echoes_cseq() {
        let (response, next) = answer(&request(Method::Options, "rtsp://host"), ports());
        assert_eq!(next, Next::Continue);
        assert_eq!(
            response
                .header(&rtsp_types::headers::CSEQ)
                .unwrap()
                .as_str(),
            "3"
        );
        assert!(
            response
                .header(&rtsp_types::headers::PUBLIC)
                .unwrap()
                .as_str()
                .contains("DESCRIBE")
        );
    }

    /// The SDP offers H.264 and never HEVC.
    #[test]
    fn describe_returns_an_h264_sdp() {
        assert!(sdp().contains("H264/90000"));
        assert!(!sdp().to_lowercase().contains("hevc"));
    }

    /// SETUP for the control stream answers with the control port.
    #[test]
    fn setup_for_control_answers_with_the_control_port() {
        let (response, _) = answer(
            &request(Method::Setup, "rtsp://host/streamid=control/13/0"),
            ports(),
        );
        let transport = response.header(&rtsp_types::headers::TRANSPORT).unwrap();
        assert!(transport.as_str().contains("server_port=47999"));
    }

    /// PLAY tells the caller to start streaming.
    #[test]
    fn play_says_to_start_streaming() {
        let (_response, next) = answer(&request(Method::Play, "rtsp://host"), ports());
        assert_eq!(next, Next::Play);
    }
}
