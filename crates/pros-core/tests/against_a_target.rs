//! What a real target says, as opposed to what a stand-in says.
//!
//! # Why these are ignored by default, and why that is not a skip
//!
//! Everything else in this project is tested against something written to behave like a
//! target. That proves the client is self-consistent. It cannot prove the target agrees,
//! and the difference between those two kinds of evidence is the thing this project grades
//! most carefully everywhere else - so it would be strange to blur it here.
//!
//! These run only when asked: `--ignored`, with `PROS_TARGET` naming the machine. The
//! default run reports them as **ignored**, which is visible in the output, rather than as
//! passing - a test that quietly passes without doing anything is the defect this whole
//! project is organised around.
//!
//! And **running them with no address set fails** rather than passing. Otherwise asking for
//! them explicitly and getting silence would look exactly like asking for them and having
//! them work.
//!
//! ```text
//! PROS_TARGET=192.168.1.211 cargo test -p pros-core --test against_a_target -- --ignored --nocapture
//! ```
//!
//! # Read-only, deliberately
//!
//! Nothing here writes to the target, sends a payload, or runs a command that changes
//! anything. A test suite that can alter the machine it is measuring is a test suite whose
//! failures are ambiguous, and this one runs against somebody's actual target.

use std::time::Duration;

use pros_core::chain::Chain;
use pros_core::target::Target;
use pros_link::files::{Kind, Session};

/// The target to talk to.
///
/// # Panics
///
/// When the variable is not set. **Failing rather than returning early**: these tests were
/// asked for by name, and a run that did nothing and said it passed is worse than one that
/// says what is missing.
fn target() -> Target {
    let address = std::env::var("PROS_TARGET").unwrap_or_else(|_| {
        panic!(
            "PROS_TARGET is not set. These tests were asked for explicitly, so doing \
             nothing and reporting success is not an option - set it to the target's \
             address and run again"
        )
    });
    Target {
        name: "target".to_owned(),
        address,
        ports: std::collections::BTreeMap::new(),
        chain: None,
    }
}

/// The target answers, and says what it can currently do.
///
/// The first thing worth knowing, and the thing every other test here depends on.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_target_says_what_it_can_do() {
    let target = target();
    let report = pros_core::check(&target);

    println!("{} ({})", report.name, report.address);
    for finding in &report.findings {
        println!(
            "  {:<4} {:<9} :{:<5} {}ms  {}",
            if finding.reachability.open {
                "up"
            } else {
                "--"
            },
            finding.service.name,
            finding.service.port,
            finding.reachability.took.as_millis(),
            finding.service.unlocks
        );
    }
    println!("verdict: {:?}", report.verdict());

    assert_eq!(
        report.findings.len(),
        pros_link::service::SERVICES.len(),
        "every service should be asked about, whatever the answer"
    );
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.reachability.open),
        "nothing answered at all - is the target on, and is the chain loaded?"
    );
}

/// A session opens, which means the login was accepted **and binary mode was agreed**.
///
/// The second half is the one worth having against real target: this project refuses to
/// continue in text mode, and until now nothing had confirmed a real server will agree.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_file_service_agrees_to_binary_mode() {
    let target = target();
    let session = Session::open(&target.link())
        .expect("the file service should accept an anonymous login and binary mode");
    session.close();
}

/// A real directory listing parses.
///
/// **This is the test that matters most here.** Against a stand-in, the passive-mode reply
/// and the listing format are whatever the stand-in was written to produce. Against the
/// target they are whatever the target produces, and every guess this client makes about
/// them is on the line.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn a_real_listing_parses() {
    let target = target();
    let mut session = Session::open(&target.link()).expect("a session");
    let entries = session.list("/").expect("the root should list");
    session.close();

    for entry in &entries {
        println!(
            "  {:<4} {:>12}  {}",
            if entry.is_usable() { "ok" } else { "??" },
            entry.size.map_or_else(String::new, |size| size.to_string()),
            entry.raw
        );
    }

    assert!(!entries.is_empty(), "the root listed nothing at all");
    // Not "every line parsed" - a target is allowed to have a listing format this client
    // has not seen. But if *nothing* parsed, the format is not the one this was written
    // against, and that is worth failing over rather than shrugging at.
    assert!(
        entries.iter().any(pros_link::files::Entry::is_usable),
        "not one line of a real listing could be read - the format is not what this client \
         expects, and every path it builds from a listing would be wrong"
    );
    for entry in entries.iter().filter(|entry| entry.is_usable()) {
        assert!(
            !entry.name.is_empty(),
            "a usable entry with no name: {entry:?}"
        );
    }
}

