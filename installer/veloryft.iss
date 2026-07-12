; ============================================================================
;  Veloryft — installer (Inno Setup 6)
;  Compile:  ISCC.exe installer\veloryft.iss
;  Output:   dist\Veloryft-Setup-<version>-beta.exe
; ============================================================================

#define MyAppName "Veloryft"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "Veloryft"
#define MyAppURL "https://veloryft.com"
#define MyAppExeName "Veloryft.exe"
#define MyBuildExe "..\src-tauri\target\release\forza-hud-native.exe"
; Stable product GUID (do NOT change between versions — it's the identity used
; for updates/uninstall).
#define MyAppId "{{A7F3C2E1-9B4D-4E6A-8C1F-2D5B7E0A9C34}"

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
; Kept identical to AppName on purpose: the product is named just "Veloryft",
; nothing appended (no "HUD" suffix, no beta tag baked into the display name).
AppVerName={#MyAppName}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
AppCopyright=Copyright (C) 2026 Veloryft
VersionInfoVersion={#MyAppVersion}
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Setup (Beta)

; Per-user install (no UAC); the user can still opt for "all users" in the
; privileges dialog.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
AllowNoIcons=yes

; 64-bit Windows 10/11.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

; Native Windows light/dark mode. The installer reads the user's system
; appearance once at startup and keeps its title bar, controls, and pages in sync.
WizardStyle=modern dynamic windows11 hidebevels
WizardSizePercent=120
DefaultDialogFontName=Segoe UI
WizardImageFile=assets\wizard-large-light.bmp,assets\wizard-large-light@2x.bmp
WizardImageFileDynamicDark=assets\wizard-large.bmp,assets\wizard-large@2x.bmp
WizardSmallImageFile=assets\wizard-small-light.bmp,assets\wizard-small-light@2x.bmp
WizardSmallImageFileDynamicDark=assets\wizard-small.bmp,assets\wizard-small@2x.bmp
SetupIconFile=assets\veloryft.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

; License terms (mandatory acceptance) + post-install quick-start guide.
LicenseFile=EULA.txt
InfoAfterFile=README.txt

; Packaging.
Compression=lzma2/max
SolidCompression=yes
OutputDir=..\dist
OutputBaseFilename=Veloryft-Setup-{#MyAppVersion}-beta
ShowLanguageDialog=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#MyBuildExe}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{app}"; DestName: "Read Me.txt"; Flags: ignoreversion isreadme
Source: "EULA.txt"; DestDir: "{app}"; DestName: "License Agreement.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Read Me"; Filename: "{app}\Read Me.txt"
Name: "{group}\License Agreement"; Filename: "{app}\License Agreement.txt"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "{code:ActivationParameters}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
{ Closes Veloryft before copying or removing its executable. }

var
  ActivationPage: TInputQueryWizardPage;

procedure InitializeWizard;
begin
  ActivationPage := CreateInputQueryPage(
    wpSelectDir,
    'Activation',
    'Activate a license or start a trial',
    'Enter an optional Veloryft license key. Leave it blank to start the trial.'
  );
  ActivationPage.Add('License key:', False);
end;

function IsLicenseKey(const Key: String): Boolean;
var
  I: Integer;
  Alphabet: String;
begin
  Result := False;
  if (Length(Key) <> 24) or (Copy(Key, 1, 5) <> 'VLRY-') then Exit;
  Alphabet := 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
  for I := 1 to Length(Key) do begin
    if (I = 5) or (I = 10) or (I = 15) or (I = 20) then begin
      if Key[I] <> '-' then Exit;
    end else if Pos(Key[I], Alphabet) = 0 then begin
      Exit;
    end;
  end;
  Result := True;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Key: String;
begin
  Result := True;
  if CurPageID = ActivationPage.ID then begin
    Key := Uppercase(Trim(ActivationPage.Values[0]));
    ActivationPage.Values[0] := Key;
    if (Key <> '') and (not IsLicenseKey(Key)) then begin
      MsgBox('Enter a valid key in the format VLRY-XXXX-XXXX-XXXX-XXXX, or leave the field blank for a trial.', mbError, MB_OK);
      Result := False;
    end;
  end;
end;

function ActivationParameters(Param: String): String;
var
  Key: String;
begin
  Key := Trim(ActivationPage.Values[0]);
  if Key = '' then
    Result := ''
  else
    Result := '--activate "' + Key + '"';
end;

procedure KillRunningApp;
var
  rc: Integer;
begin
  Exec(ExpandConstant('{cmd}'), '/C taskkill /IM "{#MyAppExeName}" /F', '',
       SW_HIDE, ewWaitUntilTerminated, rc);
  Exec(ExpandConstant('{cmd}'), '/C taskkill /IM "forza-hud-native.exe" /F', '',
       SW_HIDE, ewWaitUntilTerminated, rc);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  KillRunningApp;
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  KillRunningApp;
  Result := True;
end;
