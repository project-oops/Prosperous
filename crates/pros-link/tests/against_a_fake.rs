//! What this crate does against a target that is not one.
//!
//! Every test here is about an awkwardness rather than a happy path, because the happy
//! paths are three lines each and the awkwardness is the interface: a stream with no end,
//! a server with no framing, a loader that may not answer.
//!
//! What none of this establishes is whether a real target agrees. That needs target,
//! and the difference between the two kinds of evidence should stay visible.

use std::time::{Duration, Instant};

use pros_link::fake::{Behaviour, Fake};
use pros_link::{Error, Shape, log, service, shell};

/// A log that never ends is read for a window and then let go.
///
/// The failure this pins: waiting for an EOF that is never coming, which reads as a hang
/// rather than as a bug.
#[test]
fn a_stream_with_no_end_is_read_for_a_window_and_no_longer() {
    let fake =
        Fake::start(Behaviour::Streams("kernel: something\n".to_owned())).expect("the fake binds");
    let started = Instant::now();
    let got = read_from(&fake, Duration::from_millis(300));
    let took = started.elapsed();

    assert!(
        got.contains("kernel: something"),
        "nothing arrived: {got:?}"
    );
    assert!(
        took < Duration::from_secs(3),
        "reading a stream took {took:?}, which means it waited for an end that never comes"
    );
}

/// A quiet log is a result, not a failure.
///
/// A target with nothing to say has told you something, and turning that into an error
/// would make silence look like a broken tool.
#[test]
fn a_quiet_stream_answers_with_nothing_rather_than_failing() {
    let fake = Fake::start(Behaviour::Silent).expect("the fake binds");
    let got = read_from(&fake, Duration::from_millis(200));
    assert!(got.is_empty(), "expected silence, got {got:?}");
}

/// The shell has no framing, so the reader stops when the server does.
#[test]
fn a_server_with_no_framing_is_read_until_it_goes_quiet() {
    let fake = Fake::start(Behaviour::Shell {
        banner: "welcome\n".to_owned(),
        reply: "total 0\n".to_owned(),
    })
    .expect("the fake binds");

    let got = shell_on(&fake, "ls", Duration::from_millis(300));
    assert!(got.contains("total 0"), "the reply did not arrive: {got:?}");
}

/// The banner is drained before the command is typed.
///
/// Otherwise the command lands in the middle of the greeting and the reply contains both,
/// which is the shape of bug that looks like the server being strange.
#[test]
fn the_banner_does_not_end_up_in_the_answer() {
    let fake = Fake::start(Behaviour::Shell {
        banner: "welcome to the target\n".to_owned(),
        reply: "answer\n".to_owned(),
    })
    .expect("the fake binds");

    let got = shell_on(&fake, "anything", Duration::from_millis(300));
    assert!(got.contains("answer"), "no answer: {got:?}");
    assert!(
        !got.contains("welcome to the target"),
        "the banner leaked into the answer: {got:?}"
    );
}

/// A port that nothing is listening on refuses, and says which port.
#[test]
fn a_closed_port_is_a_refusal_naming_the_port() {
    // Bound and dropped, so the port is almost certainly free and certainly not ours.
    let port = {
        let fake = Fake::start(Behaviour::Silent).expect("the fake binds");
        fake.port()
    };
    let found = service::probe("127.0.0.1", port, Duration::from_millis(400));
    assert!(
        !found.open,
        "something answered on a port nothing should hold"
    );
    assert!(
        found.took < Duration::from_secs(2),
        "a local refusal took {:?}",
        found.took
    );
}

/// A probe carries how long the answer took, because the two kinds of no differ.
#[test]
fn a_probe_reports_its_own_duration() {
    let fake = Fake::start(Behaviour::Silent).expect("the fake binds");
    let found = service::probe("127.0.0.1", fake.port(), Duration::from_millis(400));
    assert!(found.open, "the fake did not answer");
    assert!(
        found.took < Duration::from_secs(1),
        "a loopback connection took {:?}",
        found.took
    );
}

