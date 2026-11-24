use crate::profiles::Ipv4Prefix;
use crate::vpn::{AdapterCache, WindowsTunnel, MTU};
use crate::vpn_log::Log;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_APPEND_DATA, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    PIPE_ACCESS_DUPLEX, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTSGetActiveConsoleSessionId, WTSQueryUserToken,
};
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

const SERVICE: &str = "SshTunnelNetwork";
const PIPE: &str = r"\\.\pipe\SshTunnelNetwork-v2";
const MAX_REQUEST: usize = 512 * 1024;
const PIPE_BUFFER: u32 = 1024 * 1024;
const CLIENT_ACCESS: u32 = (FILE_GENERIC_READ | FILE_GENERIC_WRITE) & !FILE_APPEND_DATA;
const POLL: Duration = Duration::from_millis(2);
static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", content = "config", deny_unknown_fields)]
pub(crate) enum ServiceRequest {
    System(Request),
    Apps {
        server: Ipv4Addr,
        port: u16,
        pid: u32,
        paths: Vec<String>,
    },
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub id: Option<u128>,
    pub name: String,
    pub server: Ipv4Addr,
    pub address: Ipv4Addr,
    pub split: Option<Vec<String>>,
    pub filtered: Option<Vec<(Ipv4Addr, u8)>>,
}

impl Request {
    fn validated_routes(&self) -> Result<Option<Vec<Ipv4Prefix>>, String> {
        let [a, b, c, d] = self.address.octets();
        if !((a == 172 && b == 31 && c == 252 && d % 4 == 2)
            || self.address == Ipv4Addr::new(172, 31, 255, 2))
            || self.name.len() > 1024
            || self.server.is_unspecified()
            || self.server.is_loopback()
            || self.server.is_multicast()
            || self
                .split
                .as_ref()
                .is_some_and(|routes| routes.len() > 1024)
            || self.filtered.as_ref().is_some_and(|routes| {
                self.split.is_some()
                    || routes.len() > 32768
                    || routes.iter().any(|(_, bits)| *bits > 32)
            })
        {
            return Err("Недопустимые параметры System VPN.".into());
        }
        self.split
            .as_ref()
            .map(|routes| {
                routes
                    .iter()
                    .map(|entry| {
                        Ipv4Prefix::parse(entry)
                            .filter(|prefix| !prefix.is_host() || !prefix.contains(self.server))
                            .ok_or_else(|| "Недопустимый маршрут System VPN.".to_owned())
                    })
                    .collect()
            })
            .transpose()
    }
}

pub(crate) struct Channel {
    file: File,
}

impl Channel {
    fn handle(&self) -> HANDLE {
        self.file.as_raw_handle()
    }

    fn accept(&self) -> bool {
        // In PIPE_NOWAIT mode, nonzero only means the listener became available.
        if unsafe { ConnectNamedPipe(self.handle(), null_mut()) } != 0 {
            return false;
        }
        match io::Error::last_os_error()
            .raw_os_error()
            .map(|code| code as u32)
        {
            Some(ERROR_PIPE_CONNECTED) => true,
            Some(ERROR_NO_DATA) => {
                unsafe { DisconnectNamedPipe(self.handle()) };
                false
            }
            _ => false,
        }
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.write_all(&Self::frame(bytes))
    }

    fn frame(bytes: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(4 + bytes.len());
        frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        frame.extend_from_slice(bytes);
        frame
    }

