//! What a target can currently do, and what to do about what it cannot.
//!
//! # Not up or down
//!
//! A reader told that 3232 is closed has been given a worse answer than one told that the
//! kernel log cannot be read. A check therefore reports **what each service unlocks**, and
//! separates the services without which there is no workflow from the ones whose absence
//! only costs visibility. Those two fail differently and call for different work.
//!
//! # The loader's failure has a different remedy from every other failure
//!
//! The payload manager launches everything through the loader - including, if asked, the
//! loader itself. So when the loader dies **nothing can bring anything back**, and the
//! dashboard that would have said so keeps answering, because it is a separate listener that
//! is already running. The only recovery is re-running the jailbreak.
//!
//! That is the one finding here that changes what a person does next, so it is not left for
//! a reader to work out from a table: [`Report::verdict`] says it.
//!
//! # The decisions are separate from the probing
//!
//! Everything that turns findings into a verdict is pure, and tested without a network. What
//! needs a target is one function that fills in the timings. A rule about what a missing
//! loader means should not be reachable only by switching a real target off.

use std::collections::BTreeMap;
use std::time::Duration;

use pros_link::service::{Reachability, SERVICES, Service};

use crate::manifest::Manifest;
use crate::target::Target;

/// How slow an answer has to be before it is worth remarking on.
///
/// A port that refuses instantly and one that takes a second and a half mean different
/// things - the first is a machine saying no, the second is usually a network deciding - and
/// a reader who cannot tell them apart blames the wrong thing.
pub const REMARKABLE: Duration = Duration::from_millis(400);

/// How long to wait for any one service before calling it absent.
const TIMEOUT: Duration = Duration::from_millis(1500);

/// What was found about one service.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Which service, and what it unlocks.
    pub service: Service,
    /// Whether it answered, and how quickly.
    pub reachability: Reachability,
}

impl Finding {
    /// Whether the answer took long enough to be worth mentioning.
    #[must_use]
    pub fn was_slow(&self) -> bool {
        self.reachability.took > REMARKABLE
    }
}

/// What to do about what is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remedy {
    /// The loader is gone, so nothing can be loaded - including the loader.
    ///
    /// **Its own variant rather than one missing service among others.** Everything else on
    /// this list is fixed by loading a payload, and this is the one that cannot be.
    RerunTheJailbreak,
    /// Something required is missing, and the loader can put it back.
    LoadThese {
        /// Which services, by name.
        names: Vec<String>,
    },
}

/// What a check concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Everything answered.
    Ready,
    /// A workflow will run, but something will be invisible if it goes wrong.
    Dimmed {
        /// Which optional services are absent.
        names: Vec<String>,
    },
    /// There is no workflow until something is done.
    Blocked {
        /// What that something is.
        remedy: Remedy,
    },
}

/// Everything a check found about one target.
#[derive(Debug, Clone)]
pub struct Report {
    /// Which target, as it is registered.
    pub name: String,
    /// Where it is.
    pub address: String,
    /// One per service, in the order they are checked - the loader first.
    pub findings: Vec<Finding>,
    /// The same declared services again, keyed by name, for looking one up.
    ///
    /// # This used to be the only place they went, and that was the bug
    ///
    /// It said that a declared port was *worth probing and not worth promoting to the same
    /// list*. The consequence was that [`Report::verdict`] never saw one - so a payload
    /// somebody had marked required could be down while the check reported *ready*. The
    /// program took the measurement and then ignored it, which is this project's own recurring
    /// defect committed against its own data.
    ///
    /// They are now in `findings` as well, which is what the verdict reads. This remains
    /// because the payload table looks up by name and a map is the right shape for that.
    ///
    /// Empty when no manifest was consulted, or when nothing answered at all - see
    /// [`check_declaring`].
    pub declared: BTreeMap<String, Reachability>,
}

impl Report {
    /// Builds a report from findings that have already been gathered.
    ///
    /// Public so the reasoning below can be tested without a target, and so a caller that
    /// probes differently - through a tunnel, or on other ports - can still use the verdict.
    #[must_use]
    pub fn new(name: &str, address: &str, findings: Vec<Finding>) -> Self {
        Self {
            name: name.to_owned(),
            address: address.to_owned(),
            findings,
            declared: BTreeMap::new(),
        }
    }

