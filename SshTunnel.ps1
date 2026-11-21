param([switch]$Check)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
[Windows.Forms.Application]::EnableVisualStyles()

$profileDirectory = Join-Path $env:LOCALAPPDATA 'SshTunnelClient'
$profileFile = Join-Path $profileDirectory 'profile.json'
$profile = @{ Server = ''; User = ''; SocksPort = 1080; Key = '' }
if (Test-Path -LiteralPath $profileFile) {
    try {
        $saved = Get-Content -LiteralPath $profileFile -Raw -Encoding UTF8 | ConvertFrom-Json
        foreach ($name in @($profile.Keys)) {
            if ($null -ne $saved.$name) { $profile[$name] = $saved.$name }
        }
    } catch {
        Write-Warning "Could not read saved profile: $($_.Exception.Message)"
    }
}

$form = New-Object Windows.Forms.Form
$form.Text = 'SSH Tunnel'
$form.ClientSize = [Drawing.Size]::new(480, 464)
$form.MinimumSize = $form.Size
$form.MaximumSize = $form.Size
$form.StartPosition = 'CenterScreen'
$form.BackColor = [Drawing.Color]::FromArgb(17, 24, 39)
$form.ForeColor = [Drawing.Color]::WhiteSmoke
$form.Font = [Drawing.Font]::new('Segoe UI', 10)

function Add-Label($Text, $Top, $Size = 10) {
    $label = New-Object Windows.Forms.Label
    $label.Text = $Text
    $label.SetBounds(24, $Top, 432, 32)
    $label.Font = [Drawing.Font]::new('Segoe UI', $Size)
    $form.Controls.Add($label)
    return $label
}

function Add-Field($Text, $Value, $Top) {
    $label = Add-Label $Text $Top
    $label.Height = 23
    $field = New-Object Windows.Forms.TextBox
    $field.Text = $Value
    $field.SetBounds(24, ($Top + 24), 432, 26)
    $field.BackColor = [Drawing.Color]::FromArgb(37, 48, 67)
    $field.ForeColor = [Drawing.Color]::WhiteSmoke
    $form.Controls.Add($field)
    return $field
}

function Add-Button($Text, $Left, $Top, $Width) {
    $button = New-Object Windows.Forms.Button
    $button.Text = $Text
    $button.SetBounds($Left, $Top, $Width, 36)
    $button.FlatStyle = 'Flat'
    $button.BackColor = [Drawing.Color]::FromArgb(37, 48, 67)
    $form.Controls.Add($button)
    return $button
}

$null = Add-Label 'SSH Tunnel' 16 21
$status = Add-Label 'Disconnected' 61
$server = Add-Field 'SSH server (port 22)' $profile.Server 99
$user = Add-Field 'SSH username' $profile.User 154
$socks = Add-Field 'Local SOCKS5 port' $profile.SocksPort 209
$key = Add-Field 'Private key path (optional)' $profile.Key 264
$key.Width = 330
$browse = Add-Button 'Browse...' 362 284 94
$connect = Add-Button 'Connect' 24 332 202
$connect.BackColor = [Drawing.Color]::FromArgb(12, 110, 99)
$save = Add-Button 'Save profile' 238 332 218
$endpoint = Add-Label '' 383
$endpoint.Height = 26
$copy = Add-Button 'Copy proxy address' 24 412 202
$copy.Enabled = $false

$script:ssh = $null
$editControls = @($server, $user, $socks, $key, $browse, $save)

function Get-Profile {
    $address = $server.Text.Trim()
    $username = $user.Text.Trim()
    $port = 0
    $keyPath = $key.Text.Trim()
    if ($address -notmatch '^[a-zA-Z0-9][a-zA-Z0-9.:-]*$' -or
        [Uri]::CheckHostName($address) -eq [UriHostNameType]::Unknown) {
        if ([Uri]::CheckHostName($address) -ne [UriHostNameType]::IPv6 -or
            $address -notmatch '^[0-9a-fA-F:.]+$') {
            throw 'Enter a server hostname or IP address.'
        }
    }
    if ($username -notmatch '^[a-zA-Z0-9_][a-zA-Z0-9_.@-]*\$?$') {
        throw 'Enter a valid SSH username.'
    }
    if (-not [int]::TryParse($socks.Text.Trim(), [ref]$port) -or $port -lt 1 -or $port -gt 65535) {
        throw 'Enter a local port between 1 and 65535.'
    }
    if ($keyPath) {
        $keyPath = (Get-Item -LiteralPath $keyPath).FullName
        if (-not (Test-Path -LiteralPath $keyPath -PathType Leaf)) {
            throw 'Select a private key file.'
        }
    }
    return @{ Server = $address; User = $username; SocksPort = $port; Key = $keyPath }
}

