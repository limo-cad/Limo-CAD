@echo off
setlocal
rem Manual first-launch protocol only: capture exact boot receipt; never reboot or clear caches here.
rem Run once per boot before other Limo CAD launches, using previously trusted staging hashes.
rem Usage: measure-thunder-boot-once.cmd NEW_OUTPUT_DIR
if "%~1"=="" exit /b 2
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe D:\limo-startup-candidate-compact-poll-runtime\nbcad.exe --expected-exe-sha f9d476b25c3d5a37effd2ff3990d0c93bd5d22c3a71fe847102cd6307a220ebb --verification after-batch --webview-profile fresh-per-run --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output "%~1" --profile dev-app1-debug2-deps3-debug0 --cache first --warmups 0 --locale en --label boot-first-launch-unverified-until-boot-receipt --runs 1
