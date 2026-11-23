use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_PRIVILEGE_NOT_HELD, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::System::Threading::{
    CreateEventExW, OpenEventW, SetEvent, WaitForMultipleObjects, EVENT_MODIFY_STATE, INFINITE,
};

const SYNCHRONIZE: u32 = 0x0010_0000;

pub struct Instance(OwnedHandle);

impl Instance {
    pub fn name() -> Vec<u16> {
        let name = if cfg!(debug_assertions) {
            r"Local\SSHVPN-Connect.SshTunnel.Debug"
        } else {
            r"Local\SSHVPN-Connect.SshTunnel"
        };
        name.encode_utf16().chain(Some(0)).collect()
    }

    pub fn activate_existing(name: &[u16]) -> bool {
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if handle.is_null() {
            return false;
        }
        let event = unsafe { OwnedHandle::from_raw_handle(handle) };
        unsafe { SetEvent(event.as_raw_handle()) != 0 }
    }

    pub fn claim(name: &[u16]) -> io::Result<Option<Self>> {
        let (event, exists) = create_event(name)?;
        if exists {
            if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            return Ok(None);
        }
        Ok(Some(Self(event)))
    }

    pub fn wait_until_closed(name: &[u16], timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let handle = unsafe { OpenEventW(SYNCHRONIZE, 0, name.as_ptr()) };
            if handle.is_null() {
                return true;
            }
            drop(unsafe { OwnedHandle::from_raw_handle(handle) });
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn listen(self, hwnd: isize, ctx: eframe::egui::Context) -> io::Result<Listener> {
        let event = self.0.try_clone()?;
        let shutdown_name: Vec<_> = format!(
            r"Local\SSHVPN-Connect.SshTunnel.Shutdown.{}",
            std::process::id()
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let (shutdown, _) = create_event(&shutdown_name)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let requested = shutdown_requested.clone();
        let worker = thread::Builder::new().spawn(move || {
            let handles = [event.as_raw_handle(), shutdown.as_raw_handle()];
            loop {
                let wake = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                if wake == WAIT_OBJECT_0 {
                    crate::tray::restore(hwnd);
                    ctx.request_repaint();
                } else {
                    if wake == WAIT_OBJECT_0 + 1 {
                        requested.store(true, Ordering::Release);
                        ctx.request_repaint();
                    }
                    break;
                }
            }
        })?;
        Ok(Listener {
            event: self.0,
            stop,
            shutdown_requested,
            worker: Some(worker),
        })
    }
}

fn create_event(name: &[u16]) -> io::Result<(OwnedHandle, bool)> {
    const ACL: &str = "D:(A;;GA;;;SY)(A;;GA;;;OW)(A;;0x100002;;;IU)";
    if crate::elevation::is_admin() {
        // A normal shortcut can restore the window when System VPN elevated the app.
        let descriptor = format!("{ACL}S:(ML;;NW;;;ME)");
        match create_event_with_descriptor(name, &descriptor) {
            Err(error) if error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD as i32) => {}
            result => return result,
        }
    }
    create_event_with_descriptor(name, ACL)
}

fn create_event_with_descriptor(name: &[u16], descriptor: &str) -> io::Result<(OwnedHandle, bool)> {
    let sddl: Vec<_> = descriptor
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let handle = unsafe {
        CreateEventExW(&attributes, name.as_ptr(), 0, SYNCHRONIZE | EVENT_MODIFY_STATE)
    };
    let error = unsafe { GetLastError() };
    unsafe { LocalFree(descriptor) };
    if handle.is_null() {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let event = unsafe { OwnedHandle::from_raw_handle(handle) };
    Ok((event, error == ERROR_ALREADY_EXISTS))
}

pub struct Listener {
    event: OwnedHandle,
    stop: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Listener {
    pub fn take_shutdown_request(&self) -> bool {
        self.shutdown_requested.swap(false, Ordering::AcqRel)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe { SetEvent(self.event.as_raw_handle()) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_can_request_shutdown_and_the_instance_can_be_reclaimed() {
        let pid = std::process::id();
        let name: Vec<_> = format!(r"Local\SSHVPN-Connect.SshTunnel.Test.{pid}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let shutdown_name: Vec<_> = format!(r"Local\SSHVPN-Connect.SshTunnel.Shutdown.{pid}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let listener = Instance::claim(&name)
            .unwrap()
            .unwrap()
            .listen(0, eframe::egui::Context::default())
            .unwrap();
        let raw = unsafe { OpenEventW(SYNCHRONIZE | EVENT_MODIFY_STATE, 0, shutdown_name.as_ptr()) };
        assert!(!raw.is_null());
        let request = unsafe { OwnedHandle::from_raw_handle(raw) };
        assert_ne!(unsafe { SetEvent(request.as_raw_handle()) }, 0);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !listener.take_shutdown_request() {
            assert!(Instant::now() < deadline, "Missing shutdown request");
            thread::sleep(Duration::from_millis(10));
        }
        drop(listener);
        assert!(Instance::wait_until_closed(&name, Duration::ZERO));
    }
}
