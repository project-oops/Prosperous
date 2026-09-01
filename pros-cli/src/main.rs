//! `pros` - one instrument for talking to a prepared target.
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
        #[arg(long, default_value = "ps5")]
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
            | Self::Verify { .. } => None,
            Self::Logs { .. } => Some("klogsrv"),
            // Both are one line typed at the shell. Starting a title is not a payload
            // and does not want the loader.
            Self::Sh { .. } | Self::Launch { .. } => Some("shsrv"),
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
        Command::Logs { seconds, which } => {
            let target = pick(which.name.as_deref())?;
            println!("listening to {} for {seconds}s", target.address);
            let text = pros_link::log::read(&target.link(), Duration::from_secs(seconds))?;
            if text.trim().is_empty() {
                // A quiet log is a fact about the target, not a failure of this program.
                println!("the log was quiet - which is a result, not a failure");
            } else {
                print!("{text}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Sh { command, which } => {
            let target = pick(which.name.as_deref())?;
            let out = pros_link::shell::run(&target.link(), &command, SETTLE)?;
            if out.trim().is_empty() {
                println!("no output - is the shell loaded? `pros check` will say");
            } else {
                print!("{out}");
            }
            Ok(ExitCode::SUCCESS)
        }
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
        Command::Push { from, to, which } => {
            let target = pick(which.name.as_deref())?;
            let bytes = std::fs::read(&from)?;
            pros_link::files::store(&target.link(), &to, &bytes)?;
            println!("{} bytes {} -> {to}", bytes.len(), from.display());
            Ok(ExitCode::SUCCESS)
        }
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
        Command::Restore { from, to, which } => restore(&from, &to, which.name.as_deref()),
        Command::Saves { which } => saves(which.name.as_deref()),
        Command::Titles { appmeta, which } => titles(&appmeta, which.name.as_deref()),
        Command::Launch { id, which } => launch(&id, which.name.as_deref()),
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
    }
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

/// Puts a folder back onto the target.
fn restore(
    from: &Path,
    to: &str,
    name: Option<&str>,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let target = pick(name)?;
    let mut session = pros_link::files::Session::open(&target.link())?;
    let summary = pros_core::transfer::upload(
        &mut session,
        from,
        to,
        &mut |progress| {
            println!("  {}", progress.current);
        },
        &|| false,
    );
    session.close();
    Ok(say::copied(&summary?, to))
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
    let path = pros_core::manifest::default_path();
    if let Some(path) = path.filter(|path| path.exists()) {
        return Ok(Manifest::from_file(&path)?);
    }
    // Said out loud, because where a list came from decides how much to trust it.
    println!("no manifest of your own, so this is the built-in list");
    println!("read off a target's own repository - `pros payloads --write` to edit it");
    println!();
    Ok(pros_core::manifest::recommended())
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
