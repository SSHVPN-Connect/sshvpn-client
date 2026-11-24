use crate::profiles::Ipv4Prefix;
use crate::tunnel::{ssh_command, ssh_error, system_program, Profile};
use crate::tunnel_job::TunnelJob;
use crate::vpn_log::Log;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::HashMap;
use std::env;
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    CreateIpForwardEntry2, DeleteIpForwardEntry2, InitializeIpForwardEntry, MIB_IPFORWARD_ROW2,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, IN_ADDR, IN_ADDR_0, MIB_IPPROTO_NETMGMT, SOCKADDR_IN, SOCKADDR_INET,
};

pub(crate) const MTU: usize = 1420;
const HELLO: &[u8; 8] = b"SSHVPN1\n";
const MULTI_HELLO: &[u8; 8] = b"SSHVPN2\n";
const INVITED_VPN_COMMAND: &str = "ssh-tunnel-vpn-v2";
const OWNER_VPN_COMMAND: &str = r#"set -eu
if [ "$(id -u)" -ne 0 ]; then
    echo 'System VPN requires an SSH login as root.' >&2
    exit 1
fi
exec /usr/local/libexec/ssh-tunnel-vpn --multi"#;

enum Status {
    ConfiguringAdapter,
    Ready,
}

#[derive(Clone, Default)]
pub(crate) struct AdapterCache(Arc<Mutex<HashMap<u128, Arc<wintun::Adapter>>>>);

impl AdapterCache {
    pub(crate) fn remove(&self, id: u128) -> Option<Arc<wintun::Adapter>> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&id)
    }

    fn insert(&self, id: u128, adapter: Arc<wintun::Adapter>) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id, adapter);
    }
}

pub(crate) fn new_adapter_id() -> Result<u128, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(u128::from_be_bytes(bytes))
}

fn name_adapter(
    adapter: &wintun::Adapter,
    index: u32,
    profile: &str,
    ip: Ipv4Addr,
) -> Result<(), String> {
    let mut label = String::new();
    let mut units = 0;
    for ch in profile.trim().chars() {
        if units + ch.len_utf16() > 48 {
            break;
        }
        units += ch.len_utf16();
        label.push(if ch.is_control() || "\\/:*?\"<>|".contains(ch) {
            ' '
        } else {
            ch
        });
    }
    let label = label.trim();
    let label = if label.is_empty() {
        "SSH Tunnel VPN"
    } else {
        label
    };
    let name = format!("{label} — {ip}");
    let suffix = adapter.get_guid() as u32;
    let duplicate_name = format!("{name} ({suffix:08x})");
    let current = adapter.get_name().map_err(|error| error.to_string())?;
    if current == name || current == duplicate_name {
        return Ok(());
    }
    // Encode the user-supplied display text instead of interpolating it into PowerShell code.
    let encoded = STANDARD.encode(&name);
    powershell(&format!(
        r"
        $name = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))
        $adapters = @(Get-NetAdapter -IncludeHidden)
        $adapter = $adapters | Where-Object {{ $_.ifIndex -eq {index} }} | Select-Object -First 1
        if (!$adapter) {{ throw 'Wintun adapter not found' }}
        if ($adapters | Where-Object {{ $_.ifIndex -ne {index} -and $_.Name -eq $name }}) {{
            $name += ' ({suffix:08x})'
        }}
        Rename-NetAdapter -InputObject $adapter -NewName $name -Confirm:$false
        "
    ))
    .map(|_| ())
}

pub struct Vpn {
    log: Log,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
    status: mpsc::Receiver<Result<Status, String>>,
    worker: Option<JoinHandle<io::Result<()>>>,
    ready: bool,
    configuring_adapter: bool,
}

