//! What a real target says, as opposed to what a stand-in says.
//!
//! The rest of the suite proves the client against stand-ins; these check the target agrees.
//! They are `#[ignore]`d, so a default run reports them as ignored rather than passed, and
//! run only when asked, with `PROS_TARGET` naming the machine; asking without it set fails.
//!
//! ```text
//! PROS_TARGET=192.168.1.211 cargo test -p pros-core --test against_a_target -- --ignored --nocapture
//! ```
//!
//! They are read-only, except the package test, which says so in its ignore reason.

use std::time::Duration;

use pros_core::chain::Chain;
use pros_core::target::Target;
use pros_link::files::{Kind, Session};

/// The target to talk to.
///
/// # Panics
///
/// When the variable is not set: these tests were asked for by name, and doing nothing must
/// not report success.
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

/// The target answers, and every known service is asked about.
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

/// A real file service accepts the login and agrees to binary mode.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn the_file_service_agrees_to_binary_mode() {
    let target = target();
    let session = Session::open(&target.link())
        .expect("the file service should accept an anonymous login and binary mode");
    session.close();
}

/// A real directory listing, passive-mode reply and all, parses.
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
    // Unknown lines are allowed, but if none parse the format is not the one this reads.
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

/// A missing directory is refused with the target's words, or listed as empty.
#[test]
#[ignore = "needs a target: set PROS_TARGET and run with --ignored"]
fn a_directory_that_is_not_there_is_refused_rather_than_empty() {
    let target = target();
    let mut session = Session::open(&target.link()).expect("a session");
    let answer = session.list("/there-is-no-such-directory-on-this-target");
    session.close();

    match answer {
        Ok(entries) => {
            // Some servers list a missing directory as empty; it must then hold no entries.
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

/// The shell answers a read-only command.
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

/// The manager's web service answers, and an unknown path answers `200 OK` with a
/// `404 Not Found` body.
///
/// So a status of 200 does not mean the path exists. The dashboard is a single page of about
/// 700 kB, which exercises real framing.
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

    let invented = pros_link::manager::get(&target.address, "/there-is-no-such-endpoint")
        .expect("this server answers 200 even for paths it does not have");
    println!("an unknown path answers: {}", invented.trim());
    assert!(
        invented.contains("404"),
        "an unknown path answered something other than a refusal in its body: {invented}"
    );
}

/// The service table's ports agree with the target's own repository descriptions.
///
/// The repository describes several of the same payloads ("accepts connections on port
/// 2121"), an independent source for the ports in `SERVICES`. Only entries that name a port
/// are compared.
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

    // A comparison that compared nothing did not run.
    assert!(
        compared >= 2,
        "no repository entry named a port for any known service, so nothing was checked"
    );
    println!("{compared} services cross-checked against the target's own description");
}

/// The port a description names, if it names one.
///
/// Narrow on purpose: the word `port ` followed by digits, and nothing read into the prose.
fn port_in(description: &str) -> Option<u16> {
    let after = description.split("port ").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The boot list either reads with entries, or its absence is a refusal, not an empty chain.
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

/// A port a manifest declares is probed on the target: open reads loaded, shut not loaded.
///
/// Two entries, so a probe hard-wired to say yes would fail.
#[test]
#[ignore = "needs a target; set PROS_TARGET"]
fn a_port_a_list_declares_is_probed_against_the_target() {
    let target = target();

    // 2121 is the file service; the name is not a known service, so only the declared port
    // can find it.
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

/// System directories exist on the target; payload-made directories are surveyed only.
///
/// The conditional directories exist only where the payload that makes them is installed,
/// so they are printed, never asserted.
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

    // The save layout: user, then `savedata_prospero`, then titles.
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

/// Saves' parameter files name one account, eight bytes long, across the whole target.
///
/// Not every save has a parameter file; how many do is printed, not asserted.
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
        let account = pros_core::sfo::account_in(&bytes).expect("it names an account");
        // Not printed, since it identifies somebody.
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
    // `saves::account_on` relies on every save on one target naming one account.
    assert!(
        accounts.windows(2).all(|pair| pair[0] == pair[1]),
        "saves on one target named different accounts, so there is no single account to \
         compare an incoming save against"
    );
}

/// The manager's settings read, and an edit changes exactly one line, in memory only.
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

    // The delay is a number, so it is the safest value to change; it is never sent.
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
    // Every other setting survives, since the text is edited rather than regenerated.
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

/// The system report parses the target's real sysctl, `df` and `ps` output.
///
/// Asserts shape, not values: firmware, model and core counts differ between targets.
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
        // Printed by name only; the values identify a specific target.
        println!("  {}: {} characters", fact.name, fact.value.len());
        assert!(
            !fact.value.trim().is_empty(),
            "{} came back blank",
            fact.name
        );
    }

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
    // Most of what `df` lists is per-application bind mounts under /mnt/sandbox (measured:
    // 1183 filesystems, 22 of them the machine), so they must be told apart.
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

/// Grafting local sample saves keeps the container's keystone.
///
/// Needs no target, but needs unpacked saves in the collection's `saves/`, so it is ignored
/// with the target tests.
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

    // A donor keystone would not mount.
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

/// A package served from here is fetched by the target and accepted.
///
/// Both ends are checked: the handover's fetch count tells a target that never came for the
/// file from one that fetched it and refused it, which its reply alone cannot.
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

/// The payload scan finds the payloads the manager holds on a real target.
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
