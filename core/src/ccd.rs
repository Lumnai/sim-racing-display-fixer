//! Windows CCD (Connecting and Configuring Displays) interop: read the active display config,
//! read the span width, and re-apply a saved config. Vendor-neutral; works on any GPU.

use std::mem::zeroed;
use std::ptr::null_mut;

use windows_sys::Win32::Devices::Display::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, SetDisplayConfig, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_PATH_INFO, QDC_ONLY_ACTIVE_PATHS,
    SDC_ALLOW_CHANGES, SDC_APPLY, SDC_SAVE_TO_DATABASE, SDC_USE_SUPPLIED_DISPLAY_CONFIG,
};

#[derive(Clone)]
pub struct Snapshot {
    pub paths: Vec<DISPLAYCONFIG_PATH_INFO>,
    pub modes: Vec<DISPLAYCONFIG_MODE_INFO>,
}

impl Snapshot {
    /// Widest active source (desktop) mode - the Surround span width when healthy.
    pub fn max_source_width(&self) -> u32 {
        let mut max = 0u32;
        for m in &self.modes {
            if m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
                let w = unsafe { m.Anonymous.sourceMode.width };
                if w > max {
                    max = w;
                }
            }
        }
        max
    }

    /// Stable fingerprint of the desktop: sorted (width, height) of all active source modes.
    /// Two snapshots "match" when these are equal - resolution/layout drift changes it.
    pub fn source_dims(&self) -> Vec<(u32, u32)> {
        let mut v: Vec<(u32, u32)> = self
            .modes
            .iter()
            .filter(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE)
            .map(|m| unsafe { (m.Anonymous.sourceMode.width, m.Anonymous.sourceMode.height) })
            .collect();
        v.sort_unstable();
        v
    }
}

/// Read the current active display configuration.
pub fn query() -> Result<Snapshot, String> {
    unsafe {
        let mut n_paths: u32 = 0;
        let mut n_modes: u32 = 0;
        let e = GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, &mut n_modes);
        if e != 0 {
            return Err(format!("GetDisplayConfigBufferSizes failed: {e}"));
        }
        let mut paths = vec![zeroed::<DISPLAYCONFIG_PATH_INFO>(); n_paths as usize];
        let mut modes = vec![zeroed::<DISPLAYCONFIG_MODE_INFO>(); n_modes as usize];
        let e = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut n_paths,
            paths.as_mut_ptr(),
            &mut n_modes,
            modes.as_mut_ptr(),
            null_mut(),
        );
        if e != 0 {
            return Err(format!("QueryDisplayConfig failed: {e}"));
        }
        paths.truncate(n_paths as usize);
        modes.truncate(n_modes as usize);
        Ok(Snapshot { paths, modes })
    }
}

/// Re-apply a config and persist it to Windows' display database. Returns the WIN32 code (0 = ok).
pub fn apply(s: &Snapshot) -> i32 {
    let flags = SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_SAVE_TO_DATABASE | SDC_ALLOW_CHANGES;
    unsafe {
        SetDisplayConfig(
            s.paths.len() as u32,
            s.paths.as_ptr(),
            s.modes.len() as u32,
            s.modes.as_ptr(),
            flags,
        ) as i32
    }
}
