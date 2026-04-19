mod resolve;
mod validate;

pub use resolve::layout_uses_labels;
pub use resolve::resolve_disk_layout;
pub use resolve::validate_layout_against_platform;
pub use validate::validate_disk_layout;
