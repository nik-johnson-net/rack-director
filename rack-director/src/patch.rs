//! Tri-state partial-update field for JSON request DTOs.
//!
//! A plain `Option<T>` cannot express a partial update over a nullable column: serde's
//! `Deserialize` impl for `Option<T>` maps *both* an absent key and an explicit `null` to
//! `None`, so "leave this column alone" and "set this column to NULL" are indistinguishable.
//! [`Patch<T>`] separates them.
//!
//! # How the absent/null distinction works — do not "simplify" this
//!
//! [`Patch`]'s [`Deserialize`] impl delegates to `Option::<T>::deserialize` and maps
//! `Some(v) => Set(v)` / `None => Clear`. On its own that only ever produces `Set` or `Clear`;
//! `Keep` comes from somewhere else entirely. When a struct field is missing from the JSON
//! object, serde never calls `deserialize` for that field at all — it falls back to the field's
//! `Default`, which for [`Patch`] is [`Patch::Keep`]. When the key *is* present with a `null`
//! value, serde does call `deserialize`, hands it a null token, and the impl yields
//! [`Patch::Clear`].
//!
//! **Every `Patch<T>` struct field MUST be covered by `#[serde(default)]`** — preferably once at
//! the struct level (`#[derive(Default)]` + `#[serde(default)]` on a DTO whose fields are all
//! `Option`/`Patch`), otherwise on each field. Omitting it does not
//! produce a compile error or even a runtime error — serde's missing-field deserializer
//! supports `deserialize_option`, so `Option::<T>::deserialize` succeeds with `None` and an
//! absent key silently deserializes to [`Patch::Clear`]. Every request that leaves the field
//! out would then wipe the column. Each DTO using `Patch` has a test asserting that an absent
//! key yields `Keep`; those tests are the guard against this, so keep them.
//!
//! # Wire semantics
//!
//! | JSON                   | Value           | Effect on the column      |
//! |------------------------|-----------------|---------------------------|
//! | key absent             | [`Patch::Keep`] | unchanged                 |
//! | `"field": null`        | [`Patch::Clear`]| set to `NULL`             |
//! | `"field": <value>`     | [`Patch::Set`]  | set to `<value>`          |
//!
//! An explicitly-supplied empty string is *not* a way to clear a column; handlers reject it with
//! a 400 (see `http::ui::validation::validate_not_blank`).

use serde::{Deserialize, Deserializer};

/// A partial-update field distinguishing "absent", "explicitly null", and "explicitly set".
///
/// See the module documentation for the mandatory `#[serde(default)]` requirement (field- or struct-level).
///
/// The derived `Default` carries no `T: Default` bound — for enums, the derive only bounds the
/// fields of the `#[default]` variant, and `Keep` has none. This matters because
/// `#[serde(default)]` requires `Patch<T>: Default` for payload types like `FirmwareMode` that
/// have no `Default` of their own.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Patch<T> {
    /// Field was absent from the request — leave the column unchanged.
    #[default]
    Keep,
    /// Field was present as `null` — set the column to `NULL`.
    Clear,
    /// Field was present with a value — set the column to that value.
    Set(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(|value| match value {
            Some(v) => Patch::Set(v),
            None => Patch::Clear,
        })
    }
}

impl<T> Patch<T> {
    /// Convert to the `Option<Option<_>>` shape the store layer uses, where the outer option
    /// is "should this column be written" and the inner one is the value (`None` => `NULL`).
    pub fn as_update(&self) -> Option<Option<&T>> {
        match self {
            Patch::Keep => None,
            Patch::Clear => Some(None),
            Patch::Set(v) => Some(Some(v)),
        }
    }

