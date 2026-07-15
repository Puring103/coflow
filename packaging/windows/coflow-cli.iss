; Inno Setup script for the Windows CLI-only package.

#define AppName "Coflow CLI"
#define AppPublisher "Coflow"
#define AppUrl "https://github.com/Puring103/coflow"

#ifndef AppVersion
#define AppVersion "0.0.0-dev"
#endif

#ifndef SourceDir
#define SourceDir "..\..\dist\windows-installer\input"
#endif

#ifndef OutputDir
#define OutputDir "..\..\dist\windows-installer\output"
#endif

#ifndef IconPath
#define IconPath "..\..\editors\cfd-editor\src-tauri\icons\icon.ico"
#endif

#ifndef LicensePath
#define LicensePath "..\..\LICENSE"
#endif

[Setup]
AppId={{BCC224F8-C097-430C-9071-891235F401F0}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}
AppUpdatesURL={#AppUrl}
DefaultDirName={localappdata}\Programs\Coflow CLI
DefaultGroupName=Coflow
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=coflow-cli-windows-x64-setup
SetupIconFile={#IconPath}
UninstallDisplayIcon={app}\bin\coflow.exe
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
MinVersion=10.0
WizardStyle=modern
ChangesEnvironment=yes

[Files]
Source: "{#SourceDir}\coflow.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#LicensePath}"; DestDir: "{app}"; Flags: ignoreversion

[Run]
Filename: "{app}\bin\coflow.exe"; Parameters: "skill install -g"; StatusMsg: "Installing Coflow agent skills..."; Flags: runhidden waituntilterminated

[UninstallRun]
Filename: "{app}\bin\coflow.exe"; Parameters: "skill uninstall -g"; RunOnceId: "UninstallCoflowSkills"; Flags: runhidden waituntilterminated

[Code]
const
  EnvironmentKey = 'Environment';
  PathName = 'Path';
  FullProductUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Coflow Tools';
  LegacyFullProductUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{EC88990D-CC9E-4C34-8CA5-04AA3517E5A7}_is1';

function InitializeSetup(): Boolean;
var
  FullInstallLocation: string;
begin
  Result := True;
  if RegQueryStringValue(
    HKEY_CURRENT_USER,
    FullProductUninstallKey,
    'InstallLocation',
    FullInstallLocation
  ) then
  begin
    SuppressibleMsgBox(
      'Coflow Tools is already installed. It includes the CLI, so the CLI-only package is not needed.',
      mbInformation,
      MB_OK,
      IDOK
    );
    Result := False;
    Exit;
  end;

  if RegQueryStringValue(
    HKEY_CURRENT_USER,
    LegacyFullProductUninstallKey,
    'UninstallString',
    FullInstallLocation
  ) then
  begin
    SuppressibleMsgBox(
      'A legacy Coflow Tools installation is present and already includes the CLI. Install the new full package to migrate it.',
      mbInformation,
      MB_OK,
      IDOK
    );
    Result := False;
  end;
end;

function NormalizePath(Value: string): string;
begin
  Result := Lowercase(Trim(Value));

  if (Length(Result) >= 2) and (Copy(Result, 1, 1) = '"') and
     (Copy(Result, Length(Result), 1) = '"') then
  begin
    Result := Copy(Result, 2, Length(Result) - 2);
  end;

  while (Length(Result) > 3) and (Copy(Result, Length(Result), 1) = '\') do
  begin
    Delete(Result, Length(Result), 1);
  end;
end;

procedure SplitPathEntries(Existing: string; var Parts: TArrayOfString);
var
  Index: Integer;
  Separator: Integer;
begin
  SetArrayLength(Parts, 0);

  repeat
    Index := GetArrayLength(Parts);
    SetArrayLength(Parts, Index + 1);

    Separator := Pos(';', Existing);
    if Separator > 0 then
    begin
      Parts[Index] := Copy(Existing, 1, Separator - 1);
      Existing := Copy(Existing, Separator + 1, Length(Existing));
    end
      else
    begin
      Parts[Index] := Existing;
      Existing := '';
    end;
  until Existing = '';
end;

function PathContains(Existing: string; Entry: string): Boolean;
var
  Parts: TArrayOfString;
  I: Integer;
begin
  Result := False;
  SplitPathEntries(Existing, Parts);

  for I := 0 to GetArrayLength(Parts) - 1 do
  begin
    if NormalizePath(Parts[I]) = NormalizePath(Entry) then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

function RemovePathEntry(Existing: string; Entry: string): string;
var
  Parts: TArrayOfString;
  I: Integer;
  Part: string;
begin
  Result := '';
  SplitPathEntries(Existing, Parts);

  for I := 0 to GetArrayLength(Parts) - 1 do
  begin
    Part := Trim(Parts[I]);
    if (Part <> '') and (NormalizePath(Part) <> NormalizePath(Entry)) then
    begin
      if Result <> '' then
      begin
        Result := Result + ';';
      end;
      Result := Result + Part;
    end;
  end;
end;

procedure AddToUserPath(Entry: string);
var
  Existing: string;
  NewValue: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, PathName, Existing) then
  begin
    Existing := '';
  end;

  if PathContains(Existing, Entry) then
  begin
    Exit;
  end;

  if Existing = '' then
  begin
    NewValue := Entry;
  end
    else if Copy(Existing, Length(Existing), 1) = ';' then
  begin
    NewValue := Existing + Entry;
  end
    else
  begin
    NewValue := Existing + ';' + Entry;
  end;

  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, PathName, NewValue);
end;

procedure RemoveFromUserPath(Entry: string);
var
  Existing: string;
  NewValue: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, PathName, Existing) then
  begin
    Exit;
  end;

  NewValue := RemovePathEntry(Existing, Entry);

  if NewValue <> Existing then
  begin
    RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, PathName, NewValue);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    AddToUserPath(ExpandConstant('{app}\bin'));
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RemoveFromUserPath(ExpandConstant('{app}\bin'));
  end;
end;
