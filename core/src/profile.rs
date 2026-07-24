//! The known-good display profile: the config the user confirmed they want. Serialized as raw
//! POD struct bytes with a magic + version + per-array size guard so a truncated or version-
//! mismatched file fails cleanly instead of feeding a corrupt config to SetDisplayConfig.

use std::mem::size_of;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Devices::Display::{DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO};

use crate::ccd::Snapshot;

const MAGIC: u32 = 0x4C44_4631; // "LDF1"
const VERSION: u32 = 1;

/// C:\ProgramData\Lunis\DisplayFixer\profile.bin - machine-wide, survives reboots.
pub fn default_path() -> PathBuf {
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".into());
    Path::new(&base).join("Lunis").join("DisplayFixer").join("profile.bin")
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}

pub fn save(s: &Snapshot, path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, to_bytes(s)).map_err(|e| e.to_string())
}

pub fn load(path: &Path) -> Result<Snapshot, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    from_bytes(&data)
}

fn to_bytes(s: &Snapshot) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    write_arr(&mut out, &s.paths);
    write_arr(&mut out, &s.modes);
    out
}

fn from_bytes(data: &[u8]) -> Result<Snapshot, String> {
    let mut p = 0usize;
    if read_u32(data, &mut p)? != MAGIC {
        return Err("not a Lunis display profile".into());
    }
    let ver = read_u32(data, &mut p)?;
    if ver != VERSION {
        return Err(format!("unsupported profile version {ver}"));
    }
    let paths = read_arr::<DISPLAYCONFIG_PATH_INFO>(data, &mut p)?;
    let modes = read_arr::<DISPLAYCONFIG_MODE_INFO>(data, &mut p)?;
    Ok(Snapshot { paths, modes })
}

fn write_arr<T: Copy>(out: &mut Vec<u8>, arr: &[T]) {
    out.extend_from_slice(&(arr.len() as u32).to_le_bytes());
    out.extend_from_slice(&(size_of::<T>() as u32).to_le_bytes());
    let bytes =
        unsafe { std::slice::from_raw_parts(arr.as_ptr() as *const u8, std::mem::size_of_val(arr)) };
    out.extend_from_slice(bytes);
}

fn read_u32(data: &[u8], p: &mut usize) -> Result<u32, String> {
    if *p + 4 > data.len() {
        return Err("profile truncated".into());
    }
    let v = u32::from_le_bytes(data[*p..*p + 4].try_into().unwrap());
    *p += 4;
    Ok(v)
}

fn read_arr<T: Copy>(data: &[u8], p: &mut usize) -> Result<Vec<T>, String> {
    let count = read_u32(data, p)? as usize;
    let size = read_u32(data, p)? as usize;
    let expected = size_of::<T>();
    if size != expected {
        return Err(format!("profile format mismatch: element size {size} != {expected}"));
    }
    let need = count.checked_mul(size).ok_or("profile element count overflow")?;
    if *p + need > data.len() {
        return Err("profile truncated".into());
    }
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        let off = *p + i * size;
        let mut item = std::mem::MaybeUninit::<T>::uninit();
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr().add(off),
                item.as_mut_ptr() as *mut u8,
                size,
            );
            v.push(item.assume_init());
        }
    }
    *p += need;
    Ok(v)
}
