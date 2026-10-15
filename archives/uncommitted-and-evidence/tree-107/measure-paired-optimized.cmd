@echo off
setlocal
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe D:\limo-startup-baseline-optimized-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-final\baseline-empty-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label baseline --runs 5
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-final-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-final\candidate-empty-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label candidate --runs 5
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-baseline-optimized-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-final\baseline-document-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label baseline --runs 5 --document "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad"
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-final-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-final\candidate-document-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label candidate --runs 5 --document "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad"
if errorlevel 1 exit /b %errorlevel%
