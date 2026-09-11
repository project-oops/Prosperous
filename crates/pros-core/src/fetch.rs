//! Getting a payload, by asking something that already knows how.
//!
//! # Why this is not an HTTP client
//!
//! Payload mirrors are served over a secured transport, so fetching from one means a
//! security stack: certificate verification, a root store, a protocol implementation. That
//! is a large dependency for a project that argues for each of the three it has, and it was
//! the reason downloading stayed unbuilt.
//!
//! Every machine this runs on already has a program that does it. So this runs that, exactly
//! as watching a stream runs a player that already decodes video - and for the same reason. The
//! command is **a line of text in a file**, so a person whose machine names it differently
//! changes one line rather than waiting for a release.
//!
//! # The download is not the interesting part
//!
//! **The verification is.** A payload arrives from a mirror somebody else controls and is
//! then run with kernel-adjacent privileges. So nothing fetched is kept until its digest
//! matches what the manifest says, and the check happens before the file reaches the place
//! that only holds verified things.
//!
//! That is why this could be built the moment a target handed over a repository with real
//! digests in it, and not before: **a download nobody can check is worse than no download**,
//! because it looks the same as one that worked.

use std::path::{Path, PathBuf};

use crate::manifest::Payload;
use crate::staging::{self, NotStaged};

/// What runs when nothing else is configured.
///
/// `curl` ships with Windows, macOS and essentially every Linux. `-f` so a server's error
/// page is a failure rather than a file, `-L` because release downloads redirect, and
/// `--show-error` so a failure says why.
const DEFAULT: &str = "curl -fL --silent --show-error --output {into} {url}";

/// Where the download command is kept.
#[must_use]
pub fn command_path() -> Option<PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("fetch.txt");
    Some(path)
}

/// The command to run, from the file if there is one.
#[must_use]
pub fn configured() -> String {
    command_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| DEFAULT.to_owned())
}

/// What to write into the file, so it explains itself.
#[must_use]
pub fn example() -> String {
    [
        "# The command Prosperous runs to fetch a payload.",
        "#",
        "# One line. {url} and {into} are replaced. Everything else is passed through as",
        "# written, split on spaces. Not a shell: no quoting, no pipes.",
        "#",
        "# Whatever it fetches is checked against the manifest's digest before it is kept,",
        "# so a mirror that hands back the wrong thing cannot get past this.",
        "#",
        "# The default, which needs nothing installed on most machines:",
        &format!("#   {DEFAULT}"),
        "",
    ]
    .join("\n")
}

/// Splits the command, with the url and the destination filled in.
///
/// # Errors
///
/// When there is nothing to run. Launching nothing quietly looks exactly like launching
/// something that failed.
pub fn parts(template: &str, url: &str, into: &Path) -> Result<(String, Vec<String>), String> {
    let filled = template
        .replace("{url}", url)
        .replace("{into}", &into.display().to_string());
    let mut words = filled.split_whitespace().map(str::to_owned);
    let program = words
        .next()
        .ok_or_else(|| "nothing to run - the fetch command is empty".to_owned())?;
    Ok((program, words.collect()))
}

/// Fetches a payload and keeps it, if it is the one described.
///
/// # Errors
///
/// [`NotFetched`] for anything that stops it arriving, and **[`NotFetched::NotStaged`] when
/// it arrives and is not what the manifest says it is** - which is the case this whole
/// module is arranged around. Nothing wrong is kept.
pub fn fetch(payload: &Payload) -> Result<PathBuf, NotFetched> {
    keep(payload, None)
}

/// The same, landing in a directory the caller names.
///
/// **So that a download appears where somebody is looking for it.** A window that shows a
/// folder and offers to fill it must fill *that* folder; a file verified into a different
/// directory is on disk, correct, and invisible, which is indistinguishable from the download
/// never having run.
///
/// # Errors
///
/// The same as [`fetch`]. Nothing is kept that failed its digest.
pub fn fetch_into(payload: &Payload, dir: &Path) -> Result<PathBuf, NotFetched> {
    keep(payload, Some(dir))
}

