//! Sending controller records to a target's input payload over TCP.
//!
//! Input goes down its own socket so that playing a target needs no vendor protocol, pairing
//! or account: both ends are ours (`docs/VIDEO.md` part three). Reading a keyboard is a
//! separate module because an unbound key and a dropped connection need different fixes.
//!
//! [`crate::feed::Feed::status`] tells idle, sending, lost and refused apart, and keeps the
//! reason a connection ended until the caller reconnects or closes.

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use crate::pad::RECORD;

/// The port a target's input payload listens on.
///
/// Chosen by us in `docs/VIDEO.md` part three, since both ends are ours; every other port in
/// this crate is measured.
pub const PORT: u16 = 9806;

/// How long to wait for a target to accept a connection.
///
/// Short: a target on the same network answers at once or is not there.
pub const PATIENCE: Duration = Duration::from_millis(1500);

/// Where a feed has got to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    /// Nothing has been tried.
    #[default]
    Idle,
    /// Records are going across.
    Sending,
    /// It was connected and is not any more, for this reason.
    ///
    /// Distinct from [`Status::Idle`]: a connection that broke needs different work from one
    /// that never started.
    Lost(String),
    /// It would not connect at all.
    Refused(String),
}

impl Status {
    /// Whether records are going anywhere.
    #[must_use]
    pub const fn is_sending(&self) -> bool {
        matches!(self, Self::Sending)
    }

    /// The status as a short phrase for display.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Idle => "not connected".to_owned(),
            Self::Sending => "sending".to_owned(),
            Self::Lost(why) => format!("the connection ended: {why}"),
            Self::Refused(why) => format!("could not connect: {why}"),
        }
    }
}

/// An open connection to a target's input payload.
#[derive(Debug, Default)]
pub struct Feed {
    stream: Option<TcpStream>,
    /// Where it has got to.
    pub status: Status,
    /// How many records have gone across since it connected.
    pub sent: u64,
    /// How many were dropped because nothing was connected.
    ///
    /// Counted so a panel can tell a wrong mapping from a feed that was not open.
    pub dropped: u64,
}

