use std::{env, io, path::PathBuf, process::Command};

const ASSET: &str = "ssh-tunnel-setup-x64.exe";

pub fn check() -> Result<Option<String>, String> {
    let result = crate::vpn::powershell(&update_script(env!("CARGO_PKG_VERSION"), None))
        .map_err(|error| format!("Не удалось проверить обновления: {error}"))?;
    let version = result.trim();
    Ok((!version.is_empty()).then(|| version.to_owned()))
}

pub fn prepare(version: &str) -> Result<PathBuf, String> {
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

    let update_dir = crate::Profiles::directory().map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&update_dir).map_err(|error| error.to_string())?;
    let staging = tempfile::NamedTempFile::new_in(&update_dir)
        .map_err(|error| format!("Не удалось подготовить обновление: {error}"))?;
    let temporary = staging.into_temp_path();
    let result = crate::vpn::powershell(&update_script(
        env!("CARGO_PKG_VERSION"),
        Some((&temporary, version)),
    ))
    .map_err(|error| format!("Не удалось обновить приложение: {error}"))?;
    if result.trim() != version {
        return Err("Версия релиза изменилась. Повторите проверку обновлений.".into());
    }
    let pending = update_dir.join("pending-update.exe");
    temporary
        .persist(&pending)
        .map_err(|error| format!("Не удалось сохранить установщик: {error}"))?;
    Ok(pending)
}