function Set-Disconnected($Message) {
    if ($null -ne $script:ssh) {
        $script:ssh.Dispose()
        $script:ssh = $null
    }
    $status.Text = $Message
    $connect.Text = 'Connect'
    $copy.Enabled = $false
    $endpoint.Text = ''
    foreach ($control in $editControls) { $control.Enabled = $true }
}

function Stop-Tunnel {
    if ($null -ne $script:ssh -and -not $script:ssh.HasExited) {
        Stop-Process -Id $script:ssh.Id -ErrorAction Stop
    }
    Set-Disconnected 'Disconnected'
}

$browse.Add_Click({
    $dialog = New-Object Windows.Forms.OpenFileDialog
    try {
        if ($dialog.ShowDialog($form) -eq 'OK') { $key.Text = $dialog.FileName }
    } finally {
        $dialog.Dispose()
    }
})

$save.Add_Click({
    try {
        $settings = Get-Profile
        [IO.Directory]::CreateDirectory($profileDirectory) | Out-Null
        $settings | ConvertTo-Json | Set-Content -LiteralPath $profileFile -Encoding UTF8
        $status.Text = 'Profile saved'
    } catch {
        $status.Text = $_.Exception.Message
    }
})

$copy.Add_Click({ [Windows.Forms.Clipboard]::SetText($endpoint.Text) })

$connect.Add_Click({
    try {
        if ($null -ne $script:ssh) {
            Stop-Tunnel
            return
        }
        $settings = Get-Profile
        $sshPath = Join-Path $env:WINDIR 'System32\OpenSSH\ssh.exe'
        if (-not (Test-Path -LiteralPath $sshPath)) {
            throw 'Install the Windows OpenSSH Client optional feature.'
        }
        $script:localPort = $settings.SocksPort
        $arguments = @(
            '-N', '-T', '-F', 'none', '-p', '22',
            '-o', 'ExitOnForwardFailure=yes',
            '-o', 'ConnectTimeout=15',
            '-o', 'ServerAliveInterval=15',
            '-o', 'ServerAliveCountMax=3',
            '-o', 'StrictHostKeyChecking=ask',
            '-D', "127.0.0.1:${script:localPort}",
            '-l', $settings.User
        )
        if ($settings.Key) { $arguments += @('-i', ('"' + $settings.Key + '"')) }
        $arguments += @('--', $settings.Server)
        # Keep a console for native OpenSSH authentication and host key prompts.
        $script:ssh = Start-Process -FilePath $sshPath -ArgumentList $arguments -PassThru -WindowStyle Normal
        $status.Text = 'Connecting. Check the OpenSSH window.'
        $connect.Text = 'Cancel'
        foreach ($control in $editControls) { $control.Enabled = $false }
    } catch {
        $status.Text = $_.Exception.Message
    }
})

$timer = New-Object Windows.Forms.Timer
$timer.Interval = 1000
$timer.Add_Tick({
    if ($null -eq $script:ssh) { return }
    if ($script:ssh.HasExited) {
        $exitCode = $script:ssh.ExitCode
        Set-Disconnected "SSH session ended (exit code $exitCode)."
        return
    }
    if ($copy.Enabled) { return }
    $listeners = @(Get-NetTCPConnection -State Listen -LocalAddress '127.0.0.1' `
        -LocalPort $script:localPort -OwningProcess $script:ssh.Id -ErrorAction SilentlyContinue)
    if ($listeners.Count -gt 0) {
        $status.Text = 'Connected'
        $connect.Text = 'Disconnect'
        $endpoint.Text = "127.0.0.1:${script:localPort}"
        $copy.Enabled = $true
    }
})

$form.Add_FormClosing({
    param($sender, $eventArgs)
    try { Stop-Tunnel } catch {
        $status.Text = $_.Exception.Message
        $eventArgs.Cancel = $true
    }
})

try {
    if ($Check) {
        $server.Text = '::1'
        $user.Text = 'test_user'
        $socks.Text = '1080'
        $key.Text = ''
        $settings = Get-Profile
        if ($settings.Server -ne '::1' -or $settings.SocksPort -ne 1080) {
            throw 'Profile validation failed.'
        }
        Write-Output 'Form and profile validation passed.'
    } else {
        $timer.Start()
        $null = $form.ShowDialog()
    }
} finally {
    $timer.Dispose()
    $form.Dispose()
}
