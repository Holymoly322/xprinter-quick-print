param([string]$StageDirectory='dist\portable')
function Copy-BinaryIfChanged([string]$Source,[string]$Destination){
 if((Test-Path -LiteralPath $Destination) -and ((Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash)){return}
 Copy-Item -LiteralPath $Source -Destination $Destination -Force
}
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
Set-Location $root
& "$PSScriptRoot\prepare-runtime.ps1"
$out=Join-Path $root $StageDirectory
New-Item -ItemType Directory -Force $out | Out-Null
Copy-BinaryIfChanged 'target\release\xprinter-quick-print.exe' "$out\XprinterQuickPrint.exe"
$icon=Get-ChildItem 'target\release\build\xprinter-quick-print-*\out\app.ico' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if(!$icon){throw 'Release application icon missing'}
Copy-Item -LiteralPath $icon.FullName -Destination "$out\app.ico" -Force
Copy-BinaryIfChanged 'runtime\pdfium\bin\pdfium.dll' "$out\pdfium.dll"
New-Item -ItemType Directory -Force "$out\pdfium-licenses" | Out-Null
Copy-Item 'runtime\pdfium\licenses\*' "$out\pdfium-licenses" -Recurse -Force
Copy-Item 'runtime\pdfium\LICENSE' "$out\PDFium-package-LICENSE.txt" -Force
Copy-Item 'README.md','TEST_REPORT.md','THIRD_PARTY_NOTICES.md','LICENSE','Slint-LICENSE.md','LLVM-MinGW-LICENSE.txt' $out -Force
if (Test-Path 'THIRD_PARTY_LICENSES.html') {Copy-Item 'THIRD_PARTY_LICENSES.html' $out -Force} else {throw 'Generate dependency notices with cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html'}
$source=Join-Path $out 'source'
New-Item -ItemType Directory -Force $source | Out-Null
Copy-Item 'src','ui','assets','tests','installer','scripts','.cargo','.github','docs','presets' $source -Recurse -Force
Copy-Item 'Cargo.toml','Cargo.lock','build.rs','build.bat','build_gnu.bat','build_installer.bat','build_portable.bat','LICENSE','README.md','THIRD_PARTY_NOTICES.md','.gitignore','.gitattributes','about.hbs','about.toml','Slint-LICENSE.md','LLVM-MinGW-LICENSE.txt','TEST_REPORT.md' $source -Force
$verification=Join-Path $out 'verification'
New-Item -ItemType Directory -Force $verification | Out-Null
Get-ChildItem $root -Filter 'ui-*.png' | Copy-Item -Destination $verification -Force
foreach($evidence in @('runtime-smoke-preview.png','virtual-print-result.pdf','license-delta.json','installer-smoke-result.json','icon-preview.png')){if(Test-Path $evidence){Copy-Item $evidence $verification -Force}}
New-Item -ItemType File -Force "$out\portable.flag" | Out-Null
$allowed=@('XprinterQuickPrint.exe','app.ico','pdfium.dll','portable.flag','README.md','TEST_REPORT.md','THIRD_PARTY_NOTICES.md','THIRD_PARTY_LICENSES.html','LICENSE','Slint-LICENSE.md','LLVM-MinGW-LICENSE.txt','PDFium-package-LICENSE.txt','pdfium-licenses','source','verification')
$entries=Get-ChildItem -LiteralPath $out -Force | Where-Object { $_.Name -in $allowed }
Compress-Archive -Path $entries.FullName -DestinationPath 'dist\XprinterQuickPrint_Portable.zip' -Force
Get-FileHash 'dist\XprinterQuickPrint_Portable.zip' -Algorithm SHA256









