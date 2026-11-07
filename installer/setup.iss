#ifndef PackageDir
#define PackageDir "..\dist\portable"
#endif
[Setup]
AppId={{90386497-3FA5-4560-84D1-0EA54835C65A}
AppName=Xprinter Quick Print
AppVersion=0.4.2
AppPublisher=Xprinter Quick Print contributors
DefaultDirName={localappdata}\Programs\XprinterQuickPrint
DefaultGroupName=Xprinter Quick Print
PrivilegesRequired=lowest
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=XprinterQuickPrint_Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#PackageDir}\app.ico
UninstallDisplayIcon={app}\XprinterQuickPrint.exe
LicenseFile=..\LICENSE
CloseApplications=yes

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
[Tasks]
Name: "desktopicon"; Description: "Создать ярлык на рабочем столе"; Flags: checkedonce
[Files]
Source: "..\assets\default-presets.json"; DestDir: "{userappdata}\XprinterQuickPrint"; DestName: "presets.json"; Flags: onlyifdoesntexist uninsneveruninstall
Source: "{#PackageDir}\*"; DestDir: "{app}"; Excludes: "portable.flag,config\*"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{group}\Xprinter Quick Print"; Filename: "{app}\XprinterQuickPrint.exe"
Name: "{autodesktop}\Xprinter Quick Print"; Filename: "{app}\XprinterQuickPrint.exe"; Tasks: desktopicon
[Run]
Filename: "{app}\XprinterQuickPrint.exe"; Description: "Запустить Xprinter Quick Print"; Flags: postinstall nowait skipifsilent runasoriginaluser
[Code]
var
  PrinterPage: TInputOptionWizardPage;
  QueueFound, DriverFound: Boolean;
  DetectionText: String;

procedure DetectPrinter;
var Locator, Services, Rows, Row: Variant; Index: Integer; Name: String;
begin
  QueueFound := False; DriverFound := False;
  DetectionText := '';
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Services := Locator.ConnectServer('', 'root\CIMV2');
    Rows := Services.ExecQuery('SELECT Name, DriverName FROM Win32_Printer');
    for Index := 0 to Rows.Count - 1 do begin
      Row := Rows.ItemIndex(Index);
      Name := Lowercase(String(Row.Name) + ' ' + String(Row.DriverName));
      if Pos('xprinter', Name) > 0 then QueueFound := True;
    end;
    Rows := Services.ExecQuery('SELECT Name FROM Win32_PrinterDriver');
    for Index := 0 to Rows.Count - 1 do begin
      Row := Rows.ItemIndex(Index);
      if Pos('xprinter', Lowercase(String(Row.Name))) > 0 then DriverFound := True;
    end;
    if DriverFound then DetectionText := 'Драйвер с именем Xprinter зарегистрирован. Это не подтверждает наличие настроенной очереди.'
    else DetectionText := 'Драйвер с явным именем Xprinter не найден. Пакет INF может присутствовать в Windows; это проверит штатный мастер.';
  except
    DetectionText := 'Автоматическая проверка не завершилась. Вы можете выбрать установленный принтер в приложении или открыть мастер Windows.';
  end;
end;

procedure InitializeWizard;
begin
  DetectPrinter;
  PrinterPage := CreateInputOptionPage(wpSelectTasks, 'Настройка принтера Xprinter',
    'Совместимый принтер Xprinter пока не подтверждён.',
    'Хотите установить драйвер или настроить принтер сейчас?' + #13#10 + DetectionText + #13#10 +
    'Неизвестная модель не выбирается автоматически. Для USB и сетевых устройств способы настройки различаются.', True, False);
  PrinterPage.Add('Установить драйвер / добавить принтер через мастер Windows');
  PrinterPage.Add('Выбрать уже установленный принтер в приложении');
  PrinterPage.Add('Пропустить');
  PrinterPage.SelectedValueIndex := 2;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = PrinterPage.ID) and QueueFound;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var Code: Integer;
begin
  if (CurStep = ssPostInstall) and (not WizardSilent) and (not QueueFound) then begin
    if PrinterPage.SelectedValueIndex = 0 then
      ShellExecAsOriginalUser('open', ExpandConstant('{sys}\rundll32.exe'), 'printui.dll,PrintUIEntry /il', '', SW_SHOWNORMAL, ewNoWait, Code);
    if PrinterPage.SelectedValueIndex = 1 then
      ShellExecAsOriginalUser('open', ExpandConstant('{app}\XprinterQuickPrint.exe'), '--setup', '', SW_SHOWNORMAL, ewNoWait, Code);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usPostUninstall) and (not UninstallSilent) then
    if MsgBox('Удалить пользовательские настройки и пресеты Xprinter Quick Print?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{userappdata}\XprinterQuickPrint'), True, True, True);
end;





