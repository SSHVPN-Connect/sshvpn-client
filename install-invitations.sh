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
OWNER = "/usr/local/libexec/ssh-tunnel-invite"


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


def write_key(destination, content):
    with tempfile.NamedTemporaryFile(dir=KEYS, delete=False) as output:
        try:
            os.fchmod(output.fileno(), 0o644)
            output.write((content + "\n").encode())
            output.flush()
            os.fsync(output.fileno())
            os.replace(output.name, destination)
        finally:
            Path(output.name).unlink(missing_ok=True)


def request():
    data = sys.stdin.buffer.readline(4097)
    if not data or len(data) > 4096:
        raise ValueError("Invalid invitation request.")
    return json.loads(data)


def issue(data):
    key = public_key(data["public_key"])
    host = public_key(" ".join(Path("/etc/ssh/ssh_host_ed25519_key.pub").read_text().split()[:2]))
    user = "stv_" + secrets.token_hex(8)
    expiry = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=30)).strftime("%Y%m%d%H%M%SZ")
    # Random crypt-shaped password hash with no known password. SSH disables passwords for this group.
    password = "$6$" + secrets.token_hex(8) + "$" + ".".join([secrets.token_hex(42), "a"])
    subprocess.run(["useradd", "--system", "--no-create-home", "--home-dir", "/nonexistent",
                    "--gid", "ssh-tunnel-share", "--shell", "/bin/sh", "--password", password, user], check=True)
    try:
        command = f'restrict,expiry-time="{expiry}",command="/usr/bin/sudo -n {OWNER} redeem"'
        write_key(KEYS / user, f"{command} {key}")
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
        write_key(KEYS / user, f'restrict,port-forwarding,command="/bin/false" {key}')
    return {"user": user}


def revoke(data):
    user = account(data["user"])
    with (STATE / user).open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
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
        result = {"issue": issue, "redeem": redeem, "revoke": revoke}[operation](request())
        print(json.dumps(result))
    except (OSError, ValueError, TypeError, KeyError, subprocess.CalledProcessError) as error:
        print(f"SSH Tunnel invitations: {error}", file=sys.stderr)
        sys.exit(1)
PYTHON
install -m 0755 "$temp" /usr/local/libexec/ssh-tunnel-invite
printf '%s\n' '%ssh-tunnel-share ALL=(root) NOPASSWD: /usr/local/libexec/ssh-tunnel-invite redeem' > "$temp"
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
touch /var/lib/ssh-tunnel-invites/installed-v1
printf '%s\n' 'SSH Tunnel invitations are installed on port 22.'