    /// The supplied value, if any. `Clear` yields `None` — use this for validating input that
    /// only makes sense for an explicitly-provided value.
    pub fn value(&self) -> Option<&T> {
        match self {
            Patch::Set(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_keep(&self) -> bool {
        matches!(self, Patch::Keep)
    }
}

impl<T: Copy> Patch<T> {
    /// [`Patch::as_update`] for `Copy` payloads, yielding owned values instead of references.
    pub fn copied_update(&self) -> Option<Option<T>> {
        self.as_update().map(|inner| inner.copied())
    }
}

impl Patch<String> {
    /// [`Patch::as_update`] borrowed as `&str`, matching store parameters that take
    /// `Option<Option<&str>>`.
    pub fn as_deref_update(&self) -> Option<Option<&str>> {
        self.as_update().map(|inner| inner.map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct StringPatch {
        #[serde(default)]
        field: Patch<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct U32Patch {
        #[serde(default)]
        field: Patch<u32>,
    }

    #[derive(Debug, Deserialize, PartialEq, Clone)]
    struct Nested {
        name: String,
        mode: Mode,
    }

    #[derive(Debug, Deserialize, PartialEq, Clone)]
    #[serde(rename_all = "lowercase")]
    enum Mode {
        Bios,
        Uefi,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct NestedPatch {
        #[serde(default)]
        field: Patch<Nested>,
    }

    fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> T {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn test_default_is_keep() {
        assert_eq!(Patch::<String>::default(), Patch::Keep);
        assert!(Patch::<String>::default().is_keep());
    }

    /// `Default` must not require `T: Default`, otherwise `#[serde(default)]` would not compile
    /// for payload types that lack a `Default` impl (e.g. `common::FirmwareMode`).
    #[test]
    fn test_default_requires_no_default_bound_on_payload() {
        #[derive(Debug, PartialEq)]
        struct NotDefault;

        assert_eq!(Patch::<NotDefault>::default(), Patch::Keep);
    }

    #[test]
    fn test_string_patch_all_three_states() {
        assert_eq!(parse::<StringPatch>("{}").field, Patch::Keep);
        assert_eq!(
            parse::<StringPatch>(r#"{"field": null}"#).field,
            Patch::Clear
        );
        assert_eq!(
            parse::<StringPatch>(r#"{"field": "hello"}"#).field,
            Patch::Set("hello".to_string())
        );
    }

    #[test]
    fn test_u32_patch_all_three_states() {
        assert_eq!(parse::<U32Patch>("{}").field, Patch::Keep);
        assert_eq!(parse::<U32Patch>(r#"{"field": null}"#).field, Patch::Clear);
        assert_eq!(parse::<U32Patch>(r#"{"field": 42}"#).field, Patch::Set(42));
    }

    #[test]
    fn test_nested_struct_patch_all_three_states() {
        assert_eq!(parse::<NestedPatch>("{}").field, Patch::Keep);
        assert_eq!(
            parse::<NestedPatch>(r#"{"field": null}"#).field,
            Patch::Clear
        );
        assert_eq!(
            parse::<NestedPatch>(r#"{"field": {"name": "a", "mode": "uefi"}}"#).field,
            Patch::Set(Nested {
                name: "a".to_string(),
                mode: Mode::Uefi,
            })
        );
    }

    /// Documents *why* `#[serde(default)]` is mandatory, and why omitting it is dangerous
    /// rather than merely wrong: serde's missing-field deserializer supports
    /// `deserialize_option`, so `Option::<T>::deserialize` succeeds with `None` and a missing
    /// key silently becomes `Clear` — an unrequested column wipe, with no error anywhere.
    #[test]
    fn test_patch_without_serde_default_treats_absent_key_as_clear() {
        #[derive(Debug, Deserialize)]
        struct NoDefault {
            field: Patch<String>,
        }

        let parsed: NoDefault = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed.field, Patch::Clear);
    }

    #[test]
    fn test_as_update() {
        assert_eq!(Patch::<String>::Keep.as_update(), None);
        assert_eq!(Patch::<String>::Clear.as_update(), Some(None));
        let set = Patch::Set("v".to_string());
        assert_eq!(set.as_update(), Some(Some(&"v".to_string())));
    }

    #[test]
    fn test_as_deref_update() {
        assert_eq!(Patch::<String>::Keep.as_deref_update(), None);
        assert_eq!(Patch::<String>::Clear.as_deref_update(), Some(None));
        assert_eq!(
            Patch::Set("v".to_string()).as_deref_update(),
            Some(Some("v"))
        );
    }

    #[test]
    fn test_copied_update() {
        assert_eq!(Patch::<u32>::Keep.copied_update(), None);
        assert_eq!(Patch::<u32>::Clear.copied_update(), Some(None));
        assert_eq!(Patch::Set(7u32).copied_update(), Some(Some(7)));
    }

    #[test]
    fn test_value() {
        assert_eq!(Patch::<u32>::Keep.value(), None);
        assert_eq!(Patch::<u32>::Clear.value(), None);
        assert_eq!(Patch::Set(7u32).value(), Some(&7));
    }

    #[test]
    fn test_is_keep() {
        assert!(Patch::<u32>::Keep.is_keep());
        assert!(!Patch::<u32>::Clear.is_keep());
        assert!(!Patch::Set(1u32).is_keep());
    }
}