    fn receive(&mut self, max: usize) -> io::Result<Option<Vec<u8>>> {
        let (mut read, mut available) = (0, 0);
        let mut header = [0u8; 4];
        let ok = unsafe {
            PeekNamedPipe(
                self.handle(),
                header.as_mut_ptr().cast(),
                4,
                &mut read,
                &mut available,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if read != 4 {
            return Ok(None);
        }
        let len = u32::from_le_bytes(header) as usize;
        if len > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid VPN frame",
            ));
        }
        if (available as usize) < len + 4 {
            return Ok(None);
        }
        self.file.read_exact(&mut header)?;
        let mut data = vec![0; len];
        self.file.read_exact(&mut data)?;
        Ok(Some(data))
    }

    fn wait(
        &mut self,
        max: usize,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> io::Result<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "VPN cancelled"));
            }
            if let Some(message) = self.receive(max)? {
                return Ok(message);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows VPN service timeout",
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    pub(crate) fn connect(
        request: &ServiceRequest,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        let pid = service_pid().map_err(|error| {
            format!(
                "Служба SSH Tunnel Network недоступна. Установите клиент через установщик. {error}"
            )
        })?;
        let deadline = Instant::now() + Duration::from_secs(8);
        let file = loop {
            // Explicit rights exclude FILE_CREATE_PIPE_INSTANCE. The server cannot impersonate the client.
            match OpenOptions::new()
                .access_mode(CLIENT_ACCESS)
                .security_qos_flags(SECURITY_SQOS_PRESENT)
                .open(PIPE)
            {
                Ok(file) => break file,
                Err(error)
                    if matches!(error.raw_os_error(), Some(2 | 231))
                        && Instant::now() < deadline
                        && !cancelled.load(Ordering::Acquire) =>
                {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let mut channel = Self { file };
        let mut actual_pid = 0;
        if unsafe { GetNamedPipeServerProcessId(channel.handle(), &mut actual_pid) } == 0
            || pid != actual_pid
        {
            return Err("Не удалось подтвердить подлинность службы SSH Tunnel Network.".into());
        }
        let payload = serde_json::to_vec(request).map_err(|error| error.to_string())?;
        if payload.len() > MAX_REQUEST {
            return Err("Слишком большой список маршрутов VPN.".into());
        }
        channel.send(&payload).map_err(|error| error.to_string())?;
        let status = channel
            .wait(4096, Duration::from_secs(60), cancelled)
            .map_err(|error| error.to_string())?;
        if status != b"ok" {
            return Err(
                String::from_utf8_lossy(status.strip_prefix(&[0]).unwrap_or(&status)).into_owned(),
            );
        }
        Ok(channel)
    }

    pub(crate) fn inject(&mut self, packet: &[u8]) -> Result<(), String> {
        self.send(packet).map_err(|error| error.to_string())
    }

    pub(crate) fn poll_apps(&mut self) -> Result<(), String> {
        match self.receive(4096).map_err(|error| error.to_string())? {
            None => Ok(()),
            Some(status) => Err(String::from_utf8_lossy(
                status.strip_prefix(&[0]).unwrap_or(&status),
            )
            .into_owned()),
        }
    }

    pub(crate) fn packet(&mut self) -> Result<Option<Vec<u8>>, String> {
        for _ in 0..32 {
            let Some(data) = self.receive(4096).map_err(|error| error.to_string())? else {
                break;
            };
            if let Some(status) = data.strip_prefix(&[0]) {
                return if status == b"ok" {
                    Err("Служба Windows завершила туннель.".into())
                } else {
                    Err(String::from_utf8_lossy(status).into_owned())
                };
            }
            if is_ipv4_packet(&data) {
                return Ok(Some(data));
            }
        }
        Ok(None)
    }

    pub(crate) fn stop(mut self) -> Result<(), String> {
        self.send(&[]).map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            let status = self
                .wait(4096, timeout, &STOP)
                .map_err(|error| error.to_string())?;
            if status.first() == Some(&0) {
                break status;
            }
            if Instant::now() >= deadline {
                return Err("Истекло время отключения службы VPN.".into());
            }
        };
        let status = &status[1..];
        if status == b"ok" {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&status).into_owned())
        }
    }
}

fn service_pid() -> io::Result<u32> {
    unsafe {
        let manager = OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return Err(io::Error::last_os_error());
        }
        let service = OpenServiceW(manager, wide(SERVICE).as_ptr(), SERVICE_QUERY_STATUS);
        let open_error = io::Error::last_os_error();
        CloseServiceHandle(manager);
        if service.is_null() {
            return Err(open_error);
        }
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0;
        let ok = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of_val(&status) as u32,
            &mut needed,
        );
        let error = io::Error::last_os_error();
        CloseServiceHandle(service);
        if ok == 0 {
            return Err(error);
        }
        if status.dwCurrentState != SERVICE_RUNNING || status.dwProcessId == 0 {
            return Err(io::Error::other("Служба VPN остановлена."));
        }
        Ok(status.dwProcessId)
    }
}

