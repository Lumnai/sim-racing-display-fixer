//! Detect drift from the saved target and enforce it: CCD re-apply first (fixes the common
//! resolution/refresh/layout drift on any GPU), then an adapter restart as the fallback for the
//! collapse case, then re-apply.

use std::path::Path;

use crate::{adapter, ccd, profile};

pub struct Status {
    pub current_width: u32,
    pub target_width: Option<u32>,
    pub has_profile: bool,
    pub matches: bool,
    pub adapters: Vec<String>,
    pub summary: String,
}

pub fn status(profile_path: &Path) -> Status {
    let live = ccd::query().ok();
    let current_width = live.as_ref().map(|s| s.max_source_width()).unwrap_or(0);
    let target = profile::load(profile_path).ok();
    let target_width = target.as_ref().map(|s| s.max_source_width());
    let matches = match (&live, &target) {
        (Some(l), Some(t)) => l.source_dims() == t.source_dims(),
        _ => false,
    };
    let adapters = adapter::list().unwrap_or_default();
    let summary = match (&target, &live) {
        (None, _) => "no saved profile yet - capture your current display".to_string(),
        (Some(_), None) => "could not read the live display (Session 0?)".to_string(),
        (Some(_), Some(_)) if matches => {
            format!("OK - display matches your saved profile ({current_width}px)")
        }
        (Some(_), Some(_)) => format!(
            "DRIFTED - live {current_width}px vs saved {}px",
            target_width.unwrap_or(0)
        ),
    };
    Status {
        current_width,
        target_width,
        has_profile: target.is_some(),
        matches,
        adapters,
        summary,
    }
}

#[derive(PartialEq, Debug)]
pub enum FixOutcome {
    AlreadyOk,
    FixedByCcd,
    FixedByRestart,
    Failed,
    NoProfile,
}

pub struct FixResult {
    pub outcome: FixOutcome,
    pub message: String,
}

pub fn fix(profile_path: &Path, mut log: impl FnMut(&str)) -> FixResult {
    let target = match profile::load(profile_path) {
        Ok(t) => t,
        Err(e) => {
            return FixResult {
                outcome: FixOutcome::NoProfile,
                message: format!("no usable saved profile ({e}); capture first"),
            }
        }
    };

    if matches_target(&target) {
        return FixResult {
            outcome: FixOutcome::AlreadyOk,
            message: "already matches your saved profile".into(),
        };
    }

    // Step 1: CCD re-apply (vendor-neutral; fixes most drift).
    log("drift detected - re-applying your saved display config (CCD)");
    let rc = ccd::apply(&target);
    log(&format!("SetDisplayConfig -> {rc}"));
    std::thread::sleep(std::time::Duration::from_millis(1500));
    if matches_target(&target) {
        return FixResult {
            outcome: FixOutcome::FixedByCcd,
            message: "re-applied your saved display config".into(),
        };
    }

    // Step 2: adapter restart, then re-apply (the collapse case).
    log("CCD alone did not restore it - restarting the display adapter");
    let has_nvidia = adapter::list()
        .unwrap_or_default()
        .iter()
        .any(|n| n.to_uppercase().contains("NVIDIA"));
    let filter = if has_nvidia { Some("NVIDIA") } else { None };
    if let Err(e) = adapter::restart(filter, &mut log) {
        log(&format!("adapter restart error: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let rc2 = ccd::apply(&target);
    log(&format!("post-restart SetDisplayConfig -> {rc2}"));
    std::thread::sleep(std::time::Duration::from_millis(1500));

    if matches_target(&target) {
        FixResult {
            outcome: FixOutcome::FixedByRestart,
            message: "recovered via adapter restart".into(),
        }
    } else {
        FixResult {
            outcome: FixOutcome::Failed,
            message: "could not restore your saved display config".into(),
        }
    }
}

fn matches_target(target: &ccd::Snapshot) -> bool {
    ccd::query()
        .map(|l| l.source_dims() == target.source_dims())
        .unwrap_or(false)
}
