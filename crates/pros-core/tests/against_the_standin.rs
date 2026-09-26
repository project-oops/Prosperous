//! `pros-core::watch` against the stand-in's video port, over real loopback connections.
//!
//! Each test produces one fault the watcher claims to tell apart and checks what it says,
//! including the faults that look identical from outside. The pump takes its sink as an
//! argument, so the stream is watched into a buffer and no media player starts; that also
//! checks that what a player would be handed is byte-for-byte what arrived.

use std::io::Read as _;
use std::net::TcpStream;
use std::time::Duration;

use pros_link::standin::{Serves, Standin, video};

/// How long a test waits for a loopback stream that should take no time at all.
const PATIENCE: Duration = Duration::from_secs(5);

/// Connects to a fake and watches it into a buffer, returning what arrived and what was
/// counted.
fn watch(serves: Serves) -> (Vec<u8>, pros_core::watch::Counts, String) {
    watch_with(serves, &pros_core::watch::Watching::idle())
}

/// The same, with the watcher supplied, for a test that needs control of the rate window.
fn watch_with(
    serves: Serves,
    watching: &pros_core::watch::Watching,
) -> (Vec<u8>, pros_core::watch::Counts, String) {
    let fake = Standin::start(serves).expect("the loopback interface must exist");
    let mut from = TcpStream::connect((fake.address(), fake.video_port()))
        .expect("a fake that is listening must accept");
    from.set_read_timeout(Some(PATIENCE))
        .expect("a socket must take a timeout");

    let mut into: Vec<u8> = Vec::new();
    let why = pros_core::watch::carry_into(&mut from, &mut into, watching);
    (into, watching.counts(), why)
}

/// Puts an ended run back into the state it was in while it ran.
///
/// [`pros_core::watch::Counts::diagnose`] speaks only while a stream is running, and these
/// tests read counts after the fake has closed. A finished run also records the player as
/// gone, and watching with a dead player would be diagnosed on the player, so both are reset.
fn as_if_still_running(counts: &mut pros_core::watch::Counts) {
    counts.status = pros_core::watch::Status::Watching;
    counts.player_alive = true;
}

/// A working stream arrives byte-for-byte, fully counted, with nothing diagnosed.
#[test]
fn a_working_stream_arrives_byte_for_byte() {
    let serves = Serves::Video {
        units: 24,
        apart: Duration::ZERO,
    };
    let (arrived, counts, why) = watch(serves.clone());

    assert_eq!(
        arrived,
        video(&serves),
        "the player must be handed what the target sent, unchanged"
    );
    assert_eq!(
        u64::try_from(arrived.len()).unwrap_or(u64::MAX),
        counts.bytes
    );
    assert_eq!(counts.units, 24, "every unit sent was counted");
    assert_eq!(counts.keyframes, 3, "one in eight, so three in twenty-four");
    assert_eq!(
        counts.diagnose(),
        None,
        "a working stream has nothing to complain about"
    );
    assert!(
        why.contains("closed"),
        "it ended because the target ended it"
    );
}

/// The last unit is counted at the end of a stream, so one keyframe is not reported as none.
///
/// A unit is known to be whole only when the next start code arrives, so the last one read is
/// held until the stream ends.
#[test]
fn a_single_keyframe_is_counted_rather_than_left_held() {
    let (arrived, counts, _) = watch(Serves::Video {
        units: 1,
        apart: Duration::ZERO,
    });

    assert!(!arrived.is_empty(), "one unit did arrive");
    assert_eq!(counts.units, 1, "and the last unit must not be left held");
    assert_eq!(counts.keyframes, 1, "it was a keyframe, and it must say so");
    assert_eq!(
        counts.pending, 0,
        "nothing may still be waiting for a boundary once the stream has ended"
    );

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    assert_eq!(
        watching.diagnose(),
        None,
        "a stream carrying one perfectly good keyframe must not be reported as having none"
    );
}

/// A framed stream with no keyframe is diagnosed, though it looks like a dead one.
#[test]
fn a_stream_with_no_keyframe_is_told_apart_from_a_dead_one() {
    let (arrived, counts, _) = watch(Serves::Dependent { units: 40 });

    assert!(!arrived.is_empty(), "it genuinely arrived");
    assert_eq!(counts.units, 40, "and it genuinely framed");
    assert_eq!(counts.keyframes, 0, "with nothing to begin at");

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    let said = watching.diagnose().expect("this must not pass silently");
    assert!(said.contains("no keyframe"), "{said}");
    assert!(
        said.contains("looks exactly like no stream at all"),
        "the point is that it is deceptive, and the message should say so: {said}"
    );
}

