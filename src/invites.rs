use crate::profiles::{Authentication, Mode, SavedProfile};
use crate::tunnel::{pin_host, ssh_command_policy, ssh_error, system_program, Profile};
use crate::tunnel_job::TunnelJob;
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
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
};

const VERSION: &str = "ssh-tunnel-invite-1";
const HANDLER: &str = "/usr/local/libexec/ssh-tunnel-invite";
const LIST_COMMAND: &str = "/usr/local/libexec/ssh-tunnel-invite list-v5";
const INVITATION_INSTALLER: &str = include_str!("../install-invitations.sh");
const VPN_INSTALLER: &str = include_str!("../install-server.sh");
const VPN_MISSING: &str = "[ ! -x /usr/local/libexec/ssh-tunnel-vpn ]";
const INVITATIONS_MISSING: &str = "[ ! -x /usr/local/libexec/ssh-tunnel-invite ]";
const VPN_REQUIRED: &str = r#"[ ! -x /usr/local/libexec/ssh-tunnel-vpn ] || [ ! -f /var/lib/ssh-tunnel-vpn/installed-v3 ] || [ "$(cat /proc/sys/net/ipv4/ip_forward)" != 1 ]"#;
const INVITATIONS_REQUIRED: &str = r#"[ ! -x /usr/local/libexec/ssh-tunnel-invite ] || [ ! -x /usr/local/libexec/ssh-tunnel-invite-session ] || [ ! -d /var/lib/ssh-tunnel-invites/records ] || [ ! -f /var/lib/ssh-tunnel-invites/installed-v8 ] || [ ! -r /etc/ssh/ssh-tunnel-invites.conf ] || [ ! -r /etc/sudoers.d/ssh-tunnel-invites ] || ! grep -Fxq 'Include /etc/ssh/ssh-tunnel-invites.conf' /etc/ssh/sshd_config"#;
pub(crate) const UPDATE_REQUIRED: &str =
    "Серверу требуется настройка или обновление.";

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
}

#[derive(Deserialize, Serialize, Clone)]
pub struct AccessEntry {
    pub user: String,
    pub name: String,
    pub state: AccessState,
    pub last_connected: Option<String>,
    #[serde(default)]
    pub can_restore: bool,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AccessState {
    Pending,
    Active,
    Expired,
    Revoked,
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

pub(crate) fn valid_user(user: &str) -> bool {
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

fn write_bootstrap_key(path: &Path, key: &[u8]) -> std::io::Result<()> {
    use std::{
        io,
        mem::{size_of, size_of_val},
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        ptr::null_mut,
    };
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Security::{
            AddAccessAllowedAce, GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor,
            SetSecurityDescriptorControl, SetSecurityDescriptorDacl, SetSecurityDescriptorOwner,
            TokenUser, ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, SECURITY_ATTRIBUTES,
            SECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
        },
        Storage::FileSystem::{
            CreateFileW, GetVolumeInformationByHandleW, CREATE_NEW, FILE_ALL_ACCESS,
            FILE_ATTRIBUTE_NORMAL,
        },
        System::{
            SystemServices::{FILE_PERSISTENT_ACLS, SECURITY_DESCRIPTOR_REVISION},
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    #[repr(C)]
    struct TokenUserBuffer {
        user: TOKEN_USER,
        sid: [u8; SECURITY_MAX_SID_SIZE as usize],
    }

    let path = path.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
    // Create the key with a protected ACL before any secret bytes reach the filesystem.
    let mut file = unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut account = TokenUserBuffer {
            user: TOKEN_USER::default(),
            sid: [0; SECURITY_MAX_SID_SIZE as usize],
        };
        let mut needed = 0;
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            (&mut account as *mut TokenUserBuffer).cast(),
            size_of_val(&account) as u32,
            &mut needed,
        ) == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid = account.user.User.Sid;
        // SidStart is the first DWORD of the variable-length SID.
        const ACL_BYTES: usize = size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>()
            - size_of::<u32>() + SECURITY_MAX_SID_SIZE as usize;
        let mut acl = [0u32; ACL_BYTES / size_of::<u32>()];
        let acl = acl.as_mut_ptr().cast::<ACL>();
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let descriptor = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
        if InitializeAcl(acl, ACL_BYTES as u32, ACL_REVISION) == 0
            || AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid) == 0
            || InitializeSecurityDescriptor(descriptor, SECURITY_DESCRIPTOR_REVISION) == 0
            || SetSecurityDescriptorOwner(descriptor, sid, 0) == 0
            || SetSecurityDescriptorDacl(descriptor, 1, acl, 0) == 0
            || SetSecurityDescriptorControl(descriptor, SE_DACL_PROTECTED, SE_DACL_PROTECTED) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let file = fs::File::from_raw_handle(handle);
        let mut volume_flags = 0;
        if GetVolumeInformationByHandleW(
            handle,
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut volume_flags,
            null_mut(),
            0,
        ) == 0 {
            return Err(io::Error::last_os_error());
        }
        if volume_flags & FILE_PERSISTENT_ACLS == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Файловая система временного каталога не поддерживает права доступа Windows.",
            ));
        }
        file
    };
    file.write_all(key)
}

