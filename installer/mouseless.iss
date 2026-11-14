; Inno Setup script for mouseless.
; Build it with installer\build.ps1, which builds the release binary first and passes the version.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "mouseless"
#define AppExe "mouseless.exe"

[Setup]
; never change the AppId, it identifies the installation for upgrades and uninstallation
AppId={{5A679F79-1B77-470B-8E7F-F5D520AF9DBB}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=mouseless
AppPublisherURL=https://github.com/jbensmann/mouseless
VersionInfoVersion={#AppVersion}
; installed for all users in Program Files, which only administrators can modify; this matters since
; mouseless can run with administrator rights
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\target\installer
OutputBaseFilename=mouseless-setup-{#AppVersion}
SetupIconFile=..\assets\mouseless.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
LicenseFile=..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; a running mouseless is stopped with "mouseless --quit" (see PrepareToInstall)
CloseApplications=no

[Tasks]
Name: "autostart"; Description: "Start mouseless automatically at login, with administrator rights (so that it also works in admin windows)"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\examples\config_full.yaml"; DestDir: "{app}\examples"; Flags: ignoreversion
Source: "..\examples\config_minimal.yaml"; DestDir: "{app}\examples"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
; the setup runs as administrator, so this creates the scheduled task without another UAC prompt
Filename: "{app}\{#AppExe}"; Parameters: "--enable-autostart"; StatusMsg: "Setting up the autostart..."; Flags: runhidden waituntilterminated; Tasks: autostart
Filename: "{app}\{#AppExe}"; Parameters: "--disable-autostart"; Flags: runhidden waituntilterminated; Tasks: not autostart
; start mouseless now: through the task (with administrator rights), or as the normal user
Filename: "{sys}\schtasks.exe"; Parameters: "/Run /TN mouseless"; Description: "Start mouseless now"; Flags: postinstall runhidden nowait skipifsilent; Tasks: autostart
Filename: "{app}\{#AppExe}"; Description: "Start mouseless now"; Flags: postinstall nowait skipifsilent runasoriginaluser; Tasks: not autostart

[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "--quit"; Flags: runhidden waituntilterminated; RunOnceId: "StopMouseless"
Filename: "{app}\{#AppExe}"; Parameters: "--disable-autostart"; Flags: runhidden waituntilterminated; RunOnceId: "RemoveAutostart"

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  // stop a running mouseless, so that its executable can be replaced
  if FileExists(ExpandConstant('{app}\{#AppExe}')) then
    Exec(ExpandConstant('{app}\{#AppExe}'), '--quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := '';
end;
