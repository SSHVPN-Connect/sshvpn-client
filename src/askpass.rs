use std::env;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const ENDPOINT: &str = "SSH_TUNNEL_ASKPASS_ENDPOINT";
const TOKEN: &str = "SSH_TUNNEL_ASKPASS_TOKEN";

pub fn configure(command: &mut Command, secret: String) -> io::Result<()> {
    if secret.contains(['\r', '\n', '\0']) {
        return Err(io::Error::other(
            "Invalid characters in SSH password or passphrase.",
        ));
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    command
        .env("SSH_ASKPASS", env::current_exe()?)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env(ENDPOINT, listener.local_addr()?.to_string())
        .env(TOKEN, &token);
    thread::Builder::new().spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut peer, _)) => {
                    let mut received = [0_u8; 64];
                    let timeout = Some(Duration::from_millis(500));
                    if peer.set_read_timeout(timeout).is_ok()
                        && peer.set_write_timeout(timeout).is_ok()
                        && peer.read_exact(&mut received).is_ok()
                        && received == token.as_bytes()
                    {
                        let _ = peer.write_all(secret.as_bytes());
                        return;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(30));
                }
                Err(_) => return,
            }
        }
    })?;
    Ok(())
}

pub fn run_helper() -> Option<io::Result<()>> {
    let endpoint = env::var_os(ENDPOINT)?;
    Some((|| {
        let address: SocketAddrV4 = endpoint
            .to_string_lossy()
            .parse()
            .map_err(io::Error::other)?;
        if !address.ip().is_loopback() {
            return Err(io::Error::other("Invalid askpass endpoint."));
        }
        let token = env::var(TOKEN).map_err(io::Error::other)?;
        let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(token.as_bytes())?;
        let mut secret = Vec::new();
        stream.take(4096).read_to_end(&mut secret)?;
        let mut stdout = io::stdout().lock();
        stdout.write_all(&secret)?;
        stdout.write_all(b"\n")?;
        stdout.flush()
    })())
}