/// Bytes that never frame are diagnosed as such.
#[test]
fn bytes_that_never_frame_are_named_as_such() {
    let (arrived, counts, _) = watch(Serves::Noise { bytes: 8192 });

    assert_eq!(arrived.len(), 8192, "it all arrived");
    assert_eq!(counts.units, 0, "and none of it was a unit");

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    let said = watching.diagnose().expect("this must not pass silently");
    assert!(said.contains("none of it framed"), "{said}");
}

/// A start code split across two reads is still one unit.
#[test]
fn a_start_code_split_across_reads_is_still_one_unit() {
    let (arrived, counts, _) = watch(Serves::Awkward);

    assert_eq!(
        arrived,
        video(&Serves::Awkward),
        "cut badly and reassembled, it is still the same stream"
    );
    // Eight units sent in pieces as small as one byte, with start codes straddling the cuts.
    assert_eq!(counts.units, 8, "the cuts must not create or destroy units");
    assert_eq!(counts.keyframes, 1);
    assert_eq!(
        counts.diagnose(),
        None,
        "a badly cut stream is still a stream"
    );
}

/// Connected with nothing arriving is its own state, and not an end.
#[test]
fn connected_and_silent_is_not_the_same_as_ended() {
    let fake = Standin::start(Serves::Silence).expect("the loopback interface must exist");
    let mut from = TcpStream::connect((fake.address(), fake.video_port()))
        .expect("a fake that is listening must accept");
    // Short, since nothing is expected to arrive.
    from.set_read_timeout(Some(Duration::from_millis(200)))
        .expect("a socket must take a timeout");

    let mut buffer = [0_u8; 64];
    let read = from.read(&mut buffer);
    assert!(
        matches!(&read, Err(why) if why.kind() == std::io::ErrorKind::WouldBlock
            || why.kind() == std::io::ErrorKind::TimedOut),
        "a payload that is running and not producing must not look like a closed socket: \
         {read:?}"
    );

    let mut counts = pros_core::watch::Counts {
        status: pros_core::watch::Status::Watching,
        ..pros_core::watch::Counts::default()
    };
    counts.bytes = 0;
    let said = counts.diagnose().expect("silence must be reported");
    assert!(said.contains("nothing has arrived"), "{said}");
}

/// A target that closes ends the stream as ended, not idle, and keeps its counts.
#[test]
fn a_target_that_closes_ends_the_stream_and_says_why() {
    let (_, counts, why) = watch(Serves::Video {
        units: 4,
        apart: Duration::ZERO,
    });

    assert!(why.contains("closed the connection"), "{why}");
    match counts.status {
        pros_core::watch::Status::Ended(said) => {
            assert!(said.contains("closed"), "{said}");
        }
        other => panic!("a finished stream must report as ended, not {other:?}"),
    }
    assert_ne!(
        counts.units, 0,
        "and it must keep what it counted before it ended"
    );
}

/// Asking it to stop stops it, and that reason is distinct from a target going away.
#[test]
fn stopping_is_reported_as_stopping_rather_than_as_a_fault() {
    let fake = Standin::start(Serves::Silence).expect("the loopback interface must exist");
    let mut from = TcpStream::connect((fake.address(), fake.video_port()))
        .expect("a fake that is listening must accept");
    from.set_read_timeout(Some(Duration::from_millis(50)))
        .expect("a socket must take a timeout");

    let watching = pros_core::watch::Watching::idle();
    // Asked before it begins, so the first pass through the loop sees it.
    watching.stop();
    let mut into: Vec<u8> = Vec::new();
    let why = pros_core::watch::carry_into(&mut from, &mut into, &watching);

    assert_eq!(why, "stopped", "a deliberate stop is not a failure");
    assert!(
        !watching.counts().status.is_watching(),
        "and it must not still claim to be watching"
    );
}