/// A directory that is not there is refused cleanly.
///
/// A stand-in can be written to answer this correctly by accident. What is being checked is
/// that a real refusal arrives as a **typed error naming the target's own words**, rather
/// than as a hang, an empty listing, or a success with nothing in it.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn a_directory_that_is_not_there_is_refused_rather_than_empty() {
    let target = target();
    let mut session = Session::open(&target.link()).expect("a session");
    let answer = session.list("/there-is-no-such-directory-on-this-target");
    session.close();

    match answer {
        Ok(entries) => {
            // Some servers list a missing directory as empty. That is a real answer and not
            // this client's fault - but it must be *empty*, not full of something else.
            println!("listed as empty rather than refused, which some servers do");
            assert!(
                entries.iter().all(|entry| !entry.is_usable()),
                "a directory that does not exist listed actual entries: {entries:?}"
            );
        }
        Err(why) => {
            println!("refused: {why}");
            assert!(
                !why.to_string().is_empty(),
                "a refusal with nothing said in it"
            );
        }
    }
}

/// The shell answers something.
///
/// `ls /` and nothing else: this suite does not change the machine it is measuring.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_shell_answers() {
    let target = target();
    let said = pros_link::shell::run(&target.link(), "ls /", Duration::from_millis(1500))
        .expect("the shell should answer");
    println!("{said}");

    assert!(
        !said.trim().is_empty(),
        "the shell accepted a command and said nothing - the banner drain may have taken \
         the answer with it"
    );
}

/// The manager's web service answers, and answers oddly.
///
/// # The first time this client has met a real web server
///
/// `pros-link`'s web client was written against a stand-in and had never spoken to
/// anything else. Two things came back that a stand-in would not have produced:
///
/// - the dashboard is a single page of about 700 kB, so the framing is real rather than
///   the tidy `Content-Length` a fake sends;
/// - **an unknown path answers `200 OK` with `404 Not Found` as the body.**
///
/// That second one is worth a test of its own. A caller that treats a status of 200 as
/// *this path existed* would hand its user the words "404 Not Found" as data - success
/// reported for something that did not happen, which is the defect this project keeps
/// meeting, arriving this time from the other end of the wire.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_managers_web_service_answers_and_a_status_is_not_a_promise() {
    let target = target();
    let report = pros_core::check(&target);
    let up = report
        .findings
        .iter()
        .any(|finding| finding.service.name == "pldmgr" && finding.reachability.open);
    assert!(
        up,
        "the payload manager is not answering on this target, so this could not be          checked - that is this target's state, not a fault in the tool"
    );

    let page = pros_link::manager::get(&target.address, "/").expect("the dashboard should serve");
    println!("the dashboard is {} bytes", page.len());
    assert!(!page.is_empty(), "it answered with nothing at all");

    // The finding: a status of 200 does not mean the path was there.
    let invented = pros_link::manager::get(&target.address, "/there-is-no-such-endpoint")
        .expect("this server answers 200 even for paths it does not have");
    println!("an unknown path answers: {}", invented.trim());
    assert!(
        invented.contains("404"),
        "an unknown path answered something other than a refusal in its body: {invented}"
    );
}

/// The target's own repository agrees with this project's service table about ports.
///
/// # Why this is worth a test
///
/// The five ports in `SERVICES` were written down from a measurement made once. The
/// repository describes several of the same payloads in its own words - *accepts connections
/// on port 2121* - which makes it an **oracle this project did not write**.
///
/// So a typo in the table, or a payload that changed its port between versions, is caught by
/// the target rather than by somebody noticing that a check has been reporting a service as
/// absent for a month.
///
/// Only entries that name a port are compared, and only against services this project knows.
/// A description that says nothing about ports says nothing about ports.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_service_table_agrees_with_the_targets_own_repository() {
    let target = target();
    let bytes = pros_link::files::retrieve(&target.link(), pros_core::manifest::TARGET_REPOSITORY)
        .expect("the repository should be where it was measured");
    let described = pros_core::manifest::Manifest::from_json(&String::from_utf8_lossy(&bytes))
        .expect("the repository should read");

    let mut compared = 0_usize;
    for service in pros_link::service::SERVICES {
        let Some(entry) = described.find(service.name.as_ref()) else {
            continue;
        };
        let Some(said) = entry.description.as_deref().and_then(port_in) else {
            continue;
        };
        compared += 1;
        assert_eq!(
            said, service.port,
            "{} is described as using port {said} and this project probes {}",
            service.name, service.port
        );
        println!("  {:<9} :{said}  agrees", service.name);
    }

    // A comparison that compared nothing is not a passing test, it is a test that did not
    // run. Either the repository stopped describing ports or this stopped finding them, and
    // both are worth knowing.
    assert!(
        compared >= 2,
        "no repository entry named a port for any known service, so nothing was checked"
    );
    println!("{compared} services cross-checked against the target's own description");
}

