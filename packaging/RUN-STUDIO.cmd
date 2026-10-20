@echo off
setlocal
cd /d "%~dp0"
if not defined LOCALAPPDATA (
  echo LOCALAPPDATA is unavailable. Use PowerShell with an explicit --workspace.
  pause
  exit /b 1
)
if not exist "%LOCALAPPDATA%\ICR-Studio\var\windows-secrets.json" (
  ICR-Studio.exe init-secrets
  if errorlevel 1 goto failed
)
echo Open http://127.0.0.1:8000 after the server starts.
echo Unlock with only the operator token from the private windows-secrets.json.
echo See README.txt for its location and instructions. Keep this window open.
ICR-Studio.exe serve
if errorlevel 1 goto failed
exit /b 0
:failed
echo Startup failed. Read the error above and README.txt. Do not delete existing keys.
pause
exit /b 1
