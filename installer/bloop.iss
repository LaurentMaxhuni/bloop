; bloop per-user installer. Build with installer\build.ps1 (passes /DAppVersion).
#ifndef AppVersion
  #error Pass /DAppVersion=x.y.z (installer\build.ps1 does this from Cargo.toml).
#endif

#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

; /DFixture builds a separate test app (examples/update_dry_run.rs): its own AppId, name,
; mutex, folder, shortcuts and Run value, so testing self-update never touches a real bloop.
#ifdef Fixture
  #define AppGuid "{{3C1F6A52-7E0B-4D4B-9B57-0F4C2A9E1D11}"
  #define AppTitle "bloop-update-fixture"
  #define AppMutexName "bloop-update-fixture"
#else
  ; Never change AppId: upgrades and uninstall find the installed copy by it.
  #define AppGuid "{{85FBA6EE-B3ED-477E-B82D-23CC70284CD0}"
  #define AppTitle "bloop"
  #define AppMutexName "bloop-app"
#endif
#ifndef AppExe
  #define AppExe "..\target\release\bloop.exe"
#endif
#ifndef OutDir
  #define OutDir "..\dist"
#endif

[Setup]
AppId={#AppGuid}
AppName={#AppTitle}
AppVersion={#AppVersion}
AppVerName={#AppTitle} {#AppVersion}
AppPublisher=Laurent Maxhuni
DefaultDirName={localappdata}\Programs\{#AppTitle}
DefaultGroupName={#AppTitle}
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
OutputDir={#OutDir}
OutputBaseFilename=bloop-setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
WizardImageFile=art\wizard.bmp
WizardSmallImageFile=art\wizard-small.bmp
SetupIconFile=art\bloop.ico
UninstallDisplayIcon={app}\bloop.exe
UninstallDisplayName={#AppTitle}
; bloop holds this mutex while running; setup asks the user to close it first.
AppMutex={#AppMutexName}
CloseApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE

[Tasks]
Name: "autostart"; Description: "Start bloop when I sign in"
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"

[Icons]
Name: "{userprograms}\{#AppTitle}"; Filename: "{app}\bloop.exe"
Name: "{userdesktop}\{#AppTitle}"; Filename: "{app}\bloop.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "{#AppTitle}"; \
  ValueData: """{app}\bloop.exe"" --background"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\bloop.exe"; Description: "Launch bloop"; Flags: nowait postinstall skipifsilent
; bloop's self-update runs setup silently with /RELAUNCH=1 and quits; this reopens it.
Filename: "{app}\bloop.exe"; Parameters: "--background"; Flags: nowait; Check: Relaunch

[Code]
var
  Installed: Boolean;

function Relaunch(): Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;

{ A self-update starts setup and then quits; bloop may still hold its mutex for a moment.
  Silent setup would abort on AppMutex, so wait (up to 30 s) for it to clear first. This
  runs before Setup's own AppMutex check. }
function InitializeSetup(): Boolean;
var
  Waited: Integer;
begin
  Waited := 0;
  if Relaunch() then
    while CheckForMutexes('{#AppMutexName}') and (Waited < 300) do
    begin
      Sleep(100);
      Waited := Waited + 1;
    end;
  Result := True;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    Installed := True;
end;

{ bloop has already quit when a self-update runs. If setup stops before installing (a file
  in use, a full disk), reopen the copy that is still there so bloop doesn't just vanish.
  When the [Run] entry above already reopened it, or bloop never quit, the mutex is held. }
procedure DeinitializeSetup();
var
  App: String;
  Code: Integer;
begin
  if not Relaunch() or Installed or CheckForMutexes('{#AppMutexName}') then
    exit;
  try
    App := ExpandConstant('{app}');
  except
    App := '';
  end;
  if (App <> '') and FileExists(App + '\bloop.exe') then
    Exec(App + '\bloop.exe', '--background', '', SW_SHOWNORMAL, ewNoWait, Code);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    { The app's own "Start with Windows" switch may have written this even if setup did not. }
    RegDeleteValue(HKEY_CURRENT_USER, '{#RunKey}', '{#AppTitle}');
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox('Also delete your bloop chats and settings?', mbConfirmation,
      MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{localappdata}\{#AppTitle}'), True, True, True);
end;
