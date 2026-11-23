#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
    printf '%s\n' 'Run as root, for example: sudo sh install-server.sh' >&2
    exit 1
fi

if [ ! -r /etc/os-release ]; then
    printf '%s\n' 'Cannot determine Linux distribution.' >&2
    exit 1
fi
. /etc/os-release
case "${ID:-}" in
    debian|ubuntu) ;;
    *)
        printf 'Unsupported distribution: %s. Use Debian or Ubuntu.\n' "${ID:-unknown}" >&2
        exit 1
        ;;
esac

set --
for package in openssh-server iproute2 nftables iptables conntrack python3; do
    status=$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null || :)
    if [ "$status" != 'install ok installed' ]; then
        set -- "$@" "$package"
    fi
done

if [ "$#" -gt 0 ]; then
    if command -v sshvpn_setup_stage >/dev/null; then sshvpn_setup_stage p; fi
    apt-get -o Acquire::Retries=1 -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15 update
    if command -v sshvpn_setup_stage >/dev/null; then sshvpn_setup_stage d; fi
    DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=1 -o Acquire::http::Timeout=15 \
        -o Acquire::https::Timeout=15 -o DPkg::Lock::Timeout=120 install -y --no-install-recommends "$@"
fi

if command -v sshvpn_setup_stage >/dev/null; then sshvpn_setup_stage v; fi
dpkg-query -W -f='${Package} ${Version}\n' openssh-server iproute2 nftables iptables conntrack python3
if [ ! -c /dev/net/tun ]; then
    printf '%s\n' 'Packages are installed. The kernel or hosting provider must enable /dev/net/tun.' >&2
    exit 1
fi
if ! ( : <> /dev/net/tun ); then
    printf '%s\n' 'Packages are installed, but this environment does not allow opening /dev/net/tun.' >&2
    exit 1
fi
install -d -m 0755 /usr/local/libexec
target=/usr/local/libexec/ssh-tunnel-vpn
temporary=$(mktemp "$target.XXXXXX")
trap 'rm -f "$temporary"' EXIT
cat > "$temporary" <<'PYTHON'
#!/usr/bin/python3
"""Carry framed IPv4 packets over an authenticated SSH exec channel."""
from contextlib import contextmanager
import fcntl
import ipaddress
import json
import os
import select
import shlex
import signal
import struct
import subprocess
import sys
import time

ADDRESS_POOL = ipaddress.IPv4Network("172.31.252.0/24")
SESSION_DIR = "/run/ssh-tunnel-vpn"
MAX_PACKET = 1420


def command(*args, **kwargs):
    return subprocess.run(args, check=True, stdout=subprocess.DEVNULL, **kwargs)


def write_all(fd, data):
    view = memoryview(data)
    while view:
        view = view[os.write(fd, view):]


def forward(tun, client, multi):
    pending = bytearray()
    write_all(1, b"SSHVPN2\n" + client if multi else b"SSHVPN1\n")
    while True:
        ready, _, _ = select.select([0, tun], [], [])
        if 0 in ready:
            incoming = os.read(0, 16384)
            if not incoming:
                return
            pending.extend(incoming)
            while len(pending) >= 2:
                length = int.from_bytes(pending[:2], "big")
                if not 20 <= length <= MAX_PACKET:
                    raise ValueError("invalid IPv4 frame length")
                if len(pending) < length + 2:
                    break
                packet = pending[2:length + 2]
                del pending[:length + 2]
                if packet[0] >> 4 != 4 or packet[12:16] != client:
                    continue
                if os.write(tun, packet) != length:
                    raise OSError("short write to TUN")
        if tun in ready:
            packet = os.read(tun, MAX_PACKET)
            if not packet:
                return
            if len(packet) < 20 or packet[0] >> 4 != 4 or packet[16:20] != client:
                continue
            write_all(1, len(packet).to_bytes(2, "big") + packet)


def acquire_vpn_lock(lock):
    deadline = time.monotonic() + 8
    while True:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return
        except BlockingIOError as error:
            if time.monotonic() >= deadline:
                raise RuntimeError(
                    "Another System VPN session still holds the server interface. "
                    "Disconnect it or wait for it to close, then reconnect."
                ) from error
            # SSH stdin reaches EOF if the waiting connection closes.
            ready, _, _ = select.select([0], [], [], 0.25)
            if ready:
                raise RuntimeError("SSH channel closed before the VPN started.")


