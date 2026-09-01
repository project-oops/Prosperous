//! The input half of the stand-in, against a payload that is not a payload.
//!
//! # What this is for
//!
//! `pad` builds a 24-byte record, `pads` decides when one is worth sending, and `feed` puts it
//! on a socket. All three had unit tests and **none of them had ever been parsed by anything**,
//! which is a wire format nobody has read from the other end.
//!
//! The bits in that record are measured, credited to Ghostpad, and were wrong in three
//! separate ways before somebody read a working implementation. Testing them against a reader
//! is the cheapest way to keep them right.

use std::time::Duration;

use pros_link::pad::{Button, CENTRE, Pad, RECORD};
use pros_link::standin::{Serves, Standin};

/// How long to wait for a loopback write that should take no time at all.
const PATIENCE: Duration = Duration::from_secs(5);

/// Starts a fake and a feed pointed at it.
fn connected() -> (Standin, pros_link::feed::Feed) {
    let fake = Standin::start(Serves::Silence).expect("the loopback interface must exist");
    let mut feed = pros_link::feed::Feed::default();
    feed.open(fake.address(), fake.input_port())
        .expect("a fake that is listening must accept");
    (fake, feed)
}

/// **A record survives the wire.** What is pressed here is what arrives there.
///
/// The whole point of the format, and until now an assertion.
#[test]
fn what_is_pressed_here_is_what_arrives_there() {
    let (fake, mut feed) = connected();

    let mut pad = Pad::rest();
    pad.hold(Button::Cross, true);
    pad.hold(Button::L1, true);
    pad.left_x = 200;
    pad.right_y = 40;
    pad.slot = 1;
    pad.sequence = 7;
    let sent = pad.to_wire();

    assert_eq!(feed.send(&[sent]), 1, "one record went out");
    assert!(
        fake.received().wait_for(1, PATIENCE),
        "and one has to arrive"
    );

    let got = fake.received().records();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0], sent, "the bytes must not change on the way");

    let read = Pad::from_wire(&got[0]).expect("what this crate wrote, this crate must read");
    assert!(read.holds(Button::Cross), "cross was held");
    assert!(read.holds(Button::L1), "and so was L1");
    assert!(!read.holds(Button::Circle), "circle was not");
    assert_eq!(read.left_x, 200);
    assert_eq!(read.right_y, 40);
}

/// **A resting pad is not a zeroed one**, and this is where that would bite.
///
/// A zeroed record decodes to both sticks held hard left and up. If a resting pad ever
/// serialised as zeroes, a target would see a permanent diagonal - and the pad would look
/// broken in a way nothing on this side would explain.
#[test]
fn a_resting_pad_arrives_resting() {
    let (fake, mut feed) = connected();

    let sent = Pad {
        slot: 1,
        ..Pad::rest()
    }
    .to_wire();
    assert_eq!(feed.send(&[sent]), 1);
    assert!(fake.received().wait_for(1, PATIENCE), "it has to arrive");

    let read = Pad::from_wire(&fake.received().records()[0]).expect("a resting pad must decode");
    assert_eq!(read.left_x, CENTRE, "resting is the middle, not zero");
    assert_eq!(read.left_y, CENTRE);
    assert_eq!(read.right_x, CENTRE);
    assert_eq!(read.right_y, CENTRE);
    for button in Button::ALL {
        assert!(
            !read.holds(button),
            "{} must not be held at rest",
            button.name()
        );
    }
    assert!(read.is_at_rest(), "and it must say so");
}

