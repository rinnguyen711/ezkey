//! ezkey for Windows: a tray app that rewrites keystrokes with a low-level
//! keyboard hook. A hotkey (Ctrl+Shift by default, user-definable) or a
//! click on the tray icon toggles Vietnamese / English, optionally
//! remembered per app.

#![windows_subsystem = "windows"]

mod apps;
mod hotkey;
mod startup;

use std::cell::RefCell;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use ezkey_core::{Action, Engine, InputMethod, ToneStyle};
use hotkey::{Event, Hotkey, Matcher};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Marks input we inject so the hook lets it through untouched.
const INJECTED_TAG: usize = 0x455A_4B59;
const WM_TRAY: u32 = WM_APP + 1;
const ID_TOGGLE: usize = 1;
const ID_MODERN: usize = 2;
const ID_EXIT: usize = 3;
const ID_TELEX: usize = 4;
const ID_VNI: usize = 5;
const ID_AUTOSTART: usize = 6;
const ID_ADMIN: usize = 7;
const ID_PER_APP: usize = 8;
const ID_HOTKEY_MENU: usize = 19;
const ID_HOTKEY_CTRL_SHIFT: usize = 20;
const ID_HOTKEY_ALT_Z: usize = 21;
const ID_HOTKEY_NONE: usize = 22;
const ID_HOTKEY_RECORD: usize = 23;
const ID_HOTKEY_CUSTOM: usize = 24;

/// Recording a new hotkey gives up after this long.
const TIMER_RECORD: usize = 1;
const RECORD_TIMEOUT_MS: u32 = 10_000;

const SETTINGS_KEY: &str = "Software\\ezkey";
const RUN_AS_ADMIN: &str = "RunAsAdmin";
const PER_APP: &str = "PerApp";
const HOTKEY: &str = "Hotkey";

/// Injected after a swallowed hotkey involving Alt or Win, so releasing
/// them does not open the app's menu bar or the Start menu (0xE8 is an
/// unassigned virtual key).
const VK_MENU_MASK: VIRTUAL_KEY = 0xE8;

/// Settings live in HKCU\Software\ezkey as DWORDs.
fn read_setting(name: &str) -> Option<bool> {
    read_number(name).map(|v| v != 0)
}

fn write_setting(name: &str, value: bool) {
    write_number(name, value as u32);
}

fn read_number(name: &str) -> Option<u32> {
    let (key, name) = (wide(SETTINGS_KEY), wide(name));
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

fn write_number(name: &str, data: u32) {
    let (key, name) = (wide(SETTINGS_KEY), wide(name));
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_DWORD,
            (&data as *const u32).cast(),
            size_of::<u32>() as u32,
        )
    };
}

fn save_settings(st: &State) {
    write_setting("Enabled", st.enabled);
    write_setting("ModernTone", st.engine.style() == ToneStyle::Modern);
    write_setting("Vni", st.engine.method() == InputMethod::Vni);
}

struct State {
    engine: Engine,
    enabled: bool,
    hwnd: HWND,
    foreground: HWND,
    /// Executable of the app the user is typing in; the taskbar and our own
    /// menu do not replace it (see `on_foreground`).
    app: Option<String>,
    /// That app is a browser (see `apps::is_browser`).
    browser: bool,
    /// Remember Vietnamese / English per app.
    per_app: bool,
    hotkeys: Matcher,
    icon_vi: HICON,
    icon_en: HICON,
    taskbar_created: u32,
    /// This process runs as administrator (it can type into elevated apps).
    elevated: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("state initialised")))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) < 0 }
}

fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_TAG,
            },
        },
    }
}

/// Typed first in browsers: it replaces any selected autocomplete suggestion
/// and is then erased with one extra Backspace.
const AUTOCOMPLETE_BREAKER: u16 = 0x202F;