    /// Services that did not answer.
    #[must_use]
    pub fn missing(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|finding| !finding.reachability.open)
            .collect()
    }

    /// What was found about one service, by name.
    ///
    /// The one place anything asks *is this answering*, so that a panel needing a service
    /// and the check reporting on it cannot disagree.
    #[must_use]
    pub fn about(&self, service: &str) -> Option<&Finding> {
        self.findings
            .iter()
            .find(|finding| finding.service.name == service)
    }

    /// Whether the loader itself is gone.
    ///
    /// Answered by name rather than by position: the loader being first in the table is a
    /// presentation choice, and a rule that depended on it would break quietly the day
    /// somebody sorted the list.
    #[must_use]
    pub fn loader_is_down(&self) -> bool {
        self.findings.iter().any(|finding| {
            finding.service.name == pros_link::service::LOADER.name && !finding.reachability.open
        })
    }

    /// What all of this means.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        if self.loader_is_down() {
            return Verdict::Blocked {
                remedy: Remedy::RerunTheJailbreak,
            };
        }
        let required: Vec<String> = self
            .missing()
            .iter()
            .filter(|finding| finding.service.required)
            .map(|finding| finding.service.name.to_string())
            .collect();
        if !required.is_empty() {
            return Verdict::Blocked {
                remedy: Remedy::LoadThese { names: required },
            };
        }
        let optional: Vec<String> = self
            .missing()
            .iter()
            .map(|finding| finding.service.name.to_string())
            .collect();
        if optional.is_empty() {
            Verdict::Ready
        } else {
            Verdict::Dimmed { names: optional }
        }
    }

    /// Findings worth remarking on for their timing alone.
    #[must_use]
    pub fn slow(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|finding| finding.was_slow())
            .collect()
    }
}

/// Asks a target what it can currently do.
///
/// Every service, every time, because a cached answer is a claim that expires without notice.
#[must_use]
pub fn check(target: &Target) -> Report {
    check_with(target, TIMEOUT)
}

/// The same, with a timeout of the caller's choosing.
///
/// A target on the other side of a link somebody is tunnelling through needs longer than one
/// on the same network, and a caller that knows that should be able to say so.
#[must_use]
pub fn check_with(target: &Target, timeout: Duration) -> Report {
    let link = target.link();
    let findings = SERVICES
        .iter()
        .map(|service| {
            // **The registered port, not the compiled-in one**, and the finding carries the
            // port it actually used - a check that probed 2122 and reported 2121 would be a
            // report about a machine nobody has.
            let port = link.port(&service.name, service.port);
            Finding {
                service: Service {
                    port,
                    ..service.clone()
                },
                reachability: pros_link::probe(&target.address, port, timeout),
            }
        })
        .collect();
    Report::new(&target.name, &target.address, findings)
}

/// The same, and then whatever ports the manifest declared.
///
/// # Why a manifest gets to widen a check
///
/// Five services have ports this project measured. Everything else in a repository has no
/// port anything here knows, so its presence is unanswerable - which is honest, and is not
/// useful when a person is looking at twenty rows of *nothing here can tell*.
///
/// A manifest entry that declares a port makes itself answerable. That is the whole of it:
/// **the list is editable, so presence becomes something a person can extend without a
/// rebuild.** A wrong port reports one payload's state from another's socket, which is why
/// the field is a deliberate entry in a file rather than a number scraped out of a
/// description.
///
/// # Why a dead target is not probed twenty-five times
///
/// If nothing at all answered, the target is not there, and every further probe is a full
/// timeout spent learning what the first five already established. So the extra probes are
/// skipped and those entries read as unknown - which is exactly what they are.
///
/// Without that, a manifest growing would quietly turn an offline check from seconds into
/// minutes, and the waiting would look like the tool having hung.
#[must_use]
pub fn check_declaring(target: &Target, manifest: &Manifest, timeout: Duration) -> Report {
    let mut report = check_with(target, timeout);
    if report.findings.iter().all(|f| !f.reachability.open) {
        return report;
    }
    let known = |name: &str| SERVICES.iter().any(|s| s.name.eq_ignore_ascii_case(name));
    for payload in manifest.payloads() {
        if known(&payload.name) {
            continue;
        }
        let Some(mut service) = payload.as_service() else {
            continue;
        };
        // A declared service can be overridden per target too. Two ways of saying where
        // something is, and the registration is the more specific.
        service.port = target.link().port(&service.name, service.port);
        let reachability = pros_link::probe(&target.address, service.port, timeout);
        report.declared.insert(payload.name.clone(), reachability);
        // **Also a finding, which is what makes it count.** Kept in `declared` too, because
        // the payload table looks things up by name there; but a finding is what the verdict
        // reads, and a declared service that could not reach the verdict was a measurement
        // this program took and then ignored.
        report.findings.push(Finding {
            service,
            reachability,
        });
    }
    report
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pros_link::service::{LOADER, Reachability, SERVICES, Service};

    use super::{Finding, REMARKABLE, Remedy, Report, Verdict};

    fn found(service: Service, open: bool, took: Duration) -> Finding {
        Finding {
            service,
            reachability: Reachability { open, took },
        }
    }

    fn all(open: bool) -> Vec<Finding> {
        SERVICES
            .iter()
            .map(|service| found(service.clone(), open, Duration::from_millis(5)))
            .collect()
    }

    fn without(name: &str) -> Vec<Finding> {
        SERVICES
            .iter()
            .map(|service| {
                found(
                    service.clone(),
                    service.name != name,
                    Duration::from_millis(5),
                )
            })
            .collect()
    }

    #[test]
    fn everything_answering_is_ready() {
        let report = Report::new("ps5", "10.0.0.1", all(true));
        assert_eq!(report.verdict(), Verdict::Ready);
        assert!(report.missing().is_empty());
    }

    /// The finding that changes what a person does next.
    ///
    /// Not "the loader is one of three services that are down": nothing on this machine can
    /// put it back, and every other remedy on the list assumes it is there.
    #[test]
    fn a_missing_loader_says_rerun_the_jailbreak_rather_than_reload_a_payload() {
        let report = Report::new("ps5", "10.0.0.1", without(LOADER.name.as_ref()));
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::RerunTheJailbreak
            }
        );
    }

    /// Even when several things are down, the loader is the one that decides the remedy.
    #[test]
    fn the_loader_decides_the_remedy_when_several_are_down() {
        let report = Report::new("ps5", "10.0.0.1", all(false));
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::RerunTheJailbreak
            }
        );
    }

    /// A required service that is not the loader can be put back by the loader.
    #[test]
    fn a_required_service_that_is_not_the_loader_can_be_loaded_again() {
        let file_service = SERVICES
            .iter()
            .find(|service| service.required && service.name != LOADER.name)
            .expect("something required beside the loader");
        let report = Report::new("ps5", "10.0.0.1", without(file_service.name.as_ref()));
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::LoadThese {
                    names: vec![file_service.name.to_string()]
                }
            }
        );
    }

    /// An optional service missing is a different kind of important: the work can proceed,
    /// and less of it will be visible if it goes wrong.
    #[test]
    fn an_optional_service_missing_dims_rather_than_blocks() {
        let optional = SERVICES
            .iter()
            .find(|service| !service.required)
            .expect("something optional");
        let report = Report::new("ps5", "10.0.0.1", without(optional.name.as_ref()));
        assert_eq!(
            report.verdict(),
            Verdict::Dimmed {
                names: vec![optional.name.to_string()]
            }
        );
    }

    /// A slow answer is carried rather than rounded to up or down.
    #[test]
    fn a_slow_answer_is_surfaced() {
        let mut findings = all(true);
        if let Some(first) = findings.first_mut() {
            first.reachability.took = REMARKABLE + Duration::from_millis(1);
        }
        let report = Report::new("ps5", "10.0.0.1", findings);
        assert_eq!(report.slow().len(), 1);
        assert_eq!(report.verdict(), Verdict::Ready, "slow is not down");
    }
}

