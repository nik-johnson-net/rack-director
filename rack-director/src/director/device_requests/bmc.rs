//! Comparing a device's desired BMC config with the BMC its last scan detected.

use common::device_attributes::{BmcConfig, BmcInfo};
use serde::Serialize;
use serde_json::Value;

/// How a device's desired BMC config stands against the detected BMC.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BmcConfigState {
    /// Nothing to apply: the detected BMC matches, or there is nothing to compare.
    InSync,
    /// It differs and these settings haven't been tried: the next idle boot or poll applies them.
    Pending,
    /// It differs, but an automatic reconfiguration with these settings already ran.
    Attempted,
}

/// Compare the desired config with the detected BMC and the settings last tried. The settings
/// to apply come back when the state is `Pending`.
pub(super) fn assess(
    bmc: Option<&BmcInfo>,
    config: Option<&BmcConfig>,
    attempted: Option<&Value>,
) -> (BmcConfigState, Option<Value>) {
    let (Some(bmc), Some(config)) = (bmc, config) else {
        return (BmcConfigState::InSync, None);
    };
    if !config_differs(bmc, config) {
        return (BmcConfigState::InSync, None);
    }
    let target = network_settings(config);
    if attempted == Some(&target) {
        (BmcConfigState::Attempted, None)
    } else {
        (BmcConfigState::Pending, Some(target))
    }
}

/// The settings `configure_bmc` applies to the BMC's network. Credentials are left out: they
/// are generated into `bmc_config` when the agent fetches it, and would otherwise make every
/// attempt look new.
fn network_settings(config: &BmcConfig) -> Value {
    serde_json::json!({
        "ip_address_source": config.ip_address_source.to_ascii_lowercase(),
        "ip_address": config.ip_address,
        "netmask": config.netmask,
        "gateway": config.gateway,
    })
}

/// Whether the detected BMC differs from its desired config: a different address source, or
/// a different static address. Netmask and gateway aren't detected, so they can't be compared.
///
/// ipmitool reports the source as e.g. "DHCP Address" or "Static Address"; an unknown source
/// never differs.
fn config_differs(bmc: &BmcInfo, config: &BmcConfig) -> bool {
    let Some(source) = bmc.ip_address_source.as_deref() else {
        return false;
    };
    let detected_dhcp = source.to_ascii_lowercase().contains("dhcp");
    let wants_dhcp = config.ip_address_source.eq_ignore_ascii_case("dhcp");
    if detected_dhcp != wants_dhcp {
        return true;
    }
    let wanted_ip = config.ip_address.filter(|_| !wants_dhcp);
    wanted_ip.is_some_and(|ip| bmc.ip_address.as_deref() != Some(ip.to_string().as_str()))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn bmc(source: &str, ip: Option<&str>) -> BmcInfo {
        BmcInfo {
            mac_address: "aa:bb:cc:dd:ee:ff".into(),
            ip_address: ip.map(Into::into),
            ip_address_source: Some(source.into()),
        }
    }

    pub fn config(source: &str, ip: Option<&str>) -> BmcConfig {
        serde_json::from_value(serde_json::json!({
            "ip_address_source": source,
            "ip_address": ip,
            "netmask": ip.map(|_| "255.255.255.0"),
            "gateway": ip.map(|_| "10.0.0.1"),
        }))
        .unwrap()
    }

    #[test]
    fn test_config_differs() {
        let static_cfg = config("static", Some("10.0.0.5"));
        assert!(config_differs(
            &bmc("DHCP Address", Some("10.0.0.9")),
            &static_cfg
        ));
        assert!(config_differs(
            &bmc("Static Address", Some("10.0.0.9")),
            &static_cfg
        ));
        assert!(!config_differs(
            &bmc("Static Address", Some("10.0.0.5")),
            &static_cfg
        ));
        assert!(config_differs(
            &bmc("Static Address", None),
            &config("dhcp", None)
        ));
        assert!(!config_differs(
            &bmc("DHCP Address", Some("10.0.0.9")),
            &config("dhcp", None)
        ));

        let unknown = BmcInfo {
            ip_address_source: None,
            ..bmc("", None)
        };
        assert!(!config_differs(&unknown, &static_cfg));
    }

    #[test]
    fn test_assess_tracks_attempts_and_ignores_credentials() {
        let detected = bmc("DHCP Address", None);
        let mut desired = config("static", Some("10.0.0.5"));

        let (state, target) = assess(Some(&detected), Some(&desired), None);
        assert_eq!(state, BmcConfigState::Pending);
        let target = target.expect("a pending config has settings to apply");

        // Credentials generated after the attempt don't make it a new target.
        desired.password = Some("generated".into());
        assert_eq!(
            assess(Some(&detected), Some(&desired), Some(&target)),
            (BmcConfigState::Attempted, None)
        );

        // A changed setting is tried again.
        desired.ip_address = Some("10.0.0.6".parse().unwrap());
        assert_eq!(
            assess(Some(&detected), Some(&desired), Some(&target)).0,
            BmcConfigState::Pending
        );

        // Nothing detected, or nothing desired: nothing to apply.
        assert_eq!(
            assess(None, Some(&desired), None),
            (BmcConfigState::InSync, None)
        );
        assert_eq!(
            assess(Some(&detected), None, None),
            (BmcConfigState::InSync, None)
        );
    }
}
