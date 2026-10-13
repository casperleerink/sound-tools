; The setup program of Sound Tools for Windows, for Inno Setup 6. tooling/bundle-windows.sh
; compiles it:
;
;   ISCC /DVersion=<version> /DSource=<folder> /DOutput=<folder> tooling\windows\setup.iss
;
; Source holds sound-tools.exe, bun.exe, bun-LICENSE.md and LICENSE. Sound-Tools-Setup-<version>.exe
; goes to Output. It installs for this user, with no administrator, and adds a Start menu entry,
; the sound-tools command on the user PATH and an uninstaller in Settings, Apps. Run again, it
; updates. The app's own update runs it with /VERYSILENT.

[Setup]
; Windows knows the installed app by this. A change makes the next version a second app.
AppId=SoundTools
AppName=Sound Tools
AppVersion={#Version}
AppPublisher=Casper Leerink
UninstallDisplayName=Sound Tools
UninstallDisplayIcon={app}\sound-tools.exe
; The one folder the app's update looks in, so the wizard does not ask for another. The user
; data is in %LOCALAPPDATA%\Sound Tools, which the uninstaller never touches.
DefaultDirName={localappdata}\Programs\Sound Tools
DisableDirPage=yes
; The folder exists when install.ps1 of an older release made it.
DirExistsWarning=no
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
; A running Sound Tools is moved aside, see MoveAside, not closed.
CloseApplications=no
SetupIconFile=..\icon\sound-tools.ico
WizardStyle=modern
OutputDir={#Output}
OutputBaseFilename=Sound-Tools-Setup-{#Version}

[Files]
Source: "{#Source}\sound-tools.exe"; DestDir: "{app}"; Flags: ignoreversion; BeforeInstall: MoveAside
Source: "{#Source}\bun.exe"; DestDir: "{app}"; Flags: ignoreversion; BeforeInstall: MoveAside
Source: "{#Source}\bun-LICENSE.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Sound Tools"; Filename: "{app}\sound-tools.exe"; WorkingDir: "{app}"

[Run]
Filename: "{app}\sound-tools.exe"; Description: "Open Sound Tools"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What an update moved aside and the app did not remove yet.
Type: files; Name: "{app}\*.old"
; The uninstaller removes only a folder its setup made, and install.ps1 may have made this one.
Type: dirifempty; Name: "{app}"

[Code]
var
  MovedAside: array of String;

{ Windows cannot overwrite a running program but can rename it, so a running Sound Tools or Bun
  does not stop the update. The app removes the old ones at its next start. }
procedure MoveAside;
var
  Target: String;
begin
  Target := ExpandConstant(CurrentFileName);
  DeleteFile(Target + '.old');
  if RenameFile(Target, Target + '.old') then
  begin
    SetArrayLength(MovedAside, GetArrayLength(MovedAside) + 1);
    MovedAside[GetArrayLength(MovedAside) - 1] := Target;
  end;
end;

{ An install that stopped halfway leaves the old programs whole. }
procedure DeinitializeSetup;
var
  Index: Integer;
begin
  for Index := 0 to GetArrayLength(MovedAside) - 1 do
    if not FileExists(MovedAside[Index]) then
      RenameFile(MovedAside[Index] + '.old', MovedAside[Index]);
end;

{ Where Folder starts in Path, the folders of the user PATH with a ; between, or 0. }
function PlaceOnPath(Path, Folder: String): Integer;
begin
  Result := Pos(';' + Uppercase(Folder) + ';', ';' + Uppercase(Path) + ';');
end;

{ The sound-tools command for terminals and agents. A new terminal sees it; one open already
  does not. }
procedure AddToPath(Folder: String);
var
  Path: String;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path) then
    Path := '';
  if PlaceOnPath(Path, Folder) > 0 then
    exit;
  if (Path <> '') and (Path[Length(Path)] <> ';') then
    Path := Path + ';';
  RegWriteExpandStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path + Folder);
end;

procedure RemoveFromPath(Folder: String);
var
  Path: String;
  Place: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path) then
    exit;
  Place := PlaceOnPath(Path, Folder);
  if Place = 0 then
    exit;
  { The ; before it goes too, or the one after it when it is first. }
  if Place > 1 then
    Place := Place - 1;
  Delete(Path, Place, Length(Folder) + 1);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Path);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  { Before the files go: after them, Windows was already told of the old PATH. }
  if CurUninstallStep = usUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