fn request<T: DeserializeOwned>(
    profile: &Profile,
    password_auth: bool,
    secret: String,
    operation: &str,
    data: Value,
) -> Result<T, String> {
    exchange(
        profile,
        password_auth,
        secret,
        &format!("{HANDLER} {operation}"),
        &format!("{data}\n"),
        35,
        true,
    )
}

fn exchange<T: DeserializeOwned>(
    profile: &Profile,
    password_auth: bool,
    secret: String,
    remote: &str,
    input: &str,
    timeout: u64,
    strict_host: bool,
) -> Result<T, String> {
    profile.validate()?;
    let (mut command, mut diagnostics) =
        ssh_command_policy(profile, password_auth, secret, strict_host)?;
    let _pin = pin_host(&mut command, profile)?;
    command
        .args(["--", &profile.server, remote])
        .stdin(if input.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped());
    let job = TunnelJob::new().map_err(|error| error.to_string())?;
    let mut child = job.spawn(&mut command).map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take().expect("piped SSH output");
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        stdout.read_to_end(&mut data)?;
        Ok::<_, std::io::Error>(data)
    });
    let writer = child.stdin.take().map(|mut stdin| {
        let input = input.as_bytes().to_vec();
        std::thread::spawn(move || stdin.write_all(&input))
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ssh_error(
                &mut diagnostics,
                "Истекло время ожидания ответа SSH.".into(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    if !status.success() {
        return Err(ssh_error(
            &mut diagnostics,
            format!("SSH invitation failed: {status}"),
        ));
    }
    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| "Не удалось передать данные по SSH.")?
            .map_err(|error| error.to_string())?;
    }
    let data = reader
        .join()
        .map_err(|_| "Не удалось прочитать ответ SSH.")?
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&data).map_err(|_| "Сервер вернул неверный ответ приглашения.".into())
}

pub fn issue(profile: SavedProfile, secret: String, name: String) -> Result<String, String> {
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
        json!({ "public_key": staging_key, "name": name }),
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
    Ok(format!("{encoded}:{code}"))
}

pub fn needs_legacy_code(encoded: &str) -> bool {
    matches!(encoded.trim().split_once(':'), Some((VERSION, payload))
        if !payload.is_empty() && !payload.contains(':'))
}

fn open_invitation(encoded: &str, legacy_code: &str) -> Result<Ticket, String> {
    let encoded = encoded.trim();
    let (ticket, code) = if needs_legacy_code(encoded) {
        (encoded, legacy_code)
    } else {
        encoded
            .rsplit_once(':')
            .ok_or("Неверная строка приглашения.")?
    };
    open_ticket(ticket, code)
}

