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
AppPublisher=SSH Tunnel
AppPublisherURL=https://github.com/d1llr/sshvpn
DefaultDirName={autopf}\SSH Tunnel
DisableDirPage=yes
DisableProgramGroupPage=yes
UsePreviousAppDir=no
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
UninstallDisplayIcon={app}\ssh-tunnel.exe
OutputDir=..\target\release
OutputBaseFilename=ssh-tunnel-setup-x64
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
CloseApplicationsFilter=ssh-tunnel.exe,wintun.dll
RestartApplications=no
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Files]
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
Filename: "{app}\ssh-tunnel.exe"; Description: "{cm:LaunchProgram,SSH Tunnel}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\ssh-tunnel.exe"; Flags: nowait; Check: RestartFromUpdate

[UninstallDelete]
Type: files; Name: "{app}\pending-update.exe"

[Code]
function RestartFromUpdate: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:RESTARTAPP|0}') = '1');
end;
