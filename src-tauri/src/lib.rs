//! Lunis Display Fixer - Tauri app layer over the native display-core engine.
//!
//! The GUI (this) handles capture / status / one-click fix / updates. The actual silent fixing at
//! logon is an ELEVATED scheduled task running `<exe> --fix` (the adapter restart needs admin, so
//! we don't UAC-prompt every boot - the task carries the elevation, registered once at setup).

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;
use tauri::Manager;

use lunis_display_core::{ccd, engine, profile};

const TASK_NAME: &str = "LunisDisplayFixer";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn profile_path() -> PathBuf {
    profile::default_path()
}

fn log_path() -> PathBuf {
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".into());
    PathBuf::from(base).join("Lunis").join("DisplayFixer").join("fixer.log")
}

fn append_log(line: &str) {
    let p = log_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
    }
}

// ---------- Tauri commands ----------

#[derive(Serialize)]
struct StatusDto {
    current_width: u32,
    target_width: Option<u32>,
    has_profile: bool,
    matches: bool,
    autofix_enabled: bool,
    adapters: Vec<String>,
    summary: String,
}

#[tauri::command]
fn get_status() -> StatusDto {
    let s = engine::status(&profile_path());
    StatusDto {
        current_width: s.current_width,
        target_width: s.target_width,
        has_profile: s.has_profile,
        matches: s.matches,
        autofix_enabled: task_exists(),
        adapters: s.adapters,
        summary: s.summary,
    }
}

#[tauri::command]
fn capture_profile() -> Result<String, String> {
    let snap = ccd::query()?;
    profile::save(&snap, &profile_path())?;
    append_log(&format!("captured target ({}px)", snap.max_source_width()));
    Ok(format!(
        "Saved your current display ({}px) as the target.",
        snap.max_source_width()
    ))
}

#[tauri::command]
fn fix_now() -> Result<String, String> {
    // The adapter-restart step needs admin, so run the headless --fix elevated.
    let code = elevate_and_wait("--fix")?;
    Ok(format!("Fix finished (exit {code})."))
}

#[tauri::command]
fn set_autofix(enabled: bool) -> Result<bool, String> {
    let arg = if enabled { "--install-task" } else { "--remove-task" };
    elevate_and_wait(arg)?;
    Ok(task_exists())
}

#[derive(Serialize)]
struct UpdateDto {
    available: bool,
    version: String,
    notes: String,
}

#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<UpdateDto, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(u)) => Ok(UpdateDto {
            available: true,
            version: u.version.clone(),
            notes: u.body.clone().unwrap_or_default(),
        }),
        Ok(None) => Ok(UpdateDto { available: false, version: String::new(), notes: String::new() }),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    if let Some(update) = updater.check().await.map_err(|e| e.to_string())? {
        update
            .download_and_install(|_chunk: usize, _total: Option<u64>| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

// ---------- entry points ----------

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_status,
            capture_profile,
            fix_now,
            set_autofix,
            check_update,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lunis Display Fixer");
}

/// Headless fix, run by the elevated logon task (or by `fix_now`'s elevated relaunch).
pub fn run_fix_headless() {
    append_log("=== --fix ===");
    let r = engine::fix(&profile_path(), append_log);
    append_log(&format!("[{:?}] {}", r.outcome, r.message));
}

pub fn install_task() {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let me = whoami();
    let script = format!(
        "$a=New-ScheduledTaskAction -Execute '{exe}' -Argument '--fix';\
         $t=New-ScheduledTaskTrigger -AtLogOn -User '{me}';$t.Delay='PT40S';\
         $p=New-ScheduledTaskPrincipal -UserId '{me}' -LogonType Interactive -RunLevel Highest;\
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 3);\
         Register-ScheduledTask -TaskName '{TASK_NAME}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null"
    );
    let out = run_ps(&script);
    append_log(&format!("install_task: {out}"));
}

pub fn remove_task() {
    let out = run_ps(&format!(
        "Unregister-ScheduledTask -TaskName '{TASK_NAME}' -Confirm:$false -ErrorAction SilentlyContinue"
    ));
    append_log(&format!("remove_task: {out}"));
}

// ---------- helpers ----------

fn task_exists() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn whoami() -> String {
    let d = std::env::var("USERDOMAIN").unwrap_or_default();
    let u = std::env::var("USERNAME").unwrap_or_default();
    if d.is_empty() {
        u
    } else {
        format!("{d}\\{u}")
    }
}

fn run_ps(script: &str) -> String {
    match Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => {
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            )
            .trim()
            .to_string()
        }
        Err(e) => e.to_string(),
    }
}

/// Relaunch this exe elevated with `arg`, hidden, and wait for it. Returns the child exit code.
fn elevate_and_wait(arg: &str) -> Result<u32, String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let verb = to_wide("runas");
    let file = to_wide(&exe.display().to_string());
    let params = to_wide(arg);

    unsafe {
        let mut sei: SHELLEXECUTEINFOW = std::mem::zeroed();
        sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = SEE_MASK_NOCLOSEPROCESS;
        sei.lpVerb = verb.as_ptr();
        sei.lpFile = file.as_ptr();
        sei.lpParameters = params.as_ptr();
        sei.nShow = SW_HIDE;
        if ShellExecuteExW(&mut sei) == 0 {
            return Err("elevation was cancelled or failed".into());
        }
        if sei.hProcess.is_null() {
            return Ok(0);
        }
        WaitForSingleObject(sei.hProcess, u32::MAX);
        let mut code: u32 = 0;
        GetExitCodeProcess(sei.hProcess, &mut code);
        CloseHandle(sei.hProcess);
        Ok(code)
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