fn listener() -> io::Result<Channel> {
    // Let Windows assign ownership from the service token. The DACL still limits
    // interactive users to client access and allows SYSTEM to create the server.
    create_listener(PIPE, &listener_acl())
}

fn listener_acl() -> String {
    format!("D:P(A;;GA;;;SY)(A;;0x{CLIENT_ACCESS:x};;;IU)")
}

fn create_listener(pipe: &str, acl: &str) -> io::Result<Channel> {
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(acl).as_ptr(),
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
        CreateNamedPipeW(
            wide(pipe).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER,
            PIPE_BUFFER,
            0,
            &attributes,
        )
    };
    let error = io::Error::last_os_error();
    unsafe { LocalFree(descriptor) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(error);
    }
    Ok(Channel {
        file: unsafe { File::from_raw_handle(handle) },
    })
}

fn authorized_session(pipe: HANDLE) -> Option<u32> {
    let session = unsafe { WTSGetActiveConsoleSessionId() };
    let (mut peer_session, mut peer_pid) = (0, 0);
    if session == u32::MAX
        || session == 0
        || unsafe { GetNamedPipeClientSessionId(pipe, &mut peer_session) } == 0
        || peer_session != session
        || unsafe { GetNamedPipeClientProcessId(pipe, &mut peer_pid) } == 0
    {
        return None;
    }
    console_process(session, peer_pid).map(|_| session)
}

fn console_process(session: u32, peer_pid: u32) -> Option<OwnedHandle> {
    let mut process_session = 0;
    if unsafe { ProcessIdToSessionId(peer_pid, &mut process_session) } == 0
        || process_session != session
    {
        return None;
    }

    #[repr(C)]
    struct UserBuffer {
        user: TOKEN_USER,
        sid: [u8; 68],
    }

    fn read_user(token: &OwnedHandle, buffer: &mut UserBuffer) -> bool {
        let mut length = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                (buffer as *mut UserBuffer).cast(),
                std::mem::size_of::<UserBuffer>() as u32,
                &mut length,
            ) != 0
        }
    }

    let peer_process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, peer_pid) };
    if peer_process.is_null() {
        return None;
    }
    let peer_process = unsafe { OwnedHandle::from_raw_handle(peer_process) };
    let mut peer_token = null_mut();
    if unsafe { OpenProcessToken(peer_process.as_raw_handle(), TOKEN_QUERY, &mut peer_token) } == 0
    {
        return None;
    }
    let peer_token = unsafe { OwnedHandle::from_raw_handle(peer_token) };
    let mut console_token = null_mut();
    if unsafe { WTSQueryUserToken(session, &mut console_token) } == 0 {
        return None;
    }
    let console_token = unsafe { OwnedHandle::from_raw_handle(console_token) };
    let mut peer = UserBuffer {
        user: TOKEN_USER::default(),
        sid: [0; 68],
    };
    let mut console = UserBuffer {
        user: TOKEN_USER::default(),
        sid: [0; 68],
    };
    (read_user(&peer_token, &mut peer)
        && read_user(&console_token, &mut console)
        && unsafe { EqualSid(peer.user.User.Sid, console.user.User.Sid) } != 0)
        .then_some(peer_process)
}

