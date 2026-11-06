//! The install-time shell snippet OSM templates run to hand first boot over to cloud-init.
//!
//! It writes a `cloud.cfg.d` drop-in that points cloud-init's NoCloud datasource at
//! rack-director, plus a systemd unit that reports the first-boot result back so the
//! device's `first_boot` plan step can complete.

use anyhow::{Result, anyhow};
use serde_json::json;
use uuid::Uuid;

/// Where the drop-in is written. Sorts after Ubuntu installer drop-ins (`99-installer.cfg`).
const DROP_IN_PATH: &str = "/etc/cloud/cloud.cfg.d/99-rack-director.cfg";
const REPORTER_PATH: &str = "/usr/local/sbin/rack-director-first-boot";
const UNIT_NAME: &str = "rack-director-first-boot.service";

/// The outer snippet. It writes the other files through quoted heredocs, so they are copied
/// verbatim.
const INSTALL_SH: &str = include_str!("files/install.sh");

/// Waits for cloud-init, then reports success or failure to rack-director once.
///
/// `cloud-init status` exits 2 ("degraded done") when cloud-init finished but logged warnings
/// or deprecations, for example a schema warning in the cloud-config. That counts as success;
/// the warnings go to the journal. The report names `first_boot` so it can't complete any
/// other step.
const REPORTER_SH: &str = include_str!("files/first-boot.sh");

/// Summarizes `cloud-init status --format json` errors on one line. The reporter passes it to
/// `python3 -c` in single quotes, so it must not contain `'`.
const ERRORS_PY: &str = include_str!("files/cloud-init-errors.py");

/// Runs the reporter in the background (`Type=simple`) so boot never waits on it, and it is
/// deliberately not ordered after `cloud-final.service`: on some distros that unit is
/// `After=multi-user.target`, which would form an ordering cycle. The script itself waits.
const REPORTER_UNIT: &str = include_str!("files/first-boot.service");

/// Path of a device's NoCloud seed endpoint, relative to the rack-director root URL.
///
/// NoCloud appends `meta-data`, `user-data`, etc. to this base, so it ends in `/`.
pub fn seed_path(uuid: &Uuid) -> String {
    format!("/cnc/devices/{uuid}/cloud-init/")
}

/// Absolute NoCloud `seedfrom` URL of a device's seed endpoint.
pub fn seed_url(root_url: &str, uuid: &Uuid) -> String {
    format!("{root_url}{}", seed_path(uuid))
}

/// Render the POSIX `sh` snippet an install template runs from its post-install hook.
///
/// It writes into `$RD_TARGET` (the installed root; empty when already chrooted into it)
/// and exits the hook non-zero if any step fails. `plan_id` is reported back with the
/// first-boot result so a stale report cannot complete a newer plan.
pub fn install_script(root_url: &str, uuid: &Uuid, plan_id: Option<i64>) -> Result<String> {
    let seed_url = seed_url(root_url, uuid);
    let unit_path = format!("/etc/systemd/system/{UNIT_NAME}");
    let paths = format!("{DROP_IN_PATH} {REPORTER_PATH} {unit_path}");
    render(
        INSTALL_SH,
        &[
            ("SEED_URL", &seed_url),
            ("UNIT_NAME", UNIT_NAME),
            ("UNIT_PATH", &unit_path),
            ("DROP_IN_PATH", DROP_IN_PATH),
            ("REPORTER_PATH", REPORTER_PATH),
            ("PATHS", &paths),
            ("DROP_IN", &drop_in(&seed_url)?),
            ("REPORTER", &reporter_script(root_url, uuid, plan_id)?),
            ("UNIT", REPORTER_UNIT),
        ],
    )
}

