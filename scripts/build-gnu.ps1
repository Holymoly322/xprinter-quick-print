param([ValidateSet('Check','Validate','Release')][string]$Mode='Validate')
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
Set-Location $root
$env:PATH="$env:USERPROFILE\.cargo\bin;"+$env:PATH
$llvm=Join-Path $root '.tools\llvm-mingw-20261104-ucrt-x86_64'
if (!(Test-Path "$llvm\bin\clang.exe")) {
 $archive=Join-Path $root '.tools\llvm-mingw-20261104-ucrt-x86_64.zip'
 New-Item -ItemType Directory -Force (Split-Path $archive) | Out-Null
 Invoke-WebRequest 'https://github.com/mstorsjo/llvm-mingw/releases/download/20261104/llvm-mingw-20261104-ucrt-x86_64.zip' -OutFile $archive
 if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne '552191B5754C49A5A20723276A6FC56541EA1922C4C50B7B669022333FC1B40F'){throw 'LLVM-MinGW hash mismatch'}
 Expand-Archive $archive -DestinationPath (Split-Path $archive) -Force
}
Copy-Item "$llvm\lib\clang\23\lib\windows\libclang_rt.builtins-x86_64.a" "$llvm\x86_64-w64-mingw32\lib\libgcc.a" -Force
Copy-Item "$llvm\x86_64-w64-mingw32\lib\libunwind.a" "$llvm\x86_64-w64-mingw32\lib\libgcc_eh.a" -Force
$env:PATH="$llvm\bin;"+$env:PATH
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$llvm\bin\x86_64-w64-mingw32-clang.exe"
$toolchain='stable-x86_64-pc-windows-gnu'
$version=& rustc "+$toolchain" --version
if ($version -notmatch '^rustc 1\.99\.1 '){throw 'Expected Rust stable 1.99.1 GNU. Install: rustup toolchain install 1.99.1-x86_64-pc-windows-gnu --component rustfmt --component clippy and update toolchain variable.'}
function Invoke-Cargo([string[]]$Arguments) {
 & cargo "+$toolchain" @Arguments
 if ($LASTEXITCODE -ne 0){throw "Cargo failed: $Arguments"}
}
if ($Mode -eq 'Check'){Invoke-Cargo -Arguments @('check','--locked');exit}
Invoke-Cargo -Arguments @('fmt','--check')
Invoke-Cargo -Arguments @('clippy','--locked','--all-targets','--','-D','warnings')
Invoke-Cargo -Arguments @('test','--locked')
if ($Mode -eq 'Release') {Invoke-Cargo -Arguments @('build','--release','--locked')}


if ($Mode -eq 'Release') { & "$PSScriptRoot\prepare-runtime.ps1"; Copy-Item 'runtime\pdfium\bin\pdfium.dll' 'target\release\pdfium.dll' -Force }

