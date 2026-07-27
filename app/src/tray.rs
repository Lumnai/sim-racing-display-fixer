//! A notification-area (system tray) icon.
//!
//! Runs its own message-only window on a dedicated thread, because the tray needs a window
//! procedure to receive click callbacks and Slint owns the main event loop. Clicking the icon
//! shows the app window; right-click offers Open and Quit.

use std::sync::mpsc::Sender;

use windows_sys::core::PCWSTR;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// What the tray asks the app to do.
pub enum TrayEvent {
    Show,
    Quit,
}

const WM_TRAY: u32 = WM_APP + 1;
const ID_OPEN: usize = 1;
const ID_QUIT: usize = 2;

static mut SENDER: Option<Sender<TrayEvent>> = None;
/// The tray window handle, so the icon can be removed on exit. Without this the icon lingers in
/// the notification area as a ghost until the user happens to hover over it.
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Remove the tray icon. Call before exiting.
pub fn remove() {
    use std::sync::atomic::Ordering;
    let hwnd = TRAY_HWND.swap(0, Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd as HWND;
        nid.uID = 1;
        Shell_NotifyIconW(NIM_DELETE, &mut nid);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Start the tray icon. Returns once the icon exists; it lives until the process exits.
pub fn start(tx: Sender<TrayEvent>) {
    std::thread::spawn(move || unsafe {
        SENDER = Some(tx);
        run();
    });
}

unsafe fn run() {
    let class = wide("LunisDisplayFixerTray");
    let hinst = GetModuleHandleW(std::ptr::null());

    let mut wc: WNDCLASSW = std::mem::zeroed();
    wc.lpfnWndProc = Some(wnd_proc);
    wc.hInstance = hinst;
    wc.lpszClassName = class.as_ptr();
    RegisterClassW(&wc);

    // A message-only window: never visible, exists purely to receive the tray callbacks.
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        wide("tray").as_ptr(),
        0,
        0,
        0,
        0,
        0,
        HWND_MESSAGE,
        std::ptr::null_mut(),
        hinst,
        std::ptr::null(),
    );
    if hwnd.is_null() {
        return;
    }

    // Icon 1 is the app icon embedded in the exe's resources. Ask for the small-icon metric rather
    // than using LoadIconW: that loads the 32x32 frame and leaves the shell to shrink it, whereas
    // the .ico carries real 16x16 and 20x20 frames that render cleanly at tray size.
    let icon = LoadImageW(
        hinst,
        1 as PCWSTR,
        IMAGE_ICON,
        GetSystemMetrics(SM_CXSMICON),
        GetSystemMetrics(SM_CYSMICON),
        LR_DEFAULTCOLOR,
    ) as HICON;

    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon;
    let tip = wide("Sim Display Fixer");
    for (i, c) in tip.iter().take(127).enumerate() {
        nid.szTip[i] = *c;
    }
    TRAY_HWND.store(hwnd as isize, std::sync::atomic::Ordering::SeqCst);
    Shell_NotifyIconW(NIM_ADD, &mut nid);

    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    Shell_NotifyIconW(NIM_DELETE, &mut nid);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = l as u32;
            if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                send(TrayEvent::Show);
            } else if event == WM_RBUTTONUP {
                show_menu(hwnd);
            }
            0
        }
        WM_COMMAND => {
            match w & 0xFFFF {
                ID_OPEN => send(TrayEvent::Show),
                ID_QUIT => send(TrayEvent::Quit),
                _ => {}
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    AppendMenuW(menu, MF_STRING, ID_OPEN, wide("Open").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(menu, MF_STRING, ID_QUIT, wide("Quit").as_ptr());

    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    // Required so the menu closes when the user clicks elsewhere.
    SetForegroundWindow(hwnd);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);
}

unsafe fn send(ev: TrayEvent) {
    if let Some(tx) = &*std::ptr::addr_of!(SENDER) {
        let _ = tx.send(ev);
    }
}
