//! ezkey-setup: installs ezkey for the current user (no admin rights) into
//! %LOCALAPPDATA%\Programs\ezkey, with a Start menu shortcut and an entry in
//! Settings → Apps. The same program, copied as uninstall.exe, removes it.
//!
//!     ezkey-setup.exe [--quiet] [--autostart] [--no-launch] [--dir <path>]
//!     uninstall.exe --uninstall [--quiet]
//!
//! Exit codes: 0 done, 1 cancelled, 2 failed.

#![windows_subsystem = "windows"]

mod shortcut;

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::{null, null_mut};
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
use windows_sys::Win32::UI::Shell::{
    SHChangeNotify, SHCNE_CREATE, SHCNE_DELETE, SHCNE_ID, SHCNF_PATHW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const EZKEY_EXE: &[u8] = include_bytes!(env!("EZKEY_EXE_PATH"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\ezkey";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const TITLE: &str = "ezkey";

struct Options {
    uninstall: bool,
    quiet: bool,
    autostart: bool,
    no_launch: bool,
    dir: Option<PathBuf>,
}

fn parse_args() -> Options {
    let mut opts = Options {
        uninstall: false,
        quiet: false,
        autostart: false,
        no_launch: false,
        dir: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--uninstall" => opts.uninstall = true,
            "--quiet" => opts.quiet = true,
            "--autostart" => opts.autostart = true,
            "--no-launch" => opts.no_launch = true,
            "--dir" => opts.dir = args.next().map(PathBuf::from),
            _ => {}
        }
    }
    opts
}

fn main() {
    let opts = parse_args();
    let result = if opts.uninstall {
        uninstall(&opts)
    } else {
        install(&opts)
    };
    let code = match result {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(message) => {
            if !opts.quiet {
                message_box(&message, MB_OK | MB_ICONERROR);
            }
            2
        }
    };
    std::process::exit(code);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn message_box(text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let (text, title) = (wide(text), wide(TITLE));
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), flags) }
}

fn ask(text: &str) -> bool {
    message_box(text, MB_YESNO | MB_ICONQUESTION) == IDYES
}

fn default_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or("Không tìm thấy thư mục LOCALAPPDATA.")?;
    Ok(PathBuf::from(base).join("Programs").join("ezkey"))
}

fn start_menu_shortcut() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\ezkey.lnk"))
}

fn install(opts: &Options) -> Result<bool, String> {
    let dir = match &opts.dir {
        Some(dir) => dir.clone(),
        None => default_dir()?,
    };
    let autostart = if opts.quiet {
        opts.autostart
    } else {
        let question = format!(
            "Cài ezkey {VERSION} cho tài khoản này?\n\nThư mục: {}",
            dir.display()
        );
        if !ask(&question) {
            return Ok(false);
        }
        ask("Khởi động ezkey cùng Windows?")
    };
    if !close_running(opts.quiet)? {
        return Ok(false);
    }

    std::fs::create_dir_all(&dir).map_err(|e| format!("Không tạo được {}: {e}", dir.display()))?;
    let exe = dir.join("ezkey.exe");
    write_with_retry(&exe, EZKEY_EXE)?;
    let uninstaller = dir.join("uninstall.exe");
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if !same_file(&me, &uninstaller) {
        std::fs::copy(&me, &uninstaller)
            .map_err(|e| format!("Không chép được {}: {e}", uninstaller.display()))?;
    }

    // A missing shortcut is reported but does not fail the install.
    let shortcut_error = start_menu_shortcut()
        .ok_or_else(|| "Không tìm thấy thư mục Start menu.".to_string())
        .and_then(|link| create_shortcut(&link, &exe, &dir))
        .err();
    register_uninstall(&dir, &exe, &uninstaller)?;
    if autostart {
        set_string(RUN_KEY, "ezkey", &format!("\"{}\"", exe.display()))?;
    }
    if !opts.no_launch {
        Command::new(&exe)
            .current_dir(&dir)
            .spawn()
            .map_err(|e| format!("Không mở được ezkey: {e}"))?;
    }
    if !opts.quiet {
        let mut text = String::from(
            "Đã cài xong ezkey.\n\nBiểu tượng V/E nằm ở khay hệ thống (góc phải thanh taskbar). \
             Gỡ cài đặt trong Settings → Apps.",
        );
        if let Some(error) = &shortcut_error {
            text.push_str(&format!(
                "\n\nKhông tạo được lối tắt trong Start menu: {error}\nMở ezkey trực tiếp tại {}.",
                exe.display()
            ));
        }
        message_box(&text, MB_OK | MB_ICONINFORMATION);
    }
    Ok(true)
}

