@echo off
setlocal EnableExtensions DisableDelayedExpansion

if "%~1"=="" exit /b 64

set "INSTALLER=%~1"
set "RESULT_FILE=%~2"
if not defined RESULT_FILE set "RESULT_FILE=%TEMP%\yeschoy-installer-result.txt"

start "" /wait "%INSTALLER%" /S
set "INSTALL_EXIT=%ERRORLEVEL%"

>"%RESULT_FILE%" echo exitCode=%INSTALL_EXIT%
>>"%RESULT_FILE%" echo userName=%USERNAME%
>>"%RESULT_FILE%" echo localAppData=%LOCALAPPDATA%
>>"%RESULT_FILE%" echo completedAt=%DATE% %TIME%

exit /b %INSTALL_EXIT%
