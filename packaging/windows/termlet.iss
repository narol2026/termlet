; Inno Setup 6 script for the per-user Windows installer.
;
; Build it with:
;   packaging\windows\build.ps1
;
; The script is deliberately per-user (PrivilegesRequired=lowest) so installing
; Termlet does not need an administrator prompt. Uninstalling leaves
; ~/.termlet/ and any saved hosts alone.

#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif

[Setup]
AppId={{8E2C1B7A-6D4F-4C3A-9B1E-7F5D2A9C6E10}
AppName=Termlet
AppVersion={#MyAppVersion}
AppVerName=Termlet {#MyAppVersion}
AppPublisher=narol
AppPublisherURL=https://github.com/narol2026/termlet
AppSupportURL=https://github.com/narol2026/termlet/issues
AppUpdatesURL=https://github.com/narol2026/termlet/releases
DefaultDirName={localappdata}\Programs\Termlet
DefaultGroupName=Termlet
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\..\dist
OutputBaseFilename=Termlet-{#MyAppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\app-icons\windows\termlet.ico
UninstallDisplayIcon={app}\termlet.ico
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UsePreviousAppDir=yes
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#MyAppVersion}
VersionInfoProductName=Termlet
VersionInfoProductVersion={#MyAppVersion}
VersionInfoCompany=narol
VersionInfoDescription=Termlet installer

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\termlet.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\release\termlet-bridge.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\app-icons\windows\termlet.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\THIRD_PARTY_LICENSES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\fonts\licenses\*.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Termlet"; Filename: "{app}\termlet.exe"; WorkingDir: "{app}"; IconFilename: "{app}\termlet.ico"
Name: "{autodesktop}\Termlet"; Filename: "{app}\termlet.exe"; WorkingDir: "{app}"; IconFilename: "{app}\termlet.ico"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\termlet.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\termlet.exe"; Flags: uninsdeletekey

[Run]
Filename: "{app}\termlet.exe"; Description: "{cm:LaunchProgram,Termlet}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent
