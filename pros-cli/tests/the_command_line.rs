//! The command line, run as a command line.
//!
//! These spawn the binary because the exit code and the printed text are only observable
//! from outside it. A child process also takes its environment as an argument, so each test
//! gets its own registry without setting process-global environment variables, which is
//! unsafe in this edition.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, PoisonError};

use std::time::Duration;

use pros_link::fake::{Behaviour, Fake, Store};
use pros_link::service::SERVICES;

/// Long enough for a loopback answer, short enough that five of them are not a wait.
const SHORT: Duration = Duration::from_millis(300);

/// Held by every test that cares whether the target's own ports are answering.
///
/// The command line reaches for fixed port numbers, so a test that stands fakes on them and a
/// test that asserts nothing is there cannot run at the same time. The ports are not moved,
/// because a real run is not told where to look.
static WELL_KNOWN_PORTS: Mutex<()> = Mutex::new(());

/// An absent target exits 2, distinct from a tool failure, and names the remedy.
#[test]
fn a_target_that_is_not_there_is_blocked_and_says_what_to_do() {
    let _ports = WELL_KNOWN_PORTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let home = scratch("absent");
    register(&home, "127.0.0.1");

    // The premise: a stand-in left running on this machine would answer on these ports.
    for service in SERVICES {
        let answered = pros_link::probe("127.0.0.1", service.port, SHORT).open;
        assert!(
            !answered,
            "port {} ({}) is answering here, so a target being absent could not be established - that is this machine's state, not a fault in the tool",
            service.port, service.name
        );
    }

    let out = pros(&home).arg("check").output().expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(
        said.contains("re-running the entry point"),
        "the remedy is missing: {said}"
    );
    // The loader being unreachable is not a diagnosis of the target, which can run its whole
    // chain with 9021 unreachable.
    assert!(
        said.contains("says nothing about the target"),
        "it stated more than was measured: {said}"
    );
    // The other blocked verdict assumes the loader is up.
    assert!(
        !said.contains("can be sent again"),
        "it offered the remedy for a different failure: {said}"
    );
}

/// A target with the chain up reports what each service makes possible, and what is dimmed.
#[test]
fn a_target_that_answers_reports_what_each_service_buys() {
    let _ports = WELL_KNOWN_PORTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let home = scratch("present");
    register(&home, "127.0.0.1");

    // Everything except the dashboard, so there is something optional to report on.
    let dashboard = "pldmgr";
    let _services: Vec<Fake> = SERVICES
        .iter()
        .filter(|service| service.name != dashboard)
        .map(|service| {
            Fake::start_at(service.port, Behaviour::Silent).unwrap_or_else(|error| {
                panic!(
                    "port {} is already held on this machine, so this could not be checked \
                     - that is a failure, not a reason to move: {error}",
                    service.port
                )
            })
        })
        .collect();

    let out = pros(&home).arg("check").output().expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(
        said.contains("send a payload to the target and run it"),
        "a service is described by its port rather than by what it buys: {said}"
    );
    assert!(
        said.contains(dashboard) && said.contains("invisible"),
        "an absent optional service should dim rather than block: {said}"
    );
}

/// An unregistered name exits 1, distinct from a target being down.
#[test]
fn a_name_that_is_not_registered_is_a_different_failure_from_a_target_being_down() {
    let home = scratch("unknown");
    register(&home, "127.0.0.1");

    let out = pros(&home)
        .args(["check", "--name", "not-registered"])
        .output()
        .expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("not-registered"), "which name: {said}");
}

/// A manifest names its unverifiable entries from the file alone, with no target.
#[test]
fn a_manifest_says_which_entries_cannot_be_verified() {
    let home = scratch("manifest");
    let file = home.join("payloads.json");
    std::fs::write(&file, MANIFEST).expect("the manifest is written");

    let out = pros(&home)
        .arg("payloads")
        .arg(&file)
        .output()
        .expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("2 of these cannot be verified"), "{said}");
    assert!(
        said.contains("md5"),
        "it does not name the algorithm: {said}"
    );
    assert!(
        said.contains("no checksum"),
        "an entry with no digest at all is a different problem and should read as one: {said}"
    );
}

/// A file that is not the file the manifest describes is refused, and both digests are said.
#[test]
fn the_wrong_file_is_refused_and_the_message_carries_both_digests() {
    let home = scratch("verify");
    let file = home.join("payloads.json");
    std::fs::write(&file, MANIFEST).expect("the manifest is written");
    let wrong = home.join("wrong.elf");
    std::fs::write(&wrong, b"not the payload that was described").expect("written");

    let out = pros(&home)
        .arg("verify")
        .arg(&wrong)
        .args(["--against", "elfldr", "--manifest"])
        .arg(&file)
        .output()
        .expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("mismatch"), "{said}");
    assert!(
        said.contains("do not send this"),
        "a mismatch should say what not to do next: {said}"
    );
}

