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
Name: "{commonprograms}\SSH Tunnel"; Filename: "{app}\ssh-tunnel.exe"; WorkingDir: "{app}"
Name: "{commondesktop}\SSH Tunnel"; Filename: "{app}\ssh-tunnel.exe"; WorkingDir: "{app}"

[Run]
Filename: "{app}\ssh-tunnel.exe"; Description: "{cm:LaunchProgram,SSH Tunnel}"; Flags: nowait postinstall skipifsilent runasoriginaluser
Filename: "{app}\ssh-tunnel.exe"; Flags: nowait runasoriginaluser; Check: RestartFromUpdate

[UninstallDelete]
Type: files; Name: "{app}\pending-update.exe"

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Status: Integer;
begin
  Result := '';
  if not FileExists(ExpandConstant('{app}\ssh-tunnel.exe')) then
    Exit;
  ExtractTemporaryFile('stop-client.ps1');
  if not Exec(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    ExpandConstant('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{tmp}\stop-client.ps1" -InstallDir "{app}"'),
    '', SW_HIDE, ewWaitUntilTerminated, Status) or (Status <> 0) then
    Result := ExpandConstant('{cm:StopClientError}');
end;

function RestartFromUpdate: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:RESTARTAPP|0}') = '1');
end;