/// **Every button survives the wire, one at a time.**
///
/// The bit table is measured and was wrong in three ways before it was checked against a
/// working implementation. A button that arrived as a different button would show here.
#[test]
fn every_button_arrives_as_itself() {
    let (fake, mut feed) = connected();

    let mut sending = Vec::new();
    for button in Button::ALL {
        let mut pad = Pad::rest();
        pad.hold(button, true);
        pad.slot = 1;
        sending.push(pad.to_wire());
    }
    let many = sending.len();
    assert_eq!(feed.send(&sending), many, "all of them went out");
    assert!(
        fake.received().wait_for(many, PATIENCE),
        "and all of them have to arrive"
    );

    for (at, button) in Button::ALL.iter().enumerate() {
        let read = Pad::from_wire(&fake.received().records()[at]).expect("each must decode");
        assert!(read.holds(*button), "{} did not survive", button.name());
        // And nothing else came with it, which is what a wrong bit looks like.
        for other in Button::ALL {
            if other != *button {
                assert!(
                    !read.holds(other),
                    "{} arrived as {} as well",
                    button.name(),
                    other.name()
                );
            }
        }
    }
}

/// **A batch is not a promise about how it arrives.**
///
/// The feed writes a frame's records in one call. TCP is free to split or join those however
/// it likes, so the other end has to reassemble by length - and a reader that assumed one read
/// is one record would pass every test above.
#[test]
fn a_batch_is_reassembled_by_length_not_by_read() {
    let (fake, mut feed) = connected();

    let many = 200;
    let sending: Vec<[u8; RECORD]> = (0..many)
        .map(|at| {
            let mut pad = Pad::rest();
            // Something different in each, so an off-by-one in reassembly cannot hide.
            pad.left_x = u8::try_from(at % 256).unwrap_or(0);
            pad.slot = 1;
            pad.sequence = u32::try_from(at).unwrap_or(0);
            pad.to_wire()
        })
        .collect();

    assert_eq!(feed.send(&sending), many, "all of them went out");
    assert!(
        fake.received().wait_for(many, PATIENCE),
        "and all of them have to arrive"
    );

    let got = fake.received().records();
    assert_eq!(got.len(), many, "no record may be lost or invented");
    for (at, record) in got.iter().enumerate() {
        let read = Pad::from_wire(record).expect("each must decode");
        assert_eq!(
            read.left_x,
            u8::try_from(at % 256).unwrap_or(0),
            "record {at} arrived out of order or misaligned"
        );
    }
}

/// A feed with nowhere to go counts what it dropped rather than losing it quietly.
///
/// **The difference that matters:** a mapping that works with nothing listening and a mapping
/// that does not work at all produce the same picture on screen unless something counts.
#[test]
fn a_feed_that_is_not_open_counts_what_it_could_not_send() {
    let mut feed = pros_link::feed::Feed::default();
    assert!(!feed.status.is_sending(), "nothing has been opened");

    let sent = feed.send(&[
        Pad {
            slot: 1,
            ..Pad::rest()
        }
        .to_wire(),
        Pad {
            slot: 2,
            ..Pad::rest()
        }
        .to_wire(),
    ]);
    assert_eq!(sent, 0, "nothing could go anywhere");
    assert_eq!(feed.dropped, 2, "and both must be counted as dropped");
    assert_eq!(feed.sent, 0);
}

/// A target that goes away is noticed, rather than silently swallowing everything after.
#[test]
fn a_feed_notices_when_the_other_end_goes() {
    let (fake, mut feed) = connected();
    assert_eq!(
        feed.send(&[Pad {
            slot: 1,
            ..Pad::rest()
        }
        .to_wire()]),
        1
    );
    assert!(fake.received().wait_for(1, PATIENCE));

    // The fake stops listening when it is dropped.
    drop(fake);

    // A closed socket is not always noticed on the first write - the first one lands in a
    // buffer for a connection that is gone. What matters is that it is noticed at all rather
    // than reporting success forever.
    let mut noticed = false;
    for at in 0..200 {
        feed.send(&[Pad {
            slot: 1,
            sequence: at + 1,
            ..Pad::rest()
        }
        .to_wire()]);
        if !feed.status.is_sending() {
            noticed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        noticed,
        "a feed to a target that has gone must stop claiming to be sending"
    );
    assert!(feed.dropped > 0, "and must count what did not get there");
}
