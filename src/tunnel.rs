use crate::tunnel_job::TunnelJob;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
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
    pub host_key: Option<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            server: String::new(),
            user: String::new(),
            socks_port: 1080,
            key: String::new(),
            host_key: None,
        }
    }
}

impl Profile {
    pub(crate) fn is_invited(&self) -> bool {
        self.host_key.is_some() && crate::invites::valid_user(&self.user)
    }

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
    _job: TunnelJob,
    diagnostics: fs::File,
    endpoint: SocketAddrV4,
    netstat: PathBuf,
    next_probe: Instant,
    ready: bool,
    _known_host: Option<tempfile::TempDir>,
}

impl Tunnel {
    pub fn start(profile: &Profile, password_auth: bool, secret: String) -> Result<Self, String> {
        Self::start_to(profile, password_auth, secret, None)
    }

    pub fn start_to(
        profile: &Profile,
        password_auth: bool,
        secret: String,
        address: Option<Ipv4Addr>,
    ) -> Result<Self, String> {
        let netstat = system_program("netstat.exe")?;
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, profile.socks_port);
        drop(
            TcpListener::bind(endpoint)
                .map_err(|error| format!("Cannot use local address {endpoint}: {error}"))?,
        );

        let (mut command, diagnostics) = ssh_command(profile, password_auth, secret)?;
        let known_host = pin_host(&mut command, profile)?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .args(["-N", "-o", "ExitOnForwardFailure=yes", "-D"])
            .arg(endpoint.to_string());
        if let Some(address) = address {
            command.args([
                "-4",
                "-o",
                &format!("HostKeyAlias={}", profile.server),
                "--",
                &address.to_string(),
            ]);
        } else {
            command.args(["--", &profile.server]);
        }
        let job = TunnelJob::new().map_err(|error| error.to_string())?;
        let process = job
            .spawn(&mut command)
            .map_err(|error| format!("Could not launch OpenSSH: {error}"))?;
        Ok(Self {
            process,
            _job: job,
            diagnostics,
            endpoint,
            netstat,
            next_probe: Instant::now(),
            ready: false,
            _known_host: known_host,
        })
    }

    pub(crate) fn process_id(&self) -> u32 {
        self.process.id()
    }
}

pub fn pin_host(
    command: &mut Command,
    profile: &Profile,
) -> Result<Option<tempfile::TempDir>, String> {
    let Some(ref key) = profile.host_key else {
        return Ok(None);
    };
    if !crate::invites::valid_public_key(key) {
        return Err("Неверный публичный ключ SSH-сервера.".into());
    }
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = directory.path().join("known_hosts");
    let host = profile.server.to_ascii_lowercase();
    // Close the file before Windows OpenSSH opens it with its own sharing mode.
    fs::write(&path, format!("{host} {key}\n")).map_err(|e| e.to_string())?;
    command.args([
        "-o",
        &format!("UserKnownHostsFile=\"{}\"", path.display()),
        "-o",
        "GlobalKnownHostsFile=none",
        "-o",
        "HostKeyAlgorithms=ssh-ed25519",
        "-o",
        &format!("HostKeyAlias={host}"),
    ]);
    Ok(Some(directory))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    #[test]
    fn openssh_can_read_the_pinned_host_file_while_the_profile_is_active() {
        let mut key = b"\0\0\0\x0bssh-ed25519\0\0\0\x20".to_vec();
        key.extend([42; 32]);
        let profile = Profile {
            server: "Test.Example".into(),
            user: "test".into(),
            host_key: Some(format!("ssh-ed25519 {}", STANDARD.encode(key))),
            ..Profile::default()
        };
        let (mut ssh, _) = ssh_command(&profile, false, String::new()).unwrap();
        let pin = pin_host(&mut ssh, &profile).unwrap().unwrap();
        let config = ssh.args(["-G", "--", "127.0.0.1"]).output().unwrap();
        assert!(config.status.success());
        let config = String::from_utf8(config.stdout).unwrap();
        assert!(config
            .lines()
            .any(|line| line == "stricthostkeychecking true"));
        let host = config
            .lines()
            .find_map(|line| line.strip_prefix("hostkeyalias "))
            .unwrap();
        assert_eq!(host, "test.example");
        let path = config
            .lines()
            .find_map(|line| line.strip_prefix("userknownhostsfile "))
            .unwrap();
        assert_eq!(Path::new(path), pin.path().join("known_hosts"));
        let lookup = Command::new(system_program(r"OpenSSH\ssh-keygen.exe").unwrap())
            .args(["-F", host, "-f", path])
            .output()
            .unwrap();
        assert!(lookup.status.success(), "{:?}", lookup);
        assert!(
            String::from_utf8_lossy(&lookup.stdout).contains(profile.host_key.as_deref().unwrap())
        );
    }
}

pub fn ssh_command(
    profile: &Profile,
    password_auth: bool,
    secret: String,
) -> Result<(Command, fs::File), String> {
    ssh_command_policy(profile, password_auth, secret, false)
}

pub fn ssh_command_policy(
    profile: &Profile,
    password_auth: bool,
    secret: String,
    strict: bool,
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
            if strict || profile.host_key.is_some() {
                "StrictHostKeyChecking=yes"
            } else {
                "StrictHostKeyChecking=accept-new"
            },
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
        if profile.host_key.is_some() {
            command.args(["-o", "IdentitiesOnly=yes"]);
        }
    }
    Ok((command, diagnostics))
}

pub fn ssh_error(diagnostics: &mut fs::File, fallback: String) -> String {
    let mut bytes = Vec::new();
    let start = diagnostics
        .metadata()
        .map(|metadata| metadata.len().saturating_sub(8192));
    if start
        .and_then(|start| diagnostics.seek(SeekFrom::Start(start)))
        .is_ok()
    {
        let _ = diagnostics.take(8192).read_to_end(&mut bytes);
    }
    let message = String::from_utf8_lossy(&bytes).trim().to_owned();
    if message.is_empty() {
        fallback
    } else {
        message
    }
}

pub(crate) fn owns_socks_listener(
    netstat: &Path,
    endpoint: SocketAddrV4,
    pid: u32,
) -> Result<bool, String> {
    let output = Command::new(netstat)
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
    let endpoint = endpoint.to_string();
    let pid = pid.to_string();
    Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        let mut columns = line.split_whitespace();
        columns.next() == Some("TCP")
            && columns.next() == Some(endpoint.as_str())
            && columns.next() == Some("0.0.0.0:0")
            && columns.last() == Some(pid.as_str())
    }))
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
            self.ready = owns_socks_listener(&self.netstat, self.endpoint, self.process.id())?;
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
