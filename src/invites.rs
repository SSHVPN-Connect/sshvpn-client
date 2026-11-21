use crate::profiles::{Authentication, Mode, SavedProfile};
use crate::tunnel::{pin_host, ssh_command_policy, ssh_error, system_program, Profile};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
};

const VERSION: &str = "ssh-tunnel-invite-1";
const HANDLER: &str = "/usr/local/libexec/ssh-tunnel-invite";

#[derive(Serialize, Deserialize)]
struct Ticket {
    server: String,
    user: String,
    host_key: String,
    temporary_key: String,
}

#[derive(Deserialize)]
struct Issued {
    user: String,
    host_key: String,
    expires: String,
}

pub struct Invitation {
    pub encoded: String,
    pub code: String,
    pub user: String,
    pub expires: String,
}

pub struct Imported {
    pub profile: SavedProfile,
    pub key_directory: tempfile::TempDir,
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn seal(ticket: &Ticket) -> Result<(String, String), String> {
    let key = random::<32>()?;
    let nonce = random::<12>()?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "Invalid encryption key.")?;
    let data = serde_json::to_vec(ticket).map_err(|error| error.to_string())?;
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &data,
                aad: VERSION.as_bytes(),
            },
        )
        .map_err(|_| "Не удалось зашифровать приглашение.")?;
    let mut envelope = nonce.to_vec();
    envelope.extend(encrypted);
    Ok((
        format!("{VERSION}:{}", URL_SAFE_NO_PAD.encode(envelope)),
        URL_SAFE_NO_PAD.encode(key),
    ))
}

fn open_ticket(encoded: &str, code: &str) -> Result<Ticket, String> {
    let (version, payload) = encoded
        .trim()
        .split_once(':')
        .ok_or("Неверная строка приглашения.")?;
    if version != VERSION || payload.len() > 4096 || code.trim().len() != 43 {
        return Err("Неверная строка приглашения или код.".into());
    }
    let data = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "Неверная строка приглашения.")?;
    let (nonce, encrypted) = data
        .split_at_checked(12)
        .ok_or("Строка приглашения повреждена.")?;
    let key = URL_SAFE_NO_PAD
        .decode(code.trim())
        .map_err(|_| "Неверный код приглашения.")?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "Неверный код приглашения.")?;
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: encrypted,
                aad: VERSION.as_bytes(),
            },
        )
        .map_err(|_| "Код неверен или приглашение изменилось.")?;
    serde_json::from_slice(&plaintext).map_err(|_| "Неверные данные приглашения.".into())
}

pub fn valid_public_key(key: &str) -> bool {
    let Some(body) = key.strip_prefix("ssh-ed25519 ") else {
        return false;
    };
    STANDARD
        .decode(body)
        .is_ok_and(|data| data.len() == 51 && data.starts_with(b"\0\0\0\x0bssh-ed25519\0\0\0\x20"))
}

