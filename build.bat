@echo off
setlocal
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cargo build --release --locked
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\prepare-runtime.ps1
if errorlevel 1 exit /b 1
copy /Y runtime\pdfium\bin\pdfium.dll target\release\pdfium.dll >nul
exit /b %errorlevel%