impl Vpn {
    pub fn start(
        profile: Profile,
        profile_name: String,
        password_auth: bool,
        password: String,
        split: Option<Vec<Ipv4Prefix>>,
        ru_cache: Option<PathBuf>,
        adapter_id: Option<u128>,
        adapters: AdapterCache,
    ) -> Result<Self, String> {
        let dll = env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("wintun.dll");
        if !dll.is_file() {
            return Err("Place the official x64 wintun.dll next to ssh-tunnel.exe.".into());
        }
        let child = Arc::new(Mutex::new(None));
        let child_for_worker = child.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (tx, status) = mpsc::channel();
        let log = Log::new();
        let worker = thread::Builder::new()
            .spawn(move || {
                let mut connecting = Some(log.begin("connect"));
                let job = TunnelJob::new();
                let mut worker_ready = false;
                let result = (|| {
                    let job = job.as_ref().map_err(|error| format!("Could not configure VPN SSH process cleanup: {error}"))?;
                    let prefixes = if let Some(directory) = &ru_cache {
                        let regions = log.step("load_ru_ranges", || crate::ru_ranges::load(directory))?;
                        if let Some(routes) = &split {
                            regions.validate_included(routes)?;
                            None
                        } else {
                            Some(regions.non_ru_prefixes())
                        }
                    } else {
                        None
                    };
                    let ip = log.step("resolve_endpoint", || (profile.server.as_str(), 22).to_socket_addrs())
                        .map_err(|e| e.to_string())?
                        .find_map(|address| match address.ip() {
                            std::net::IpAddr::V4(ip) => Some(ip),
                            _ => None,
                        })
                        .ok_or("System VPN requires an IPv4 SSH endpoint.")?;
                    if split.as_ref().is_some_and(|routes| routes.iter().any(|route| route.is_host() && route.contains(ip))) {
                        return Err("Удалите адрес SSH-сервера /32 из CIDR-списка: соединение с сервером должно идти напрямую.".into());
                    }
                    let (mut command, mut diagnostics) =
                        ssh_command(&profile, password_auth, password)?;
                    let _known_host = log.step("prepare_host_key", || crate::tunnel::pin_host(&mut command, &profile))?;
                    let mut process = child_for_worker.lock().map_err(|e| e.to_string())?;
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let invited = profile.is_invited();
                    let server_command = if invited {
                        INVITED_VPN_COMMAND
                    } else {
                        OWNER_VPN_COMMAND
                    };
                    command.stdin(Stdio::piped()).stdout(Stdio::piped()).args([
                        "-4",
                        "-o",
                        &format!("HostKeyAlias={}", profile.server),
                        "-o",
                        "LogLevel=ERROR",
                        "--",
                        &ip.to_string(),
                        server_command,
                    ]);
                    let mut ssh = log.step("spawn_ssh", || job.spawn(&mut command)).map_err(|e| e.to_string())?;
                    let stdin = ssh.stdin.take().expect("piped SSH stdin");
                    let stdout = ssh.stdout.take().expect("piped SSH stdout");
                    *process = Some(ssh);
                    drop(process);

                    let (hello_tx, hello_rx) = mpsc::channel();
                    let (frames_tx, frames_rx) = mpsc::sync_channel(128);
                    thread::spawn(move || read_packets(stdout, hello_tx, frames_tx));
                    let client_address = log.step("ssh_auth_and_server_greeting", || wait_for_greeting(&hello_rx, Duration::from_secs(22)))
                        .map_err(|error| {
                            let error = ssh_error(&mut diagnostics, error);
                            if invited {
                                format!("Подключение по приглашению не запустилось. Попросите владельца проверить обновление сервера во вкладке «Пользователи».\n{error}")
                            } else {
                                format!("Подключение VPN не запустилось. Если серверу требуется настройка, откройте «Пользователи» → «Обновить сервер».\n{error}")
                            }
                        })?;
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    tx.send(Ok(Status::ConfiguringAdapter)).map_err(|e| e.to_string())?;
                    let mut network = if crate::elevation::is_admin() {
                        let driver = unsafe { wintun::load_from_path(dll) }.map_err(|e| e.to_string())?;
                        Network::Local(WindowsTunnel::start(
                            &driver, &adapters, adapter_id, &profile_name, ip, client_address,
                            split.as_deref(), prefixes.as_deref(), &worker_cancelled, log,
                        )?)
                    } else {
                        let request = crate::vpn_service::Request {
                            id: adapter_id,
                            name: profile_name,
                            server: ip,
                            address: client_address,
                            split: split.map(|routes| routes.iter().map(ToString::to_string).collect()),
                            filtered: prefixes,
                        };
                        Network::Service(log.step("configure_vpn_service", || {
                            crate::vpn_service::Channel::connect(
                                &crate::vpn_service::ServiceRequest::System(request),
                                &worker_cancelled,
                            )
                        })?)
                    };
                    worker_ready = true;
                    if let Some(trace) = connecting.take() {
                        trace.finish("ready");
                    }
                    let result = tx.send(Ok(Status::Ready)).map_err(|e| e.to_string())
                        .and_then(|()| forward(&mut network, stdin, &frames_rx, &worker_cancelled));
                    let graceful = if result.is_ok() && worker_cancelled.load(Ordering::Acquire) {
                        log.step("wait_for_server_exit", || wait_for_ssh_exit(&child_for_worker, &frames_rx)).map_err(|error| error.to_string())
                    } else {
                        Ok(())
                    };
                    let cleanup = match network {
                        Network::Local(network) => network.stop(&adapters, log),
                        Network::Service(network) => log.step("stop_vpn_service", || network.stop()),
                    };
                    result.and(graceful).and(cleanup)
                })();
                if let Some(trace) = connecting {
                    trace.finish(if worker_cancelled.load(Ordering::Acquire) { "cancelled" } else { "failed" });
                }
                if let Ok(mut child) = child_for_worker.lock() {
                    if let Some(mut ssh) = child.take() {
                        let _ = ssh.kill();
                        let _ = ssh.wait();
                    }
                }
                if worker_cancelled.load(Ordering::Acquire) {
                    return if result.is_err() && !worker_ready { Ok(()) } else { result.map_err(io::Error::other) };
                }
                let _ = tx.send(result.and(Err("VPN session ended.".into())));
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            log,
            child,
            cancelled,
            status,
            worker: Some(worker),
            ready: false,
            configuring_adapter: false,
        })
    }

