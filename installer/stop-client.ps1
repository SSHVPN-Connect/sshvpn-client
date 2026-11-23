param(
    [Parameter(Mandatory)]
    [string]$InstallDir
)

$ErrorActionPreference = 'Stop'
$app = Join-Path $InstallDir 'ssh-tunnel.exe'
$engine = Join-Path $InstallDir 'sing-box.exe'

function Get-InstalledClientProcess {
    Get-Process -Name 'ssh-tunnel', 'sing-box' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $app -or $_.Path -eq $engine }
}

foreach ($process in @(Get-InstalledClientProcess | Where-Object { $_.Path -eq $app })) {
    try {
        $eventName = "Local\SSHVPN-Connect.SshTunnel.Shutdown.$($process.Id)"
        $event = [Threading.EventWaitHandle]::OpenExisting($eventName)
        try {
            if ($event.Set()) {
                [void]$process.WaitForExit(25000)
            }
        } finally {
            $event.Dispose()
        }
    } catch [Threading.WaitHandleCannotBeOpenedException], [UnauthorizedAccessException], [InvalidOperationException] {
        # Older clients have no shutdown event, and Local events are session-specific.
    }
}

foreach ($process in @(Get-InstalledClientProcess)) {
    if (!$process.HasExited) {
        Stop-Process -InputObject $process -Force
        if (!$process.WaitForExit(8000)) {
            throw "Process $($process.Id) did not exit."
        }
    }
}

if (Get-InstalledClientProcess) {
    throw 'SSH Tunnel files are still in use.'
}
