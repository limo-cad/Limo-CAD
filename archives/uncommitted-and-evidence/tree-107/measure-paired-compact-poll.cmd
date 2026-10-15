@echo off
setlocal
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-no-warmup-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-compact-poll\before-v3-empty-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label before-v3 --runs 5
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-compact-poll-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-compact-poll\after-compact-poll-empty-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label after-compact-poll --runs 5
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-no-warmup-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-compact-poll\before-v3-document-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label before-v3 --runs 5 --document "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad"
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-compact-poll-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-compact-poll\after-compact-poll-document-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label after-compact-poll --runs 5 --document "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad"
if errorlevel 1 exit /b %errorlevel%
