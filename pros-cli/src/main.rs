//! `pros` - one instrument for talking to a prepared target.
//!
//! Here *target* means a registered target (`pros register`), never a build or install target.
//! The logic lives in `pros-core` and `pros-link`, shared with the window; this binary holds
//! argument parsing and wording.
//!
//! Exit codes are part of the interface:
//! - 0 - it worked, or the target answered and the answer was "not ready".
//! - 1 - this program could not do what it was asked: no such target, a missing file, a
//!   failed transfer, a title the target refused to start.
//! - 2 - a check found the target blocked. An absent target is an answer, not a malfunction,
//!   so a script can tell it from a tool failure without reading the message.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use pros_core::check::Verdict;
use pros_core::manifest::Manifest;
use pros_core::target::{self, Target};

mod say;

/// What a blocked check exits with.
const BLOCKED: u8 = 2;

/// What every command returns: an exit code, or the error that stopped it.
type CliResult<T = ExitCode> = Result<T, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(
    name = "pros",
    about = "Talk to a prepared target: register it, ask what it can do, move files, run things",
    // The same build line the window's footer shows, from the same source.
    version = pros_core::build::line_static()
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// The same global flag every subcommand carries.
    ///
    /// Read here too so the warning before the subcommand knows which target was meant. A
    /// global argument is populated at both levels.
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
    /// Ask a target what it can do now
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
    /// A conformance probe faults as a normal part of its work and leaves restarting to
    /// someone else. This watches its port and, when it stops answering, sends the same
    /// bytes again.
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
    /// The identifier, not a path. This asks the target's system service to start an
    /// application the way the home screen does; it does not run an ELF (`pros send` does).
    Launch {
        /// Which title. Nine characters, four letters then five digits - `pros titles`
        /// lists them
        ///
        /// An identifier, not a name: the builtin resolves nothing else.
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
    /// Ends every process the title owns. A stopped process is woken first so its exit
    /// teardown completes; killing it while stopped leaves locked files behind.
    Close {
        /// Which title, by identifier - `pros titles` lists them
        id: String,
        #[command(flatten)]
        which: Which,
    },
    /// End one process by its pid, freeing what it holds open
    ///
    /// Sends the signal in the form the target's `kill` builtin takes (`-s <number>`; it
    /// rejects the `-9` shorthand), and wakes a stopped process first so its teardown
    /// completes. `pros ps` lists pids.
    Kill {
        /// Which process, by pid - `pros ps` lists them
        pid: String,
        #[command(flatten)]
        which: Which,
    },
    /// List the processes running on the target
    ///
    /// The same `ps` the window's system panel reads; the pids here are what `pros kill`
    /// takes.
    Ps {
        #[command(flatten)]
        which: Which,
    },
    /// Watch the running processes, redrawn on an interval until stopped
    ///
    /// The `ps` table re-read every few seconds. Ctrl-C stops it, or `--seconds` caps it.
    /// Read-only: to end something use `pros close` or `pros kill`.
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
    /// Closes the title if it is running, restores it from a local build into
    /// `/data/homebrew/<id>` (overwriting what is there), launches it, and streams its log until
    /// the title leaves the process list or `--seconds` elapses.
    ///
    /// A title that parks (idles rather than exits, the conforming ending for a big-app) never
    /// leaves the process list, so the watch ends at the `--seconds` cap. Closing is
    /// best-effort: a parked big-app ignores signals, and the launch reports a held slot.
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
        /// Called `payload` because `--name` already means which target.
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
    /// Stand in for the target's Porthole payload, so the Moonlight bridge can be tested with no
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
    // Held for the whole of `main`: the guard keeps the writers alive, and `let _` would drop
    // it here. `oops-log` prints `build` on its own startup line.
    let _logging = oops_log::Logging::new("pros")
        .build(pros_core::build::line_static())
        .init();
    let cli = Cli::parse();
    // Before the command, so a command that then hangs has its explanation above it.
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
    /// Declared, not discovered. The window keeps the matching table in `Section::requires`.
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
            // Probe also uses shsrv and klogsrv, but the restore is the step it cannot begin
            // without; the others report their own failures.
            | Self::Probe { .. }
            | Self::Titles { .. } => Some("ftpsrv"),
        }
    }
}

