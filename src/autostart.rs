use crate::error::{Error, Result};
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "VulcanControl";

fn autostart_exe() -> Result<std::path::PathBuf> {
    let current = std::env::current_exe().map_err(|e| Error::new(format!("current_exe: {e}")))?;
    let sibling = current
        .parent()
        .map(|p| p.join("vulcand.exe"))
        .filter(|p| p.exists());
    Ok(sibling.unwrap_or(current))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_run_key() -> Result<HKEY> {
    let subkey = wide(RUN_KEY);
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status != 0 || key.is_null() {
        return Err(Error::new(format!(
            "RegOpenKeyExW(HKCU\\{RUN_KEY}) failed: win32 {status}"
        )));
    }
    Ok(key)
}

/// Register `vulcand.exe` for the current user. No elevation required.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let key = open_run_key()?;
    let name = wide(VALUE_NAME);
    let result = if enabled {
        let exe = autostart_exe()?;
        let command = format!("\"{}\"", exe.display());
        let data = wide(&command);
        unsafe {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                (data.len() * std::mem::size_of::<u16>()) as u32,
            )
        }
    } else {
        let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
        if status == ERROR_FILE_NOT_FOUND {
            0
        } else {
            status
        }
    };
    unsafe { RegCloseKey(key) };
    if result != 0 {
        return Err(Error::new(format!(
            "{} autostart failed: win32 {result}",
            if enabled { "set" } else { "delete" }
        )));
    }
    Ok(())
}

/// Whether the per-user Run entry exists.
pub fn is_enabled() -> bool {
    let subkey = wide(RUN_KEY);
    let name = wide(VALUE_NAME);
    let mut size = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    status == 0
}
