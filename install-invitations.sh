#!/bin/sh
set -eu
[ "$(id -u)" = 0 ] || { echo 'Run as root.' >&2; exit 1; }
. /etc/os-release
case "$ID" in debian|ubuntu) ;; *) echo 'Debian or Ubuntu is required.' >&2; exit 1 ;; esac
if ! command -v sudo >/dev/null || ! command -v python3 >/dev/null; then
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends sudo python3
fi
getent group ssh-tunnel-share >/dev/null || groupadd --system ssh-tunnel-share
install -d -m 0755 /usr/local/libexec /etc/ssh/ssh-tunnel-invites
install -d -m 0700 /var/lib/ssh-tunnel-invites
temp=$(mktemp)
candidate=$(mktemp /etc/ssh/.ssh-tunnel-config.XXXXXX)
trap 'rm -f "$temp" "$candidate"' EXIT
cat > "$temp" <<'PYTHON'
#!/usr/bin/python3
import base64
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import struct
import subprocess
import sys
import tempfile

KEYS = Path("/etc/ssh/ssh-tunnel-invites")
STATE = Path("/var/lib/ssh-tunnel-invites")
RECORDS = STATE / "records"
OWNER = "/usr/local/libexec/ssh-tunnel-invite"
SESSION = "/usr/local/libexec/ssh-tunnel-invite-session"
LEGACY_KEY = 'restrict,port-forwarding,command="/bin/false" '


def public_key(value):
    parts = value.split()
    if len(parts) != 2 or parts[0] != "ssh-ed25519" or len(parts[1]) != 68:
        raise ValueError("Expected an Ed25519 public key.")
    payload = base64.b64decode(parts[1], validate=True)
    prefix = struct.pack(">I", 11) + b"ssh-ed25519" + struct.pack(">I", 32)
    if not payload.startswith(prefix) or len(payload) != len(prefix) + 32:
        raise ValueError("Invalid Ed25519 public key.")
    return " ".join(parts)


def account(value):
    if not re.fullmatch(r"stv_[0-9a-f]{16}", value):
        raise ValueError("Invalid invitation account.")
    return value


def write_file(destination, content, mode):
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as output:
        try:
            os.fchmod(output.fileno(), mode)
            output.write((content + "\n").encode())
            output.flush()
            os.fsync(output.fileno())
            os.replace(output.name, destination)
        finally:
            Path(output.name).unlink(missing_ok=True)


def write_key(destination, content):
    write_file(destination, content, 0o644)


def read_record(user):
    try:
        return json.loads((RECORDS / user).read_text())
    except FileNotFoundError:
        return {}


def save_record(user, record):
    write_file(RECORDS / user, json.dumps(record), 0o600)


def fingerprint(key):
    digest = hashlib.sha256(base64.b64decode(key.split()[1])).digest()
    return "SHA256:" + base64.b64encode(digest).decode().rstrip("=")


def invitation_state(user, record):
    try:
        key = (KEYS / user).read_text().strip()
    except FileNotFoundError:
        return "revoked", record.get("fingerprint")
    match = re.match(r'restrict,expiry-time="([0-9]{14}Z)",command=', key)
    if match:
        now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%d%H%M%SZ")
        return ("pending" if match[1] > now else "expired"), None
    return "active", fingerprint(public_key(key.split(" ", 1)[1]))


