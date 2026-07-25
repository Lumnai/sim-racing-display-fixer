#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

mod http;
mod updater;

use lunis_display_core::{ccd, engine, modes, profile};
use slint::{ModelRc, Timer, TimerMode, VecModel};

slint::include_modules!();

const TASK_NAME: &str = "SimDisplayFixer";
const RUN_VALUE: &str = "SimDisplayFixer";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SITE_URL: &str = "https://lunis.live";
const DOCS_URL: &str = "https://github.com/Lumnai/sim-racing-display-fixer#readme";
const CUSTOM_LABEL: &str = "Custom...";
const REVERT_SECS: i32 = 12;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if has("--fix") {
        run_fix_headless();
        return;
    }
    if has("--install-task") {
        install_task();
        return;
    }
    if has("--remove-task") {
        remove_task();
        return;
    }

    // One window only: a second launch focuses the first. If the lock is held but no window can
    // be found (e.g. the previous instance is still shutting down), fall through and start
    // normally rather than exiting silently - otherwise the app just fails to open.
    if !claim_single_instance() && focus_existing() {
        return;
    }

    run_gui(has("--hidden"));
}

/// UI state for the resolution / refresh pickers.
struct Sel {
    resolutions: Vec<(u32, u32)>,
    res_idx: usize,
    rates: Vec<u32>,
    hz_idx: usize,
    custom: bool,
    /// mode active before the last Apply, for the revert safety net
    previous: Option<modes::DisplayMode>,
}

impl Sel {
    fn chosen(&self, ui: &AppWindow) -> Option<modes::DisplayMode> {
        if self.custom {
            let w: u32 = ui.get_custom_w().trim().parse().ok()?;
            let h: u32 = ui.get_custom_h().trim().parse().ok()?;
            let hz: u32 = ui.get_custom_hz().trim().parse().ok()?;
            // Deliberately permissive: any positive numbers are offerable. Whether the monitors
            // can actually drive it is decided by the CDS_TEST check at Apply time.
            if w == 0 || h == 0 || hz == 0 {
                return None;
            }
            Some(modes::DisplayMode { width: w, height: h, hz })
        } else {
            let (w, h) = *self.resolutions.get(self.res_idx)?;
            let hz = *self.rates.get(self.hz_idx)?;
            Some(modes::DisplayMode { width: w, height: h, hz })
        }
    }
}

