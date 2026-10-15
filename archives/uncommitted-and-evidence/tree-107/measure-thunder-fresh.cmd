@echo off
setlocal
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-compact-poll-runtime\nbcad.exe --expected-exe-sha f9d476b25c3d5a37effd2ff3990d0c93bd5d22c3a71fe847102cd6307a220ebb --verification after-batch --webview-profile fresh-per-run --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-thunder-fresh-20261011\empty-en --profile dev-app1-debug2-deps3-debug0 --cache first --warmups 0 --locale en --label thunder-fresh-process-profile --runs 5
if errorlevel 1 exit /b %errorlevel%
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-compact-poll-runtime\nbcad.exe --expected-exe-sha f9d476b25c3d5a37effd2ff3990d0c93bd5d22c3a71fe847102cd6307a220ebb --verification after-batch --webview-profile fresh-per-run --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results-thunder-fresh-20261011\document-en --profile dev-app1-debug2-deps3-debug0 --cache first --warmups 0 --locale en --label thunder-fresh-process-profile --runs 5 --document "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad" --expected-document-sha d7e65fb222a0bb59f0ac82c3f3143a3437a487f3bbacf3a1f314b989b96de609
if errorlevel 1 exit /b %errorlevel%
