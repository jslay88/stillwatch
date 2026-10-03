//! Schema-driven settings: the form, its controls, and the atomic write.

mod assemble;
mod controls;
mod document;
mod editor;
mod values;
mod view;

#[cfg(test)]
mod tests;

pub use editor::{Editor, Outcome, handle};
pub use view::page;