fn run_gui(hidden: bool) {
    let ui = AppWindow::new().expect("failed to create window");
    std::thread::spawn(set_dark_titlebar);
    if hidden {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(400));
            minimize_self();
        });
    }

    let sel = Arc::new(Mutex::new(Sel {
        resolutions: modes::resolutions(),
        res_idx: 0,
        rates: Vec::new(),
        hz_idx: 0,
        custom: false,
        previous: None,
    }));

    // start on whatever the display is running now
    if let Some(cur) = modes::current() {
        let mut s = sel.lock().unwrap();
        if let Some(i) = s.resolutions.iter().position(|r| *r == (cur.width, cur.height)) {
            s.res_idx = i;
        }
        let (w, h) = s.resolutions.get(s.res_idx).copied().unwrap_or((cur.width, cur.height));
        s.rates = modes::rates_for(w, h);
        s.hz_idx = s.rates.iter().position(|z| *z == cur.hz).unwrap_or(0);
    }

    // First run: adopt whatever the display is on now as the target, so the startup auto-fix has
    // something to protect from day one. After that the target only changes when a mode is kept.
    if profile::load(&profile_path()).is_err() {
        save_target();
    }

    sync_pickers(&ui, &sel.lock().unwrap());
    refresh(&ui, &sel.lock().unwrap());
    refresh_toggles(&ui);

    // window chrome
    ui.on_close_app(|| std::process::exit(0));
    ui.on_minimize_app(minimize_self);
    ui.on_start_drag(drag_self);
    ui.on_open_site(|| open_url(SITE_URL));
    ui.on_open_docs(|| open_url(DOCS_URL));

    // Check for a newer release in the background so the window never blocks on the network.
    {
        let w = ui.as_weak();
        std::thread::spawn(move || {
            if let Ok(Some(a)) = updater::check_if_due() {
                let v = a.version.clone();
                let _ = w.upgrade_in_event_loop(move |ui| {
                    ui.set_update_version(v.into());
                    ui.set_update_available(true);
                });
            }
            // The TLS stack and root certificates are only needed for this one request;
            // give the pages back rather than holding them for the life of the window.
            trim_memory();
        });
    }

    // Download + verify + run the installer, also off the UI thread.
    {
        let w = ui.as_weak();
        ui.on_install_update(move || {
            let ui = w.unwrap();
            ui.set_busy(true);
            ui.set_busy_text("Downloading update...".into());
            let w2 = ui.as_weak();
            std::thread::spawn(move || {
                let outcome = match updater::check() {
                    Ok(Some(a)) => updater::download_and_run(&a).map(|_| true),
                    Ok(None) => Ok(false),
                    Err(e) => Err(e),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = w2.upgrade() else { return };
                    ui.set_busy(false);
                    match outcome {
                        Ok(true) => std::process::exit(0), // installer takes over
                        Ok(false) => ui.set_update_available(false),
                        Err(e) => {
                            ui.set_status_text(e.into());
                            ui.set_status_color(slint::Color::from_rgb_u8(253, 96, 16));
                        }
                    }
                });
            });
        });
    }

    // resolution picked (last entry is "Custom...")
    {
        let w = ui.as_weak();
        let sel = sel.clone();
        ui.on_pick_res(move |i| {
            let ui = w.unwrap();
            {
                let mut s = sel.lock().unwrap();
                let i = i.max(0) as usize;
                if i >= s.resolutions.len() {
                    s.custom = true;
                } else {
                    s.custom = false;
                    s.res_idx = i;
                    let (rw, rh) = s.resolutions[i];
                    s.rates = modes::rates_for(rw, rh);
                    s.hz_idx = 0;
                }
            }
            sync_pickers(&ui, &sel.lock().unwrap());
            refresh(&ui, &sel.lock().unwrap());
        });
    }

    {
        let w = ui.as_weak();
        let sel = sel.clone();
        ui.on_pick_hz(move |i| {
            let ui = w.unwrap();
            {
                let mut s = sel.lock().unwrap();
                let i = i.max(0) as usize;
                if i < s.rates.len() {
                    s.hz_idx = i;
                }
            }
            sync_pickers(&ui, &sel.lock().unwrap());
            refresh(&ui, &sel.lock().unwrap());
        });
    }

    // typing in the custom width/height/Hz fields must re-evaluate whether Apply is available
    {
        let w = ui.as_weak();
        let sel = sel.clone();
        ui.on_custom_changed(move || {
            let ui = w.unwrap();
            let s = sel.lock().unwrap();
            refresh(&ui, &s);
        });
    }

    // Apply: validate first (never blind-apply a mode that could black the screen),
    // then hold a keep-or-revert prompt.
    let revert_timer = Rc::new(Timer::default());
    {
        let w = ui.as_weak();
        let sel = sel.clone();
        let timer = revert_timer.clone();
        ui.on_apply_mode(move || {
            let ui = w.unwrap();
            let target = sel.lock().unwrap().chosen(&ui);
            let Some(target) = target else {
                ui.set_status_text("Enter a valid width, height and refresh rate.".into());
                ui.set_status_color(slint::Color::from_rgb_u8(253, 96, 16));
                return;
            };
            if !modes::test(target) {
                // Be specific: values under the Windows floor are never acceptable, which is a
                // different problem from "this monitor cannot drive that mode".
                let msg = if target.width < 640 || target.height < 480 {
                    "Windows does not accept anything smaller than 640 x 480.".to_string()
                } else if target.hz < 23 {
                    "Windows does not accept refresh rates below about 24 Hz.".to_string()
                } else {
                    // Windows can only switch to modes the GPU driver already publishes. Anything
                    // else has to be created as a custom resolution in the driver's own panel.
                    format!(
                        "Your graphics driver does not offer {} x {} at {} Hz. Add it as a custom \
                         resolution in your NVIDIA or AMD control panel first, then pick it here.",
                        target.width, target.height, target.hz
                    )
                };
                ui.set_status_text(msg.into());
                ui.set_status_color(slint::Color::from_rgb_u8(253, 96, 16));
                return;
            }
            let previous = modes::current();
            if let Err(e) = modes::apply(target) {
                ui.set_status_text(e.into());
                ui.set_status_color(slint::Color::from_rgb_u8(253, 96, 16));
                return;
            }
            sel.lock().unwrap().previous = previous;

            // countdown; if the screen went black the user simply waits and it reverts
            ui.set_revert_secs(REVERT_SECS);
            ui.set_confirming(true);
            let w2 = ui.as_weak();
            let sel2 = sel.clone();
            timer.start(
                TimerMode::Repeated,
                std::time::Duration::from_secs(1),
                move || {
                    let Some(ui) = w2.upgrade() else { return };
                    let left = ui.get_revert_secs() - 1;
                    ui.set_revert_secs(left);
                    if left <= 0 {
                        let prev = sel2.lock().unwrap().previous;
                        if let Some(p) = prev {
                            let _ = modes::apply(p);
                        }
                        ui.set_confirming(false);
                        resync(&ui, &sel2);
                    }
                },
            );
        });
    }

    {
        let w = ui.as_weak();
        let sel = sel.clone();
        let timer = revert_timer.clone();
        ui.on_keep_settings(move || {
            let ui = w.unwrap();
            timer.stop();
            ui.set_confirming(false);
            // Keeping a mode IS the act of choosing it: record it as the target the startup
            // auto-fix restores. Without this nothing ever writes a profile and --fix no-ops.
            save_target();
            resync(&ui, &sel);
        });
    }
    {
        let w = ui.as_weak();
        let sel = sel.clone();
        let timer = revert_timer.clone();
        ui.on_revert_settings(move || {
            let ui = w.unwrap();
            timer.stop();
            let prev = sel.lock().unwrap().previous;
            if let Some(p) = prev {
                let _ = modes::apply(p);
            }
            ui.set_confirming(false);
            resync(&ui, &sel);
        });
    }

    // heal - elevated, off the UI thread
    {
        let w = ui.as_weak();
        let sel = sel.clone();
        ui.on_fix(move || {
            let ui = w.unwrap();
            ui.set_busy(true);
            ui.set_busy_text("Fixing... screens may blink".into());
            let w2 = ui.as_weak();
            let sel2 = sel.clone();
            std::thread::spawn(move || {
                let _ = elevate_and_wait("--fix");
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = w2.upgrade() else { return };
                    ui.set_busy(false);
                    resync(&ui, &sel2);
                });
            });
        });
    }

    // auto-fix toggle - elevation MUST run off the UI thread or the window freezes
    {
        let w = ui.as_weak();
        let sel = sel.clone();
        ui.on_set_autofix(move |enable| {
            let ui = w.unwrap();
            ui.set_busy(true);
            ui.set_busy_text(if enable { "Turning on auto-fix...".into() } else { slint::SharedString::from("Turning off auto-fix...") });
            let w2 = ui.as_weak();
            let sel2 = sel.clone();
            std::thread::spawn(move || {
                let _ = elevate_and_wait(if enable { "--install-task" } else { "--remove-task" });
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = w2.upgrade() else { return };
                    ui.set_busy(false);
                    refresh_toggles(&ui);
                    resync(&ui, &sel2);
                });
            });
        });
    }

    // hide on startup (per-user Run entry; no elevation needed)
    {
        let w = ui.as_weak();
        let sel = sel.clone();
        ui.on_set_hide_startup(move |enable| {
            let ui = w.unwrap();
            ui.set_hide_on_startup(enable); // move the switch immediately
            set_hide_on_startup(enable);
            ui.set_hide_on_startup(hide_on_startup_enabled());
            resync(&ui, &sel);
        });
    }

    // Hand back pages that startup touched, then keep trimming periodically so the footprint
    // does not creep while the window sits open.
    {
        let trim = Timer::default();
        trim.start(TimerMode::Repeated, std::time::Duration::from_secs(15), trim_memory);
        std::mem::forget(trim);
    }

    ui.run().expect("event loop failed");
}

