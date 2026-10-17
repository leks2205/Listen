; Listen: per-user Windows installer (Inno Setup 6.3+).
; Built by packaging/windows/build-installer.sh:
;   ISCC /DAppVersion=0.29.0 /DDistDir=<assembled folder> /DOutputDir=<out> listen.iss
; No administrator prompt: everything goes to %LOCALAPPDATA%\Programs\Listen and the
; current user's Start menu / desktop.

#ifndef AppVersion
  #error Pass /DAppVersion=x.y.z (build-installer.sh takes it from Cargo.toml)
#endif
#ifndef DistDir
  #error Pass /DDistDir=<folder with listen.exe, LICENSE, NOTICE.md, THIRD_PARTY_LICENSES.html>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
; Never change: it identifies the install for upgrades and the uninstaller.
AppId={{5F3748DD-EEEE-4FEE-A85B-6D6DC189DF28}
AppName=Listen
AppVersion={#AppVersion}
AppVerName=Listen {#AppVersion}
AppPublisher=Listen contributors
AppPublisherURL=https://github.com/leks2205/Listen
DefaultDirName={localappdata}\Programs\Listen
DefaultGroupName=Listen
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=Listen-Setup-{#AppVersion}
SetupIconFile=listen.ico
UninstallDisplayIcon={app}\listen.exe
UninstallDisplayName=Listen
LicenseFile={#DistDir}\LICENSE
Compression=lzma2/normal
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Listen
VersionInfoDescription=Listen setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#DistDir}\listen.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\NOTICE.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DistDir}\THIRD_PARTY_LICENSES.html"; DestDir: "{app}"; Flags: ignoreversion
Source: "listen.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Listen"; Filename: "{app}\listen.exe"; IconFilename: "{app}\listen.ico"
Name: "{autodesktop}\Listen"; Filename: "{app}\listen.exe"; IconFilename: "{app}\listen.ico"; Tasks: desktopicon

[Run]
; Wizard install: offer to start Listen. Silent update started by Listen itself (/RELAUNCH): start it again.
Filename: "{app}\listen.exe"; Description: "{cm:LaunchProgram,Listen}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\listen.exe"; Flags: nowait; Check: ShouldRelaunch

[UninstallDelete]
; Removes the whole install folder, including mpv, yt-dlp and Deno placed next to the program.
Type: filesandordirs; Name: "{app}"

[Code]
function ShouldRelaunch: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/RELAUNCH') = 0 then
      Result := True;
end;
