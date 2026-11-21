use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, Read, Seek};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, TcpListener};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, rename_all = "PascalCase")]
pub struct Profile {
    pub server: String,
    pub user: String,
    pub socks_port: u16,
    pub key: String,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            server: String::new(),
            user: String::new(),
            socks_port: 1080,
            key: String::new(),
        }
    }
}

impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        let hostname = self.server.trim_end_matches('.');
        let valid_hostname = !hostname.is_empty()
            && hostname.len() <= 253
            && hostname.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            });
        if !valid_hostname && self.server.parse::<IpAddr>().is_err() {
            return Err("Enter a server hostname or IP address.".into());
        }
        if self.user.is_empty()
            || !self
                .user
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.@-$".contains(&byte))
        {
            return Err("Enter a valid SSH username.".into());
        }
        if !self.key.is_empty() && !Path::new(&self.key).is_file() {
            return Err("Select an existing private key file.".into());
        }
        Ok(())
    }
}

pub fn system_program(relative_path: &str) -> Result<PathBuf, String> {
    let root = env::var_os("SystemRoot").ok_or("SystemRoot is unavailable.")?;
    Ok(PathBuf::from(root).join("System32").join(relative_path))
}

pub struct Tunnel {
    process: Child,
    diagnostics: fs::File,
    endpoint: SocketAddrV4,
    netstat: PathBuf,
    next_probe: Instant,
    ready: bool,
}

impl Tunnel {
    pub fn start(profile: &Profile, password_auth: bool, secret: String) -> Result<Self, String> {
        let netstat = system_program("netstat.exe")?;
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, profile.socks_port);
        drop(
            TcpListener::bind(endpoint)
                .map_err(|error| format!("Cannot use local address {endpoint}: {error}"))?,
        );

        let (mut command, diagnostics) = ssh_command(profile, password_auth, secret)?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .args(["-N", "-o", "ExitOnForwardFailure=yes", "-D"])
            .arg(endpoint.to_string())
            .args(["--", &profile.server]);
        let process = command
            .spawn()
            .map_err(|error| format!("Could not launch OpenSSH: {error}"))?;
        Ok(Self {
            process,
            diagnostics,
            endpoint,
            netstat,
            next_probe: Instant::now(),
            ready: false,
        })
    }
}

pub fn ssh_command(
    profile: &Profile,
    password_auth: bool,
    secret: String,
) -> Result<(Command, fs::File), String> {
    let ssh = system_program(r"OpenSSH\ssh.exe")?;
    if !ssh.is_file() {
        return Err("Install the Windows OpenSSH Client optional feature.".into());
    }
    let mut command = Command::new(ssh);
    let diagnostics = tempfile::tempfile().map_err(|error| error.to_string())?;
    command
        .creation_flags(CREATE_NO_WINDOW)
        .stderr(diagnostics.try_clone().map_err(|error| error.to_string())?)
        .args([
            "-T",
            "-F",
            "none",
            "-p",
            "22",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "NumberOfPasswordPrompts=1",
        ]);
    command.args(["-l", &profile.user]);
    if password_auth {
        command.args([
            "-o",
            "PreferredAuthentications=password",
            "-o",
            "PubkeyAuthentication=no",
        ]);
    } else {
        command.args(["-o", "PreferredAuthentications=publickey"]);
    }
    command
        .env_remove("SSH_TUNNEL_ASKPASS_ENDPOINT")
        .env_remove("SSH_TUNNEL_ASKPASS_TOKEN");
    if secret.is_empty() {
        command.env("SSH_ASKPASS_REQUIRE", "never");
    } else {
        crate::askpass::configure(&mut command, secret).map_err(|error| error.to_string())?;
    }
    if !profile.key.is_empty() {
        command.args(["-i", &profile.key]);
    }
    Ok((command, diagnostics))
}

pub fn ssh_error(diagnostics: &mut fs::File, fallback: String) -> String {
    let mut bytes = Vec::new();
    if diagnostics.rewind().is_ok() {
        let _ = diagnostics.take(8192).read_to_end(&mut bytes);
    }
    let message = String::from_utf8_lossy(&bytes).trim().to_owned();
    if message.is_empty() {
        fallback
    } else {
        message
    }
}

impl Tunnel {
    pub fn endpoint(&self) -> SocketAddrV4 {
        self.endpoint
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn poll(&mut self) -> Result<(), String> {
        if let Some(status) = self.process.try_wait().map_err(|error| error.to_string())? {
            return Err(ssh_error(
                &mut self.diagnostics,
                format!("SSH session ended ({status})."),
            ));
        }
        if !self.ready && Instant::now() >= self.next_probe {
            let output = Command::new(&self.netstat)
                .creation_flags(CREATE_NO_WINDOW)
                .args(["-ano", "-p", "tcp"])
                .output()
                .map_err(|error| format!("Could not check SOCKS5 listener: {error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "Could not check SOCKS5 listener ({})",
                    output.status
                ));
            }
            let endpoint = self.endpoint.to_string();
            let pid = self.process.id().to_string();
            self.ready = String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                let mut columns = line.split_whitespace();
                columns.next() == Some("TCP")
                    && columns.next() == Some(endpoint.as_str())
                    && columns.next() == Some("0.0.0.0:0")
                    && columns.last() == Some(pid.as_str())
            });
            self.next_probe = Instant::now() + Duration::from_secs(1);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> io::Result<()> {
        if self.process.try_wait()?.is_none() {
            if let Err(error) = self.process.kill() {
                if self.process.try_wait()?.is_none() {
                    return Err(error);
                }
            }
            self.process.wait()?;
        }
        Ok(())
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