fn send_replacement(backspaces: usize, text: &str, browser: bool) {
    let mut inputs = Vec::with_capacity(backspaces * 2 + text.len() * 2 + 4);
    let mut backspaces = backspaces;
    if browser && backspaces > 0 {
        inputs.push(key_input(0, AUTOCOMPLETE_BREAKER, KEYEVENTF_UNICODE));
        inputs.push(key_input(
            0,
            AUTOCOMPLETE_BREAKER,
            KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
        ));
        backspaces += 1;
    }
    for _ in 0..backspaces {
        inputs.push(key_input(VK_BACK, 0, 0));
        inputs.push(key_input(VK_BACK, 0, KEYEVENTF_KEYUP));
    }
    for unit in text.encode_utf16() {
        inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
        inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
}

fn send_menu_mask() {
    let inputs = [
        key_input(VK_MENU_MASK, 0, 0),
        key_input(VK_MENU_MASK, 0, KEYEVENTF_KEYUP),
    ];
    unsafe { SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32) };
}

enum KeyResult {
    Pass,
    /// Swallow the key and type a replacement.
    Replace(usize, String, bool),
    /// Toggle the mode, swallowing the key unless it is a modifier release.
    Toggle {
        swallow: bool,
        mask: bool,
    },
    /// Swallow the key (hotkey repeats, keys pressed while recording).
    Eat {
        mask: bool,
    },
}

/// Called when another window comes to the front: forgets the current word
/// and, with per-app memory, switches to the mode last used in that app.
fn on_foreground(st: &mut State, hwnd: HWND) {
    if hwnd == st.foreground {
        return;
    }
    st.foreground = hwnd;
    st.engine.reset();
    if hwnd.is_null() || apps::is_own_or_shell(hwnd) {
        return;
    }
    st.app = apps::exe_name(hwnd);
    st.browser = st.app.as_deref().is_some_and(apps::is_browser);
    let remembered = st
        .app
        .as_deref()
        .filter(|_| st.per_app)
        .and_then(apps::load);
    if let Some(enabled) = remembered.filter(|on| *on != st.enabled) {
        st.enabled = enabled;
        notify_tray(st, NIM_MODIFY);
    }
}

unsafe extern "system" fn foreground_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    with_state(|st| on_foreground(st, hwnd));
}

