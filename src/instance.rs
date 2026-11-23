use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::System::Threading::{
    CreateEventExW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
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
        // A regular desktop shortcut must be able to signal an elevated window.
        // Other interactive processes in the same session can only signal or wait.
        let sddl: Vec<_> = "D:(A;;GA;;;SY)(A;;GA;;;OW)(A;;0x100002;;;IU)S:(ML;;NW;;;ME)"
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
            CreateEventExW(
                &attributes,
                name.as_ptr(),
                0,
                SYNCHRONIZE | EVENT_MODIFY_STATE,
            )
        };
        let error = unsafe { GetLastError() };
        unsafe { LocalFree(descriptor) };
        if handle.is_null() {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let event = unsafe { OwnedHandle::from_raw_handle(handle) };
        if error == ERROR_ALREADY_EXISTS {
            if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            return Ok(None);
        }
        Ok(Some(Self(event)))
    }

    pub fn listen(self, hwnd: isize, ctx: eframe::egui::Context) -> io::Result<Listener> {
        let event = self.0.try_clone()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::Builder::new().spawn(move || {
            while unsafe { WaitForSingleObject(event.as_raw_handle(), INFINITE) } == WAIT_OBJECT_0
                && !stopped.load(Ordering::Acquire)
            {
                crate::tray::restore(hwnd);
                ctx.request_repaint();
            }
        })?;
        Ok(Listener {
            event: self.0,
            stop,
            worker: Some(worker),
        })
    }
}

pub struct Listener {
    event: OwnedHandle,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
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