@contextmanager
def session_slot(multi):
    if not multi:
        with open("/run/ssh-tunnel-vpn.lock", "a", encoding="ascii") as lock:
            acquire_vpn_lock(lock)
            yield "sshvpn0", "ssh_tunnel_vpn", ipaddress.IPv4Network("172.31.255.0/30")
        return

    os.makedirs(SESSION_DIR, mode=0o700, exist_ok=True)
    for slot, network in enumerate(ADDRESS_POOL.subnets(new_prefix=30)):
        with open(f"{SESSION_DIR}/{slot}.lock", "a", encoding="ascii") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                continue
            yield f"sshvpnm{slot}", f"ssh_tunnel_vpn_m{slot}", network
            return
    raise RuntimeError("All System VPN session slots are in use. Try again later.")


def clear_slot_filters(table):
    rules = subprocess.check_output(["iptables", "-w", "-S", "FORWARD"], text=True)
    for rule in rules.splitlines():
        parts = shlex.split(rule)
        if parts[:2] != ["-A", "FORWARD"] or "--comment" not in parts:
            continue
        if parts[parts.index("--comment") + 1] == table:
            command("iptables", "-w", "-D", *parts[1:])


def clear_slot_connections(client):
    selector = ["-f", "ipv4", "--orig-src", str(client)]
    deletion = subprocess.run(["conntrack", "-D", *selector], capture_output=True)
    if deletion.returncode:
        # conntrack reports a non-zero status when there are no matches to delete.
        if subprocess.check_output(["conntrack", "-L", *selector], stderr=subprocess.DEVNULL).strip():
            raise RuntimeError("Could not clear previous VPN connection tracking entries.")


def main():
    if os.geteuid() != 0 or sys.argv[1:] not in ([], ["--multi"]):
        raise RuntimeError("ssh-tunnel-vpn requires root and accepts only --multi")
    if open("/proc/sys/net/ipv4/ip_forward", encoding="ascii").read().strip() != "1":
        raise RuntimeError("IPv4 forwarding is disabled. Run install-server.sh.")
    for sig in (signal.SIGHUP, signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, lambda *_: sys.exit(0))
    multi = sys.argv[1:] == ["--multi"]
    with session_slot(multi) as (device, table, network):
        gateway, client = network.hosts()
        uplink = json.loads(subprocess.check_output(
            ["ip", "-j", "-4", "route", "get", "1.1.1.1"]
        ))[0]["dev"]
        tun = os.open("/dev/net/tun", os.O_RDWR | os.O_CLOEXEC)
        table_created = False
        filters = []
        try:
            if multi:
                clear_slot_connections(client)
            fcntl.ioctl(tun, 0x400454CA, struct.pack("16sH22x", device.encode(), 0x9001))
            command("ip", "-4", "address", "add", f"{gateway}/30", "dev", device)
            command("ip", "link", "set", device, "mtu", str(MAX_PACKET), "up")
            if multi:
                # The slot lock guarantees no running session owns these rules.
                clear_slot_filters(table)
                subprocess.run(["nft", "delete", "table", "ip", table],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            rules = f"""
                create table ip {table}
                add chain ip {table} postrouting {{ type nat hook postrouting priority srcnat; }}
                add rule ip {table} postrouting iifname "{device}" oifname {json.dumps(uplink)} ip saddr {client} masquerade
                add chain ip {table} isolate {{ type filter hook forward priority -5; }}
                add rule ip {table} isolate iifname "{device}" oifname != {json.dumps(uplink)} drop
                add rule ip {table} isolate oifname "{device}" iifname != {json.dumps(uplink)} drop
            """
            command("nft", "-f", "-", input=rules.encode())
            table_created = True
            for rule in (
                ["-i", device, "-o", uplink, "-s", f"{client}/32",
                 "--ctstate", "NEW,ESTABLISHED,RELATED"],
                ["-i", uplink, "-o", device, "-d", f"{client}/32",
                 "--ctstate", "ESTABLISHED,RELATED"],
            ):
                match = rule[:-2] + ["-m", "conntrack"] + rule[-2:]
                match += ["-m", "comment", "--comment", table, "-j", "ACCEPT"]
                command("iptables", "-w", "-I", "FORWARD", "1", *match)
                filters.append(match)
            forward(tun, client.packed, multi)
        finally:
            try:
                for match in reversed(filters):
                    subprocess.run(["iptables", "-w", "-D", "FORWARD", *match], check=False)
                if table_created:
                    command("nft", "delete", "table", "ip", table)
            finally:
                os.close(tun)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"SSH Tunnel VPN: {error}", file=sys.stderr)
        sys.exit(1)
PYTHON
chmod 0755 "$temporary"
mv -f "$temporary" "$target"
printf '%s\n' 'net.ipv4.ip_forward = 1' > /etc/sysctl.d/91-ssh-tunnel-vpn.conf
sysctl -w net.ipv4.ip_forward=1
install -d -m 0755 /var/lib/ssh-tunnel-vpn
touch /var/lib/ssh-tunnel-vpn/installed-v2 /var/lib/ssh-tunnel-vpn/installed-v3
printf '%s\n' "VPN handler installed at $target."
