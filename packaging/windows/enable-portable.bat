@echo off
setlocal DisableDelayedExpansion
if exist "%~dp0markview-portable-marker.txt" exit /b 0
type nul > "%~dp0markview-portable-marker.txt"
if errorlevel 1 exit /b 1
echo Portable mode enabled. Restart Markview to use the data directory.
