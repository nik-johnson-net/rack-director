use serde::{Deserialize, Serialize};

/// OSM archive manifest (manifest.toml at archive root).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: semver::Version,
    pub author: String,
    pub description: String,
    pub operating_systems: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_valid_manifest() {
        let toml_str = r#"
name = "Default"
version = "1.0.0"
author = "Rack Director Project"
description = "Default operating system module"
operating_systems = ["ubuntu-2204", "rhel-9"]
"#;
        let manifest: Manifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "Default");
        assert_eq!(manifest.version, semver::Version::new(1, 0, 0));
        assert_eq!(manifest.operating_systems.len(), 2);
    }

    #[test]
    fn test_deserialize_missing_required_field() {
        let toml_str = r#"
name = "Default"
version = "1.0.0"
"#;
        let result: Result<Manifest, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_semver() {
        let toml_str = r#"
name = "Default"
version = "not-a-version"
author = "Test"
description = "Test"
operating_systems = []
"#;
        let result: Result<Manifest, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_empty_operating_systems() {
        let toml_str = r#"
name = "Empty"
version = "0.1.0"
author = "Test"
description = "No OSes"
operating_systems = []
"#;
        let manifest: Manifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.operating_systems.is_empty());
    }

    #[test]
    fn test_roundtrip_serialize_deserialize() {
        let manifest = Manifest {
            name: "Custom".to_string(),
            version: semver::Version::new(2, 1, 0),
            author: "Test Author".to_string(),
            description: "A test module".to_string(),
            operating_systems: vec!["os1".to_string()],
        };
        let serialized = toml::to_string(&manifest).unwrap();
        let deserialized: Manifest = toml::from_str(&serialized).unwrap();
        assert_eq!(manifest, deserialized);
    }
}