def list_access(_data):
    entries = []
    fingerprints = {}
    for user in sorted({path.name for directory in (KEYS, RECORDS) for path in directory.iterdir()}):
        if not re.fullmatch(r"stv_[0-9a-f]{16}", user):
            continue
        with (STATE / user).open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_SH)
            record = read_record(user)
            if not record and not (KEYS / user).exists():
                continue
            state, key = invitation_state(user, record)
        entry = {"user": user, "name": record.get("name", ""), "state": state,
                 "last_connected": None}
        entries.append(entry)
        if key:
            fingerprints[user, key] = entry
    if not fingerprints:
        return entries
    pattern = r"^Accepted publickey for (stv_[0-9a-f]{16}) from .* (SHA256:[A-Za-z0-9+/]+)$"
    try:
        output = subprocess.run(
            ["journalctl", "--quiet", "--no-pager", "--reverse", "--output=json",
             "--lines=4096", "--output-fields=MESSAGE", "--identifier=sshd",
             "--identifier=sshd-session", "--grep", pattern, "_UID=0"],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=6,
        ).stdout
    except subprocess.TimeoutExpired as error:
        output = error.stdout or b""
    except OSError:
        return entries
    for line in output.splitlines():
        try:
            event = json.loads(line)
            match = re.fullmatch(pattern, event.get("MESSAGE", ""))
            if not match or match.groups() not in fingerprints:
                continue
            seconds = int(event["__REALTIME_TIMESTAMP"]) // 1_000_000
            last_connected = datetime.datetime.fromtimestamp(
                seconds, datetime.timezone.utc
            ).strftime("%Y-%m-%d %H:%M:%S UTC")
        except (ValueError, TypeError, KeyError, OverflowError, OSError):
            continue
        fingerprints.pop(match.groups())["last_connected"] = last_connected
        if not fingerprints:
            break
    return entries


def request():
    data = sys.stdin.buffer.readline(4097)
    if not data or len(data) > 4096:
        raise ValueError("Invalid invitation request.")
    return json.loads(data)


def issue(data):
    key = public_key(data["public_key"])
    name = data.get("name", "").strip()
    if len(name) > 80 or any(not char.isprintable() for char in name):
        raise ValueError("Invalid recipient name.")
    host = public_key(" ".join(Path("/etc/ssh/ssh_host_ed25519_key.pub").read_text().split()[:2]))
    user = "stv_" + secrets.token_hex(8)
    expiry = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=30)).strftime("%Y%m%d%H%M%SZ")
    # Random crypt-shaped password hash with no known password. SSH disables passwords for this group.
    password = "$6$" + secrets.token_hex(8) + "$" + ".".join([secrets.token_hex(42), "a"])
    subprocess.run(["useradd", "--system", "--no-create-home", "--home-dir", "/nonexistent",
                    "--gid", "ssh-tunnel-share", "--shell", "/bin/sh", "--password", password, user], check=True)
    try:
        with (STATE / user).open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            save_record(user, {"name": name})
            try:
                command = f'restrict,expiry-time="{expiry}",command="/usr/bin/sudo -n {OWNER} redeem"'
                write_key(KEYS / user, f"{command} {key}")
            except BaseException:
                (RECORDS / user).unlink(missing_ok=True)
                raise
    except BaseException:
        subprocess.run(["userdel", user], check=False)
        raise
    return {"user": user, "host_key": host, "expires": expiry}


def redeem(data):
    user = account(os.environ.get("SUDO_USER", ""))
    key = public_key(data["public_key"])
    with (STATE / user).open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        saved = (KEYS / user).read_text()
        match = re.match(r'restrict,expiry-time="([0-9]{14}Z)",command=', saved)
        now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%d%H%M%SZ")
        if not match or match[1] <= now:
            raise ValueError("Invitation was already used or has expired.")
        write_session_key(user, key)
    return {"user": user}


def write_session_key(user, key):
    write_key(KEYS / user, f'restrict,port-forwarding,command="{SESSION}" {key}')


def upgrade():
    for path in KEYS.iterdir():
        if not re.fullmatch(r"stv_[0-9a-f]{16}", path.name):
            continue
        with (STATE / path.name).open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            try:
                saved = path.read_text().strip()
            except FileNotFoundError:
                continue
            if saved.startswith(LEGACY_KEY):
                write_session_key(path.name, public_key(saved[len(LEGACY_KEY):]))


