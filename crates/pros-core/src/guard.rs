//! Validating destination paths and title identifiers before staging to a target.
//!
//! A title staged into an inert system path (such as `/user/app`) is never scanned or mounted
//! by `ShadowMountPlus` or `ShellCore`, leaving the user with an upload that succeeds on wire
//! and never appears on the home screen.
//!
//! Similarly, titles using non-standard prefixes (e.g. `GLCB`, `PROH`) are ignored by
//! `ShadowMountPlus`'s `is_supported_title_id` filter (which strictly requires `PPSA`, `CUSA`,
//! or `FAKE`).
//!
//! This module checks local source directories and target destinations before transfer,
//! identifying inert locations and incompatible identifiers and offering corrected paths.

use std::path::{Path, PathBuf};

/// Supported prefixes recognized by the console's automounter and home screen.
pub const SUPPORTED_PREFIXES: &[&str] = &["PPSA", "CUSA", "FAKE"];

/// The standard homebrew installation directory scanned by `ShadowMountPlus`.
pub const CANONICAL_HOMEBREW_ROOT: &str = "/data/homebrew";

/// An inert system path where direct uploads are silently ignored.
pub const INERT_USER_APP: &str = "/user/app";

/// Classification of a transfer target defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// Target path is under an inert system directory (`/user/app`).
    InertPath,
    /// Title ID prefix is not recognized by the console scanner (`GLCB`, `PROH`, etc.).
    IncompatiblePrefix,
    /// Both an inert destination path and an incompatible title prefix.
    Both,
}

/// Metadata extracted from a local title directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedTitle {
    /// The title ID found in `param.json` or derived from directory name.
    pub title_id: Option<String>,
    /// Whether `eboot.bin` was present.
    pub has_eboot: bool,
    /// Whether `param.json` was present.
    pub has_param_json: bool,
}

/// A diagnosed destination issue with suggested remedy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Local source path.
    pub from: PathBuf,
    /// Requested remote target path.
    pub target_path: String,
    /// Identified or requested title ID.
    pub title_id: String,
    /// Corrected Title ID conforming to console expectations.
    pub suggested_id: String,
    /// Suggested destination path under `/data/homebrew`.
    pub suggested_path: String,
    /// The category of issue.
    pub kind: IssueKind,
    /// Explanation of why this transfer will fail to appear on the console.
    pub explanation: String,
    /// Actionable advice for the user.
    pub remedy: String,
}

/// Whether a title ID prefix is supported by console automounters.
#[must_use]
pub fn is_supported_prefix(prefix: &str) -> bool {
    let upper = prefix.to_ascii_uppercase();
    SUPPORTED_PREFIXES.iter().any(|&p| p == upper)
}

/// Whether a destination path points into an inert system directory.
#[must_use]
pub fn is_inert_target_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    trimmed == INERT_USER_APP || trimmed.starts_with("/user/app/")
}

/// Extracts title ID from a simple JSON slice without full serde dependency.
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

/// Normalizes or maps a legacy/custom title prefix to `PPSA`.
#[must_use]
pub fn sanitize_title_id(raw_id: &str) -> String {
    let trimmed = raw_id.trim();
    if trimmed.len() == 9 {
        let prefix = &trimmed[0..4];
        let suffix = &trimmed[4..9];
        if is_supported_prefix(prefix) {
            return trimmed.to_string();
        }
        if suffix.chars().all(|c| c.is_ascii_digit()) {
            return format!("PPSA{suffix}");
        }
    }
    trimmed.to_string()
}

