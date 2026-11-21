use std::{env, io, path::PathBuf, process::Command};

const ASSET: &str = "ssh-tunnel-setup-x64.exe";

pub fn prepare() -> Result<Option<PathBuf>, String> {
    let program_files = env::var_os("ProgramW6432")
        .or_else(|| env::var_os("ProgramFiles"))
        .ok_or("Не найдена папка Program Files.")?;
    let installed = PathBuf::from(program_files).join("SSH Tunnel");
    let canonical = installed
        .canonicalize()
        .map_err(|_| "Установи SSH Tunnel через установщик из GitHub Releases.")?;
    let program = env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| error.to_string())?;
    if program.parent() != Some(canonical.as_path()) {
        return Err(
            "Обновление доступно для версии, установленной через установщик в Program Files."
                .into(),
        );
    }

    let staging = tempfile::NamedTempFile::new_in(&installed)
        .map_err(|error| format!("Не удалось подготовить обновление: {error}"))?;
    let temporary = staging.into_temp_path();
    let result = crate::vpn::powershell(&download_script(&temporary, env!("CARGO_PKG_VERSION")))
        .map_err(|error| format!("Не удалось обновить приложение: {error}"))?;
    if result.trim().is_empty() {
        return Ok(None);
    }
    let pending = installed.join("pending-update.exe");
    temporary
        .persist(&pending)
        .map_err(|error| format!("Не удалось сохранить установщик: {error}"))?;
    Ok(Some(pending))
}

fn download_script(path: &std::path::Path, current: &str) -> String {
    let path = path.to_string_lossy().replace('\'', "''");
    format!(
        r#"
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $headers = @{{ 'User-Agent' = 'SSH-Tunnel'; Accept = 'application/vnd.github+json' }}
        $api = 'https://api.github.com/repos/d1llr/sshvpn'
        $token = $env:SSH_TUNNEL_GITHUB_TOKEN
        if ($token) {{ $headers.Authorization = 'Bearer ' + $token.Trim() }}
        try {{
            $release = Invoke-RestMethod -Uri "$api/releases/latest" -Headers $headers -TimeoutSec 20
        }} catch {{
            $status = [int]$_.Exception.Response.StatusCode
            if ($status -eq 404 -or $status -eq 401) {{
                throw 'Нет доступа к релизам GitHub. Для закрытого репозитория настройте SSH_TUNNEL_GITHUB_TOKEN с доступом Contents: read к d1llr/sshvpn.'
            }}
            throw
        }}
        if ($release.draft -or $release.prerelease -or $release.tag_name -cnotmatch '^v(\d+\.\d+\.\d+)$') {{
            throw 'Unexpected GitHub release version.'
        }}
        $version = [version]$Matches[1]
        if ($version -le [version]'{current}') {{ return }}

        $asset = $release.assets | Where-Object {{ $_.name -ceq '{ASSET}' -and $_.state -eq 'uploaded' }} | Select-Object -First 1
        $url = 'https://github.com/d1llr/sshvpn/releases/download/' + $release.tag_name + '/{ASSET}'
        if (!$asset -or $asset.browser_download_url -cne $url -or
            $asset.digest -cnotmatch '^sha256:[0-9a-f]{{64}}$' -or $asset.size -le 0 -or $asset.size -gt 52428800) {{
            throw 'The release is missing a verifiable Windows installer.'
        }}
        $ProgressPreference = 'SilentlyContinue'
        if ($token) {{
            if ([string]$asset.id -cnotmatch '^[1-9][0-9]*$') {{ throw 'Invalid release asset ID.' }}
            $headers.Accept = 'application/octet-stream'
            Invoke-WebRequest -UseBasicParsing -Uri "$api/releases/assets/$($asset.id)" -Headers $headers -OutFile '{path}' -TimeoutSec 180
        }} else {{
            Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile '{path}' -TimeoutSec 180
        }}
        $file = Get-Item -LiteralPath '{path}'
        $hasher = [Security.Cryptography.SHA256]::Create()
        $fileStream = [IO.File]::OpenRead('{path}')
        try {{
            $digest = 'sha256:' + [BitConverter]::ToString($hasher.ComputeHash($fileStream)).Replace('-', '').ToLowerInvariant()
        }} finally {{
            $fileStream.Dispose()
            $hasher.Dispose()
        }}
        if ($file.Length -ne $asset.size -or $digest -cne $asset.digest) {{
            throw 'The installer size or SHA-256 did not match the GitHub release.'
        }}
        [Console]::WriteLine($release.tag_name)
        "#,
    )
}

pub fn launch(installer: PathBuf) -> io::Result<()> {
    Command::new(installer)
        .args(["/SILENT", "/NORESTART", "/RESTARTAPP=1"])
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_skips_old_releases_and_rejects_corrupt_installers() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("installer.exe");
        let mock = r#"
            function Invoke-RestMethod {
                @{
                    tag_name = 'v0.3.0'
                    assets = @(@{
                        name = 'ssh-tunnel-setup-x64.exe'
                        id = 1234
                        state = 'uploaded'
                        browser_download_url = 'https://github.com/d1llr/sshvpn/releases/download/v0.3.0/ssh-tunnel-setup-x64.exe'
                        digest = 'sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824'
                        size = 5
                    })
                }
            }
            function Invoke-WebRequest {
                param([switch]$UseBasicParsing, $Uri, $OutFile, $TimeoutSec, $Headers)
                if ($env:SSH_TUNNEL_GITHUB_TOKEN) {
                    if ($Uri -ne 'https://api.github.com/repos/d1llr/sshvpn/releases/assets/1234' -or
                        $Headers.Authorization -ne 'Bearer test-token' -or
                        $Headers.Accept -ne 'application/octet-stream') { throw 'Invalid private download request.' }
                }
                [IO.File]::WriteAllText($OutFile, 'hello')
            }
        "#;
        let script = |current| {
            format!(
                "$env:SSH_TUNNEL_GITHUB_TOKEN = 'test-token'\n{mock}\n{}",
                download_script(&target, current)
            )
        };
        assert!(crate::vpn::powershell(&script("0.3.0"))
            .unwrap()
            .trim()
            .is_empty());
        assert!(!target.exists());
        assert_eq!(
            crate::vpn::powershell(&script("0.2.0")).unwrap().trim(),
            "v0.3.0"
        );
        let corrupted = script("0.2.0").replace("'hello'", "'jello'");
        assert!(crate::vpn::powershell(&corrupted)
            .unwrap_err()
            .contains("SHA-256"));
    }
}
