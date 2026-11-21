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
const HELLO: &[u8; 8] = b"SSHVPN1\n";

pub struct Vpn {
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
    status: mpsc::Receiver<Result<(), String>>,
    worker: Option<JoinHandle<()>>,
    ready: bool,
}

impl Vpn {
    pub fn start(profile: Profile, password_auth: bool, password: String) -> Result<Self, String> {
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
                    // Use an absolute path beside the executable for the signed Wintun binary.
                    let driver =
                        unsafe { wintun::load_from_path(dll) }.map_err(|e| e.to_string())?;
                    let (mut command, mut diagnostics) =
                        ssh_command(&profile, password_auth, password)?;
                    let mut process = child_for_worker.lock().map_err(|e| e.to_string())?;
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    command.stdin(Stdio::piped()).stdout(Stdio::piped()).args([
                        "-4",
                        "-o",
                        &format!("HostKeyAlias={}", profile.server),
                        "--",
                        &ip.to_string(),
                        "/usr/local/libexec/ssh-tunnel-vpn",
                    ]);
                    let mut ssh = command.spawn().map_err(|e| e.to_string())?;
                    let stdin = ssh.stdin.take().expect("piped SSH stdin");
                    let stdout = ssh.stdout.take().expect("piped SSH stdout");
                    *process = Some(ssh);
                    drop(process);

                    let (hello_tx, hello_rx) = mpsc::channel();
                    let (frames_tx, frames_rx) = mpsc::sync_channel(128);
                    thread::spawn(move || read_packets(stdout, hello_tx, frames_tx));
                    match hello_rx.recv_timeout(Duration::from_secs(22)) {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            return Err(ssh_error(&mut diagnostics, error.to_string()))
                        }
                        Err(error) => {
                            return Err(ssh_error(
                                &mut diagnostics,
                                format!("VPN handshake: {error}"),
                            ))
                        }
                    }
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let adapter =
                        wintun::Adapter::create(&driver, "SSH Tunnel VPN", "SSH Tunnel", None)
                            .map_err(|e| {
                                format!(
                                    "Could not create Wintun adapter. Run as administrator. {e}"
                                )
                            })?;
                    let index = adapter.get_adapter_index().map_err(|e| e.to_string())?;
                    let session = Arc::new(
                        adapter
                            .start_session(wintun::MIN_RING_CAPACITY * 16)
                            .map_err(|e| e.to_string())?,
                    );
                    let mut routes = Routes::new(index, ip);
                    routes.apply()?;
                    tx.send(Ok(())).map_err(|e| e.to_string())?;
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
        })
    }

    pub fn poll(&mut self) -> Result<(), String> {
        loop {
            match self.status.try_recv() {
                Ok(event) => {
                    event?;
                    self.ready = true;
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Err("VPN worker stopped.".into()),
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
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

fn read_packets(
    pipe: impl Read,
    hello: mpsc::Sender<io::Result<()>>,
    frames: mpsc::SyncSender<io::Result<Vec<u8>>>,
) {
    let mut reader = BufReader::new(pipe);
    let mut greeting = [0; HELLO.len()];
    let result = reader.read_exact(&mut greeting).and_then(|()| {
        if &greeting == HELLO {
            Ok(())
        } else {
            Err(io::Error::other("Unexpected VPN server handshake."))
        }
    });
    let ready = result.is_ok();
    if hello.send(result).is_err() || !ready {
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

    fn apply(&mut self) -> Result<(), String> {
        let index = self.index;
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
        powershell(&format!(
            r"
            New-NetIPAddress -InterfaceIndex {index} -IPAddress 172.31.255.2 -PrefixLength 30 -PolicyStore ActiveStore | Out-Null;
            Set-NetIPInterface -InterfaceIndex {index} -AddressFamily IPv4 -NlMtuBytes {MTU} -InterfaceMetric 4 -PolicyStore ActiveStore;
            Set-DnsClientServerAddress -InterfaceIndex {index} -ServerAddresses @('1.1.1.1','1.0.0.1');
            New-NetRoute -InterfaceIndex {index} -DestinationPrefix '0.0.0.0/1' -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
            New-NetRoute -InterfaceIndex {index} -DestinationPrefix '128.0.0.0/1' -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
            New-NetIPAddress -InterfaceIndex {index} -IPAddress 'fd53:5353:4856::2' -PrefixLength 64 -PolicyStore ActiveStore | Out-Null;
            New-NetRoute -InterfaceIndex {index} -DestinationPrefix '2000::/3' -NextHop '::' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null;
        "
        ))?;
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

fn powershell(script: &str) -> Result<String, String> {
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
    use super::powershell;

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