fn uninstall(opts: &Options) -> Result<bool, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = match &opts.dir {
        Some(dir) => dir.clone(),
        None => match me.parent() {
            Some(parent) if parent.join("ezkey.exe").exists() => parent.to_path_buf(),
            _ => default_dir()?,
        },
    };
    if !opts.quiet && !ask("Gỡ ezkey khỏi máy?") {
        return Ok(false);
    }
    if !close_running(opts.quiet)? {
        return Ok(false);
    }

    // Only startup entries that launch this installed copy belong to us; a
    // development build's entries stay.
    let exe = dir.join("ezkey.exe");
    if read_string(RUN_KEY, "ezkey").is_some_and(|cmd| mentions(&cmd, &dir)) {
        delete_value(RUN_KEY, "ezkey");
    }
    if task_command().is_some_and(|xml| mentions(&xml, &dir)) {
        schtasks(&["/Delete", "/TN", "ezkey", "/F"]);
    }
    if let Some(link) = start_menu_shortcut() {
        if std::fs::remove_file(&link).is_ok() {
            notify_shell(SHCNE_DELETE, &link);
        }
    }
    let key = wide(UNINSTALL_KEY);
    unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, key.as_ptr()) };

    remove_with_retry(&exe)?;
    let uninstaller = dir.join("uninstall.exe");
    if same_file(&me, &uninstaller) {
        // A running program cannot delete itself; cmd does it once we exit.
        let script = format!(
            "/c ping -n 3 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"",
            uninstaller.display(),
            dir.display()
        );
        let _ = Command::new("cmd")
            .raw_arg(script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    } else {
        let _ = std::fs::remove_file(&uninstaller);
        let _ = std::fs::remove_dir(&dir);
    }
    if !opts.quiet {
        message_box(
            "Đã gỡ ezkey.\n\nCài đặt của bạn vẫn được giữ lại (registry, tại HKCU\\Software\\ezkey).",
            MB_OK | MB_ICONINFORMATION,
        );
    }
    Ok(true)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn mentions(text: &str, dir: &Path) -> bool {
    text.to_lowercase()
        .contains(&dir.display().to_string().to_lowercase())
}

/// Asks a running ezkey to quit. An elevated one ignores messages from a
/// normal process, so the user is asked to quit it from the tray.
fn close_running(quiet: bool) -> Result<bool, String> {
    let class = wide("ezkey");
    loop {
        let window = unsafe { FindWindowW(class.as_ptr(), null()) };
        if window.is_null() {
            return Ok(true);
        }
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if unsafe { FindWindowW(class.as_ptr(), null()) }.is_null() {
                return Ok(true);
            }
            sleep(Duration::from_millis(100));
        }
        let message = "ezkey đang chạy với quyền Administrator nên không tự đóng được.\n\n\
                       Hãy thoát nó (chuột phải biểu tượng ở khay → Thoát), rồi bấm Retry.";
        if quiet {
            return Err(message.into());
        }
        if message_box(message, MB_RETRYCANCEL | MB_ICONWARNING) != IDRETRY {
            return Ok(false);
        }
    }
}

/// The old process may hold the file for a moment after its window closes.
fn write_with_retry(path: &Path, bytes: &[u8]) -> Result<(), String> {
    retry(|| std::fs::write(path, bytes))
        .map_err(|e| format!("Không ghi được {}: {e}", path.display()))
}

fn remove_with_retry(path: &Path) -> Result<(), String> {
    retry(|| match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    })
    .map_err(|e| format!("Không xoá được {}: {e}", path.display()))
}