fn update_script(current: &str, download: Option<(&std::path::Path, &str)>) -> String {
    let mut script = format!(
        r#"
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $headers = @{{ 'User-Agent' = 'SSH-Tunnel'; Accept = 'application/vnd.github+json' }}
        $api = 'https://api.github.com/repos/SSHVPN-Connect/sshvpn-client'
        $token = $env:SSH_TUNNEL_GITHUB_TOKEN
        $authenticated = $false
        try {{
            $release = Invoke-RestMethod -Uri "$api/releases/latest" -Headers $headers -TimeoutSec 20
        }} catch {{
            $status = [int]$_.Exception.Response.StatusCode
            if ($status -in @(403, 404, 429) -and $token -and $token.Trim()) {{
                $headers.Authorization = 'Bearer ' + $token.Trim()
                try {{
                    $release = Invoke-RestMethod -Uri "$api/releases/latest" -Headers $headers -TimeoutSec 20
                    $authenticated = $true
                }} catch {{
                    $status = [int]$_.Exception.Response.StatusCode
                    if ($status -in @(401, 403, 404)) {{
                        throw "GitHub отклонил проверку обновлений (HTTP $status). Проверьте доступ Contents: read к SSHVPN-Connect/sshvpn-client для SSH_TUNNEL_GITHUB_TOKEN."
                    }}
                    throw
                }}
            }} elseif ($status -eq 404 -or $status -eq 401) {{
                throw 'Нет доступа к релизам GitHub. Для закрытого репозитория настройте SSH_TUNNEL_GITHUB_TOKEN с доступом Contents: read к SSHVPN-Connect/sshvpn-client.'
            }} elseif ($status -in @(403, 429)) {{
                throw "GitHub отклонил проверку обновлений (HTTP $status). Повторите попытку позже или скачайте установщик со страницы релиза."
            }} else {{
                throw
            }}
        }}
        if ($release.draft -or $release.prerelease -or $release.tag_name -cnotmatch '^v(\d+\.\d+\.\d+)$') {{
            throw 'Unexpected GitHub release version.'
        }}
        $version = [version]$Matches[1]
        if ($version -le [version]'{current}') {{ return }}

        $asset = $release.assets | Where-Object {{ $_.name -ceq '{ASSET}' -and $_.state -eq 'uploaded' }} | Select-Object -First 1
        $url = 'https://github.com/SSHVPN-Connect/sshvpn-client/releases/download/' + $release.tag_name + '/{ASSET}'
        if (!$asset -or $asset.browser_download_url -cne $url -or
            $asset.digest -cnotmatch '^sha256:[0-9a-f]{{64}}$' -or $asset.size -le 0 -or $asset.size -gt 52428800) {{
            throw 'The release is missing a verifiable Windows installer.'
        }}
        "#,
    );
    let Some((path, version)) = download else {
        script.push_str("[Console]::WriteLine($release.tag_name)\n");
        return script;
    };
    let path = path.to_string_lossy().replace('\'', "''");
    let version = version.replace('\'', "''");
    script.push_str(&format!(
        r#"
        if ($release.tag_name -cne '{version}') {{
            throw 'Версия релиза изменилась. Повторите проверку обновлений.'
        }}
        $ProgressPreference = 'SilentlyContinue'
        try {{
            if ($authenticated) {{
                if ([string]$asset.id -cnotmatch '^[1-9][0-9]*$') {{ throw 'Invalid release asset ID.' }}
                $headers.Accept = 'application/octet-stream'
                Invoke-WebRequest -UseBasicParsing -Uri "$api/releases/assets/$($asset.id)" -Headers $headers -OutFile '{path}' -TimeoutSec 180
            }} else {{
                Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile '{path}' -TimeoutSec 180
            }}
        }} catch {{
            $status = [int]$_.Exception.Response.StatusCode
            if ($status -in @(401, 403, 404, 429)) {{
                throw "GitHub отклонил скачивание установщика (HTTP $status). Скачайте его со страницы релиза."
            }}
            throw
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
    ));
    script
}

pub fn launch(installer: PathBuf) -> io::Result<()> {
    let result = Command::new(&installer)
        .args(["/SILENT", "/NORESTART", "/RESTARTAPP=1"])
        .spawn();
    match result {
        Ok(_) => Ok(()),
        Err(error) if error.raw_os_error() == Some(740) => {
            crate::elevation::run_as_admin(&installer, "/SILENT /NORESTART /RESTARTAPP=1")
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_updates_ignore_configured_token_and_validate_installer() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("installer.exe");
        let mock = r#"
            function Invoke-RestMethod {
                param($Uri, $Headers, $TimeoutSec)
                if ($Headers.Authorization) { throw 'Public request included a token.' }
                @{
                    tag_name = 'v0.3.0'
                    assets = @(@{
                        name = 'ssh-tunnel-setup-x64.exe'
                        id = 1234
                        state = 'uploaded'
                        browser_download_url = 'https://github.com/SSHVPN-Connect/sshvpn-client/releases/download/v0.3.0/ssh-tunnel-setup-x64.exe'
                        digest = 'sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824'
                        size = 5
                    })
                }
            }
            function Invoke-WebRequest {
                param([switch]$UseBasicParsing, $Uri, $OutFile, $TimeoutSec, $Headers)
                if ($Uri -ne 'https://github.com/SSHVPN-Connect/sshvpn-client/releases/download/v0.3.0/ssh-tunnel-setup-x64.exe' -or
                    $Headers.Authorization) { throw 'Invalid public download request.' }
                [IO.File]::WriteAllText($OutFile, 'hello')
            }
        "#;
        let script = |current, download| {
            format!(
                "$env:SSH_TUNNEL_GITHUB_TOKEN = 'test-token'\n{mock}\n{}",
                update_script(current, download)
            )
        };
        assert!(crate::vpn::powershell(&script("0.3.0", None))
            .unwrap()
            .trim()
            .is_empty());
        assert!(!target.exists());
        let previous_release = script("0.4.20", None).replace("v0.3.0", "v0.4.15");
        assert!(crate::vpn::powershell(&previous_release).unwrap().trim().is_empty());
        let future_release = script("0.4.20", None).replace("v0.3.0", "v0.4.21");
        assert_eq!(crate::vpn::powershell(&future_release).unwrap().trim(), "v0.4.21");
        assert_eq!(
            crate::vpn::powershell(&script("0.2.0", None)).unwrap().trim(),
            "v0.3.0"
        );
        assert!(!target.exists());
        assert!(crate::vpn::powershell(&script("0.2.0", Some((&target, "v0.2.9"))))
            .unwrap_err()
            .contains("Версия релиза изменилась"));
        assert!(!target.exists());
        let download = script("0.2.0", Some((&target, "v0.3.0")));
        assert_eq!(crate::vpn::powershell(&download).unwrap().trim(), "v0.3.0");
        let corrupted = download.replace("'hello'", "'jello'");
        assert!(crate::vpn::powershell(&corrupted)
            .unwrap_err()
            .contains("SHA-256"));
    }
}
