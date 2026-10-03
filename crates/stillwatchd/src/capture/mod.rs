//! Screen capture backends. Each implements
//! [`ScreenCapture`](stillwatch_core::backend::ScreenCapture) and hands the
//! daemon luma grids, never frames.

pub mod kwin;
