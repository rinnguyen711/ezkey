//! Elevation and start-with-Windows.
//!
//! A normal start uses the HKCU Run key. With "run as administrator" the Run
//! key would trigger a UAC prompt at every logon, so an elevated instance
//! registers a Task Scheduler logon task with highest privileges instead.

use std::mem::{size_of, zeroed};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcessToken, CREATE_NO_WINDOW,
};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::wide;

/// Passed to an instance started by `relaunch_elevated`; it waits for the
/// old instance to exit instead of quitting on the single-instance check.
pub const RELAUNCH_ARG: &str = "--relaunch";

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const NAME: &str = "ezkey";

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut len = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Starts this executable again with the UAC "runas" verb. False if the
/// user declined the prompt or the launch failed.
pub fn relaunch_elevated() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let (verb, file, params) = (
        wide("runas"),
        wide(&exe.to_string_lossy()),
        wide(RELAUNCH_ARG),
    );
    unsafe {
        let mut info: SHELLEXECUTEINFOW = zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        ShellExecuteExW(&mut info) != 0
    }
}

pub fn autostart_enabled() -> bool {
    run_key_exists() || task_exists()
}

/// Registers or removes start-with-Windows for this executable, using a
/// highest-privilege logon task when `admin` is set and we are elevated
/// (creating such a task needs elevation; otherwise the Run key is used).
pub fn set_autostart(on: bool, admin: bool) {
    if !on {
        delete_run_key();
        delete_task();
        return;
    }
    if admin && is_elevated() && create_task() {
        delete_run_key();
        return;
    }
    write_run_key();
    if !admin {
        delete_task();
    }
}

fn run_key_exists() -> bool {
    let (key, name) = (wide(RUN_KEY), wide(NAME));
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            null_mut(),
        )
    };
    status == ERROR_SUCCESS
}

fn write_run_key() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let (key, name) = (wide(RUN_KEY), wide(NAME));
    let command = wide(&format!("\"{}\"", exe.display()));
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            command.as_ptr().cast(),
            (command.len() * size_of::<u16>()) as u32,
        );
    }
}

fn delete_run_key() {
    let (key, name) = (wide(RUN_KEY), wide(NAME));
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
}

/// Runs schtasks without flashing a console window.
fn schtasks(args: &[&str]) -> bool {
    Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|out| out.status.success())
}

fn task_exists() -> bool {
    schtasks(&["/Query", "/TN", NAME])
}

fn delete_task() {
    if task_exists() {
        schtasks(&["/Delete", "/TN", NAME, "/F"]);
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A task XML rather than schtasks flags: the flags cannot lift the default
/// 72-hour time limit, the battery restrictions or the below-normal priority.
fn create_task() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{exe}</Command></Exec>
  </Actions>
</Task>
"#,
        user = xml_escape(&user),
        exe = xml_escape(&exe.to_string_lossy()),
    );
    // schtasks reads task XML as UTF-16 with a byte order mark.
    let bytes: Vec<u8> = std::iter::once(0xFEFF)
        .chain(xml.encode_utf16())
        .flat_map(u16::to_le_bytes)
        .collect();
    let path = std::env::temp_dir().join("ezkey-task.xml");
    if std::fs::write(&path, bytes).is_err() {
        return false;
    }
    let ok = schtasks(&[
        "/Create",
        "/TN",
        NAME,
        "/XML",
        &path.to_string_lossy(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&path);
    ok
}
