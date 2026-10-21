//! Identifies the foreground application and remembers Vietnamese / English
//! per application (HKCU\Software\ezkey\Apps, one DWORD per exe name).

use std::mem::size_of;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};

use crate::wide;

const APPS_KEY: &str = "Software\\ezkey\\Apps";

/// Windows that take the foreground while the user is still "in" the
/// previous app: the taskbar and tray (e.g. clicking our tray icon).
const SHELL_CLASSES: &[&str] = &[
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
];

/// Browsers whose address bar inline-autocompletes: after "a" the bar holds
/// "a[mazon.com]" with the suggestion selected, so a plain Backspace deletes
/// the suggestion instead of the "a" and "aa" ends up as "aâ".
const BROWSERS: &[&str] = &[
    "msedge.exe",
    "chrome.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "browser.exe",
];

/// Lowercase executable file name of the window's process, e.g. "code.exe".
pub fn exe_name(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        path.rsplit('\\').next().map(str::to_owned)
    }
}

/// Our own windows (tray menu) and the taskbar are not apps to switch to.
pub fn is_own_or_shell(hwnd: HWND) -> bool {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == GetCurrentProcessId() {
            return true;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let class = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
        SHELL_CLASSES.contains(&class.as_str())
    }
}

pub fn is_browser(exe: &str) -> bool {
    BROWSERS.contains(&exe)
}

/// The mode last chosen in this app, if any.
pub fn load(exe: &str) -> Option<bool> {
    let (key, name) = (wide(APPS_KEY), wide(exe));
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
    (status == ERROR_SUCCESS).then_some(data != 0)
}

pub fn save(exe: &str, enabled: bool) {
    let (key, name) = (wide(APPS_KEY), wide(exe));
    let data = enabled as u32;
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_DWORD,
            (&data as *const u32).cast(),
            size_of::<u32>() as u32,
        );
    }
}
