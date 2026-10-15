const {root,edit,nodes,removeImports,removeVars,browserOnly,ts,fs,path}=require('./retire-frontend-host.cjs');
for(const file of ['ExtrudeManipulator','MoveCopyManipulator','OffsetPlaneManipulator','OrientationDial']) {
  const p='src/components/viewport/'+file+'.tsx';
  removeImports(p,['./nativeViewportBridge']); browserOnly(p);
  removeVars(p,['onCameraChange','cameraChange']);
  nodes(p,n=>ts.isExpressionStatement(n)&&ts.isCallExpression(n.expression)&&['window.addEventListener','window.removeEventListener'].includes(n.expression.expression.getText())&&n.expression.arguments[0]?.getText()==="'nbcad:camera-change'"?'':undefined);
  nodes(p,n=>ts.isJsxAttribute(n)&&n.name.getText().startsWith('data-native-')?'':undefined);
  edit(p,s=>s.replace(/[ \t]+(?=\r?$)/gm,'').replace(/(?:\r?\n){3,}/g,'\n\n'));
}
