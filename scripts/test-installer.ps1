param([string]$Iscc='', [string]$StageDirectory='dist\portable')
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$stage=Join-Path $root $StageDirectory
if(!$Iscc){$Iscc=@("$root\.tools\InnoSetup\ISCC.exe","$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe","${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object {Test-Path -LiteralPath $_} | Select-Object -First 1}
if(!$Iscc){throw 'Inno Setup compiler not found'}
$id=[Guid]::NewGuid().ToString().ToUpperInvariant()
$qaRoot=Join-Path $root ".installer-qa\$id"
$install=Join-Path $qaRoot 'installed'
New-Item -ItemType Directory -Force $qaRoot | Out-Null
$script=Get-Content -LiteralPath "$root\installer\setup.iss" -Raw
$script=$script.Replace('90386497-3FA5-4560-84D1-0EA54835C65A',$id).Replace('AppName=Xprinter Quick Print','AppName=Xprinter Quick Print QA').Replace('DefaultGroupName=Xprinter Quick Print',"DefaultGroupName=Xprinter Quick Print QA $id").Replace('DefaultDirName={localappdata}\Programs\XprinterQuickPrint',"DefaultDirName=$install").Replace('OutputDir=..\dist',"OutputDir=$qaRoot").Replace('OutputBaseFilename=XprinterQuickPrint_Setup','OutputBaseFilename=InstallerQA').Replace('..\dist\portable',"$stage").Replace('LicenseFile=..\LICENSE',"LicenseFile=$root\LICENSE")
 $script=$script.Replace('..\assets\',"$root\assets\").Replace('{userappdata}\XprinterQuickPrint',"$qaRoot\user-data")
$script=[regex]::Replace($script,'(?m)^Name: "\{autodesktop\}.*\r?\n','')
 $script=$script.Replace('..\assets\',"$root\assets\").Replace('{userappdata}\XprinterQuickPrint',"$qaRoot\user-data")
$script=[regex]::Replace($script,'(?s)\[Run\].*?(?=\[Code\])','')
if(!$script.Contains("DefaultDirName=$install") -or $script.Contains('90386497-3FA5-4560-84D1-0EA54835C65A')){throw 'QA isolation checks failed'}
$qaScript=Join-Path $qaRoot 'setup-qa.iss'
[IO.File]::WriteAllText($qaScript,$script,[Text.UTF8Encoding]::new($true))
& $Iscc $qaScript *> "$qaRoot\compile.log"
if($LASTEXITCODE -ne 0){throw "QA compiler failed; see $qaRoot\compile.log"}
$protected=@{}
foreach($name in @('settings.json','presets.json','history.json')){$file=Join-Path "$env:APPDATA\XprinterQuickPrint" $name;if(Test-Path -LiteralPath $file){$protected[$file]=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash}}
$arguments='/VERYSILENT /SUPPRESSMSGBOXES /SP- /NORESTART /TASKS="" /DIR="{0}" /LOG="{1}"' -f $install,"$qaRoot\install.log"
$process=Start-Process -FilePath "$qaRoot\InstallerQA.exe" -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
if($process.ExitCode -ne 0){throw "QA installation failed: $($process.ExitCode)"}
foreach($name in @('XprinterQuickPrint.exe','pdfium.dll')){if((Get-FileHash -LiteralPath "$install\$name" -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath "$stage\$name" -Algorithm SHA256).Hash){throw "Payload mismatch: $name"}}
if((Test-Path -LiteralPath "$install\portable.flag") -or (Test-Path -LiteralPath "$install\config\presets.json")){throw 'Installed app contains portable user configuration'}
$factory=Get-Content -LiteralPath "$qaRoot\user-data\presets.json" -Raw | ConvertFrom-Json
if($factory[0].name -ne 'JOPA' -or $factory[0].width -ne 44 -or $factory[0].height -ne 23){throw 'Bundled JOPA preset missing'}
if($factory[1].name -ne 'HH' -or $factory[1].width -ne 40 -or $factory[1].height -ne 24){throw 'Bundled HH preset missing'}
$factory[0].name='QA user preset'
ConvertTo-Json -InputObject @($factory) -Depth 20 | Set-Content -LiteralPath "$qaRoot\user-data\presets.json" -Encoding utf8
$before=(Get-FileHash -LiteralPath "$qaRoot\user-data\presets.json").Hash
$process=Start-Process -FilePath "$qaRoot\InstallerQA.exe" -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
if($process.ExitCode -ne 0 -or (Get-FileHash -LiteralPath "$qaRoot\user-data\presets.json").Hash -ne $before){throw 'Update replaced user presets'}
$shortcut=Join-Path ([Environment]::GetFolderPath('Programs')) "Xprinter Quick Print QA $id\Xprinter Quick Print.lnk"
if(!(Test-Path -LiteralPath $shortcut)){throw 'Start menu shortcut missing'}
$registry="HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{$id}_is1"
if(!(Test-Path -LiteralPath $registry)){throw 'Uninstall registry entry was not created'}
$uninstaller=Get-ChildItem -LiteralPath $install -Filter 'unins*.exe'
if(@($uninstaller).Count -ne 1){throw 'Expected one QA uninstaller'}
$resolved=(Resolve-Path -LiteralPath $install).Path
if(!$resolved.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Uninstall path escaped workspace'}
$process=Start-Process -FilePath $uninstaller.FullName -ArgumentList ('/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="{0}"' -f "$qaRoot\uninstall.log") -WindowStyle Hidden -PassThru -Wait
if($process.ExitCode -ne 0){throw "QA uninstall failed: $($process.ExitCode)"}
if((Test-Path -LiteralPath "$install\XprinterQuickPrint.exe") -or (Test-Path -LiteralPath $registry)){throw 'QA uninstall left installed application or registry entry'}
if(!(Test-Path -LiteralPath "$qaRoot\user-data\presets.json") -or (Get-FileHash -LiteralPath "$qaRoot\user-data\presets.json").Hash -ne $before){throw 'Uninstall removed user presets'}
if(Test-Path -LiteralPath $shortcut){throw 'Uninstall left QA shortcut'}
foreach($file in $protected.Keys){if(!(Test-Path -LiteralPath $file) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $protected[$file]){throw 'Existing user configuration changed during the test'}}
[pscustomobject]@{passed=$true;app_id=$id;install_exit=0;uninstall_exit=0;payload_hash_match=$true;portable_config_excluded=$true;existing_config_preserved=$true;app_launched=$false;physical_printing=$false} | ConvertTo-Json | Set-Content -LiteralPath "$root\installer-smoke-result.json" -Encoding utf8
Write-Output 'Isolated installer smoke test passed. Existing installation and configuration were preserved.'

