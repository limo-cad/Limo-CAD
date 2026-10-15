@echo off
set NODE_OPTIONS=--require=D:\limo-startup-baseline-e4015de\mock-startup.cjs
set MOCK_STARTUP_FAILURE=true
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
node scripts\measure-startup.mjs --exe "C:\Program Files\nodejs\node.exe" --output D:\limo-startup-harness-selfcheck-error --profile mock --cache warm --runs 5 --timeout 10000