#[cfg(test)]
mod declared_tests {
    use std::time::Duration;

    use pros_link::service::{Reachability, SERVICES, Service};

    use super::{Finding, Remedy, Report, Verdict};

    fn all_present() -> Vec<Finding> {
        SERVICES
            .iter()
            .map(|service| Finding {
                service: service.clone(),
                reachability: Reachability {
                    open: true,
                    took: Duration::from_millis(5),
                },
            })
            .collect()
    }

    fn missing(service: Service) -> Finding {
        Finding {
            service,
            reachability: Reachability {
                open: false,
                took: Duration::from_millis(5),
            },
        }
    }

    /// **A declared payload marked required can block, which it could not before.**
    ///
    /// This is the whole point of the field. A declared port used to be probed and the answer
    /// kept in a map the verdict never read - so the program could measure that a required
    /// thing was down and still report *ready*, which is its own recurring defect committed
    /// against its own measurement.
    #[test]
    fn a_declared_required_service_blocks_when_it_is_missing() {
        let mut findings = all_present();
        findings.push(missing(Service::declared(
            "garlic-savemgr".to_owned(),
            8082,
            "decrypt and browse saves".to_owned(),
            true,
            false,
            false,
        )));
        let report = Report::new("ps5", "10.0.0.1", findings);
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::LoadThese {
                    names: vec!["garlic-savemgr".to_owned()]
                }
            }
        );
    }

    /// Declared and **not** required dims rather than blocks - the same distinction the five
    /// compiled-in services already draw, applied to one that came from a file.
    #[test]
    fn a_declared_optional_service_only_dims() {
        let mut findings = all_present();
        findings.push(missing(Service::declared(
            "websrv".to_owned(),
            8080,
            "browse the target over http".to_owned(),
            false,
            false,
            false,
        )));
        let report = Report::new("ps5", "10.0.0.1", findings);
        assert_eq!(
            report.verdict(),
            Verdict::Dimmed {
                names: vec!["websrv".to_owned()]
            }
        );
    }

    /// **A declared service says which kind it is**, because a wrong compiled-in port was
    /// measured and a wrong declared one is somebody's typing - and the second reports another
    /// listener's state under this name.
    #[test]
    fn a_declared_service_knows_it_was_declared() {
        let one = Service::declared("x".to_owned(), 1, "y".to_owned(), false, false, false);
        assert!(one.declared);
        assert!(SERVICES.iter().all(|service| !service.declared));
    }
}
