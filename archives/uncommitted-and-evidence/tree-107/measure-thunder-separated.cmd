@echo off
setlocal
if "%~1"=="" exit /b 2
if "%~2"=="" exit /b 2
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe "%~1" --expected-exe-sha "%~2" --verification after-batch --webview-profile fresh-per-run --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-thunder-separated-20261011\empty-en --profile dev-app1-debug2-deps3-debug0 --cache first --warmups 0 --locale en --label thunder-separated-fresh-process-profile --runs 5
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe "%~1" --expected-exe-sha "%~2" --verification after-batch --webview-profile fresh-per-run --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-thunder-separated-20261011\runtime-document-en --profile dev-app1-debug2-deps3-debug0 --cache first --warmups 0 --locale en --label thunder-separated-fresh-process-profile --runs 5 --document-mode runtime --document "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad" --expected-document-sha d7e65fb222a0bb59f0ac82c3f3143a3437a487f3bbacf3a1f314b989b96de609
if errorlevel 1 exit /b %errorlevel%
