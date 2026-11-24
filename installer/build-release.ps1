param(
    [string]$InnoCompiler = 'ISCC.exe',
    [string]$DependencyDir,
    [string]$CargoTargetDir,
    [string]$Toolchain = 'stable-x86_64-pc-windows-gnu'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root 'target'
$release = Join-Path $target 'release'
$manifest = Join-Path $root 'Cargo.toml'
$installer = Join-Path $PSScriptRoot 'ssh-tunnel.iss'
$compiler = (Get-Command $InnoCompiler -CommandType Application).Source
$version = [regex]::Match([IO.File]::ReadAllText($manifest), '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
if (!$version) { throw 'Could not read the application version from Cargo.toml.' }

New-Item -ItemType Directory -Path $release -Force | Out-Null
if (!$DependencyDir) { $DependencyDir = $release }
if (!$CargoTargetDir) { $CargoTargetDir = $target }
$DependencyDir = (Resolve-Path -LiteralPath $DependencyDir).Path
$CargoTargetDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CargoTargetDir)

$dependencies = @('wintun.dll', 'WINTUN-LICENSE.txt')
$appRouting = @('sing-box.exe', 'libcronet.dll', 'SING-BOX-LICENSE.txt')
if ($appRouting | Where-Object { Test-Path -LiteralPath (Join-Path $DependencyDir $_) }) {
    $dependencies += $appRouting
}
foreach ($name in $dependencies) {
    if (!(Test-Path -LiteralPath (Join-Path $DependencyDir $name) -PathType Leaf)) {
        throw "Missing release dependency: $name (directory: $DependencyDir)"
    }
}

$cargo = @('build', '--release', '--locked', '--manifest-path', $manifest, '--target-dir', $CargoTargetDir)
if ($Toolchain) { $cargo = @("+$Toolchain") + $cargo }
& cargo @cargo
if ($LASTEXITCODE) { throw "Cargo failed with exit code $LASTEXITCODE." }

$built = Join-Path $CargoTargetDir 'release\ssh-tunnel.exe'
if ($CargoTargetDir -ne $target) { Copy-Item -LiteralPath $built -Destination $release -Force }
if ($DependencyDir -ne $release) {
    foreach ($name in $dependencies) {
        Copy-Item -LiteralPath (Join-Path $DependencyDir $name) -Destination $release -Force
    }
}

$stage = Join-Path $target ("installer-build-" + [guid]::NewGuid().ToString('N'))
$name = 'ssh-tunnel-setup-x64.exe'
$stagedSetup = Join-Path $stage $name
$setup = Join-Path $release $name
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    & $compiler /Qp "/DAppVersion=$version" "/DPayloadDir=$release" "/O$stage" $installer
    if ($LASTEXITCODE) { throw "Inno Setup failed with exit code $LASTEXITCODE." }
    if (Test-Path -LiteralPath $setup) {
        [IO.File]::Replace($stagedSetup, $setup, [NullString]::Value)
    } else {
        [IO.File]::Move($stagedSetup, $setup)
    }
} finally {
    if (Test-Path -LiteralPath $stagedSetup) { Remove-Item -LiteralPath $stagedSetup }
    if (!(Get-ChildItem -LiteralPath $stage -Force | Select-Object -First 1)) {
        Remove-Item -LiteralPath $stage
    }
}
if (-not ('SshTunnelInstallerShell' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class SshTunnelInstallerShell {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(uint eventId, uint flags, IntPtr item1, IntPtr item2);
}
'@
}
# Refresh Explorer's cached executable icons after replacing an installer at the same path.
[SshTunnelInstallerShell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
Get-Item -LiteralPath $setup
