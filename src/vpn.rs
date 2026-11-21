use crate::profiles::Ipv4Prefix;
use crate::tunnel::{ssh_command, ssh_error, system_program, Profile};
use std::env;
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MTU: usize = 1420;
const ADAPTER_GUID: u128 = 0x9648bb2f_098b_4d6c_8f35_7c011cdf8651;
const INSTALLER: &str = include_str!("../install-server.sh");
const INVITATION_INSTALLER: &str = crate::invites::INSTALLER;
const HELLO: &[u8; 8] = b"SSHVPN1\n";
const INSTALL_REQUEST: &[u8; 8] = b"SSHVPN0\n";

const VPN_REQUIRED: &str = r#"[ ! -x /usr/local/libexec/ssh-tunnel-vpn ] || [ "$(cat /proc/sys/net/ipv4/ip_forward)" != 1 ]"#;
const INVITATIONS_REQUIRED: &str = r#"[ ! -x /usr/local/libexec/ssh-tunnel-invite ] || [ ! -x /usr/local/libexec/ssh-tunnel-invite-session ] || [ ! -d /var/lib/ssh-tunnel-invites/records ] || [ ! -f /var/lib/ssh-tunnel-invites/installed-v3 ] || [ ! -r /etc/ssh/ssh-tunnel-invites.conf ] || [ ! -r /etc/sudoers.d/ssh-tunnel-invites ] || ! grep -Fxq 'Include /etc/ssh/ssh-tunnel-invites.conf' /etc/ssh/sshd_config"#;
const INVITED_VPN_COMMAND: &str = "ssh-tunnel-vpn";

fn is_invited(profile: &Profile) -> bool {
    profile.host_key.is_some() && crate::invites::valid_user(&profile.user)
}

fn server_installer() -> String {
    format!(
        "set -eu\nif {VPN_REQUIRED}; then\n(\n{INSTALLER}\n)\nfi\nif {INVITATIONS_REQUIRED}; then\n(\n{INVITATION_INSTALLER}\n)\nfi\n"
    ).replace("\r\n", "\n")
}

