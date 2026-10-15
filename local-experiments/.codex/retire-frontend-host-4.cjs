const {root,edit,nodes,removeImports,removeFunctions,removeEffects,removeStatements,removeVars,browserOnly,ts,fs,path}=require('./retire-frontend-host.cjs');
const bridge=fs.readFileSync(path.join(root,'src/components/viewport/nativeViewportBridge.ts'),'utf8');
let types=bridge.slice(bridge.indexOf('export type NativeViewportLinePattern'),bridge.indexOf('export interface NativeViewportAnnotation'));
types=types.replaceAll('NativeViewport','Overlay').replace('Bevy can preserve','renderers can preserve').replace('rendered by Bevy, never the kernel','for presentation, never kernel geometry').replace('Bevy owns its shaft and arrowhead pixels.','The renderer owns its shaft and arrowhead pixels.').replace('legacy command overlays continue to receive flat normals natively','command overlays can use flat normals');
fs.writeFileSync(path.join(root,'src/cam/overlayGeometry.ts'),'/** Renderer-independent CAM presentation geometry. */\n'+types);
for(const f of ['src/cam/overlay.ts','src/cam/simulationPath.ts'])edit(f,s=>s.replaceAll('NativeViewport','Overlay').replaceAll('../components/viewport/nativeViewportBridge','./overlayGeometry').replace('viewport\'s native preview channel (see `collectOverlayTransient` in','viewport presentation (see the interaction scene in'));
const f='src/components/viewport/Viewport.tsx';
removeImports(f,['./nativeViewportBridge']);
removeVars(f,['detachNativeViewport','collectNativeViewportTransient','jointConnectorFromNativePick','nativeTransientDirty','native','cachedPickerTriangles','cachedSolidTriangles','cachedSolidArrows','cachedThreadLines']);
browserOnly(f);
nodes(f,n=>ts.isExpressionStatement(n)&&['syncNativeViewportCamera(','syncNativeViewportPreview(','detachNativeViewport(','nativeTransientDirty =','delete w.__nativeViewport'].some(s=>n.getText().startsWith(s))?'':undefined);
nodes(f,n=>ts.isExpressionStatement(n)&&ts.isBinaryExpression(n.expression)&&n.expression.left.getText().includes('__nativeViewport')?'':undefined);
nodes(f,n=>ts.isIfStatement(n)&&n.expression.getText().startsWith('native &&')?'':undefined);
nodes(f,n=>ts.isIfStatement(n)&&n.expression.getText().includes('!native ||')?n.thenStatement.getText():undefined);
edit(f,s=>s.replaceAll('NativeViewportSnapKind','SnapMarkerKind').replaceAll('nativeSnapKind','snapMarkerKindForTarget')
  .replace(/  nativeIcon: NativeViewportToolIcon;\r?\n/,'').replace(/, nativeIcon: '[^']+'/g,'')
  .replace(/        __nativeViewport(?:Transient|Presentation)\?: unknown;\r?\n/g,'')
  .replace(/\/\*\*[\s\S]*?Convert the CPU interaction scene into a small semantic payload[\s\S]*?\*\//, match=>match.slice(0,match.lastIndexOf('/**'))));
// The snap marker selects existing browser sprite artwork as well as position.
edit(f,s=>s.replace('    let snapMarkerKind: SnapMarkerKind',"    type SnapMarkerKind = 'grid' | 'origin' | 'point' | 'midpoint' | 'reference_midpoint' | 'curve';\n    let snapMarkerKind: SnapMarkerKind"));
for(const file of ['ExtrudeManipulator','MoveCopyManipulator','OffsetPlaneManipulator','OrientationDial']) {
  const p='src/components/viewport/'+file+'.tsx';
  removeImports(p,['./nativeViewportBridge']); browserOnly(p);
  removeVars(p,['onCameraChange','cameraChange']);
  nodes(p,n=>ts.isExpressionStatement(n)&&n.getText().includes("'nbcad:camera-change'")?'':undefined);
}
edit('src/index.css',s=>s.replace(/\/\*\r?\n \* Desktop production boundary:[\s\S]*?(?=\/\*\r?\n \* noBS CAD feature panels)/,'.viewport-surface {\n  background: linear-gradient(180deg, var(--vp-top) 0%, var(--vp-bottom) 100%);\n}\n\n').replace('and Tauri-hosted command dialogs.','command dialogs.'));
edit(f,s=>s.replaceAll('native-viewport-surface','viewport-surface'));
fs.unlinkSync(path.join(root,'src/components/viewport/nativeViewportBridge.ts'));
// Remove retired native compositor and window-drag attributes, retaining all
// user-facing roles and browser test selectors.
function all(dir){return fs.readdirSync(path.join(root,dir),{withFileTypes:true}).flatMap(e=>e.isDirectory()?all(dir+'/'+e.name):[dir+'/'+e.name]);}
for(const file of all('src').filter(f=>f.endsWith('.tsx')))nodes(file,n=>ts.isJsxAttribute(n)&&(n.name.getText().startsWith('data-native-')||n.name.getText()==='data-tauri-drag-region')?'':undefined);
for(const file of all('src').filter(f=>/\.[tj]sx?$/.test(f)))edit(file,s=>s.replace(/[ \t]+(?=\r?$)/gm,'').replace(/(?:\r?\n){3,}/g,'\n\n'));
