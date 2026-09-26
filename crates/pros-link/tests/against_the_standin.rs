//! The input half of the stand-in, against a payload that is not a payload.
//!
//! `pad` builds a 24-byte record, `pads` decides when to send one, and `feed` puts it on a
//! socket. These tests read the records back from the other end of a real socket. The bit
//! layout is measured and credited to Ghostpad.

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

/// A record survives the wire: what is pressed here is what arrives there.
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

/// A resting pad arrives centred, not zeroed (zero is both sticks hard left and up).
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

/// Every button arrives as itself and as no other.
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

/// A batch written in one call is reassembled by record length, in order.
#[test]
fn a_batch_is_reassembled_by_length_not_by_read() {
    let (fake, mut feed) = connected();

    let many = 200;
    let sending: Vec<[u8; RECORD]> = (0..many)
        .map(|at| {
            let mut pad = Pad::rest();
            // A distinct value per record exposes an off-by-one in reassembly.
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

/// A feed with nowhere to go counts what it dropped.
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

/// A feed notices when the other end goes away.
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

    drop(fake);

    // The first write after a close can land in a buffer, so allow several attempts.
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
