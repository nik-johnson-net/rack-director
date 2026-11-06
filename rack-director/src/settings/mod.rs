//! Site-wide settings, stored as key/value rows in the `settings` table.

pub mod store;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// A stored setting value and when it last changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Setting {
    pub value: String,
    pub updated_at: DateTime<Utc>,
}
