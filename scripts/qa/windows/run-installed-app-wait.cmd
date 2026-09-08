@echo off
setlocal EnableExtensions DisableDelayedExpansion

set "FOUND_APP="
for /r "%LOCALAPPDATA%\Programs" %%F in (*.exe) do (
    if %%~zF GTR 5000000 if not defined FOUND_APP set "FOUND_APP=%%~fF"
)

if not defined FOUND_APP exit /b 2

>C:\yeschoy-qa\app-run-result.txt echo state=starting
>>C:\yeschoy-qa\app-run-result.txt echo executable=%FOUND_APP%
>>C:\yeschoy-qa\app-run-result.txt echo userName=%USERNAME%

"%FOUND_APP%"
set "APP_EXIT=%ERRORLEVEL%"

>>C:\yeschoy-qa\app-run-result.txt echo state=exited
>>C:\yeschoy-qa\app-run-result.txt echo exitCode=%APP_EXIT%
exit /b %APP_EXIT%