fn resync(ui: &AppWindow, sel: &Arc<Mutex<Sel>>) {
    let s = sel.lock().unwrap();
    sync_pickers(ui, &s);
    refresh(ui, &s);
}

/// Read the two startup-related toggle states. Costs a process spawn + registry read, so it is
/// only called at launch and right after a toggle finishes - never on the interaction path.
fn refresh_toggles(ui: &AppWindow) {
    ui.set_autofix(task_exists());
    ui.set_hide_on_startup(hide_on_startup_enabled());
}

fn sync_pickers(ui: &AppWindow, s: &Sel) {
    let mut res_labels: Vec<slint::SharedString> = s
        .resolutions
        .iter()
        .map(|(w, h)| slint::SharedString::from(format!("{w} x {h}")))
        .collect();
    res_labels.push(CUSTOM_LABEL.into());
    let hz_labels: Vec<slint::SharedString> = s
        .rates
        .iter()
        .map(|z| slint::SharedString::from(format!("{z} Hz")))
        .collect();

    let sel_res = if s.custom {
        slint::SharedString::from(CUSTOM_LABEL)
    } else {
        res_labels
            .get(s.res_idx)
            .cloned()
            .unwrap_or_else(|| "-".into())
    };
    let sel_hz = if s.custom {
        slint::SharedString::from("custom")
    } else {
        hz_labels.get(s.hz_idx).cloned().unwrap_or_else(|| "-".into())
    };

    ui.set_res_options(ModelRc::new(VecModel::from(res_labels)));
    ui.set_hz_options(ModelRc::new(VecModel::from(hz_labels)));
    ui.set_selected_res(sel_res);
    ui.set_selected_hz(sel_hz);
    ui.set_custom_mode(s.custom);
}

