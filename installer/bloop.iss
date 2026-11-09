; bloop per-user installer. Build with installer\build.ps1 (passes /DAppVersion).
#ifndef AppVersion
  #error Pass /DAppVersion=x.y.z (installer\build.ps1 does this from Cargo.toml).
#endif

#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
; Never change AppId: upgrades and uninstall find the installed copy by it.
AppId={{85FBA6EE-B3ED-477E-B82D-23CC70284CD0}
AppName=bloop
AppVersion={#AppVersion}
AppVerName=bloop {#AppVersion}
AppPublisher=Laurent Maxhuni
DefaultDirName={localappdata}\Programs\bloop
DefaultGroupName=bloop
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=bloop-setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
WizardImageFile=art\wizard.bmp
WizardSmallImageFile=art\wizard-small.bmp
SetupIconFile=art\bloop.ico
UninstallDisplayIcon={app}\bloop.exe
UninstallDisplayName=bloop
; bloop holds this mutex while running; setup asks the user to close it first.
AppMutex=bloop-app
CloseApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE

[Tasks]
Name: "autostart"; Description: "Start bloop when I sign in"
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\bloop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"

[Icons]
Name: "{userprograms}\bloop"; Filename: "{app}\bloop.exe"
Name: "{userdesktop}\bloop"; Filename: "{app}\bloop.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "bloop"; \
  ValueData: """{app}\bloop.exe"" --background"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\bloop.exe"; Description: "Launch bloop"; Flags: nowait postinstall skipifsilent
; bloop's self-update runs setup silently with /RELAUNCH=1 and quits; this reopens it.
Filename: "{app}\bloop.exe"; Parameters: "--background"; Flags: nowait; Check: Relaunch

[Code]
function Relaunch(): Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;

{ A self-update starts setup and then quits; bloop may still hold its mutex for a moment.
  Silent setup would abort on AppMutex, so wait (up to 15 s) for it to clear first. This
  runs before Setup's own AppMutex check. }
function InitializeSetup(): Boolean;
var
  Waited: Integer;
begin
  Waited := 0;
  if Relaunch() then
    while CheckForMutexes('bloop-app') and (Waited < 150) do
    begin
      Sleep(100);
      Waited := Waited + 1;
    end;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    { The app's own "Start with Windows" switch may have written this even if setup did not. }
    RegDeleteValue(HKEY_CURRENT_USER, '{#RunKey}', 'bloop');
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox('Also delete your bloop chats and settings?', mbConfirmation,
      MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{localappdata}\bloop'), True, True, True);
end;
