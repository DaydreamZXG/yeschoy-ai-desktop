@echo off
setlocal EnableExtensions DisableDelayedExpansion

set "FOUND_APP="
for /r "%LOCALAPPDATA%\Programs" %%F in (*.exe) do (
    if %%~zF GTR 5000000 if not defined FOUND_APP set "FOUND_APP=%%~fF"
)

if not defined FOUND_APP exit /b 2

start "" "%FOUND_APP%"
>C:\yeschoy-qa\launch-result.txt echo executable=%FOUND_APP%
>>C:\yeschoy-qa\launch-result.txt echo userName=%USERNAME%
exit /b 0
