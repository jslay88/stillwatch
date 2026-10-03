//! Schema-driven settings: the form, its controls, and the atomic write.

mod assemble;
mod catalog;
mod controls;
mod document;
mod editor;
mod picker_view;
mod pickers;
mod preset_view;
mod presets;
mod values;
mod view;

#[cfg(test)]
mod tests;

pub(crate) use catalog::load as load_devices;
pub use catalog::{Catalog, GamepadSeen, PlayerSeen, activity_lit};
pub use editor::{Editor, Outcome, handle};
pub use pickers::{PickerRow, gamepad_rows, output_rows, player_label, player_rows, player_value};
pub use presets::KeyChange;
pub(crate) use values::RegionInput;
pub use view::page;