/// Fill `@@NAME@@` placeholders in one pass, so substituted values are never re-scanned.
/// An unknown or unterminated placeholder is a bug in a bundled file.
fn render(template: &str, vars: &[(&str, &str)]) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("@@") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let len = after
            .find("@@")
            .ok_or_else(|| anyhow!("unterminated placeholder in cloud-init template"))?;
        let name = &after[..len];
        let (_, value) = vars
            .iter()
            .find(|(key, _)| *key == name)
            .ok_or_else(|| anyhow!("unknown placeholder @@{name}@@ in cloud-init template"))?;
        out.push_str(value);
        rest = &after[len + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// cloud-init system config for a rack-director provisioned host.
///
/// The installer already configured networking, disks and the hostname, so cloud-init must
/// not redo them; the seed is fetched once (`manual_cache_clean`) and only `NoCloud` is
/// listed, since an HTTP seed can lose detection to other datasources.
///
/// `preserve_hostname` keeps the hostname the installer set. The seed's `local-hostname` is
/// computed again at first boot from current lease data, and could name a different NIC's
/// domain, or `unknown`.
///
/// `ssh_pwauth: true` restores sshd's own default (password auth on) that RHEL's `cloud.cfg`
/// would turn off. It must be a boolean: the string `unchanged` is deprecated and logs a
/// deprecation, which makes `cloud-init status` exit 2 (degraded).
fn drop_in(seed_url: &str) -> Result<String> {
    let config = json!({
        "datasource_list": ["NoCloud"],
        "datasource": { "NoCloud": { "seedfrom": seed_url } },
        "manual_cache_clean": true,
        "network": { "config": "disabled" },
        "growpart": { "mode": "off" },
        "resize_rootfs": false,
        "preserve_hostname": true,
        "ssh_pwauth": true,
        "users": [],
    });
    let yaml = serde_saphyr::to_string(&config)?;
    Ok(format!(
        "# Managed by rack-director. Settings here are defaults: the device's cloud-config\n\
         # (served from the seedfrom URL) may override all but datasource and network.\n{yaml}"
    ))
}

fn reporter_script(root_url: &str, uuid: &Uuid, plan_id: Option<i64>) -> Result<String> {
    let plan = plan_id.map(|id| id.to_string()).unwrap_or_default();
    render(
        REPORTER_SH,
        &[
            ("URL", &shell_quote(root_url)),
            ("UUID", &shell_quote(&uuid.to_string())),
            ("PLAN_ID", &shell_quote(&plan)),
            ("ERRORS_PY", ERRORS_PY),
            ("UNIT_NAME", UNIT_NAME),
        ],
    )
}

/// Quote `value` as a single POSIX shell word.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Heredoc terminator in `files/install.sh`.
    const EOF_MARKER: &str = "RD_EOF";

    fn uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    fn script() -> String {
        install_script("http://10.0.0.1:3000", &uuid(), Some(7)).unwrap()
    }

    /// Extract the body of the heredoc written to `path`.
    fn heredoc_for(script: &str, path: &str) -> String {
        let start_marker = format!("cat > \"$rd_root{path}\" <<'{EOF_MARKER}'\n");
        let start = script.find(&start_marker).unwrap() + start_marker.len();
        let len = script[start..].find(&format!("{EOF_MARKER}\n")).unwrap();
        script[start..start + len].to_string()
    }

    #[test]
    fn test_seed_path_ends_with_slash() {
        assert_eq!(
            seed_path(&uuid()),
            "/cnc/devices/550e8400-e29b-41d4-a716-446655440001/cloud-init/"
        );
    }

    #[test]
    fn test_drop_in_points_nocloud_at_rack_director() {
        let drop_in: serde_json::Value =
            serde_saphyr::from_str(&heredoc_for(&script(), DROP_IN_PATH)).unwrap();
        assert_eq!(drop_in["datasource_list"], json!(["NoCloud"]));
        assert_eq!(
            drop_in["datasource"]["NoCloud"]["seedfrom"],
            json!(
                "http://10.0.0.1:3000/cnc/devices/550e8400-e29b-41d4-a716-446655440001/cloud-init/"
            )
        );
        assert_eq!(drop_in["manual_cache_clean"], json!(true));
        assert_eq!(drop_in["network"]["config"], json!("disabled"));
        // `off` must stay a string: YAML 1.1 would otherwise read it as boolean false.
        assert_eq!(drop_in["growpart"]["mode"], json!("off"));
        assert_eq!(drop_in["resize_rootfs"], json!(false));
        assert_eq!(drop_in["preserve_hostname"], json!(true));
        // A string value is deprecated and would leave cloud-init "degraded".
        assert_eq!(drop_in["ssh_pwauth"], json!(true));
        assert_eq!(drop_in["users"], json!([]));
    }

    #[test]
    fn test_reporter_embeds_device_and_plan() {
        let reporter = heredoc_for(&script(), REPORTER_PATH);
        assert!(reporter.starts_with("#!/bin/sh\n"));
        assert!(reporter.contains("RD_URL='http://10.0.0.1:3000'"));
        assert!(reporter.contains("RD_UUID='550e8400-e29b-41d4-a716-446655440001'"));
        assert!(reporter.contains("RD_PLAN_ID='7'"));
        assert!(reporter.contains("cloud-init status --wait"));
        assert!(reporter.contains("\"$RD_URL/cnc/$endpoint\""));
        assert!(reporter.contains(r#"\"action\":\"first_boot\""#));
        // Exit 2 ("degraded done") is a success, not a failed first boot.
        assert!(reporter.contains(
            r#"if [ "$state" = "done" ] && { [ "$rc" -eq 0 ] || [ "$rc" -eq 2 ]; }; then"#
        ));
    }

    #[test]
    fn test_reporter_without_plan_omits_plan_id() {
        let script = install_script("http://rd", &uuid(), None).unwrap();
        assert!(heredoc_for(&script, REPORTER_PATH).contains("RD_PLAN_ID=''"));
    }

    #[test]
    fn test_script_enables_unit_and_fails_loudly() {
        let script = script();
        assert!(script.contains(&format!("systemctl --root=\"$rd_root\" enable {UNIT_NAME}")));
        assert!(script.contains("set -e"));
        assert!(script.contains("exit \"$rd_status\""));
        // Kickstart ends a %post section at a line starting with %end.
        assert!(!script.lines().any(|l| l.starts_with("%end")));
    }

    #[test]
    fn test_root_url_is_shell_quoted() {
        let script = install_script("http://a'b", &uuid(), None).unwrap();
        assert!(script.contains(r"RD_URL='http://a'\''b'"));
    }

    #[test]
    fn test_render_fills_placeholders_once() {
        // A value that looks like a placeholder is not expanded again.
        let out = render("a @@X@@ b @@Y@@", &[("X", "@@Y@@"), ("Y", "y")]).unwrap();
        assert_eq!(out, "a @@Y@@ b y");
        assert!(render("@@NOPE@@", &[]).is_err());
        assert!(render("@@X", &[("X", "x")]).is_err());
    }

    #[test]
    fn test_bundled_files_are_shell_safe() {
        for file in [INSTALL_SH, REPORTER_SH, ERRORS_PY, REPORTER_UNIT] {
            // A CRLF checkout would break the scripts; .gitattributes pins these to LF.
            assert!(!file.contains('\r'));
        }
        // ERRORS_PY is embedded in a single-quoted shell word.
        assert!(!ERRORS_PY.contains('\''));
        let script = script();
        assert!(!script.contains("@@"));
        // Each of the three heredocs ends exactly once: no written file contains the marker.
        assert_eq!(script.lines().filter(|l| *l == EOF_MARKER).count(), 3);
    }
}