/// An address that resolves to nothing is a different answer from a refusal.
///
/// Retrying fixes neither, but only one of them is a typo somebody can correct.
#[test]
fn an_unresolvable_address_is_not_a_refusal() {
    let error = log::read(
        &pros_link::Link::to("no-such-host.invalid"),
        Duration::from_millis(200),
    )
    .expect_err("an invalid host should not resolve");
    assert!(
        matches!(error, Error::Unresolved { .. }),
        "expected an unresolved address, got {error:?}"
    );
}

/// The guard refuses a vendor module before opening a connection.
///
/// This is the whole reason the crate reads an ELF header at all: the loader accepts this
/// file, maps it, and dies without printing anything.
#[test]
fn a_vendor_module_is_refused_before_anything_is_sent() {
    let mut module = vec![0_u8; 64];
    module[..4].copy_from_slice(&[0x7f, 0x45, 0x4c, 0x46]);
    module[0x10..0x12].copy_from_slice(&0xFE10_u16.to_le_bytes());

    // A host that cannot resolve: if the guard were checked after connecting, this would
    // fail with an address error instead, and that difference is the assertion.
    let error = pros_link::loader::send(
        &pros_link::Link::to("no-such-host.invalid"),
        &module,
        Duration::ZERO,
    )
    .expect_err("a vendor module is not a payload");

    match error {
        Error::WrongShape { found } => {
            assert_eq!(found, Shape::VendorExecutable);
            let said = error_text(&Error::WrongShape { found });
            assert!(
                said.contains("emulator"),
                "the refusal does not say where it goes: {said}"
            );
        }
        other => panic!("expected a shape refusal before any connection, got {other:?}"),
    }
}

/// A payload that says nothing is not an error.
///
/// The loader duplicates its socket onto the payload output, but a payload started any
/// other way has no such socket - so a send that hears nothing back has still worked.
#[test]
fn a_payload_that_never_answers_is_still_a_successful_send() {
    let fake = Fake::start(Behaviour::Accepts {
        then: Box::new(Behaviour::Silent),
    })
    .expect("the fake binds");

    let payload = payload_bytes();
    let got = send_to(&fake, &payload, Duration::from_millis(200))
        .expect("a silent payload is not a failure");
    assert!(got.is_empty(), "expected silence, got {got:?}");
}

/// What the payload prints comes back when it does print.
#[test]
fn what_a_payload_prints_comes_back() {
    let fake = Fake::start(Behaviour::Accepts {
        then: Box::new(Behaviour::Says("probe: running\n".to_owned())),
    })
    .expect("the fake binds");

    let payload = payload_bytes();
    let got = send_to(&fake, &payload, Duration::from_millis(400)).expect("the send works");
    assert!(got.contains("probe: running"), "no output: {got:?}");
}

/// Reads the fake log for a window.
fn read_from(fake: &Fake, window: Duration) -> String {
    log::read_at(fake.address(), fake.port(), window).expect("the fake answers")
}

/// Runs a command against the fake shell.
fn shell_on(fake: &Fake, command: &str, settle: Duration) -> String {
    shell::run_at(fake.address(), fake.port(), command, settle).expect("the fake answers")
}

/// Sends a payload to the fake loader.
fn send_to(fake: &Fake, payload: &[u8], listen: Duration) -> Result<String, Error> {
    pros_link::loader::send_at(fake.address(), fake.port(), payload, listen)
}

/// A minimal plain shared object: magic plus the one field the guard reads.
fn payload_bytes() -> Vec<u8> {
    let mut bytes = vec![0_u8; 64];
    bytes[..4].copy_from_slice(&[0x7f, 0x45, 0x4c, 0x46]);
    bytes[0x10..0x12].copy_from_slice(&0x0003_u16.to_le_bytes());
    bytes
}

fn error_text(error: &Error) -> String {
    error.to_string()
}
