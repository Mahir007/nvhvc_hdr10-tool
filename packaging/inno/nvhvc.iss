; The per-user installer for nvhvc.exe, for Inno Setup 7. CI compiles it with:
;
;   iscc /DVersion=<version> /DBinDir=<folder with nvhvc.exe> /O<output folder> packaging\inno\nvhvc.iss
;
; BinDir must hold an nvhvc.exe built with NVHVC_WINUI_RUNTIME=framework, which uses the Windows
; App Runtime installed on the system. Setup installs that runtime when it's missing.

#ifndef Version
  #define Version "0.0.0-dev"
#endif
#ifndef BinDir
  #define BinDir "..\..\target\release"
#endif

; Microsoft's installer for the runtime, which Setup downloads when the runtime is missing. Any
; 2.x from 2.4 on runs the app. Change the URL and the hash together.
#define RuntimeUrl "https://aka.ms/windowsappsdk/2.7/2.7.0/windowsappruntimeinstall-x64.exe"
#define RuntimeSha256 "ed1ac8aa9497473389a288017c7fc352c382ba8df6c08f36ddb5815bff283b2a"

[Setup]
AppId={{6C330725-4C4B-4C40-9171-C640EA5213DD}
AppName=HDR10 Patcher
AppVersion={#Version}
AppPublisher=Mahir007
AppPublisherURL=https://github.com/Mahir007/nvhvc_hdr10-tool
AppSupportURL=https://github.com/Mahir007/nvhvc_hdr10-tool/issues
AppUpdatesURL=https://github.com/Mahir007/nvhvc_hdr10-tool/releases
; For the current user only, in %LOCALAPPDATA%\Programs, without asking for admin rights.
PrivilegesRequired=lowest
DefaultDirName={autopf}\nvhvc
DisableProgramGroupPage=yes
; The app needs Windows 11, and the exe is x64.
MinVersion=10.0.22000
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
WizardStyle=modern dynamic
SetupIconFile=..\icons\nvhvc.ico
UninstallDisplayIcon={app}\nvhvc.ico
UninstallDisplayName=HDR10 Patcher
OutputBaseFilename=nvhvc-{#Version}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: addtopath; Description: "Add nvhvc to PATH, to run it in a terminal"; GroupDescription: "Command line:"; Flags: unchecked

[Files]
Source: "{#BinDir}\nvhvc.exe"; DestDir: "{app}"; Flags: ignoreversion
; A fast build's debug info, so its backtraces have names. A release build has none.
Source: "{#BinDir}\nvhvc.pdb"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\icons\nvhvc.ico"; DestDir: "{app}"
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"

[Icons]
Name: "{autoprograms}\HDR10 Patcher"; Filename: "{app}\nvhvc.exe"; Parameters: "gui"; IconFilename: "{app}\nvhvc.ico"
Name: "{autodesktop}\HDR10 Patcher"; Filename: "{app}\nvhvc.exe"; Parameters: "gui"; IconFilename: "{app}\nvhvc.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\nvhvc.exe"; Parameters: "gui"; Description: "{cm:LaunchProgram,HDR10 Patcher}"; Flags: nowait postinstall skipifsilent

[Code]
var
  DownloadPage: TDownloadWizardPage;
  RuntimeError: String;

// Asks nvhvc.exe whether the runtime it needs is installed. Before installing, it runs from
// Setup's temporary folder.
function RuntimeInstalled(): Boolean;
var
  Code: Integer;
begin
  ExtractTemporaryFile('nvhvc.exe');
  Result := Exec(ExpandConstant('{tmp}\nvhvc.exe'), 'check-runtime', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
  if Result then
    Log('The Windows App Runtime is installed.')
  else
    Log('The Windows App Runtime is missing.');
end;

// Downloads Microsoft's runtime installer, runs it and checks again. Without admin rights it
// installs the runtime for the current user.
function InstallRuntime(): Boolean;
var
  Code: Integer;
begin
  Result := False;
  DownloadPage.Clear;
  DownloadPage.Add('{#RuntimeUrl}', 'windowsappruntimeinstall-x64.exe', '{#RuntimeSha256}');
  DownloadPage.Show;
  try
    try
      DownloadPage.Download;
      DownloadPage.SetText('Installing the Windows App Runtime...', '');
      Exec(ExpandConstant('{tmp}\windowsappruntimeinstall-x64.exe'), '--quiet', '', SW_HIDE, ewWaitUntilTerminated, Code);
      Log(Format('The runtime installer exited with code 0x%.8x.', [Code]));
      Result := RuntimeInstalled();
    except
      if DownloadPage.AbortedByUser then
        RuntimeError := 'The download was cancelled.'
      else
        RuntimeError := AddPeriod(GetExceptionMessage);
      Log(RuntimeError);
    end;
  finally
    DownloadPage.Hide;
  end;
end;

procedure InitializeWizard();
begin
  DownloadPage := CreateDownloadPage('Windows App Runtime',
    'HDR10 Patcher needs Microsoft''s Windows App Runtime 2.4 or later. Setup downloads it (about 125 MB) and installs it.', nil);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = wpReady) and not RuntimeInstalled() then
  begin
    RuntimeError := '';
    Result := InstallRuntime();
    if not Result then
      SuppressibleMsgBox('HDR10 Patcher needs the Windows App Runtime 2.4 or later, and Setup couldn''t install it. ' + RuntimeError + #13#10#13#10 +
        'Install it with "winget install Microsoft.WindowsAppRuntime.2", then run Setup again.', mbCriticalError, MB_OK, IDOK);
  end;
end;

// Path with every entry that names Dir left out, ignoring case and a trailing backslash. The
// other entries stay as they are, empty ones included.
function PathWithout(Path, Dir: String): String;
var
  Rest, Entry: String;
  P: Integer;
  First: Boolean;
begin
  Result := '';
  First := True;
  Rest := Path + ';';
  while Rest <> '' do
  begin
    P := Pos(';', Rest);
    Entry := Copy(Rest, 1, P - 1);
    Rest := Copy(Rest, P + 1, Length(Rest));
    if (Trim(Entry) = '') or (CompareText(RemoveBackslashUnlessRoot(Trim(Entry)), RemoveBackslashUnlessRoot(Dir)) <> 0) then
    begin
      if not First then
        Result := Result + ';';
      Result := Result + Entry;
      First := False;
    end;
  end;
end;

// Adds the app's folder to the user's PATH, or takes it off. Only that entry changes.
procedure SetUserPath(Add: Boolean);
var
  Path, NewPath, Dir: String;
begin
  Dir := ExpandConstant('{app}');
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
    Path := '';
  NewPath := PathWithout(Path, Dir);
  if Add then
  begin
    if (NewPath <> '') and (NewPath[Length(NewPath)] <> ';') then
      NewPath := NewPath + ';';
    NewPath := NewPath + Dir;
  end;
  if NewPath = Path then
    Exit;
  if NewPath = '' then
    RegDeleteValue(HKCU, 'Environment', 'Path')
  else
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', NewPath);
  Log('Set the user PATH to: ' + NewPath);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SetUserPath(WizardIsTaskSelected('addtopath'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    SetUserPath(False);
end;
