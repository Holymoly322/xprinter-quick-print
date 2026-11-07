@echo off
setlocal
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
call build.bat
if errorlevel 1 exit /b 1
cargo about generate --locked about.hbs -o THIRD_PARTY_LICENSES.html
if errorlevel 1 exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\package.ps1
exit /b %errorlevel%

