//! Enumerate and apply display modes for the primary display, so the app can offer a
//! ready-made list of resolutions and refresh rates.

use std::mem::zeroed;
use std::ptr::null;

use windows_sys::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplaySettingsW, CDS_TEST, CDS_UPDATEREGISTRY, DEVMODEW,
    DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH,
    ENUM_CURRENT_SETTINGS,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
}

impl DisplayMode {
    pub fn label(&self) -> String {
        format!("{} x {}  ·  {} Hz", self.width, self.height, self.hz)
    }
}

/// The mode the primary display is running right now.
pub fn current() -> Option<DisplayMode> {
    unsafe {
        let mut dm: DEVMODEW = zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(null(), ENUM_CURRENT_SETTINGS, &mut dm) == 0 {
            return None;
        }
        Some(DisplayMode {
            width: dm.dmPelsWidth,
            height: dm.dmPelsHeight,
            hz: dm.dmDisplayFrequency,
        })
    }
}

/// Available modes for the primary display: deduped, sensible ones only, widest/fastest first.
pub fn list() -> Vec<DisplayMode> {
    let mut out: Vec<DisplayMode> = Vec::new();
    unsafe {
        let mut i = 0u32;
        loop {
            let mut dm: DEVMODEW = zeroed();
            dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            if EnumDisplaySettingsW(null(), i, &mut dm) == 0 {
                break;
            }
            i += 1;
            let m = DisplayMode {
                width: dm.dmPelsWidth,
                height: dm.dmPelsHeight,
                hz: dm.dmDisplayFrequency,
            };
            // Skip legacy/interlaced junk and anything too small to be useful.
            if m.width < 1024 || m.height < 600 || m.hz < 24 {
                continue;
            }
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    // Widest first, then tallest, then highest refresh.
    out.sort_by(|a, b| {
        b.width
            .cmp(&a.width)
            .then(b.height.cmp(&a.height))
            .then(b.hz.cmp(&a.hz))
    });
    out
}

/// The triple-screen spans this tool supports. With NVIDIA Surround on, Windows presents the
/// whole span as a single display, so these are the widths it reports.
pub const TRIPLE_RESOLUTIONS: [(u32, u32); 4] = [
    (7680, 1440),
    (10320, 1440),
    (15360, 2160),
    (23040, 2160),
];

/// Refresh rates offered in the picker.
pub const REFRESH_RATES: [u32; 15] = [
    30, 60, 75, 90, 100, 120, 144, 165, 180, 200, 240, 280, 330, 360, 500,
];

/// The supported triple-screen spans, widest first.
pub fn resolutions() -> Vec<(u32, u32)> {
    let mut out = TRIPLE_RESOLUTIONS.to_vec();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    out
}

/// Refresh rates offered for a resolution, highest first. The list is fixed; whether a given
/// combination is actually drivable is decided by `test()` before anything is applied.
pub fn rates_for(_width: u32, _height: u32) -> Vec<u32> {
    let mut out = REFRESH_RATES.to_vec();
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
}

/// Is this mode actually drivable? Uses CDS_TEST, which validates WITHOUT touching the screen.
/// This is the guard against picking a resolution/refresh the monitors cannot show (black screen).
pub fn test(m: DisplayMode) -> bool {
    unsafe {
        let mut dm: DEVMODEW = zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        dm.dmPelsWidth = m.width;
        dm.dmPelsHeight = m.height;
        dm.dmDisplayFrequency = m.hz;
        dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        ChangeDisplaySettingsExW(
            null(),
            &dm,
            std::ptr::null_mut(),
            CDS_TEST,
            std::ptr::null(),
        ) == DISP_CHANGE_SUCCESSFUL
    }
}

/// Apply a mode to the primary display and persist it to the registry.
pub fn apply(m: DisplayMode) -> Result<(), String> {
    unsafe {
        let mut dm: DEVMODEW = zeroed();
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        dm.dmPelsWidth = m.width;
        dm.dmPelsHeight = m.height;
        dm.dmDisplayFrequency = m.hz;
        dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
        let rc = ChangeDisplaySettingsExW(null(), &dm, std::ptr::null_mut(), CDS_UPDATEREGISTRY, std::ptr::null());
        if rc == DISP_CHANGE_SUCCESSFUL {
            Ok(())
        } else {
            Err(format!("could not apply that mode (code {rc})"))
        }
    }
}