fn server_command(installer_size: usize) -> String {
    format!(
        r#"set -eu
handler=/usr/local/libexec/ssh-tunnel-vpn
if [ "$(id -u)" -ne 0 ]; then
    echo 'System VPN requires an SSH login as root.' >&2
    exit 1
fi
if {VPN_REQUIRED} || {INVITATIONS_REQUIRED}; then
    (
        umask 077
        printf 'SSHVPN0\n'
        script=$(mktemp /tmp/ssh-tunnel-install.XXXXXX)
        trap 'rm -f "$script"' EXIT
        head -c {installer_size} > "$script"
        if [ "$(wc -c < "$script")" -ne {installer_size} ]; then
            echo 'Server installer transfer was interrupted.' >&2
            exit 1
        fi
        flock -w 120 9
        sh "$script" </dev/null >&2
    ) 9>/run/ssh-tunnel-vpn-install.lock
fi
exec "$handler""#
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Greeting {
    Install,
    Ready,
}

enum Status {
    Installing,
    Ready,
}

pub struct Vpn {
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
    status: mpsc::Receiver<Result<Status, String>>,
    worker: Option<JoinHandle<()>>,
    ready: bool,
    installing: bool,
}

impl Vpn {
    pub fn start(
        profile: Profile,
        password_auth: bool,
        password: String,
        split: Option<Vec<Ipv4Prefix>>,
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
        let worker = thread::Builder::new()
            .spawn(move || {
                let result = (|| {
                    powershell(
                        r"
                        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
                        $principal = [Security.Principal.WindowsPrincipal]::new($identity)
                        if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
                            throw 'Run SSH Tunnel as administrator to enable System VPN.'
                        }
                        ",
                    )?;
                    let ip = (profile.server.as_str(), 22)
                        .to_socket_addrs()
                        .map_err(|e| e.to_string())?
                        .find_map(|address| match address.ip() {
                            std::net::IpAddr::V4(ip) => Some(ip),
                            _ => None,
                        })
                        .ok_or("System VPN requires an IPv4 SSH endpoint.")?;
                    if split.as_ref().is_some_and(|routes| routes.iter().any(|route| route.is_host() && route.contains(ip))) {
                        return Err("Удалите адрес SSH-сервера /32 из CIDR-списка: соединение с сервером должно идти напрямую.".into());
                    }
                    // Use an absolute path beside the executable for the signed Wintun binary.
                    let driver =
                        unsafe { wintun::load_from_path(dll) }.map_err(|e| e.to_string())?;
                    let (mut command, mut diagnostics) =
                        ssh_command(&profile, password_auth, password)?;
                    let _known_host = crate::tunnel::pin_host(&mut command, &profile)?;
                    let mut process = child_for_worker.lock().map_err(|e| e.to_string())?;
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let (server_command, installer) = if is_invited(&profile) {
                        (INVITED_VPN_COMMAND.to_owned(), None)
                    } else {
                        let installer = server_installer();
                        (server_command(installer.len()), Some(installer))
                    };
                    command.stdin(Stdio::piped()).stdout(Stdio::piped()).args([
                        "-4",
                        "-o",
                        &format!("HostKeyAlias={}", profile.server),
                        "-o",
                        "LogLevel=ERROR",
                        "--",
                        &ip.to_string(),
                        &server_command,
                    ]);
                    let mut ssh = command.spawn().map_err(|e| e.to_string())?;
                    let mut stdin = ssh.stdin.take().expect("piped SSH stdin");
                    let stdout = ssh.stdout.take().expect("piped SSH stdout");
                    *process = Some(ssh);
                    drop(process);

                    let (hello_tx, hello_rx) = mpsc::channel();
                    let (frames_tx, frames_rx) = mpsc::sync_channel(128);
                    thread::spawn(move || read_packets(stdout, hello_tx, frames_tx));
                    let first = wait_for_greeting(&hello_rx, Duration::from_secs(22))
                        .map_err(|error| {
                            let error = ssh_error(&mut diagnostics, error);
                            if installer.is_none() {
                                format!("Подключение по приглашению не запустилось. Попросите владельца подключиться обновлённым клиентом в режиме System VPN (IPv4).\n{error}")
                            } else {
                                error
                            }
                        })?;
                    if first == Greeting::Install {
                        let installer = installer.ok_or("Сервер запросил установку для профиля по приглашению.")?;
                        tx.send(Ok(Status::Installing)).map_err(|e| e.to_string())?;
                        stdin.write_all(installer.as_bytes())
                            .map_err(|e| ssh_error(&mut diagnostics, format!("Could not upload server installer: {e}")))?;
                        let second = wait_for_greeting(&hello_rx, Duration::from_secs(600))
                            .map_err(|error| format!("Server installation failed: {}", ssh_error(&mut diagnostics, error)))?;
                        if second != Greeting::Ready {
                            return Err("Unexpected server installer response.".into());
                        }
                    }
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    // The crate gives adapter-creation failures a generic message.
                    unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
                    let adapter_result =
                        wintun::Adapter::create(&driver, "SSH Tunnel VPN", "SSH Tunnel", Some(ADAPTER_GUID));
                    let windows_error = io::Error::last_os_error();
                    let adapter = adapter_result.map_err(|error| {
                        if matches!(&error, wintun::Error::String(message) if message == "Failed to create adapter")
                            && windows_error.raw_os_error().is_some_and(|code| code != 0)
                        {
                            format!("Не удалось создать адаптер Wintun: {windows_error}")
                        } else {
                            format!("Не удалось создать адаптер Wintun: {error}")
                        }
                    })?;
                    let index = adapter.get_adapter_index().map_err(|e| e.to_string())?;
                    let session = Arc::new(
                        adapter
                            .start_session(wintun::MIN_RING_CAPACITY * 16)
                            .map_err(|e| e.to_string())?,
                    );
                    let mut routes = Routes::new(index, ip);
                    routes.apply(split.as_deref())?;
                    tx.send(Ok(Status::Ready)).map_err(|e| e.to_string())?;
                    let result = forward(&session, stdin, frames_rx);
                    let cleanup = routes.remove();
                    result.and(cleanup)
                })();
                if let Ok(mut child) = child_for_worker.lock() {
                    if let Some(mut ssh) = child.take() {
                        let _ = ssh.kill();
                        let _ = ssh.wait();
                    }
                }
                let _ = tx.send(result.and(Err("VPN session ended.".into())));
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            child,
            cancelled,
            status,
            worker: Some(worker),
            ready: false,
            installing: false,
        })
    }

    pub fn poll(&mut self) -> Result<(), String> {
        loop {
            match self.status.try_recv() {
                Ok(event) => match event? {
                    Status::Installing => self.installing = true,
                    Status::Ready => {
                        self.installing = false;
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

    pub fn is_installing(&self) -> bool {
        self.installing
    }

    pub fn stop(&mut self) -> io::Result<()> {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut child) = self.child.lock() {
            if let Some(ssh) = child.as_mut() {
                let _ = ssh.kill();
            }
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("VPN worker failed."))?;
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
    hello: &mpsc::Receiver<io::Result<Greeting>>,
    timeout: Duration,
) -> Result<Greeting, String> {
    hello
        .recv_timeout(timeout)
        .map_err(|error| format!("VPN handshake: {error}"))?
        .map_err(|error| error.to_string())
}

fn read_greeting(reader: &mut impl Read) -> io::Result<Greeting> {
    let mut greeting = [0; HELLO.len()];
    reader.read_exact(&mut greeting)?;
    match &greeting {
        HELLO => Ok(Greeting::Ready),
        INSTALL_REQUEST => Ok(Greeting::Install),
        _ => Err(io::Error::other("Unexpected VPN server handshake.")),
    }
}

fn read_packets(
    pipe: impl Read,
    hello: mpsc::Sender<io::Result<Greeting>>,
    frames: mpsc::SyncSender<io::Result<Vec<u8>>>,
) {
    let mut reader = BufReader::new(pipe);
    let first = read_greeting(&mut reader);
    let requested_install = matches!(&first, Ok(Greeting::Install));
    let valid = first.is_ok();
    if hello.send(first).is_err() || !valid {
        return;
    }
    if requested_install {
        let second = read_greeting(&mut reader);
        let ready = matches!(&second, Ok(Greeting::Ready));
        if hello.send(second).is_err() || !ready {
            return;
        }
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

fn forward(
    session: &Arc<wintun::Session>,
    mut stdin: ChildStdin,
    frames: mpsc::Receiver<io::Result<Vec<u8>>>,
) -> Result<(), String> {
    loop {
        let mut busy = false;
        for _ in 0..32 {
            match frames.try_recv() {
                Ok(frame) => {
                    let data = frame.map_err(|e| format!("SSH VPN stream ended: {e}"))?;
                    if let Ok(mut packet) = session.allocate_send_packet(data.len() as u16) {
                        packet.bytes_mut().copy_from_slice(&data);
                        session.send_packet(packet);
                    }
                    busy = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Err("SSH VPN stream ended.".into()),
            }
        }
        for _ in 0..32 {
            let Some(packet) = session.try_receive().map_err(|e| e.to_string())? else {
                break;
            };
            let data = packet.bytes();
            if (20..=MTU).contains(&data.len()) && data[0] >> 4 == 4 {
                stdin
                    .write_all(&(data.len() as u16).to_be_bytes())
                    .and_then(|()| stdin.write_all(data))
                    .map_err(|e| e.to_string())?;
            }
            busy = true;
        }
        if !busy {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

struct Routes {
    index: u32,
    server: Ipv4Addr,
    bypass: Option<(u32, Ipv4Addr, u32)>,
}

impl Routes {
    fn new(index: u32, server: Ipv4Addr) -> Self {
        Self {
            index,
            server,
            bypass: None,
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

    fn apply(&mut self, split: Option<&[Ipv4Prefix]>) -> Result<(), String> {
        let index = self.index;
        if split.is_none_or(|routes| routes.iter().any(|route| route.contains(self.server))) {
            self.ensure_bypass()?;
        }
        let mut script = format!(
            r"
            New-NetIPAddress -InterfaceIndex {index} -IPAddress 172.31.255.2 -PrefixLength 30 -PolicyStore ActiveStore | Out-Null;
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
            New-NetRoute -InterfaceIndex {index} -DestinationPrefix '0.0.0.0/1' -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
            New-NetRoute -InterfaceIndex {index} -DestinationPrefix '128.0.0.0/1' -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
            New-NetIPAddress -InterfaceIndex {index} -IPAddress 'fd53:5353:4856::2' -PrefixLength 64 -PolicyStore ActiveStore | Out-Null;
            New-NetRoute -InterfaceIndex {index} -DestinationPrefix '2000::/3' -NextHop '::' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
            "));
        }
        powershell(&script)?;
        Ok(())
    }

    fn remove(&mut self) -> Result<(), String> {
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
    fn server_bootstrap_checks_both_installers_independently() {
        let installer = server_installer();
        assert!(!installer.contains('\r'));
        assert!(installer.contains(&format!("if {VPN_REQUIRED}; then\n(")));
        assert!(installer.contains(&format!("if {INVITATIONS_REQUIRED}; then\n(")));
        assert!(installer.contains("/var/lib/ssh-tunnel-invites/installed-v3"));
        let command = server_command(installer.len());
        assert!(command.contains(&format!(
            "if {VPN_REQUIRED} || {INVITATIONS_REQUIRED}; then"
        )));
        assert!(command.contains(&format!("head -c {}", installer.len())));
    }

    #[test]
    fn vpn_stream_accepts_optional_install_request_before_packet_frames() {
        for install in [false, true] {
            let mut stream = Vec::new();
            if install {
                stream.extend_from_slice(INSTALL_REQUEST);
            }
            stream.extend_from_slice(HELLO);
            stream.extend_from_slice(&20_u16.to_be_bytes());
            let mut packet = [0; 20];
            packet[0] = 0x45;
            stream.extend_from_slice(&packet);
            let (greetings, received) = mpsc::channel();
            let (frames, packets) = mpsc::sync_channel(2);

            read_packets(stream.as_slice(), greetings, frames);

            if install {
                assert!(matches!(
                    received.recv().unwrap().unwrap(),
                    Greeting::Install
                ));
            }
            assert!(matches!(received.recv().unwrap().unwrap(), Greeting::Ready));
            assert_eq!(packets.recv().unwrap().unwrap(), packet);
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
