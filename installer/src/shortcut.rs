//! Shell shortcuts (.lnk) through the IShellLinkW COM object, called by
//! vtable slot because windows-sys has no COM interface bindings.
//!
//! WScript.Shell, the usual scripting route, goes through the ANSI
//! interface: it turns "Bộ gõ" into "B? gõ" and cannot save under a
//! profile path such as C:\Users\Nguyễn.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};

use crate::wide;

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const IID_SHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_PERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

// Vtable slots: IUnknown is 0-2 for both interfaces.
const QUERY_INTERFACE: usize = 0;
const RELEASE: usize = 2;
#[cfg(test)]
const LINK_GET_PATH: usize = 3;
#[cfg(test)]
const LINK_GET_DESCRIPTION: usize = 6;
const LINK_SET_DESCRIPTION: usize = 7;
const LINK_SET_WORKING_DIRECTORY: usize = 9;
const LINK_SET_PATH: usize = 20;
#[cfg(test)]
const FILE_LOAD: usize = 5;
const FILE_SAVE: usize = 6;

type QueryInterfaceFn =
    unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
type SetStringFn = unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT;
#[cfg(test)]
type GetPathFn = unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut c_void, u32) -> HRESULT;
#[cfg(test)]
type GetDescriptionFn = unsafe extern "system" fn(*mut c_void, *mut u16, i32) -> HRESULT;
type SaveFn = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT;
#[cfg(test)]
type LoadFn = unsafe extern "system" fn(*mut c_void, *const u16, u32) -> HRESULT;

/// An owned COM interface pointer, released on drop.
struct Com(*mut c_void);

impl Com {
    /// # Safety
    /// `F` must be the signature of the method at `slot` of this interface.
    unsafe fn method<F: Copy>(&self, slot: usize) -> F {
        let vtable = *(self.0 as *const *const usize);
        std::mem::transmute_copy(&*vtable.add(slot))
    }

    fn query(&self, iid: &GUID) -> Result<Com, String> {
        let mut out = null_mut();
        let hr = unsafe { self.method::<QueryInterfaceFn>(QUERY_INTERFACE)(self.0, iid, &mut out) };
        check(hr, "QueryInterface")?;
        Ok(Com(out))
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { self.method::<ReleaseFn>(RELEASE)(self.0) };
    }
}

fn check(hr: HRESULT, what: &str) -> Result<(), String> {
    if hr >= 0 {
        Ok(())
    } else {
        Err(format!("{what} thất bại (0x{:08X})", hr as u32))
    }
}

fn shell_link() -> Result<Com, String> {
    unsafe {
        // S_FALSE (already initialised on this thread) is fine too.
        CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED as u32);
        let mut link = null_mut();
        let hr = CoCreateInstance(
            &CLSID_SHELL_LINK,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_SHELL_LINK_W,
            &mut link,
        );
        check(hr, "CoCreateInstance(ShellLink)")?;
        Ok(Com(link))
    }
}

fn set(link: &Com, slot: usize, value: &str, what: &str) -> Result<(), String> {
    let value = wide(value);
    let hr = unsafe { link.method::<SetStringFn>(slot)(link.0, value.as_ptr()) };
    check(hr, what)
}

/// Writes `path` (.lnk) pointing at `target`.
pub fn create(
    path: &Path,
    target: &Path,
    working_dir: &Path,
    description: &str,
) -> Result<(), String> {
    let link = shell_link()?;
    set(
        &link,
        LINK_SET_PATH,
        &target.display().to_string(),
        "SetPath",
    )?;
    set(
        &link,
        LINK_SET_WORKING_DIRECTORY,
        &working_dir.display().to_string(),
        "SetWorkingDirectory",
    )?;
    set(&link, LINK_SET_DESCRIPTION, description, "SetDescription")?;
    let file = link.query(&IID_PERSIST_FILE)?;
    let path = wide(&path.display().to_string());
    let hr = unsafe { file.method::<SaveFn>(FILE_SAVE)(file.0, path.as_ptr(), 1) };
    check(hr, "IPersistFile::Save")
}

/// Reads back a shortcut's target and description.
#[cfg(test)]
pub fn read(path: &Path) -> Result<(String, String), String> {
    let link = shell_link()?;
    let file = link.query(&IID_PERSIST_FILE)?;
    let path = wide(&path.display().to_string());
    check(
        unsafe { file.method::<LoadFn>(FILE_LOAD)(file.0, path.as_ptr(), 0) },
        "IPersistFile::Load",
    )?;

    let text = |buf: &[u16]| {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    };
    let mut target = [0u16; 1024];
    let hr = unsafe {
        link.method::<GetPathFn>(LINK_GET_PATH)(
            link.0,
            target.as_mut_ptr(),
            target.len() as i32,
            null_mut(),
            0,
        )
    };
    check(hr, "GetPath")?;
    let mut description = [0u16; 1024];
    let hr = unsafe {
        link.method::<GetDescriptionFn>(LINK_GET_DESCRIPTION)(
            link.0,
            description.as_mut_ptr(),
            description.len() as i32,
        )
    };
    check(hr, "GetDescription")?;
    Ok((text(&target), text(&description)))
}
