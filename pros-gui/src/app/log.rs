//! The log panel, and the filter shared with the probe screen.

use super::App;
use super::widgets::{choose_where_to_save, section_heading, section_heading_with};
use crate::state::Section;

/// How the log filter box is being read: plain text, or a regular expression.
///
/// Built once per frame from the box and its regex toggle, so the pattern compiles once rather
/// than per line.
pub(super) enum LogMatch {
    /// An empty box: every line is kept.
    All,
    /// Plain text, matched without regard to ASCII case.
    Text(String),
    /// A compiled regular expression.
    Regex(regex_lite::Regex),
    /// The box holds a regular expression that does not compile.
    ///
    /// Keeps every line, so a half-typed pattern shows the log unfiltered while the toolbar
    /// says the pattern is invalid, rather than blanking it on each keystroke.
    Invalid,
}

impl LogMatch {
    /// Reads the filter box into a matcher.
    pub(super) fn build(filter: &str, as_regex: bool) -> Self {
        let text = filter.trim();
        if text.is_empty() {
            return Self::All;
        }
        if as_regex {
            regex_lite::Regex::new(text).map_or(Self::Invalid, Self::Regex)
        } else {
            Self::Text(text.to_owned())
        }
    }

    /// Whether a line is kept by the current filter.
    fn keeps(&self, line: &str) -> bool {
        match self {
            Self::All | Self::Invalid => true,
            Self::Text(needle) => contains_ignore_ascii_case(line, needle),
            Self::Regex(regex) => regex.is_match(line),
        }
    }

    /// Whether the box holds a regex that will not compile.
    const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid)
    }
}

/// An ASCII-case-insensitive substring test that allocates nothing.
///
/// The filter runs over every kept line on each frame a line arrives, so it must not allocate
/// per line. Logs are ASCII, so folding only the ASCII range is enough.
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (haystack, needle) = (haystack.as_bytes(), needle.as_bytes());
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// The filter, copy and save controls a captured log carries - the log screen's, and the probe
/// screen's, which is the same view over a different capture.
///
/// Returns what the caller has to say: `Ok` for news, `Err` for trouble.
pub(super) fn filter_controls(
    ui: &mut egui::Ui,
    lines: &[String],
    filter: &mut String,
    regex: &mut bool,
    matcher: &LogMatch,
    save_as: &str,
) -> Option<Result<String, String>> {
    ui.label("filter");
    ui.add(
        egui::TextEdit::singleline(filter)
            .desired_width(160.0)
            .hint_text(if *regex {
                "regex to keep"
            } else {
                "text to keep"
            }),
    )
    .on_hover_text(
        "keep only the lines this matches - plain text ignoring case, or a regular expression \
         with the box ticked. What is hidden is still kept.",
    );
    ui.checkbox(regex, "regex")
        .on_hover_text("match the filter as a regular expression instead of plain text");
    // An invalid pattern is not applied; this says why every line is shown.
    if matcher.is_invalid() {
        ui.colored_label(egui::Color32::from_rgb(220, 170, 90), "invalid regex")
            .on_hover_text("the pattern does not compile yet, so every line is shown");
    }
    // Both numbers when filtered, so a filtered view does not read as a quiet target.
    let kept = lines.iter().filter(|line| matcher.keeps(line)).count();
    if filter.trim().is_empty() {
        ui.weak(format!("{} lines", lines.len()));
    } else {
        ui.weak(format!("{kept} of {} lines", lines.len()));
    }
    let shown = || {
        lines
            .iter()
            .filter(|line| matcher.keeps(line))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    };
    if ui
        .add_enabled(kept > 0, egui::Button::new("copy"))
        .on_hover_text("copy what is shown to the clipboard, filter and all")
        .on_disabled_hover_text("nothing to copy")
        .clicked()
    {
        let text = shown();
        ui.output_mut(|out| out.copied_text = text);
    }
    // Saves exactly what is shown, filter and all, like `copy`.
    if ui
        .add_enabled(kept > 0, egui::Button::new("save"))
        .on_hover_text("write what is shown to a .log file you choose, filter and all")
        .on_disabled_hover_text("nothing to save")
        .clicked()
        && let Some(path) = choose_where_to_save(save_as)
    {
        // A trailing newline, so a later append does not run onto the last line.
        let mut text = shown();
        text.push('\n');
        return Some(match std::fs::write(&path, text) {
            Ok(()) => Ok(format!("saved {kept} lines to {}", path.display())),
            Err(why) => Err(format!("could not save the log: {why}")),
        });
    }
    None
}

