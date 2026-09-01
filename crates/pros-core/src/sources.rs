//! Asking a payload's own project what it has released.
//!
//! # The version this project could never see
//!
//! Three versions were already knowable for any payload: what the list describes, what is
//! staged on this machine, and what the target holds. There is a fourth, and until now nothing
//! here could see it - **what the project has actually released**.
//!
//! Without it the list is trusted absolutely. It is a static file: the one shipped here plus
//! whatever a console's payload manager has cached, both hand-maintained, both wrong the moment
//! somebody cuts a release. Measured on 2026-08-30: the mirror this project's own list named
//! for `elfldr` answered 404, and the list had been wrong for an unknown length of time with
//! nothing able to say so.
//!
//! # Why the answer is kept with the time it was asked
//!
//! **Never asked must not look like up to date.** That is the whole of it. An `Against` that
//! collapsed *the list matches the latest release* into *nobody has checked* would be this
//! project's recurring defect committed against the one column that exists to catch it - so
//! [`crate::sources::Against::NotChecked`] carries the reason, and every stored answer carries
//! the second it was given.
//!
//! # Why this is polite about it
//!
//! Sixty requests an hour, unauthenticated, per address. A payload list of thirty-four spends
//! half of that in one sweep, so a sweep on every launch would be rate-limited by lunchtime.
//! Three things keep it reasonable, and none of them is a guess:
//!
//! - answers are cached on disk and only re-asked when they are older than
//!   [`crate::sources::STALE`];
//! - asks are spaced by a fixed gap rather than fired at once - see
//!   [`crate::sources::between`];
//! - a refusal is retried with a widening gap, and when the reply says *when* the limit lifts,
//!   that is waited for rather than guessed at - up to a limit, after which it gives up and
//!   says when to come back.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::manifest::Payload;

/// How old a stored answer may be before a sweep asks again.
///
/// **Six hours, because a release is not an event this needs to catch quickly.** The cost of
/// being a few hours behind is nil; the cost of asking too often is a rate limit that makes
/// the whole feature unavailable to somebody who restarts the program twice.
pub const STALE: Duration = Duration::from_hours(6);

/// How long to wait between one ask and the next.
const BETWEEN: Duration = Duration::from_millis(400);

/// How many times one payload is retried after a refusal.
const RETRIES: usize = 3;

/// The longest this will wait for a rate limit to lift before giving up on the sweep.
///
/// A limit that lifts in forty seconds is worth waiting out. One that lifts in forty minutes is
/// not something to hold a thread for, so it is reported with the time instead.
const PATIENCE: Duration = Duration::from_secs(90);

/// How long any one request may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// One file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// What the file is called.
    pub name: String,
    /// Where to get it.
    pub url: String,
}

/// What a project's own releases said, and when it was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upstream {
    /// The tag of the latest release, when there was an answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// The files attached to it.
    ///
    /// **Kept with the tag rather than fetched again.** Both come out of one reply, and asking
    /// twice would spend two of sixty requests an hour to learn what one already said.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<Asset>,
    /// When this was asked, in seconds since the epoch.
    pub asked_at: u64,
    /// What happened, in a person's words - the reason when there is no tag.
    pub said: String,
}

impl Upstream {
    /// Which attached file is the payload, for a description that names one.
    ///
    /// # Why this can decline to answer
    ///
    /// A release can carry a debug build, a source archive, a checksum file and the payload,
    /// and picking wrong would rewrite a list entry to point at the wrong thing. So: an exact
    /// filename match, then the only loadable file if there is exactly one, and otherwise
    /// **nothing** - which is a question for a person rather than a guess.
    #[must_use]
    pub fn payload_asset(&self, wanted: Option<&str>) -> Option<&Asset> {
        if let Some(wanted) = wanted
            && let Some(exact) = self
                .assets
                .iter()
                .find(|one| one.name.eq_ignore_ascii_case(wanted))
        {
            return Some(exact);
        }
        let loadable: Vec<&Asset> = self
            .assets
            .iter()
            .filter(|one| {
                // Extension, not suffix: a release called `notes-for-the.elf-format.txt` is
                // not a payload, and a `.ELF` from a build on Windows is.
                std::path::Path::new(&one.name)
                    .extension()
                    .is_some_and(|it| {
                        it.eq_ignore_ascii_case("elf") || it.eq_ignore_ascii_case("bin")
                    })
            })
            .collect();
        match loadable.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }

