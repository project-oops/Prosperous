//! What a target can do now, and what to do about what it cannot.
//!
//! A check reports what each service unlocks, and separates required services (no workflow
//! without them) from optional ones (their absence costs visibility).
//!
//! The payload manager launches everything through the loader, so a dead loader cannot be
//! reloaded; the only recovery is re-running the entry point, and [`Report::verdict`] says
//! so. Turning findings into a verdict is pure and tested without a network; only the probe
//! functions need a target.

use std::collections::BTreeMap;
use std::time::Duration;

use pros_link::service::{Reachability, SERVICES, Service};

use crate::manifest::Manifest;
use crate::target::Target;

/// How slow an answer has to be before it is worth remarking on.
///
/// An instant refusal is the target saying no; a slow one is usually the network.
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
    /// The loader is gone, so nothing can be loaded, including the loader. Every other missing
    /// service is fixed by loading a payload.
    RerunTheEntryPoint,
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

/// One sentence saying what the check concluded and what to do, shared by both programs.
impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let were = |count: usize| if count == 1 { "is" } else { "are" };
        match self {
            Self::Ready => f.write_str("ready"),
            Self::Dimmed { names } => write!(
                f,
                "usable, but {} {} not loaded, so something will be invisible if a run goes wrong",
                names.join(" and "),
                were(names.len())
            ),
            Self::Blocked {
                remedy: Remedy::RerunTheEntryPoint,
            } => f.write_str(
                "the loader is not answering, so nothing can be sent or started from here. \
                 This says nothing about the target: a console can run its whole chain with \
                 9021 unreachable. Getting it back means starting elfldr the way it was first \
                 started, which means re-running the entry point",
            ),
            Self::Blocked {
                remedy: Remedy::LoadThese { names },
            } => write!(
                f,
                "blocked: {} {} not loaded. The loader is up, so {} can be sent again",
                names.join(" and "),
                were(names.len()),
                if names.len() == 1 { "it" } else { "they" }
            ),
        }
    }
}

/// Everything a check found about one target.
#[derive(Debug, Clone)]
pub struct Report {
    /// Which target, as it is registered.
    pub name: String,
    /// Where it is.
    pub address: String,
    /// One per service, in the order they are checked, the loader first. Includes declared
    /// services, so the verdict sees them.
    pub findings: Vec<Finding>,
    /// The declared services again, keyed by name for the payload table.
    ///
    /// Empty when no manifest was consulted, or when nothing answered at all; see
    /// [`check_declaring`].
    pub declared: BTreeMap<String, Reachability>,
}

impl Report {
    /// Builds a report from findings that have already been gathered.
    ///
    /// Public so the verdict can be tested without a target and used by a caller that probes
    /// differently.
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
    /// The one place that answers whether a service is up, so a panel and the check cannot
    /// disagree.
    #[must_use]
    pub fn about(&self, service: &str) -> Option<&Finding> {
        self.findings
            .iter()
            .find(|finding| finding.service.name == service)
    }

