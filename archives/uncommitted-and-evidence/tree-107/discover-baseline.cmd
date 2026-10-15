@echo off
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe D:\limo-startup-baseline-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results\baseline-discovery-empty-3 --profile dev-opt0-debug2 --cache first --locale en --label baseline-discovery --runs 1 --timeout 180000
