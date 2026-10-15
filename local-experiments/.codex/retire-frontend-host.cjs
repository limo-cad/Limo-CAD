const fs = require('fs');
const path = require('path');
const ts = require('C:/Users/jeffg/dev/noBS-CAD/node_modules/typescript');
const root = 'C:/Users/jeffg/dev/noBS-CAD-worktrees/browser-without-desktop-host';
function edit(file, callback) {
  const p = path.join(root, file);
  const before = fs.readFileSync(p, 'utf8');
  const after = callback(before);
  if (after !== before) fs.writeFileSync(p, after);
}
function nodes(file, choose) {
  edit(file, source => {
    const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, file.endsWith('tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
    const edits = [];
    function walk(node) {
      const replace = choose(node, source, tree);
      if (replace !== undefined) { edits.push([node.getStart(tree), node.end, replace]); return; }
      ts.forEachChild(node, walk);
    }
    walk(tree);
    for (const [start,end,replacement] of edits.sort((a,b)=>b[0]-a[0])) source = source.slice(0,start)+replacement+source.slice(end);
    return source;
  });
}
function removeImports(file, modules) { nodes(file, n => ts.isImportDeclaration(n) && modules.includes(n.moduleSpecifier.text) ? '' : undefined); }
function removeFunctions(file, names) { nodes(file, n => ts.isFunctionDeclaration(n) && names.includes(n.name?.text) ? '' : undefined); }
function removeEffects(file, snippets) { nodes(file, n => ts.isExpressionStatement(n) && ts.isCallExpression(n.expression) && ['useEffect','useLayoutEffect'].includes(n.expression.expression.getText()) && snippets.some(s=>n.getText().includes(s)) ? '' : undefined); }
function removeStatements(file, snippets) { nodes(file, n => ts.isExpressionStatement(n) && snippets.some(s=>n.getText().startsWith(s)) ? '' : undefined); }
function removeVars(file, names) { nodes(file, n => ts.isVariableStatement(n) && n.declarationList.declarations.every(d=>names.includes(d.name.getText())) ? '' : undefined); }
function browserOnly(file) {
  const falseCalls = new Set(['isTauriRuntime()', 'nativeViewportIsActive()', 'nativeMacMenuOwnsUndoRedo()', 'centralLibraryAvailable()']);
  function value(n) {
    if (falseCalls.has(n.getText())) return false;
    if (ts.isParenthesizedExpression(n)) return value(n.expression);
    if (ts.isPrefixUnaryExpression(n) && n.operator===ts.SyntaxKind.ExclamationToken) { const v=value(n.operand); return v===undefined?undefined:!v; }
    return undefined;
  }
  for(let round=0;round<4;round++) nodes(file, n => {
    if(ts.isIfStatement(n)) { const v=value(n.expression); if(v!==undefined) return v?n.thenStatement.getText():(n.elseStatement?.getText()??''); }
    if(ts.isConditionalExpression(n)) { const v=value(n.condition); if(v!==undefined) return '('+(v?n.whenTrue:n.whenFalse).getText()+')'; }
    if(ts.isBinaryExpression(n)) { const v=value(n.left); if(v!==undefined) { if(n.operatorToken.kind===ts.SyntaxKind.AmpersandAmpersandToken) return v?n.right.getText():'false'; if(n.operatorToken.kind===ts.SyntaxKind.BarBarToken) return v?'true':n.right.getText(); } }
    return undefined;
  });
}
edit('src/engine/index.ts', s=>s.replace("readonly kind: 'tauri' | 'wasm';", "readonly kind: 'wasm';"));
removeFunctions('src/engine/index.ts',['isTauriRuntime']);
nodes('src/engine/index.ts', n=> ts.isFunctionDeclaration(n)&&n.name?.text==='getEngine' ? `export function getEngine(): Promise<Engine> {
  enginePromise ??= import('./wasm').then((m) => m.WasmEngine.create());
  return enginePromise;
}`:undefined);
removeImports('src/files/fileIO.ts',['@tauri-apps/api/core','../engine']);
removeFunctions('src/files/fileIO.ts',['withNativeViewportSuspended','pathName']);
browserOnly('src/files/fileIO.ts');
nodes('src/files/fileIO.ts', n=>ts.isIfStatement(n)&&n.expression.getText()==="target.kind === 'native'"?'':undefined);
edit('src/files/fileIO.ts',s=>s.replace("type NativeTarget = { kind: 'native'; path: string; name: string };",'').replace('NativeTarget | BrowserTarget','BrowserTarget').replace('type: SaveType, pathOverride?: string','type: SaveType'));
removeImports('src/App.tsx',['./nativeEditMenu','./nativeFileMenu','./engine','./scripts/recipeLinks','./components/ScriptPanel','./files/applicationExit','./files/exitSettlement']);
removeEffects('src/App.tsx',['installRecipeLinks','installNativeEditMenu','installNativeFileMenu','isTauriRuntime()']);
browserOnly('src/App.tsx');
edit('src/App.tsx',s=>s.replace('        <ScriptPanel />','').replace('  saveAllUnsavedProjects,\n',''));
removeImports('src/main.tsx',['./sessionBridge','./preferences/bootstrap']);
removeStatements('src/main.tsx',['startSessionBridge()']);
removeVars('src/main.tsx',['stopDesktopPreferences']);
nodes('src/main.tsx',n=>ts.isIfStatement(n)&&n.getText().includes('dispose(stopDesktopPreferences)')?'':undefined);
for(const file of ['src/navigationPreferences.ts','src/theme.ts','src/i18n/locales.ts']) {
  removeImports(file,['./preferences/desktop','../preferences/desktop']);
  removeStatements(file,['persistDesktopPreference(']);
}
removeImports('src/components/AppearanceDialog.tsx',['@tauri-apps/api/core','../engine','../preferences/desktop','./cam/CamLibrarySettings']);
removeEffects('src/components/AppearanceDialog.tsx',['isTauriRuntime']);
removeVars('src/components/AppearanceDialog.tsx',['preferenceError','[build, setBuild]','[buildError, setBuildError]']);
nodes('src/components/AppearanceDialog.tsx',n=>ts.isJsxExpression(n)&&n.expression?.getText().startsWith('preferenceError &&')?'':undefined);
nodes('src/components/AppearanceDialog.tsx',n=>ts.isJsxExpression(n)&&n.expression?.getText().startsWith('build\n')?"{t('appearance.browserWorkspace')}":undefined);
edit('src/components/AppearanceDialog.tsx',s=>s.replace('          <CamLibrarySettings />','').replace('useEffect, useRef, useState, useSyncExternalStore','useEffect, useRef'));
module.exports={root,edit,nodes,removeImports,removeFunctions,removeEffects,removeStatements,removeVars,browserOnly,ts,fs,path};
