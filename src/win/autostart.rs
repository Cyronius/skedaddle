use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE,
    REG_SAM_FLAGS, REG_SZ,
};

use super::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "skedaddle";

fn open(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let path = wide(RUN_KEY);
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, access, &mut key) };
    (rc == ERROR_SUCCESS).then_some(key)
}

pub fn is_enabled() -> bool {
    let Some(key) = open(KEY_READ) else { return false };
    let name = wide(VALUE);
    let rc = unsafe { RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, None, None, None) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    rc == ERROR_SUCCESS
}

// Keeps an enabled autostart entry working after the exe moves to its installed location.
pub fn repoint(exe: &std::path::Path) {
    if is_enabled() {
        write(exe);
    }
}

fn write(exe: &std::path::Path) {
    let Some(key) = open(KEY_WRITE) else { return };
    let name = wide(VALUE);
    let w = wide(&format!("\"{}\"", exe.display()));
    unsafe {
        let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
        let _ = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
    }
}

pub fn set(enabled: bool) {
    let Some(key) = open(KEY_WRITE) else { return };
    let name = wide(VALUE);
    unsafe {
        if enabled {
            if let Ok(exe) = std::env::current_exe() {
                let w = wide(&format!("\"{}\"", exe.display()));
                let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
                let _ = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
        }
        let _ = RegCloseKey(key);
    }
}