fn on_key(vk: VIRTUAL_KEY, down: bool, alt: bool) -> KeyResult {
    with_state(|st| {
        let event = st.hotkeys.on_key(vk, down);
        let mask = st.hotkeys.held() & (hotkey::ALT | hotkey::WIN) != 0;
        match event {
            Event::Pass => {}
            Event::Toggle { swallow } => {
                return KeyResult::Toggle {
                    swallow,
                    mask: swallow && mask,
                }
            }
            Event::Swallow => return KeyResult::Eat { mask: false },
            Event::Recorded(hotkey) => {
                finish_recording(st, Some(hotkey));
                return KeyResult::Eat { mask };
            }
            Event::Rejected => {
                show_balloon(
                    st,
                    "Không dùng được tổ hợp này",
                    "Phím này dùng để gõ chữ. Hãy thêm Ctrl, Alt hoặc Win, ví dụ Ctrl+Alt+V. Esc để huỷ.",
                );
                return KeyResult::Eat { mask };
            }
            Event::Cancelled => {
                finish_recording(st, None);
                return KeyResult::Eat { mask: false };
            }
        }
        if !down || hotkey::modifier_bit(vk).is_some() {
            return KeyResult::Pass;
        }

        // Backup for the foreground event hook, which arrives asynchronously.
        on_foreground(st, unsafe { GetForegroundWindow() });
        if !st.enabled {
            return KeyResult::Pass;
        }
        if alt || key_down(VK_CONTROL) || key_down(VK_LWIN) || key_down(VK_RWIN) {
            st.engine.reset();
            return KeyResult::Pass;
        }

        let shift = key_down(VK_SHIFT);
        let typed = match vk {
            0x41..=0x5A => {
                let caps = unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0;
                let upper = shift != caps;
                Some(if upper { vk as u8 } else { vk as u8 + 32 } as char)
            }
            // Digits matter for VNI; shifted they are symbols.
            0x30..=0x39 if !shift => Some(vk as u8 as char),
            VK_NUMPAD0..=VK_NUMPAD9 => Some((b'0' + (vk - VK_NUMPAD0) as u8) as char),
            _ => None,
        };

        match vk {
            _ if typed.is_some() => match st.engine.process_key(typed.unwrap_or_default()) {
                Action::PassThrough => KeyResult::Pass,
                Action::Replace { backspaces, text } => {
                    KeyResult::Replace(backspaces, text, st.browser)
                }
            },
            VK_BACK => {
                st.engine.backspace();
                KeyResult::Pass
            }
            VK_CAPITAL => KeyResult::Pass,
            _ => {
                st.engine.reset();
                KeyResult::Pass
            }
        }
    })
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        if kb.dwExtraInfo != INJECTED_TAG {
            let msg = wparam as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let alt = kb.flags & LLKHF_ALTDOWN != 0;
            // The state borrow ends inside on_key, before any input is sent
            // or any window message is handled.
            match on_key(kb.vkCode as VIRTUAL_KEY, down, alt) {
                KeyResult::Pass => {}
                KeyResult::Replace(backspaces, text, browser) => {
                    send_replacement(backspaces, &text, browser);
                    return 1;
                }
                KeyResult::Toggle { swallow, mask } => {
                    toggle();
                    if mask {
                        send_menu_mask();
                    }
                    if swallow {
                        return 1;
                    }
                }
                KeyResult::Eat { mask } => {
                    if mask {
                        send_menu_mask();
                    }
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(
            wparam as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
        )
    {
        with_state(|st| st.engine.reset());
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

fn copy_wide(dst: &mut [u16], text: &str) {
    // Leave room for the terminating zero.
    let limit = dst.len() - 1;
    for (d, s) in dst.iter_mut().take(limit).zip(text.encode_utf16()) {
        *d = s;
    }
}

fn tray_data(st: &State) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { zeroed() };
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = st.hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = if st.enabled { st.icon_vi } else { st.icon_en };
    let mode = if st.enabled {
        "Tiếng Việt"
    } else {
        "English"
    };
    let hotkey = st.hotkeys.hotkey;
    let mut tip = format!("ezkey: {mode}");
    if st.hotkeys.recording() {
        tip = "ezkey: nhấn phím tắt mới (Esc để huỷ)".into();
    } else if !hotkey.is_none() {
        let action = if st.enabled { "tắt" } else { "bật" };
        tip.push_str(&format!(" ({} để {action})", hotkey.label()));
    }
    if st.elevated {
        tip.push_str(" [Admin]");
    }
    copy_wide(&mut nid.szTip, &tip);
    nid
}

fn notify_tray(st: &State, message: NOTIFY_ICON_MESSAGE) {
    let nid = tray_data(st);
    unsafe { Shell_NotifyIconW(message, &nid) };
}

fn update_tray(message: NOTIFY_ICON_MESSAGE) {
    with_state(|st| notify_tray(st, message));
}

/// A notification from the tray icon (a toast on Windows 10 and 11).
fn show_balloon(st: &State, title: &str, text: &str) {
    let mut nid = tray_data(st);
    nid.uFlags |= NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    copy_wide(&mut nid.szInfoTitle, title);
    copy_wide(&mut nid.szInfo, text);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

fn toggle() {
    with_state(|st| {
        st.enabled = !st.enabled;
        st.engine.reset();
        save_settings(st);
        if let Some(app) = st.app.as_deref().filter(|_| st.per_app) {
            apps::save(app, st.enabled);
        }
        notify_tray(st, NIM_MODIFY);
    });
}

fn set_hotkey(st: &mut State, hotkey: Hotkey) {
    st.hotkeys.set(hotkey);
    write_number(HOTKEY, hotkey.to_setting());
    notify_tray(st, NIM_MODIFY);
}

fn start_recording(st: &mut State) {
    st.hotkeys.start_recording();
    unsafe { SetTimer(st.hwnd, TIMER_RECORD, RECORD_TIMEOUT_MS, None) };
    show_balloon(
        st,
        "Đặt phím tắt bật/tắt",
        "Nhấn tổ hợp phím mới, ví dụ Ctrl+Alt+V, Ctrl+Space hoặc Ctrl+Alt. Esc để huỷ.",
    );
}

/// Ends recording: `Some` keeps the new hotkey, `None` keeps the old one.
fn finish_recording(st: &mut State, hotkey: Option<Hotkey>) {
    unsafe { KillTimer(st.hwnd, TIMER_RECORD) };
    st.hotkeys.cancel_recording();
    match hotkey {
        Some(hotkey) => {
            set_hotkey(st, hotkey);
            show_balloon(st, "Đã đặt phím tắt", &hotkey.label());
        }
        None => {
            notify_tray(st, NIM_MODIFY);
            let kept = format!("Giữ phím tắt cũ: {}", st.hotkeys.hotkey.label());
            show_balloon(st, "Đã huỷ đặt phím tắt", &kept);
        }
    }
}

unsafe fn make_icon(text: &str, background: COLORREF) -> HICON {
    let size = GetSystemMetrics(SM_CXSMICON);
    let screen = GetDC(null_mut());
    let dc = CreateCompatibleDC(screen);
    let color = CreateCompatibleBitmap(screen, size, size);
    // Monochrome rows are WORD-aligned; an all-zero mask makes every pixel opaque.
    let mask_bits = vec![0u8; (size as usize).div_ceil(16) * 2 * size as usize];
    let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr().cast());

    let old_bitmap = SelectObject(dc, color);
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: size,
        bottom: size,
    };
    let brush = CreateSolidBrush(background);
    FillRect(dc, &rect, brush);
    DeleteObject(brush);

    let face = wide("Segoe UI");
    let font = CreateFontW(
        -(size * 7 / 8),
        0,
        0,
        0,
        FW_BOLD as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        ANTIALIASED_QUALITY as u32,
        DEFAULT_PITCH as u32 | FF_DONTCARE as u32,
        face.as_ptr(),
    );
    let old_font = SelectObject(dc, font);
    SetTextColor(dc, 0x00FF_FFFF);
    SetBkMode(dc, TRANSPARENT as _);
    let mut label = wide(text);
    DrawTextW(
        dc,
        label.as_mut_ptr(),
        -1,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(dc, old_font);
    DeleteObject(font);
    SelectObject(dc, old_bitmap);

    let info = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: color,
    };
    let icon = CreateIconIndirect(&info);
    DeleteObject(color);
    DeleteObject(mask);
    DeleteDC(dc);
    ReleaseDC(null_mut(), screen);
    icon
}

unsafe fn append_item(menu: HMENU, id: usize, label: &str, checked: Option<bool>) {
    let label = wide(label);
    let check = match checked {
        Some(true) => MF_CHECKED,
        Some(false) => MF_UNCHECKED,
        None => 0,
    };
    AppendMenuW(menu, MF_STRING | check, id, label.as_ptr());
}

unsafe fn hotkey_menu(current: Hotkey) -> HMENU {
    let menu = CreatePopupMenu();
    let presets = [
        (ID_HOTKEY_CTRL_SHIFT, Hotkey::CTRL_SHIFT),
        (ID_HOTKEY_ALT_Z, Hotkey::ALT_Z),
    ];
    for (id, preset) in presets {
        append_item(menu, id, &preset.label(), Some(preset == current));
    }
    let custom = !current.is_none() && presets.iter().all(|(_, p)| *p != current);
    if custom {
        append_item(menu, ID_HOTKEY_CUSTOM, &current.label(), Some(true));
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    append_item(menu, ID_HOTKEY_RECORD, "Đặt phím tắt khác…", None);
    append_item(
        menu,
        ID_HOTKEY_NONE,
        &Hotkey::NONE.label(),
        Some(current.is_none()),
    );
    menu
}

unsafe fn show_menu(hwnd: HWND) {
    let (enabled, modern, vni, per_app, hotkey) = with_state(|st| {
        (
            st.enabled,
            st.engine.style() == ToneStyle::Modern,
            st.engine.method() == InputMethod::Vni,
            st.per_app,
            st.hotkeys.hotkey,
        )
    });
    let autostart = startup::autostart_enabled();
    let admin = read_setting(RUN_AS_ADMIN) == Some(true);

    let toggle_label = if hotkey.is_none() {
        "Gõ tiếng Việt".to_string()
    } else {
        format!("Gõ tiếng Việt\t{}", hotkey.label())
    };
    let menu = CreatePopupMenu();
    let items: [(usize, &str, Option<bool>); 12] = [
        (ID_TOGGLE, &toggle_label, Some(enabled)),
        (0, "", None),
        (ID_TELEX, "Telex", Some(!vni)),
        (ID_VNI, "VNI", Some(vni)),
        (ID_MODERN, "Đặt dấu kiểu mới (hoà, thuý)", Some(modern)),
        (0, "", None),
        (ID_PER_APP, "Nhớ chế độ V/E theo từng app", Some(per_app)),
        (ID_HOTKEY_MENU, "Phím tắt bật/tắt", None),
        (ID_AUTOSTART, "Khởi động cùng Windows", Some(autostart)),
        (ID_ADMIN, "Chạy với quyền Administrator", Some(admin)),
        (0, "", None),
        (ID_EXIT, "Thoát", None),
    ];
    for (id, label, checked) in items {
        match id {
            0 => {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
            }
            ID_HOTKEY_MENU => {
                let label = wide(label);
                let sub = hotkey_menu(hotkey);
                AppendMenuW(menu, MF_POPUP, sub as usize, label.as_ptr());
            }
            _ => append_item(menu, id, label, checked),
        }
    }

    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    ) as usize;
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);

    match cmd {
        ID_TOGGLE => toggle(),
        ID_MODERN => with_state(|st| {
            let style = if modern {
                ToneStyle::Classic
            } else {
                ToneStyle::Modern
            };
            st.engine.set_style(style);
            st.engine.reset();
            save_settings(st);
        }),
        ID_TELEX | ID_VNI => with_state(|st| {
            let method = if cmd == ID_VNI {
                InputMethod::Vni
            } else {
                InputMethod::Telex
            };
            st.engine.set_method(method);
            save_settings(st);
        }),
        ID_PER_APP => with_state(|st| {
            st.per_app = !per_app;
            write_setting(PER_APP, st.per_app);
            // Start from the current mode for the app the user came from.
            if let Some(app) = st.app.as_deref().filter(|_| st.per_app) {
                apps::save(app, st.enabled);
            }
        }),
        ID_HOTKEY_CTRL_SHIFT => with_state(|st| set_hotkey(st, Hotkey::CTRL_SHIFT)),
        ID_HOTKEY_ALT_Z => with_state(|st| set_hotkey(st, Hotkey::ALT_Z)),
        ID_HOTKEY_NONE => with_state(|st| set_hotkey(st, Hotkey::NONE)),
        ID_HOTKEY_RECORD => with_state(start_recording),
        ID_AUTOSTART => startup::set_autostart(!autostart, admin),
        ID_ADMIN => set_run_as_admin(hwnd, !admin, autostart),
        ID_EXIT => PostQuitMessage(0),
        _ => {}
    }
}

/// Turning it on restarts ezkey elevated right away (UAC prompt); turning
/// it off takes effect at the next start, since an elevated process cannot
/// simply drop its rights.
unsafe fn set_run_as_admin(hwnd: HWND, on: bool, autostart: bool) {
    let elevated = startup::is_elevated();
    if on && !elevated {
        if startup::relaunch_elevated() {
            write_setting(RUN_AS_ADMIN, true);
            // The elevated instance waits for this one to exit, then moves
            // start-with-Windows to an admin logon task.
            PostQuitMessage(0);
        }
        return;
    }
    write_setting(RUN_AS_ADMIN, on);
    if autostart {
        startup::set_autostart(true, on);
    }
    if !on && elevated {
        let text = wide("ezkey sẽ chạy với quyền thường từ lần mở sau.");
        let caption = wide("ezkey");
        MessageBoxW(
            hwnd,
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Holds the single-instance mutex for the life of the process. An instance
/// relaunched to change elevation waits for the old one to exit.
fn acquire_single_instance(wait: bool) -> bool {
    let name = wide("Local\\ezkey-single-instance");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        unsafe {
            let handle = CreateMutexW(null(), 0, name.as_ptr());
            // A normal process opening an elevated instance's mutex gets
            // access denied (null handle) rather than ERROR_ALREADY_EXISTS.
            let taken = handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS;
            if !taken {
                return true;
            }
            if !handle.is_null() {
                CloseHandle(handle);
            }
        }
        if !wait || std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_TRAY {
        match (lparam & 0xFFFF) as u32 {
            WM_LBUTTONUP => toggle(),
            WM_RBUTTONUP => show_menu(hwnd),
            _ => {}
        }
        return 0;
    }
    if msg == WM_TIMER && wparam == TIMER_RECORD {
        with_state(|st| {
            if st.hotkeys.recording() {
                finish_recording(st, None);
            } else {
                KillTimer(hwnd, TIMER_RECORD);
            }
        });
        return 0;
    }
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    let taskbar_created = STATE.with(|s| s.borrow().as_ref().map_or(0, |st| st.taskbar_created));
    if msg == taskbar_created && msg != 0 {
        // Explorer restarted: the tray icon has to be added again.
        update_tray(NIM_ADD);
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn main() {
    let relaunched = std::env::args().any(|a| a == startup::RELAUNCH_ARG);
    let run_as_admin = read_setting(RUN_AS_ADMIN) == Some(true);
    let elevated = startup::is_elevated();
    // If the UAC prompt is declined, keep running without admin rights.
    if run_as_admin && !elevated && !relaunched && startup::relaunch_elevated() {
        return;
    }
    if !acquire_single_instance(relaunched) {
        return;
    }

    unsafe {
        let instance = GetModuleHandleW(null());
        let class_name = wide("ezkey");
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = instance;
        wc.lpszClassName = class_name.as_ptr();
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );

        let mut engine = Engine::new();
        if read_setting("ModernTone") == Some(true) {
            engine.set_style(ToneStyle::Modern);
        }
        if read_setting("Vni") == Some(true) {
            engine.set_method(InputMethod::Vni);
        }
        // Keep the startup entry pointing at this executable if it moved,
        // and on the right mechanism for the admin setting.
        if startup::autostart_enabled() {
            startup::set_autostart(true, run_as_admin);
        }

        let taskbar_msg = wide("TaskbarCreated");
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                engine,
                enabled: read_setting("Enabled").unwrap_or(true),
                hwnd,
                foreground: null_mut(),
                app: None,
                browser: false,
                per_app: read_setting(PER_APP).unwrap_or(true),
                hotkeys: Matcher::new(Hotkey::from_setting(read_number(HOTKEY))),
                icon_vi: make_icon("V", 0x00B3_6B1E),
                icon_en: make_icon("E", 0x0060_6060),
                taskbar_created: RegisterWindowMessageW(taskbar_msg.as_ptr()),
                elevated,
            })
        });
        update_tray(NIM_ADD);

        let kb_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), instance, 0);
        let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), instance, 0);
        let foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        with_state(|st| on_foreground(st, GetForegroundWindow()));

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnhookWindowsHookEx(kb_hook);
        UnhookWindowsHookEx(mouse_hook);
        UnhookWinEvent(foreground_hook);
        update_tray(NIM_DELETE);
    }
}
