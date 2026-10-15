const {root,edit,nodes,removeImports,removeFunctions,removeEffects,removeStatements,removeVars,browserOnly,ts,fs,path}=require('./retire-frontend-host.cjs');
edit('src/files/projectFiles.ts',s=>s.replace('chooseOpenFile(PROJECT_TYPE, options?.filePath)','chooseOpenFile(PROJECT_TYPE)'));
removeImports('src/preferences/shared.test.ts',['./desktop']);
nodes('src/preferences/shared.test.ts',n=>ts.isBlock(n)&&n.getText().slice(0,150).includes('Test the real public desktop setters/IPC adapter')?'':undefined);
edit('src/components/cam/CamToolDialog.tsx',s=>s
  .replace(/\/\*\* Tool library:[\s\S]*?\*\//,'/** Project tool library with a tool table and the existing cutter editor. */')
  .replace(/  \/\/ Picking starts on the project scope;[^\r\n]*\r?\n/,'')
  .replace(/  \/\*\* Confirm a picker selection:[\s\S]*?\*\//,'  /** Return the selected project tool to the waiting operation dialog. */')
  .replace(/  \/\/ Picker mode: when the project scope[\s\S]*?\r?\n\r?\n/,'')
  .replace(/      \/\/ Project-scope creation[\s\S]*?\r?\n      await/,'      await')
  .replace(/  \/\/ Sync state of the selected project snapshot[^\r\n]*\r?\n/,'')
  .replace('  syncActions,','').replace(/  \/\*\* Optional project↔central[^\r\n]*\r?\n  syncActions\?: ReactNode;\r?\n/,'')
  .replace('        {syncActions}','').replace('/** Scope-aware save (central collection vs project snapshot). */','/** Save an existing or new project tool. */')
  .replace('// Validate central-library geometry too, not only project snapshots.','// Validate tool geometry before updating the project snapshot.'));
edit('src/cam/document.ts',s=>s
  .replace(/\/\*\* Import \(or refresh\) a central-library tool[\s\S]*?\*\//,'')
  .replace(/\/\*\* Publish a project snapshot back[\s\S]*?\*\//,'')
  .replace(/\/\*\* Edit the project's snapshot of a tool\.[\s\S]*?\*\//,"/** Edit the project's tool snapshot. */")
  .replace(/  \/\/ No library synchronisation[^\r\n]*\r?\n  \/\/ snapshots[^\r\n]*\r?\n/,'  // Project tools remain part of the existing CAM document.\n'));
edit('src/components/TopBar.tsx',s=>s.replace(/    \/\/ The Bevy surface is a native sibling[^\r\n]*\r?\n    \/\/ mounted menu[^\r\n]*\r?\n/,''));
edit('src/files/projectTabs.ts',s=>s.replace(/Keep professional documents warm by default,[\s\S]*?never eligible for eviction\./,'Retain warm browser documents, evicting long-idle inactive contexts. The active tab is never eligible for eviction.'));
function all(dir) {return fs.readdirSync(path.join(root,dir),{withFileTypes:true}).flatMap(e=>e.isDirectory()?all(dir+'/'+e.name):[dir+'/'+e.name]);}
const removed=[];
for(const f of all('src')) {
  if(/\.test\.tsx?$/.test(f)&&fs.readFileSync(path.join(root,f),'utf8').includes('__TAURI')) {fs.unlinkSync(path.join(root,f));removed.push(f);}
}
for(const f of ['src/sessionBridge.ts','src/liveUiBridge.ts','src/uiFiles.ts','src/scripts/workspace.ts','src/scripts/recipeLinks.ts','src/components/ScriptPanel.tsx','src/components/ScriptPreview.tsx','src/components/FeatureScriptPreview.tsx','src/cam/library.ts','src/cam/posts.ts','src/components/cam/CamLibrarySettings.tsx','src/components/cam/CamPostSettings.tsx']) {if(fs.existsSync(path.join(root,f)))fs.unlinkSync(path.join(root,f));removed.push(f);}
for(const f of all('src')) if(/\.[tj]sx?$/.test(f))edit(f,s=>s.replace(/[ \t]+(?=\r?$)/gm,'').replace(/(?:\r?\n){3,}/g,'\n\n'));
fs.writeFileSync('C:/Users/jeffg/dev/noBS-CAD/.codex/retired-frontend-host-files.txt',removed.join('\n'));
console.log(removed.join('\n'));
