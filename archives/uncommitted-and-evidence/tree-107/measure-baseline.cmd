@echo off
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe D:\limo-startup-baseline-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results\baseline-empty-en --profile dev-opt0-debug2 --cache warm --locale en --label baseline --runs 5
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-baseline-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results\baseline-document-en --profile dev-opt0-debug2 --cache warm --locale en --label baseline --runs 5 --document C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad
