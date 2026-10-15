@echo off
setlocal
if exist D:\limo-startup-baseline-optimized-runtime\nbcad.exe (echo Immutable baseline already staged & exit /b 1)
copy /b D:\limo-startup-target-optimized\debug\nbcad.exe D:\limo-startup-baseline-optimized-runtime\nbcad.exe
if errorlevel 1 exit /b %errorlevel%