    /// Whether this answer is old enough to ask again.
    #[must_use]
    pub fn is_stale(&self, now: u64, older_than: Duration) -> bool {
        now.saturating_sub(self.asked_at) >= older_than.as_secs()
    }
}

/// How the payload list stands against what the project has released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Against {
    /// The list describes the latest release.
    Current,
    /// The project has released something newer than the list describes.
    Behind {
        /// What the list says.
        listed: String,
        /// What the project's latest release is called.
        upstream: String,
    },
    /// They differ and cannot be ordered.
    ///
    /// **Said rather than called behind.** A tag is somebody's text; `1.6beta16` and `v1.6` are
    /// not two numbers, and deciding which is newer would be a guess in the one place this
    /// column exists to stop guessing.
    Different {
        /// What the list says.
        listed: String,
        /// What the project's latest release is called.
        upstream: String,
    },
    /// Nobody has asked, or the asking did not answer - and this is why.
    NotChecked(String),
}

impl Against {
    /// Whether this is a finding rather than an absence.
    #[must_use]
    pub const fn is_behind(&self) -> bool {
        matches!(self, Self::Behind { .. } | Self::Different { .. })
    }
}

/// How a payload's list entry stands against what was found upstream.
///
/// `None` for `found` is *nobody asked*, which is deliberately not the same as an ask that came
/// back empty - the second one carries what the project said.
#[must_use]
pub fn against(payload: &Payload, found: Option<&Upstream>) -> Against {
    let Some(found) = found else {
        return Against::NotChecked("nobody has asked this project yet".to_owned());
    };
    let Some(upstream) = found
        .latest
        .as_deref()
        .map(str::trim)
        .filter(|it| !it.is_empty())
    else {
        return Against::NotChecked(found.said.clone());
    };
    let Some(listed) = payload
        .version
        .as_deref()
        .map(str::trim)
        .filter(|it| !it.is_empty())
    else {
        return Against::NotChecked("this list states no version to compare".to_owned());
    };
    if listed.eq_ignore_ascii_case(upstream) {
        return Against::Current;
    }
    // The same rule the target column uses, in the same place, so two columns comparing version
    // strings cannot come to different conclusions about the same pair.
    if crate::payloads::is_older(listed, upstream) {
        return Against::Behind {
            listed: listed.to_owned(),
            upstream: upstream.to_owned(),
        };
    }
    Against::Different {
        listed: listed.to_owned(),
        upstream: upstream.to_owned(),
    }
}

/// The owner and repository a releases page belongs to.
///
/// **Only the shapes that are certain.** A source that is not plainly a repository address
/// returns `None` and is reported as unasked rather than guessed at - a wrong repository would
/// answer confidently about somebody else's software.
#[must_use]
pub fn owner_repo(source: &str) -> Option<(String, String)> {
    let rest = source
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .strip_prefix("github.com/")?;
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let owner = parts.next()?.to_owned();
    let repo = parts.next()?.trim_end_matches(".git").to_owned();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

/// Where a payload's releases live, from whichever field says so.
///
/// `source` first, because it is the page a person would open. The download address is a
/// fallback: a release asset URL carries the same owner and repository.
#[must_use]
pub fn repository_of(payload: &Payload) -> Option<(String, String)> {
    payload
        .source
        .as_deref()
        .and_then(owner_repo)
        .or_else(|| payload.source_direct.as_deref().and_then(owner_repo))
}

/// Why one ask did not produce a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAsked {
    /// Nothing in the description points at a repository this knows how to ask.
    NoRepository,
    /// The address answered, and said there are no releases.
    NoReleases,
    /// Too many requests, and this is when the limit lifts if it said.
    Limited {
        /// Seconds since the epoch, when the reply gave a time.
        until: Option<u64>,
    },
    /// Anything else - the downloader, the network, an unreadable reply.
    Failed(String),
}

