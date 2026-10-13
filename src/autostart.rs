use std::env;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "yuantuanswitch";

pub fn enable() -> anyhow::Result<()> {
    let exe = env::current_exe()?;
    let path = exe.to_string_lossy().to_string();
    set_run_value(&path)
}

pub fn disable() -> anyhow::Result<()> {
    delete_run_value()
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_run_value(path: &str) -> anyhow::Result<()> {
    unsafe {
        let mut hkey = std::ptr::null_mut();
        let key = to_wide(RUN_KEY);
        if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) != 0 {
            return Err(anyhow::anyhow!("RegOpenKeyExW failed"));
        }
        let name = to_wide(VALUE_NAME);
        let data = to_wide(path);
        let status = RegSetValueExW(
            hkey,
            name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr() as *const _,
            (data.len() * 2) as u32,
        );
        RegCloseKey(hkey);
        if status != 0 {
            return Err(anyhow::anyhow!("RegSetValueExW failed: {}", status));
        }
    }
    Ok(())
}

fn delete_run_value() -> anyhow::Result<()> {
    use windows_sys::Win32::System::Registry::RegDeleteValueW;
    unsafe {
        let mut hkey = std::ptr::null_mut();
        let key = to_wide(RUN_KEY);
        if RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_SET_VALUE, &mut hkey) != 0 {
            return Err(anyhow::anyhow!("RegOpenKeyExW failed"));
        }
        let name = to_wide(VALUE_NAME);
        let status = RegDeleteValueW(hkey, name.as_ptr());
        RegCloseKey(hkey);
        if status != 0 {
            return Err(anyhow::anyhow!("RegDeleteValueW failed: {}", status));
        }
    }
    Ok(())
}
