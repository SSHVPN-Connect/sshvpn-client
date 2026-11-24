param(
    [Parameter(Mandatory)]
    [ValidateSet('Install', 'Stop', 'Uninstall')]
    [string]$Action,

    [Parameter(Mandatory)]
    [string]$InstallDir,

    [string]$ErrorFile
)

$ErrorActionPreference = 'Stop'
trap {
    if ($ErrorFile) {
        [IO.File]::WriteAllText($ErrorFile, $_.Exception.Message, [Text.UTF8Encoding]::new($false))
    }
    Write-Error -Message $_.Exception.Message -ErrorAction Continue
    exit 1
}
$name = 'SshTunnelNetwork'
$expected = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'SSH Tunnel'
if ([IO.Path]::GetFullPath($InstallDir) -ne $expected) {
    throw "SSH Tunnel Network must be installed in $expected."
}

$service = Get-Service -Name $name -ErrorAction SilentlyContinue
if ($Action -eq 'Install') {
    $binary = '"{0}" --vpn-service' -f (Join-Path $expected 'ssh-tunnel.exe')
    if (!$service) {
        $service = New-Service -Name $name -BinaryPathName $binary -DisplayName 'SSH Tunnel Network' -StartupType Automatic
    } else {
        $registered = Get-CimInstance Win32_Service -Filter "Name='$name'"
        if ($registered.PathName -ne $binary -or $registered.StartName -notin @('LocalSystem','Local System')) {
            throw 'SSH Tunnel Network service registration does not match this installation.'
        }
        Set-Service -Name $name -StartupType Automatic
    }
    Start-Service -Name $name
    $service.WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
} elseif ($service) {
    if ($service.Status -ne 'Stopped') {
        Stop-Service -Name $name
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(50))
    }
    if ($Action -eq 'Uninstall') {
        $service.Dispose()
        & (Join-Path $env:SystemRoot 'System32\sc.exe') delete $name
        if ($LASTEXITCODE -ne 0) {
            throw 'Could not remove SSH Tunnel Network.'
        }
    }
}
