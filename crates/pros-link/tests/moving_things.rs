//! Files and the manager's web service, against a target that is not one.
//!
//! The awkward parts here are a transfer on a second connection the server names, and a
//! body with no stated length.

use pros_link::fake::{Behaviour, Fake, Store};
use pros_link::files::{Kind, Session};
use pros_link::{Error, manager};

/// The address in a passive reply (here a claimed `10.0.0.1`) is ignored for the one dialled.
#[test]
fn the_address_a_server_claims_is_not_the_one_that_is_dialled() {
    let contents = Store::new(&[("report.txt", b"measured\n")]);
    let fake = files_fake(&contents, [10, 0, 0, 1], true);

    let got = retrieve_from(&fake, "report.txt").expect("the transfer happens on loopback");
    assert_eq!(got, b"measured\n");
}

/// A server that will not do binary mode fails the session outright.
#[test]
fn a_server_that_refuses_binary_mode_is_not_used_at_all() {
    let contents = Store::new(&[("payload.elf", b"\x7fELF")]);
    let fake = files_fake(&contents, [127, 0, 0, 1], false);

    let error = Session::open_at(fake.address(), fake.port())
        .expect_err("text mode must not be accepted quietly");
    assert!(
        matches!(error, Error::Rejected { .. }),
        "expected a refusal naming what the server said, got {error:?}"
    );
}

/// Bytes go across unedited, including the ones a text-mode transfer would rewrite.
#[test]
fn a_stored_file_arrives_byte_for_byte() {
    let contents = Store::new(&[]);
    let fake = files_fake(&contents, [127, 0, 0, 1], true);

    // A line ending, a lone carriage return and a zero, which text mode would rewrite.
    let payload: &[u8] = b"\x7fELF\r\n\r\x00\x01\x02end";
    let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
    session
        .store("sent.bin", payload)
        .expect("the fake accepts it");
    session.close();

    assert_eq!(
        contents.get("sent.bin").as_deref(),
        Some(payload),
        "what arrived is not what was sent"
    );
}

