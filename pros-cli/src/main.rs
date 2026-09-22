//! `pros` - one instrument for talking to a prepared target.
//!
//! Throughout `pros`, *target* means a **registered target** - a machine registered by name and
//! address (`pros register`) - never a *build target* (the machine an artifact is built for,
//! `selfish --target`) or an *install target* (a download manifest's destination). Prosperous
//! owns this sense and keeps the bare word; the qualifier is stated once, here, so a reader
//! crossing from the other repositories does not import the collision. (OOPS conventions section
//! 2, "The four axes of a build and a run".)
//!
//! # This program holds no logic, on purpose
//!
//! Registering, probing, reading a manifest, verifying a digest, refusing the wrong kind of
//! file: all of it is in `pros-core` and `pros-link`, because a graphical version of this
//! has to do exactly the same things and a second implementation of them would drift within
//! a month. What is here is argument parsing and wording.
//!
//! That is the sibling projects' principle 13 taken as a starting condition rather than
//! arrived at after the first drift.
//!
//! # Exit codes are part of the interface
//!
//! - **0** - it worked, or the target answered and the answer was *not ready*.
//! - **1** - this program could not do what it was asked: no such target, a file that is
//!   not there, a transfer that failed, a title the target refused to start.
//! - **2** - a check found the target blocked.
//!
//! Two and one are separated deliberately. **A target that is switched off is an answer,
//! not a malfunction**, and a script that branches on it should not have to tell that apart
//! from the tool falling over by reading the message.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use pros_core::check::Verdict;
use pros_core::manifest::Manifest;
use pros_core::target::{self, Target};

mod say;

/// What a blocked check exits with. See the module note.
const BLOCKED: u8 = 2;

#[derive(Parser)]
#[command(
    name = "pros",
    about = "Talk to a prepared target: register it, ask what it can do, move files, run things",
    // The same line the window's footer shows, from the same place. Two front ends over one
    // library that disagree about which build they are is a bug report nobody can act on.
    version = pros_core::build::line_static()
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// The same global flag every subcommand carries.
    ///
    /// **Captured here as well as there** so that anything running before the subcommand -
    /// the warning below - knows which target was meant. A global argument is populated at
    /// both levels, so this is one flag read twice rather than two flags.
    #[command(flatten)]
    which: Which,
}

