; AirPlay3 安装包（Inno Setup 7）：装到当前用户（%LOCALAPPDATA%\Programs\AirPlay3），不需要管理员权限。
; 构建（先 cargo build --release）：
;   ISCC.exe installer\AirPlay3.iss /DBinDir=<AirPlay3.exe 所在目录> /DOutDir=<输出目录>
; 不写 /D 时用仓库里的 target\release 和 target\installer。版本号从 AirPlay3.exe 的版本信息里读。

#ifndef BinDir
  #define BinDir SourcePath + "..\target\release"
#endif
#ifndef OutDir
  #define OutDir SourcePath + "..\target\installer"
#endif
#define AppExe BinDir + "\AirPlay3.exe"
#if !FileExists(AppExe)
  #error 找不到 AirPlay3.exe，请先 cargo build --release，或用 /DBinDir= 指定它所在的目录
#endif
#define VerMajor
#define VerMinor
#define VerRev
#define VerBuild
#expr GetVersionComponents(AppExe, VerMajor, VerMinor, VerRev, VerBuild)
#define AppVer Str(VerMajor) + "." + Str(VerMinor) + "." + Str(VerRev)

[Setup]
; AppId 不能改：升级、卸载都靠它认出已经装过的 AirPlay3
AppId={{FF91F774-9775-48E1-9D00-1CFB4A52725A}
AppName=AirPlay3
AppVersion={#AppVer}
AppVerName=AirPlay3 {#AppVer}
AppPublisher=Lianzy-Baimiao
AppPublisherURL=https://github.com/Lianzy-Baimiao/AirPlay3
AppSupportURL=https://github.com/Lianzy-Baimiao/AirPlay3/issues
AppUpdatesURL=https://github.com/Lianzy-Baimiao/AirPlay3/releases
AppCopyright=Copyright (c) 2026 Lianzy-Baimiao
VersionInfoVersion={#AppVer}.0
VersionInfoDescription=AirPlay3 安装程序
VersionInfoProductName=AirPlay3
PrivilegesRequired=lowest
DefaultDirName={autopf}\AirPlay3
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableDirPage=auto
; 程序开着时不能覆盖或删掉它：提示先退出（程序的单实例互斥量）
AppMutex=AirPlay3.Rust.SingleInstance
SetupMutex=AirPlay3.Rust.Setup
CloseApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern dark
WizardImageFile=WizardImage.png
WizardSmallImageFile=WizardSmallImage.png
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\AirPlay3.exe
UninstallDisplayName=AirPlay3
OutputDir={#OutDir}
OutputBaseFilename=AirPlay3-v{#AppVer}-windows-x64-setup
Compression=lzma2/ultra64
SolidCompression=yes

[Languages]
Name: "chs"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[CustomMessages]
DeleteData=要同时删除 AirPlay3 的配置、曲库、歌词和图片缓存吗？%n%n只删除默认位置（%1）和程序文件夹里的数据；在「配置文件存放位置」里选的其它文件夹不会动。

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-LGPL-2.1.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\AirPlay3"; Filename: "{app}\AirPlay3.exe"
Name: "{autodesktop}\AirPlay3"; Filename: "{app}\AirPlay3.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\AirPlay3.exe"; Description: "{cm:LaunchProgram,AirPlay3}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; 删掉指向这份 AirPlay3.exe 的文件关联和开机启动（指向别的副本的不动）
Filename: "{app}\AirPlay3.exe"; Parameters: "--unregister"; RunOnceId: "Unregister"; Flags: waituntilterminated

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data, Portable: String;
begin
  if CurUninstallStep <> usPostUninstall then
    exit;
  Data := ExpandConstant('{userappdata}\AirPlay3');
  Portable := ExpandConstant('{app}\AIRPLAY_CONFIG');
  if not DirExists(Data) and not DirExists(Portable) then
    exit;
  // 静默卸载（/SUPPRESSMSGBOXES）时按默认的「否」，保留数据
  if SuppressibleMsgBox(FmtMessage(CustomMessage('DeleteData'), [Data]), mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES then
  begin
    DelTree(Data, True, True, True);
    DelTree(Portable, True, True, True);
    RemoveDir(ExpandConstant('{app}'));
  end;
end;
