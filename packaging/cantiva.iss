; Cantiva 安裝程式（Inno Setup 6.5 以上）。
; 不要直接編譯這個檔案：scripts\package.ps1 會先組好 StageDir，再帶入版本號呼叫 ISCC。
;
;   ISCC /DAppVersion=0.2.0 /DStageDir=<資料夾> /DOutputDir=<資料夾> packaging\cantiva.iss
;
; 安裝到使用者自己的 %LOCALAPPDATA%\Programs\Cantiva，不需要系統管理員權限。
; 解除安裝只移除程式檔；%USERPROFILE%\Cantiva 裡的設定、引擎、聲音與音檔都會保留。

#ifndef AppVersion
  #error 請用 scripts\package.ps1 建置（缺少 /DAppVersion）
#endif
#ifndef StageDir
  #error 請用 scripts\package.ps1 建置（缺少 /DStageDir）
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
AppId={{6E0B7C1A-4F2D-4C8E-9B57-2A3D9C1F5E84}
AppName=Cantiva
AppVersion={#AppVersion}
AppVerName=Cantiva {#AppVersion}
AppPublisher=Naritaroad
AppPublisherURL=https://github.com/naritaroad/cantiva
AppSupportURL=https://github.com/naritaroad/cantiva/issues
AppUpdatesURL=https://github.com/naritaroad/cantiva/releases
AppCopyright=Copyright (c) Naritaroad
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Cantiva 安裝程式
DefaultDirName={localappdata}\Programs\Cantiva
DisableProgramGroupPage=yes
DisableWelcomePage=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=Cantiva-{#AppVersion}-windows-x64-setup
SetupIconFile=..\app\assets\icon.ico
UninstallDisplayIcon={app}\cantiva.exe
UninstallDisplayName=Cantiva
LicenseFile=..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; agent 常會一直開著 cantiva-mcp.exe；更新時請使用者先關掉，免得檔案換不掉。
CloseApplications=yes
RestartApplications=no
ShowLanguageDialog=auto

[Languages]
Name: "zhtw"; MessagesFile: "ChineseTraditional.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
zhtw.TaskShortcuts=捷徑：
zhtw.TaskDesktop=建立桌面捷徑
zhtw.TaskAgents=AI agent：
zhtw.TaskSkill=安裝 cantiva-voice skill（Claude Code 與 ~/.agents/skills）
zhtw.TaskTools=用 winget 安裝 uv 與 Git（安裝語音引擎時需要；會開一個 PowerShell 視窗）
zhtw.RunTools=正在安裝 uv 與 Git…
zhtw.LaunchApp=開啟 Cantiva
en.TaskShortcuts=Shortcuts:
en.TaskDesktop=Create a desktop shortcut
en.TaskAgents=AI agents:
en.TaskSkill=Install the cantiva-voice skill (Claude Code and ~/.agents/skills)
en.TaskTools=Install uv and Git with winget (needed to install voice engines; opens a PowerShell window)
en.RunTools=Installing uv and Git...
en.LaunchApp=Open Cantiva

[Tasks]
Name: "desktopicon"; Description: "{cm:TaskDesktop}"; GroupDescription: "{cm:TaskShortcuts}"; Flags: unchecked
Name: "skill"; Description: "{cm:TaskSkill}"; GroupDescription: "{cm:TaskAgents}"
Name: "tools"; Description: "{cm:TaskTools}"; GroupDescription: "{cm:TaskAgents}"; Check: NeedsTools

[InstallDelete]
; 引擎與 skill 每次都整包換新，避免舊版留下已刪除的檔案。
Type: filesandordirs; Name: "{app}\engines"
Type: filesandordirs; Name: "{app}\skill"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#StageDir}\skill\cantiva-voice\*"; DestDir: "{%USERPROFILE}\.claude\skills\cantiva-voice"; Tasks: skill; Flags: ignoreversion recursesubdirs createallsubdirs uninsneveruninstall
Source: "{#StageDir}\skill\cantiva-voice\*"; DestDir: "{%USERPROFILE}\.agents\skills\cantiva-voice"; Tasks: skill; Flags: ignoreversion recursesubdirs createallsubdirs uninsneveruninstall

[Icons]
Name: "{autoprograms}\Cantiva"; Filename: "{app}\cantiva.exe"; WorkingDir: "{app}"; Comment: "Cantiva 本機語音工作室"
Name: "{autodesktop}\Cantiva"; Filename: "{app}\cantiva.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\install-tools.ps1"" -Pause"; StatusMsg: "{cm:RunTools}"; Tasks: tools; Flags: waituntilterminated
Filename: "{app}\cantiva.exe"; Description: "{cm:LaunchApp}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[Code]
var
  ToolsChecked: Boolean;
  ToolsMissing: Boolean;

function CommandExists(const Name: String): Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'), '/C where ' + Name + ' >nul 2>nul', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
end;

function NeedsTools: Boolean;
begin
  if not ToolsChecked then
  begin
    ToolsMissing := not (CommandExists('uv') and CommandExists('git'));
    ToolsChecked := True;
  end;
  Result := ToolsMissing;
end;