/// How long a service is given to answer before the command is warned about.
///
/// Short: a service that is up answers on a local network in microseconds, and a wrong
/// reading costs a warning, never a refusal.
const GLANCE: Duration = Duration::from_millis(600);

/// How long silence has to last before a shell command's answer is considered complete.
///
/// The shell sends no end marker, so quiet is the only signal. The window uses the same
/// value for the same commands.
const SETTLE: Duration = Duration::from_millis(1200);

/// Warns, before the command runs, when it needs a service the target is not offering.
///
/// It warns rather than refuses: one short connection attempt can be wrong (a firewall, a
/// service still starting), and a failed attempt with the reason on screen beats a refusal.
fn forewarn(command: &Command, name: Option<&str>) {
    let Some(service) = command.requires() else {
        return;
    };
    // With no target registered, the command itself reports that.
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
fn run(command: Command) -> CliResult {
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
/// An empty reply is reported, because a shell that is not loaded and a command that printed
/// nothing otherwise look identical.
fn sh(command: &str, name: Option<&str>) -> CliResult {
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
/// A quiet log is a result, not an error.
fn logs(seconds: u64, name: Option<&str>) -> CliResult {
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
/// The behaviour is `pros_moonlight`'s; this finds the LAN address and the apps to offer,
/// starts a PIN prompt on standard input, and hands over. It blocks until stopped.
fn moonlight(hostname: String, ip: Option<std::net::Ipv4Addr>) -> CliResult {
    let local_ip = ip
        .or_else(detect_lan_ip)
        .ok_or("could not work out this machine's LAN address; pass it with --ip")?;
    // One app per registered target; a placeholder if none is registered.
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
    // Porthole's 9805/9806: the first registered target, or the local fake target.
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

/// The LAN address the target and clients reach this machine on, from the local address the
/// OS routes outward with. A UDP connect sends no packet.
fn detect_lan_ip() -> Option<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) => Some(v4),
        std::net::IpAddr::V6(_) => None,
    }
}

/// Runs a stand-in target for testing the Moonlight bridge without hardware.
///
/// The behaviour is `pros_moonlight::fake`'s; this reads the clip, says what it is doing, and
/// hands over. It blocks until the process is stopped.
fn fake_target(clip: &Path, video_port: u16, input_port: u16) -> CliResult {
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
/// The shape guard is the library's: a vendor module and a payload share their first four
/// bytes, and the loader accepts either and dies silently on the one it cannot run.
fn send(path: &Path, seconds: u64, name: Option<&str>) -> CliResult {
    let target = pick(name)?;
    let payload = std::fs::read(path)?;

    // Checked here as well as in the library so "sending" is never printed for a send that
    // is then refused.
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
        // Only a payload launched this way reports on the socket, so silence is not failure.
        println!("not necessarily failure: only a payload launched this way reports here");
        println!("at all. If it writes a file, `pros pull` will get it");
    } else {
        print!("{out}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Fetches a file off the target.
///
/// Written by this program rather than a shell redirect, which can choose the encoding and
/// prepend a byte-order mark.
fn pull(path: &str, into: Option<PathBuf>, name: Option<&str>) -> CliResult {
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
/// Warns before an inert destination: a file under a system mount point is not indexed or
/// mounted, so it lands and does nothing.
fn push(from: &Path, to: &str, name: Option<&str>) -> CliResult {
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
fn verify(file: &Path, against: &str, manifest: &Path) -> CliResult {
    let manifest = Manifest::from_file(manifest)?;
    let payload = manifest
        .find(against)
        .ok_or_else(|| format!("the manifest describes no payload called {against:?}"))?;
    // Both refuse: a payload that cannot be checked is as unsendable as one that fails.
    let expected = payload.checksum()?;
    expected.verify(&std::fs::read(file)?)?;
    println!("{} is {against}, {expected}", file.display());
    Ok(ExitCode::SUCCESS)
}

/// Reports on a manifest, from here or from the target.
///
/// The payload manager on a configured target keeps its own repository description, in the
/// schema this project follows, so the target can be read instead of typed in again.
fn payloads(
    file: Option<&Path>,
    from_target: Option<&str>,
    check: bool,
    write: bool,
    save: bool,
    name: Option<&str>,
) -> CliResult {
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
        // Falls back to the built-in list rather than refusing, so a new install can still
        // see what a target ought to be running.
        (None, None) => read_or_recommend()?,
    };

    // Saving only on request: it rewrites a file somebody may have edited.
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

    // Probing only on request: it costs a connection attempt per service.
    let (report, chain) = if check {
        let target = pick(name)?;
        let report = pros_core::check(&target);
        // An unreadable boot list is `None`, which the survey reports as unknown rather than
        // absent; the failure is printed because the file service being down is a finding.
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
fn backup(from: &str, into: Option<PathBuf>, name: Option<&str>) -> CliResult {
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
    // Progress is printed so waiting is distinguishable from stuck. The cancel callback is
    // always false: on a command line, Ctrl-C stops the process.
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
/// Skipping unchanged files is the default; `--all` sends every file, for when the record of
/// what landed cannot be trusted. See `pros_core::deployed`.
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
) -> CliResult {
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
    let summary = deploy(&target, from, &target_dest, all)?;
    Ok(say::copied(&summary, &target_dest))
}

/// The restore both `restore` and `probe` do: [`pros_core::transfer::restore`], each file said as
/// it goes, and the record's note said if it would not write.
fn deploy(
    target: &Target,
    from: &Path,
    to: &str,
    all: bool,
) -> CliResult<pros_core::transfer::Summary> {
    let restored = pros_core::transfer::restore(
        target,
        from,
        to,
        resend(all),
        &mut |progress| {
            println!("  {}", progress.current);
        },
        &|| false,
    )?;
    // A cache: if it will not write, the only cost is a full re-send next time.
    if let Some(why) = restored.unrecorded {
        eprintln!("note: could not record what landed for next time: {why}");
    }
    Ok(restored.summary)
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

/// The commands that touch the registry and no target.
fn registry(what: &Registry) -> CliResult {
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
fn read_or_recommend() -> CliResult<Manifest> {
    let path = pros_core::manifest::Tracked::Payloads.path();
    if let Some(path) = path.filter(|path| path.exists()) {
        return Ok(Manifest::from_file(&path)?);
    }
    // Where a list came from decides how much to trust it.
    println!("no manifest of your own, so this is the built-in list");
    println!("read off a target's own repository - `pros payloads --write` to edit it");
    println!();
    Ok(pros_core::manifest::Tracked::Payloads.shipped())
}

/// Watches a probe's port and re-sends it when it stops answering.
///
/// It looks about once a second so it does not compete with the driver for the target. Every
/// restart and every change of state is printed, so two processes never read as one session.
///
/// # Errors
///
/// When the probe cannot be read, or no target is registered. A failed send is not an error:
/// it is one dead start, counted against the patience.
fn supervise(
    path: &Path,
    port: u16,
    patience: usize,
    restarts: usize,
    name: Option<&str>,
) -> CliResult {
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
            // Both directions: a driver lines its records up against each change.
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
                    // Not fatal: one dead start, and the patience decides whether to go on.
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
fn write_recommended() -> CliResult {
    let path = pros_core::manifest::default_path()
        .ok_or("no home directory, so there is nowhere for a manifest to live")?;
    // Refused rather than overwritten: the existing file may hold hand-checked digests.
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
fn library(path: &str, titles_only: bool, name: Option<&str>) -> CliResult {
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
/// `--fix` sends only what is missing, described, and already staged here verified. It does
/// not fetch and does not touch the boot list.
fn check(fix: bool, name: Option<&str>) -> CliResult {
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
            // Named, not skipped, so each service shows which outcome it had.
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
    // Checked again: a payload being sent does not mean it answers.
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
fn saves(name: Option<&str>) -> CliResult {
    let target = pick(name)?;
    let where_to = match pros_core::saves::find(&target.link())? {
        pros_core::saves::Found::Here(path) => path,
        // Listed rather than chosen between: each account's saves are its own.
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
        // Named from the installed title's description; an uninstalled title shows only its
        // identifier.
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
/// One round trip per title, because each name lives in the title's own description.
fn titles(appmeta: &str, name: Option<&str>) -> CliResult {
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
            // An unreadable description shows the identifier and the reason, not a blank name.
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
/// It sends the same line as `pros sh "launch <id>"`, with a check either side. The shell
/// splits on spaces with no quoting and passes every extra word to the application as an
/// argument, so the identifier is validated and refused, not trimmed. A refusal arrives as a
/// `perror` line on the same socket as any other reply, so the reply is read and sets the
/// exit code.
fn launch(id: &str, name: Option<&str>) -> CliResult {
    // A usage error goes to stderr; what the target says is a result and goes to stdout.
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
        // Asked, not started: the target has no reply meaning a title came up, so success
        // says only that it took the request.
        pros_core::launch::Said::Asked(_) => Ok(ExitCode::SUCCESS),
        // The target declining, by printing its usage or by naming the call that failed.
        pros_core::launch::Said::NotAnId | pros_core::launch::Said::Refused(_) => {
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Restarts the user interface to clear a softlock.
fn restart_ui(name: Option<&str>) -> CliResult {
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
fn close(id: &str, name: Option<&str>) -> CliResult {
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
    // Listed again: a title still listed did not close.
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
/// The primitive `close` uses, aimed by pid: the commands `pros_core::system::end` produces,
/// which wake a stopped process before killing it. A pid not in the listing is reported, not
/// signalled.
fn kill_pid(pid: &str, name: Option<&str>) -> CliResult {
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
    // Listed again, as in `close`: a pid still listed did not end.
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
/// The same `ps` the window's system panel reads, parsed the same way; its pids are what
/// `pros kill` takes.
fn ps(name: Option<&str>) -> CliResult {
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
/// The [`ps`] table re-read every `every` seconds until Ctrl-C or the `--seconds` cap.
/// Read-only, so it needs no key handling or raw mode. On a terminal it clears between draws;
/// piped, it prints successive tables. A failed read is reported and the watch goes on.
fn top(seconds: Option<u64>, every: u64, name: Option<&str>) -> CliResult {
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
                    // Clear and home, so each draw replaces the last. No raw mode, so nothing
                    // to restore on Ctrl-C.
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
/// A restored tree is on disk at once, but `ShadowMountPlus` has to mount and register it
/// before it appears in the appmeta list ([`pros_core::titles::APPMETA`]) and `launch` can
/// start it. So this polls that list, not the filesystem. Returns `true` once the id is
/// listed, `false` if the timeout elapses first.
fn wait_for_registration(link: &pros_link::Link, id: &str, timeout: Duration) -> bool {
    use std::io::Write as _;

    print!("waiting for {id} to register in the title list");
    let _ = std::io::stdout().flush();
    let deadline = std::time::Instant::now() + timeout;
    let listed = loop {
        // The id is the folder name under appmeta; a failed listing is retried until the
        // deadline.
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
/// Close, restore, launch and follow, each from `pros-core`, in sequence. A parked big-app
/// ignores the close, and a title that parks rather than exits ends the watch at the cap.
fn probe(id: &str, from: &Path, seconds: u64, all: bool, name: Option<&str>) -> CliResult {
    if !pros_core::launch::is_an_app_id(id) {
        eprintln!("not an application identifier: {id}");
        eprintln!("nine characters, four letters then five digits, no spaces");
        return Ok(ExitCode::FAILURE);
    }

    let target = pick(name)?;
    let link = target.link();
    let dest = pros_core::guard::homebrew_path(id);

    // Close it if it is running, best-effort: a parked big-app ignores every signal (measured),
    // and the launch below reports whether the slot is still held.
    match pros_core::probe::close(&link, id) {
        0 => println!("{id} is not running"),
        closed => println!("closed {id} ({closed} process(es))"),
    }

    // Restore the build into /data/homebrew/<id>, overwriting. An incomplete deploy is not
    // launched. Unchanged files are skipped unless `--all`.
    println!("restoring {} -> {dest}", from.display());
    let summary = deploy(&target, from, &dest, all)?;
    let restored = say::copied(&summary, &dest);
    if !summary.is_complete() {
        eprintln!("not launching {id} - the deploy did not land cleanly");
        return Ok(restored);
    }

    // The title must be registered before `launch` can resolve it.
    if !wait_for_registration(&link, id, Duration::from_secs(60)) {
        eprintln!(
            "{id} did not appear in the title list within 60s - not launching. It restored, but \
             the console has not registered it (ShadowMountPlus may not have mounted it)."
        );
        return Ok(ExitCode::FAILURE);
    }
    println!("{id} is registered.");

    // Follow the log before launching: a probe does its work in the first second or two and
    // then parks silently, so a follower attached after the launch misses it (measured). The
    // connection is the subscription; `SUBSCRIBE_SETTLE` makes sure it is live.
    println!("attaching to {id}'s log before launch...");
    let (stopper, lines) = pros_link::log::follow(&link)?;
    std::thread::sleep(pros_core::probe::SUBSCRIBE_SETTLE);

    let said = pros_core::probe::launch(&link, id)?;
    println!("launching {id}: {}", said.describe());
    if !matches!(said, pros_core::launch::Said::Asked(_)) {
        stopper.stop();
        eprintln!(
            "the launch was refused - if a parked title is holding the slot it cannot be closed \
             by signal (see `pros close`); the console's own dashboard Close ends it"
        );
        return Ok(ExitCode::FAILURE);
    }

    // Until the title parks, exits, or the cap elapses; shared with the window's probe screen.
    println!("following {id} (until it parks, exits, or {seconds}s)...");
    let mut any = false;
    let ending = pros_core::probe::follow(
        &std::sync::Arc::new(stopper),
        lines,
        &link,
        id,
        seconds,
        &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        &mut |line| {
            use std::io::Write as _;
            println!("{line}");
            let _ = std::io::stdout().flush();
            any = true;
        },
    );
    if !any {
        println!("the log was quiet while {id} ran - which is a result, not a failure");
    }
    println!("{}", ending.describe(id, seconds));
    Ok(ExitCode::SUCCESS)
}

/// Fetches payloads and keeps the ones that are what they claim to be.
fn fetch(wanted: Option<&str>, all: bool, from_target: bool, name: Option<&str>) -> CliResult {
    let manifest = if from_target {
        // The target's own repository carries urls and digests. (D013)
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
    println!("{} not: {}", refused.len(), refused.join(", "));
    Ok(ExitCode::FAILURE)
}

/// Keeps a payload ready to send, having checked it is the one described.
///
/// The check happens on the way in, so everything in the staging directory is known to be
/// what it claims; a file dropped there by hand is not.
fn stage(file: &Path, name: &str, manifest: Option<&Path>) -> CliResult {
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
fn read_manifest(named: Option<&Path>) -> CliResult<Manifest> {
    if let Some(path) = named {
        return Ok(Manifest::from_file(path)?);
    }
    let path = pros_core::manifest::default_path()
        .ok_or("no home directory, so there is nowhere for a manifest to live")?;
    // A missing manifest and an unreadable one are different problems, so different messages.
    if !path.exists() {
        return Err(format!("no manifest at {}", path.display()).into());
    }
    Ok(Manifest::from_file(&path)?)
}

/// The target to act on.
fn pick(name: Option<&str>) -> CliResult<Target> {
    Ok(target::resolve(target::load()?, name)?)
}