/// The port a description names, if it names one.
///
/// Deliberately narrow: the exact words *port* and a number. A description is prose, and
/// anything cleverer than this would be reading meaning into somebody's sentence.
fn port_in(description: &str) -> Option<u16> {
    let after = description.split("port ").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The boot list, if this target keeps one where it was measured keeping one.
///
/// **Either answer is a finding**, and both are printed. What is being checked is that a
/// missing file arrives as a refusal rather than as an empty chain - because an empty chain
/// would say *this target boots nothing*, which is a claim about the target rather than
/// about the path this client guessed.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_boot_list_either_reads_or_refuses_by_name() {
    let target = target();
    match Chain::read(&target.link()) {
        Ok(chain) => {
            println!("boot list at {}:", pros_core::chain::PATH);
            for (position, name) in chain.order().iter().enumerate() {
                println!("  {position}. {name}");
            }
            assert!(
                !chain.is_empty(),
                "the boot list read and named nothing, which would mean this target loads \
                 no payloads at all"
            );
        }
        Err(why) => {
            println!("no boot list at {}: {why}", pros_core::chain::PATH);
            println!("that is a fact about the path this project guessed, not about the target");
        }
    }
}

/// **A port a list declares is really probed, and a shut one is really reported shut.**
///
/// The unit tests prove the wiring with a report built by hand. This proves it against the
/// thing itself: a name this project has never heard of, given a port, becomes as measurable
/// as the five that are compiled in.
///
/// Two entries, because one of them has to fail. A test that only ever saw an open port would
/// pass just as happily if the probe were hard-wired to say yes.
#[test]
#[ignore = "needs a target; set PROS_TARGET"]
fn a_port_a_list_declares_is_probed_against_the_target() {
    let target = target();

    // 2121 is the file service, which the other tests here have already used. The name is
    // deliberately not one of the five, so it can only be found through the declared port.
    let manifest = pros_core::manifest::Manifest::from_json(
        r#"[
            { "name": "something-this-project-has-never-heard-of", "port": 2121 },
            { "name": "something-that-is-not-running-at-all", "port": 49732 }
        ]"#,
    )
    .expect("reads");

    let report = pros_core::check::check_declaring(&target, &manifest, Duration::from_millis(1500));
    assert_eq!(
        report.declared.len(),
        2,
        "both declared ports should be probed"
    );

    let rows = pros_core::payloads::survey(&manifest, Some(&report), None);
    assert_eq!(
        rows[0].presence,
        pros_core::payloads::Presence::Loaded,
        "a declared port that is open read as {:?}",
        rows[0].presence
    );
    assert_eq!(
        rows[1].presence,
        pros_core::payloads::Presence::NotLoaded,
        "a declared port that is shut must be absent, not unknown - being measurable is the          entire point of declaring one"
    );
    println!("declared ports: open -> Loaded, shut -> NotLoaded");
}

/// **Which directories a target actually has, and which are a tool's own invention.**
///
/// Paths were taken from another working tool and checked here rather than copied. Five of
/// its constants turned out to be absent: they are made by the payloads that use them, so
/// they exist on a target running those and nowhere else.
///
/// This prints the survey and asserts only the system directories, because those are the ones
/// that are a property of the machine. Asserting the conditional ones would encode one
/// target's setup as a fact about the platform - the exact mistake this test exists to
/// document.
#[test]
#[ignore = "needs a target; set PROS_TARGET"]
fn the_directories_a_target_has_are_measured_rather_than_assumed() {
    let target = target();

    // A property of the machine: confirmed present, and asserted.
    let system = [
        "/user/app",
        "/user/appmeta",
        "/user/home",
        "/data/pkg",
        "/data/homebrew",
    ];
    // Made by whatever payload is installed: surveyed and reported, never asserted.
    let conditional = [
        "/data/cheatrunner/cheats",
        "/data/etaHEN/cheats",
        "/data/elf-arsenal/cheats",
        "/data/garlic",
        "/data/payloads",
        "/data/AVATARS",
        "/data/ps5_autoloader",
        "/mnt/sandbox/pfsmnt",
    ];

    let mut session = Session::open(&target.link()).expect("it connects");
    for path in system {
        assert!(
            session.list(path).is_ok(),
            "{path} is meant to be a property of the target and was not there"
        );
    }
    println!("system directories: all {} present", system.len());
    for path in conditional {
        let there = session.list(path).is_ok();
        println!("  {:5}  {path}", if there { "here" } else { "-" });
    }
    session.close();

    // Saves, whose layout the tool depends on: user, then the prospero folder, then titles.
    let mut session = Session::open(&target.link()).expect("it connects");
    let users = session
        .list("/user/home")
        .expect("the home directory lists");
    let user = users
        .iter()
        .find(|entry| entry.kind == Kind::Directory)
        .expect("at least one user");
    let saves = format!("/user/home/{}/savedata_prospero", user.name);
    let titles = session.list(&saves).expect("the save folder lists");
    println!("{} holds saves for {} titles", saves, titles.len());
    session.close();
}