fn refresh(ui: &AppWindow, s: &Sel) {
    let cur = modes::current();
    ui.set_current_mode(
        cur.map(|m| m.label())
            .unwrap_or_else(|| "unknown".into())
            .into(),
    );

    // Apply is pointless when the pick already matches what is on screen.
    let chosen = s.chosen(ui);
    let same = matches!((chosen, cur), (Some(a), Some(b)) if a == b);
    ui.set_can_apply(chosen.is_some() && !same);

    // Fix now only appears when the live display has drifted from the saved target.
    let st = engine::status(&profile_path());
    ui.set_show_fix(st.has_profile && !st.matches && st.current_width > 0);

    // NOTE: the toggle states are deliberately NOT queried here. Both cost a process spawn
    // (schtasks / registry) and refresh() runs on every interaction. They are read once at
    // startup and re-read only after a toggle actually changes something.

    let (text, color) = if let Some(c) = cur {
        if st.has_profile && !st.matches {
            (
                format!("Now {}. This does not match your saved target.", c.label()),
                slint::Color::from_rgb_u8(253, 96, 16),
            )
        } else if same {
            (
                format!("Now {}. This is your current display.", c.label()),
                slint::Color::from_rgb_u8(165, 162, 160),
            )
        } else {
            (
                format!("Now {}.", c.label()),
                slint::Color::from_rgb_u8(165, 162, 160),
            )
        }
    } else {
        (
            "Could not read the display.".to_string(),
            slint::Color::from_rgb_u8(253, 96, 16),
        )
    };
    ui.set_status_text(text.into());
    ui.set_status_color(color);
}

// ---------- headless entry points ----------

fn run_fix_headless() {
    append_log("=== --fix ===");
    // Fire as early as possible after logon, but the graphics stack may not be up yet. Poll for a
    // readable display instead of padding the trigger with a long fixed delay - that way the fix
    // starts the moment it can, and a slow boot still works.
    for i in 0..30 {
        if modes::current().is_some() {
            if i > 0 {
                append_log(&format!("display ready after {}ms", i * 500));
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let r = engine::fix(&profile_path(), append_log);
    append_log(&format!("[{:?}] {}", r.outcome, r.message));
}

fn install_task() {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let me = whoami();
    let script = format!(
        "$a=New-ScheduledTaskAction -Execute '{exe}' -Argument '--fix';\
         $t=New-ScheduledTaskTrigger -AtLogOn -User '{me}';$t.Delay='PT3S';\
         $p=New-ScheduledTaskPrincipal -UserId '{me}' -LogonType Interactive -RunLevel Highest;\
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 3);\
         Register-ScheduledTask -TaskName '{TASK_NAME}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null"
    );
    let out = run_ps(&script);
    append_log(&format!("install_task: {out}"));
}

fn remove_task() {
    let out = run_ps(&format!(
        "Unregister-ScheduledTask -TaskName '{TASK_NAME}' -Confirm:$false -ErrorAction SilentlyContinue"
    ));
    append_log(&format!("remove_task: {out}"));
}

// ---------- settings ----------

// Direct registry access. These used to shell out to PowerShell, which cost ~1s per call and
// made the whole window feel sluggish because they ran on the UI thread.
const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn hide_on_startup_enabled() -> bool {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ,
    };
    let sub = to_wide(RUN_SUBKEY);
    let name = to_wide(RUN_VALUE);
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return false;
        }
        let mut size: u32 = 0;
        let found = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        ) == 0;
        RegCloseKey(key);
        found
    }
}