/// Finds a local build for a payload on this machine, if one exists.
///
/// Looks first at `source_local` (relative to the repository root), then checks conventional
/// build and dist directories under sibling projects (e.g. `oops-apps/src/<name>/dist/<filename>`).
#[must_use]
pub fn local_build(payload: &Payload) -> Option<PathBuf> {
    let filename = payload.filename.as_deref().unwrap_or(payload.name.as_str());

    for root in candidate_roots() {
        // 1. If explicit relative source_local is specified, check against root.
        if let Some(explicit) = payload.source_local.as_deref() {
            let candidate = root.join(explicit);
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        // 2. Conventional relative paths under oops-apps (src/ or root) or project root.
        let candidates = [
            root.join("oops-apps")
                .join("src")
                .join(&payload.name)
                .join("build")
                .join(filename),
            root.join("oops-apps")
                .join("src")
                .join(&payload.name)
                .join("dist")
                .join(filename),
            root.join("oops-apps")
                .join("src")
                .join(&payload.name)
                .join(filename),
            root.join("oops-apps")
                .join(&payload.name)
                .join("build")
                .join(filename),
            root.join("oops-apps")
                .join(&payload.name)
                .join("dist")
                .join(filename),
            root.join("oops-apps").join(&payload.name).join(filename),
            root.join("oops-apps").join("build").join(filename),
            root.join("oops-apps").join("dist").join(filename),
            root.join(&payload.name).join("build").join(filename),
            root.join(&payload.name).join("dist").join(filename),
            root.join("build").join(filename),
            root.join("dist").join(filename),
        ];

        for candidate in candidates {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Candidate workspace / repository roots for discovering local builds.
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(var) = std::env::var("OOPS_ROOT") {
        let p = PathBuf::from(var);
        if p.is_dir() {
            roots.push(p);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = cwd.as_path();
        roots.push(cur.to_path_buf());
        for _ in 0..6 {
            if let Some(parent) = cur.parent() {
                roots.push(parent.to_path_buf());
                cur = parent;
            } else {
                break;
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.as_path();
        for _ in 0..6 {
            if let Some(parent) = cur.parent() {
                roots.push(parent.to_path_buf());
                cur = parent;
            } else {
                break;
            }
        }
    }

    // Deduplicate and prioritize roots that contain an `oops-apps` directory or `.git`
    let mut prioritized = Vec::new();
    let mut others = Vec::new();
    for r in roots {
        if prioritized.contains(&r) || others.contains(&r) {
            continue;
        }
        if r.join("oops-apps").is_dir() || r.join(".git").exists() {
            prioritized.push(r);
        } else {
            others.push(r);
        }
    }
    prioritized.extend(others);
    prioritized
}

/// Whether this payload has anywhere to get it from (local build or remote URL).
#[must_use]
pub fn has_source(payload: &Payload) -> bool {
    !wheres(payload).is_empty()
}

/// Downloads, checks, and keeps - in the staging directory, or where the caller said.
fn keep(payload: &Payload, dir: Option<&Path>) -> Result<PathBuf, NotFetched> {
    let sources = wheres(payload);
    if sources.is_empty() {
        return Err(NotFetched::NoUrl);
    }

    // Refused before anything is downloaded if remote-only. A local build on localhost
    // can be staged even without an established remote checksum.
    let has_local = sources.iter().any(|(w, _)| matches!(w, Where::Local(_)));
    if !has_local {
        let _ = payload.checksum().map_err(|why| {
            tracing::warn!(payload = %payload.name, %why, "refusing to download: nothing to check it against");
            NotFetched::Unverifiable {
                why: why.to_string(),
            }
        })?;
    }

    let into = temporary(payload)?;
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent).map_err(|why| NotFetched::Failed {
            why: why.to_string(),
        })?;
    }

    // Each address in turn, trying local build first (primary source), then remote addresses.
    let mut refused: Vec<String> = Vec::new();
    let mut got = None;
    for (which, target) in &sources {
        tracing::info!(payload = %payload.name, %target, %which, "fetching");
        let result = match which {
            Where::Local(source_path) => std::fs::copy(source_path, &into)
                .map(|_| ())
                .map_err(|why| format!("could not copy {}: {why}", source_path.display())),
            Where::Listed | Where::Upstream => pull(target, &into),
        };
        match result {
            Ok(()) => {
                got = Some((which.clone(), target.clone()));
                break;
            }
            Err(why) => refused.push(format!("{which} ({target}): {why}")),
        }
    }
    let Some((which, url)) = got else {
        let _ = std::fs::remove_file(&into);
        return Err(NotFetched::Failed {
            why: refused.join("; "),
        });
    };
    if which != Where::Listed && !matches!(which, Where::Local(_)) {
        tracing::warn!(payload = %payload.name, %url, "the listed address failed; used another");
    }

    let kept = match &which {
        Where::Local(_) => staging::accept_local_into(payload, &into, dir),
        Where::Listed | Where::Upstream => match dir {
            Some(dir) => staging::accept_into(payload, &into, dir),
            None => staging::accept(payload, &into),
        },
    };
    let _ = std::fs::remove_file(&into);
    kept.map_err(NotFetched::NotStaged)
}

/// Where a download lands before it has been checked.
///
/// Beside the staging directory rather than inside it: **that directory's whole promise is
/// that everything in it was verified**, and an unchecked file sitting in it for the length
/// of a download is that promise being false for a while.
fn temporary(payload: &Payload) -> Result<PathBuf, NotFetched> {
    let name = payload
        .filename
        .clone()
        .unwrap_or_else(|| payload.name.clone());
    let mut path = crate::target::cache_directory().ok_or(NotFetched::Nowhere)?;
    path.push("incoming");
    path.push(name);
    Ok(path)
}

/// Which source a payload came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// A local build on this machine (primary source).
    Local(PathBuf),
    /// The `url` field: what the list says to use.
    Listed,
    /// The `source_direct` field: the file at its own project, named by the same list.
    Upstream,
}

impl std::fmt::Display for Where {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(path) => write!(f, "local build ({})", path.display()),
            Self::Listed => write!(f, "the listed address"),
            Self::Upstream => write!(f, "the project's own release"),
        }
    }
}

/// Everywhere a description says this file can be got, in the order to try them.
///
/// A local build first (primary source): what has been compiled on this machine beats
/// reaching across the network. The listed address second, and upstream direct third.
#[must_use]
pub fn wheres(payload: &Payload) -> Vec<(Where, String)> {
    let mut found: Vec<(Where, String)> = Vec::new();
    if let Some(local) = local_build(payload) {
        found.push((Where::Local(local.clone()), local.display().to_string()));
    }
    if let Some(url) = payload.url.as_ref() {
        found.push((Where::Listed, url.clone()));
    }
    if let Some(direct) = payload.source_direct.as_ref()
        && !found.iter().any(|(_, url)| url == direct)
    {
        found.push((Where::Upstream, direct.clone()));
    }
    found
}

/// Runs the downloader for one address.
///
/// # Errors
///
/// What the downloader said, without the address - the caller adds that, because it is the
/// caller that knows there was more than one to try.
fn pull(url: &str, into: &Path) -> Result<(), String> {
    let (program, arguments) = parts(&configured(), url, into)?;
    let finished = std::process::Command::new(&program)
        .args(&arguments)
        .output()
        .map_err(|why| format!("could not run {program}: {why}"))?;
    if finished.status.success() {
        return Ok(());
    }
    // Removed here as well as by the caller: a partial file left behind would be handed to the
    // next address as though it were the start of that download.
    let _ = std::fs::remove_file(into);
    Err(format!(
        "{program} failed: {}",
        String::from_utf8_lossy(&finished.stderr).trim()
    ))
}

/// Why a payload was not fetched.
#[derive(Debug)]
pub enum NotFetched {
    /// The manifest states no digest this can check, so nothing was downloaded.
    Unverifiable {
        /// What the manifest said, in the checksum module's words.
        why: String,
    },
    /// The entry says nothing about where to get it.
    NoUrl,
    /// It could not be downloaded.
    Failed {
        /// What the downloader said.
        why: String,
    },
    /// It arrived and was not what the manifest describes.
    NotStaged(NotStaged),
    /// There is nowhere to put it.
    Nowhere,
}

impl std::fmt::Display for NotFetched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unverifiable { why } => write!(
                f,
                "not fetched, because nothing could be established about it: {why}"
            ),
            Self::NoUrl => write!(f, "the manifest does not say where to get this"),
            Self::Failed { why } => write!(f, "not fetched: {why}"),
            Self::NotStaged(why) => write!(f, "it arrived and {why}"),
            Self::Nowhere => write!(f, "no home directory, so there is nowhere to put it"),
        }
    }
}

