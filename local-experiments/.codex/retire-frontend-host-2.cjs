const {root,edit,nodes,removeImports,removeFunctions,removeEffects,removeStatements,removeVars,browserOnly,ts,fs,path}=require('./retire-frontend-host.cjs');
function fold(file, constants) {
  function val(n) {
    const t=n.getText(); if(Object.hasOwn(constants,t)) return constants[t];
    if(n.kind===ts.SyntaxKind.FalseKeyword)return false;
    if(n.kind===ts.SyntaxKind.TrueKeyword)return true;
    if(ts.isParenthesizedExpression(n))return val(n.expression);
    if(ts.isPrefixUnaryExpression(n)&&n.operator===ts.SyntaxKind.ExclamationToken){const v=val(n.operand);return v===undefined?undefined:!v;}
    if(ts.isBinaryExpression(n)) {
      const a=val(n.left),b=val(n.right),k=n.operatorToken.kind;
      if(k===ts.SyntaxKind.AmpersandAmpersandToken) {if(a===false||b===false)return false;if(a===true&&b===true)return true;}
      if(k===ts.SyntaxKind.BarBarToken) {if(a===true||b===true)return true;if(a===false&&b===false)return false;}
    }
  }
  for(let i=0;i<4;i++)nodes(file,n=>{
    if(ts.isJsxExpression(n)&&n.expression&&val(n.expression)===false)return '';
    if(ts.isIfStatement(n)){const v=val(n.expression);if(v!==undefined)return v?n.thenStatement.getText():n.elseStatement?.getText()??'';}
    if(ts.isConditionalExpression(n)){const v=val(n.condition);if(v!==undefined)return '('+(v?n.whenTrue:n.whenFalse).getText()+')';}
    if(ts.isBinaryExpression(n)){
      const a=val(n.left),b=val(n.right),k=n.operatorToken.kind,v=val(n);
      if(v!==undefined)return String(v);
      if(k===ts.SyntaxKind.AmpersandAmpersandToken){if(a===true)return n.right.getText();if(b===true)return n.left.getText();}
      if(k===ts.SyntaxKind.BarBarToken){if(a===false)return n.right.getText();if(b===false)return n.left.getText();}
    }
  });
}
removeImports('src/components/TopBar.tsx',['../engine','../files/applicationExit','../scripts/workspace','./viewport/nativeViewportBridge']);
removeVars('src/components/TopBar.tsx',['scriptsOpen']);
removeEffects('src/components/TopBar.tsx',['if (menuOpen && menuPosition) requestNativeViewportLayout()']);
removeStatements('src/components/TopBar.tsx',['requestNativeViewportLayout()']);
nodes('src/components/TopBar.tsx',n=>((ts.isJsxElement(n)||ts.isJsxSelfClosingElement(n))&&['void openScriptFile()','onClick={showScripts}'].some(s=>n.getText().includes(s))&&n.getText().length<600)?'':undefined);
fold('src/components/TopBar.tsx',{'isTauriRuntime()':false});
removeImports('src/components/Ribbon.tsx',['./FeatureScriptPreview']);
nodes('src/components/Ribbon.tsx',n=>ts.isReturnStatement(n)&&n.getText().includes('<FeatureScriptPreview')?'return control;':undefined);
removeImports('src/input/sixDofMouse.ts',['../engine']);
removeVars('src/input/sixDofMouse.ts',['connectNative','detachNative','nativeUnlisten']);
fold('src/input/sixDofMouse.ts',{'isTauriRuntime()':false});
removeStatements('src/input/sixDofMouse.ts',['await detachNative()']);
removeImports('src/components/viewport/NavBar.tsx',['../../engine']);
removeVars('src/components/viewport/NavBar.tsx',['windowsDesktop']);
fold('src/components/viewport/NavBar.tsx',{'windowsDesktop':false});
edit('src/files/projectTabs.ts',s=>s.replace('getEngine, isTauriRuntime','getEngine'));
removeFunctions('src/files/projectTabs.ts',['systemMemoryStatus']);
nodes('src/files/projectTabs.ts',n=>ts.isFunctionDeclaration(n)&&n.name?.text==='enforceProjectTabRetention'?n.getText().replace(/\r?\n  const memory = await systemMemoryStatus\(\);[\s\S]*\r?\n}/,'\n}'):undefined);
removeImports('src/components/cam/CamMachineFields.tsx',['react','../../engine','../../cam/posts']);
removeVars('src/components/cam/CamMachineFields.tsx',['[profiles, setProfiles]','[notice, setNotice]','[saving, setSaving]']);
removeEffects('src/components/cam/CamMachineFields.tsx',['privatePosts()']);
nodes('src/components/cam/CamMachineFields.tsx',n=>ts.isIfStatement(n)&&n.expression.getText()==="value.startsWith('private:')"?'':undefined);
nodes('src/components/cam/CamMachineFields.tsx',n=>ts.isJsxExpression(n)&&n.expression&&['profiles.length','notice &&'].some(s=>n.expression.getText().startsWith(s))?'':undefined);
fold('src/components/cam/CamMachineFields.tsx',{'isTauriRuntime()':false});
removeImports('src/cam/document.ts',['./library']);
removeFunctions('src/cam/document.ts',['importCamToolFromCentral','publishCamToolToCentral']);
removeVars('src/cam/document.ts',['centralTool']);
edit('src/cam/document.ts',s=>s.replace('centralTool?.id ?? next.next_tool_id','next.next_tool_id').replace(/\/\*\* Add one operator-defined tool[\s\S]*?\*\//,'/** Add a tool to the project, allocating from its existing document counter. */'));
const tool='src/components/cam/CamToolDialog.tsx';
removeImports(tool,['@tauri-apps/api/event','../../cam/library']);
edit(tool,s=>s.replace('  importCamToolFromCentral,\n','').replace('  publishCamToolToCentral,\n','').replace(/  importCamToolFromCentral,\r\n/,'').replace(/  publishCamToolToCentral,\r\n/,''));
removeEffects(tool,['reloadCentral','if (!pickFor || scope']);
removeVars(tool,['centralOn','[scope, setScope]','[central, setCentral]','reloadCentral','centralTwin','twinDiffers','importable','syncActions','[importId, setImportId]']);
fold(tool,{"scope === 'central'":false,"scope === 'project'":true,'centralOn':false});
edit(tool,s=>s.replace(/\(central\?\.tools \?\? \[\]\)/g,'cam.tools').replace(/\(cam.tools\)/g,'cam.tools').replace(/, scope, cam.tools.length, central/g,', cam.tools.length').replace(/syncActions=\{syncActions\}/g,''));
removeStatements(tool,['await reloadCentral()']);
nodes(tool,n=>ts.isIfStatement(n)&&n.getText().includes('central.tools.length === 0')?'':undefined);
for(const file of ['src/components/cam/CamToolDialog.tsx','src/cam/document.ts'])edit(file,s=>s.replace(/[ \t]+(?=\r?$)/gm,''));
module.exports={fold};
