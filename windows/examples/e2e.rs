//! End-to-end check on a real desktop: starts ezkey.exe and Notepad, types
//! Telex with SendInput (through the hook, like a physical keyboard) and reads
//! Notepad's text back. Aborts if Notepad loses focus so no keys go astray.
//!
//!     cargo build --release -p ezkey-win
//!     cargo run --release -p ezkey-win --example e2e -- target\release\ezkey.exe

use std::cell::Cell;
use std::mem::size_of;
use std::process::{Child, Command};
use std::ptr::null;
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CASES: &[(&str, &str)] = &[
    ("tieengs vieetj", "tiếng việt"),
    ("Vieetj Nam", "Việt Nam"),
    ("xin chaof cacs banj", "xin chào các bạn"),
    ("dduwowngf", "đường"),
    ("nguowif", "người"),
    ("NGUYEENX", "NGUYỄN"),
    ("hoafn<ng", "hoàng"),
    ("ass aaa ww", "as aa w"),
    ("hello world", "hello world"),
];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    sleep(Duration::from_millis(25));
}

fn tap(vk: VIRTUAL_KEY, shift: bool) {
    let mut v = Vec::new();
    if shift {
        v.push(key(VK_LSHIFT, false));
    }
    v.push(key(vk, false));
    v.push(key(vk, true));
    if shift {
        v.push(key(VK_LSHIFT, true));
    }
    send(&v);
}

fn toggle_alt_z() {
    send(&[
        key(VK_LMENU, false),
        key(0x5A, false),
        key(0x5A, true),
        key(VK_LMENU, true),
    ]);
}

fn toggle_ime() {
    send(&[
        key(VK_LCONTROL, false),
        key(VK_LSHIFT, false),
        key(VK_LSHIFT, true),
        key(VK_LCONTROL, true),
    ]);
}

struct Notepad {
    child: Child,
    window: HWND,
    edit: HWND,
    /// Set once Notepad stops being the foreground window; no more keys are
    /// sent after that so they cannot land in another app.
    focus_lost: Cell<bool>,
}

impl Notepad {
    fn has_focus(&self) -> bool {
        if !self.focus_lost.get() && unsafe { GetForegroundWindow() } != self.window {
            self.focus_lost.set(true);
        }
        !self.focus_lost.get()
    }

    fn type_str(&self, s: &str) {
        for c in s.chars() {
            if !self.has_focus() {
                return;
            }
            match c {
                ' ' => tap(VK_SPACE, false),
                '<' => tap(VK_BACK, false),
                c if c.is_ascii_digit() => tap(c as VIRTUAL_KEY, false),
                c if c.is_ascii_alphabetic() => tap(
                    c.to_ascii_uppercase() as VIRTUAL_KEY,
                    c.is_ascii_uppercase(),
                ),
                _ => panic!("unsupported test character {c:?}"),
            }
        }
        sleep(Duration::from_millis(150));
    }

    fn clear(&self) {
        if self.has_focus() {
            tap(VK_ESCAPE, false); // non-letter key: ends the current word
        }
        let empty = wide("");
        unsafe { SendMessageW(self.edit, WM_SETTEXT, 0, empty.as_ptr() as LPARAM) };
    }