/// A refused connection's failure names the port.
#[test]
fn a_port_nothing_serves_refuses_and_names_itself() {
    // Bound and dropped, so the port is real and free.
    let port = {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback must exist");
        taken
            .local_addr()
            .expect("a bound socket has an address")
            .port()
    };

    let watching = pros_core::watch::Watching::start("127.0.0.1", port, "no-such-player -");
    let until = std::time::Instant::now() + PATIENCE;
    while std::time::Instant::now() < until {
        if !matches!(watching.counts().status, pros_core::watch::Status::Idle) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    match watching.counts().status {
        pros_core::watch::Status::Failed(why) => {
            assert!(
                why.contains(&port.to_string()),
                "the message has to name the port, or it says nothing useful: {why}"
            );
        }
        other => panic!("nothing is serving that port, so this must fail: {other:?}"),
    }
}

/// A stream at a real frame rate is measured as moving.
///
/// Slow on purpose: the rate window is one real second. `thread::sleep` never sleeps less
/// than asked, so eighty units 20 ms apart take at least 1.6 s and a rate is always measured;
/// the assertion is on the threshold because a coarse timer can make it slower.
#[test]
fn a_stream_fast_enough_to_be_a_stream_is_measured_as_one() {
    let (_, counts, _) = watch(Serves::Video {
        units: 80,
        apart: Duration::from_millis(20),
    });

    let rate = counts
        .rate
        .expect("1.6 seconds is longer than the window, so a rate must have been measured");
    assert!(
        rate.is_moving(),
        "eighty units over about two seconds is a stream, not a slideshow: {}",
        rate.describe()
    );
    assert!(rate.bytes > 0.0, "and bytes were moving too");

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    assert_eq!(
        watching.diagnose(),
        None,
        "a stream arriving at a real rate has nothing wrong with it"
    );
}

/// A stream too slow to be a stream is diagnosed as a slideshow, with every other count
/// healthy.
#[test]
fn a_stream_too_slow_to_be_a_stream_is_named_as_a_slideshow() {
    let (_, counts, _) = watch(Serves::Video {
        units: 10,
        apart: Duration::from_millis(150),
    });

    assert!(counts.units > 0, "it framed");
    assert!(counts.keyframes > 0, "and a decoder had somewhere to start");

    let rate = counts
        .rate
        .expect("1.5 seconds is longer than the window, so a rate must have been measured");
    assert!(
        !rate.is_moving(),
        "under seven a second is a slideshow: {}",
        rate.describe()
    );

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    let said = watching
        .diagnose()
        .expect("a slideshow must not pass as a working stream");
    assert!(said.contains("slideshow"), "{said}");
}

/// A stream shorter than the rate window has no rate, not a rate of zero.
#[test]
fn a_stream_shorter_than_the_window_has_no_rate_rather_than_a_rate_of_zero() {
    // An hour-long window, so the stream is shorter than it by construction even when other
    // tests load the machine.
    let (_, counts, _) = watch_with(
        Serves::Video {
            units: 24,
            apart: Duration::ZERO,
        },
        &pros_core::watch::Watching::idle_measuring_over(Duration::from_hours(1)),
    );

    assert!(counts.units > 0, "it did arrive, and quickly");
    assert_eq!(
        counts.rate, None,
        "nobody measured a second, so there is no rate to report"
    );

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    assert_eq!(
        watching.diagnose(),
        None,
        "and it must not be accused of stalling for want of a measurement"
    );
}

/// A quiet connected socket is waited on across read timeouts, on this platform's timeout kind.
///
/// Unix reports a read timeout as `WouldBlock` and Windows as `TimedOut`; the pump treats
/// both as a pause.
#[test]
fn a_quiet_socket_is_waited_on_rather_than_given_up_on() {
    let fake = Standin::start(Serves::Silence).expect("the loopback interface must exist");
    let mut from = TcpStream::connect((fake.address(), fake.video_port()))
        .expect("a fake that is listening must accept");
    // Short, so several reads time out during the wait below.
    from.set_read_timeout(Some(Duration::from_millis(50)))
        .expect("a socket must take a timeout");

    let watching = pros_core::watch::Watching::idle();
    let mut into: Vec<u8> = Vec::new();
    // Stopped before anything is asserted, so a failing assertion cannot leave the pump
    // running.
    let (still_watching, why) = std::thread::scope(|scope| {
        let pump = scope.spawn(|| pros_core::watch::carry_into(&mut from, &mut into, &watching));
        std::thread::sleep(Duration::from_millis(300));
        let still_watching = watching.counts().status.is_watching();
        watching.stop();
        (
            still_watching,
            pump.join().expect("the pump must not panic"),
        )
    });

    assert!(
        still_watching,
        "a quiet socket must still be watched after several timeouts, not ended"
    );
    assert_eq!(
        why, "stopped",
        "the only reason it ended is that it was asked to"
    );
    assert_eq!(
        watching.counts().bytes,
        0,
        "nothing arrived, which is the premise"
    );
}