fn set_hide_on_startup(enable: bool) {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    let sub = to_wide(RUN_SUBKEY);
    let name = to_wide(RUN_VALUE);
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != 0
        {
            return;
        }
        if enable {
            let exe = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let value = to_wide(&format!("\"{exe}\" --hidden"));
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr() as *const u8,
                (value.len() * 2) as u32,
            );
        } else {
            RegDeleteValueW(key, name.as_ptr());
        }
        RegCloseKey(key);
    }
}

// ---------- helpers ----------

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
    if d.is_empty() { u } else { format!("{d}\\{u}") }
}

fn run_ps(script: &str) -> String {
    match Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
        .trim()
        .to_string(),
        Err(e) => e.to_string(),
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------- win32 ----------

fn claim_single_instance() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name = to_wide("Global\\SimDisplayFixerSingleInstance");
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        if h.is_null() {
            return true;
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Focus an already-running instance. Returns false if no such window exists.
fn focus_existing() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    static FOUND: AtomicBool = AtomicBool::new(false);
    unsafe extern "system" fn cb(hwnd: HWND, _l: LPARAM) -> i32 {
        let mut buf = [0u16; 128];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), 128);
        if n > 0 && IsWindowVisible(hwnd) != 0 {
            let t = String::from_utf16_lossy(&buf[..n as usize]);
            if t == "Sim Display Fixer" {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
                FOUND.store(true, Ordering::SeqCst);
                return 0;
            }
        }
        1
    }
    FOUND.store(false, Ordering::SeqCst);
    unsafe { EnumWindows(Some(cb), 0) };
    FOUND.load(Ordering::SeqCst)
}

fn self_hwnd() -> windows_sys::Win32::Foundation::HWND {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };
    static FOUND: AtomicIsize = AtomicIsize::new(0);
    unsafe extern "system" fn cb(hwnd: HWND, _l: LPARAM) -> i32 {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == GetCurrentProcessId() && IsWindowVisible(hwnd) != 0 {
            FOUND.store(hwnd as isize, Ordering::SeqCst);
            return 0;
        }
        1
    }
    FOUND.store(0, Ordering::SeqCst);
    unsafe { EnumWindows(Some(cb), 0) };
    FOUND.load(Ordering::SeqCst) as HWND
}

fn minimize_self() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MINIMIZE};
    let hwnd = self_hwnd();
    if !hwnd.is_null() {
        unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
    }
    // Nothing needs to stay resident while hidden - hand the pages back.
    trim_memory();
}

/// Release resident pages back to Windows. Anything still needed is faulted back in on demand;
/// this is what keeps a background utility from sitting on tens of MB it is not using.
fn trim_memory() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Drag the frameless window by its header. The trailing WM_LBUTTONUP is essential: the
/// Win32 move loop swallows the real button-up, and without it the UI keeps thinking the
/// mouse is held down and stops responding to clicks.
fn drag_self() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PostMessageW, SendMessageW, HTCAPTION, WM_LBUTTONUP, WM_NCLBUTTONDOWN,
    };
    let hwnd = self_hwnd();
    if !hwnd.is_null() {
        unsafe {
            ReleaseCapture();
            SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, 0);
            PostMessageW(hwnd, WM_LBUTTONUP, 0, 0);
        }
    }
}

fn open_url(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let op = to_wide("open");
    let file = to_wide(url);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

fn set_dark_titlebar() {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    for _ in 0..30 {
        let hwnd = self_hwnd();
        if !hwnd.is_null() {
            let on: i32 = 1;
            unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE,
                    &on as *const i32 as *const core::ffi::c_void,
                    4,
                );
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}

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

/// Snapshot the live display config as the target the startup auto-fix restores.
fn save_target() {
    if let Ok(snap) = ccd::query() {
        let _ = profile::save(&snap, &profile_path());
    }
}
