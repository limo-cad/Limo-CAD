@echo off
setlocal
set "MOCK_STARTUP_FAILURE=true"
set "NODE_OPTIONS=--require D:\limo-startup-baseline-e4015de\mock-startup.cjs"
node "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\scripts\measure-startup.mjs" --exe "C:\Program Files\nodejs\node.exe" --output D:\limo-startup-harness-final-error --profile mock --cache warm --locale en --runs 5 --document "C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7\examples\testPiece.nbcad"
