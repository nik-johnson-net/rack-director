//! Migration 26: move the bundled OSMs' `packages`/`postinstall` variables into cloud-config.
//!
//! The bundled AlmaLinux and CentOS OSMs no longer declare these install-template variables;
//! cloud-config (`packages:` and `runcmd:`) replaces them. For every role on those OSes, this
//! moves the values out of `config_template` into the role's new `cloud_config` column.
//!
//! Roles on uploaded OSMs are left alone: their templates may still use the variables. That
//! includes an uploaded module named `Default` that startup will keep (see
//! [`default_module_will_be_bundled`]).

use anyhow::Result;
use serde_json::{Map, Value};

use crate::database::Connection;

/// OS names provided by the bundled `Default` module (compared case-insensitively).
const BUNDLED_OSES: &[&str] = &["AlmaLinux", "CentOS"];

/// The bundled `Default` version that retired the variables.
const RETIRED_IN: semver::Version = semver::Version::new(0, 2, 0);

const MIGRATION_NOTE: &str = "\
# Migrated by rack-director from this role's `packages`/`postinstall` OS variables.
# `postinstall` now runs on first boot (runcmd) instead of during installation.
";

/// Move `packages`/`postinstall` from `config_template` into `cloud_config` for roles on the
/// bundled Default OSM.
pub async fn move_os_variables_to_cloud_config(conn: &Connection) -> Result<()> {
    if !default_module_will_be_bundled(conn).await? {
        log::info!(
            "Skipping packages/postinstall migration: no Default OSM, or an uploaded one that startup keeps"
        );
        return Ok(());
    }
    let rows: Vec<(i64, String, String)> = conn
        .query(
            "SELECT id, os_name, config_template FROM roles
             WHERE osm_module = 'Default' COLLATE NOCASE AND config_template IS NOT NULL",
            (),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .await?;

    let mut migrated = 0;
    for (id, os_name, config_json) in rows {
        if !BUNDLED_OSES
            .iter()
            .any(|os| os.eq_ignore_ascii_case(&os_name))
        {
            continue;
        }
        let Ok(Value::Object(mut config)) = serde_json::from_str::<Value>(&config_json) else {
            log::warn!("Role {id}: config_template is not a JSON object; skipping");
            continue;
        };
        if !config.contains_key("packages") && !config.contains_key("postinstall") {
            continue;
        }
        let cloud_config = extract_cloud_config(id, &mut config)?;
        let remaining = (!config.is_empty()).then(|| Value::Object(config).to_string());
        conn.execute(
            "UPDATE roles SET cloud_config = ?1, config_template = ?2 WHERE id = ?3",
            (cloud_config, remaining, id),
        )
        .await?;
        migrated += 1;
    }

    log::info!("Moved packages/postinstall into cloud-config for {migrated} role(s)");
    Ok(())
}

/// Whether roles on `Default` will use the bundled module's templates, which no longer read the
/// variables, once startup has synced the bundled OSM (that sync runs after migrations).
///
/// Sync always refreshes a bundled module, but replaces an uploaded `Default` only with a
/// strictly newer bundled version. With no `Default` module there are no roles to move.
async fn default_module_will_be_bundled(conn: &Connection) -> Result<bool> {
    let modules: Vec<(String, String)> = conn
        .query(
            "SELECT source, version FROM osm_modules WHERE name = 'Default' COLLATE NOCASE",
            (),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .await?;
    Ok(modules
        .iter()
        .any(|(source, version)| will_be_bundled(source, version)))
}

/// The sync's replacement rule, frozen here so this run-once migration doesn't change when the
/// sync does. A bundled module is always refreshed, whatever its version (even one that isn't
/// semver, which the sync later rejects). An uploaded one is replaced only by a newer bundled
/// version; this release bundles `RETIRED_IN`.
///
/// Known gap: the bundled OSM is loaded after migrations, so a later release bundling a newer
/// `Default` still compares against `RETIRED_IN`. An uploaded `Default` between the two is
/// replaced by the sync but its roles keep the variables.
fn will_be_bundled(source: &str, version: &str) -> bool {
    match source {
        "bundled" => true,
        "uploaded" => version
            .parse::<semver::Version>()
            .is_ok_and(|v| v < RETIRED_IN),
        _ => false,
    }
}

/// Remove `packages`/`postinstall` from `config` and build the equivalent cloud-config text,
/// or `None` when they held only empty values.
fn extract_cloud_config(role_id: i64, config: &mut Map<String, Value>) -> Result<Option<String>> {
    let packages = packages_from(role_id, config.remove("packages"));
    let postinstall = config
        .remove("postinstall")
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.trim().is_empty());

    let mut doc = Map::new();
    if !packages.is_empty() {
        doc.insert("packages".to_string(), Value::from(packages));
    }
    if let Some(script) = postinstall {
        doc.insert("runcmd".to_string(), Value::from(vec![script]));
    }
    if doc.is_empty() {
        return Ok(None);
    }

    let yaml = serde_saphyr::to_string(&Value::Object(doc))?;
    Ok(Some(format!("#cloud-config\n{MIGRATION_NOTE}{yaml}")))
}

/// Package names to carry over. Kickstart exclusions (`-name`) have no cloud-config
/// equivalent and are dropped with a warning.
fn packages_from(role_id: i64, value: Option<Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
        .filter(|name| !name.is_empty())
        .filter(|name| {
            let excluded = name.starts_with('-');
            if excluded {
                log::warn!("Role {role_id}: dropping package exclusion '{name}' (no cloud-config equivalent)");
            }
            !excluded
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database, test_connection_factory};

    /// Insert a role row directly so the test does not depend on the roles store.
    async fn insert_role(
        conn: &Connection,
        name: &str,
        module: &str,
        os: &str,
        config: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO roles (name, osm_module, os_name, os_release, os_arch, disk_layout, config_template)
             VALUES (?1, ?2, ?3, '10', 'x86-64', '{\"disks\":[]}', ?4)",
            (name.to_string(), module.to_string(), os.to_string(), config.map(str::to_string)),
        )
        .await
        .unwrap();
    }

    async fn columns(conn: &Connection, name: &str) -> (Option<String>, Option<String>) {
        conn.query_one(
            "SELECT config_template, cloud_config FROM roles WHERE name = ?1",
            (name.to_string(),),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .await
        .unwrap()
    }

    fn parsed(cloud_config: &str) -> Value {
        assert!(cloud_config.starts_with("#cloud-config\n# Migrated by rack-director"));
        serde_saphyr::from_str(cloud_config).unwrap()
    }

    /// A migrated database with a `Default` module of the given source and version.
    async fn setup(
        factory: database::DatabaseConnectionFactory,
        source: &str,
        version: &str,
    ) -> Connection {
        let conn = database::run_migrations(&factory).await.unwrap();
        crate::osm::store::create_module(
            &conn, "Default", version, "A", "d", source, "p", true, None,
        )
        .await
        .unwrap();
        conn
    }

    #[tokio::test]
    async fn test_moves_packages_and_postinstall() {
        let conn = setup(test_connection_factory!(), "bundled", "0.1.0").await;
        insert_role(
            &conn,
            "both",
            "Default",
            "AlmaLinux",
            Some(r#"{"rootpw":"$6$x","packages":["vim"," jq ","","-firewalld"],"postinstall":"systemctl enable foo\necho done","halt":false}"#),
        )
        .await;

        move_os_variables_to_cloud_config(&conn).await.unwrap();

        let (config, cloud_config) = columns(&conn, "both").await;
        let doc = parsed(&cloud_config.unwrap());
        assert_eq!(doc["packages"], serde_json::json!(["vim", "jq"]));
        assert_eq!(
            doc["runcmd"],
            serde_json::json!(["systemctl enable foo\necho done"])
        );
        let config: Value = serde_json::from_str(&config.unwrap()).unwrap();
        assert_eq!(config, serde_json::json!({"rootpw": "$6$x", "halt": false}));
    }

    #[tokio::test]
    async fn test_empty_values_are_dropped_without_cloud_config() {
        let conn = setup(test_connection_factory!(), "bundled", "0.1.0").await;
        insert_role(
            &conn,
            "empty",
            "default",
            "centos",
            Some(r#"{"rootpw":"x","packages":[],"postinstall":"  "}"#),
        )
        .await;

        move_os_variables_to_cloud_config(&conn).await.unwrap();

        let (config, cloud_config) = columns(&conn, "empty").await;
        assert_eq!(cloud_config, None);
        assert_eq!(config.as_deref(), Some(r#"{"rootpw":"x"}"#));
    }

    #[tokio::test]
    async fn test_config_template_cleared_when_nothing_remains() {
        let conn = setup(test_connection_factory!(), "bundled", "0.1.0").await;
        insert_role(
            &conn,
            "only",
            "Default",
            "CentOS",
            Some(r#"{"packages":["git"]}"#),
        )
        .await;

        move_os_variables_to_cloud_config(&conn).await.unwrap();

        let (config, cloud_config) = columns(&conn, "only").await;
        assert_eq!(config, None);
        assert_eq!(
            parsed(&cloud_config.unwrap())["packages"],
            serde_json::json!(["git"])
        );
    }

    /// Uploaded OSMs (or other OSes) may still use the variables in their own templates.
    #[tokio::test]
    async fn test_other_modules_and_oses_untouched() {
        let conn = setup(test_connection_factory!(), "bundled", "0.1.0").await;
        let config = r#"{"packages":["git"],"postinstall":"echo hi"}"#;
        insert_role(&conn, "uploaded", "Custom", "AlmaLinux", Some(config)).await;
        insert_role(&conn, "ubuntu", "Default", "Ubuntu", Some(config)).await;
        insert_role(
            &conn,
            "plain",
            "Default",
            "AlmaLinux",
            Some(r#"{"rootpw":"x"}"#),
        )
        .await;

        move_os_variables_to_cloud_config(&conn).await.unwrap();

        assert_eq!(
            columns(&conn, "uploaded").await,
            (Some(config.to_string()), None)
        );
        assert_eq!(
            columns(&conn, "ubuntu").await,
            (Some(config.to_string()), None)
        );
        assert_eq!(
            columns(&conn, "plain").await,
            (Some(r#"{"rootpw":"x"}"#.to_string()), None)
        );
    }

    const GIT_ONLY: &str = r#"{"packages":["git"]}"#;

    /// Run the hook for one AlmaLinux role on a `Default` module of the given source and
    /// version, returning the role's `(config_template, cloud_config)`.
    async fn migrate_on_default(
        factory: database::DatabaseConnectionFactory,
        source: &str,
        version: &str,
    ) -> (Option<String>, Option<String>) {
        let conn = setup(factory, source, version).await;
        insert_role(&conn, "role", "Default", "AlmaLinux", Some(GIT_ONLY)).await;
        move_os_variables_to_cloud_config(&conn).await.unwrap();
        columns(&conn, "role").await
    }

    /// An uploaded `Default` that startup keeps still reads the variables, so its roles stay.
    #[tokio::test]
    async fn test_kept_uploaded_default_untouched() {
        let columns = migrate_on_default(test_connection_factory!(), "uploaded", "0.2.0").await;
        assert_eq!(columns, (Some(GIT_ONLY.to_string()), None));
    }

    /// An uploaded `Default` older than the bundled one is replaced by it at startup.
    #[tokio::test]
    async fn test_replaced_uploaded_default_migrated() {
        let (config, cloud_config) =
            migrate_on_default(test_connection_factory!(), "uploaded", "0.1.5").await;
        assert_eq!(config, None);
        let cloud_config = cloud_config.expect("packages moved into cloud-config");
        assert!(cloud_config.starts_with(&format!(
            "#cloud-config
{MIGRATION_NOTE}"
        )));
        assert_eq!(
            parsed(&cloud_config)["packages"],
            serde_json::json!(["git"])
        );
    }

    #[test]
    fn test_will_be_bundled() {
        assert!(will_be_bundled("bundled", "9.0.0"));
        assert!(will_be_bundled("bundled", "dev"));
        assert!(will_be_bundled("uploaded", "0.1.9"));
        assert!(!will_be_bundled("uploaded", "0.2.0"));
        assert!(!will_be_bundled("uploaded", "not-semver"));
    }
}