/// **A save says which account it belongs to, and this reads it off a real one.**
///
/// The whole save-transfer gate rests on this: a copy going back to the account that wrote it
/// is a plain send, and one going anywhere else needs decrypting and re-signing. The two are
/// indistinguishable while they happen and differ only later, when a target refuses a save.
///
/// Also measures the thing that stops this being the only source: **not every save has a
/// parameter file.** Of three on the target this was written against, one did and two did
/// not. The count is printed rather than asserted, because how many is a fact about somebody's
/// target rather than about the platform.
#[test]
#[ignore = "needs a target; set PROS_TARGET"]
fn a_save_carries_the_account_that_wrote_it() {
    let target = target();
    let mut session = Session::open(&target.link()).expect("it connects");

    let users = session.list(pros_core::saves::HOME).expect("home lists");
    let user = users
        .iter()
        .find(|entry| entry.kind == Kind::Directory)
        .expect("at least one user");
    let meta = format!(
        "{}/{}/savedata_prospero_meta/user",
        pros_core::saves::HOME,
        user.name
    );

    let titles = session.list(&meta).expect("the metadata folder lists");
    let mut accounts = Vec::new();
    let mut without = 0;
    for title in &titles {
        let files = session
            .list(&format!("{meta}/{}", title.name))
            .unwrap_or_default();
        let Some(parameters) = files
            .iter()
            .find(|file| file.name.to_ascii_lowercase().ends_with(".sfo"))
        else {
            without += 1;
            continue;
        };
        let bytes = session
            .retrieve(&format!("{meta}/{}/{}", title.name, parameters.name))
            .expect("the parameter file comes across");
        let read = pros_core::sfo::read(&bytes).expect("it parses as parameters");
        let account = pros_core::sfo::account_id(&read).expect("it names an account");
        // Not printed: it identifies somebody. Its length and consistency are the findings.
        assert_eq!(account.len(), 16, "an account identifier is eight bytes");
        accounts.push(account);
    }
    session.close();

    println!(
        "{} saves: {} carry parameters, {} carry none",
        titles.len(),
        accounts.len(),
        without
    );
    assert!(
        !accounts.is_empty(),
        "no save on this target carried a parameter file, so the account could not be read \
         from any of them - that is this target's state, not a fault in the parser"
    );
    // **Every save on one target belongs to one account.** If this ever failed, comparing a
    // copy's account against "the target's account" would be comparing against one of
    // several, and the gate would pass or fail depending on which save was read first.
    assert!(
        accounts.windows(2).all(|pair| pair[0] == pair[1]),
        "saves on one target named different accounts, so there is no single account to \
         compare an incoming save against"
    );
}

