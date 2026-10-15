const {root,edit,nodes,removeImports,removeFunctions,removeStatements,removeVars,ts,fs,path}=require('./retire-frontend-host.cjs');
const f='src/components/viewport/Viewport.tsx';
removeVars(f,['snapMarkerKind','transientStart','transientEnd','transientPosition','profileRegionCache','cachedProfileRegion','cachedPicker','cachedPickerHidden','cachedPickerDocument','cachedSolidPreview','cachedSolidScene','committedHoleDefinitions','committedBodyFeatureDefinitions','cachedThreadDefinitions','cachedThreadBodyFeatureDefinitions','cachedThreadScene','cachedThreadHidden','cachedThreadAppearances','cachedThreadSolution','activeToolCursorScreen','holeDefinitionsRequest','refreshCommittedHoleDefinitions']);
removeStatements(f,['snapMarkerKind =','activeToolCursorScreen =','holeDefinitionsRequest +=','refreshCommittedHoleDefinitions(']);
edit(f,s=>s
  .replace('Native Bevy viewport interaction layer','Browser viewport interaction layer')
  .replace('builds CPU interaction proxies and forwards pointer/camera intent to the\r\n * native viewport.','builds CPU interaction proxies for browser pointer and camera input.')
  .replace(/  \/\*\* Compact procedural icon id[^\r\n]*\r?\n/,'')
  .replace(/    \/\*\* The native viewport renders on demand[\s\S]*?\*\//,'    /** Cursor changes request another interaction frame. */')
  .replace(/      \/\/ The badge lives in the native HUD[\s\S]*?      const badge = toolCursorRef.current;/,'      wakeCursorHud();\n      const badge = toolCursorRef.current;')
  .replace(/      \/\/ Solid-command fills and arrows are world-space retained Bevy assets\.[\s\S]*?      \{\r?\n        wakeControllerFrame\(\);\r?\n      }/,'      wakeControllerFrame();')
  .replaceAll('slow WASM/Tauri replies','slow WASM replies'));
nodes('src/App.tsx',n=>ts.isIfStatement(n)&&n.expression.getText().trim().startsWith('false &&')?'':undefined);
edit('src/App.tsx',s=>s.replace(/        \/\/ The custom native menu[^\r\n]*\r?\n        \/\/ WKWebView[^\r\n]*\r?\n/,'').replace(/        \/\/ On macOS\/Tauri[^\r\n]*\r?\n        \/\/ emit one command[^\r\n]*\r?\n/,''));
edit('src/files/projectFiles.ts',s=>s.replace('openProject(options?: { filePath: string; discardChanges?: boolean })','openProject()').replace(/  if \(state.dirty && options[^\r\n]*\r?\n/,'').replace('if (state.dirty && !options)','if (state.dirty)'));
edit('src/store/appStore.ts',s=>s.replace("engineKind: 'tauri' | 'wasm' | null;","engineKind: 'wasm' | null;"));
edit('src/engine/index.ts',s=>s.replace(/\/\*\*[\s\S]*?\*\//,'/** Browser CAD engine API backed by the existing WASM host. */'));
edit('src/engine/wasm.ts',s=>s.replace(/\/\*\*[\s\S]*?\*\//,'/** Browser adapter for the existing shared Rust/WASM engine and OCCT worker. */'));
edit('src/engine/types.ts',s=>s.replace('Both hosts (Tauri, WASM) exchange','The native and WASM engines exchange'));
edit('src/INDEX.md',s=>s.replace('React app bootstrap + snapshot bridge start','Browser React bootstrap').replace(/\| \[sessionBridge.ts\][^\r\n]*\r?\n/,'').replace('Host-neutral CAD core (TS)','Browser WASM engine API (TS)').replace('one hydrated OCCT/Bevy engine','one hydrated browser OCCT engine').replace(/\r?\nFocus mapping for steerable MCP:[\s\S]*/, '\n'));
edit('src/components/viewport/ExtrudeManipulator.tsx',s=>s.replace(/      \/\/ Browser development has no native camera event source\.[\s\S]*?      frame =/,'      frame ='));
edit('src/components/viewport/OrientationDial.tsx',s=>s.replace(/      \/\/ Browser\/dev rendering has no native bridge event source[^\r\n]*\r?\n      \/\/[^\r\n]*\r?\n/,''));
function all(dir){return fs.readdirSync(path.join(root,dir),{withFileTypes:true}).flatMap(e=>e.isDirectory()?all(dir+'/'+e.name):[dir+'/'+e.name]);}
const modified=require('child_process').execFileSync('git',['diff','--name-only'],{cwd:root,encoding:'utf8'}).trim().split(/\r?\n/);
for(const p of modified)if(fs.existsSync(path.join(root,p))&&/\.[tj]sx?$/.test(p))edit(p,s=>s.replace(/[ \t]+(?=\r?$)/gm,'').replace(/(?:\r?\n){3,}/g,'\n\n'));
const issues=[];
for(const p of all('src').filter(p=>/\.[tj]sx?$/.test(p))) {
  const source=fs.readFileSync(path.join(root,p),'utf8');
  const tree=ts.createSourceFile(p,source,ts.ScriptTarget.Latest,true,p.endsWith('tsx')?ts.ScriptKind.TSX:ts.ScriptKind.TS);
  for(const d of tree.parseDiagnostics)issues.push(p+': '+ts.flattenDiagnosticMessageText(d.messageText,' '));
  for(const n of tree.statements)if(ts.isImportDeclaration(n)&&n.moduleSpecifier.text.startsWith('.')) {
    const target=path.resolve(root,path.dirname(p),n.moduleSpecifier.text);
    if(!['','.ts','.tsx','.js','.mjs','.json','/index.ts','/index.tsx'].some(s=>fs.existsSync(target+s)))issues.push(p+': missing '+n.moduleSpecifier.text);
  }
}
console.log(issues.join('\n')||'Source parser and relative import inventory: coherent');
