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

pub enum Startup {
    Elevated,
    Relaunched,
}

pub fn request() -> Result<Startup, String> {
    if unsafe { IsUserAnAdmin() } != 0 {
        return Ok(Startup::Elevated);
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not request administrator rights: {error}"))?;
    let executable: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let runas: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    // The executable and verb are null-terminated, and no arguments are passed.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            runas.as_ptr(),
            executable.as_ptr(),
            ptr::null(),
            ptr::null(),
            1,
        )
    };
    if result > 32 {
        Ok(Startup::Relaunched)
    } else {
        Err("Administrator rights were not granted. SOCKS5 is available. Restart the app to enable System VPN.".into())
    }
}
