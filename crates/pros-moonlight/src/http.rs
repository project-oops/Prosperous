//! The little HTTP server the GameStream endpoints are served over.
//!
//! # Why this is hand-written rather than a framework
//!
//! Every request is a `GET` with query parameters and a small XML reply. That is the whole
//! surface, and it is served over both a plain socket (47989) and a TLS one (47984) with the same
//! routing - so a request handler generic over "something you can read and write" is all it takes,
//! and a full HTTP stack would be weight for a job this size does not have. The requests are read
//! up to the blank line that ends the headers; a `GET` has no body.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::IpAddr;

use crate::apps::Apps;
use crate::host::{Host, RTSP_PORT};
use crate::pairing::Pairing;
use crate::session::{Sessions, StreamConfig};

/// The largest request head accepted, so a peer cannot make the bridge buffer without bound. The
/// client certificate in a pairing request is the biggest thing sent and is a few kilobytes.
const MAX_HEAD: usize = 64 * 1024;

/// Everything the routes need: who we are, the pairing state, the apps, and the stream sessions.
pub(crate) struct Bridge {
    /// The host identity and `serverinfo` values.
    pub(crate) host: Host,
    /// The pairing service.
    pub(crate) pairing: Pairing,
    /// The apps offered, one per registered target.
    pub(crate) apps: Apps,
    /// The streaming sessions the RTSP handshake drives.
    pub(crate) sessions: Sessions,
}

/// A parsed request: the path and its query parameters.
struct Request {
    /// The path, without the query string.
    path: String,
    /// The decoded query parameters.
    query: HashMap<String, String>,
}

impl Request {
    /// A query parameter, or the empty string if absent.
    fn get(&self, key: &str) -> &str {
        self.query.get(key).map_or("", String::as_str)
    }

    /// The client id, which every authenticated route keys off.
    fn id(&self) -> &str {
        self.get("uniqueid")
    }
}

impl Bridge {
    /// Route one parsed request to its response body (always XML). `peer` is the client's address,
    /// which a launch needs so the stream can be sent back to it.
    fn route(&self, request: &Request, peer: IpAddr) -> String {
        match request.path.as_str() {
            "/serverinfo" => self.host.serverinfo(self.pairing.is_paired(request.id())),
            "/pair" => self.pair(request),
            "/applist" => self.apps.applist(),
            // launch and resume both begin a stream; resume differs only in that the client is
            // returning to one, which for this bridge is the same setup.
            "/launch" | "/resume" => self.launch(request, peer),
            // A control path of ours, not the protocol's: how the PIN read off the client gets in.
            "/pin" => {
                self.pairing.submit_pin(request.get("pin"));
                "<?xml version=\"1.0\"?><root status_code=\"200\"><accepted>1</accepted></root>"
                    .to_owned()
            }
            _ => "<?xml version=\"1.0\"?><root status_code=\"404\"></root>".to_owned(),
        }
    }

    /// Handle a launch/resume: record the stream the client asked for, and hand back the RTSP URL
    /// it should connect to next.
    fn launch(&self, request: &Request, peer: IpAddr) -> String {
        let config = StreamConfig::from_query(&request.query);
        tracing::info!(client = %peer, width = config.width, height = config.height, fps = config.fps, "launch");
        self.sessions.launched(peer, config);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<root status_code=\"200\">\
             <sessionUrl0>rtsp://{}:{RTSP_PORT}</sessionUrl0><gamesession>1</gamesession></root>",
            self.host.local_ip,
        )
    }

    /// Dispatch a `/pair` request to the phase its parameters name.
    fn pair(&self, request: &Request) -> String {
        let id = request.id();
        let result = if request.get("phrase") == "getservercert" {
            self.pairing
                .get_server_cert(id, request.get("salt"), request.get("clientcert"))
        } else if !request.get("clientchallenge").is_empty() {
            self.pairing
                .client_challenge(id, request.get("clientchallenge"))
        } else if !request.get("serverchallengeresp").is_empty() {
            self.pairing
                .server_challenge_resp(id, request.get("serverchallengeresp"))
        } else if !request.get("clientpairingsecret").is_empty() {
            self.pairing
                .client_pairing_secret(id, request.get("clientpairingsecret"))
        } else if request.get("phrase") == "pairchallenge" {
            return self.pairing.pair_challenge(id);
        } else {
            Ok("<?xml version=\"1.0\"?><root status_code=\"400\"></root>".to_owned())
        };
        result.unwrap_or_else(|error| {
            tracing::warn!(%error, "pairing phase failed");
            "<?xml version=\"1.0\"?><root status_code=\"200\"><paired>0</paired></root>".to_owned()
        })
    }
}

/// Read one request off `stream`, route it, and write the reply. `peer` is the client's address,
/// needed so a launch can send the stream back to it.
///
/// # Errors
///
/// On a read or write error, or a request head larger than [`MAX_HEAD`].
pub(crate) fn serve_one<S: Read + Write>(
    stream: &mut S,
    bridge: &Bridge,
    peer: IpAddr,
) -> io::Result<()> {
    let request = read_request(stream)?;
    tracing::debug!(path = %request.path, "request");
    let body = bridge.route(&request, peer);
    write_response(stream, &body)
}

/// Read the request line and headers, stopping at the blank line. A `GET` has no body.
fn read_request<S: Read>(stream: &mut S) -> io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = (&mut reader).take(MAX_HEAD as u64).read_line(&mut line)?;
    if read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "empty request",
        ));
    }
    // "GET /path?query HTTP/1.1"
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    // Drain the remaining header lines up to the blank one, so the socket is left at the body.
    loop {
        let mut header = String::new();
        let n = (&mut reader).take(MAX_HEAD as u64).read_line(&mut header)?;
        if n == 0 || header == "\r\n" || header == "\n" {
            break;
        }
    }
    Ok(Request {
        path: path.to_owned(),
        query: parse_query(query),
    })
}

/// Split a query string into decoded key/value pairs.
fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

/// Minimal percent-decoding, enough for the values GameStream sends (`+` is a space, `%XX` a byte).
fn percent_decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(' '),
            b'%' => {
                let hi = bytes.next();
                let lo = bytes.next();
                if let (Some(hi), Some(lo)) = (hi, lo)
                    && let (Some(hi), Some(lo)) =
                        ((hi as char).to_digit(16), (lo as char).to_digit(16))
                    && let Ok(byte) = u8::try_from(hi * 16 + lo)
                {
                    out.push(char::from(byte));
                }
            }
            other => out.push(char::from(other)),
        }
    }
    out
}

/// Write a `200 OK` with `body` as XML and close the connection.
fn write_response<S: Write>(stream: &mut S, body: &str) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/xml\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::{parse_query, percent_decode};

    #[test]
    fn a_query_splits_into_pairs() {
        let q = parse_query("uniqueid=abc&phrase=getservercert&salt=00ff");
        assert_eq!(q.get("uniqueid").unwrap(), "abc");
        assert_eq!(q.get("phrase").unwrap(), "getservercert");
        assert_eq!(q.get("salt").unwrap(), "00ff");
    }

    #[test]
    fn percent_and_plus_decode() {
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("%41%42"), "AB");
    }
}