/// **The manager's settings read, and an edit produces a diff without writing anything.**
///
/// Read-only on purpose. This is the one thing in the project that *could* write to a target,
/// and the test that proves the edit works must not be the thing that performs it - a test
/// that reorders somebody's boot list to check it can is a test that breaks their target to
/// prove it works.
///
/// So: fetch the real file, make a change in memory, and check the diff touches exactly the
/// line it should. Nothing goes back.
#[test]
#[ignore = "needs a target; set PROS_TARGET"]
fn the_managers_settings_read_and_an_edit_stays_in_memory() {
    let target = target();
    let bytes = pros_link::files::retrieve(&target.link(), pros_core::autoload::CONFIG)
        .expect("the settings file comes across");
    let text = String::from_utf8_lossy(&bytes);
    let settings = pros_core::autoload::Settings::parse(&text);

    println!("{} settings:", settings.all().len());
    for (key, value) in settings.all() {
        println!("  {key} = {value}");
    }
    assert!(
        !settings.all().is_empty(),
        "the settings file was readable and held nothing this recognises"
    );

    // The delay is a number, so changing it is the safest possible demonstration - and it is
    // never sent.
    let Some(delay) = settings.get("AUTOLOAD_DELAY") else {
        println!("no AUTOLOAD_DELAY on this target - nothing further to check");
        return;
    };
    let different = if delay == "9" { "8" } else { "9" };
    let change = settings
        .set("AUTOLOAD_DELAY", different)
        .expect("a different value is a change");

    let gone = change
        .diff()
        .into_iter()
        .filter(|line| matches!(line, pros_core::autoload::Line::Gone(_)))
        .count();
    let added = change
        .diff()
        .into_iter()
        .filter(|line| matches!(line, pros_core::autoload::Line::Added(_)))
        .count();
    assert_eq!(gone, 1, "one line should go, not {gone}");
    assert_eq!(added, 1, "and one arrive, not {added}");
    // Every other setting survives, which is the whole promise of editing the text rather
    // than regenerating it.
    for (key, value) in settings.all() {
        if key == "AUTOLOAD_DELAY" {
            continue;
        }
        assert!(
            change.now.contains(&format!("{key}={value}")),
            "{key} was lost by an edit to a different setting"
        );
    }
    println!("edit to AUTOLOAD_DELAY touches 1 line, leaves the rest - nothing written");
}

/// **What the target says it is, read through the shell.**
///
/// Firmware is the fact everything else on this platform depends on, and this is where the
/// parsers meet the real output rather than a fixture written from it.
///
/// Asserts the shape and not the values: firmware strings, model numbers and core counts are
/// facts about somebody's target, and pinning them here would make the test fail on anybody
/// else's - which is the opposite of what it is for.
#[test]
#[ignore = "needs a target; set PROS_TARGET"]
fn the_target_says_what_it_is() {
    let target = target();
    let settle = Duration::from_millis(1200);
    let ask =
        |command: &str| pros_link::shell::run(&target.link(), command, settle).unwrap_or_default();

    let mut answers = std::collections::BTreeMap::new();
    for (key, _) in pros_core::system::FACTS {
        let said = ask(&format!("sysctl {key}"));
        if !said.contains("No such file") {
            answers.insert((*key).to_owned(), said);
        }
    }
    let report = pros_core::system::Report::from(&answers, &ask("df"), &ask("ps"));

    assert!(
        !report.facts.is_empty(),
        "the target answered no sysctl this knows to ask - the keys in FACTS were each \
         measured, so this means the shell stopped answering rather than that they are wrong"
    );
    for fact in &report.facts {
        // Printed by name only. The values identify a specific target.
        println!("  {}: {} characters", fact.name, fact.value.len());
        assert!(
            !fact.value.trim().is_empty(),
            "{} came back blank",
            fact.name
        );
    }

    // The firmware is the one worth insisting on, because it is the reason for the view.
    let firmware = report
        .facts
        .iter()
        .find(|fact| fact.name == "firmware")
        .expect("the target names its firmware");
    assert!(
        firmware.value.contains("releases/"),
        "the firmware line did not look like one: {}",
        firmware.value
    );

    assert!(
        report.storage.iter().any(|one| one.at == "/user"),
        "no /user filesystem in the storage listing, which every target has"
    );
    // **Most of what `df` lists is not the machine.** Every running application brings dozens
    // of bind mounts under /mnt/sandbox: a target measured here listed 1183 filesystems,
    // of which 22 were the machine. Worth asserting, because a change that stopped telling
    // them apart would bury the figures somebody came to read.
    let machine = report
        .storage
        .iter()
        .filter(|one| !one.is_a_sandbox_mount())
        .count();
    assert!(
        machine < report.storage.len(),
        "no sandbox mounts at all - either nothing is running, or they stopped being          recognised as such"
    );
    println!(
        "{} filesystems: {machine} are the machine, {} are sandbox mounts",
        report.storage.len(),
        report.storage.len() - machine
    );
    assert!(
        report.processes.len() > 3,
        "only {} processes - the shell itself accounts for two",
        report.processes.len()
    );
    println!(
        "{} filesystems, {} processes, {} of them titles",
        report.storage.len(),
        report.processes.len(),
        report
            .processes
            .iter()
            .filter(|one| one.is_a_title())
            .count()
    );
}

