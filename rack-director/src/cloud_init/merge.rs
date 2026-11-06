//! Layering of cloud-config documents (global first, then role).

use serde_json::Value;

/// Merge `overlay` into `base`.
///
/// Mappings merge key by key, lists concatenate (base items first), and anything else —
/// including a type mismatch — takes the overlay's value. Concatenating lists is what lets a
/// global layer's `users`/`packages`/`runcmd` survive a role that adds its own.
pub(crate) fn deep_merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(base_value) => deep_merge(base_value, overlay_value),
                    None => {
                        base_map.insert(key, overlay_value);
                    }
                }
            }
        }
        (Value::Array(base_items), Value::Array(overlay_items)) => {
            base_items.extend(overlay_items);
        }
        (base, overlay) => *base = overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn merged(base: Value, overlay: Value) -> Value {
        let mut base = base;
        deep_merge(&mut base, overlay);
        base
    }

    #[test]
    fn test_lists_concatenate_base_first() {
        let result = merged(json!({"packages": ["vim"]}), json!({"packages": ["jq"]}));
        assert_eq!(result, json!({"packages": ["vim", "jq"]}));
    }

    #[test]
    fn test_maps_merge_recursively() {
        let result = merged(
            json!({"apt": {"proxy": "http://p", "primary": [1]}}),
            json!({"apt": {"primary": [2], "conf": "x"}}),
        );
        assert_eq!(
            result,
            json!({"apt": {"proxy": "http://p", "primary": [1, 2], "conf": "x"}})
        );
    }

    #[test]
    fn test_scalars_take_overlay() {
        let result = merged(
            json!({"disable_root": true}),
            json!({"disable_root": false}),
        );
        assert_eq!(result, json!({"disable_root": false}));
    }

    #[test]
    fn test_type_mismatch_takes_overlay() {
        let result = merged(json!({"ntp": {"enabled": true}}), json!({"ntp": null}));
        assert_eq!(result, json!({"ntp": null}));
    }

    /// The headline use: global root keys and a role's own users both reach the host.
    #[test]
    fn test_global_root_keys_survive_role_users() {
        let result = merged(
            json!({"disable_root": false, "users": [{"name": "root", "ssh_authorized_keys": ["k1"]}]}),
            json!({"users": [{"name": "app"}]}),
        );
        assert_eq!(
            result["users"],
            json!([{"name": "root", "ssh_authorized_keys": ["k1"]}, {"name": "app"}])
        );
        assert_eq!(result["disable_root"], json!(false));
    }
}