/// A captured log's lines, filtered, in a view that lays out only the rows on screen.
///
/// Virtualized like the log panel, and pinned to the bottom while `live`.
pub(super) fn filtered_rows(
    ui: &mut egui::Ui,
    salt: &str,
    lines: &[String],
    matcher: &LogMatch,
    live: bool,
) {
    let shown: Vec<&str> = lines
        .iter()
        .filter(|line| matcher.keeps(line))
        .map(String::as_str)
        .collect();
    if shown.is_empty() {
        if !lines.is_empty() {
            ui.weak("nothing matches that filter - the lines are still kept");
        }
        return;
    }
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .stick_to_bottom(live)
        .show_rows(ui, row_height, shown.len(), |ui, range| {
            for row in range {
                ui.monospace(shown[row]);
            }
        });
}

impl App {
    /// The target's log, virtualized so the buffer can be large without the view paying for it.
    ///
    /// [`egui::ScrollArea::show_rows`] lays out only the rows on screen, so a full buffer costs
    /// what a screenful costs; one text box over every line would be laid out in full every
    /// frame.
    pub(super) fn log_panel(&mut self, ui: &mut egui::Ui) {
        if self.state.target().is_none() {
            section_heading(ui, Section::Log);
            ui.label("no target selected");
            return;
        }

        let following = self.tail.is_some();
        // Built once for both the toolbar and the rows.
        let matcher = LogMatch::build(&self.state.log.filter, self.state.log.regex);
        self.log_toolbar(ui, following, &matcher);

        if self.state.log.lines.is_empty() {
            ui.weak(if following {
                "nothing yet - a quiet log is a fact about the target, not a fault"
            } else {
                "not following"
            });
            return;
        }
        filtered_rows(ui, "log", &self.state.log.lines, &matcher, following);
    }

    /// The log screen controls: following, filtering, copying, and where it is kept.
    fn log_toolbar(&mut self, ui: &mut egui::Ui, following: bool, matcher: &LogMatch) {
        // Re-read rather than passed in, so the closure below does not also borrow `self`.
        let known = self.state.target().cloned();
        section_heading_with(ui, Section::Log, |ui| {
            if following {
                if ui
                    .button("stop")
                    .on_hover_text("close the connection and stop following")
                    .clicked()
                {
                    self.tail = None;
                }
                ui.colored_label(egui::Color32::from_rgb(120, 190, 120), "following");
            } else {
                if ui
                    .button("follow")
                    .on_hover_text("open the log and show lines as they arrive")
                    .clicked()
                {
                    match known.as_ref().map_or_else(
                        || Err("no target selected".to_owned()),
                        |target| crate::tail::Tail::start(&target.name, &target.link()),
                    ) {
                        Ok(tail) => {
                            self.state.log.lines.clear();
                            self.tail = Some(tail);
                        }
                        Err(why) => self.state.trouble = Some(why),
                    }
                }
                ui.weak("not following");
            }
            if ui
                .add_enabled(!self.state.log.lines.is_empty(), egui::Button::new("clear"))
                .on_hover_text("forget what has been shown - the log keeps arriving")
                .clicked()
            {
                self.state.log.lines.clear();
            }
            // Said in words: an ended log and a quiet one look identical otherwise.
            if self.tail.as_ref().is_some_and(crate::tail::Tail::has_ended) {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 120, 120),
                    "the target closed the connection",
                );
            }
            // Where the log is kept, so it can be found afterwards.
            if let Some(target) = known.as_ref()
                && let Some(path) = crate::tail::kept_at(&target.name)
            {
                ui.weak("kept").on_hover_text(format!(
                    "{}
every line is appended here as it arrives, and the previous file is kept beside it",
                    path.display()
                ));
                if ui
                    .small_button("open folder")
                    .on_hover_text("show the kept logs in this machine file browser")
                    .clicked()
                    && let Some(at) = path.parent()
                {
                    self.reveal(at);
                }
            }
            // Shared with the probe screen.
            let suggested = known
                .as_ref()
                .map_or_else(|| "log".to_owned(), |target| format!("{}.log", target.name));
            match filter_controls(
                ui,
                &self.state.log.lines,
                &mut self.state.log.filter,
                &mut self.state.log.regex,
                matcher,
                &suggested,
            ) {
                Some(Ok(said)) => self.state.said = said,
                Some(Err(why)) => self.state.trouble = Some(why),
                None => {}
            }
        });
    }
}