/// Which target, when more than one is registered.
#[derive(Args)]
struct Which {
    /// Which target, by its registered name
    #[arg(long, global = true)]
    name: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Remember a target's address under a name
    Register {
        /// Host or address
        address: String,
        /// What to call it
        #[arg(long, default_value = "prospero")]
        name: String,
    },
    /// Show what is registered
    List,
    /// Forget a registration
    Forget {
        /// Which one
        name: String,
    },
    /// Ask a target what it can currently do
    Check {
        /// Send anything that is missing and is staged here, then ask again
        #[arg(long)]
        fix: bool,
        #[command(flatten)]
        which: Which,
    },
    /// Listen to the system log for a while
    Logs {
        /// How long to listen
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        #[command(flatten)]
        which: Which,
    },
    /// Run one command on the target
    Sh {
        /// The command
        command: String,
        #[command(flatten)]
        which: Which,
    },
    /// Send a payload and run it
    Send {
        /// The payload
        path: PathBuf,
        /// How long to listen for anything it prints
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        #[command(flatten)]
        which: Which,
    },
    /// Keep a probe alive on the target while something else drives it
    ///
    /// A conformance probe answers questions by calling functions whose arity is not known
    /// yet, so faulting is the normal case. Its own protocol says restarting afterwards is
    /// somebody else's job. This is that job: watch the port, and when it stops answering,
    /// send the same bytes again.
    Supervise {
        /// The probe, as a file here
        path: PathBuf,
        /// The port it listens on once it is up
        #[arg(long, default_value_t = pros_core::supervise::PORT)]
        port: u16,
        /// How many dead starts in a row before giving up
        #[arg(long, default_value_t = pros_core::supervise::Supervisor::PATIENCE)]
        patience: usize,
        /// Stop after this many restarts. Zero means keep going.
        #[arg(long, default_value_t = 0)]
        restarts: usize,
        #[command(flatten)]
        which: Which,
    },
    /// List a directory on the target
    Ls {
        /// Which directory
        #[arg(default_value = "/data")]
        path: String,
        #[command(flatten)]
        which: Which,
    },
    /// Fetch a file off the target
    Pull {
        /// The file, as the target sees it
        path: String,
        /// Where to write it. Defaults to the file's own name
        #[arg(long)]
        into: Option<PathBuf>,
        #[command(flatten)]
        which: Which,
    },
    /// Put a file onto the target
    Push {
        /// The local file
        from: PathBuf,
        /// Where it goes, as the target sees it
        to: String,
        #[command(flatten)]
        which: Which,
    },
    /// Show what payloads are described, what can be trusted, and what is on the target
    Payloads {
        /// A manifest on this machine. Defaults to the one beside the registry
        file: Option<PathBuf>,
        /// Read the target's own repository instead. The path was measured on a target
        /// and is the default; give one to look elsewhere
        #[arg(
            long,
            conflicts_with = "file",
            num_args = 0..=1,
            default_missing_value = pros_core::manifest::TARGET_REPOSITORY
        )]
        from_target: Option<String>,
        /// Probe the target too, so the table says what is actually loaded
        #[arg(long)]
        check: bool,
        /// Write the built-in recommended list out, so it can be edited
        #[arg(long)]
        write: bool,
        /// Keep what was read: merge it into your own list and save it
        #[arg(long)]
        save: bool,
        #[command(flatten)]
        which: Which,
    },
    /// Browse the target's storage: titles, saves and packages
    Library {
        /// Which directory. Conventional, and not measured by this project
        #[arg(default_value = "/user/app")]
        path: String,
        /// Show only what looks like a title
        #[arg(long)]
        titles: bool,
        #[command(flatten)]
        which: Which,
    },
    /// Copy a folder off the target - a save, a title's data, anything
    Backup {
        /// The folder, as the target sees it
        from: String,
        /// Where to put it here. Defaults to the folder's own name
        #[arg(long)]
        into: Option<PathBuf>,
        #[command(flatten)]
        which: Which,
    },
    /// Put a folder back onto the target
    Restore {
        /// The folder on this machine
        from: PathBuf,
        /// Where it goes, as the target sees it
        to: String,
        /// Accept the suggested destination path without prompting
        #[arg(short, long)]
        yes: bool,
        /// Force transfer to requested destination without guard checks
        #[arg(long)]
        force: bool,
        /// Send every file, even ones already on the target and unchanged since last restore
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        which: Which,
    },
    /// What saves are on the target, by the name of the game they belong to
    Saves {
        #[command(flatten)]
        which: Which,
    },
    /// What titles are installed, by name rather than by identifier
    Titles {
        /// Where the target keeps title descriptions
        #[arg(long, default_value = pros_core::titles::APPMETA)]
        appmeta: String,
        #[command(flatten)]
        which: Which,
    },
    /// Start an installed title on the target, by its application identifier
    ///
    /// The identifier, not a path. This asks the target's own system service to start an
    /// application the way selecting it on the home screen does; it does not run an ELF.
    /// `pros send` is that door.
    Launch {
        /// Which title. Nine characters, four letters then five digits - `pros titles`
        /// lists them
        ///
        /// Called `id` rather than `title` because it is not a name: the builtin resolves
        /// an identifier and nothing else.
        id: String,
        #[command(flatten)]
        which: Which,
    },
    /// Restart the user interface to clear a softlock
    ///
    /// Kills `SceShellUI`; the system's own `SceSysCore` respawns it, so the screen comes
    /// back without a reboot. Nothing else is touched.
    RestartUi {
        #[command(flatten)]
        which: Which,
    },
    /// Close a title, freeing what it holds open
    ///
    /// Ends every process the title owns. A stopped process is woken first so its own exit
    /// teardown completes - killing it while stopped leaves locked files behind.
    Close {
        /// Which title, by identifier - `pros titles` lists them
        id: String,
        #[command(flatten)]
        which: Which,
    },
    /// End one process by its pid, freeing what it holds open
    ///
    /// The native way to do what a hand-typed `sh kill …` fumbles: it sends the signal the
    /// target's `kill` builtin actually takes (`-s <number>`, not the `-9` shorthand it rejects),
    /// and it wakes a stopped process first so its own teardown completes. `pros ps` lists pids.
    Kill {
        /// Which process, by pid - `pros ps` lists them
        pid: String,
        #[command(flatten)]
        which: Which,
    },
    /// List the processes running on the target
    ///
    /// The same `ps` the window's system panel reads, so a pid to `pros kill` comes from here
    /// rather than from a raw shell.
    Ps {
        #[command(flatten)]
        which: Which,
    },
    /// Watch the running processes, redrawn on an interval until stopped
    ///
    /// The live form of `ps`: the same table - pid, state, memory, title, command - re-read every
    /// few seconds. Ctrl-C stops it, or `--seconds` caps it. Read-only, like `ps`: to end
    /// something use `pros close` or `pros kill`.
    Top {
        /// Stop after this many seconds (default: keep going until Ctrl-C)
        #[arg(long)]
        seconds: Option<u64>,
        /// Seconds between refreshes
        #[arg(long, default_value_t = 2)]
        every: u64,
        #[command(flatten)]
        which: Which,
    },
    /// Deploy a homebrew title from a local build, launch it, and follow its log until it ends
    ///
    /// The probe loop in one command, replacing a hand-run `restore` then `launch` then `logs`:
    /// close the title if it is running, restore it from a local build into
    /// `/data/homebrew/<id>` (overwriting what is there), launch it, and stream its log until the
    /// title leaves the process list - it exited or crashed - or `--seconds` elapses.
    ///
    /// A probe that finishes by parking (idling rather than exiting, the conforming ending for a
    /// big-app) never leaves the process list, so the watch ends at the `--seconds` cap and says
    /// so. Closing a running title is best-effort: a parked big-app ignores signals, and if it is
    /// still holding the slot the launch below will say so.
    Probe {
        /// Which title. Nine characters, four letters then five digits - `pros titles` lists them
        id: String,
        /// The local build directory to restore from - the title tree (`eboot.bin`, `sce_sys`, ...)
        from: PathBuf,
        /// Seconds to follow the log before giving up on a title that parks rather than exits
        #[arg(long, default_value_t = 120)]
        seconds: u64,
        /// Redeploy every file, even ones unchanged since the last probe
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        which: Which,
    },
    /// Fetch a payload described by the manifest, and keep it if it is the right one
    Fetch {
        /// Which entry. Omit with --all to fetch everything that can be checked
        ///
        /// Called `payload` rather than `name` because `--name` already means *which
        /// target*, and two arguments called the same thing is a question every time.
        payload: Option<String>,
        /// Everything the manifest describes that is not here already
        #[arg(long)]
        all: bool,
        /// Read the target's own repository first, which carries urls and digests
        #[arg(long)]
        from_target: bool,
        #[command(flatten)]
        which: Which,
    },
    /// Keep a payload you already have, ready to send. It is checked on the way in
    Stage {
        /// The file you downloaded
        file: PathBuf,
        /// Which manifest entry it claims to be
        #[arg(long)]
        r#as: String,
        /// The manifest. Defaults to the one beside the registry
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Check a file against what a manifest says it should be
    Verify {
        /// The file to check
        file: PathBuf,
        /// Which entry it claims to be
        #[arg(long)]
        against: String,
        /// The manifest
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Stand in for a console's Porthole payload, so the Moonlight bridge can be tested with no
    /// hardware. Serves an Annex-B clip on 9805 and prints the controller records that arrive on
    /// 9806. Runs until stopped.
    FakeTarget {
        /// The Annex-B H.264 clip to loop on the video port
        #[arg(long)]
        clip: PathBuf,
        /// The video port a client reads from. Porthole's is 9805
        #[arg(long, default_value_t = pros_moonlight::fake::VIDEO_PORT)]
        video_port: u16,
        /// The input port a client writes records to. Porthole's is 9806
        #[arg(long, default_value_t = pros_link::feed::PORT)]
        input_port: u16,
    },
    /// Run the Moonlight host bridge in front of Porthole, so any Moonlight client can find, pair
    /// with and stream a registered target. Offers one app per target. Runs until stopped.
    Moonlight {
        /// The name shown in the client's host list
        #[arg(long, default_value = "prosperous")]
        hostname: String,
        /// The LAN address to advertise. Auto-detected if omitted
        #[arg(long)]
        ip: Option<std::net::Ipv4Addr>,
    },
}

fn main() -> ExitCode {
    // Held for the whole of `main`: the guard is what keeps the writers alive, and `let _`
    // would drop it here and lose everything after this line.
    // `build` and `root` are what `oops-log` prints on its own startup line, so no tool has to
    // remember to write one - or to write it after the subscriber exists, which is the part that
    // would be got wrong separately in each of them.
    let _logging = oops_log::Logging::new("pros")
        .build(pros_core::build::line_static())
        .init();
    let cli = Cli::parse();
    // Before the command, not after: a command that then hangs has its explanation already
    // above it, which is the whole value of saying anything at all.
    forewarn(&cli.command, cli.which.name.as_deref());
    match run(cli.command) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

impl Command {
    /// Which service on the target this cannot work without.
    ///
    /// **Declared, not discovered.** The window has the same table in
    /// `Section::requires`, and two places deciding what a command needs would disagree the
    /// first time one gained a feature.
    const fn requires(&self) -> Option<&'static str> {
        match self {
            // Local, or the asking itself.
            Self::Register { .. }
            | Self::List
            | Self::Forget { .. }
            | Self::Check { .. }
            | Self::Fetch { .. }
            | Self::Stage { .. }
            | Self::Verify { .. }
            // Local: stand-in and bridge are servers this machine runs, not calls to a target.
            | Self::FakeTarget { .. }
            | Self::Moonlight { .. } => None,
            Self::Logs { .. } => Some("klogsrv"),
            // A line typed at the shell, a title started, and process control - all shell
            // work, and none of it wants the loader.
            Self::Sh { .. }
            | Self::Launch { .. }
            | Self::RestartUi { .. }
            | Self::Close { .. }
            | Self::Kill { .. }
            | Self::Ps { .. }
            | Self::Top { .. } => Some("shsrv"),
            // Running a payload, and re-running one that died.
            Self::Send { .. } | Self::Supervise { .. } => Some("elfldr"),
            // Everything that reads or moves a file.
            Self::Ls { .. }
            | Self::Pull { .. }
            | Self::Push { .. }
            | Self::Payloads { .. }
            | Self::Library { .. }
            | Self::Backup { .. }
            | Self::Restore { .. }
            | Self::Saves { .. }
            // Probe needs shsrv (launch/close/ps) and klogsrv (its log) too, but the restore is
            // its first hard step and the one it cannot begin without - the launch and log
            // failures surface at their own steps, in their own words.
            | Self::Probe { .. }
            | Self::Titles { .. } => Some("ftpsrv"),
        }
    }
}

/// How long a service is given to answer before the command is warned about.
///
/// Short on purpose. A service that is up answers a connection on a local network in
/// microseconds, so this is not measuring the service - it is deciding whether to print a
/// paragraph. **A wrong answer here costs a warning, never a refusal**, which is why it can
/// afford to be brief.
const GLANCE: Duration = Duration::from_millis(600);

/// How long silence has to last before a shell command's answer is considered complete.
///
/// The shell sends no end marker, so quiet is the only signal there is. The window uses the
/// same number for the same commands, and two front ends that waited different lengths would
/// disagree about whether a target had answered.
const SETTLE: Duration = Duration::from_millis(1200);

/// Says so, loudly, when a command needs something the target is not offering.
///
/// # Why this warns and does not refuse
///
/// The check is one connection attempt with a short budget. It can be wrong - a firewall, a
/// service still starting, a network having a moment - and a tool that refused on a
/// possibly-wrong reading would be worse than one that tried and failed with the reason
/// already on screen.
///
/// # Why it prints before the command rather than after
///
/// So that a command which then hangs has its explanation above it. The sibling probe holds
/// the same rule for the same reason and states it more sharply: announce before attempting,
/// because a program cannot narrate its own failure to return.
fn forewarn(command: &Command, name: Option<&str>) {
    let Some(service) = command.requires() else {
        return;
    };
    // No target registered is a different complaint, and the command itself will make it.
    let Ok(target) = pick(name) else {
        return;
    };
    let Some(known) = pros_link::SERVICES.iter().find(|one| one.name == service) else {
        return;
    };
    let found = pros_link::probe(&target.address, known.port, GLANCE);
    if found.open {
        return;
    }

    let bar = "!".repeat(72);
    eprintln!("{bar}");
    eprintln!(
        "  {service} is not answering on {}:{}",
        target.address, known.port
    );
    eprintln!("  this command needs it to {}", known.unlocks);
    eprintln!();
    eprintln!("  what follows will probably fail, and this is why. Run `pros check` for the");
    eprintln!("  whole picture, or `pros send <payload>` if it simply is not loaded.");
    eprintln!("{bar}");
    eprintln!();
}

/// Everything the program does, so `main` can hold one error path.
fn run(command: Command) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match command {
        Command::Register { address, name } => registry(&Registry::Add(name, address)),
        Command::List => registry(&Registry::Show),
        Command::Forget { name } => registry(&Registry::Remove(name)),
        Command::Check { fix, which } => check(fix, which.name.as_deref()),
        Command::Logs { seconds, which } => logs(seconds, which.name.as_deref()),
        Command::Sh { command, which } => sh(&command, which.name.as_deref()),
        Command::Send {
            path,
            seconds,
            which,
        } => send(&path, seconds, which.name.as_deref()),
        Command::Ls { path, which } => {
            let target = pick(which.name.as_deref())?;
            say::listing(&pros_link::files::list(&target.link(), &path)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Pull { path, into, which } => pull(&path, into, which.name.as_deref()),
        Command::Push { from, to, which } => push(&from, &to, which.name.as_deref()),
        Command::Payloads {
            file,
            from_target,
            check,
            write,
            save,
            which,
        } => payloads(
            file.as_deref(),
            from_target.as_deref(),
            check,
            write,
            save,
            which.name.as_deref(),
        ),
        Command::Library {
            path,
            titles,
            which,
        } => library(&path, titles, which.name.as_deref()),
        Command::Backup { from, into, which } => backup(&from, into, which.name.as_deref()),
        Command::Restore {
            from,
            to,
            yes,
            force,
            all,
            which,
        } => restore(&from, &to, yes, force, all, which.name.as_deref()),
        Command::Saves { which } => saves(which.name.as_deref()),
        Command::Titles { appmeta, which } => titles(&appmeta, which.name.as_deref()),
        Command::Launch { id, which } => launch(&id, which.name.as_deref()),
        Command::RestartUi { which } => restart_ui(which.name.as_deref()),
        Command::Close { id, which } => close(&id, which.name.as_deref()),
        Command::Ps { which } => ps(which.name.as_deref()),
        Command::Top {
            seconds,
            every,
            which,
        } => top(seconds, every, which.name.as_deref()),
        Command::Probe {
            id,
            from,
            seconds,
            all,
            which,
        } => probe(&id, &from, seconds, all, which.name.as_deref()),
        Command::Kill { pid, which } => kill_pid(&pid, which.name.as_deref()),
        Command::Fetch {
            payload,
            all,
            from_target,
            which,
        } => fetch(payload.as_deref(), all, from_target, which.name.as_deref()),
        Command::Stage {
            file,
            r#as,
            manifest,
        } => stage(&file, &r#as, manifest.as_deref()),
        Command::Verify {
            file,
            against,
            manifest,
        } => verify(&file, &against, &manifest),
        Command::Supervise {
            path,
            port,
            patience,
            restarts,
            which,
        } => supervise(&path, port, patience, restarts, which.name.as_deref()),
        Command::FakeTarget {
            clip,
            video_port,
            input_port,
        } => fake_target(&clip, video_port, input_port),
        Command::Moonlight { hostname, ip } => moonlight(hostname, ip),
    }
}

/// Runs one shell command on the target and prints what it said.
///
/// An empty reply is said out loud rather than shown as nothing, because a shell that is not
/// loaded and a command that printed nothing look identical otherwise - the check knows which.
fn sh(command: &str, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let out = pros_link::shell::run(&target.link(), command, SETTLE)?;
    if out.trim().is_empty() {
        println!("no output - is the shell loaded? `pros check` will say");
    } else {
        print!("{out}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Listens to the target's system log for a while and prints it.
///
/// A quiet log is reported as a result rather than an error, the same distinction the exit codes
/// draw: a target that had nothing to say is not a program that failed.
fn logs(seconds: u64, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    use std::io::Write as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let target = pick(name)?;
    println!("listening to {} for {seconds}s", target.address);

    let (stopper, lines) = pros_link::log::follow(&target.link())?;
    let stopper = Arc::new(stopper);
    let stopper_clone = Arc::clone(&stopper);

    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    let done = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done);

    let watcher = std::thread::spawn(move || {
        while !done_clone.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
            if done_clone.load(Ordering::Relaxed) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                stopper_clone.stop();
                break;
            }
        }
    });

    let mut any = false;
    for line in lines {
        match line {
            Ok(l) => {
                println!("{l}");
                let _ = std::io::stdout().flush();
                any = true;
            }
            Err(_) => break,
        }
    }

    done.store(true, Ordering::Relaxed);
    stopper.stop();
    let _ = watcher.join();

    if !any {
        println!("the log was quiet - which is a result, not a failure");
    }
    Ok(ExitCode::SUCCESS)
}

/// Runs the Moonlight host bridge in front of Porthole.
///
/// The behaviour is `pros_moonlight`'s (principle 3); this finds the LAN address and the apps to
/// offer, starts a PIN prompt on standard input, and hands over. It blocks until stopped.
fn moonlight(
    hostname: String,
    ip: Option<std::net::Ipv4Addr>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let local_ip = ip
        .or_else(detect_lan_ip)
        .ok_or("could not work out this machine's LAN address; pass it with --ip")?;
    // One app per registered target; a default if none is registered yet.
    let targets = target::load().unwrap_or_default();
    let apps = if targets.is_empty() {
        println!(
            "no targets registered - offering one placeholder app. Register with `pros register`."
        );
        pros_moonlight::Apps::from_titles(["Prosperous target"])
    } else {
        pros_moonlight::Apps::from_titles(targets.iter().map(|one| one.name.clone()))
    };
    let data_dir = target::directory()
        .ok_or("no data directory for the certificate")?
        .join("moonlight");
    // Where Porthole's 9805/9806 are served: the first registered target, or the local fake target.
    let porthole = targets
        .first()
        .map_or_else(|| "127.0.0.1".to_owned(), |one| one.address.clone());

    println!(
        "Moonlight bridge: host '{hostname}' on {local_ip}, {} target(s) offered.",
        targets.len().max(1)
    );
    println!(
        "Porthole video/input expected at {porthole} (9805/9806) - run `pros fake-target` there to test."
    );
    println!("On your Moonlight client, add {local_ip} (or find '{hostname}'), then pair.");
    println!("When it shows a PIN, type it here and press enter.");
    spawn_pin_prompt();
    pros_moonlight::run(hostname, local_ip, apps, porthole, &data_dir)?;
    Ok(ExitCode::SUCCESS)
}

/// Read PINs from standard input and hand each to the bridge over its local control path, so a
/// person just types the number the client shows.
fn spawn_pin_prompt() {
    std::thread::spawn(|| {
        use std::io::BufRead as _;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            let pin = line.trim();
            if pin.is_empty() {
                continue;
            }
            match submit_pin(pin) {
                Ok(()) => println!("PIN {pin} submitted; finishing pairing."),
                Err(error) => eprintln!("could not submit PIN: {error}"),
            }
        }
    });
}

/// Hand a PIN to the running bridge by calling its local `/pin` control path.
fn submit_pin(pin: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", pros_moonlight::host::HTTP_PORT))?;
    write!(
        stream,
        "GET /pin?pin={pin} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()
}

/// Work out the LAN address the target and clients would reach this machine on, by asking the OS
/// which local address it would use to reach the network - no packet is sent.
fn detect_lan_ip() -> Option<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) => Some(v4),
        std::net::IpAddr::V6(_) => None,
    }
}