    pub fn poll(&mut self) -> Result<(), String> {
        loop {
            match self.status.try_recv() {
                Ok(event) => match event? {
                    Status::ConfiguringAdapter => {
                        self.configuring_adapter = true;
                    }
                    Status::Ready => {
                        self.configuring_adapter = false;
                        self.ready = true;
                    }
                },
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Err("VPN worker stopped.".into()),
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn is_configuring_adapter(&self) -> bool {
        self.configuring_adapter
    }

    pub fn stop(&mut self) -> io::Result<()> {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let trace = self.log.begin("disconnect");
            if self.ready {
                let deadline = Instant::now() + Duration::from_secs(6);
                while !worker.is_finished() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(20));
                }
            }
            if !worker.is_finished() {
                if let Ok(mut child) = self.child.lock() {
                    if let Some(ssh) = child.as_mut() {
                        let _ = ssh.kill();
                    }
                }
            }
            let result = worker
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("VPN worker failed.")));
            trace.finish(if result.is_ok() { "stopped" } else { "failed" });
            return result;
        }
        Ok(())
    }
}

impl Drop for Vpn {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn wait_for_greeting(
    hello: &mpsc::Receiver<io::Result<Ipv4Addr>>,
    timeout: Duration,
) -> Result<Ipv4Addr, String> {
    hello
        .recv_timeout(timeout)
        .map_err(|error| format!("VPN handshake: {error}"))?
        .map_err(|error| error.to_string())
}

fn read_greeting(reader: &mut impl Read) -> io::Result<Ipv4Addr> {
    let mut greeting = [0; HELLO.len()];
    reader.read_exact(&mut greeting)?;
    match &greeting {
        HELLO => Ok(Ipv4Addr::new(172, 31, 255, 2)),
        MULTI_HELLO => {
            let mut address = [0; 4];
            reader.read_exact(&mut address)?;
            if address[..3] != [172, 31, 252] || address[3] % 4 != 2 {
                return Err(io::Error::other(
                    "Invalid IPv4 address assigned by the VPN server.",
                ));
            }
            Ok(address.into())
        }
        _ => Err(io::Error::other("Unexpected VPN server handshake.")),
    }
}

fn read_packets(
    pipe: impl Read,
    hello: mpsc::Sender<io::Result<Ipv4Addr>>,
    frames: mpsc::SyncSender<io::Result<Vec<u8>>>,
) {
    let mut reader = BufReader::new(pipe);
    let first = read_greeting(&mut reader);
    let valid = first.is_ok();
    if hello.send(first).is_err() || !valid {
        return;
    }
    loop {
        let result = (|| {
            let mut header = [0; 2];
            reader.read_exact(&mut header)?;
            let size = usize::from(u16::from_be_bytes(header));
            if !(20..=MTU).contains(&size) {
                return Err(io::Error::other("Invalid VPN packet length."));
            }
            let mut packet = vec![0; size];
            reader.read_exact(&mut packet)?;
            if packet[0] >> 4 != 4 {
                return Err(io::Error::other("Expected an IPv4 packet."));
            }
            Ok(packet)
        })();
        let failed = result.is_err();
        if matches!(
            frames.try_send(result),
            Err(mpsc::TrySendError::Disconnected(_))
        ) || failed
        {
            return;
        }
    }
}

fn wait_for_ssh_exit(
    child: &Mutex<Option<Child>>,
    frames: &mpsc::Receiver<io::Result<Vec<u8>>>,
) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for _ in 0..128 {
            if frames.try_recv().is_err() {
                break;
            }
        }
        let status = child
            .lock()
            .map_err(|_| io::Error::other("VPN SSH process lock failed."))?
            .as_mut()
            .ok_or_else(|| io::Error::other("VPN SSH process is missing."))?
            .try_wait()?;
        if status.is_some_and(|status| status.success()) {
            return Ok(());
        }
        if status.is_some() || Instant::now() >= deadline {
            return Err(io::Error::other(
                "Сервер не подтвердил отключение VPN. Подождите несколько секунд перед повторным подключением.",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

enum Network {
    Local(WindowsTunnel),
    Service(crate::vpn_service::Channel),
}

fn forward(
    network: &mut Network,
    mut stdin: ChildStdin,
    frames: &mpsc::Receiver<io::Result<Vec<u8>>>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    while !cancelled.load(Ordering::Acquire) {
        let mut busy = false;
        for _ in 0..32 {
            match frames.try_recv() {
                Ok(frame) => {
                    let data = frame.map_err(|e| format!("SSH VPN stream ended: {e}"))?;
                    match network {
                        Network::Local(network) => network.inject(&data),
                        Network::Service(network) => network.inject(&data)?,
                    }
                    busy = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Err("SSH VPN stream ended.".into()),
            }
        }
        for _ in 0..32 {
            let packet = match network {
                Network::Local(network) => network.packet()?,
                Network::Service(network) => network.packet()?,
            };
            let Some(data) = packet else { break };
            if (20..=MTU).contains(&data.len()) && data[0] >> 4 == 4 {
                stdin
                    .write_all(&(data.len() as u16).to_be_bytes())
                    .and_then(|()| stdin.write_all(&data))
                    .map_err(|e| e.to_string())?;
            }
            busy = true;
        }
        if !busy {
            thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}

pub(crate) struct WindowsTunnel {
    adapter_id: Option<u128>,
    routes: Routes,
    session: Arc<wintun::Session>,
    adapter: Arc<wintun::Adapter>,
    client_address: Ipv4Addr,
    full_tunnel: bool,
}

impl WindowsTunnel {
    pub(crate) fn inject(&self, data: &[u8]) {
        if let Ok(mut packet) = self.session.allocate_send_packet(data.len() as u16) {
            packet.bytes_mut().copy_from_slice(data);
            self.session.send_packet(packet);
        }
    }

    pub(crate) fn packet(&self) -> Result<Option<Vec<u8>>, String> {
        self.session
            .try_receive()
            .map(|packet| packet.map(|packet| packet.bytes().to_vec()))
            .map_err(|error| error.to_string())
    }

    pub(crate) fn start(
        driver: &wintun::Wintun,
        adapters: &AdapterCache,
        adapter_id: Option<u128>,
        profile_name: &str,
        ip: Ipv4Addr,
        client_address: Ipv4Addr,
        split: Option<&[Ipv4Prefix]>,
        prefixes: Option<&[(Ipv4Addr, u8)]>,
        worker_cancelled: &AtomicBool,
        log: Log,
    ) -> Result<Self, String> {
        let adapter = if let Some(adapter) = adapter_id.and_then(|id| adapters.remove(id)) {
            log.step("reuse_wintun_adapter", || Ok::<_, String>(adapter))?
        } else {
            log.step("create_wintun_adapter", || {
            let id = adapter_id.map_or_else(new_adapter_id, Ok)?;
            let name = format!("SSH Tunnel VPN {id:032x}");
            // The crate gives adapter-creation failures a generic message.
            unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
            let adapter_result =
                wintun::Adapter::create(driver, &name, "SSH Tunnel", Some(id));
            let windows_error = io::Error::last_os_error();
            adapter_result.map_err(|error| {
            if matches!(&error, wintun::Error::String(message) if message == "Failed to create adapter")
                && windows_error.raw_os_error().is_some_and(|code| code != 0)
            {
                format!("Не удалось создать адаптер Wintun: {windows_error}")
            } else {
                format!("Не удалось создать адаптер Wintun: {error}")
            }
            })
          })?
        };
        let index = adapter.get_adapter_index().map_err(|e| e.to_string())?;
        let _ = log.step("name_wintun_adapter", || {
            name_adapter(&adapter, index, profile_name, ip)
        });
        let session = log
            .step("start_wintun_session", || {
                adapter.start_session(wintun::MIN_RING_CAPACITY * 16)
            })
            .map_err(|e| e.to_string())?;
        let mut routes = Routes::new(index, ip);
        log.step("apply_routes", || {
            routes.apply(client_address, split, prefixes, worker_cancelled, log)
        })?;
        Ok(Self {
            adapter_id,
            routes,
            session: Arc::new(session),
            adapter,
            client_address,
            full_tunnel: split.is_none(),
        })
    }

    pub(crate) fn stop(self, adapters: &AdapterCache, log: Log) -> Result<(), String> {
        let Self {
            adapter_id,
            mut routes,
            session,
            adapter,
            client_address,
            full_tunnel,
        } = self;
        let cleanup = log.step("remove_routes", || {
            routes.remove_session_config(client_address, full_tunnel)
        });
        drop(routes);
        drop(session);
        if let Some(id) = adapter_id.filter(|_| cleanup.is_ok()) {
            adapters.insert(id, adapter);
        } else {
            let _ = log.step("close_wintun_adapter", || Ok::<_, io::Error>(drop(adapter)));
        }
        cleanup
    }
}

struct Routes {
    index: u32,
    server: Ipv4Addr,
    bypass: Option<(u32, Ipv4Addr, u32)>,
    filtered: Vec<MIB_IPFORWARD_ROW2>,
}

fn ipv4_socket(address: Ipv4Addr) -> SOCKADDR_INET {
    SOCKADDR_INET {
        Ipv4: SOCKADDR_IN {
            sin_family: AF_INET,
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: u32::from_ne_bytes(address.octets()),
                },
            },
            ..Default::default()
        },
    }
}

impl Routes {
    fn new(index: u32, server: Ipv4Addr) -> Self {
        Self {
            index,
            server,
            bypass: None,
            filtered: Vec::new(),
        }
    }

    fn ensure_bypass(&mut self) -> Result<(), String> {
        let server = self.server;
        let route = powershell(&format!(
            "$r = Find-NetRoute -RemoteIPAddress '{server}' | Where-Object {{ $_.DestinationPrefix }} | Select-Object -First 1; if (!$r) {{ throw 'No physical route to SSH server' }}; [Console]::WriteLine(('{{0}} {{1}}' -f $r.InterfaceIndex,$r.NextHop))"
        ))?;
        let (physical, hop) = route
            .trim()
            .split_once(' ')
            .ok_or("Invalid Windows route output.")?;
        let physical: u32 = physical
            .parse()
            .map_err(|_| "Invalid physical interface index.")?;
        let hop: Ipv4Addr = hop.parse().map_err(|_| "Invalid physical next hop.")?;
        let metric = 427;
        // Add a route only when there is no pre-existing host-specific route.
        let added = powershell(&format!(
            "if (!(Get-NetRoute -DestinationPrefix '{server}/32' -ErrorAction SilentlyContinue)) {{ New-NetRoute -DestinationPrefix '{server}/32' -InterfaceIndex {physical} -NextHop '{hop}' -RouteMetric {metric} -PolicyStore ActiveStore | Out-Null; [Console]::WriteLine('added') }}"
        ))?;
        if added.trim() == "added" {
            self.bypass = Some((physical, hop, metric));
        }
        Ok(())
    }

    fn apply(
        &mut self,
        client_address: Ipv4Addr,
        split: Option<&[Ipv4Prefix]>,
        filtered: Option<&[(Ipv4Addr, u8)]>,
        cancelled: &AtomicBool,
        log: Log,
    ) -> Result<(), String> {
        let index = self.index;
        if split.is_none_or(|routes| routes.iter().any(|route| route.contains(self.server))) {
            log.step("configure_ssh_bypass", || self.ensure_bypass())?;
        }
        let mut script = format!(
            r"
            New-NetIPAddress -InterfaceIndex {index} -IPAddress {client_address} -PrefixLength 30 -PolicyStore ActiveStore | Out-Null;
            Set-NetIPInterface -InterfaceIndex {index} -AddressFamily IPv4 -NlMtuBytes {MTU} -InterfaceMetric 4 -PolicyStore ActiveStore;
            "
        );
        if let Some(prefixes) = split {
            use std::fmt::Write as _;
            for prefix in prefixes {
                writeln!(script, "New-NetRoute -InterfaceIndex {index} -DestinationPrefix '{prefix}' -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;").expect("format into String");
            }
        } else {
            script.push_str(&format!(r"
            Set-DnsClientServerAddress -InterfaceIndex {index} -ServerAddresses @('1.1.1.1','1.0.0.1');
            New-NetIPAddress -InterfaceIndex {index} -IPAddress 'fd53:5353:4856::2' -PrefixLength 64 -PolicyStore ActiveStore | Out-Null;
            New-NetRoute -InterfaceIndex {index} -DestinationPrefix '2000::/3' -NextHop '::' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
            "));
            if filtered.is_none() {
                script.push_str(&format!(r"
                New-NetRoute -InterfaceIndex {index} -DestinationPrefix '0.0.0.0/1' -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
                New-NetRoute -InterfaceIndex {index} -DestinationPrefix '128.0.0.0/1' -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
                "));
            }
        }
        log.step("configure_windows_network", || powershell(&script))?;
        if let Some(prefixes) = filtered {
            log.step("apply_filtered_routes", || {
                for &(address, length) in prefixes {
                    if cancelled.load(Ordering::Acquire) {
                        return Err("Подключение отменено.".into());
                    }
                    if length == 32 && address == self.server {
                        continue;
                    }
                    let mut row = MIB_IPFORWARD_ROW2::default();
                    // Windows supplies the route lifetimes and other default fields.
                    unsafe { InitializeIpForwardEntry(&mut row) };
                    row.InterfaceIndex = index;
                    row.DestinationPrefix.Prefix = ipv4_socket(address);
                    row.DestinationPrefix.PrefixLength = length;
                    row.NextHop = ipv4_socket(Ipv4Addr::UNSPECIFIED);
                    row.Metric = 1;
                    row.Protocol = MIB_IPPROTO_NETMGMT;
                    let result = unsafe { CreateIpForwardEntry2(&row) };
                    if result != 0 {
                        return Err(format!(
                            "Не удалось добавить маршрут {address}/{length}: {}",
                            io::Error::from_raw_os_error(result as i32)
                        ));
                    }
                    self.filtered.push(row);
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    fn remove_session_config(
        &mut self,
        address: Ipv4Addr,
        full_tunnel: bool,
    ) -> Result<(), String> {
        let index = self.index;
        self.remove_filtered_routes();
        let mut script = format!(
            "Get-NetRoute -InterfaceIndex {index} -PolicyStore ActiveStore | Where-Object {{ $_.RouteMetric -eq 1 }} | Remove-NetRoute -Confirm:$false;
             Remove-NetIPAddress -InterfaceIndex {index} -IPAddress '{address}' -PolicyStore ActiveStore -Confirm:$false;"
        );
        if full_tunnel {
            script.push_str(&format!(
                "Remove-NetIPAddress -InterfaceIndex {index} -IPAddress 'fd53:5353:4856::2' -PolicyStore ActiveStore -Confirm:$false;
                 Set-DnsClientServerAddress -InterfaceIndex {index} -ResetServerAddresses;"
            ));
        }
        // Clean up the direct SSH exception even if clearing the tunnel interface fails.
        let result = powershell(&script);
        result.and(self.remove()).map(|_| ())
    }

    fn remove_filtered_routes(&mut self) {
        for row in self.filtered.drain(..).rev() {
            // The adapter also releases its routes when it is closed.
            unsafe { DeleteIpForwardEntry2(&row) };
        }
    }

    fn remove(&mut self) -> Result<(), String> {
        self.remove_filtered_routes();
        if let Some((index, hop, metric)) = self.bypass {
            let server = self.server;
            powershell(&format!(
                "Get-NetRoute -DestinationPrefix '{server}/32' -InterfaceIndex {index} -NextHop '{hop}' -PolicyStore ActiveStore -ErrorAction SilentlyContinue | Where-Object {{ $_.RouteMetric -eq {metric} }} | Remove-NetRoute -Confirm:$false"
            ))?;
            self.bypass = None;
        }
        Ok(())
    }
}

impl Drop for Routes {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

pub(crate) fn powershell(script: &str) -> Result<String, String> {
    let bootstrap = r"
        [Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        $ErrorActionPreference = 'Stop'
        try {
            & ([scriptblock]::Create([Console]::In.ReadToEnd()))
        } catch {
            [Console]::Error.WriteLine($_.Exception.Message)
            exit 1
        }
    ";
    let mut process = Command::new(system_program(r"WindowsPowerShell\v1.0\powershell.exe")?)
        .creation_flags(0x0800_0000)
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(bootstrap)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let write = process
        .stdin
        .take()
        .expect("piped PowerShell stdin")
        .write_all(script.as_bytes());
    let output = process.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Windows network configuration failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    write.map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpn_stream_starts_with_a_packet_greeting() {
        for multi_session in [false, true] {
            let mut stream = Vec::new();
            let address = if multi_session {
                stream.extend_from_slice(MULTI_HELLO);
                stream.extend_from_slice(&[172, 31, 252, 6]);
                Ipv4Addr::new(172, 31, 252, 6)
            } else {
                stream.extend_from_slice(HELLO);
                Ipv4Addr::new(172, 31, 255, 2)
            };
            stream.extend_from_slice(&20_u16.to_be_bytes());
            let mut packet = [0; 20];
            packet[0] = 0x45;
            stream.extend_from_slice(&packet);
            let (greetings, received) = mpsc::channel();
            let (frames, packets) = mpsc::sync_channel(2);

            read_packets(stream.as_slice(), greetings, frames);

            assert_eq!(received.recv().unwrap().unwrap(), address);
            assert_eq!(packets.recv().unwrap().unwrap(), packet);
        }
    }

    #[test]
    fn vpn_rejects_the_retired_server_install_request() {
        assert!(read_greeting(&mut b"SSHVPN0\n".as_slice()).is_err());
    }

    #[test]
    fn multi_vpn_rejects_addresses_outside_session_slots() {
        for address in [[172, 31, 252, 5], [172, 31, 255, 2]] {
            let stream = [MULTI_HELLO.as_slice(), &address].concat();
            assert!(read_greeting(&mut stream.as_slice()).is_err());
        }
    }

    #[test]
    fn powershell_runs_multiline_scripts_and_reports_utf8_errors() {
        let output =
            powershell("$value = 'проверка'\nif ($value) { [Console]::WriteLine($value) }")
                .unwrap();
        assert_eq!(output.trim(), "проверка");

        let error = powershell("throw 'Ошибка маршрута'").unwrap_err();
        assert!(error.contains("Ошибка маршрута"), "{error}");
    }
}
