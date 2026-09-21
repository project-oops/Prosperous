//! Validating a destination path before staging a title to a target.
//!
//! There is exactly one thing this catches: a title staged into an **inert system path** such as
//! `/user/app`, where the upload succeeds on the wire and is then silently ignored - never
//! scanned or mounted by `ShadowMountPlus` or `ShellCore`, so it never appears on the home
//! screen. For that one case it offers the location the console does scan instead:
//! `/data/homebrew/<TITLE_ID>`, **under the title's own id**.
//!
//! # What it deliberately does not do
//!
//! It does **not** rewrite a title id, and it does not judge a prefix. A homebrew title carries
//! whatever id it was built with - `MESA…`, `GLCB…`, a `FAKE…`, anything that is not a Sony
//! prefix - and the homebrew folder is exactly where such an id belongs. So a destination under
//! `/data/homebrew` is always accepted as written.
//!
//! It used to do more, and the more was wrong: it treated any non-`PPSA`/`CUSA`/`FAKE` prefix as
//! a defect and *rewrote* the id to `PPSA<suffix>`, so `MESA00001` staged to the homebrew folder
//! became `PPSA00001` - a Sony id the caller never asked for, which then landed on top of an
//! unrelated title that already had it. Where a title goes is the caller's decision; this only
//! keeps it out of a folder the console ignores, and it never changes what the title is called.
//! (D030)

use std::path::{Path, PathBuf};

/// The standard homebrew installation directory scanned by `ShadowMountPlus`.
pub const CANONICAL_HOMEBREW_ROOT: &str = "/data/homebrew";

/// An inert system path where direct uploads are silently ignored.
pub const INERT_USER_APP: &str = "/user/app";

/// Classification of a transfer target defect.
///
/// One kind, because there is one defect this catches - a destination the console ignores. It
/// stays an enum rather than a bool so a [`Refusal`] reads as *what kind of problem*, and so a
/// second **measured** defect could be added later without changing the shape callers match on.
/// It is not the place for a reasoned-about one: the prefix rule that used to live here was
/// exactly that, and it did harm (see the module note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// Target path is under an inert system directory (`/user/app`).
    InertPath,
}

/// Metadata extracted from a local title directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedTitle {
    /// The title ID found in `param.json` or derived from the directory name.
    pub title_id: Option<String>,
    /// Whether `eboot.bin` was present.
    pub has_eboot: bool,
    /// Whether `param.json` was present.
    pub has_param_json: bool,
}

/// A diagnosed destination issue with a suggested remedy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Local source path.
    pub from: PathBuf,
    /// Requested remote target path.
    pub target_path: String,
    /// The title ID, exactly as detected - **never rewritten**.
    pub title_id: String,
    /// The id the suggested path uses. **Always the same as `title_id`** - kept as its own field
    /// so callers reading it need not change, and named so it is plain that nothing here alters
    /// an identity.
    pub suggested_id: String,
    /// Suggested destination under `/data/homebrew`, using the title's own id.
    pub suggested_path: String,
    /// The category of issue.
    pub kind: IssueKind,
    /// Explanation of why this transfer will fail to appear on the console.
    pub explanation: String,
    /// Actionable advice for the user.
    pub remedy: String,
}

/// Where a homebrew title with this id lives on the target: `/data/homebrew/<id>`.
///
/// The one place `ShadowMountPlus` scans, composed with the title's own id and nothing else - so a
/// caller with a title id need not spell the path, and the spelling lives with the root it is
/// built from rather than in each caller. The id is used verbatim; this validates nothing, which
/// is [`check`]'s job.
#[must_use]
pub fn homebrew_path(id: &str) -> String {
    format!("{CANONICAL_HOMEBREW_ROOT}/{id}")
}

/// Whether a destination path points into an inert system directory.
#[must_use]
pub fn is_inert_target_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    trimmed == INERT_USER_APP || trimmed.starts_with("/user/app/")
}

