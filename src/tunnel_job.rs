use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::{Child, Command};
use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

pub(crate) struct TunnelJob(OwnedHandle);

impl TunnelJob {
    pub fn new() -> io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        };
        if configured == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(job))
    }

    pub(crate) fn spawn(&self, command: &mut Command) -> io::Result<Child> {
        let mut child = command.spawn()?;
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) } == 0 {
            let error = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(format!(
                "Could not register tunnel process cleanup: {error}"
            )));
        }
        Ok(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    #[test]
    fn closing_job_terminates_both_tunnel_processes() {
        let job = TunnelJob::new().unwrap();
        let cmd = crate::tunnel::system_program("cmd.exe").unwrap();
        let mut command = Command::new(cmd);
        command
            .creation_flags(0x0800_0000)
            .args(["/d", "/c", "set /p input="])
            .stdin(Stdio::piped())
            .stdout(Stdio::null());
        let mut ssh = job.spawn(&mut command).unwrap();
        let mut companion = job.spawn(&mut command).unwrap();
        assert!(ssh.try_wait().unwrap().is_none());
        assert!(companion.try_wait().unwrap().is_none());
        drop(job);
        let deadline = Instant::now() + Duration::from_secs(3);
        for child in [&mut ssh, &mut companion] {
            while child.try_wait().unwrap().is_none() {
                assert!(
                    Instant::now() < deadline,
                    "Tunnel process survived job closure"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