    fn text(&self) -> String {
        let mut buf = vec![0u16; 1024];
        let n =
            unsafe { SendMessageW(self.edit, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as LPARAM) };
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn find_window(pid: u32) -> HWND {
    struct Search {
        pid: u32,
        found: HWND,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam as *mut Search);
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == search.pid && IsWindowVisible(hwnd) != 0 {
            search.found = hwnd;
            return 0;
        }
        1
    }
    let mut search = Search {
        pid,
        found: std::ptr::null_mut(),
    };
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    search.found
}

fn open_notepad() -> Notepad {
    let child = Command::new("notepad.exe").spawn().expect("start notepad");
    let deadline = Instant::now() + Duration::from_secs(10);
    let window = loop {
        let w = find_window(child.id());
        if !w.is_null() {
            break w;
        }
        assert!(Instant::now() < deadline, "Notepad window did not appear");
        sleep(Duration::from_millis(100));
    };
    let class = wide("Edit");
    let edit = unsafe { FindWindowExW(window, std::ptr::null_mut(), class.as_ptr(), null()) };
    assert!(
        !edit.is_null(),
        "Notepad has no Edit control (Windows 11 Notepad is not supported)"
    );
    unsafe {
        if SetForegroundWindow(window) == 0 || GetForegroundWindow() != window {
            // Foreground lock: a keystroke from this process unlocks it.
            tap(VK_MENU, false);
            SetForegroundWindow(window);
        }
    }
    sleep(Duration::from_millis(300));
    Notepad {
        child,
        window,
        edit,
        focus_lost: Cell::new(false),
    }
}

const VNI_CASES: &[(&str, &str)] = &[
    ("tie6ng1 Vie6t5", "tiếng Việt"),
    ("d9u7o7ng2", "đường"),
    ("a11 123", "a1 123"),
];

const SETTINGS: [&str; 6] = [
    "Enabled",
    "Vni",
    "ModernTone",
    "PerApp",
    "Hotkey",
    "RunAsAdmin",
];
const SETTINGS_KEY: &str = r"HKCU\Software\ezkey";

fn read_setting(name: &str) -> Option<u32> {
    let out = Command::new("reg")
        .args(["query", SETTINGS_KEY, "/v", name])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let hex = text.split_whitespace().find(|w| w.starts_with("0x"))?;
    u32::from_str_radix(&hex[2..], 16).ok()
}

fn write_setting(name: &str, value: Option<u32>) {
    let mut cmd = Command::new("reg");
    match value {
        Some(v) => cmd
            .args(["add", SETTINGS_KEY, "/v", name, "/t", "REG_DWORD", "/d"])
            .arg(v.to_string()),
        None => cmd.args(["delete", SETTINGS_KEY, "/v", name]),
    };
    let _ = cmd.arg("/f").output();
}

/// Starts ezkey and checks it stays up; otherwise restores the user's
/// settings and stops, since every result would be meaningless.
fn start_ime(exe: &str, notepad: &mut Notepad, saved: &[(&str, Option<u32>)]) -> Child {
    let mut ime = Command::new(exe).spawn().expect("start ezkey");
    sleep(Duration::from_millis(800));
    if ime.try_wait().ok().flatten().is_some() {
        let _ = notepad.child.kill();
        for (name, value) in saved {
            write_setting(name, *value);
        }
        println!("ezkey exited right after starting (another instance running, or it relaunched itself as administrator)");
        std::process::exit(5);
    }
    unsafe { SetForegroundWindow(notepad.window) };
    sleep(Duration::from_millis(200));
    ime
}

/// Closes ezkey's window; false if it had to be killed.
fn stop_ime(ime: &mut Child) -> bool {
    let class = wide("ezkey");
    unsafe {
        let ime_window = FindWindowW(class.as_ptr(), null());
        PostMessageW(ime_window, WM_CLOSE, 0, 0);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while ime.try_wait().ok().flatten().is_none() {
        if Instant::now() > deadline {
            let _ = ime.kill();
            return false;
        }
        sleep(Duration::from_millis(50));
    }
    true
}

fn main() {
    let exe = std::env::args()
        .nth(1)
        .expect("usage: e2e <path to ezkey.exe>");
    let class = wide("ezkey");
    if !unsafe { FindWindowW(class.as_ptr(), null()) }.is_null() {
        println!("ezkey is already running; quit it from the tray and rerun");
        std::process::exit(4);
    }
    let mut notepad = open_notepad();

    // Without ezkey, Telex keys must come out unchanged; otherwise another
    // input method (Unikey, EVKey, ...) is active and would mask our results.
    notepad.clear();
    notepad.type_str("vieetj");
    let baseline = notepad.text();
    if notepad.focus_lost.get() {
        let _ = notepad.child.kill();
        println!("Notepad lost focus (keyboard or mouse in use?); rerun without touching the PC");
        std::process::exit(3);
    }
    if baseline != "vieetj" {
        let _ = notepad.child.kill();
        println!("another Vietnamese input method is active (typed \"vieetj\", got {baseline:?}); turn it off and rerun");
        std::process::exit(2);
    }

    // Run with known settings; the user's own values are restored at the end.
    let saved: Vec<_> = SETTINGS.iter().map(|n| (*n, read_setting(n))).collect();
    write_setting("Enabled", Some(1));
    write_setting("Vni", Some(0));
    write_setting("ModernTone", Some(0));
    write_setting("PerApp", Some(0));
    write_setting("Hotkey", Some(0));
    // An elevated ezkey cannot be closed from this (normal) process.
    write_setting("RunAsAdmin", Some(0));

    let mut ime = start_ime(&exe, &mut notepad, &saved);

    let mut failures = 0;
    let mut check = |name: &str, input: &str, want: &str, np: &Notepad| {
        if np.focus_lost.get() {
            return;
        }
        np.clear();
        np.type_str(input);
        let got = np.text();
        let ok = got == want;
        println!(
            "{} {name}: {input:?} -> {got:?}",
            if ok { "PASS" } else { "FAIL" }
        );
        if !ok {
            println!("       want {want:?}");
            failures += 1;
        }
    };

    for (input, want) in CASES {
        check("telex", input, want, &notepad);
    }
    if notepad.has_focus() {
        toggle_ime();
    }
    check("toggled off", "vieetj", "vieetj", &notepad);
    if notepad.has_focus() {
        toggle_ime();
    }
    check("toggled on", "vieetj", "việt", &notepad);

    // VNI, chosen through the saved setting a restarted ezkey must pick up.
    let clean_exit = stop_ime(&mut ime);
    write_setting("Vni", Some(1));
    write_setting("Hotkey", Some(1));
    let mut ime = start_ime(&exe, &mut notepad, &saved);
    for (input, want) in VNI_CASES {
        check("vni", input, want, &notepad);
    }

    // Hotkey switched to Alt+Z: Ctrl+Shift must no longer toggle.
    if notepad.has_focus() {
        toggle_ime();
    }
    check("ctrl+shift ignored", "a1", "á", &notepad);
    if notepad.has_focus() {
        toggle_alt_z();
    }
    check("alt+z off", "a1", "a1", &notepad);
    if notepad.has_focus() {
        toggle_alt_z();
    }
    check("alt+z on", "a1", "á", &notepad);

    notepad.clear();
    let notepad_focus_lost = notepad.focus_lost.get();
    let _ = notepad.child.kill();
    let clean_exit = stop_ime(&mut ime) && clean_exit;
    for (name, value) in saved {
        write_setting(name, value);
    }
    if !clean_exit {
        println!("FAIL ezkey did not exit on WM_CLOSE");
        failures += 1;
    }

    if notepad_focus_lost {
        println!("FAIL Notepad lost focus (keyboard or mouse in use?); remaining cases skipped");
        failures += 1;
    }
    println!("{} failure(s)", failures);
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
