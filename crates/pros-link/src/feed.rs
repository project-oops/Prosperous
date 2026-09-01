//! Sending controller records to a target.
//!
//! # Why this is separate from reading a keyboard
//!
//! What drives a pad and where its records go are different failures. A key that is not bound
//! is somebody's mapping; a connection that dropped is the network; and a panel that showed
//! one when it meant the other would send a person to fix the wrong thing.
//!
//! # The state that must never be guessed
//!
//! **Connected, not connected, and lost are three states.** A feed that reported *not
//! connected* after a drop would look identical to one nobody had started, and the difference
//! is the whole question - the first is a thing that broke, the second is a thing that has not
//! begun.
//!
//! So [`crate::feed::Feed::status`] carries the reason a connection ended, and carries it until somebody
//! either reconnects or gives up. Silence is never success here and never failure either; it
//! is one of three things and the caller is told which.
//!
//! # Why input goes down a socket at all
//!
//! `docs/VIDEO.md` part three: the whole stand-in exists so that watching and playing a target
//! does not require speaking the vendor's protocol. Video comes back over one socket and input
//! goes down another, and neither needs pairing, encryption or a vendor account - because the
//! target is prepared and runs our code, so the protocol is a decision rather than a
//! specification to reverse.

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use crate::pad::RECORD;

/// The port a target's input payload listens on.
///
/// From `docs/VIDEO.md` part three. **Ours to choose**, because both ends are ours - unlike
/// every other port this crate knows, which were measured.
pub const PORT: u16 = 9806;

/// How long to wait for a target to accept a connection.
///
/// Short. A target on the same network answers immediately or is not there, and a person
/// pressing *connect* is watching the button.
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
    /// **Distinct from [`Status::Idle`]** - one is a thing that broke and the other is a thing
    /// that has not begun, and reporting the first as the second loses the only fact worth
    /// having.
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

    /// How to put it to somebody.
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
    /// **Counted rather than ignored.** Input that went nowhere while somebody was pressing
    /// keys is the difference between *the mapping is wrong* and *the feed was not open*, and
    /// a panel that showed neither would leave them guessing between the two.
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
    /// When the target will not accept. The reason is kept in [`crate::feed::Feed::status`] as well, so a
    /// caller that ignores the result still has it to show.
    pub fn open(&mut self, address: &str, port: u16) -> Result<(), String> {
        self.close();
        let target = format!("{address}:{port}");
        let resolved = target
            .parse()
            .map_err(|_| format!("{target} is not an address this can reach"));
        let stream = match resolved {
            Ok(at) => TcpStream::connect_timeout(&at, PATIENCE),
            // A name rather than an address: fall back to the resolving connect, which is
            // slower and handles what the parse could not.
            Err(_) => TcpStream::connect(&target),
        };
        match stream {
            Ok(stream) => {
                // **Nagle off.** It exists to coalesce small writes, and every record here is
                // a small write that matters immediately - holding one back to fill a packet
                // is latency added on purpose to input.
                let _ = stream.set_nodelay(true);
                // A write that blocks would block the window, and a window that has stopped
                // repainting is indistinguishable from one that has crashed.
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
    /// For somebody choosing to stop. A drop nobody asked for goes through
    /// [`Status::Lost`] instead, because *asked to stop* and *stopped by itself* are the two
    /// things a person needs told apart.
    pub fn close(&mut self) {
        if self.stream.take().is_some() {
            self.status = Status::Idle;
        }
    }

    /// Sends whatever records are ready.
    ///
    /// Returns how many went. **A feed that is not open counts them as dropped** rather than
    /// failing: somebody pressing keys with nothing connected is an ordinary state, and an
    /// error per frame would bury the one thing worth saying.
    pub fn send(&mut self, records: &[[u8; RECORD]]) -> usize {
        if records.is_empty() {
            return 0;
        }
        let Some(stream) = self.stream.as_mut() else {
            self.dropped = self.dropped.saturating_add(records.len() as u64);
            return 0;
        };
        // One write for the batch. Several records in a frame are several pads, and a target
        // reading them together sees one moment rather than four.
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
                // The connection is gone, and saying so is the whole point of this branch.
                // Reverting to Idle here would make a break look like a thing never started.
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

    /// **What is written is exactly what a payload will read.**
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

    /// **Nothing connected is not an error, and the records are counted.**
    ///
    /// Somebody pressing keys with no connection is ordinary. What is not ordinary is not
    /// knowing whether the mapping is wrong or the feed was never open, which is what the
    /// count answers.
    #[test]
    fn records_with_nowhere_to_go_are_counted_rather_than_lost_silently() {
        let mut feed = Feed::new();
        assert_eq!(feed.send(&[pressed()]), 0);
        assert_eq!(feed.dropped, 1);
        assert_eq!(feed.status, Status::Idle, "it never started");
    }

    /// **A connection that dropped is not a connection that never started.**
    ///
    /// The two look identical from the outside and need different work, so the reason is kept
    /// rather than reset.
    #[test]
    fn a_dropped_connection_says_so_rather_than_going_quiet() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("has one").port();

        let mut feed = Feed::new();
        feed.open("127.0.0.1", port).expect("connects");
        let (accepted, _) = listener.accept().expect("accepts");
        drop(accepted);
        drop(listener);

        // The first write after a close may succeed - the failure arrives with the reset, so
        // this sends until it is told, which is what a caller does too.
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
        // Port zero cannot be connected to, so this fails without depending on what happens
        // to be listening on the machine running the test.
        let refused = feed.open("127.0.0.1", 0).expect_err("nothing is there");
        assert!(refused.contains("127.0.0.1:0"), "{refused}");
        assert!(matches!(feed.status, Status::Refused(_)));
        assert!(!feed.status.is_sending());
    }

    /// Choosing to stop is not the same as being stopped, so it records no reason.
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
