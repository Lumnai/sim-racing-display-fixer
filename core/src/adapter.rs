//! Native NVIDIA/display-adapter restart via SetupAPI (DIF_PROPERTYCHANGE) - the proven heal for
//! a collapsed Surround span that CCD can't re-apply. Disabling + re-enabling the adapter forces a
//! full driver re-init + link re-train + internal grid re-apply. Vendor-neutral mechanism.
//! Requires elevation.

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiCallClassInstaller, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
    SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW, SetupDiSetClassInstallParamsW,
    DIGCF_PRESENT, HDEVINFO, SP_CLASSINSTALL_HEADER, SP_DEVINFO_DATA, SP_PROPCHANGE_PARAMS,
};
use windows_sys::Win32::Foundation::GetLastError;

// {4d36e968-e325-11ce-bfc1-08002be10318} = GUID_DEVCLASS_DISPLAY
const GUID_DEVCLASS_DISPLAY: GUID = GUID {
    data1: 0x4d36_e968,
    data2: 0xe325,
    data3: 0x11ce,
    data4: [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18],
};
const DICS_ENABLE: u32 = 0x0000_0001;
const DICS_DISABLE: u32 = 0x0000_0002;
const DICS_FLAG_GLOBAL: u32 = 0x0000_0001;
const DIF_PROPERTYCHANGE: u32 = 0x0000_0012;
const SPDRP_DEVICEDESC: u32 = 0x0000_0000;
const SPDRP_FRIENDLYNAME: u32 = 0x0000_000C;

/// Friendly names of every display-class adapter on the machine.
pub fn list() -> Result<Vec<String>, String> {
    unsafe {
        let mut guid = GUID_DEVCLASS_DISPLAY;
        let set = SetupDiGetClassDevsW(&mut guid, null(), null_mut(), DIGCF_PRESENT);
        if set as isize == -1 {
            return Err("SetupDiGetClassDevs failed".into());
        }
        let mut names = Vec::new();
        let mut i = 0u32;
        loop {
            let mut data: SP_DEVINFO_DATA = zeroed();
            data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, i, &mut data) == 0 {
                break;
            }
            i += 1;
            names.push(get_name(set, &data));
        }
        SetupDiDestroyDeviceInfoList(set);
        Ok(names)
    }
}

/// Restart (disable, settle, re-enable) every display adapter whose name contains `filter`
/// (case-insensitive), or all of them if `filter` is None. Re-enable is guaranteed even if the
/// disable loop or an individual enable throws - never leave an adapter disabled.
pub fn restart(filter: Option<&str>, mut log: impl FnMut(&str)) -> Result<usize, String> {
    unsafe {
        let mut guid = GUID_DEVCLASS_DISPLAY;
        let set = SetupDiGetClassDevsW(&mut guid, null(), null_mut(), DIGCF_PRESENT);
        if set as isize == -1 {
            return Err("SetupDiGetClassDevs failed".into());
        }
        let result = restart_inner(set, filter, &mut log);
        SetupDiDestroyDeviceInfoList(set);
        result
    }
}

unsafe fn restart_inner(
    set: HDEVINFO,
    filter: Option<&str>,
    log: &mut impl FnMut(&str),
) -> Result<usize, String> {
    let filter_lc = filter.map(|f| f.to_lowercase());
    let mut targets: Vec<SP_DEVINFO_DATA> = Vec::new();
    let mut i = 0u32;
    loop {
        let mut data: SP_DEVINFO_DATA = zeroed();
        data.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
        if SetupDiEnumDeviceInfo(set, i, &mut data) == 0 {
            break;
        }
        i += 1;
        let name = get_name(set, &data);
        let want = match &filter_lc {
            Some(f) => name.to_lowercase().contains(f.as_str()),
            None => true,
        };
        if want {
            log(&format!("adapter: {name}"));
            targets.push(data);
        }
    }

    if targets.is_empty() {
        return Ok(0);
    }

    // Disable all, then ALWAYS re-enable all (per-adapter), even if a step fails.
    let mut disable_err: Option<String> = None;
    for t in &targets {
        if let Err(e) = set_state(set, t, DICS_DISABLE) {
            disable_err = Some(e);
            break;
        }
    }
    log("adapter(s) disabled");
    std::thread::sleep(std::time::Duration::from_millis(4000));
    for t in &targets {
        if let Err(e) = set_state(set, t, DICS_ENABLE) {
            log(&format!("re-enable failed for one adapter: {e}"));
        }
    }
    log("adapter(s) re-enabled");
    std::thread::sleep(std::time::Duration::from_millis(15000));

    match disable_err {
        Some(e) => Err(e),
        None => Ok(targets.len()),
    }
}

unsafe fn set_state(set: HDEVINFO, data: &SP_DEVINFO_DATA, state: u32) -> Result<(), String> {
    let mut pcp: SP_PROPCHANGE_PARAMS = zeroed();
    pcp.ClassInstallHeader.cbSize = size_of::<SP_CLASSINSTALL_HEADER>() as u32;
    pcp.ClassInstallHeader.InstallFunction = DIF_PROPERTYCHANGE;
    pcp.StateChange = state;
    pcp.Scope = DICS_FLAG_GLOBAL;
    pcp.HwProfile = 0;

    let hdr = &pcp as *const SP_PROPCHANGE_PARAMS as *const SP_CLASSINSTALL_HEADER;
    if SetupDiSetClassInstallParamsW(
        set,
        data as *const SP_DEVINFO_DATA,
        hdr,
        size_of::<SP_PROPCHANGE_PARAMS>() as u32,
    ) == 0
    {
        return Err(format!("SetupDiSetClassInstallParams (err {})", GetLastError()));
    }
    if SetupDiCallClassInstaller(DIF_PROPERTYCHANGE, set, data as *const SP_DEVINFO_DATA) == 0 {
        return Err(format!("SetupDiCallClassInstaller (err {})", GetLastError()));
    }
    Ok(())
}

unsafe fn get_name(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> String {
    read_prop(set, data, SPDRP_FRIENDLYNAME)
        .or_else(|| read_prop(set, data, SPDRP_DEVICEDESC))
        .unwrap_or_default()
}

unsafe fn read_prop(set: HDEVINFO, data: &SP_DEVINFO_DATA, prop: u32) -> Option<String> {
    let mut needed: u32 = 0;
    SetupDiGetDeviceRegistryPropertyW(
        set,
        data as *const SP_DEVINFO_DATA,
        prop,
        null_mut(),
        null_mut(),
        0,
        &mut needed,
    );
    if needed == 0 {
        return None;
    }
    let mut buf = vec![0u8; needed as usize];
    if SetupDiGetDeviceRegistryPropertyW(
        set,
        data as *const SP_DEVINFO_DATA,
        prop,
        null_mut(),
        buf.as_mut_ptr(),
        needed,
        null_mut(),
    ) == 0
    {
        return None;
    }
    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16_lossy(&u16s);
    let s = s.trim_end_matches('\0').trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
