use crate::tunnel::{Profile, Tunnel};
use crate::tunnel_job::TunnelJob;
use crate::vpn_service::{Channel, ServiceRequest};
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct AppTunnel {
    ssh: Tunnel,
    request: Option<ServiceRequest>,
    pending: Option<JoinHandle<Result<Channel, String>>>,
    service: Option<Channel>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl AppTunnel {
    pub fn start(
        profile: &Profile,
        password_auth: bool,
        secret: String,
        paths: Vec<String>,
    ) -> Result<Self, String> {
        let server = (profile.server.as_str(), 22)
            .to_socket_addrs()
            .map_err(|error| error.to_string())?
            .find_map(|address| match address.ip() {
                IpAddr::V4(ip) => Some(ip),
                IpAddr::V6(_) => None,
            })
            .ok_or("Для режима по приложениям нужен IPv4-адрес SSH-сервера.")?;
        let ssh = Tunnel::start_to(profile, password_auth, secret, Some(server))?;
        let request = ServiceRequest::Apps {
            server,
            port: profile.socks_port,
            pid: ssh.process_id(),
            paths,
        };
        Ok(Self {
            ssh,
            request: Some(request),
            pending: None,
            service: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + Duration::from_secs(45),
        })
    }

    pub fn poll(&mut self) -> Result<(), String> {
        self.ssh.poll()?;
        if self.ssh.is_ready() {
            if let Some(request) = self.request.take() {
                let cancelled = self.cancelled.clone();
                self.pending = Some(thread::spawn(move || {
                    Channel::connect(&request, &cancelled)
                }));
            }
        } else if Instant::now() >= self.deadline {
            return Err("Истекло время запуска SSH для режима по приложениям.".into());
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let worker = self.pending.take().expect("completed service worker");
            self.service = Some(
                worker
                    .join()
                    .map_err(|_| "VPN service worker failed".to_owned())??,
            );
        }
        if let Some(service) = &mut self.service {
            service.poll_apps()?;
        }
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.service.is_some()
    }

    pub fn stop(&mut self) -> io::Result<()> {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.pending.take() {
            self.service = worker.join().ok().and_then(Result::ok);
        }
        let service_result = self
            .service
            .take()
            .map_or(Ok(()), |service| service.stop().map_err(io::Error::other));
        service_result.and(self.ssh.stop())
    }
}

impl Drop for AppTunnel {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(crate) struct Engine {
    child: Child,
    _job: TunnelJob,
    logs: mpsc::Receiver<String>,
}

impl Engine {
    pub(crate) fn start(server: Ipv4Addr, port: u16, paths: Vec<String>) -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name("sing-box.exe");
        let directory = executable
            .parent()
            .ok_or("Нет каталога установки sing-box.")?;
        if !executable.is_file() {
            return Err("Для режима по приложениям установите сборку с sing-box.".into());
        }
        let payload = serde_json::to_vec(&configuration(server, port, paths))
            .map_err(|error| error.to_string())?;
        let mut command = Command::new(&executable);
        command
            .current_dir(directory)
            .creation_flags(0x0800_0000)
            .args(["run", "--disable-color", "-c", "stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let job = TunnelJob::new().map_err(|error| error.to_string())?;
        let mut child = job.spawn(&mut command).map_err(|error| error.to_string())?;
        let (tx, logs) = mpsc::sync_channel(128);
        forward_logs(child.stdout.take().expect("piped stdout"), tx.clone());
        forward_logs(child.stderr.take().expect("piped stderr"), tx);
        let mut stdin = child.stdin.take().expect("piped config stdin");
        stdin
            .write_all(&payload)
            .map_err(|error| error.to_string())?;
        drop(stdin);
        Ok(Self {
            child,
            _job: job,
            logs,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<bool, String> {
        let ready = self.logs.try_iter().take(128).fold(false, |ready, line| {
            ready || line.contains("sing-box started")
        });
        if let Some(status) = self.child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!(
                "Маршрутизация по приложениям завершилась ({status})."
            ));
        }
        Ok(ready)
    }

    pub(crate) fn stop(&mut self) -> Result<(), String> {
        if self
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            self.child.kill().map_err(|error| error.to_string())?;
            self.child.wait().map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn forward_logs(pipe: impl io::Read + Send + 'static, sender: mpsc::SyncSender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if matches!(
                sender.try_send(line),
                Err(mpsc::TrySendError::Disconnected(_))
            ) {
                break;
            }
        }
    });
}

fn exact_process_path(path: &str) -> String {
    let mut pattern = String::from("(?i)^");
    for char in path.chars() {
        if ".+*?()|[]{}^$\\".contains(char) {
            pattern.push('\\');
        }
        pattern.push(char);
    }
    pattern.push('$');
    pattern
}

pub fn configuration(server: Ipv4Addr, port: u16, paths: Vec<String>) -> Value {
    let patterns: Vec<_> = paths.iter().map(|path| exact_process_path(path)).collect();
    json!({
        "log": { "level": "info", "timestamp": true },
        "inbounds": [{
            "type": "tun", "tag": "app-tun", "interface_name": "SSH Tunnel Apps",
            "address": ["172.30.255.1/30", "fd53:5353:4150::1/126"],
            "auto_route": true, "strict_route": false,
            "route_exclude_address": [format!("{server}/32")],
            "stack": "system"
        }],
        "outbounds": [
            { "type": "direct", "tag": "direct" },
            { "type": "socks", "tag": "ssh", "server": "127.0.0.1", "server_port": port, "version": "5", "network": "tcp" }
        ],
        "route": {
            "auto_detect_interface": true, "final": "direct",
            "rules": [
                { "process_path_regex": patterns, "network": "tcp", "action": "route", "outbound": "ssh" },
                { "process_path_regex": patterns, "action": "reject" }
            ]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_routes_only_selected_executables_to_ssh() {
        let config = configuration(
            Ipv4Addr::new(192, 0, 2, 10),
            1080,
            vec![r"C:\Apps (Test)\Example.exe".into()],
        );
        assert_eq!(
            config["route"]["rules"][0]["process_path_regex"][0],
            r"(?i)^C:\\Apps \(Test\)\\Example\.exe$"
        );
        assert_eq!(config["route"]["rules"][0]["network"], "tcp");
        assert_eq!(config["route"]["rules"][1]["action"], "reject");
        assert_eq!(config["route"]["final"], "direct");
        assert_eq!(
            config["inbounds"][0]["route_exclude_address"][0],
            "192.0.2.10/32"
        );
        if let Some(engine) = std::env::var_os("SSH_TUNNEL_TEST_SING_BOX") {
            let mut child = Command::new(engine)
                .args(["check", "-c", "stdin"])
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            serde_json::to_writer(child.stdin.take().unwrap(), &config).unwrap();
            let result = child.wait_with_output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
