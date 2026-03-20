//! Device warning CRUD operations.
//!
//! Device warnings are non-fatal alerts attached to a device that surface in the UI.
//! They are created automatically (e.g. when a stale disk label override is dropped)
//! or can be dismissed by an operator via the API.

mod store;

pub use store::{
    DeviceWarning, create_warning, delete_warning, get_device_id_by_uuid, list_warnings,
};
