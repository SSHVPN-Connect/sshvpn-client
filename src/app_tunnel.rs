use crate::tunnel::{Profile, Tunnel};
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct AppTunnel {
    ssh: Tunnel,
    engine: Option<Child>,
    config: tempfile::NamedTempFile,
    executable: PathBuf,
    logs: Option<mpsc::Receiver<String>>,
    last_message: String,
    ready: bool,
    deadline: Instant,
}

impl AppTunnel {
    pub fn start(
        profile: &Profile,
        password_auth: bool,
        secret: String,
        paths: Vec<String>,
    ) -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name("sing-box.exe");
        if !executable.is_file() {
            return Err("Для режима по приложениям нужен sing-box.exe рядом с SSH Tunnel. Используйте сборку с sing-box.".into());
        }
        let server = (profile.server.as_str(), 22)
            .to_socket_addrs()
            .map_err(|error| error.to_string())?
            .find_map(|address| match address.ip() {
                IpAddr::V4(ip) => Some(ip),
                IpAddr::V6(_) => None,
            })
            .ok_or("Для режима по приложениям нужен IPv4-адрес SSH-сервера.")?;
        let mut config = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
        serde_json::to_writer(
            config.as_file_mut(),
            &configuration(server, profile.socks_port, paths),
        )
        .map_err(|error| error.to_string())?;
        config
            .as_file_mut()
            .flush()
            .map_err(|error| error.to_string())?;
        let ssh = Tunnel::start_to(profile, password_auth, secret, Some(server))?;
        Ok(Self {
            ssh,
            engine: None,
            config,
            executable,
            logs: None,
            last_message: String::new(),
            ready: false,
            deadline: Instant::now() + Duration::from_secs(45),
        })
    }

    pub fn poll(&mut self) -> Result<(), String> {
        self.ssh.poll()?;
        if self.ssh.is_ready() && self.engine.is_none() {
            let mut command = Command::new(&self.executable);
            command
                .creation_flags(0x0800_0000)
                .args(["run", "--disable-color", "-c"])
                .arg(self.config.path())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut engine = self
                .ssh
                .spawn_companion(&mut command)
                .map_err(|error| format!("Не удалось запустить sing-box: {error}"))?;
            let (tx, rx) = mpsc::sync_channel(128);
            forward_logs(engine.stdout.take().expect("piped stdout"), tx.clone());
            forward_logs(engine.stderr.take().expect("piped stderr"), tx);
            self.logs = Some(rx);
            self.engine = Some(engine);
            self.deadline = Instant::now() + Duration::from_secs(30);
        }
        if let Some(logs) = &self.logs {
            for line in logs.try_iter().take(128) {
                self.ready |= line.contains("sing-box started");
                self.last_message = line;
            }
        }
        if let Some(engine) = &mut self.engine {
            if let Some(status) = engine.try_wait().map_err(|error| error.to_string())? {
                return Err(format!(
                    "Режим по приложениям завершился ({status}). {}",
                    self.last_message
                ));
            }
        }
        if !self.ready && Instant::now() >= self.deadline {
            return Err(format!(
                "Не удалось запустить режим по приложениям. {}",
                self.last_message
            ));
        }
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn stop(&mut self) -> io::Result<()> {
        let engine_result = if let Some(engine) = &mut self.engine {
            stop_child(engine)
        } else {
            Ok(())
        };
        engine_result.and(self.ssh.stop())
    }
}

impl Drop for AppTunnel {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn stop_child(child: &mut Child) -> io::Result<()> {
    if child.try_wait()?.is_none() {
        if let Err(error) = child.kill() {
            if child.try_wait()?.is_none() {
                return Err(error);
            }
        }
        child.wait()?;
    }
    Ok(())
}

fn forward_logs(pipe: impl io::Read + Send + 'static, sender: mpsc::SyncSender<String>) {
    std::thread::spawn(move || {
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
            let mut file = tempfile::NamedTempFile::new().unwrap();
            serde_json::to_writer(file.as_file_mut(), &config).unwrap();
            file.as_file_mut().flush().unwrap();
            let result = Command::new(engine)
                .args(["check", "-c"])
                .arg(file.path())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