/// A file that is not there is a refusal carrying the server's words, not a broken link.
#[test]
fn a_missing_file_is_a_refusal_rather_than_a_failure() {
    let contents = Store::new(&[("here.txt", b"yes")]);
    let fake = files_fake(&contents, [127, 0, 0, 1], true);

    let error = retrieve_from(&fake, "not-here.txt").expect_err("there is no such file");
    match error {
        Error::Rejected { reply, .. } => assert!(
            reply.contains("550"),
            "the server's own words are missing: {reply}"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A listing keeps and marks a line it could not read, and skips its own header.
#[test]
fn a_listing_keeps_what_it_could_not_read() {
    // Absolute keys: the fake lists `/` by its members' basenames.
    let contents = Store::new(&[("/one.txt", b"1"), ("/two.txt", b"22")]);
    let fake = files_fake(&contents, [127, 0, 0, 1], true);

    let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
    let entries = session.list("/").expect("the listing arrives");
    session.close();

    let names: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.is_usable())
        .map(|entry| entry.name.as_str())
        .collect();
    assert!(names.contains(&"one.txt"), "a file is missing: {names:?}");
    assert!(
        names.contains(&"a directory"),
        "a name with a space in it was truncated: {names:?}"
    );
    // The header is not an entry; a line that is neither is kept and marked.
    assert!(
        entries
            .iter()
            .any(|entry| entry.kind == Kind::Unrecognised
                && entry.raw.contains("not a listing entry")),
        "a line that is not an entry was dropped rather than reported"
    );
    assert!(
        !entries.iter().any(|entry| entry.raw.starts_with("total ")),
        "the listing's own header was reported as an entry"
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.name == "two.txt")
            .and_then(|entry| entry.size),
        Some(2),
        "the size column was not read"
    );
}

/// One session answers several commands without its replies falling one behind.
#[test]
fn a_session_does_several_things_without_losing_its_place() {
    let contents = Store::new(&[("first.txt", b"one"), ("second.txt", b"two")]);
    let fake = files_fake(&contents, [127, 0, 0, 1], true);

    let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
    let listed = session.list("/").expect("a listing");
    let first = session.retrieve("first.txt").expect("the first file");
    let second = session.retrieve("second.txt").expect("the second file");
    session.close();

    assert!(!listed.is_empty());
    assert_eq!(first, b"one");
    assert_eq!(second, b"two", "the second answer is one behind");
}

/// A chunked body arrives whole, with no chunk sizes left in the data.
#[test]
fn a_body_sent_in_pieces_is_reassembled() {
    let body = "{\"payloads\":[{\"name\":\"elfldr\",\"checksum\":\"abc\"}]}";
    let fake = Fake::start(Behaviour::Serves {
        status: 200,
        body: body.to_owned(),
        chunked: true,
    })
    .expect("the fake binds");

    let got = manager::fetch_at(fake.address(), fake.port(), "/repository").expect("it answers");
    assert_eq!(String::from_utf8_lossy(&got), body);
}

/// A body with a stated length arrives at exactly that length.
#[test]
fn a_body_with_a_length_arrives_at_that_length() {
    let body = "twenty-nine bytes exactly here";
    let fake = Fake::start(Behaviour::Serves {
        status: 200,
        body: body.to_owned(),
        chunked: false,
    })
    .expect("the fake binds");

    let got = manager::fetch_at(fake.address(), fake.port(), "/anything").expect("it answers");
    assert_eq!(got.len(), body.len(), "the body was cut short or ran on");
}

/// A non-success status is a refusal carrying the status line, never a body.
#[test]
fn a_status_that_is_not_success_carries_the_servers_own_words() {
    let fake = Fake::start(Behaviour::Serves {
        status: 404,
        body: "no such thing".to_owned(),
        chunked: false,
    })
    .expect("the fake binds");

    let error =
        manager::fetch_at(fake.address(), fake.port(), "/nowhere").expect_err("404 is not a body");
    match error {
        Error::Rejected { reply, .. } => assert!(
            reply.contains("404"),
            "the status is missing from the refusal: {reply}"
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// A stored file's size reads back, and a missing one is a refusal.
#[test]
fn a_stored_files_size_reads_back() {
    let contents = Store::new(&[]);
    let fake = files_fake(&contents, [127, 0, 0, 1], true);

    let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
    session
        .store("sent.bin", b"nineteen bytes here")
        .expect("the fake accepts it");
    assert_eq!(
        session.size("sent.bin").expect("a size comes back"),
        19,
        "the size read back is not the size sent"
    );
    let missing = session
        .size("not-here.bin")
        .expect_err("a file that is not there has no size");
    assert!(
        matches!(missing, Error::Rejected { .. }),
        "a missing size should be a refusal, got {missing:?}"
    );
    session.close();
}

/// The target's non-standard `226 Directory created` reply to `MKD` is success.
#[test]
fn a_directory_the_target_makes_with_226_is_not_a_refusal() {
    let contents = Store::new(&[]);
    let fake = files_fake(&contents, [127, 0, 0, 1], true);

    let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
    session
        .make_directory("/data/homebrew/MESA00001")
        .expect("226 is a made directory, not a refusal");
    session.close();
}

/// Starts a fake file service over the given contents.
fn files_fake(contents: &Store, claims: [u8; 4], binary: bool) -> Fake {
    Fake::start(Behaviour::Files {
        contents: contents.clone(),
        claims,
        binary,
        swallows_stores: false,
    })
    .expect("the fake binds")
}

/// Fetches one file from a fake.
fn retrieve_from(fake: &Fake, path: &str) -> Result<Vec<u8>, Error> {
    let mut session = Session::open_at(fake.address(), fake.port())?;
    let got = session.retrieve(path);
    session.close();
    got
}