impl std::fmt::Display for NotAsked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRepository => write!(f, "nothing here says which project released it"),
            Self::NoReleases => write!(f, "the project has published no releases"),
            Self::Limited { until: Some(when) } => write!(
                f,
                "too many requests - the limit lifts in about {} minutes",
                in_minutes(*when)
            ),
            Self::Limited { until: None } => write!(f, "too many requests, and no time was given"),
            Self::Failed(why) => write!(f, "{why}"),
        }
    }
}

/// Roughly how long until a moment, for a person.
fn in_minutes(when: u64) -> u64 {
    when.saturating_sub(now()).div_ceil(60)
}

/// Seconds since the epoch.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Asks one project for its latest release, retrying a refusal with a widening gap.
///
/// # Errors
///
/// [`NotAsked`], which distinguishes *there is nothing to ask* from *it refused* from *it
/// failed* - three states that call for three different things and would otherwise all be
/// drawn as a payload nobody could check.
pub fn ask(owner: &str, repo: &str) -> Result<(String, Vec<Asset>), NotAsked> {
    let mut waited = Duration::from_secs(1);
    let mut last = NotAsked::Failed("nothing was tried".to_owned());
    for attempt in 0..=RETRIES {
        match ask_once(owner, repo) {
            Ok(tag) => return Ok(tag),
            // Neither of these changes by being asked twice.
            Err(why @ (NotAsked::NoRepository | NotAsked::NoReleases)) => return Err(why),
            Err(NotAsked::Limited { until }) => {
                // **The reply says when, so that is what is waited for.** A guessed backoff
                // either wakes too early and spends another request on the same refusal, or
                // sleeps long past the moment the limit lifted.
                let gap = until.map_or(waited, |when| {
                    Duration::from_secs(when.saturating_sub(now()).saturating_add(1))
                });
                if gap > PATIENCE || attempt == RETRIES {
                    return Err(NotAsked::Limited { until });
                }
                std::thread::sleep(gap);
                last = NotAsked::Limited { until };
            }
            Err(why) => {
                if attempt == RETRIES {
                    return Err(why);
                }
                std::thread::sleep(waited);
                waited = waited.saturating_mul(2);
                last = why;
            }
        }
    }
    Err(last)
}

/// How long to leave between one project and the next in a sweep.
#[must_use]
pub const fn between() -> Duration {
    BETWEEN
}

/// One request, with no retrying.
fn ask_once(owner: &str, repo: &str) -> Result<(String, Vec<Asset>), NotAsked> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let finished = std::process::Command::new("curl")
        .args([
            "-sS",
            "-i",
            "--max-time",
            &TIMEOUT.as_secs().to_string(),
            "-H",
            "Accept: application/vnd.github+json",
            // Sent because the address requires one, and named plainly: a request that
            // disguises itself is a request somebody cannot account for in their own logs.
            "-H",
            "User-Agent: prosperous",
            &url,
        ])
        .output()
        .map_err(|why| NotAsked::Failed(format!("could not run curl: {why}")))?;
    if !finished.status.success() {
        return Err(NotAsked::Failed(format!(
            "curl failed: {}",
            String::from_utf8_lossy(&finished.stderr).trim()
        )));
    }
    let said = String::from_utf8_lossy(&finished.stdout);
    let (headers, body) = split(&said);
    match status(&headers) {
        Some(200) => tag_in(body)
            .ok_or_else(|| NotAsked::Failed("the reply had no tag_name in it".to_owned())),
        Some(404) => Err(NotAsked::NoReleases),
        // 403 is what the unauthenticated limit answers with; 429 is the documented one.
        Some(403 | 429) => Err(NotAsked::Limited {
            until: header(&headers, "x-ratelimit-reset").and_then(|it| it.parse().ok()),
        }),
        Some(code) => Err(NotAsked::Failed(format!("the address answered {code}"))),
        None => Err(NotAsked::Failed("the reply had no status line".to_owned())),
    }
}

