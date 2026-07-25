//! Native display heal engine for the Lunis Display Fixer.
//!
//! The fixer is profile-based, not hardcoded: it captures the display config the user wants
//! (all active monitors, resolution, refresh, layout) via Windows CCD, then on each check
//! enforces it. Two escalating steps:
//!   1. CCD `SetDisplayConfig` re-applies the saved config - fixes the common drift on ANY GPU
//!      (a monitor that dropped resolution, a refresh reset 120->60, a rearranged layout).
//!   2. If CCD can't (the NVIDIA Surround-collapse case, where the wide mode isn't offered until
//!      the DP links re-train), restart the display adapter (SetupAPI disable/enable), then re-apply.
//!
//! `ccd` + `adapter` are vendor-neutral; the same mechanism heals NVIDIA / AMD / Intel.

pub mod adapter;
pub mod ccd;
pub mod engine;
pub mod modes;
pub mod profile;

pub use engine::{fix, status, FixOutcome, FixResult, Status};