/// Extracts a title ID from a simple JSON slice without a full serde dependency.
#[must_use]
pub fn parse_title_id_from_json(json: &str) -> Option<String> {
    let key_pos = json.find("\"titleId\"")?;
    let after_key = &json[key_pos + 9..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let string_content = &after_colon[1..];
    let quote_pos = string_content.find('"')?;
    let id = string_content[..quote_pos].trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Inspects a local directory for title markers and metadata.
#[must_use]
pub fn detect_title(dir: &Path) -> Option<DetectedTitle> {
    if !dir.is_dir() {
        return None;
    }
    let eboot = dir.join("eboot.bin");
    let has_eboot = eboot.is_file();

    let param_json_path = dir.join("sce_sys").join("param.json");
    let mut title_id = None;
    let mut has_param_json = false;

    if param_json_path.is_file() {
        has_param_json = true;
        if let Ok(content) = std::fs::read_to_string(&param_json_path) {
            title_id = parse_title_id_from_json(&content);
        }
    }

    if !has_eboot && !has_param_json {
        return None;
    }

    Some(DetectedTitle {
        title_id,
        has_eboot,
        has_param_json,
    })
}

/// The title's own id, taken from the first place that has one and **kept verbatim**: the
/// metadata, then the destination's own last path component, then the source directory's name.
///
/// A nine-character component is taken as an id; `app` (the tail of `/user/app`) is not, so the
/// inert path itself is never mistaken for a title. Empty when nothing names one.
fn title_id_of(detected: Option<&DetectedTitle>, target_last: &str, from_dir: &str) -> String {
    if let Some(id) = detected.and_then(|d| d.title_id.clone()) {
        return id;
    }
    if target_last.len() == 9 && !target_last.eq_ignore_ascii_case("app") {
        return target_last.to_owned();
    }
    if from_dir.len() == 9 {
        return from_dir.to_owned();
    }
    String::new()
}

/// Evaluates a proposed transfer for an inert destination.
///
/// Returns `Some(Refusal)` **only** when `to` points into an inert system path the console
/// ignores; the refusal's suggested path is the same title, under its own id, in
/// `/data/homebrew`. Every other destination - `/data/homebrew/<anything>` included - returns
/// `None`. This never second-guesses where a caller puts a title beyond keeping it out of a
/// folder that eats it, and it never changes a title id. See the module note and D030.
#[must_use]
pub fn check(from: &Path, to: &str) -> Option<Refusal> {
    let normalized_to = to.replace('\\', "/");
    let trimmed_to = normalized_to.trim_end_matches('/');

    // The one defect. A destination that is not an inert system path - the homebrew folder above
    // all - is the caller's to choose, whatever the title's prefix.
    if !is_inert_target_path(trimmed_to) {
        return None;
    }

    let detected = detect_title(from);
    let from_dir_name = from
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let target_last_component = trimmed_to.rsplit('/').next().unwrap_or_default();
    let title_id = title_id_of(detected.as_ref(), target_last_component, &from_dir_name);

    let suggested_path = if title_id.is_empty() {
        CANONICAL_HOMEBREW_ROOT.to_string()
    } else {
        format!("{CANONICAL_HOMEBREW_ROOT}/{title_id}")
    };

    let explanation = format!(
        "'{trimmed_to}' is an internal system mount point. Files staged here are ignored by the console scanner and will not appear on the home screen."
    );
    let remedy = if title_id.is_empty() {
        format!(
            "Stage it into '{CANONICAL_HOMEBREW_ROOT}/<TITLE_ID>' where ShadowMountPlus can discover and mount it."
        )
    } else {
        format!(
            "Stage it into '{suggested_path}', where ShadowMountPlus can discover and mount it - under its own id, unchanged."
        )
    };

    Some(Refusal {
        from: from.to_path_buf(),
        target_path: trimmed_to.to_string(),
        title_id: title_id.clone(),
        suggested_id: title_id,
        suggested_path,
        kind: IssueKind::InertPath,
        explanation,
        remedy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all};
    use std::io::Write;

    #[test]
    fn a_homebrew_title_path_is_the_scan_root_and_the_id() {
        assert_eq!(homebrew_path("GLPB00001"), "/data/homebrew/GLPB00001");
        assert_eq!(homebrew_path("MESA00001"), "/data/homebrew/MESA00001");
    }

    #[test]
    fn inert_path_detection() {
        assert!(is_inert_target_path("/user/app"));
        assert!(is_inert_target_path("/user/app/"));
        assert!(is_inert_target_path("/user/app/GLCB00001"));
        assert!(is_inert_target_path("/user/app/PPSA90001/eboot.bin"));
        assert!(!is_inert_target_path("/data/homebrew"));
        assert!(!is_inert_target_path("/data/homebrew/PPSA90001"));
        assert!(!is_inert_target_path("/system/app"));
    }

    #[test]
    fn json_title_id_parsing() {
        let json = r#"{"titleId":"PPSA90001","titleName":"Home Shell"}"#;
        assert_eq!(
            parse_title_id_from_json(json),
            Some("PPSA90001".to_string())
        );

        let formatted = "{\n  \"titleId\": \"GLCB00002\",\n  \"category\": \"big-app\"\n}";
        assert_eq!(
            parse_title_id_from_json(formatted),
            Some("GLCB00002".to_string())
        );

        assert_eq!(parse_title_id_from_json("{}"), None);
    }

    /// **The homebrew folder is accepted whatever the prefix, and the id is left alone.** This is
    /// the reported bug: `MESA00001` bound for `/data/homebrew` was rewritten to `PPSA00001` - a
    /// Sony id nobody asked for, which then clobbered an unrelated title. A non-Sony prefix in the
    /// homebrew folder is not a defect; it is the whole point of the homebrew folder.
    #[test]
    fn a_non_sony_prefix_in_the_homebrew_folder_is_left_alone() {
        let temp = std::env::temp_dir().join("pros_guard_homebrew_ok");
        let _ = std::fs::remove_dir_all(&temp);
        create_dir_all(temp.join("sce_sys")).unwrap();
        File::create(temp.join("eboot.bin")).unwrap();
        writeln!(
            File::create(temp.join("sce_sys").join("param.json")).unwrap(),
            "{{\"titleId\":\"MESA00001\"}}"
        )
        .unwrap();

        assert!(
            check(&temp, "/data/homebrew/MESA00001").is_none(),
            "a homebrew title in the homebrew folder must be accepted as written"
        );
        assert!(check(&temp, "/data/homebrew/GLCB00002").is_none());
        assert!(check(&temp, "/data/homebrew/PPSA90001").is_none());

        let _ = std::fs::remove_dir_all(&temp);
    }

    /// **An inert path is redirected to the homebrew folder under the title's OWN id.** The one
    /// thing the guard catches - a destination the console ignores - and the suggestion keeps the
    /// id exactly, never swapping the prefix for `PPSA`.
    #[test]
    fn an_inert_path_redirect_keeps_the_title_id() {
        let temp = std::env::temp_dir().join("pros_guard_inert");
        let _ = std::fs::remove_dir_all(&temp);
        create_dir_all(temp.join("sce_sys")).unwrap();
        File::create(temp.join("eboot.bin")).unwrap();
        writeln!(
            File::create(temp.join("sce_sys").join("param.json")).unwrap(),
            "{{\"titleId\":\"GLCB00001\"}}"
        )
        .unwrap();

        let res = check(&temp, "/user/app/GLCB00001").expect("an inert path is refused");
        assert_eq!(res.kind, IssueKind::InertPath);
        assert_eq!(res.title_id, "GLCB00001");
        assert_eq!(res.suggested_id, "GLCB00001", "the id is never rewritten");
        assert_eq!(res.suggested_path, "/data/homebrew/GLCB00001");

        // A Sony-prefix title staged inert is redirected the same way, keeping its id.
        writeln!(
            File::create(temp.join("sce_sys").join("param.json")).unwrap(),
            "{{\"titleId\":\"PPSA90001\"}}"
        )
        .unwrap();
        let res2 = check(&temp, "/user/app/PPSA90001").expect("still inert");
        assert_eq!(res2.suggested_path, "/data/homebrew/PPSA90001");

        let _ = std::fs::remove_dir_all(&temp);
    }
}
