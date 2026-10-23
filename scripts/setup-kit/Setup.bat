@echo off
setlocal
cd /d "%~dp0"

echo ==========================================
echo   GIF Replacer Tool - one-time setup
echo ==========================================
echo.
echo This may take 15-30 minutes the first time.
echo Do not close this window until it says PASS.
echo.

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
set "RC=%ERRORLEVEL%"

echo.
if "%RC%"=="0" (
    echo Setup finished successfully.
) else (
    echo Setup finished with errors. See the messages above.
)
echo.
pause
exit /b %RC%