/// Evaluates a proposed transfer for inert paths and unsupported prefixes.
///
/// Returns `Some(Refusal)` if the transfer would fail to be mounted or shown by the console,
/// along with the suggested corrected destination path.
#[must_use]
pub fn check(from: &Path, to: &str) -> Option<Refusal> {
    let normalized_to = to.replace('\\', "/");
    let trimmed_to = normalized_to.trim_end_matches('/');

    let inert = is_inert_target_path(trimmed_to);

    let detected = detect_title(from);
    let from_dir_name = from
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let target_last_component = trimmed_to
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();

    // Determine the active title ID: from metadata, destination directory, or local directory name
    let detected_id = detected
        .as_ref()
        .and_then(|d| d.title_id.clone());

    let raw_title_id = if let Some(ref id) = detected_id {
        id.clone()
    } else if target_last_component.len() == 9 && !target_last_component.eq_ignore_ascii_case("app") {
        target_last_component.clone()
    } else if from_dir_name.len() == 9 {
        from_dir_name.clone()
    } else {
        String::new()
    };

    let prefix_unsupported = if raw_title_id.len() >= 4 {
        !is_supported_prefix(&raw_title_id[0..4])
    } else {
        false
    };

    if !inert && !prefix_unsupported {
        return None;
    }

    let kind = match (inert, prefix_unsupported) {
        (true, true) => IssueKind::Both,
        (true, false) => IssueKind::InertPath,
        (false, true) => IssueKind::IncompatiblePrefix,
        (false, false) => unreachable!(),
    };

    let suggested_id = if raw_title_id.is_empty() {
        "PPSA90001".to_string()
    } else {
        sanitize_title_id(&raw_title_id)
    };

    let suggested_path = format!("{CANONICAL_HOMEBREW_ROOT}/{suggested_id}");

    let (explanation, remedy) = match kind {
        IssueKind::InertPath => (
            format!(
                "'{trimmed_to}' is an internal system mount point. Files staged here are ignored by the console scanner and will not appear on the home screen."
            ),
            format!(
                "Stage titles into '{CANONICAL_HOMEBREW_ROOT}/<TITLE_ID>' where ShadowMountPlus can discover and mount them."
            ),
        ),
        IssueKind::IncompatiblePrefix => (
            format!(
                "Title ID '{raw_title_id}' has prefix '{}', which is not recognized by ShadowMountPlus (requires PPSA, CUSA, or FAKE).",
                if raw_title_id.len() >= 4 { &raw_title_id[0..4] } else { "unknown" }
            ),
            format!(
                "Use standard prefix '{suggested_id}' so the console indexes the title."
            ),
        ),
        IssueKind::Both => (
            format!(
                "Destination '{trimmed_to}' is an inert mount point, and Title ID '{raw_title_id}' uses an unsupported prefix."
            ),
            format!(
                "Install to '{suggested_path}' using the conforming prefix."
            ),
        ),
    };

    Some(Refusal {
        from: from.to_path_buf(),
        target_path: trimmed_to.to_string(),
        title_id: raw_title_id,
        suggested_id,
        suggested_path,
        kind,
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
    fn prefix_validation() {
        assert!(is_supported_prefix("PPSA"));
        assert!(is_supported_prefix("ppsa"));
        assert!(is_supported_prefix("CUSA"));
        assert!(is_supported_prefix("cusa"));
        assert!(is_supported_prefix("FAKE"));
        assert!(!is_supported_prefix("GLCB"));
        assert!(!is_supported_prefix("PROH"));
        assert!(!is_supported_prefix(""));
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
        assert_eq!(parse_title_id_from_json(json), Some("PPSA90001".to_string()));

        let formatted = "{\n  \"titleId\": \"GLCB00002\",\n  \"category\": \"big-app\"\n}";
        assert_eq!(parse_title_id_from_json(formatted), Some("GLCB00002".to_string()));

        assert_eq!(parse_title_id_from_json("{}"), None);
    }

    #[test]
    fn sanitize_converts_custom_prefix() {
        assert_eq!(sanitize_title_id("GLCB00001"), "PPSA00001");
        assert_eq!(sanitize_title_id("PROH00001"), "PPSA00001");
        assert_eq!(sanitize_title_id("PPSA90001"), "PPSA90001");
        assert_eq!(sanitize_title_id("CUSA00001"), "CUSA00001");
    }

    #[test]
    fn check_detects_inert_and_prefix_issues() {
        let temp = std::env::temp_dir().join("pros_guard_test_dir");
        let _ = std::fs::remove_dir_all(&temp);
        create_dir_all(temp.join("sce_sys")).unwrap();
        File::create(temp.join("eboot.bin")).unwrap();
        let mut param = File::create(temp.join("sce_sys").join("param.json")).unwrap();
        writeln!(param, "{{\"titleId\":\"GLCB00001\"}}").unwrap();

        // Target inert and bad prefix
        let res = check(&temp, "/user/app/GLCB00001").expect("should refuse");
        assert_eq!(res.kind, IssueKind::Both);
        assert_eq!(res.suggested_id, "PPSA00001");
        assert_eq!(res.suggested_path, "/data/homebrew/PPSA00001");

        // Target inert but valid prefix
        writeln!(File::create(temp.join("sce_sys").join("param.json")).unwrap(), "{{\"titleId\":\"PPSA90001\"}}").unwrap();
        let res2 = check(&temp, "/user/app/PPSA90001").expect("should refuse");
        assert_eq!(res2.kind, IssueKind::InertPath);
        assert_eq!(res2.suggested_path, "/data/homebrew/PPSA90001");

        // Target valid path and valid prefix
        assert!(check(&temp, "/data/homebrew/PPSA90001").is_none());

        let _ = std::fs::remove_dir_all(&temp);
    }
}