fn serve_connection(channel: &mut Channel, driver: &wintun::Wintun, adapters: &AdapterCache) {
    let Some(client_session) = authorized_session(channel.handle()) else {
        if channel
            .send("VPN доступен только в активном локальном сеансе Windows.".as_bytes())
            .is_ok()
        {
            let _ = channel.wait(0, Duration::from_secs(2), &STOP);
        }
        return;
    };
    let result = (|| -> Result<(), String> {
        let payload = channel
            .wait(MAX_REQUEST, Duration::from_secs(5), &STOP)
            .map_err(|error| error.to_string())?;
        let request: ServiceRequest = serde_json::from_slice(&payload)
            .map_err(|_| "Недопустимый запрос службы VPN.".to_owned())?;
        let request = match request {
            ServiceRequest::System(request) => request,
            ServiceRequest::Apps {
                server,
                port,
                pid,
                paths,
            } => {
                return serve_apps(channel, client_session, server, port, pid, paths);
            }
        };
        let split = request.validated_routes()?;
        let log = Log::new();
        let network = WindowsTunnel::start(
            driver,
            adapters,
            request.id,
            &request.name,
            request.server,
            request.address,
            split.as_deref(),
            request.filtered.as_deref(),
            &STOP,
            log,
        )?;
        let relay = channel
            .send(b"ok")
            .and_then(|()| pump(channel, &network, client_session));
        let cleanup = network.stop(adapters, log);
        relay.map_err(|error| error.to_string()).and(cleanup)
    })();
    let status = match &result {
        Ok(()) => b"ok".as_slice(),
        Err(error) => error.as_bytes(),
    };
    let status = if status.len() <= 4095 {
        status
    } else {
        b"VPN service error"
    };
    let mut response = Vec::with_capacity(status.len() + 1);
    response.push(0);
    response.extend_from_slice(status);
    if channel.send(&response).is_ok() {
        // DisconnectNamedPipe discards unread data, so let the client consume the response.
        let _ = channel.wait(0, Duration::from_secs(2), &STOP);
    }
}

fn valid_app_paths(paths: &[String]) -> bool {
    !paths.is_empty()
        && paths.len() <= 256
        && paths.iter().all(|value| {
            let path = std::path::Path::new(value);
            value.len() <= 1024
                && !value.chars().any(char::is_control)
                && path.is_absolute()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        })
}

fn serve_apps(
    channel: &mut Channel,
    session: u32,
    server: Ipv4Addr,
    port: u16,
    pid: u32,
    paths: Vec<String>,
) -> Result<(), String> {
    if port == 0
        || server.is_unspecified()
        || server.is_loopback()
        || server.is_multicast()
        || !valid_app_paths(&paths)
    {
        return Err("Недопустимые параметры маршрутизации по приложениям.".into());
    }
    let process = console_process(session, pid)
        .ok_or("SSH-процесс не принадлежит текущему пользователю Windows.")?;
    let expected = crate::tunnel::system_program(r"OpenSSH\ssh.exe")?;
    let mut image = vec![0u16; 32768];
    let mut length = image.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.as_raw_handle(), 0, image.as_mut_ptr(), &mut length)
    } == 0
        || !String::from_utf16_lossy(&image[..length as usize])
            .eq_ignore_ascii_case(&expected.to_string_lossy())
        || !crate::tunnel::owns_socks_listener(
            &crate::tunnel::system_program("netstat.exe")?,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, port),
            pid,
        )?
    {
        return Err("Не удалось подтвердить локальный SOCKS5-порт OpenSSH.".into());
    }
    if channel
        .receive(0)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let mut engine = crate::app_tunnel::Engine::start(server, port, paths)?;
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut ready = false;
        while !STOP.load(Ordering::Acquire) && session == unsafe { WTSGetActiveConsoleSessionId() }
        {
            if channel
                .receive(0)
                .map_err(|error| error.to_string())?
                .is_some()
            {
                break;
            }
            let mut code = 0;
            if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0
                || code != STILL_ACTIVE as u32
            {
                return Err("SSH-процесс завершился.".into());
            }
            if engine.poll()? && !ready {
                channel.send(b"ok").map_err(|error| error.to_string())?;
                ready = true;
            }
            if !ready && Instant::now() >= deadline {
                return Err("Истекло время запуска sing-box.".into());
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        Ok(())
    })();
    result.and(engine.stop())
}

fn is_ipv4_packet(data: &[u8]) -> bool {
    (20..=MTU).contains(&data.len()) && data[0] >> 4 == 4
}

