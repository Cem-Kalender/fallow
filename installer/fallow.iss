; 014 (research R-11, T024): Fallow's installer for Windows x64, for the current user only,
; without elevation (constitution VI). Built in CI with Inno Setup 6:
;   iscc /DAppVersion=<version> installer\fallow.iss
; from the release binaries in target\release. Unsigned (015 decides signing).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F1C5A2E-8B44-4F0D-9C71-2D3B8E5A9F14}
AppName=Fallow
AppVersion={#AppVersion}
AppPublisher=Fallow
AppVerName=Fallow {#AppVersion}
DefaultDirName={localappdata}\Programs\Fallow
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\installer
OutputBaseFilename=Fallow-{#AppVersion}-x64-setup
SetupIconFile=..\assets\icon\fallow.ico
UninstallDisplayIcon={app}\fallow-app.exe
UninstallDisplayName=Fallow
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
; Restart Manager as a second guard against files in use (Fallow quits first, below).
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes

[Tasks]
Name: "addtopath"; Description: "Add Fallow to your PATH, for the `fallow` command"; Flags: unchecked

[Files]
Source: "..\target\release\fallow.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\fallow-app.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; The Start menu's entry, with Fallow's identity (008), so its notifications and the taskbar group
; under Fallow.
Name: "{userprograms}\Fallow"; Filename: "{app}\fallow-app.exe"; AppUserModelID: "Fallow"

[Run]
Filename: "{app}\fallow-app.exe"; Description: "Launch Fallow"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvKey = 'Environment';

{ Quits a running Fallow the way its icon does, asking first when work is running (FR-009,
  FR-021): exit 0 quit (or not running), 1 cancelled, 2 an error. }
function QuitFallow(Exe: String): Integer;
var
  Code: Integer;
begin
  Result := 0;
  if not FileExists(Exe) then
    Exit;
  if not Exec(Exe, 'quit --installer', '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Result := 2
  else
    Result := Code;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Result := '';
  Code := QuitFallow(ExpandConstant('{app}\fallow.exe'));
  if Code = 1 then
    Result := 'Fallow is still running: Quit was cancelled. Install again when its work can pause.'
  else if Code <> 0 then
    Result := 'Fallow could not be quit. Quit it from its icon, then install again.';
end;

function InitializeUninstall(): Boolean;
var
  Code: Integer;
begin
  Code := QuitFallow(ExpandConstant('{app}\fallow.exe'));
  Result := Code = 0;
  if Code = 1 then
    SuppressibleMsgBox('Fallow is still running: Quit was cancelled. Uninstall again when its work can pause.', mbInformation, MB_OK, IDOK)
  else if Code <> 0 then
    SuppressibleMsgBox('Fallow could not be quit. Quit it from its icon, then uninstall again.', mbError, MB_OK, IDOK);
end;

{ The user's PATH, with Fallow's folder added or removed. }
procedure SetPath(Add: Boolean);
var
  Path, Dir: String;
  P: Integer;
begin
  Dir := ExpandConstant('{app}');
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path) then
    Path := '';
  P := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';');
  if Add and (P = 0) then
  begin
    if (Path <> '') and (Copy(Path, Length(Path), 1) <> ';') then
      Path := Path + ';';
    RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path + Dir);
  end
  else if (not Add) and (P > 0) then
  begin
    { Removed whatever its letter case: P is where it starts in ';' + Path + ';'. }
    Path := ';' + Path + ';';
    Delete(Path, P, Length(Dir) + 1);
    Path := Copy(Path, 2, Length(Path) - 2);
    RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Path);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    SetPath(True);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data, Config: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    SetPath(False);
    { Fallow's notification identity (008). }
    RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\AppUserModelId\Fallow');
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    Data := ExpandConstant('{localappdata}\fallow');
    Config := ExpandConstant('{userappdata}\fallow');
    { Kept unless the engineer chooses (FR-020); projects are never touched. }
    if DirExists(Data) and (SuppressibleMsgBox(
      'Also remove Fallow''s data folder?' + #13#10#13#10 + Data + #13#10#13#10 +
      'It holds the runs, the sessions and the working copies of loops and sessions not merged yet. ' +
      'Your projects are not touched either way.' + #13#10#13#10 +
      'Your settings and agent definitions stay in ' + Config + '.',
      mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES) then
      DelTree(Data, True, True, True);
  end;
end;