/// An entry whose digest cannot be checked is refused, never reported as verified.
#[test]
fn an_entry_that_cannot_be_verified_is_refused_rather_than_passed() {
    let home = scratch("unverifiable");
    let file = home.join("payloads.json");
    std::fs::write(&file, MANIFEST).expect("the manifest is written");
    let any = home.join("any.elf");
    std::fs::write(&any, b"anything at all").expect("written");

    let out = pros(&home)
        .arg("verify")
        .arg(&any)
        .args(["--against", "klogsrv", "--manifest"])
        .arg(&file)
        .output()
        .expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(said.contains("cannot be checked"), "{said}");
}

/// The wrong kind of file is refused before any send is announced.
#[test]
fn a_vendor_module_is_refused_without_anything_being_announced() {
    let home = scratch("shape");
    register(&home, "127.0.0.1");
    let module = home.join("module.eboot.bin");
    let mut bytes = vec![0_u8; 64];
    bytes[..4].copy_from_slice(&[0x7f, 0x45, 0x4c, 0x46]);
    bytes[0x10..0x12].copy_from_slice(&0xFE10_u16.to_le_bytes());
    std::fs::write(&module, &bytes).expect("written");

    let out = pros(&home).arg("send").arg(&module).output().expect("runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        !said.contains("sending"),
        "it announced a send that did not happen: {said}"
    );
    assert!(
        said.contains("vendor executable"),
        "it does not say what the file actually is: {said}"
    );
}

/// The target's own repository can be read as a source and is judged like a file on disk.
///
/// The stand-in serves a manifest in this project's own shape, so this covers the plumbing
/// and says nothing about the real file's shape.
#[test]
fn the_target_can_be_asked_for_its_own_repository() {
    let _ports = WELL_KNOWN_PORTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let home = scratch("repository");
    register(&home, "127.0.0.1");

    let held = "/data/pldmgr/repository_cache.json";
    let contents = Store::new(&[(held, MANIFEST.as_bytes())]);
    let file_service = SERVICES
        .iter()
        .find(|service| service.name == "ftpsrv")
        .expect("a file service");
    let _fake = Fake::start_at(
        file_service.port,
        Behaviour::Files {
            contents,
            // Wrong on purpose: the client dials the address that reached the target, not
            // the one the target claims.
            claims: [10, 0, 0, 1],
            binary: true,
            swallows_stores: false,
        },
    )
    .unwrap_or_else(|error| panic!("port {} is already held here: {error}", file_service.port));

    let out = pros(&home)
        .args(["payloads", "--from-target", held])
        .output()
        .expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("elfldr"), "{said}");
    assert!(
        said.contains("2 of these cannot be verified"),
        "a manifest read off the target is judged exactly as one read from disk: {said}"
    );
}

/// With no manifest set up, the built-in list is shown and says where it came from.
#[test]
fn a_machine_with_no_manifest_gets_the_built_in_list() {
    let home = scratch("nosource");
    let out = pros(&home).arg("payloads").output().expect("it runs");
    let said = text(&out);

    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(said.contains("built-in list"), "{said}");
    assert!(
        said.contains("elfldr") && said.contains("ftpsrv"),
        "the recommended list should name what a target needs: {said}"
    );
    assert!(
        said.contains("read off a target's own repository"),
        "{said}"
    );
}

/// Writing the built-in list never overwrites an existing manifest.
#[test]
fn writing_the_built_in_list_never_destroys_one_already_there() {
    let home = scratch("write");

    let first = pros(&home)
        .args(["payloads", "--write"])
        .output()
        .expect("it runs");
    assert_eq!(first.status.code(), Some(0), "{}", text(&first));

    let again = pros(&home)
        .args(["payloads", "--write"])
        .output()
        .expect("it runs");
    let said = text(&again);
    assert_eq!(again.status.code(), Some(1), "{said}");
    assert!(said.contains("not overwritten"), "{said}");
}

/// A manifest with one good entry, one digest that cannot be checked, and one with none.
const MANIFEST: &str = r#"[
    {
        "name": "elfldr",
        "checksum": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    },
    { "name": "klogsrv", "checksum": "d41d8cd98f00b204e9800998ecf8427e" },
    { "name": "shsrv" }
]"#;

/// The program under test, with its registry pointed somewhere harmless.
fn pros(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pros"));
    // Both, because the registry looks for whichever this platform uses.
    command.env("USERPROFILE", home).env("HOME", home);
    command
}

/// Registers a target in the scratch home.
fn register(home: &Path, address: &str) {
    let out = pros(home)
        .args(["register", address, "--name", "stand-in"])
        .output()
        .expect("it runs");
    assert!(out.status.success(), "could not register: {}", text(&out));
}

/// A directory of this test's own, named after what it is for.
fn scratch(what: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pros-{}-{what}", std::process::id()));
    // Fresh every run, so no registry survives from an earlier run.
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path
}

/// Everything the program said, wherever it said it.
fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