fn pump(channel: &mut Channel, network: &WindowsTunnel, client_session: u32) -> io::Result<()> {
    let mut last_check = Instant::now();
    let mut outgoing = Vec::new();
    let mut sent = 0;
    while !STOP.load(Ordering::Acquire) {
        if last_check.elapsed() >= Duration::from_secs(1) {
            if client_session != unsafe { WTSGetActiveConsoleSessionId() } {
                break;
            }
            last_check = Instant::now();
        }
        let mut busy = false;
        for _ in 0..32 {
            let Some(packet) = channel.receive(MTU)? else {
                break;
            };
            if packet.is_empty() {
                // Finish any partially transmitted frame before the final status.
                if sent != 0 {
                    channel.file.write_all(&outgoing[sent..])?;
                }
                return Ok(());
            }
            if is_ipv4_packet(&packet) {
                network.inject(&packet);
            }
            busy = true;
        }
        for _ in 0..32 {
            if sent == outgoing.len() {
                let Some(packet) = network.packet().map_err(io::Error::other)? else {
                    break;
                };
                if !is_ipv4_packet(&packet) {
                    busy = true;
                    continue;
                }
                outgoing = Channel::frame(&packet);
                sent = 0;
            }
            let count = match channel.file.write(&outgoing[sent..]) {
                Ok(count) => count,
                Err(error) if error.raw_os_error() == Some(ERROR_NO_DATA as i32) => 0,
                Err(error) => return Err(error),
            };
            if count == 0 {
                break;
            }
            sent += count;
            busy = true;
        }
        if !busy {
            std::thread::sleep(POLL);
        }
    }
    Ok(())
}

unsafe extern "system" fn control(action: u32) {
    if matches!(action, SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN) {
        STOP.store(true, Ordering::Release);
    }
}

fn report(handle: SERVICE_STATUS_HANDLE, state: u32, exit: u32) {
    let active = state == SERVICE_RUNNING;
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if active {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: exit,
        ..Default::default()
    };
    unsafe { SetServiceStatus(handle, &status) };
}

unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let handle = unsafe { RegisterServiceCtrlHandlerW(wide(SERVICE).as_ptr(), Some(control)) };
    if handle.is_null() {
        return;
    }
    let executable = std::env::current_exe();
    let startup_log = executable
        .as_ref()
        .ok()
        .map(|path| path.with_file_name("vpn-service-startup.log"));
    let result = (|| -> Result<(), (u32, String)> {
        let dll = executable
            .map_err(|error| {
                (
                    ERROR_GEN_FAILURE,
                    format!("Locate the service executable: {error}"),
                )
            })?
            .with_file_name("wintun.dll");
        let driver = unsafe { wintun::load_from_path(dll) }
            .map_err(|error| (ERROR_MOD_NOT_FOUND, format!("Load wintun.dll: {error}")))?;
        let mut channel = listener().map_err(|error| {
            (
                error.raw_os_error().unwrap_or(ERROR_GEN_FAILURE as i32) as u32,
                format!("Create the VPN named pipe: {error}"),
            )
        })?;
        let adapters = AdapterCache::default();
        if let Some(path) = &startup_log {
            let _ = std::fs::remove_file(path);
        }
        report(handle, SERVICE_RUNNING, 0);
        while !STOP.load(Ordering::Acquire) {
            if channel.accept() {
                serve_connection(&mut channel, &driver, &adapters);
                unsafe { DisconnectNamedPipe(channel.handle()) };
            } else {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        report(handle, SERVICE_STOP_PENDING, 0);
        Ok(())
    })();
    let exit = match result {
        Ok(()) => 0,
        Err((code, message)) => {
            if let Some(path) = startup_log {
                let _ = std::fs::write(path, format!("{message}\nWindows error: {code}\n"));
            }
            code
        }
    };
    report(handle, SERVICE_STOPPED, exit);
}

pub(crate) fn run() -> io::Result<()> {
    let mut name = wide(SERVICE);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: null_mut(),
            lpServiceProc: None,
        },
    ];
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_pipe_transfers_config_and_cleanup_status() {
        let name = format!("{PIPE}-test-{}", crate::vpn::new_adapter_id().unwrap());
        let mut server = create_listener(&name, &listener_acl()).unwrap();
        assert!(!server.accept());
        let file = OpenOptions::new()
            .access_mode(CLIENT_ACCESS)
            .security_qos_flags(SECURITY_SQOS_PRESENT)
            .open(&name)
            .unwrap();
        let mut client = Channel { file };
        assert!(server.accept());

        client.send(b"profile").unwrap();
        assert_eq!(server.receive(MAX_REQUEST).unwrap().unwrap(), b"profile");
        server.send(&[0x60; 40]).unwrap();
        server.send(&[0x45; 20]).unwrap();
        assert_eq!(client.packet().unwrap().unwrap(), [0x45; 20]);
        server.send(b"\0service error").unwrap();
        assert_eq!(client.packet().unwrap_err(), "service error");
        client.send(&[]).unwrap();
        assert!(server.receive(MTU).unwrap().unwrap().is_empty());
        server.send(b"\0ok").unwrap();
        assert_eq!(client.receive(4096).unwrap().unwrap(), b"\0ok");
        drop(client);
        assert_ne!(unsafe { DisconnectNamedPipe(server.handle()) }, 0);
        assert!(!server.accept());
        let _second_client = OpenOptions::new()
            .access_mode(CLIENT_ACCESS)
            .open(&name)
            .unwrap();
        assert!(server.accept());
    }

    fn request() -> Request {
        Request {
            id: Some(u128::MAX),
            name: "Домашний сервер".into(),
            server: Ipv4Addr::new(203, 0, 113, 24),
            address: Ipv4Addr::new(172, 31, 252, 6),
            split: Some(vec!["192.0.2.0/24".into()]),
            filtered: None,
        }
    }

    #[test]
    fn service_round_trips_a_profile_and_validates_routes() {
        let encoded = serde_json::to_vec(&ServiceRequest::System(request())).unwrap();
        let ServiceRequest::System(decoded) = serde_json::from_slice(&encoded).unwrap() else {
            panic!("Expected a System VPN request");
        };
        assert_eq!(decoded.id, Some(u128::MAX));
        assert_eq!(
            decoded.validated_routes().unwrap().unwrap()[0].to_string(),
            "192.0.2.0/24"
        );
    }

    #[test]
    fn app_requests_accept_executable_paths_but_not_arbitrary_commands() {
        let request = ServiceRequest::Apps {
            server: Ipv4Addr::new(203, 0, 113, 24),
            port: 1080,
            pid: 1234,
            paths: vec![r"C:\Apps (Test)\Browser.EXE".into()],
        };
        let encoded = serde_json::to_string(&request).unwrap();
        let ServiceRequest::Apps { paths, .. } = serde_json::from_str(&encoded).unwrap() else {
            panic!("Expected an application-routing request");
        };
        assert!(valid_app_paths(&paths));
        assert!(!valid_app_paths(&["Browser.exe".into()]));
        assert!(!valid_app_paths(&[r"C:\Apps\script.cmd".into()]));
        let extra = encoded.replace("\"port\":1080", "\"port\":1080,\"command\":\"other.exe\"");
        assert_ne!(extra, encoded);
        assert!(serde_json::from_str::<ServiceRequest>(&extra).is_err());
    }

    #[test]
    fn service_rejects_invalid_addresses_routes_and_extra_fields() {
        let mut invalid = request();
        invalid.address = Ipv4Addr::new(192, 168, 1, 1);
        assert!(invalid.validated_routes().is_err());
        invalid.address = request().address;
        invalid.split = Some(vec!["203.0.113.24/32".into()]);
        assert!(invalid.validated_routes().is_err());
        invalid.split = None;
        invalid.filtered = Some(vec![(Ipv4Addr::UNSPECIFIED, 33)]);
        assert!(invalid.validated_routes().is_err());
        let mut extra = serde_json::to_string(&request()).unwrap();
        extra.pop();
        extra.push_str(r#","command":"arbitrary"}"#);
        assert!(serde_json::from_str::<Request>(&extra).is_err());
    }
}