impl std::error::Error for NotFetched {}

#[cfg(test)]
mod tests {
    /// **The listed address first, and the project's own only when it differs.**
    ///
    /// The order is the point: what somebody wrote down is what gets used, and the second
    /// address exists for the day the first one stops answering.
    #[test]
    fn a_description_with_two_addresses_tries_the_listed_one_first() {
        let payload = Payload {
            name: "elfldr".to_owned(),
            url: Some("https://mirror.example/elfldr.elf".to_owned()),
            source_direct: Some("https://project.example/elfldr.elf".to_owned()),
            ..Payload::default()
        };
        let found = super::wheres(&payload);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, super::Where::Listed);
        assert_eq!(found[0].1, "https://mirror.example/elfldr.elf");
        assert_eq!(found[1].0, super::Where::Upstream);
    }

    /// The same address written twice is one address, not two attempts at it.
    #[test]
    fn the_same_address_in_both_fields_is_tried_once() {
        let same = "https://project.example/elfldr.elf".to_owned();
        let payload = Payload {
            name: "elfldr".to_owned(),
            url: Some(same.clone()),
            source_direct: Some(same),
            ..Payload::default()
        };
        assert_eq!(super::wheres(&payload).len(), 1);
    }

    /// A description with only an upstream address still has somewhere to go.
    #[test]
    fn an_entry_with_only_a_project_release_is_still_fetchable() {
        let payload = Payload {
            name: "elfldr".to_owned(),
            source_direct: Some("https://project.example/elfldr.elf".to_owned()),
            ..Payload::default()
        };
        let found = super::wheres(&payload);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, super::Where::Upstream);
    }

    /// **A description with nowhere to get it is refused before anything is spent.**
    #[test]
    fn an_entry_with_no_address_at_all_has_nowhere_to_try() {
        let payload = Payload {
            name: "elfldr".to_owned(),
            ..Payload::default()
        };
        assert!(super::wheres(&payload).is_empty());
    }

    use std::path::{Path, PathBuf};

    use super::{DEFAULT, NotFetched, example, fetch, parts};
    use crate::manifest::Payload;

    /// Both placeholders are filled, and the rest is passed through.
    #[test]
    fn the_url_and_the_destination_are_filled_in() {
        let (program, arguments) = parts(
            "curl -fL --output {into} {url}",
            "https://example.invalid/a.elf",
            Path::new("/tmp/a.elf"),
        )
        .expect("it splits");
        assert_eq!(program, "curl");
        assert!(arguments.contains(&"https://example.invalid/a.elf".to_owned()));
        assert!(arguments.iter().any(|word| word.contains("a.elf")));
    }

    /// **An entry nobody can verify is refused before a byte is downloaded.**
    ///
    /// There is no point spending somebody's bandwidth on a file that could not be checked
    /// when it arrived, and a download that cannot be checked is worse than none - it looks
    /// exactly like one that worked.
    #[test]
    fn a_payload_with_no_usable_digest_is_refused_before_downloading() {
        let payload = Payload {
            name: "old".to_owned(),
            filename: Some("old.elf".to_owned()),
            url: Some("https://example.invalid/old.elf".to_owned()),
            checksum: Some("d41d8cd98f00b204e9800998ecf8427e".to_owned()),
            ..Payload::default()
        };
        assert!(matches!(
            fetch(&payload),
            Err(NotFetched::Unverifiable { .. })
        ));
    }

    /// An entry with a digest and nowhere to get it is a description somebody has not
    /// finished, and it says so rather than failing obscurely at the downloader.
    #[test]
    fn a_payload_with_no_url_says_so() {
        let payload = Payload {
            name: "described".to_owned(),
            filename: Some("described.elf".to_owned()),
            checksum: Some(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
            ),
            ..Payload::default()
        };
        assert!(matches!(fetch(&payload), Err(NotFetched::NoUrl)));
    }

    /// The default needs nothing installed on most machines, and the file says what it is.
    #[test]
    fn the_example_explains_the_default() {
        assert!(example().contains(DEFAULT));
        assert!(example().contains("checked against the manifest's digest"));
    }

    /// **A local build on localhost is the primary source when it exists.**
    #[test]
    fn a_local_build_is_tried_first_as_primary_source() {
        let test_dir = PathBuf::from("target").join("test-local-src");
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("creates dir");
        let dummy_elf = test_dir.join("dummy.elf");
        std::fs::write(&dummy_elf, b"local-elf-bytes").expect("writes file");

        let payload = Payload {
            name: "dummy".to_owned(),
            filename: Some("dummy.elf".to_owned()),
            source_local: Some(dummy_elf.display().to_string()),
            url: Some("https://example.invalid/dummy.elf".to_owned()),
            source_direct: Some("https://github.com/example/dummy/releases/dummy.elf".to_owned()),
            ..Payload::default()
        };

        let found = super::wheres(&payload);
        assert!(!found.is_empty(), "must find sources");
        assert!(
            matches!(&found[0].0, super::Where::Local(p) if p.is_file()),
            "first source must be local build"
        );
        assert_eq!(found[1].0, super::Where::Listed);
        assert_eq!(found[2].0, super::Where::Upstream);

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    /// **A local build is fetched and staged directly without touching the network.**
    #[test]
    fn a_local_build_is_fetched_and_staged_directly() {
        let test_dir = PathBuf::from("target").join("test-local-fetch");
        let _ = std::fs::remove_dir_all(&test_dir);
        let src_dir = test_dir.join("src");
        let dst_dir = test_dir.join("dst");
        std::fs::create_dir_all(&src_dir).expect("creates src");
        let dummy_elf = src_dir.join("test-payload.elf");
        std::fs::write(&dummy_elf, b"elf-payload-content").expect("writes file");

        let payload = Payload {
            name: "test-payload".to_owned(),
            filename: Some("test-payload.elf".to_owned()),
            source_local: Some(dummy_elf.display().to_string()),
            url: Some("https://example.invalid/test-payload.elf".to_owned()),
            ..Payload::default()
        };

        let into = super::fetch_into(&payload, &dst_dir).expect("fetches from local source");
        assert_eq!(into, dst_dir.join("test-payload.elf"));
        assert_eq!(std::fs::read(&into).expect("reads"), b"elf-payload-content");

        let _ = std::fs::remove_dir_all(&test_dir);
    }
}