pub fn redeem(encoded: &str, code: &str, base: &Path) -> Result<Imported, String> {
    let ticket = open_invitation(encoded, code)?;
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
    write_bootstrap_key(&bootstrap, ticket.temporary_key.as_bytes())
        .map_err(|error| format!("Не удалось безопасно сохранить временный SSH-ключ: {error}"))?;
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

#[derive(Clone, Copy)]
pub enum AccessChange {
    Revoke,
    RemoveExpired,
    RemoveRevoked,
    Restore,
}

pub fn change_access(
    profile: &SavedProfile,
    secret: String,
    user: String,
    change: AccessChange,
) -> Result<(), String> {
    if profile.connection.user != "root" || !valid_user(&user) {
        return Err("Выберите профиль root и укажите SSH-пользователя из приглашения.".into());
    }
    let (command, acknowledgement) = match change {
        AccessChange::Revoke => ("revoke", "revoked"),
        AccessChange::RemoveExpired => ("remove-expired", "revoked"),
        AccessChange::RemoveRevoked => ("remove-revoked", "revoked"),
        AccessChange::Restore => ("restore", "restored"),
    };
    let response: Value = request(
        &profile.connection,
        profile.authentication == Authentication::Password,
        secret,
        command,
        json!({ "user": user }),
    )?;
    if response[acknowledgement] != true {
        return Err("Сервер не подтвердил изменение доступа.".into());
    }
    Ok(())
}

pub fn list(profile: &SavedProfile, secret: String) -> Result<Vec<AccessEntry>, String> {
    if profile.connection.user != "root" {
        return Err("Просмотр приглашений доступен владельцу сервера.".into());
    }
    let command = format!(
        "if {VPN_REQUIRED} || {INVITATIONS_REQUIRED}; then printf '\"setup-required\"\\n'; else exec {LIST_COMMAND}; fi"
    );
    let response: Value = exchange(
        &profile.connection,
        profile.authentication == Authentication::Password,
        secret,
        &command,
        "",
        35,
        false,
    )
    .map_err(|error| {
        if error.lines().any(|line| {
            line.trim() == "SSH Tunnel invitations: 'list-v5'"
                || (line.contains(HANDLER) && line.contains("No such file or directory"))
        }) {
            UPDATE_REQUIRED.to_owned()
        } else {
            error
        }
    })?;
    if response.as_str() == Some("setup-required") {
        return Err(UPDATE_REQUIRED.into());
    }
    serde_json::from_value(response).map_err(|_| "Сервер вернул неверный список приглашений.".into())
}

fn setup_installer(update: bool) -> (String, String) {
    let (vpn, invitations) = if update {
        (VPN_REQUIRED, INVITATIONS_REQUIRED)
    } else {
        (VPN_MISSING, INVITATIONS_MISSING)
    };
    let installer = format!(
        "set -eu\nif {vpn}; then\n(\n{VPN_INSTALLER}\n)\nfi\nif {invitations}; then\n(\n{INVITATION_INSTALLER}\n)\nfi\n"
    )
    .replace("\r\n", "\n");
    let size = installer.len();
    let reply = if update {
        format!("exec {LIST_COMMAND}")
    } else {
        "printf 'true\\n'".to_owned()
    };
    let command = format!(
        r#"set -eu
[ "$(id -u)" = 0 ]
umask 077
script=$(mktemp /tmp/ssh-tunnel-install.XXXXXX)
trap 'rm -f "$script"' EXIT
timeout 30 head -c {size} > "$script"
if [ "$(wc -c < "$script")" -ne {size} ]; then
    echo 'Server installer transfer was interrupted.' >&2
    exit 1
fi
(
    flock -w 120 9
    sh "$script" </dev/null >&2
) 9>/run/ssh-tunnel-vpn-install.lock
rm -f "$script"
{reply}"#
    );
    (command, installer)
}

fn configure_server<T: DeserializeOwned>(
    profile: &SavedProfile,
    secret: String,
    update: bool,
) -> Result<T, String> {
    if profile.connection.user != "root" {
        return Err("Для настройки сервера требуется SSH-доступ от root.".into());
    }
    let (command, installer) = setup_installer(update);
    exchange(
        &profile.connection,
        profile.authentication == Authentication::Password,
        secret,
        &command,
        &installer,
        600,
        update,
    )
}

pub fn setup_server(profile: &SavedProfile, secret: String) -> Result<(), String> {
    if configure_server::<bool>(profile, secret, false)? {
        Ok(())
    } else {
        Err("Сервер не подтвердил настройку.".into())
    }
}

pub fn update_and_list(profile: &SavedProfile, secret: String) -> Result<Vec<AccessEntry>, String> {
    configure_server(profile, secret, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_setup_installs_missing_components_and_manual_update_checks_versions() {
        for (update, expected_vpn, expected_invites) in [
            (false, VPN_MISSING, INVITATIONS_MISSING),
            (true, VPN_REQUIRED, INVITATIONS_REQUIRED),
        ] {
            let (command, installer) = setup_installer(update);
            assert!(installer.contains(&format!("if {expected_vpn}; then\n(")));
            assert!(installer.contains(&format!("if {expected_invites}; then\n(")));
            assert!(!installer.contains('\r'));
            assert!(command.contains(&format!("head -c {}", installer.len())));
            assert!(command.contains("flock -w 120"));
            assert_eq!(command.contains(LIST_COMMAND), update);
        }
    }

    #[test]
    fn openssh_accepts_bootstrap_key_in_temp_directory_with_inherited_access() {
        let directory = tempfile::tempdir().unwrap();
        let generated = directory.path().join("generated");
        let expected_public = generate(&generated).unwrap();
        let key = fs::read(&generated).unwrap();

        let acl = Command::new(system_program("icacls.exe").unwrap())
            .arg(directory.path())
            .args(["/grant", "*S-1-1-0:(OI)(CI)(RX)"])
            .output()
            .unwrap();
        assert!(
            acl.status.success(),
            "{}",
            String::from_utf8_lossy(&acl.stderr)
        );

        let read_public = |path: &Path| {
            Command::new(system_program(r"OpenSSH\ssh-keygen.exe").unwrap())
                .args(["-y", "-f"])
                .arg(path)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        };
        let inherited = directory.path().join("inherited");
        fs::write(&inherited, &key).unwrap();
        assert!(!read_public(&inherited).status.success());

        let bootstrap = directory.path().join("bootstrap");
        write_bootstrap_key(&bootstrap, &key).unwrap();
        let result = read_public(&bootstrap);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let actual_public = String::from_utf8(result.stdout).unwrap();
        assert!(actual_public.starts_with(&expected_public));
    }

    #[test]
    fn invitation_accepts_combined_and_legacy_formats() {
        let ticket = Ticket {
            server: "host.example".into(),
            user: "stv_0123456789abcdef".into(),
            host_key: "public".into(),
            temporary_key: "temporary".into(),
        };
        let (encrypted, code) = seal(&ticket).unwrap();
        assert!(!encrypted.contains("host.example") && !encrypted.contains(&code));
        assert_eq!(
            open_invitation(&encrypted, &code).unwrap().temporary_key,
            "temporary"
        );
        let combined = format!("{encrypted}:{code}");
        assert!(needs_legacy_code(&encrypted));
        assert!(!needs_legacy_code(&combined));
        assert_eq!(
            open_invitation(&combined, "").unwrap().temporary_key,
            ticket.temporary_key
        );
        assert!(open_invitation(&encrypted, "").is_err());
        assert!(open_invitation(
            &format!("{encrypted}:{}", URL_SAFE_NO_PAD.encode([0; 32])),
            ""
        )
        .is_err());
    }
}