fn retry(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match op() {
            Err(_) if Instant::now() < deadline => sleep(Duration::from_millis(200)),
            result => return result,
        }
    }
}

fn create_shortcut(link: &Path, target: &Path, dir: &Path) -> Result<(), String> {
    shortcut::create(link, target, dir, "Bộ gõ tiếng Việt")?;
    notify_shell(SHCNE_CREATE, link);
    Ok(())
}

/// Tells Explorer and Start search about a shortcut right away instead of
/// whenever they next rescan the folder.
fn notify_shell(event: SHCNE_ID, path: &Path) {
    let path = wide(&path.display().to_string());
    unsafe { SHChangeNotify(event as i32, SHCNF_PATHW, path.as_ptr().cast(), null()) };
}

fn register_uninstall(dir: &Path, exe: &Path, uninstaller: &Path) -> Result<(), String> {
    let quoted = format!("\"{}\"", uninstaller.display());
    let size_kb =
        (EZKEY_EXE.len() as u64 + std::fs::metadata(uninstaller).map_or(0, |m| m.len())) / 1024;
    set_string(UNINSTALL_KEY, "DisplayName", "ezkey")?;
    set_string(UNINSTALL_KEY, "DisplayVersion", VERSION)?;
    set_string(UNINSTALL_KEY, "Publisher", "ezkey")?;
    set_string(UNINSTALL_KEY, "InstallLocation", &dir.display().to_string())?;
    set_string(UNINSTALL_KEY, "DisplayIcon", &exe.display().to_string())?;
    set_string(
        UNINSTALL_KEY,
        "UninstallString",
        &format!("{quoted} --uninstall"),
    )?;
    set_string(
        UNINSTALL_KEY,
        "QuietUninstallString",
        &format!("{quoted} --uninstall --quiet"),
    )?;
    set_dword(UNINSTALL_KEY, "EstimatedSize", size_kb as u32)?;
    set_dword(UNINSTALL_KEY, "NoModify", 1)?;
    set_dword(UNINSTALL_KEY, "NoRepair", 1)
}

fn set_value(key: &str, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> Result<(), String> {
    let (k, n) = (wide(key), wide(name));
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            n.as_ptr(),
            kind,
            data.as_ptr().cast(),
            data.len() as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "Không ghi được registry HKCU\\{key}\\{name} (lỗi {status})."
        ))
    }
}

fn set_string(key: &str, name: &str, value: &str) -> Result<(), String> {
    let bytes: Vec<u8> = wide(value).iter().flat_map(|u| u.to_le_bytes()).collect();
    set_value(key, name, REG_SZ, &bytes)
}

fn set_dword(key: &str, name: &str, value: u32) -> Result<(), String> {
    set_value(key, name, REG_DWORD, &value.to_le_bytes())
}

fn read_string(key: &str, name: &str) -> Option<String> {
    let (k, n) = (wide(key), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            n.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn delete_value(key: &str, name: &str) {
    let (k, n) = (wide(key), wide(name));
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) };
}

fn schtasks(args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// The XML of the "ezkey" logon task created by ezkey's run-as-admin option.
fn task_command() -> Option<String> {
    let bytes = schtasks(&["/Query", "/TN", "ezkey", "/XML"])?;
    Some(match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a real shortcut in a folder with a Vietnamese name and reads
    /// it back: both the path and the description must survive.
    #[test]
    fn shortcut_keeps_vietnamese_text() {
        let dir = std::env::temp_dir().join(format!("ezkey-test-Nguyễn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ezkey.exe");
        std::fs::write(&target, b"").unwrap();
        let link = dir.join("ezkey.lnk");
        create_shortcut(&link, &target, &dir).unwrap();

        let read = shortcut::read(&link);
        // The shell may store the long form of a short (8.3) path.
        let same_target = read
            .as_ref()
            .is_ok_and(|(got, _)| same_file(Path::new(got), &target));
        let _ = std::fs::remove_dir_all(&dir);
        let (got_target, description) = read.unwrap();
        assert_eq!(description, "Bộ gõ tiếng Việt");
        assert!(same_target, "{got_target} is not {}", target.display());
    }
}