/// Runs a stand-in target for testing the Moonlight bridge without a console.
///
/// The behaviour is entirely `pros_moonlight::fake`'s (principle 3); this reads the clip, says
/// what it is doing, and hands over. It blocks until the process is stopped.
fn fake_target(
    clip: &Path,
    video_port: u16,
    input_port: u16,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(clip)?;
    let ports = pros_moonlight::fake::Ports {
        video: video_port,
        input: input_port,
    };
    let (video, input) = pros_moonlight::fake::bind(&ports)?;
    println!(
        "fake target: serving {} ({} bytes) on :{video_port}, sinking input on :{input_port}",
        clip.display(),
        bytes.len(),
    );
    println!(
        "connect the bridge to these, or point Porthole's own watch/feed at them. Ctrl-C to stop."
    );
    pros_moonlight::fake::run(&video, &input, &bytes)?;
    Ok(ExitCode::SUCCESS)
}

/// Sends a payload.
///
/// The shape guard is the library's, not this program's: a vendor module and a payload
/// share their first four bytes, and the loader accepts either and then dies silently on
/// the one it cannot run.
fn send(
    path: &Path,
    seconds: u64,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let payload = std::fs::read(path)?;

    // **Asked here as well as in the library, so nothing is announced that is not happening.**
    //
    // The send guards on this too, and that is the real check. But printing "sending 64
    // bytes" and then refusing describes an action that never took place - which is the
    // same defect as a check that cannot fail, wearing different clothes. The announcement
    // comes after the refusal, so it is only ever made about a send.
    let found = pros_link::identify(&payload);
    if !found.is_payload() {
        return Err(format!(
            "{} is {} - {}",
            path.display(),
            found.describe(),
            found.remedy()
        )
        .into());
    }

    println!("sending {} bytes to {}", payload.len(), target.address);
    let out = pros_link::loader::send(&target.link(), &payload, Duration::from_secs(seconds))?;
    if out.trim().is_empty() {
        println!("nothing arrived on the socket within {seconds}s");
        // Said every time, because the opposite belief is how a working payload gets
        // reported as broken: only a payload launched *this way* reports here at all.
        println!("not necessarily failure: only a payload launched this way reports here");
        println!("at all. If it writes a file, `pros pull` will get it");
    } else {
        print!("{out}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Fetches a file off the target.
///
/// Written by this program rather than left to a shell redirect: a redirect decides the
/// encoding itself and can put a byte-order mark at the front of a file that every parser
/// afterwards has to cope with.
fn pull(
    path: &str,
    into: Option<PathBuf>,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let into = into.unwrap_or_else(|| {
        PathBuf::from(
            path.rsplit('/')
                .next()
                .filter(|last| !last.is_empty())
                .unwrap_or("pulled"),
        )
    });
    let bytes = pros_link::files::retrieve(&target.link(), path)?;
    std::fs::write(&into, &bytes)?;
    println!("{} bytes from {path} -> {}", bytes.len(), into.display());
    Ok(ExitCode::SUCCESS)
}

/// Copies one local file onto the target, at a path the caller chose.
///
/// **Warns before an inert destination.** A file put under a system mount point is not indexed
/// or mounted, so it lands and does nothing - said here rather than left to be discovered.
fn push(from: &Path, to: &str, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if pros_core::guard::is_inert_target_path(to) {
        eprintln!(
            "warning: destination '{to}' is an internal system mount point (/user/app). Uploaded files here will not be indexed or mounted as apps."
        );
    }
    let target = pick(name)?;
    let bytes = std::fs::read(from)?;
    pros_link::files::store(&target.link(), to, &bytes)?;
    println!("{} bytes {} -> {to}", bytes.len(), from.display());
    Ok(ExitCode::SUCCESS)
}

/// Checks a local file against what a manifest says it should be.
fn verify(
    file: &Path,
    against: &str,
    manifest: &Path,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let manifest = Manifest::from_file(manifest)?;
    let payload = manifest
        .find(against)
        .ok_or_else(|| format!("the manifest describes no payload called {against:?}"))?;
    // Both of these are refusals rather than warnings: a payload that cannot be checked and
    // one that fails its check are equally not to be sent.
    let expected = payload.checksum()?;
    expected.verify(&std::fs::read(file)?)?;
    println!("{} is {against}, {expected}", file.display());
    Ok(ExitCode::SUCCESS)
}

/// Reports on a manifest, from here or from the target.
///
/// # Why the target is a source at all
///
/// The payload manager keeps its own repository description, in the schema this project
/// copied rather than invented. **A target that is already configured is already
/// described**, so reading it beats typing it in again - and it is the only way to find out
/// what that file actually looks like, which has not been measured.
///
/// No path is assumed. Where the manager keeps that file is a guess this program is not
/// going to make on somebody's behalf; it is asked for, and when the answer is known it can
/// become a default with a measurement behind it.
fn payloads(
    file: Option<&Path>,
    from_target: Option<&str>,
    check: bool,
    write: bool,
    save: bool,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if write {
        return write_recommended();
    }
    let manifest = match (file, from_target) {
        (Some(file), _) => Manifest::from_file(file)?,
        (None, Some(path)) => {
            let target = pick(name)?;
            let bytes = pros_link::files::retrieve(&target.link(), path)?;
            Manifest::from_json(&String::from_utf8_lossy(&bytes))?
        }
        // The one beside the registry when there is one, and the built-in recommended list
        // when there is not. **Falling back rather than refusing**: a person who has just
        // installed this wants to know what a target ought to be running, and telling them
        // to write a file first is telling them to already know the answer.
        (None, None) => read_or_recommend()?,
    };

    // **Asked for, not assumed.** Reading is a look; keeping is a change to a file
    // somebody may have edited, and a command that quietly rewrote it while showing a table
    // would be doing two things when it was asked to do one.
    let manifest = if save {
        let before = read_or_recommend().unwrap_or_default();
        let merged = before.merged_with(&manifest);
        let (added, changed) = merged.difference_from(&before);
        let path = merged.save()?;
        println!("{added} added, {changed} filled in -> {}", path.display());
        println!();
        merged
    } else {
        manifest
    };

    // Probing is asked for rather than assumed: it costs five ports at a second and a half,
    // and a person who wants the description alone should not pay for it.
    let (report, chain) = if check {
        let target = pick(name)?;
        let report = pros_core::check(&target);
        // **A boot list that could not be read is not an empty one.** The failure becomes
        // `None`, which the survey reports as unknown rather than as absent - and it is said
        // out loud rather than passed over, because the file service being down is itself
        // worth knowing.
        let chain = match pros_core::chain::Chain::read(&target.link()) {
            Ok(chain) => Some(chain),
            Err(why) => {
                println!("could not read the boot list ({why})");
                println!("so what comes back after a reboot is unknown, not empty");
                println!();
                None
            }
        };
        (Some(report), chain)
    } else {
        (None, None)
    };
    say::payloads(
        &pros_core::payloads::survey(&manifest, report.as_ref(), chain.as_ref()),
        check,
    );
    Ok(ExitCode::SUCCESS)
}

/// Copies a folder off the target.
fn backup(
    from: &str,
    into: Option<PathBuf>,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let into = into.unwrap_or_else(|| {
        PathBuf::from(
            from.rsplit('/')
                .next()
                .filter(|last| !last.is_empty())
                .unwrap_or("backup"),
        )
    });
    let mut session = pros_link::files::Session::open(&target.link())?;
    // Printed as it happens: a backup of any size is a long silence otherwise, and a
    // person watching cannot tell waiting from stuck.
    // Nothing to press here: on a command line, the way to stop something is to stop it, and
    // the shell already offers that. Saying never rather than pretending otherwise.
    let summary = pros_core::transfer::download(
        &mut session,
        from,
        &into,
        &mut |progress| {
            println!("  {}", progress.current);
        },
        &|| false,
    );
    session.close();
    Ok(say::copied(&summary?, &into.display().to_string()))
}

/// Which files a transfer may skip, from the `--all` flag.
///
/// **Skip-unchanged is the default**, because re-sending a whole title where one file changed is
/// the cost worth removing; `--all` forces every file across, for when the record cannot be
/// trusted. See `pros_core::deployed`.
fn resend(all: bool) -> pros_core::transfer::Resend {
    if all {
        pros_core::transfer::Resend::Everything
    } else {
        pros_core::transfer::Resend::OnlyChanged
    }
}

/// Puts a folder back onto the target.
fn restore(
    from: &Path,
    to: &str,
    yes: bool,
    force: bool,
    all: bool,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut target_dest = to.to_string();
    if !force && let Some(refusal) = pros_core::guard::check(from, to) {
        if yes {
            eprintln!("redirecting: {}", refusal.explanation);
            eprintln!("using suggested path: {}", refusal.suggested_path);
            target_dest = refusal.suggested_path;
        } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            use std::io::Write as _;
            eprintln!("\nRefusal: {}", refusal.explanation);
            eprintln!("  Remedy:    {}", refusal.remedy);
            eprintln!("  Requested: {}", refusal.target_path);
            eprintln!("  Suggested: {}", refusal.suggested_path);
            eprint!(
                "\nUse suggested path '{}' instead? [Y/n] ",
                refusal.suggested_path
            );
            std::io::stderr().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            let choice = line.trim();
            if choice.is_empty()
                || choice.eq_ignore_ascii_case("y")
                || choice.eq_ignore_ascii_case("yes")
            {
                target_dest = refusal.suggested_path;
                eprintln!("Proceeding with destination {target_dest}");
            } else {
                eprintln!("Transfer aborted. Pass --force to upload to requested path anyway.");
                return Ok(ExitCode::FAILURE);
            }
        } else {
            eprintln!("Refusal: {}", refusal.explanation);
            eprintln!("  Remedy:    {}", refusal.remedy);
            eprintln!("  Requested: {}", refusal.target_path);
            eprintln!("  Suggested: {}", refusal.suggested_path);
            eprintln!("Pass -y / --yes to accept suggested path, or --force to override.");
            return Ok(ExitCode::FAILURE);
        }
    }
    let target = pick(name)?;
    let mut session = pros_link::files::Session::open(&target.link())?;
    let mut deployed = pros_core::deployed::load();
    let summary = {
        let ledger = deployed.for_target(&target.name);
        let done = pros_core::transfer::upload(
            &mut session,
            from,
            &target_dest,
            ledger,
            resend(all),
            &mut |progress| {
                println!("  {}", progress.current);
            },
            &|| false,
        );
        session.close();
        done
    }?;
    // A record of what landed, so the next restore can skip what did not change. A cache: if it
    // will not write, the only cost is a full re-send next time, so it is a note rather than a
    // failure.
    if let Err(why) = pros_core::deployed::save(&deployed) {
        eprintln!("note: could not record what landed for next time: {why}");
    }
    Ok(say::copied(&summary, &target_dest))
}

/// What a registry command is asking for.
enum Registry {
    /// Remember this address under this name.
    Add(String, String),
    /// Say what is remembered.
    Show,
    /// Forget this name.
    Remove(String),
}

/// The three commands that touch the registry and no target.
///
/// Grouped because they are one subject, and because a dispatch that holds every command
/// inline grows until nobody reads it.
fn registry(what: &Registry) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match what {
        Registry::Add(name, address) => {
            let path = target::register(name, address)?;
            println!("registered {name} at {address}");
            println!("  {}", path.display());
        }
        Registry::Show => {
            let targets = target::load()?;
            if targets.is_empty() {
                println!("no targets registered - see `pros register <address>`");
            }
            for one in &targets {
                println!("{:<16} {}", one.name, one.address);
            }
        }
        Registry::Remove(name) => {
            if target::forget(name)? {
                println!("forgot {name}");
            } else {
                println!("nothing was registered as {name}");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The manifest beside the registry, or the built-in list when there is none.
fn read_or_recommend() -> Result<Manifest, Box<dyn std::error::Error>> {
    let path = pros_core::manifest::Tracked::Payloads.path();
    if let Some(path) = path.filter(|path| path.exists()) {
        return Ok(Manifest::from_file(&path)?);
    }
    // Said out loud, because where a list came from decides how much to trust it.
    println!("no manifest of your own, so this is the built-in list");
    println!("read off a target's own repository - `pros payloads --write` to edit it");
    println!();
    Ok(pros_core::manifest::Tracked::Payloads.shipped())
}

/// Watches a probe's port and re-sends it when it stops answering.
///
/// # Why this waits rather than polls hard
///
/// The thing on the other end is being driven by somebody asking questions, and most of those
/// questions take milliseconds. A supervisor that checked constantly would spend the target's
/// time competing with the driver for it; one that checks every second or so notices a death
/// within a second of it mattering, which is as fast as anybody can use.
///
/// # Every restart is printed
///
/// The driver detects a restart by the probe's session identifier changing. This side knows
/// for certain, and a restart nobody mentioned would let two separate processes read as one
/// continuous session - the discontinuity the protocol takes care to keep visible.
///
/// # Errors
///
/// When the probe cannot be read, or no target is registered. A send that fails is **not** an
/// error: it is one dead start, counted, and the loop carries on until the patience runs out.
fn supervise(
    path: &Path,
    port: u16,
    patience: usize,
    restarts: usize,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    /// How long to wait for a connection before calling the port shut.
    const REACH: Duration = Duration::from_millis(400);
    /// How long between looks.
    const BETWEEN: Duration = Duration::from_secs(1);
    /// How long to listen to a freshly sent probe before checking on it.
    const SETTLING: Duration = Duration::from_secs(2);

    let target = pick(name)?;
    let payload = std::fs::read(path)?;
    let mut supervisor = pros_core::supervise::Supervisor::new(patience);

    println!(
        "supervising {} on {} ({}), port {port}",
        path.display(),
        target.name,
        target.address
    );
    println!("every restart is printed. Ctrl-C to stop.");

    let mut alive = false;
    loop {
        let answering = pros_core::supervise::is_answering(&target.address, port, REACH);
        if answering != alive {
            // Said in both directions: a probe coming back is as much a fact about the
            // session as one going away, and a driver reading this log needs both to line
            // its records up against.
            println!(
                "  {}",
                if answering {
                    "answering"
                } else {
                    "not answering"
                }
            );
            alive = answering;
        }
        match supervisor.next(answering) {
            pros_core::supervise::Step::Answering => std::thread::sleep(BETWEEN),
            pros_core::supervise::Step::Resend { attempt } => {
                println!("  sending again (attempt {attempt})");
                match pros_link::loader::send(&target.link(), &payload, SETTLING) {
                    Ok(said) if said.trim().is_empty() => {}
                    Ok(said) => println!("    {}", said.trim()),
                    // Not fatal. A loader that refused is one dead start, and the patience
                    // is what decides whether to keep trying.
                    Err(why) => println!("    the loader refused: {why}"),
                }
                if restarts > 0 && attempt >= restarts {
                    println!("stopping: {restarts} restarts, as asked");
                    return Ok(ExitCode::SUCCESS);
                }
            }
            pros_core::supervise::Step::GaveUp { after, why } => {
                eprintln!("giving up after {after}: {why}");
                return Ok(ExitCode::FAILURE);
            }
        }
    }
}

/// Writes the built-in list where a person can edit it.
fn write_recommended() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let path = pros_core::manifest::default_path()
        .ok_or("no home directory, so there is nowhere for a manifest to live")?;
    // **Refused rather than overwritten.** The thing this would destroy is the digests
    // somebody typed in by hand, which is the expensive half of a manifest.
    if path.exists() {
        return Err(format!(
            "{} already exists and is not overwritten - what it holds that this does not is exactly the part somebody had to find out",
            path.display()
        )
        .into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, pros_core::manifest::recommended().to_json()?)?;
    println!("written to {}", path.display());
    println!("  add a url and a checksum to each entry and they become sendable");
    Ok(ExitCode::SUCCESS)
}

/// Lists a directory on the target and says what the entries look like.
fn library(
    path: &str,
    titles_only: bool,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let entries = pros_link::files::list(&target.link(), path)?;
    let items = pros_core::library::scan(&entries);
    let shown: Vec<&pros_core::library::Item> = if titles_only {
        pros_core::library::titles(&items)
    } else {
        items.iter().collect()
    };
    say::library(&shown);
    Ok(ExitCode::SUCCESS)
}

/// Asks a target what it can do, and optionally does something about the answer.
///
/// **A tool that can see a problem and cannot act on it has left the interesting half
/// undone.** What it can do is narrow and stays narrow: send something that is missing, is
/// described, and is already here verified. It does not fetch, and it does not touch the
/// boot list - what that file accepts has not been measured.
fn check(fix: bool, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let report = pros_core::check(&target);
    say::report(&report);

    if !fix {
        return Ok(match report.verdict() {
            Verdict::Blocked { .. } => ExitCode::from(BLOCKED),
            _ => ExitCode::SUCCESS,
        });
    }

    let manifest = read_or_recommend()?;
    let missing: Vec<&str> = report
        .findings
        .iter()
        .filter(|finding| !finding.reachability.open)
        .map(|finding| finding.service.name.as_ref())
        .collect();
    if missing.is_empty() {
        println!();
        println!("nothing to fix");
        return Ok(ExitCode::SUCCESS);
    }

    println!();
    let mut sent = 0_usize;
    for name in missing {
        let staged = manifest
            .find(name)
            .and_then(pros_core::staging::path_for)
            .filter(|path| path.exists());
        let Some(path) = staged else {
            // Named, not skipped. *Not here* and *sent* are different outcomes and a
            // person reading this needs to know which happened to which.
            println!("{name:<10} not staged here - `pros fetch {name} --from-target`");
            continue;
        };
        let payload = std::fs::read(&path)?;
        match pros_link::loader::send(&target.link(), &payload, Duration::from_secs(3)) {
            Ok(_) => {
                sent += 1;
                println!("{name:<10} sent");
            }
            Err(why) => println!("{name:<10} {why}"),
        }
    }

    if sent == 0 {
        return Ok(ExitCode::from(BLOCKED));
    }
    // Asked again rather than assumed: sending a payload and it answering are two things,
    // and only the second one is what somebody wanted.
    println!();
    println!("asking again");
    let after = pros_core::check(&target);
    say::report(&after);
    Ok(match after.verdict() {
        Verdict::Blocked { .. } => ExitCode::from(BLOCKED),
        _ => ExitCode::SUCCESS,
    })
}

/// Lists the saves on a target, named by the game they belong to.
fn saves(name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let where_to = match pros_core::saves::find(&target.link())? {
        pros_core::saves::Found::Here(path) => path,
        // Offered rather than chosen between: a target with two accounts has two people's
        // saves on it.
        pros_core::saves::Found::Several(users) => {
            println!("several users, so this does not choose between them:");
            for user in users {
                println!(
                    "  {}/{user}/{}",
                    pros_core::saves::HOME,
                    pros_core::saves::SAVES
                );
            }
            println!();
            println!("give one to `pros library <path>`, or back it up by name");
            return Ok(ExitCode::SUCCESS);
        }
        pros_core::saves::Found::None => {
            println!("no user folders under {}", pros_core::saves::HOME);
            return Ok(ExitCode::SUCCESS);
        }
    };

    println!("{where_to}");
    let entries = pros_link::files::list(&target.link(), &where_to)?;
    let found = pros_core::library::scan(&entries);
    if found.is_empty() {
        println!("  nothing saved here");
        return Ok(ExitCode::SUCCESS);
    }
    for item in &found {
        // A save belongs to a title, and the title's own description names it - when that
        // title is still installed. One that is not shows its identifier, which is true.
        let named = pros_core::titles::read(&target.link(), &item.name)
            .ok()
            .and_then(|about| about.name);
        println!(
            "  {:<12} {}",
            item.name,
            named.unwrap_or_else(|| "(not installed, so nothing names it)".to_owned())
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Lists what is installed, by name.
///
/// One round trip per title, because the names live one file down. Worth it: a list of
/// identifiers is a list somebody has to decode, and the decoding is not something they can
/// do without the target.
fn titles(appmeta: &str, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let entries = pros_link::files::list(&target.link(), appmeta)?;
    let found = pros_core::library::scan(&entries);
    let installed = pros_core::library::titles(&found);

    if installed.is_empty() {
        println!("nothing at {appmeta} looks like a title");
        return Ok(ExitCode::SUCCESS);
    }

    let mut unread = 0_usize;
    for item in installed {
        match pros_core::titles::read(&target.link(), &item.name) {
            Ok(about) => println!(
                "{:<12} {:<10} {}",
                about.id,
                about.version.as_deref().unwrap_or("-"),
                about.display()
            ),
            // **The identifier, and a mark saying why that is all there is.** A title whose
            // description could not be read is not a title with no name.
            Err(why) => {
                unread += 1;
                println!("{:<12} {:<10} ? {why}", item.name, "-");
            }
        }
    }
    if unread > 0 {
        println!();
        println!("{unread} could not be read - those rows show an identifier, not a name");
    }
    Ok(ExitCode::SUCCESS)
}

/// Starts an installed title, and says what the target made of being asked.
///
/// # Why this exists when `pros sh "launch PPSA00000"` sends the same bytes
///
/// It does send the same bytes. What the shell cannot do is the two things either side of
/// them.
///
/// **Before**: the shell splits its line on spaces and offers no quoting, and the builtin
/// hands everything from the first word onwards to the application as its own arguments. A
/// stray word therefore does not start the wrong title - it starts the right one and passes
/// it something nobody meant to pass. That is checked here and refused, not trimmed.
///
/// **After**: a refusal arrives as a `perror` line on the same socket as everything else, so
/// a shell that printed whatever came back would exit zero on a launch the target turned
/// down. The answer is read, and the exit code is the reading.
fn launch(id: &str, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    // A usage complaint, so it goes where usage complaints go. What the *target* says is a
    // result and goes to stdout below - the two are different kinds of thing and a script
    // that pipes one should not catch the other.
    if !pros_core::launch::is_an_app_id(id) {
        eprintln!("not an application identifier: {id}");
        eprintln!("nine characters, four letters then five digits, no spaces");
        eprintln!("`pros titles` lists what is installed, identifier first");
        return Ok(ExitCode::FAILURE);
    }

    let target = pick(name)?;
    let said = pros_link::shell::run(&target.link(), &pros_core::launch::command(id), SETTLE)?;
    let said = pros_core::launch::read(&said);
    println!("{}", said.describe());

    match said {
        // **Asked, which is not started.** There is no reply that means a game came up, so
        // zero here says the target took the request - and the wording above says exactly
        // that rather than letting an exit code imply more than was measured.
        pros_core::launch::Said::Asked(_) => Ok(ExitCode::SUCCESS),
        // Both are the target declining, one by printing its usage and one by naming the
        // call that failed. A drawn negative is the whole reason this is not `pros sh`.
        pros_core::launch::Said::NotAnId | pros_core::launch::Said::Refused(_) => {
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Restarts the user interface to clear a softlock.
fn restart_ui(name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let listing = pros_link::shell::run(&target.link(), "ps", SETTLE)?;
    let processes = pros_core::system::processes(&listing);
    let Some(ui) = pros_core::system::shell_ui(&processes) else {
        println!("SceShellUI is not running - nothing to restart");
        return Ok(ExitCode::FAILURE);
    };
    let pid = ui.pid.clone();
    println!("restarting the user interface (SceShellUI, PID {pid})");
    let said = pros_link::shell::run(
        &target.link(),
        &pros_core::system::kill(&pid, pros_core::system::Signal::Terminate),
        SETTLE,
    )?;
    if !said.trim().is_empty() {
        print!("{said}");
    }
    println!("SceSysCore respawns it, so the screen comes back on its own");
    Ok(ExitCode::SUCCESS)
}

/// Closes a title, freeing what it holds open.
fn close(id: &str, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let listing = pros_link::shell::run(&target.link(), "ps", SETTLE)?;
    let processes = pros_core::system::processes(&listing);
    let mine = pros_core::system::of_title(&processes, id);
    if mine.is_empty() {
        println!("no running process found for {id}");
        return Ok(ExitCode::SUCCESS);
    }
    for process in mine {
        println!(
            "closing {id} (PID {}, state {})",
            process.pid, process.state
        );
        for command in pros_core::system::end(process) {
            let _ = pros_link::shell::run(&target.link(), &command, SETTLE)?;
        }
    }
    // Ask again rather than assume: a title still listed did not close, and saying so is worth
    // more than an exit code that implies it did.
    let after = pros_link::shell::run(&target.link(), "ps", SETTLE)?;
    if pros_core::system::of_title(&pros_core::system::processes(&after), id).is_empty() {
        println!("{id} is gone");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("{id} is still listed - it did not close");
        Ok(ExitCode::FAILURE)
    }
}

/// Ends one process by pid, freeing what it holds open.
///
/// The same primitive `close` uses, aimed by pid rather than by title: read the listing, find the
/// one process it names, and run the kill commands `pros_core::system::end` produces - which wake
/// a stopped process before killing it. A pid nothing is using is said, not signalled into.
fn kill_pid(pid: &str, name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let listing = pros_link::shell::run(&target.link(), "ps", SETTLE)?;
    let processes = pros_core::system::processes(&listing);
    let Some(process) = pros_core::system::by_pid(&processes, pid) else {
        println!("no process with pid {} is running", pid.trim());
        return Ok(ExitCode::FAILURE);
    };
    let pid = process.pid.clone();
    println!(
        "ending {} (pid {pid}, state {}){}",
        if process.command.is_empty() {
            "the process"
        } else {
            &process.command
        },
        process.state,
        if process.title.is_empty() {
            String::new()
        } else {
            format!(", title {}", process.title)
        }
    );
    for command in pros_core::system::end(process) {
        let _ = pros_link::shell::run(&target.link(), &command, SETTLE)?;
    }
    // Ask again rather than assume, exactly as `close` does: a pid still listed did not end.
    let after = pros_link::shell::run(&target.link(), "ps", SETTLE)?;
    if pros_core::system::by_pid(&pros_core::system::processes(&after), &pid).is_none() {
        println!("pid {pid} is gone");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("pid {pid} is still listed - it did not end");
        Ok(ExitCode::FAILURE)
    }
}

/// Lists the processes running on the target.
///
/// The same `ps` the window's system panel reads, parsed the same way, so a pid handed to
/// `pros kill` comes from here rather than from a raw shell. Prints the columns this project
/// keeps - pid, state, title, command - and nothing the parser dropped.
fn ps(name: Option<&str>) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let listing = pros_link::shell::run(&target.link(), "ps", SETTLE)?;
    let processes = pros_core::system::processes(&listing);
    if processes.is_empty() {
        println!("no processes listed - is the shell loaded? `pros check` will say");
        return Ok(ExitCode::SUCCESS);
    }
    say::processes(&processes);
    Ok(ExitCode::SUCCESS)
}

/// Watches the running processes, redrawing on an interval until stopped.
///
/// The live form of [`ps`]: the same table, re-read every `every` seconds until Ctrl-C or the
/// `--seconds` cap. Read-only, like `ps` - to end something, `pros close` / `pros kill` - so it
/// needs no interactive key handling and no terminal raw mode. On a terminal it clears between
/// draws; piped, it prints successive tables so the output stays readable in a file. A target that
/// blips is reported and the watch goes on, because a monitor that quits on one missed read is one
/// that is never running when it is wanted.
fn top(
    seconds: Option<u64>,
    every: u64,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let every = every.max(1);
    let clears = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let deadline = seconds.map(|s| std::time::Instant::now() + Duration::from_secs(s));
    loop {
        match pros_link::shell::run(&target.link(), "ps", SETTLE) {
            Ok(listing) => {
                let processes = pros_core::system::processes(&listing);
                let titles = processes.iter().filter(|p| p.is_a_title()).count();
                if clears {
                    // Clear the screen and home the cursor, so each draw replaces the last rather
                    // than scrolling. No raw mode, so nothing to restore on Ctrl-C.
                    print!("\x1b[2J\x1b[H");
                }
                println!(
                    "{}  -  {} processes, {titles} titles  -  every {every}s, Ctrl-C to stop  -  MEM = MiB in use",
                    target.address,
                    processes.len()
                );
                if processes.is_empty() {
                    println!("no processes listed - is the shell loaded? `pros check` will say");
                } else {
                    say::processes(&processes);
                }
            }
            Err(why) => eprintln!("could not read the target: {why}"),
        }
        if let Some(deadline) = deadline
            && std::time::Instant::now() >= deadline
        {
            break;
        }
        std::thread::sleep(Duration::from_secs(every));
    }
    Ok(ExitCode::SUCCESS)
}

/// Waits until the target has the title registered so `launch` can resolve it, up to `timeout`.
///
/// **The files on disk are not the title being registered.** A restore lands the tree under
/// `/data/homebrew/<id>` at once, but `ShadowMountPlus` has to mount it and the shell has to register
/// it before it appears in the appmeta list ([`pros_core::titles::APPMETA`], the same list
/// `pros titles` reads) and before `launch` will start it. So this polls that list for the id -
/// which is the very thing that gates the launch - rather than the filesystem, which says yes
/// immediately and would launch too soon. Returns `true` once the id is listed, `false` if the
/// timeout elapses first.
fn wait_for_registration(link: &pros_link::Link, id: &str, timeout: Duration) -> bool {
    use std::io::Write as _;

    print!("waiting for {id} to register in the title list");
    let _ = std::io::stdout().flush();
    let deadline = std::time::Instant::now() + timeout;
    let listed = loop {
        // The id is the folder name under appmeta; a listing that fails (the service is not up
        // yet, say) is simply "not yet", to be retried until the deadline.
        let there = pros_link::files::list(link, pros_core::titles::APPMETA)
            .is_ok_and(|entries| entries.iter().any(|e| e.name.eq_ignore_ascii_case(id)));
        if there {
            break true;
        }
        if std::time::Instant::now() >= deadline {
            break false;
        }
        print!(".");
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(2));
    };
    println!();
    listed
}

/// Deploys a homebrew title from a local build, launches it, and follows its log until it ends.
///
/// The probe loop in one command. Each step is an existing capability - close, restore, launch,
/// follow the log - tied together here the way [`logs`] ties its own stream and watcher, because
/// this is an interactive orchestration and the pieces it stands on are all in `pros-core`. Two
/// honest limits, stated at the `Probe` variant and again where they bite below: a parked big-app
/// ignores the close, and a title that parks rather than exits ends the watch at the cap.
fn probe(
    id: &str,
    from: &Path,
    seconds: u64,
    all: bool,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if !pros_core::launch::is_an_app_id(id) {
        eprintln!("not an application identifier: {id}");
        eprintln!("nine characters, four letters then five digits, no spaces");
        return Ok(ExitCode::FAILURE);
    }

    let target = pick(name)?;
    let link = target.link();
    let dest = pros_core::guard::homebrew_path(id);

    // 1. Close it if it is running. **Best-effort.** A parked big-app ignores every signal
    //    (measured; oops-mesa's b1e4), so this ends a killable process and no more - the launch
    //    below is what says whether the slot is still held.
    let listing = pros_link::shell::run(&link, "ps", SETTLE).unwrap_or_default();
    let running = pros_core::system::processes(&listing);
    let mine = pros_core::system::of_title(&running, id);
    if mine.is_empty() {
        println!("{id} is not running");
    } else {
        println!("closing {id} ({} process(es))...", mine.len());
        for process in &mine {
            for command in pros_core::system::end(process) {
                let _ = pros_link::shell::run(&link, &command, SETTLE);
            }
        }
    }

    // 2. Restore the local build into /data/homebrew/<id>, overwriting. A half-landed deploy is
    //    not launched: `say::copied` prints the count, and its incomplete branch exits non-zero.
    //    Skip-unchanged by default (the whole point of the deploy loop is that most files are the
    //    same build to build); `--all` forces every file across. What landed is remembered for
    //    next time, and a cache that will not write is a note, not a failure.
    println!("restoring {} -> {dest}", from.display());
    let mut session = pros_link::files::Session::open(&link)?;
    let mut deployed = pros_core::deployed::load();
    let summary = {
        let ledger = deployed.for_target(&target.name);
        let done = pros_core::transfer::upload(
            &mut session,
            from,
            &dest,
            ledger,
            resend(all),
            &mut |_| {},
            &|| false,
        );
        session.close();
        done
    }?;
    if let Err(why) = pros_core::deployed::save(&deployed) {
        eprintln!("note: could not record what landed for next time: {why}");
    }
    let restored = say::copied(&summary, &dest);
    if !summary.is_complete() {
        eprintln!("not launching {id} - the deploy did not land cleanly");
        return Ok(restored);
    }

    // 3. **Wait for the console to register the title before launching.** The files are on disk
    //    the moment the restore finishes, but ShadowMountPlus has to mount and register the title
    //    before it appears in the appmeta list (the one `pros titles` reads) and before `launch`
    //    will resolve it - and the hand-run recipe only got away with launching straight after a
    //    restore because typing the second command gave registration a few seconds. This poll is
    //    that pause made explicit: the appmeta list, up to a minute.
    if !wait_for_registration(&link, id, Duration::from_secs(60)) {
        eprintln!(
            "{id} did not appear in the title list within 60s - not launching. It restored, but \
             the console has not registered it (ShadowMountPlus may not have mounted it)."
        );
        return Ok(ExitCode::FAILURE);
    }
    println!("{id} is registered.");

    // 4. **Attach the log follower BEFORE launching.** These probes do their whole job in the
    //    first second or two and then park silently, so a follower attached *after* the launch
    //    misses all of it: the output lands in the gap between the launch returning and the stream
    //    opening, and the run succeeds while its capture is empty (measured, and the reason this
    //    order matters). The connection is the subscription - `log::follow` returns once it is
    //    open, and klogsrv buffers what it emits after that - so following first and launching
    //    second is the fix. `SUBSCRIBE_SETTLE` is a conservative beat so the stream is certainly
    //    live before the launch it must not miss; it is the same ordering the hand-run recipe got
    //    with a second window and a sleep.
    println!("attaching to {id}'s log before launch...");
    let (stopper, lines) = pros_link::log::follow(&link)?;
    std::thread::sleep(SUBSCRIBE_SETTLE);

    // 5. Launch it, now that the title is registered and the follower is up to catch it.
    let said = pros_core::launch::read(&pros_link::shell::run(
        &link,
        &pros_core::launch::command(id),
        SETTLE,
    )?);
    println!("launching {id}: {}", said.describe());
    if !matches!(said, pros_core::launch::Said::Asked(_)) {
        stopper.stop();
        eprintln!(
            "the launch was refused - if a parked title is holding the slot it cannot be closed \
             by signal (see `pros close`); the console's own dashboard Close ends it"
        );
        return Ok(ExitCode::FAILURE);
    }

    // 6. Follow the attached stream until the title parks, exits, or the cap elapses.
    println!("following {id} (until it parks, exits, or {seconds}s)...");
    match follow_stream(stopper, lines, &target, id, seconds) {
        FollowEnd::Finished => println!(
            "{id} finished its work and parked - a payload cannot exit, so it idles until the \
             dashboard Close ends it."
        ),
        FollowEnd::Exited => println!("{id} left the process list - it exited or crashed."),
        FollowEnd::Parked => println!(
            "reached the {seconds}s cap - {id} is still running (a probe that finished may have \
             parked; the dashboard Close ends it)."
        ),
        FollowEnd::NeverSeen => println!(
            "reached the {seconds}s cap - {id} was never seen in the process list, so it exited \
             at once or did not start."
        ),
    }
    Ok(ExitCode::SUCCESS)
}

/// A beat between attaching the log follower and issuing the launch.
///
/// **The connection is the subscription** - [`pros_link::log::follow`] returns once the socket to
/// klogsrv is open, and everything klogsrv emits after that is buffered until it is read, so the
/// ordering (follow, then launch) is what prevents the loss. This settle is insurance on top of
/// the ordering, not the mechanism: a conservative margin so the stream is certainly live before a
/// launch whose output arrives and parks within a second or two, where missing the subscription
/// loses the whole run. Measured need is sub-second; this is deliberately more.
const SUBSCRIBE_SETTLE: Duration = Duration::from_secs(3);

/// How a `follow_title` watch ended.
enum FollowEnd {
    /// The payload printed its park sentinel - it finished its work and is now idling. The
    /// only ending that distinguishes "done" from "still going" for a title that cannot exit.
    Finished,
    /// The title was seen and then left the process list - it exited or crashed.
    Exited,
    /// The cap elapsed with the title still present - a finished probe that parked rather than
    /// exiting looks exactly like this.
    Parked,
    /// The cap elapsed without the title ever appearing - it exited at once or never started.
    NeverSeen,
}

/// Streams an already-attached log until the title parks, leaves the process list, or `seconds`
/// elapse.
///
/// **The follower is attached by the caller, before the launch** - see the ordering note in
/// `probe`. This takes the open stream (`stopper`, `lines`) rather than opening it, so the
/// subscription is already live by the time the title prints anything.
///
/// **Two connections, two services, on purpose.** The stream is klogsrv and the poll is shsrv, so
/// they do not contend: a background watcher runs `ps` once a second while the foreground drains
/// the log to stdout, and stopping the stream is what ends the drain. The watcher waits for the
/// title to *appear* before treating its absence as an exit, so the gap between a launch and the
/// process showing is never read as a crash.
fn follow_stream<I>(
    stopper: pros_link::log::Stopper,
    lines: I,
    target: &Target,
    id: &str,
    seconds: u64,
) -> FollowEnd
where
    I: Iterator<Item = pros_link::log::Line>,
{
    use std::io::Write as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let stopper = Arc::new(stopper);
    let done = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicBool::new(false));
    let (stopper_w, done_w, exited_w, seen_w) = (
        Arc::clone(&stopper),
        Arc::clone(&done),
        Arc::clone(&exited),
        Arc::clone(&seen),
    );
    let target_w = target.clone();
    let id_w = id.to_owned();
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);

    let watcher = std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if done_w.load(Ordering::Relaxed) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                stopper_w.stop();
                break;
            }
            let ps = pros_link::shell::run(&target_w.link(), "ps", SETTLE).unwrap_or_default();
            let present =
                !pros_core::system::of_title(&pros_core::system::processes(&ps), &id_w).is_empty();
            if present {
                seen_w.store(true, Ordering::Relaxed);
            } else if seen_w.load(Ordering::Relaxed) {
                exited_w.store(true, Ordering::Relaxed);
                stopper_w.stop();
                break;
            }
        }
    });

    let mut any = false;
    let mut finished = false;
    for line in lines {
        match line {
            Ok(l) => {
                println!("{l}");
                let _ = std::io::stdout().flush();
                any = true;
                // **The payload saying it is done.** A homebrew title cannot exit - `exit`,
                // `_Exit` and `sceKernelExit` are absent, `_exit` raises `SIGSYS` under a
                // big-app's credentials, and returning from the entry point faults at zero -
                // so the conforming ending is to park, and a finished probe is indistinguish-
                // able from a working one by the process list alone. oops-sdk's
                // `oops_system_park_until_closed` prints this line once, immediately before it
                // starts idling, so that a watcher does not have to wait out its whole cap
                // after a run that is already over. Matched on the tail, not the whole line,
                // because the log prefixes it with the title and the app id.
                if l.contains(PARK_SENTINEL) {
                    finished = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    done.store(true, Ordering::Relaxed);
    stopper.stop();
    let _ = watcher.join();

    if !any {
        println!("the log was quiet while {id} ran - which is a result, not a failure");
    }
    if finished {
        FollowEnd::Finished
    } else if exited.load(Ordering::Relaxed) {
        FollowEnd::Exited
    } else if seen.load(Ordering::Relaxed) {
        FollowEnd::Parked
    } else {
        FollowEnd::NeverSeen
    }
}

/// What a payload prints immediately before it parks, from oops-sdk's
/// `oops_system_park_until_closed`.
///
/// **The tag goes inside the brackets, not before the message.** oops-sdk's klog renders
/// `[<app id>:<tag>] <message>`, so the line on the wire is `[GLPB00001:park] work done` and a
/// payload with no app id set prints `[park] work done`. This matched `park: work done` when it
/// first shipped and therefore matched nothing: the sentinel was printed on 2026-09-21 at
/// 11:47Z and the watch ran to its cap anyway. Matching from the closing bracket covers both
/// spellings and cannot collide with a title whose own log says "work done".
const PARK_SENTINEL: &str = "park] work done";

/// Fetches payloads and keeps the ones that are what they claim to be.
fn fetch(
    wanted: Option<&str>,
    all: bool,
    from_target: bool,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let manifest = if from_target {
        // The target's own repository carries urls **and** digests, which is what makes
        // fetching worth doing at all. (D013)
        let target = pick(name)?;
        let bytes =
            pros_link::files::retrieve(&target.link(), pros_core::manifest::TARGET_REPOSITORY)?;
        Manifest::from_json(&String::from_utf8_lossy(&bytes))?
    } else {
        read_or_recommend()?
    };

    let chosen: Vec<&pros_core::manifest::Payload> = match (wanted, all) {
        (Some(wanted), _) => vec![
            manifest
                .find(wanted)
                .ok_or_else(|| format!("the manifest describes no payload called {wanted:?}"))?,
        ],
        (None, true) => manifest
            .payloads()
            .iter()
            .filter(|payload| !pros_core::staging::is_staged(payload))
            .collect(),
        (None, false) => return Err("name one, or --all".into()),
    };

    if chosen.is_empty() {
        println!("everything the manifest describes is already here");
        return Ok(ExitCode::SUCCESS);
    }

    let mut kept = 0_usize;
    let mut refused = Vec::new();
    for payload in chosen {
        print!("{:<28} ", payload.name);
        match pros_core::fetch::fetch(payload) {
            Ok(into) => {
                kept += 1;
                println!("kept, verified: {}", into.display());
            }
            Err(why) => {
                println!("{why}");
                refused.push(payload.name.clone());
            }
        }
    }

    println!();
    println!("{kept} kept");
    if refused.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    // **Not a footnote.** Anything that did not arrive, or arrived wrong, is the thing
    // somebody needs to act on.
    println!("{} not: {}", refused.len(), refused.join(", "));
    Ok(ExitCode::FAILURE)
}

/// Keeps a payload ready to send, having checked it is the one described.
///
/// **The check happens on the way in, not on the way out**, so that everything in the
/// staging directory is already known to be what it claims. A file dropped there by hand is
/// not, which is the whole reason this command exists rather than a note saying where to
/// put things.
fn stage(
    file: &Path,
    name: &str,
    manifest: Option<&Path>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let manifest = read_manifest(manifest)?;
    let payload = manifest
        .find(name)
        .ok_or_else(|| format!("the manifest describes no payload called {name:?}"))?;
    let into = pros_core::staging::accept(payload, file)?;
    println!("staged {}", into.display());
    println!("  it is what the manifest says it should be, so it can be sent");
    Ok(ExitCode::SUCCESS)
}

/// Reads a manifest from a path, or from the usual place beside the registry.
fn read_manifest(named: Option<&Path>) -> Result<Manifest, Box<dyn std::error::Error>> {
    if let Some(path) = named {
        return Ok(Manifest::from_file(path)?);
    }
    let path = pros_core::manifest::default_path()
        .ok_or("no home directory, so there is nowhere for a manifest to live")?;
    // Absent is its own message. *There is none yet* and *this one will not read* are
    // different problems for different people.
    if !path.exists() {
        return Err(format!("no manifest at {}", path.display()).into());
    }
    Ok(Manifest::from_file(&path)?)
}

/// The target to act on.
fn pick(name: Option<&str>) -> Result<Target, Box<dyn std::error::Error>> {
    Ok(target::resolve(target::load()?, name)?)
}