    /// Whether the loader itself is gone.
    ///
    /// Answered by name, not position: the loader being first is a presentation choice.
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
                remedy: Remedy::RerunTheEntryPoint,
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
/// Every service, every time: a cached answer expires without notice.
#[must_use]
pub fn check(target: &Target) -> Report {
    check_with(target, TIMEOUT)
}

/// The same, with a timeout of the caller's choosing.
///
/// A target behind a tunnel needs longer than one on the same network.
#[must_use]
pub fn check_with(target: &Target, timeout: Duration) -> Report {
    let link = target.link();
    let findings = SERVICES
        .iter()
        .map(|service| {
            // The registered port, and the finding carries the port actually probed.
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
/// A manifest entry that declares a port makes its presence checkable without a rebuild. The
/// port is a deliberate field, never scraped from a description, because a wrong one reports
/// another listener's state.
///
/// When none of the compiled-in services answered, the target is not there and the declared
/// probes are skipped (they read as unknown), so an offline check does not spend one timeout
/// per manifest entry.
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
        // The target's registration overrides the declared port.
        service.port = target.link().port(&service.name, service.port);
        let reachability = pros_link::probe(&target.address, service.port, timeout);
        report.declared.insert(payload.name.clone(), reachability);
        // Also a finding, because the verdict reads only `findings`.
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

    /// Every service answering is ready.
    #[test]
    fn everything_answering_is_ready() {
        let report = Report::new("prospero", "10.0.0.1", all(true));
        assert_eq!(report.verdict(), Verdict::Ready);
        assert!(report.missing().is_empty());
    }

    /// A missing loader asks for the entry point to be re-run, not a payload reloaded.
    #[test]
    fn a_missing_loader_says_rerun_the_entry_point_rather_than_reload_a_payload() {
        let report = Report::new("prospero", "10.0.0.1", without(LOADER.name.as_ref()));
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::RerunTheEntryPoint
            }
        );
    }

    /// When several services are down, the loader decides the remedy.
    #[test]
    fn the_loader_decides_the_remedy_when_several_are_down() {
        let report = Report::new("prospero", "10.0.0.1", all(false));
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::RerunTheEntryPoint
            }
        );
    }

    /// The loader's remedy names the entry point and does not diagnose the target.
    #[test]
    fn the_loader_remedy_says_what_to_do_and_no_more() {
        let said = Verdict::Blocked {
            remedy: Remedy::RerunTheEntryPoint,
        }
        .to_string();
        assert!(said.contains("re-running the entry point"), "{said}");
        assert!(said.contains("says nothing about the target"), "{said}");
        assert!(!said.contains("can be sent again"), "{said}");
    }

    /// Two absent services read as two, not as one.
    #[test]
    fn a_pair_of_missing_services_agrees_with_itself() {
        let said = Verdict::Dimmed {
            names: vec!["klogsrv".to_owned(), "pldmgr".to_owned()],
        }
        .to_string();
        assert!(said.contains("klogsrv and pldmgr are not loaded"), "{said}");
    }

    /// A required service that is not the loader can be put back by the loader.
    #[test]
    fn a_required_service_that_is_not_the_loader_can_be_loaded_again() {
        let file_service = SERVICES
            .iter()
            .find(|service| service.required && service.name != LOADER.name)
            .expect("something required beside the loader");
        let report = Report::new("prospero", "10.0.0.1", without(file_service.name.as_ref()));
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::LoadThese {
                    names: vec![file_service.name.to_string()]
                }
            }
        );
    }

    /// A missing optional service dims rather than blocks.
    #[test]
    fn an_optional_service_missing_dims_rather_than_blocks() {
        let optional = SERVICES
            .iter()
            .find(|service| !service.required)
            .expect("something optional");
        let report = Report::new("prospero", "10.0.0.1", without(optional.name.as_ref()));
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
        let report = Report::new("prospero", "10.0.0.1", findings);
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

    /// A missing declared service marked required blocks.
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
        let report = Report::new("prospero", "10.0.0.1", findings);
        assert_eq!(
            report.verdict(),
            Verdict::Blocked {
                remedy: Remedy::LoadThese {
                    names: vec!["garlic-savemgr".to_owned()]
                }
            }
        );
    }

    /// A missing declared optional service dims rather than blocks.
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
        let report = Report::new("prospero", "10.0.0.1", findings);
        assert_eq!(
            report.verdict(),
            Verdict::Dimmed {
                names: vec!["websrv".to_owned()]
            }
        );
    }

    /// A declared service is marked as declared, and no compiled-in one is.
    #[test]
    fn a_declared_service_knows_it_was_declared() {
        let one = Service::declared("x".to_owned(), 1, "y".to_owned(), false, false, false);
        assert!(one.declared);
        assert!(SERVICES.iter().all(|service| !service.declared));
    }
}
