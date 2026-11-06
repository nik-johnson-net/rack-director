//! Parsing and validation of operator-supplied cloud-config text.

use serde_json::{Map, Value};

/// Largest accepted cloud-config document, per layer.
pub const MAX_CLOUD_CONFIG_BYTES: usize = 64 * 1024;

/// First-line markers of cloud-init user-data formats other than `#cloud-config`.
///
/// rack-director merges layers as YAML, so it only accepts plain cloud-config.
const UNSUPPORTED_FORMATS: &[(&str, &str)] = &[
    (
        "## template:",
        "Jinja templates are not supported; use plain #cloud-config",
    ),
    (
        "#!",
        "Shell scripts are not supported; use runcmd in #cloud-config",
    ),
    (
        "#include",
        "#include user-data is not supported; use plain #cloud-config",
    ),
    (
        "#cloud-boothook",
        "Boothooks are not supported; use bootcmd in #cloud-config",
    ),
    (
        "#cloud-config-archive",
        "Cloud-config archives are not supported; use plain #cloud-config",
    ),
    (
        "#part-handler",
        "Part handlers are not supported; use plain #cloud-config",
    ),
    (
        "content-type:",
        "MIME multipart user-data is not supported; use plain #cloud-config",
    ),
];

/// Parse cloud-config text into a top-level mapping.
///
/// Returns `Ok(None)` for a document with no content (blank or comments only). Booleans and
/// legacy octal numbers follow YAML 1.1 (`yes`, `0644`) so values mean what cloud-init's
/// PyYAML parser would make of them.
pub(crate) fn parse(text: &str) -> Result<Option<Map<String, Value>>, String> {
    if text.len() > MAX_CLOUD_CONFIG_BYTES {
        return Err(format!(
            "Cloud-config must be at most {} KiB",
            MAX_CLOUD_CONFIG_BYTES / 1024
        ));
    }
    reject_unsupported_format(text)?;

    let options = serde_saphyr::options! { legacy_octal_numbers: true };
    let value: Value = serde_saphyr::from_str_with_options(text, options)
        .map_err(|e| format!("Invalid YAML: {e}"))?;

    match value {
        Value::Null => Ok(None),
        Value::Object(map) => Ok(Some(map)),
        _ => Err("Cloud-config must be a YAML mapping of cloud-init settings".to_string()),
    }
}

/// Whether `text` would contribute anything to a merged cloud-config. Blank, comment-only and
/// `{}` documents don't. Text that doesn't parse counts as content, so callers that refuse
/// cloud-config err on the side of refusing; saves validate it anyway.
pub fn has_content(text: &str) -> bool {
    match parse(text) {
        Ok(None) => false,
        Ok(Some(map)) => !map.is_empty(),
        Err(_) => true,
    }
}

/// Check that `text` is cloud-config rack-director can store and merge.
pub fn validate(text: &str) -> Result<(), String> {
    parse(text).map(|_| ())
}

fn reject_unsupported_format(text: &str) -> Result<(), String> {
    let Some(first_line) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return Ok(());
    };
    let lower = first_line.to_ascii_lowercase();
    match UNSUPPORTED_FORMATS
        .iter()
        .find(|(marker, _)| lower.starts_with(marker))
    {
        Some((_, message)) => Err(message.to_string()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mapping_with_header() {
        let map = parse("#cloud-config\npackages: [jq]\n").unwrap().unwrap();
        assert_eq!(map["packages"], serde_json::json!(["jq"]));
    }

    #[test]
    fn test_parse_without_header() {
        let map = parse("disable_root: false\n").unwrap().unwrap();
        assert_eq!(map["disable_root"], Value::Bool(false));
    }

    #[test]
    fn test_parse_empty_and_comment_only_documents_are_none() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("#cloud-config\n# nothing yet\n").unwrap(), None);
    }

    /// Unquoted `0644` and `yes` must mean what PyYAML (YAML 1.1) reads, or a merged
    /// document would silently change file modes and booleans.
    #[test]
    fn test_has_content() {
        assert!(!has_content(""));
        assert!(!has_content(
            "#cloud-config
# nothing yet
"
        ));
        assert!(!has_content(
            "#cloud-config
{}
"
        ));
        assert!(has_content(
            "#cloud-config
packages: [jq]
"
        ));
        assert!(has_content(
            "- not a mapping
"
        ));
    }

    #[test]
    fn test_parse_follows_yaml_1_1_scalars() {
        let map = parse("package_update: yes\nwrite_files:\n  - path: /x\n    permissions: 0644\n")
            .unwrap()
            .unwrap();
        assert_eq!(map["package_update"], Value::Bool(true));
        assert_eq!(map["write_files"][0]["permissions"], Value::from(0o644));
    }

    #[test]
    fn test_parse_rejects_non_mapping() {
        let err = parse("- a\n- b\n").unwrap_err();
        assert!(err.contains("mapping"), "{err}");
        assert!(parse("just a string\n").is_err());
    }

    #[test]
    fn test_parse_rejects_invalid_yaml() {
        let err = parse("packages: [jq,\n").unwrap_err();
        assert!(err.starts_with("Invalid YAML"), "{err}");
    }

    #[test]
    fn test_parse_rejects_jinja_template() {
        let err = parse("## template: jinja\n#cloud-config\nhostname: x\n").unwrap_err();
        assert!(err.contains("Jinja"), "{err}");
    }

    #[test]
    fn test_parse_rejects_other_user_data_formats() {
        assert!(parse("#!/bin/sh\necho hi\n").is_err());
        assert!(parse("#include\nhttp://example.com/x\n").is_err());
        assert!(parse("Content-Type: multipart/mixed; boundary=x\n").is_err());
    }

    #[test]
    fn test_parse_rejects_oversized_document() {
        let text = format!(
            "runcmd:\n  - echo '{}'\n",
            "x".repeat(MAX_CLOUD_CONFIG_BYTES)
        );
        let err = parse(&text).unwrap_err();
        assert!(err.contains("KiB"), "{err}");
    }
}