impl Feed {
    /// A feed that has not been opened.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stream: None,
            status: Status::Idle,
            sent: 0,
            dropped: 0,
        }
    }

    /// Opens a connection, replacing any that was already there.
    ///
    /// # Errors
    ///
    /// When the target will not accept. The reason is also kept in [`crate::feed::Feed::status`],
    /// so a caller that ignores the result still has it to show.
    pub fn open(&mut self, address: &str, port: u16) -> Result<(), String> {
        self.close();
        let target = format!("{address}:{port}");
        let resolved = target
            .parse()
            .map_err(|_| format!("{target} is not an address this can reach"));
        let stream = match resolved {
            Ok(at) => TcpStream::connect_timeout(&at, PATIENCE),
            // A host name rather than an address: the resolving connect handles it.
            Err(_) => TcpStream::connect(&target),
        };
        match stream {
            Ok(stream) => {
                // Nagle off: every record is a small write that matters immediately.
                let _ = stream.set_nodelay(true);
                // A blocking write would stall the window's repaint.
                let _ = stream.set_write_timeout(Some(PATIENCE));
                self.stream = Some(stream);
                self.status = Status::Sending;
                self.sent = 0;
                Ok(())
            }
            Err(why) => {
                let why = format!("{target}: {why}");
                self.status = Status::Refused(why.clone());
                Err(why)
            }
        }
    }

    /// Closes the connection, without recording a reason.
    ///
    /// For a deliberate stop. An unrequested drop goes through [`Status::Lost`] instead.
    pub fn close(&mut self) {
        if self.stream.take().is_some() {
            self.status = Status::Idle;
        }
    }

    /// Sends whatever records are ready.
    ///
    /// Returns how many went. A feed that is not open counts them in `dropped` rather than
    /// failing, since pressing keys with nothing connected is an ordinary state.
    pub fn send(&mut self, records: &[[u8; RECORD]]) -> usize {
        if records.is_empty() {
            return 0;
        }
        let Some(stream) = self.stream.as_mut() else {
            self.dropped = self.dropped.saturating_add(records.len() as u64);
            return 0;
        };
        // One write for the batch, so a target reads all pads of a frame as one moment.
        let mut batch = Vec::with_capacity(records.len() * RECORD);
        for record in records {
            batch.extend_from_slice(record);
        }
        match stream.write_all(&batch) {
            Ok(()) => {
                self.sent = self.sent.saturating_add(records.len() as u64);
                records.len()
            }
            Err(why) => {
                // Lost, not Idle: a break must not look like a feed that never started.
                self.stream = None;
                self.status = Status::Lost(why.to_string());
                self.dropped = self.dropped.saturating_add(records.len() as u64);
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::TcpListener;

    use super::{Feed, Status};
    use crate::pad::{Button, Pad};

    /// A record with something in it.
    fn pressed() -> [u8; 24] {
        let mut pad = Pad::rest();
        pad.hold(Button::Cross, true);
        pad.to_wire()
    }

    /// What is written is exactly what a payload reads.
    #[test]
    fn records_arrive_as_they_were_written() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("has one").port();

        let mut feed = Feed::new();
        feed.open("127.0.0.1", port).expect("connects");
        assert!(feed.status.is_sending());

        let record = pressed();
        assert_eq!(feed.send(&[record]), 1);

        let (mut accepted, _) = listener.accept().expect("accepts");
        let mut got = [0_u8; 24];
        accepted.read_exact(&mut got).expect("reads");
        assert_eq!(got, record);

        let read = Pad::from_wire(&got).expect("parses");
        assert!(read.holds(Button::Cross));
    }

    /// Several pads in one frame go in one write, so a target sees one moment.
    #[test]
    fn a_frame_with_several_pads_arrives_together() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("has one").port();

        let mut feed = Feed::new();
        feed.open("127.0.0.1", port).expect("connects");

        let one = Pad {
            slot: 0,
            ..Pad::rest()
        };
        let two = Pad {
            slot: 1,
            ..Pad::rest()
        };
        assert_eq!(feed.send(&[one.to_wire(), two.to_wire()]), 2);

        let (mut accepted, _) = listener.accept().expect("accepts");
        let mut got = [0_u8; 48];
        accepted.read_exact(&mut got).expect("reads both");
        assert_eq!(Pad::from_wire(&got[..24]).expect("first").slot, 0);
        assert_eq!(Pad::from_wire(&got[24..]).expect("second").slot, 1);
    }

    /// Sending with nothing connected is not an error, and the records are counted as dropped.
    #[test]
    fn records_with_nowhere_to_go_are_counted_rather_than_lost_silently() {
        let mut feed = Feed::new();
        assert_eq!(feed.send(&[pressed()]), 0);
        assert_eq!(feed.dropped, 1);
        assert_eq!(feed.status, Status::Idle, "it never started");
    }

    /// A dropped connection reports `Lost` with its reason, not `Idle`.
    #[test]
    fn a_dropped_connection_says_so_rather_than_going_quiet() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("has one").port();

        let mut feed = Feed::new();
        feed.open("127.0.0.1", port).expect("connects");
        let (accepted, _) = listener.accept().expect("accepts");
        drop(accepted);
        drop(listener);

        // The first write after a close may succeed; the failure arrives with the reset.
        let mut said = None;
        for _ in 0..50 {
            feed.send(&[pressed()]);
            if let Status::Lost(why) = &feed.status {
                said = Some(why.clone());
                break;
            }
        }
        let why = said.expect("the connection ended and should have said so");
        assert!(!why.is_empty(), "and should have said why");
        assert_ne!(feed.status, Status::Idle, "lost is not idle");
    }

    /// Refusing to connect is its own state, with the address in it.
    #[test]
    fn a_refusal_names_what_it_could_not_reach() {
        let mut feed = Feed::new();
        // Port zero cannot be connected to, whatever is listening on the test machine.
        let refused = feed.open("127.0.0.1", 0).expect_err("nothing is there");
        assert!(refused.contains("127.0.0.1:0"), "{refused}");
        assert!(matches!(feed.status, Status::Refused(_)));
        assert!(!feed.status.is_sending());
    }

    /// A deliberate close leaves the status `Idle` with no reason.
    #[test]
    fn closing_deliberately_leaves_no_complaint() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("has one").port();

        let mut feed = Feed::new();
        feed.open("127.0.0.1", port).expect("connects");
        feed.close();
        assert_eq!(feed.status, Status::Idle, "asked to stop is idle, not lost");
    }
}
