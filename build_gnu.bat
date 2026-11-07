@echo off
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-gnu.ps1 -Mode Release
exit /b %errorlevel%