fn valid_user(user: &str) -> bool {
    user.strip_prefix("stv_").is_some_and(|id| {
        id.len() == 16
            && id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn generate(path: &Path) -> Result<String, String> {
    let exe = system_program(r"OpenSSH\ssh-keygen.exe")?;
    let result = Command::new(exe)
        .creation_flags(0x0800_0000)
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "Не удалось создать SSH-ключ: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let pubkey = fs::read_to_string(path.with_extension("pub")).map_err(|e| e.to_string())?;
    let key = pubkey
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    if !valid_public_key(&key) {
        return Err("OpenSSH вернул неверный публичный ключ.".into());
    }
    Ok(key)
}

fn request<T: DeserializeOwned>(
    profile: &Profile,
    password_auth: bool,
    secret: String,
    operation: &str,
    data: Value,
) -> Result<T, String> {
    profile.validate()?;
    let (mut command, mut diagnostics) = ssh_command_policy(profile, password_auth, secret, true)?;
    let _pin = pin_host(&mut command, profile)?;
    let remote = format!("{HANDLER} {operation}");
    command
        .args(["--", &profile.server, &remote])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let write = writeln!(child.stdin.take().expect("piped SSH input"), "{data}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ssh_error(
                &mut diagnostics,
                "Истекло время ожидания ответа SSH.".into(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(ssh_error(
            &mut diagnostics,
            format!("SSH invitation failed: {}", output.status),
        ));
    }
    write.map_err(|e| e.to_string())?;
    serde_json::from_slice(&output.stdout)
        .map_err(|_| "Сервер вернул неверный ответ приглашения.".into())
}

pub fn issue(profile: SavedProfile, secret: String) -> Result<Invitation, String> {
    if profile.connection.user != "root" {
        return Err("Для выдачи приглашений нужен профиль SSH с пользователем root.".into());
    }
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = directory.path().join("bootstrap");
    let staging_key = generate(&path)?;
    let temporary_key = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let issued: Issued = request(
        &profile.connection,
        profile.authentication == Authentication::Password,
        secret,
        "issue",
        json!({ "public_key": staging_key }),
    )?;
    if !valid_user(&issued.user) || !valid_public_key(&issued.host_key) {
        return Err("Сервер вернул неверную учётную запись или ключ.".into());
    }
    let ticket = Ticket {
        server: profile.connection.server,
        user: issued.user,
        host_key: issued.host_key,
        temporary_key,
    };
    let (encoded, code) = seal(&ticket)?;
    Ok(Invitation {
        encoded,
        code,
        user: ticket.user,
        expires: issued.expires,
    })
}

pub fn redeem(encoded: &str, code: &str, base: &Path) -> Result<Imported, String> {
    let ticket = open_ticket(encoded, code)?;
    if !valid_user(&ticket.user)
        || !valid_public_key(&ticket.host_key)
        || !ticket
            .temporary_key
            .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
    {
        return Err("Приглашение содержит недопустимые данные.".into());
    }
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let bootstrap = temporary.path().join("bootstrap");
    fs::write(&bootstrap, ticket.temporary_key).map_err(|e| e.to_string())?;
    let mut connection = Profile {
        server: ticket.server,
        user: ticket.user,
        key: bootstrap.to_string_lossy().into_owned(),
        host_key: Some(ticket.host_key),
        ..Profile::default()
    };
    connection.validate()?;
    let keys = base.join("keys");
    fs::create_dir_all(&keys).map_err(|e| e.to_string())?;
    let key_directory = tempfile::Builder::new()
        .prefix("invited-")
        .tempdir_in(&keys)
        .map_err(|e| e.to_string())?;
    let permanent = key_directory.path().join("id_ed25519");
    let public = generate(&permanent)?;
    // The temporary key forces the redeem command even if the client's command is ignored.
    let reply: Value = request(
        &connection,
        false,
        String::new(),
        "redeem",
        json!({ "public_key": public }),
    )?;
    if reply["user"].as_str() != Some(connection.user.as_str()) {
        return Err("Сервер вернул другую учётную запись.".into());
    }
    connection.key = permanent.to_string_lossy().into_owned();
    let profile = SavedProfile {
        name: format!("{} · приглашение", connection.server),
        connection,
        mode: Mode::Socks5,
        authentication: Authentication::Key,
        ..SavedProfile::default()
    };
    Ok(Imported {
        profile,
        key_directory,
    })
}

pub fn revoke(profile: SavedProfile, secret: String, user: String) -> Result<(), String> {
    if profile.connection.user != "root" || !valid_user(&user) {
        return Err("Выберите профиль root и укажите SSH-пользователя из приглашения.".into());
    }
    let response: Value = request(
        &profile.connection,
        profile.authentication == Authentication::Password,
        secret,
        "revoke",
        json!({ "user": user }),
    )?;
    if response["revoked"] != true {
        return Err("Сервер не подтвердил отзыв доступа.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invitation_requires_separate_authenticated_code() {
        let ticket = Ticket {
            server: "host.example".into(),
            user: "stv_0123456789abcdef".into(),
            host_key: "public".into(),
            temporary_key: "temporary".into(),
        };
        let (encrypted, code) = seal(&ticket).unwrap();
        assert!(!encrypted.contains("host.example") && !encrypted.contains(&code));
        assert_eq!(
            open_ticket(&encrypted, &code).unwrap().temporary_key,
            "temporary"
        );
        assert!(open_ticket(&encrypted, &URL_SAFE_NO_PAD.encode([0; 32])).is_err());
    }
}