/// **A graft against the samples on this machine, if there are any.**
///
/// Not a target test - it needs no target at all - but it lives here because it needs files
/// somebody has put on this machine, and the rest of the suite must not depend on them.
///
/// Grafts each save in `saves/` into each other, and asserts the rule the whole design rests
/// on: **the container's keystone is the one that survives.**
#[test]
#[ignore = "needs saves in the collection\'s saves/; run with --ignored"]
fn saves_on_this_machine_graft_without_losing_the_container() {
    let Some(root) = pros_core::target::directory().map(|dir| dir.join("saves")) else {
        return;
    };
    let opened: Vec<pros_core::graft::Open> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("sce_sys").is_dir())
        .filter_map(|path| pros_core::graft::Open::read(&path).ok())
        .collect();

    if opened.len() < 2 {
        println!(
            "{} unpacked saves in {} - unzip two to exercise this",
            opened.len(),
            root.display()
        );
        return;
    }

    for one in &opened {
        println!(
            "  {:<28} title={:?} account={:?} keystone={} contents={}",
            one.root
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            one.title(),
            one.account(),
            one.has_keystone,
            one.contents.len()
        );
    }

    let container = &opened[0];
    let donor = &opened[1];
    let into = std::env::temp_dir().join("prosperous-graft-samples");
    let _ = std::fs::remove_dir_all(&into);

    let done = pros_core::graft::graft(container, donor, &into).expect("it grafts");
    for note in &done.notes {
        println!("  note: {note}");
    }

    // The rule the method rests on: a donor keystone would not mount.
    let kept = std::fs::read(into.join(pros_core::graft::KEYSTONE)).expect("a keystone survived");
    let theirs = std::fs::read(donor.root.join(pros_core::graft::KEYSTONE)).expect("donor has one");
    let ours = std::fs::read(container.root.join(pros_core::graft::KEYSTONE)).expect("we have one");
    assert_eq!(kept, ours, "the container's keystone must survive");
    if ours != theirs {
        assert_ne!(kept, theirs, "the donor's keystone must not have won");
    }
    assert!(!done.taken.is_empty(), "nothing was taken from the donor");

    let _ = std::fs::remove_dir_all(&into);
}

/// **A package is handed to the target over HTTP, and the target takes it.**
///
/// The whole install path in one test: hold the file out on the interface that faces the
/// target, tell the shell to fetch it, and check both ends agree - the target reports a
/// content identifier, and the handover counted a fetch.
///
/// The second half is what makes this worth writing. A target that never came for the file and
/// a target that fetched it and disliked it are **indistinguishable from its reply alone**, and
/// only this side knows which happened.
#[test]
#[ignore = "installs a package on the target; needs PROS_TARGET and a .pkg in packages/"]
fn a_package_served_from_here_is_fetched_and_accepted() {
    let target = target();
    let Some(packages) = pros_core::handover::staging() else {
        return;
    };
    let Some(package) = std::fs::read_dir(&packages)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|end| end.eq_ignore_ascii_case("pkg"))
        })
    else {
        println!("no .pkg in {} - nothing to hand over", packages.display());
        return;
    };

    let offered = pros_core::handover::offer_to(&package, &target.address).expect("it offers");
    println!("holding {} out at {}", package.display(), offered.url);

    let said = pros_link::shell::run(
        &target.link(),
        &pros_core::install::command(&offered.url),
        Duration::from_mins(2),
    )
    .expect("the shell answers");
    let read = pros_core::install::read(&said);
    println!("  target said: {}", read.describe());
    println!("  fetched {} time(s)", offered.taken());
    for (at, asked) in offered.asked().iter().enumerate() {
        println!("    {at}: {asked}");
    }

    assert!(
        offered.taken() > 0,
        "the target never came for the file, so whatever it said was not about this package"
    );
    assert!(
        read.was_accepted(),
        "the target fetched it and did not accept it: {}",
        read.describe()
    );
}

/// **What the payload scan actually returns from a real target.**
///
/// Added because the autoload screen was reported as listing the wrong things, and the code
/// path reads plainly - so the question is what the scan answers, not what it looks like it
/// answers.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_payload_scan_finds_what_the_manager_holds() {
    let target = target();
    let found = pros_core::payloads::on_target_everywhere(&target.link()).expect("the scan runs");
    for one in &found {
        println!("{}\t{}", one.name, one.path);
    }
    assert!(!found.is_empty(), "the manager holds payloads");
}
