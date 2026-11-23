use std::os::windows::ffi::OsStrExt;
use std::ptr;

#[link(name = "shell32")]
unsafe extern "system" {
    fn IsUserAnAdmin() -> i32;
    fn ShellExecuteW(
        window: *mut std::ffi::c_void,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn profile_scope() -> std::io::Result<u64> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hash = DefaultHasher::new();
    crate::Profiles::directory()?.hash(&mut hash);
    Ok(hash.finish())
}

pub fn request_connection() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Не удалось найти исполняемый файл: {error}"))?;
    let scope = profile_scope().map_err(|error| error.to_string())?;
    run_as_admin(&executable, &format!("--connect-elevated {scope}"))
        .map_err(|error| format!("Не удалось запустить System VPN с правами администратора: {error}"))
}

pub fn run_as_admin(executable: &std::path::Path, arguments: &str) -> std::io::Result<()> {
    let executable: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let runas: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    let arguments: Vec<u16> = arguments.encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            runas.as_ptr(),
            executable.as_ptr(),
            arguments.as_ptr(),
            ptr::null(),
            1,
        )
    };
    if result > 32 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
