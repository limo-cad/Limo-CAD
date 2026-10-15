@echo off
setlocal
set "GIT_INDEX_FILE=D:\limo-startup-baseline-e4015de\observer-check.index"
git -C D:\limo-startup-baseline-e4015de read-tree HEAD
if errorlevel 1 exit /b 1
git -C D:\limo-startup-baseline-e4015de apply --check --cached "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\scripts\startup-baseline-observers.patch"
if errorlevel 1 exit /b 1
set "GIT_INDEX_FILE="
git -C D:\limo-startup-baseline-e4015de apply --check --reverse "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\scripts\startup-baseline-observers.patch"
if errorlevel 1 exit /b 1
node --check "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\scripts\measure-startup.mjs"
