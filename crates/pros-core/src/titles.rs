//! What a title is called, as opposed to what its folder is called.
//!
//! A title's folder is named by identifier (`PPSA01650`); the name a person recognises is in
//! `/user/appmeta/<id>/param.json` on the target (measured, with the shape in the test
//! fixture below). The name is localised: the default language's name, else any language's,
//! else none - and none means the identifier is shown. A name is never invented.

use serde_json::Value;

/// Where the target keeps a title's description (measured).
pub const APPMETA: &str = "/user/appmeta";

/// What a title says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// The identifier, which is also the folder's name.
    pub id: String,
    /// What a person calls it, when the file says.
    ///
    /// `None` rather than a placeholder, so a caller that is given a name can believe it.
    pub name: Option<String>,
    /// Which build of it is installed.
    pub version: Option<String>,
    /// The full content identifier, which carries the region and the publisher.
    pub content_id: Option<String>,
}

impl Metadata {
    /// What to show for it: the name when there is one, the identifier otherwise.
    #[must_use]
    pub fn display(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// Where a title's description lives on the target.
#[must_use]
pub fn path_for(id: &str) -> String {
    format!("{APPMETA}/{id}/param.json")
}

/// Reads a title's description.
///
/// # Errors
///
/// When the document is not JSON. A missing name is not an error.
pub fn parse(id: &str, text: &str) -> crate::Result<Metadata> {
    let document: Value =
        serde_json::from_str(text).map_err(|why| crate::Error::failed(why.to_string()))?;

    Ok(Metadata {
        // The file's own identifier is trusted over the folder name, since a folder can be
        // copied.
        id: document
            .get("titleId")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_owned(),
        name: localised_name(&document),
        version: document
            .get("contentVersion")
            .or_else(|| document.get("masterVersion"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        content_id: document
            .get("contentId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// The title's name in the file's default language, or any language that has one.
///
/// A name in another language is still the right title's name, which beats an identifier.
fn localised_name(document: &Value) -> Option<String> {
    let languages = document.get("localizedParameters")?.as_object()?;

    let preferred = languages
        .get("defaultLanguage")
        .and_then(Value::as_str)
        .and_then(|which| languages.get(which))
        .and_then(|entry| entry.get("titleName"))
        .and_then(Value::as_str);
    if let Some(name) = preferred {
        return Some(name.to_owned());
    }

    languages
        .values()
        .find_map(|entry| entry.get("titleName").and_then(Value::as_str))
        .map(str::to_owned)
}

/// Reads what a target says about a title.
///
/// # Errors
///
/// Propagates the transfer and the parse. A description that cannot be fetched is not turned
/// into an empty name: the caller decides between showing the identifier and reporting that
/// nothing could be read.
pub fn read(link: &pros_link::Link, id: &str) -> crate::Result<Metadata> {
    let bytes = pros_link::files::retrieve(link, &path_for(id))?;
    parse(id, &String::from_utf8_lossy(&bytes))
}

#[cfg(test)]
mod tests {
    use super::{Metadata, parse, path_for};

    /// A target's own file, trimmed to the fields this reads.
    const REAL: &str = r#"{
        "applicationCategoryType": 65536,
        "contentId": "UP4381-PPSA01650_00-YOUTUBESIEA00000",
        "contentVersion": "01.000.003",
        "localizedParameters": {
            "defaultLanguage": "en-US",
            "en-US": { "titleName": "YouTube" }
        },
        "masterVersion": "01.00",
        "titleId": "PPSA01650"
    }"#;

    /// A real description yields the name, version and content identifier.
    #[test]
    fn a_real_title_says_what_it_is_called() {
        let found = parse("PPSA01650", REAL).expect("it reads");
        assert_eq!(
            found,
            Metadata {
                id: "PPSA01650".to_owned(),
                name: Some("YouTube".to_owned()),
                version: Some("01.000.003".to_owned()),
                content_id: Some("UP4381-PPSA01650_00-YOUTUBESIEA00000".to_owned()),
            }
        );
        assert_eq!(found.display(), "YouTube");
    }

    /// A name in a non-default language is used rather than the identifier.
    #[test]
    fn any_language_beats_an_identifier() {
        let text = r#"{
            "titleId": "PPSA00001",
            "localizedParameters": { "ja-JP": { "titleName": "タイトル" } }
        }"#;
        assert_eq!(
            parse("PPSA00001", text).expect("it reads").name.as_deref(),
            Some("タイトル")
        );
    }

    /// A title with no name is shown as its identifier, never a guess.
    #[test]
    fn a_title_with_no_name_is_its_identifier_and_not_a_guess() {
        let found = parse("PPSA00002", r#"{"titleId":"PPSA00002"}"#).expect("it reads");
        assert!(found.name.is_none());
        assert_eq!(found.display(), "PPSA00002");
    }

    /// The file's own identifier is believed over the folder it was found in.
    #[test]
    fn the_file_names_its_own_identifier() {
        let found = parse("WRONG0000", REAL).expect("it reads");
        assert_eq!(found.id, "PPSA01650");
    }

    /// A document that is not JSON is an error, not a nameless title.
    #[test]
    fn a_document_that_is_not_a_description_is_an_error() {
        assert!(parse("PPSA00003", "not json at all").is_err());
    }

    /// The path is built the way the target lays it out.
    #[test]
    fn the_path_is_where_the_target_keeps_it() {
        assert_eq!(path_for("PPSA01650"), "/user/appmeta/PPSA01650/param.json");
    }
}