def revoke(data):
    user = account(data["user"])
    with (STATE / user).open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        record = read_record(user)
        if (KEYS / user).exists():
            _state, key = invitation_state(user, record)
            save_record(user, {**record, "fingerprint": key})
        (KEYS / user).unlink(missing_ok=True)
        result = subprocess.run(["id", "-gn", user], capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip() == "ssh-tunnel-share":
            subprocess.run(["pkill", "-KILL", "-u", user], check=False)
            subprocess.run(["userdel", user], check=True)
    return {"revoked": True}


if __name__ == "__main__":
    try:
        if os.geteuid() != 0 or len(sys.argv) != 2:
            raise ValueError("Unauthorized request.")
        operation = sys.argv[1]
        if bool(os.environ.get("SUDO_USER")) != (operation == "redeem"):
            raise ValueError("Unauthorized operation.")
        if operation == "upgrade":
            upgrade()
            sys.exit(0)
        if operation == "list-v2":
            print(json.dumps(list_access(None)))
            sys.exit(0)
        result = {"issue": issue, "redeem": redeem, "revoke": revoke, "list": list_access}[operation](request())
        print(json.dumps(result))
    except (OSError, RuntimeError, ValueError, TypeError, KeyError, subprocess.CalledProcessError) as error:
        print(f"SSH Tunnel invitations: {error}", file=sys.stderr)
        sys.exit(1)
PYTHON
install -d -m 0700 /var/lib/ssh-tunnel-invites/records
install -m 0755 "$temp" /usr/local/libexec/ssh-tunnel-invite
cat > "$temp" <<'SESSION'
#!/bin/sh
set -eu
if [ "${SSH_ORIGINAL_COMMAND-}" != "ssh-tunnel-vpn" ]; then
    exit 1
fi
if [ ! -x /usr/local/libexec/ssh-tunnel-vpn ]; then
    echo 'System IPv4 VPN is unavailable. Ask the owner to connect in System VPN mode first.' >&2
    exit 1
fi
exec /usr/bin/sudo -n -- /usr/local/libexec/ssh-tunnel-vpn
SESSION
install -m 0755 "$temp" /usr/local/libexec/ssh-tunnel-invite-session
printf '%s\n' '%ssh-tunnel-share ALL=(root) NOPASSWD: /usr/local/libexec/ssh-tunnel-invite redeem, /usr/local/libexec/ssh-tunnel-vpn ""' > "$temp"
visudo -cf "$temp" >/dev/null
install -m 0440 "$temp" /etc/sudoers.d/ssh-tunnel-invites
cat > "$temp" <<'SSHD'
Match Group ssh-tunnel-share
    AuthorizedKeysFile /etc/ssh/ssh-tunnel-invites/%u
    AuthorizedKeysCommand none
    TrustedUserCAKeys none
    AuthenticationMethods publickey
    PasswordAuthentication no
    KbdInteractiveAuthentication no
    AllowTcpForwarding local
    AllowStreamLocalForwarding no
    AllowAgentForwarding no
    X11Forwarding no
    PermitTTY no
    PermitTunnel no
    PermitUserRC no
SSHD
install -m 0644 "$temp" /etc/ssh/ssh-tunnel-invites.conf
cp /etc/ssh/sshd_config "$candidate"
if ! grep -Fxq 'Include /etc/ssh/ssh-tunnel-invites.conf' "$candidate"; then
    printf '\nMatch all\nInclude /etc/ssh/ssh-tunnel-invites.conf\n' >> "$candidate"
fi
/usr/sbin/sshd -t -f "$candidate"
/usr/sbin/sshd -T -f "$candidate" -C user=nobody,host=localhost,addr=127.0.0.1 >/dev/null
cp -p /etc/ssh/sshd_config "$temp"
install -m 0644 "$candidate" /etc/ssh/sshd_config
if ! systemctl reload ssh; then
    install -m 0644 "$temp" /etc/ssh/sshd_config
    exit 1
fi
env -u SUDO_USER /usr/local/libexec/ssh-tunnel-invite upgrade
touch /var/lib/ssh-tunnel-invites/installed-v1 /var/lib/ssh-tunnel-invites/installed-v2 /var/lib/ssh-tunnel-invites/installed-v3 /var/lib/ssh-tunnel-invites/installed-v4
printf '%s\n' 'SSH Tunnel invitations are installed on port 22.'