/// Splits a raw reply into its last header block and its body.
///
/// The last block, because a reply that redirected carries more than one and only the final one
/// describes what actually answered.
fn split(said: &str) -> (String, &str) {
    let mut rest = said;
    let mut headers = String::new();
    loop {
        let Some(at) = rest.find("\r\n\r\n").or_else(|| rest.find("\n\n")) else {
            return (headers, rest);
        };
        let (block, after) = rest.split_at(at);
        block.clone_into(&mut headers);
        rest = after.trim_start_matches(['\r', '\n']);
        if !rest.starts_with("HTTP/") {
            return (headers, rest);
        }
    }
}

/// The status code from a header block.
fn status(headers: &str) -> Option<u16> {
    headers
        .lines()
        .find(|line| line.starts_with("HTTP/"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
}

/// One header's value, matched without regard to case.
fn header(headers: &str, name: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

/// The `tag_name` out of a release reply.
fn tag_in(body: &str) -> Option<(String, Vec<Asset>)> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = parsed.get("tag_name")?.as_str()?.trim();
    if tag.is_empty() {
        return None;
    }
    let assets = parsed
        .get("assets")
        .and_then(|it| it.as_array())
        .map(|all| {
            all.iter()
                .filter_map(|one| {
                    Some(Asset {
                        name: one.get("name")?.as_str()?.to_owned(),
                        url: one.get("browser_download_url")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some((tag.to_owned(), assets))
}

/// Rewrites a description to point at what the project has released now.
///
/// # Why this downloads before it writes anything
///
/// A list entry is `url` **and** `checksum`, and the second is what makes the first safe to
/// use: this program refuses to fetch a payload it cannot check when it arrives. A new version
/// has no digest anywhere - the project publishes a file, not a hash this can verify against -
/// so the only way to get one is to fetch the file and compute it.
///
/// **That is a trust step and it is the only one in this program.** The digest recorded proves
/// that what you download later is what was downloaded now; it proves nothing about what the
/// project published. Every manifest entry has one of these behind it somewhere, including the
/// ones shipped here - somebody trusted a download once. This makes that moment explicit and
/// puts it behind a button rather than hiding it in a file somebody edits by hand.
///
/// # Why it asks the project again rather than using what a sweep recorded
///
/// **A stored answer is for reading, not for acting on.** Two reasons, and the first has
/// already bitten:
///
/// - a cached answer from before this program recorded release files has *no files* in it, and
///   that is indistinguishable from a release which genuinely has none. The message somebody
///   got was `the v5.14.0 release has 0 files` about a release with twenty-one. Any field added
///   here in future would do the same thing again;
/// - a sweep is hours old by design. What is about to be written into a list should be what the
///   project says now, not what it said this morning.
///
/// One request, at the moment somebody presses a button, is the cheapest possible answer to
/// both - and the fresh reply is handed back so what is on screen catches up too.
///
/// # Errors
///
/// When nothing says which project released it, when the project cannot be asked, when there is
/// no attached file this can identify as the payload, when the download fails, or when what
/// arrives is empty.
pub fn relist(payload: &Payload) -> Result<(Payload, Upstream), String> {
    let (owner, repo) = repository_of(payload).ok_or_else(|| NotAsked::NoRepository.to_string())?;
    let (tag, assets) = ask(&owner, &repo).map_err(|why| why.to_string())?;
    let found = Upstream {
        latest: Some(tag.clone()),
        assets,
        asked_at: now(),
        said: format!("{owner}/{repo}"),
    };
    let asset = found
        .payload_asset(payload.filename.as_deref())
        .ok_or_else(|| {
            let names: Vec<&str> = found.assets.iter().map(|one| one.name.as_str()).collect();
            format!(
                "the {tag} release has {} files and none of them is plainly the payload - set \
                 this entry's filename to one of them by hand: {}",
                names.len(),
                if names.is_empty() {
                    "none at all".to_owned()
                } else {
                    names.join(", ")
                }
            )
        })?
        .clone();

    let into = std::env::temp_dir().join(format!("pros-relist-{}", asset.name));
    let (program, arguments) = crate::fetch::parts(&crate::fetch::configured(), &asset.url, &into)?;
    let finished = std::process::Command::new(&program)
        .args(&arguments)
        .output()
        .map_err(|why| format!("could not run {program}: {why}"))?;
    if !finished.status.success() {
        let _ = std::fs::remove_file(&into);
        return Err(format!(
            "{program} failed for {}: {}",
            asset.url,
            String::from_utf8_lossy(&finished.stderr).trim()
        ));
    }
    let bytes = std::fs::read(&into).map_err(|why| format!("{}: {why}", into.display()))?;
    let _ = std::fs::remove_file(&into);
    if bytes.is_empty() {
        return Err(format!("{} arrived empty", asset.url));
    }

    let digest = crate::checksum::Checksum::of(&bytes);
    let now = Payload {
        version: Some(tag),
        filename: Some(asset.name.clone()),
        url: Some(asset.url.clone()),
        checksum: Some(digest.to_string()),
        // **The address the file actually came from**, so the next update asks the same place.
        source_direct: Some(asset.url),
        ..payload.clone()
    };
    Ok((now, found))
}

/// Everything that has been asked, kept between runs.
///
/// **On disk beside the registry**, because the point of keeping it is to not ask again on the
/// next launch, and something held only in memory would ask every time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sources {
    /// Keyed by the payload's name as the list spells it.
    #[serde(default)]
    seen: BTreeMap<String, Upstream>,
}

impl Sources {
    /// What is known about one payload.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Upstream> {
        self.seen.get(name)
    }

    /// Records an answer.
    pub fn put(&mut self, name: &str, found: Upstream) {
        self.seen.insert(name.to_owned(), found);
    }

    /// How many answers are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// Whether nothing has been asked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// The payloads a sweep would ask about, given how old an answer may be.
    ///
    /// **A payload with no repository is not returned at all**, so a sweep does not spend a
    /// gap between requests on something it was never going to ask.
    #[must_use]
    pub fn due<'a>(&self, described: &'a [Payload], older_than: Duration) -> Vec<&'a Payload> {
        let now = now();
        described
            .iter()
            .filter(|payload| repository_of(payload).is_some())
            .filter(|payload| {
                self.get(&payload.name)
                    .is_none_or(|found| found.is_stale(now, older_than))
            })
            .collect()
    }

    /// When the oldest held answer was given, for a panel that says how fresh this is.
    #[must_use]
    pub fn oldest(&self) -> Option<u64> {
        self.seen.values().map(|found| found.asked_at).min()
    }
}

/// Where the answers are kept.
#[must_use]
pub fn path() -> Option<PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("sources.json");
    Some(path)
}

/// Reads what has been asked before.
///
/// **A file that cannot be read is an empty one**, not a failure: this is a cache of somebody
/// else's release numbers, and refusing to start over it would be absurd.
#[must_use]
pub fn load() -> Sources {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes what has been asked.
///
/// # Errors
///
/// When there is nowhere to write, or the write fails.
pub fn save(sources: &Sources) -> Result<PathBuf, String> {
    let path = path().ok_or_else(|| "no home directory, so there is nowhere for it".to_owned())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|why| why.to_string())?;
    }
    let text = serde_json::to_string_pretty(sources).map_err(|why| why.to_string())?;
    std::fs::write(&path, text).map_err(|why| why.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Against, Sources, Upstream, against, header, owner_repo, split, status, tag_in};
    use crate::manifest::Payload;

    fn listed(name: &str, version: Option<&str>, source: Option<&str>) -> Payload {
        Payload {
            name: name.to_owned(),
            version: version.map(ToOwned::to_owned),
            source: source.map(ToOwned::to_owned),
            ..Payload::default()
        }
    }

    fn answered(tag: Option<&str>, asked_at: u64) -> Upstream {
        Upstream {
            latest: tag.map(ToOwned::to_owned),
            assets: Vec::new(),
            asked_at,
            said: tag.map_or("nothing came back".to_owned(), |_| "asked".to_owned()),
        }
    }

    /// **Nobody asked is not up to date.** The whole reason this module keeps a timestamp.
    #[test]
    fn a_payload_nobody_asked_about_is_not_reported_as_current() {
        let payload = listed("elfldr", Some("v0.25"), None);
        let Against::NotChecked(why) = against(&payload, None) else {
            panic!("nothing was asked, so nothing is known");
        };
        assert!(why.contains("nobody has asked"), "{why}");
    }

    /// An ask that came back with nothing carries what happened, not a blank.
    #[test]
    fn an_ask_that_failed_reports_what_it_said() {
        let payload = listed("elfldr", Some("v0.25"), None);
        let mut empty = answered(None, 100);
        empty.said = "too many requests".to_owned();
        let Against::NotChecked(why) = against(&payload, Some(&empty)) else {
            panic!("there is no tag, so there is no comparison");
        };
        assert_eq!(why, "too many requests");
    }

    /// The list matching the latest release is the ordinary, quiet case.
    #[test]
    fn a_list_that_names_the_latest_release_is_current() {
        let payload = listed("elfldr", Some("v0.25"), None);
        assert_eq!(
            against(&payload, Some(&answered(Some("v0.25"), 100))),
            Against::Current
        );
    }

    /// **The case this exists for:** the project moved on and the list did not.
    #[test]
    fn a_project_that_has_released_something_newer_shows_the_list_as_behind() {
        let payload = listed("elfldr", Some("v0.25"), None);
        let Against::Behind { listed, upstream } =
            against(&payload, Some(&answered(Some("v0.26"), 100)))
        else {
            panic!("v0.26 is newer than v0.25");
        };
        assert_eq!(listed, "v0.25");
        assert_eq!(upstream, "v0.26");
    }

    /// Two tags that cannot be ordered are said to differ, never called behind.
    #[test]
    fn tags_that_cannot_be_ordered_are_called_different() {
        let payload = listed("ShadowMountPlus", Some("1.6beta16"), None);
        assert!(matches!(
            against(&payload, Some(&answered(Some("v1.7-rc1"), 100))),
            Against::Different { .. }
        ));
    }

    /// A repository address is taken from the shapes that are certain, and no others.
    #[test]
    fn only_a_plain_repository_address_is_recognised() {
        assert_eq!(
            owner_repo("https://github.com/ps5-payload-dev/elfldr/releases"),
            Some(("ps5-payload-dev".to_owned(), "elfldr".to_owned()))
        );
        assert_eq!(
            owner_repo("github.com/owner/repo"),
            Some(("owner".to_owned(), "repo".to_owned()))
        );
        assert_eq!(owner_repo("https://example.com/downloads/thing.elf"), None);
        assert_eq!(owner_repo("https://github.com/owner"), None);
        assert_eq!(owner_repo(""), None);
    }

    /// **A payload with no repository is never asked about**, so a sweep spends no time on it.
    #[test]
    fn a_payload_with_nowhere_to_ask_is_not_in_a_sweep() {
        let described = vec![
            listed(
                "elfldr",
                Some("v0.25"),
                Some("https://github.com/a/b/releases"),
            ),
            listed(
                "mystery",
                Some("1.0"),
                Some("https://example.com/downloads"),
            ),
        ];
        let sources = Sources::default();
        let due = sources.due(&described, Duration::from_secs(0));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "elfldr");
    }

    /// A fresh answer is left alone; a stale one is asked again.
    #[test]
    fn only_answers_older_than_the_window_are_asked_again() {
        let described = vec![listed(
            "elfldr",
            Some("v0.25"),
            Some("https://github.com/a/b/releases"),
        )];
        let mut sources = Sources::default();
        sources.put("elfldr", answered(Some("v0.25"), super::now()));
        assert!(
            sources.due(&described, Duration::from_hours(1)).is_empty(),
            "it was just asked"
        );
        assert_eq!(
            sources.due(&described, Duration::from_secs(0)).len(),
            1,
            "with no window at all, everything is due"
        );
    }

    /// The status line and headers are read out of a raw reply.
    #[test]
    fn a_reply_is_split_into_its_headers_and_its_body() {
        let said = "HTTP/2 200\r\nx-ratelimit-remaining: 58\r\n\r\n{\"tag_name\":\"v0.25\"}";
        let (headers, body) = split(said);
        assert_eq!(status(&headers), Some(200));
        assert_eq!(
            header(&headers, "X-RateLimit-Remaining").as_deref(),
            Some("58")
        );
        let (tag, assets) = tag_in(body).expect("a release with a tag");
        assert_eq!(tag, "v0.25");
        assert!(assets.is_empty(), "this reply carried no files");
    }

    /// **A redirected reply carries two header blocks**, and only the last one answered.
    #[test]
    fn only_the_final_header_block_is_read() {
        let said = "HTTP/2 301\r\nlocation: elsewhere\r\n\r\nHTTP/2 403\r\nx-ratelimit-reset: 900\r\n\r\n{}";
        let (headers, _) = split(said);
        assert_eq!(status(&headers), Some(403));
        assert_eq!(
            header(&headers, "x-ratelimit-reset").as_deref(),
            Some("900")
        );
    }

    /// **The payload is picked by name, or not at all.**
    ///
    /// A release carrying a debug build beside the real one is the case this exists for:
    /// guessing would repoint a list entry at the wrong file, and the list entry is what a
    /// digest is later checked against.
    #[test]
    fn an_exact_filename_wins_and_ambiguity_declines_to_answer() {
        let two = Upstream {
            latest: Some("v0.26".to_owned()),
            assets: vec![
                super::Asset {
                    name: "elfldr-ps5.elf".to_owned(),
                    url: "https://example/elfldr-ps5.elf".to_owned(),
                },
                super::Asset {
                    name: "elfldr-ps5.debug.elf".to_owned(),
                    url: "https://example/elfldr-ps5.debug.elf".to_owned(),
                },
            ],
            asked_at: 0,
            said: String::new(),
        };
        assert_eq!(
            two.payload_asset(Some("elfldr-ps5.elf"))
                .map(|one| one.name.as_str()),
            Some("elfldr-ps5.elf"),
            "the name the list already uses settles it"
        );
        assert!(
            two.payload_asset(None).is_none(),
            "two loadable files and no name is a question for a person"
        );
    }

    /// One loadable file among the noise is unambiguous, whatever it is called.
    #[test]
    fn the_only_loadable_file_is_the_payload() {
        let one = Upstream {
            latest: Some("v0.26".to_owned()),
            assets: vec![
                super::Asset {
                    name: "SHA256SUMS.txt".to_owned(),
                    url: "https://example/sums".to_owned(),
                },
                super::Asset {
                    name: "renamed-by-the-project.ELF".to_owned(),
                    url: "https://example/it.elf".to_owned(),
                },
            ],
            asked_at: 0,
            said: String::new(),
        };
        assert_eq!(
            one.payload_asset(Some("elfldr_v0.25.elf"))
                .map(|it| it.name.as_str()),
            Some("renamed-by-the-project.ELF"),
            "the case of an extension is not a fact about the file"
        );
    }

    /// **The real release that produced the bad message**, as its own case.
    ///
    /// `ps5upload` v5.14.0 attaches twenty-one files - installers for four operating systems,
    /// an Android package, a `latest.json`, six engine binaries with no extension at all - and
    /// exactly one payload. The name in the list does not match it, because the project renamed
    /// the file between releases, so the only thing that identifies it is being the one
    /// loadable file among twenty.
    #[test]
    fn one_payload_among_twenty_installers_is_still_found() {
        let names = [
            "latest.json",
            "PS5Upload-5.14.0-android.apk",
            "PS5Upload-5.14.0-linux-arm64.deb",
            "PS5Upload-5.14.0-linux-arm64.rpm",
            "PS5Upload-5.14.0-linux-arm64.zip",
            "PS5Upload-5.14.0-linux-x64.deb",
            "PS5Upload-5.14.0-linux-x64.rpm",
            "PS5Upload-5.14.0-linux-x64.zip",
            "PS5Upload-5.14.0-mac-arm64.dmg",
            "PS5Upload-5.14.0-mac-x64.dmg",
            "PS5Upload-5.14.0-win-arm64-setup.exe",
            "PS5Upload-5.14.0-win-arm64.zip",
            "PS5Upload-5.14.0-win-x64-setup.exe",
            "PS5Upload-5.14.0-win-x64.zip",
            "ps5upload-5.14.0.elf",
            "ps5upload-engine-5.14.0-linux-arm64",
            "ps5upload-engine-5.14.0-linux-x64",
            "ps5upload-engine-5.14.0-macos-arm64",
            "ps5upload-engine-5.14.0-macos-x64",
            "ps5upload-engine-5.14.0-windows-arm64.exe",
            "ps5upload-engine-5.14.0-windows-x64.exe",
        ];
        let release = Upstream {
            latest: Some("v5.14.0".to_owned()),
            assets: names
                .iter()
                .map(|name| super::Asset {
                    name: (*name).to_owned(),
                    url: format!("https://example/{name}"),
                })
                .collect(),
            asked_at: 0,
            said: String::new(),
        };
        assert_eq!(release.assets.len(), 21);
        // The list still names last year's file, which no longer exists in this release.
        let picked = release
            .payload_asset(Some("ps5upload_v5.4.19.elf"))
            .expect("the one loadable file is the payload");
        assert_eq!(picked.name, "ps5upload-5.14.0.elf");
    }

    /// **An answer recorded before this program knew about files is not a release with none.**
    ///
    /// It is the same shape and the opposite fact, which is why [`relist`] asks the project
    /// again instead of reading this - a cached entry from an older run said *0 files* about
    /// a release carrying twenty-one.
    #[test]
    fn an_answer_from_before_files_were_recorded_looks_empty() {
        let older: Upstream = serde_json::from_str(
            r#"{"latest":"v5.14.0","asked_at":1,"said":"phantomptr/ps5upload"}"#,
        )
        .expect("an older record still reads");
        assert_eq!(older.latest.as_deref(), Some("v5.14.0"));
        assert!(
            older.assets.is_empty(),
            "which is exactly the trap: it is not a release with no files"
        );
    }

    /// A release with nothing loadable attached has no payload to point at.
    #[test]
    fn a_release_with_no_loadable_file_offers_nothing() {
        let none = Upstream {
            latest: Some("v0.26".to_owned()),
            assets: vec![super::Asset {
                name: "notes.txt".to_owned(),
                url: "https://example/notes".to_owned(),
            }],
            asked_at: 0,
            said: String::new(),
        };
        assert!(none.payload_asset(None).is_none());
        assert!(none.payload_asset(Some("elfldr.elf")).is_none());
    }

    /// A reply that is not the expected shape produces no tag rather than a wrong one.
    #[test]
    fn a_reply_without_a_tag_yields_nothing() {
        assert_eq!(tag_in("{\"message\":\"Not Found\"}"), None);
        assert_eq!(tag_in("not json at all"), None);
        assert_eq!(tag_in("{\"tag_name\":\"\"}"), None);
    }
}
