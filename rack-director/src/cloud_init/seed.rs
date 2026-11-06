//! The NoCloud seed (`user-data` and `meta-data`) served to a device on first boot.

use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::merge::deep_merge;
use super::parse::parse;

/// Identity of the device a seed is built for.
#[derive(Debug, Clone)]
pub struct SeedDevice<'a> {
    pub uuid: &'a Uuid,
    /// Hostname cloud-init reports as `local-hostname` (the device FQDN).
    pub fqdn: &'a str,
    /// Provisioning plan the seed belongs to. Part of the instance-id, so a reprovisioned
    /// machine is a new cloud-init instance.
    pub plan_id: Option<i64>,
}

/// Rendered NoCloud seed documents.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CloudInitSeed {
    pub user_data: String,
    pub meta_data: String,
}

/// Build the seed for a device from the global and role cloud-config layers.
///
/// Layers are merged global-then-role (see [`super::merge::deep_merge`]). Absent or empty
/// layers contribute nothing; the result is always a valid `#cloud-config` document.
pub fn build_seed(
    global: Option<&str>,
    role: Option<&str>,
    device: &SeedDevice<'_>,
) -> Result<CloudInitSeed> {
    let mut merged = Value::Object(Map::new());
    for (layer, text) in [("Global", global), ("Role", role)] {
        let Some(text) = text else { continue };
        let parsed = parse(text).map_err(|e| anyhow!("{layer} cloud-config is invalid: {e}"))?;
        if let Some(map) = parsed {
            deep_merge(&mut merged, Value::Object(map));
        }
    }

    Ok(CloudInitSeed {
        user_data: to_cloud_config(&merged)?,
        meta_data: meta_data(device)?,
    })
}

/// Serialize a cloud-config mapping as a `#cloud-config` document.
pub fn to_cloud_config(doc: &Value) -> Result<String> {
    let yaml = serde_saphyr::to_string(doc)?;
    Ok(format!("#cloud-config\n{yaml}"))
}

/// The cloud-init instance-id for a device's provisioning plan.
pub fn instance_id(uuid: &Uuid, plan_id: Option<i64>) -> String {
    match plan_id {
        Some(id) => format!("rd-{uuid}-p{id}"),
        None => format!("rd-{uuid}"),
    }
}

fn meta_data(device: &SeedDevice<'_>) -> Result<String> {
    let doc = json!({
        "instance-id": instance_id(device.uuid, device.plan_id),
        "local-hostname": device.fqdn,
    });
    Ok(serde_saphyr::to_string(&doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    fn device(uuid: &Uuid) -> SeedDevice<'_> {
        SeedDevice {
            uuid,
            fqdn: "web01.example.com",
            plan_id: Some(42),
        }
    }

    fn user_data_value(seed: &CloudInitSeed) -> Value {
        assert!(seed.user_data.starts_with("#cloud-config\n"));
        Value::Object(parse(&seed.user_data).unwrap().unwrap_or_default())
    }

    #[test]
    fn test_build_seed_merges_global_then_role() {
        let uuid = uuid();
        let global = "#cloud-config\ndisable_root: false\nusers:\n  - name: root\n    ssh_authorized_keys: [k1]\n";
        let role = "packages: [jq]\nusers:\n  - name: app\n";
        let seed = build_seed(Some(global), Some(role), &device(&uuid)).unwrap();

        let doc = user_data_value(&seed);
        assert_eq!(doc["disable_root"], json!(false));
        assert_eq!(doc["packages"], json!(["jq"]));
        assert_eq!(doc["users"][0]["name"], json!("root"));
        assert_eq!(doc["users"][0]["ssh_authorized_keys"], json!(["k1"]));
        assert_eq!(doc["users"][1]["name"], json!("app"));
    }

    #[test]
    fn test_build_seed_without_layers_is_empty_cloud_config() {
        let uuid = uuid();
        let seed = build_seed(None, Some("# nothing\n"), &device(&uuid)).unwrap();
        assert_eq!(user_data_value(&seed), json!({}));
    }

    /// Strings that YAML 1.1 would read as other types must stay strings once re-emitted.
    #[test]
    fn test_build_seed_preserves_ambiguous_strings() {
        let uuid = uuid();
        let role = "write_files:\n  - path: /x\n    permissions: '0644'\n    content: \"yes\"\n";
        let seed = build_seed(None, Some(role), &device(&uuid)).unwrap();
        let doc = user_data_value(&seed);
        assert_eq!(doc["write_files"][0]["permissions"], json!("0644"));
        assert_eq!(doc["write_files"][0]["content"], json!("yes"));
    }

    #[test]
    fn test_build_seed_reports_invalid_layer() {
        let uuid = uuid();
        let err = build_seed(Some("- not a map\n"), None, &device(&uuid)).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("Global cloud-config is invalid"),
            "{err}"
        );
    }

    #[test]
    fn test_meta_data_carries_instance_id_and_hostname() {
        let uuid = uuid();
        let seed = build_seed(None, None, &device(&uuid)).unwrap();
        let meta: Value = serde_saphyr::from_str(&seed.meta_data).unwrap();
        assert_eq!(
            meta["instance-id"],
            json!("rd-550e8400-e29b-41d4-a716-446655440001-p42")
        );
        assert_eq!(meta["local-hostname"], json!("web01.example.com"));
    }

    #[test]
    fn test_instance_id_without_plan() {
        assert_eq!(
            instance_id(&uuid(), None),
            "rd-550e8400-e29b-41d4-a716-446655440001"
        );
    }
}
