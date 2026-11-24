#ifndef AppVersion
  #error AppVersion must be provided
#endif
#ifndef PayloadDir
  #error PayloadDir must be provided
#endif

[Setup]
AppId={{A86E5573-6188-483F-8F5F-D647F155DDE9}
AppName=SSH Tunnel
AppVersion={#AppVersion}
VersionInfoVersion={#AppVersion}
AppPublisher=SSH Tunnel
AppPublisherURL=https://github.com/SSHVPN-Connect/sshvpn-client
DefaultDirName={autopf}\SSH Tunnel
DisableDirPage=yes
DisableProgramGroupPage=yes
UsePreviousAppDir=no
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
UninstallDisplayIcon={app}\ssh-tunnel.exe
SetupIconFile=..\assets\ssh-tunnel.ico
ChangesAssociations=yes
OutputDir=..\target\release
OutputBaseFilename=ssh-tunnel-setup-x64
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
CloseApplicationsFilter=ssh-tunnel.exe,wintun.dll,sing-box.exe,libcronet.dll
RestartApplications=no
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[CustomMessages]
english.StopClientError=Could not close SSH Tunnel. Exit it from the system tray and retry.
russian.StopClientError=Не удалось закрыть SSH Tunnel. Выйдите из приложения через значок в трее и повторите установку.

[Files]
Source: "stop-client.ps1"; Flags: dontcopy
Source: "network-service.ps1"; Flags: dontcopy
Source: "network-service.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\ssh-tunnel.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\wintun.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\WINTUN-LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\sing-box.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#PayloadDir}\libcronet.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#PayloadDir}\SING-BOX-LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "sing-box-source.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\fonts\Inter-OFL.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\assets\fonts\IBMPlexMono-OFL.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{commonprograms}\SSH Tunnel"; Filename: "{app}\ssh-tunnel.exe"; WorkingDir: "{app}"; IconFilename: "{app}\ssh-tunnel.exe"; AppUserModelID: "SSHVPNConnect.SSHTunnel"
Name: "{commondesktop}\SSH Tunnel"; Filename: "{app}\ssh-tunnel.exe"; WorkingDir: "{app}"; IconFilename: "{app}\ssh-tunnel.exe"; AppUserModelID: "SSHVPNConnect.SSHTunnel"

[Run]
Filename: "{app}\ssh-tunnel.exe"; Description: "{cm:LaunchProgram,SSH Tunnel}"; Flags: nowait postinstall skipifsilent runasoriginaluser
Filename: "{app}\ssh-tunnel.exe"; Flags: nowait runasoriginaluser; Check: RestartFromUpdate

[UninstallDelete]
Type: files; Name: "{app}\pending-update.exe"
Type: files; Name: "{app}\vpn-service-startup.log"

[Code]
var
  NetworkError: String;

function NetworkService(const Action, ScriptPath: String): Boolean;
var
  Status: Integer;
  ErrorPath: String;
  Details: AnsiString;
begin
  NetworkError := '';
  ErrorPath := ExpandConstant('{tmp}\ssh-tunnel-network-error.txt');
  DeleteFile(ErrorPath);
  Status := 0;
  Result := Exec(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + ScriptPath +
      '" -Action ' + Action + ' -InstallDir "' + ExpandConstant('{app}') +
      '" -ErrorFile "' + ErrorPath + '"',
    ExpandConstant('{sys}'), SW_HIDE, ewWaitUntilTerminated, Status) and (Status = 0);
  if not Result then
  begin
    if LoadStringFromFile(ErrorPath, Details) then
      NetworkError := #13#10 + Trim(Utf8Decode(Details))
    else
      NetworkError := #13#10 + 'Exit code: ' + IntToStr(Status);
    if (Action = 'Install') and
       LoadStringFromFile(ExpandConstant('{app}\vpn-service-startup.log'), Details) then
      NetworkError := NetworkError + #13#10 + Trim(Utf8Decode(Details));
    Log('SSH Tunnel Network ' + Action + NetworkError);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Status: Integer;
begin
  Result := '';
  if CompareText(ExpandConstant('{app}'), ExpandConstant('{autopf}\SSH Tunnel')) <> 0 then
  begin
    Result := 'SSH Tunnel Network requires the default Program Files installation directory.';
    Exit;
  end;
  if not FileExists(ExpandConstant('{app}\ssh-tunnel.exe')) then
    Exit;
  ExtractTemporaryFile('stop-client.ps1');
  if not Exec(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    ExpandConstant('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{tmp}\stop-client.ps1" -InstallDir "{app}"'),
    '', SW_HIDE, ewWaitUntilTerminated, Status) or (Status <> 0) then
  begin
    Result := ExpandConstant('{cm:StopClientError}');
    Exit;
  end;
  ExtractTemporaryFile('network-service.ps1');
  if not NetworkService('Stop', ExpandConstant('{tmp}\network-service.ps1')) then
    Result := 'Could not stop the SSH Tunnel Network service.' + NetworkError;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and
     not NetworkService('Install', ExpandConstant('{app}\network-service.ps1')) then
    RaiseException('Could not start the SSH Tunnel Network service.' + NetworkError);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usUninstall) and
     not NetworkService('Uninstall', ExpandConstant('{app}\network-service.ps1')) then
    RaiseException('Could not remove the SSH Tunnel Network service.' + NetworkError);
end;

function RestartFromUpdate: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:RESTARTAPP|0}') = '1');
end;
