import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {chromium} from 'playwright';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
import {checkPresentationSurfaces} from './presentation.mjs';
import {checkDimensionInputs} from './dimension-input.mjs';

const dispatcher=ts.createSourceFile('dispatch.ts',await readFile(new URL('../../src/ribbon/dispatch.ts',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true);
const dispatched=new Set();
function visit(node){
 if(ts.isSwitchStatement(node)&&node.expression.getText(dispatcher)==='action') {
  for(const clause of node.caseBlock.clauses) if(ts.isCaseClause(clause)&&ts.isStringLiteral(clause.expression)) dispatched.add(clause.expression.text);
 }
 ts.forEachChild(node,visit);
}
visit(dispatcher);

const wasmBindingContract={name:'wasm-binding-contract',resolveId(id){if(id.endsWith('/engine-wasm/pkg/nbcad_wasm'))return '\0wasm-binding-contract';},load(id){
 if(id!=='\0wasm-binding-contract')return;
 // Only the generated binding is replaced; adapter methods and context switching are real.
 return `const ok=value=>JSON.stringify({ok:true,value}); export default async function init(){}
 export class WasmEngine { model='original model';
 project_export_model(){return ok(this.model)}
 document_set_name(payload){this.model=JSON.parse(payload);return this.document()}
 document(){return ok({name:this.model,features:[],rollback_index:0,browser:[],settings:{units:'mm'}})}
 solid_scene(){return ok({bodies:[],errors:[]})} free(){} }`;
}};
const server=await createServer({configFile:false,optimizeDeps:{noDiscovery:true,entries:[],include:['react','react-dom','react-dom/client','react/jsx-runtime','react/jsx-dev-runtime']},server:{host:'127.0.0.1',port:0,watch:null},logLevel:'error',plugins:[wasmBindingContract,{name:'mcp-contract',configureServer(server){server.middlewares.use('/mcp-contract',(_req,res)=>{
 res.setHeader('Content-Type','text/html');
 res.end('<!doctype html><html><body><main data-mcp-surface="test-surface"><button>Run</button><button disabled>Disabled</button><label>Name<input value="old"></label><label>Choice<select><option value="a">A</option><option disabled value="b">B</option></select></label><button id="hidden" hidden>Hidden</button></main></body></html>');
});}}]});
let browser;
try {
 await server.listen();
 browser=await chromium.launch({headless:true,...(process.env.PLAYWRIGHT_CHANNEL ? {channel:process.env.PLAYWRIGHT_CHANNEL} : {})});
 const page=await browser.newPage();
 await page.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const result=await page.evaluate(async()=>{
  const {inspectUi,operateUi}=await import('/src/uiControl.ts');
  const {SerialPlayback,presentOperation,setPlaybackPace,presentation}=await import('/src/operationPlayback.ts');
  const {interfaceGroups,operationGroup}=await import('/src/interface.ts');
  const {drivePointer}=await import('/src/uiPointer.ts');
  const {trackEngineOperation,pendingEngineOperations}=await import('/src/engine/activity.ts');
  const config=await import('/src/ribbon/config.ts');
  const check=(condition,message)=>{if(!condition)throw new Error(message);};
  const rejects=(fn,pattern)=>{let error;try{fn();}catch(e){error=String(e);}check(error&&pattern.test(error),'Expected rejection '+pattern+', received '+error);};
  const context={document:1};
  const controls=()=>inspectUi(context).surfaces.flatMap(s=>s.controls);
  let list=controls();
  const row=document.createElement('div'); row.setAttribute('role','treeitem'); row.textContent='XY plane';
  document.querySelector('main').append(row);
  let gestures=[]; row.onclick=()=>gestures.push('select'); row.ondblclick=()=>gestures.push('edit'); row.oncontextmenu=()=>gestures.push('menu');
  list=controls();
  const plane=list.find(c=>c.role==='treeitem'&&c.label==='XY plane');
  check(plane,'Browser tree row missing');
  operateUi({action:'double_click',target:plane.id},context);
  operateUi({action:'context_menu',target:plane.id},context);
  check(gestures.join(',')==='select,edit,menu','Tree gestures do not reach real handlers');
  const example=document.createElement('div'); example.tabIndex=0; example.setAttribute('role','img');
  example.setAttribute('aria-label','Inspectable example'); document.querySelector('main').append(example);
  const inspectedKeys=[]; example.addEventListener('keydown',event=>inspectedKeys.push(event.key));
  const modelControl=controls().filter(control=>control.label==='Inspectable example');
  check(modelControl.length===1&&modelControl[0].role==='img','Focusable inspection surfaces must be discoverable exactly once');
  operateUi({action:'key',key:'ArrowRight',target:modelControl[0].id},context);
  operateUi({action:'key',key:'Home',target:modelControl[0].id},context);
  check(inspectedKeys.join(',')==='ArrowRight,Home'&&document.activeElement===example,'The shared key path must reach orbit and fit controls');
  example.remove();
  list=controls();
  const canvas=document.createElement('div'); canvas.style.cssText='position:fixed;left:0;top:250px;width:200px;height:100px'; document.body.append(canvas);
  const pointer=[]; for(const type of ['pointerdown','pointermove','pointerup']) canvas.addEventListener(type,e=>pointer.push([type,e.buttons,e.clientX]));
  await drivePointer(canvas,'drag',[10,260],false,[90,270]);
  check(pointer.filter(e=>e[0]==='pointerdown').length===1&&pointer.at(-1)[0]==='pointerup'&&pointer.at(-1)[1]===0,'Drag must release its button');
  check(pointer.some(e=>e[0]==='pointermove'&&e[1]===1&&e[2]===90),'Drag did not traverse to its destination');
  let invalid=false;try{await drivePointer(canvas,'drag',[10,260],false,[900,270]);}catch{invalid=true;}check(invalid,'Out-of-canvas drag accepted');
  canvas.remove();
  check(list.every(c=>c.surface==='test-surface'),'Controls lost their actual surface grouping');
  check(!list.some(c=>c.label==='Hidden'),'Hidden control advertised');
  check(!inspectUi(context).unlabeled_controls.length,'Labeled controls reported missing labels');
  const disclosure=document.createElement('details');
  disclosure.innerHTML='<summary>Load from a file path</summary><label>Script path<input></label><details><summary>Error details</summary><button>Copy error</button></details>';
  document.querySelector('main').append(disclosure);
  list=controls();
  const closedDisclosure=list.find(c=>c.label==='Load from a file path');
  check(closedDisclosure?.role==='button'&&closedDisclosure.expanded===false,'Native summary must be an inspectable collapsed button');
  check(!list.some(c=>c.label==='Script path'||c.label==='Error details'||c.label==='Copy error'),'Closed disclosure descendants must not be advertised');
  operateUi({action:'click',target:closedDisclosure.id},context);
  list=controls();
  check(disclosure.open&&list.find(c=>c.label==='Load from a file path').expanded===true,'Summary click must use the native details toggle');
  const pathControl=list.find(c=>c.label==='Script path');
  operateUi({action:'set_value',target:pathControl.id,value:'example.nbcad.jsonc'},context);
  check(disclosure.querySelector('input').value==='example.nbcad.jsonc','Opened disclosure fields must follow the shared edit path');
  check(!list.some(c=>c.label==='Copy error'),'Nested closed disclosure must remain hidden');
  operateUi({action:'click',target:list.find(c=>c.label==='Error details').id},context);
  list=controls();
  check(list.some(c=>c.label==='Copy error'),'Nested summary click must reveal its actual controls');
  const nestedControl=list.find(c=>c.label==='Copy error');
  operateUi({action:'click',target:list.find(c=>c.label==='Load from a file path').id},context);
  rejects(()=>operateUi({action:'click',target:nestedControl.id},context),/stale or unavailable/);
  list=controls();
  check(!list.some(c=>c.label==='Script path'||c.label==='Error details'||c.label==='Copy error'),'Closing a parent must hide all descendants even when a nested details remains open');
  operateUi({action:'key',key:'Enter',target:list.find(c=>c.label==='Load from a file path').id},context);
  check(disclosure.open,'Enter on a summary must perform its native activation');
  disclosure.remove();
  const tabs=document.createElement('div');
  tabs.innerHTML='<button role="tab" aria-selected="true">Overview</button><button role="tab" aria-selected="false">Source</button>';
  document.querySelector('main').append(tabs);
  tabs.lastElementChild.onclick=()=>{tabs.firstElementChild.setAttribute('aria-selected','false');tabs.lastElementChild.setAttribute('aria-selected','true');};
  list=controls();
  check(list.find(c=>c.label==='Overview').selected===true&&list.find(c=>c.label==='Source').selected===false,'Tab selection must be available as a boolean');
  operateUi({action:'click',target:list.find(c=>c.label==='Source').id},context);
  list=controls();
  check(list.find(c=>c.label==='Source').selected===true&&list.find(c=>c.label==='Overview').selected===false,'Inspected tab state must follow the actual click handler');
  tabs.remove();
  list=controls();
  rejects(()=>operateUi({action:'click',target:list.find(c=>c.label==='Disabled').id},context),/disabled/);
  const input=list.find(c=>c.label==='Name');
  let inputEvents=0; document.querySelector('input').addEventListener('input',()=>inputEvents++);
  operateUi({action:'set_value',target:input.id,value:'new'},context);
  check(document.querySelector('input').value==='new'&&inputEvents===1,'Field change did not follow event path');
  let blurred=false;document.querySelector('input').onblur=()=>{blurred=true;};
  operateUi({action:'click',target:list.find(c=>c.label==='Run').id},context);
  check(blurred,'Click must commit a focused field through blur');
  rejects(()=>operateUi({action:'set_value',target:list.find(c=>c.label==='Choice').id,value:'b'},context),/unavailable/);
  const old=list.find(c=>c.label==='Run').id;
  controls();
  rejects(()=>operateUi({action:'click',target:old},context),/stale/);
  list=controls();
  rejects(()=>operateUi({action:'click',target:list.find(c=>c.label==='Run').id},{}),/Document changed/);
  const run=list.find(c=>c.label==='Run');
  document.querySelector('button').textContent='Different action';
  rejects(()=>operateUi({action:'click',target:run.id},context),/Control changed/);
  const modal=document.createElement('div'); modal.setAttribute('aria-modal','true'); modal.setAttribute('role','dialog');
  modal.innerHTML='<h2>File operation failed</h2><p>The document changed while preparing export. Start the export again.</p><p hidden>Hidden details</p><p aria-hidden="true">Excluded details</p><input type="password" value="secret"><span contenteditable="true">Editable value</span><details><summary>Details</summary>Closed details</details><button>Accept</button>'; document.body.append(modal);
  const dialogSurface=inspectUi(context).surfaces.find(surface=>surface.name==='dialogs/dialog');
  check(dialogSurface?.text.includes('File operation failed')&&dialogSurface.text.includes('The document changed while preparing export. Start the export again.'),'MCP must read the actual visible dialog title and error');
  check(!/Hidden details|Excluded details|secret|Editable value|Closed details/.test(dialogSurface.text),'Dialog context must omit hidden, closed and field content');
  check(dialogSurface.controls.some(control=>control.label==='Accept'),'Dialog text must retain the existing controls and grouping');
  const notice=document.createElement('div');notice.setAttribute('role','alertdialog');notice.dataset.testid='notice';notice.textContent='Working on the file';document.body.append(notice);
  check(inspectUi(context).surfaces.some(surface=>surface.name==='dialogs/notice'&&surface.text==='Working on the file'&&surface.controls.length===0),'A visible message without controls must remain inspectable');
  notice.innerHTML='<p>Working on the file</p><button>Dismiss notice</button>';
  const alertSurface=inspectUi(context).surfaces.find(surface=>surface.name==='dialogs/notice');
  check(alertSurface.text.includes('Working on the file')&&alertSurface.controls.some(control=>control.label==='Dismiss notice'),'Alert dialog controls and text must share their existing surface');
  notice.textContent='x'.repeat(5000);
  const boundedNotice=inspectUi(context).surfaces.find(surface=>surface.name==='dialogs/notice');
  check(boundedNotice.text.length===4096&&boundedNotice.text_truncated===true,'Long dialog context must be explicitly bounded');
  notice.hidden=true;
  check(!inspectUi(context).surfaces.some(surface=>surface.name==='dialogs/notice'),'Hidden dialogs must not be advertised');
  notice.remove();
  const longSource=document.createElement('textarea'); longSource.setAttribute('aria-label','Long editable recipe');
  const chapter='A useful chapter near the end';
  const originalSource='// earlier geometry\n'.repeat(120000)+chapter+'\n// retained tail';
  longSource.value=originalSource; document.body.append(longSource);
  const chapterStart=originalSource.indexOf(chapter);
  longSource.setSelectionRange(chapterStart,chapterStart+chapter.length);
  const sourceExcerpt=controls().find(c=>c.label==='Long editable recipe');
  check(sourceExcerpt.value.length<=4096&&sourceExcerpt.value_truncated===true&&sourceExcerpt.value_length===originalSource.length,'Large source inspection must be bounded and explicitly identified as an excerpt');
  check(sourceExcerpt.value===originalSource.slice(sourceExcerpt.value_start,sourceExcerpt.value_start+sourceExcerpt.value.length)&&sourceExcerpt.value.includes(chapter),'The excerpt must describe the selected chapter using correct original offsets');
  check(sourceExcerpt.selection.start===chapterStart&&sourceExcerpt.selection.end===chapterStart+chapter.length&&longSource.value===originalSource,'Inspection must preserve source and its selection');
  longSource.value='x'.repeat(903)+'😀'+'z'.repeat(4094)+'!';
  longSource.setSelectionRange(longSource.value.length,longSource.value.length);
  const endingExcerpt=controls().find(c=>c.label==='Long editable recipe');
  check(endingExcerpt.value.length<=4096&&endingExcerpt.value.endsWith('!')&&!/^[\uDC00-\uDFFF]/.test(endingExcerpt.value),'A Unicode boundary near EOF must preserve the caret vicinity without splitting a surrogate pair');
  check(endingExcerpt.value===longSource.value.slice(endingExcerpt.value_start),'End-of-document excerpt offsets must identify the complete retained suffix');
  longSource.remove();
  list=controls();
  rejects(()=>operateUi({action:'click',target:list.find(c=>c.label==='Different action').id},context),/modal/);
  let accepted=false; modal.querySelector('button').onclick=()=>{accepted=true;};
  operateUi({action:'click',target:list.find(c=>c.label==='Accept').id},context);
  check(accepted,'Modal control did not execute');
  const playback=new SerialPlayback();
  const events=[]; let release;
  const first=playback.tick(async()=>{events.push('start');await new Promise(r=>release=r);events.push('finish');});
  check(await playback.tick(async()=>events.push('overlap'))===false,'Playback admitted overlapping work');
  release();await first;
  try{await playback.tick(async()=>{throw new Error('expected');});}catch{}
  check(await playback.tick(async()=>events.push('recovered')),'Playback remained wedged after failure');
  check(events.join(',')==='start,finish,recovered','Playback ordering changed');
  let finishEngine;
  const operation=trackEngineOperation(new Promise(resolve=>finishEngine=resolve));
  check(pendingEngineOperations()===1,'Engine work must remain pending until completion');
  finishEngine();await operation;
  try{await trackEngineOperation(Promise.reject(new Error('expected')));}catch{}
  check(pendingEngineOperations()===0,'Engine failure must release pending work');
  // Derived from the product configuration: new enabled commands automatically
  // enter this check, rather than requiring a copied inventory or count update.
  let commands=0;const actions=new Set();
  const ribbonTabs=[config.SOLID_TAB,config.SKETCH_TAB,config.DRAWING_TAB,config.ASSEMBLY_TAB,config.CAM_TAB,config.CAM_SIMULATE_TAB,config.CAM_OUTPUT_TAB];
  for(const tab of ribbonTabs) {
   for(const panel of tab.panels) {
    const group=interfaceGroups.find(g=>g.id===`${tab.id}/${panel.id}`);
    check(group&&JSON.stringify(group.operations)===JSON.stringify(panel.operations),'Renderer and API grouping drifted');
   }
  }
  const feedback=document.createElement('section');feedback.dataset.interfaceGroup=operationGroup('sketch_add_line');
  feedback.innerHTML='<button>Line</button>';document.body.append(feedback);
  check(inspectUi(context).surfaces.some(s=>s.name==='sketch/draw'),'Controls must use the product group');
  setPlaybackPace(0);await presentOperation('sketch_add_line');
  check(presentation.snapshot().operation==='add line','Operation feedback must remain readable until the next operation');
  check(!document.querySelector('[data-mcp-presentation]')&&feedback.getAnimations().length===0,'Feedback must not create flashing overlays or per-operation animations');
  feedback.remove();
  for(const tab of ribbonTabs){
   const walk=(entries)=>{for(const entry of entries){if(entry.type==='separator')continue;
    if(entry.enabled===true&&!entry.children){check(Boolean(entry.action),`Enabled UI command has no dispatch action: ${tab.id}/${entry.id}`);commands++;actions.add(entry.action);}
    if(entry.children)walk(entry.children);
   }};
   for(const panel of tab.panels){walk(panel.buttons);walk(panel.menu??[]);}
  }
  return {commands,actions:[...actions],checks:['grouping','hidden','native-disclosures','nested-disclosure-visibility','summary-activation','tab-selection','disabled','field-events','unavailable-option','stale-snapshot','document-change','recycled-control','modal','serial-order','failure-recovery','product-command-coverage']};
 });
 assert(result.commands>0);
 for(const action of result.actions) assert(dispatched.has(action), `Enabled ribbon action has no dispatcher case: ${action}`);
 console.log('PASS MCP UI contracts: '+JSON.stringify(result));
 console.log('PASS production dimension inputs: '+JSON.stringify(await checkDimensionInputs(browser, server.resolvedUrls.local[0]+'mcp-contract')));
 const exitPage=await browser.newPage();
 await exitPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const exit=await exitPage.evaluate(async()=>{
  const {checkApplicationExitEdits}=await import('/src/files/applicationExit.browser.test.ts');
  return checkApplicationExitEdits();
 });
 console.log('PASS production application exit: '+JSON.stringify(exit));
 await exitPage.close();
 const exitSavePage=await browser.newPage();
 await exitSavePage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const exitSavePolling=await exitSavePage.evaluate(async()=>{
  const {checkSaveOnExitPolling}=await import('/src/files/saveOnExit.browser.test.ts');
  return checkSaveOnExitPolling();
 });
 console.log('PASS production Save-on-exit polling: '+JSON.stringify(exitSavePolling));
 await exitSavePage.close();
 console.log('PASS production presentation surfaces: '+JSON.stringify(await checkPresentationSurfaces(browser, server.resolvedUrls.local[0]+'mcp-contract')));
 const settingsPage = await browser.newPage();
 try {
  await settingsPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
  await settingsPage.evaluate(async () => {
   const {mountSettingsContract} = await import('/src/components/AppearanceDialog.browser.test.tsx');
   window.settingsContract = mountSettingsContract();
  });
  await settingsPage.getByRole('button', {name: 'Settings opener'}).click();
  const settings = settingsPage.getByTestId('appearance-dialog');
  await settings.getByText(/0123456789abcdef0123456789abcdef01234567ab.*modified source/).waitFor();
  await settingsPage.waitForFunction(() => document.activeElement === document.querySelector('[data-settings-dialog] button'));
  console.log('PASS production Settings native history: '+JSON.stringify(await settingsPage.evaluate(() => window.settingsContract.checkNativeHistory())));
  await settingsPage.waitForFunction(() => document.activeElement === document.querySelector('[data-settings-dialog] button'));
  await settingsPage.keyboard.press('Shift+Tab');
  assert.equal(await settingsPage.evaluate(() => document.activeElement === document.querySelector('[data-settings-dialog] footer button')), true, 'Settings Shift+Tab stays inside the modal');
  await settingsPage.keyboard.press('Tab');
  assert.equal(await settingsPage.evaluate(() => document.activeElement === document.querySelector('[data-settings-dialog] button')), true, 'Settings Tab wraps to its first control');
  await settingsPage.keyboard.press('Escape');
  await settings.waitFor({state: 'detached'});
  assert.equal(await settingsPage.evaluate(() => window.settingsContract.modelEscapes()), 0, 'Closing Settings must not cancel CAD through its earlier capture listener');
  assert.equal(await settingsPage.evaluate(() => document.activeElement?.textContent), 'Settings opener');
  await settingsPage.getByRole('button', {name: 'Settings opener'}).click();
  await settings.waitFor();
  await settingsPage.evaluate(() => {
   document.querySelector('[data-settings-dialog] button').click();
   document.querySelector('[aria-label="Newer focus"]').focus();
  });
  await settings.waitFor({state: 'detached'});
  assert.equal(await settingsPage.evaluate(() => document.activeElement?.getAttribute('aria-label')), 'Newer focus', 'Settings dismissal preserves newer focus');
  await settingsPage.keyboard.press('Escape');
  assert.equal(await settingsPage.evaluate(() => window.settingsContract.modelEscapes()), 1, 'CAD keys resume after Settings closes');
  console.log('PASS production Settings modal: full build identity, initial focus, Tab/Shift+Tab trap, Escape ownership, focus restoration and newer focus');
 } finally { await settingsPage.close(); }
 const scriptPage=await browser.newPage();
 await scriptPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const scripts=await scriptPage.evaluate(async()=>{
  const {checkScriptSourceOwnership}=await import('/src/scripts/workspace.browser.test.ts');
  let timer;
  try{return await Promise.race([checkScriptSourceOwnership(),new Promise((_,reject)=>{
   timer=setTimeout(()=>reject(new Error('Script workspace contract timed out')),15000);
  })]);}finally{clearTimeout(timer);}
 });
 console.log('PASS production script source lifecycle: '+JSON.stringify(scripts));
 const nativePreview=await scriptPage.evaluate(async()=>{
  const {checkNativeScriptPreview}=await import('/src/scripts/preview.browser.test.ts');
  return checkNativeScriptPreview();
 });
 console.log('PASS production native script preview: '+JSON.stringify(nativePreview));
 await scriptPage.close();
 const navigationPage=await browser.newPage();
 try {
  await navigationPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
  await navigationPage.evaluate(async()=>{
   const {mountNativePreviewNavigation}=await import('/src/scripts/preview.browser.test.ts');
   window.unmountPreviewNavigation=mountNativePreviewNavigation();
  });
  await navigationPage.getByRole('button',{name:'Fillet',exact:true}).focus();
  const card=navigationPage.getByRole('dialog',{name:'Fillet example'});
  await card.waitFor();
  await card.getByText('2/2',{exact:true}).waitFor();
  const model=card.getByRole('img');
  await model.focus();
  await navigationPage.keyboard.press('Home');
  await card.getByRole('button',{name:'Previous preview step'}).click();
  await card.getByText('1/2',{exact:true}).waitFor();
  await navigationPage.mouse.move(800,500);
  await navigationPage.waitForTimeout(200);
  assert.equal(await card.count(),1,'Reaching the first frame must not blur a disabled Previous button and dismiss its parent');
  assert.equal(await model.evaluate(element=>document.activeElement===element),true,'Boundary navigation transfers focus into model inspection');
  await card.getByRole('button',{name:'Next preview step'}).focus();
  await navigationPage.keyboard.press('Enter');
  await card.getByText('2/2',{exact:true}).waitFor();
  await navigationPage.waitForTimeout(200);
  assert.equal(await card.count(),1,'Keyboard navigation to the last frame must preserve the parent card');
  assert.equal(await model.evaluate(element=>document.activeElement===element),true);
  await navigationPage.evaluate(async()=>{
   const {inspectUi,operateUi}=await import('/src/uiControl.ts');
   const previous=inspectUi().surfaces.flatMap(surface=>surface.controls).find(control=>control.label==='Previous preview step');
   if (!previous) throw new Error('MCP must expose the ordinary preview navigation control');
   operateUi({action:'click',target:previous.id});
  });
  await card.getByText('1/2',{exact:true}).waitFor();
  await navigationPage.waitForTimeout(200);
  assert.equal(await model.evaluate(element=>document.activeElement===element),true,'MCP navigation shares the focus-safe boundary action');
  const replay=card.getByRole('button',{name:'Replay feature preview'});
  await replay.click();
  await card.getByText('1/2',{exact:true}).waitFor();
  await card.getByText('2/2',{exact:true}).waitFor();
  assert.equal(await replay.evaluate(element=>document.activeElement===element),true,'Passive autoplay must preserve the chosen focus');
  console.log('PASS production preview boundary navigation: pointer Previous, keyboard Next, MCP click, parent dismissal timers, passive focus');
 } finally {
  await navigationPage.evaluate(()=>window.unmountPreviewNavigation?.());
  await navigationPage.close();
 }
 const roundedThreadPage=await browser.newPage();
 await roundedThreadPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const roundedThreads=await roundedThreadPage.evaluate(async()=>{
  const {checkRoundedThreadEditors}=await import('/src/engine/roundedThread.browser.test.ts');
  return checkRoundedThreadEditors();
 });
 console.log('PASS production rounded-thread edit preservation: '+JSON.stringify(roundedThreads));
 await roundedThreadPage.close();
 const recoveryPage=await browser.newPage();
 await recoveryPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const recovery=await recoveryPage.evaluate(async()=>{
  const {checkProjectLoadRecovery}=await import('/src/files/projectFiles.browser.test.ts');
  let timer;
  try{return await Promise.race([checkProjectLoadRecovery(),new Promise((_,reject)=>{
   timer=setTimeout(()=>reject(new Error('Project recovery timed out: '+document.body.innerHTML)),15000);
  })]);}finally{clearTimeout(timer);}
 });
 console.log('PASS production project-open/export recovery: '+JSON.stringify(recovery));
 await recoveryPage.close();
 const consentPage=await browser.newPage();
 await consentPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const consent=await consentPage.evaluate(async()=>{
  const {checkOpenConsentOwnership}=await import('/src/files/openConsent.browser.test.ts');
  return checkOpenConsentOwnership();
 });
 console.log('PASS production Open consent ownership: '+JSON.stringify(consent));
 await consentPage.close();
 const namingPage=await browser.newPage();
 await namingPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const naming=await namingPage.evaluate(async()=>{
  const {checkOpenedProjectNaming}=await import('/src/files/projectFiles.browser.test.ts');
  return checkOpenedProjectNaming();
 });
 console.log('PASS production Open naming: '+JSON.stringify(naming));
 await namingPage.close();
 const stepOpenPage=await browser.newPage();
 await stepOpenPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const stepOpen=await stepOpenPage.evaluate(async()=>{
  const {checkOpenedStepProject}=await import('/src/files/projectFiles.browser.test.ts');
  return checkOpenedStepProject();
 });
 console.log('PASS production Open STEP as project: '+JSON.stringify(stepOpen));
 await stepOpenPage.close();
 const openFramingPage=await browser.newPage();
 const openFramingErrors=[];
 openFramingPage.on('pageerror',error=>openFramingErrors.push(error.message));
 await openFramingPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const openFraming=await openFramingPage.evaluate(async()=>{
  const {checkOpenedProjectFraming}=await import('/src/files/openProjectFraming.browser.test.tsx');
  return checkOpenedProjectFraming();
 });
 assert.deepEqual(openFramingErrors,[],'Open framing must not leave asynchronous errors');
 console.log('PASS production Open framing: '+JSON.stringify(openFraming));
 await openFramingPage.close();
 const tabCameraPage=await browser.newPage();
 const tabCameraErrors=[];
 tabCameraPage.on('pageerror',error=>tabCameraErrors.push(error.message));
 await tabCameraPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const tabCameras=await tabCameraPage.evaluate(async()=>{
  const {checkProjectTabCameras}=await import('/src/files/projectTabCamera.browser.test.tsx');
  return checkProjectTabCameras();
 });
 assert.deepEqual(tabCameraErrors,[],'Per-tab cameras must not leave asynchronous errors');
 console.log('PASS production per-tab cameras: '+JSON.stringify(tabCameras));
 await tabCameraPage.close();
 const stepPage=await browser.newPage();
 await stepPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const step=await stepPage.evaluate(async()=>{
  const {checkStepExportOwnership}=await import('/src/files/projectFiles.browser.test.ts');
  return checkStepExportOwnership();
 });
 console.log('PASS production STEP export ownership: '+JSON.stringify(step));
 const browserStep=await stepPage.evaluate(async()=>{
  const {checkBrowserStepExportOwnership}=await import('/src/engine/stepExport.browser.test.ts');
  return checkBrowserStepExportOwnership();
 });
 console.log('PASS browser STEP adapter ownership: '+JSON.stringify(browserStep));
 await stepPage.close();
 const savePage=await browser.newPage();
 await savePage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const saveOwnership=await savePage.evaluate(async()=>{
  const {checkProjectSaveOwnership}=await import('/src/files/projectSave.browser.test.ts');
  return await checkProjectSaveOwnership();
 });
 console.log('PASS production project Save ownership: '+JSON.stringify(saveOwnership));
 await savePage.close();
 const historyPage=await browser.newPage();
 await historyPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const historyMetadata=await historyPage.evaluate(async()=>{
  const {checkHistoryMetadata}=await import('/src/engine/historyMetadata.browser.test.ts');
  let timer;
  try{return await Promise.race([checkHistoryMetadata(),new Promise((_,reject)=>{
   timer=setTimeout(()=>reject(new Error('History metadata contract timed out')),15000);
  })]);}finally{clearTimeout(timer);}
 });
 console.log('PASS production history metadata: '+JSON.stringify(historyMetadata));
 const historyEditors=await historyPage.evaluate(async()=>{
  const {checkHistoryEditorCallbacks}=await import('/src/engine/historyEditor.browser.test.ts');
  return checkHistoryEditorCallbacks();
 });
 console.log('PASS production history editor callbacks: '+JSON.stringify(historyEditors));
 await historyPage.close();
 for(const phase of ['before-bind','during-bind']) {
  const startupPage=await browser.newPage();
  try {
   await startupPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
   const startup=await startupPage.evaluate(async phase=>{
    const {checkStartupPublication}=await import('/src/scripts/startupPublication.browser.test.ts');
    let timer;
    try { return await Promise.race([checkStartupPublication(phase),new Promise((_,reject)=>{
     timer=setTimeout(()=>reject(new Error('Startup publication contract timed out')),15000);
    })]); } finally { clearTimeout(timer); }
   },phase);
   console.log('PASS production startup publication: '+JSON.stringify(startup));
  } finally { await startupPage.close(); }
 }
 const ownershipPage=await browser.newPage();
 await ownershipPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const ownership=await ownershipPage.evaluate(async()=>{
  const {checkScriptDocumentOwnership}=await import('/src/scripts/documentOwnership.browser.test.ts');
  const {checkInboxDocumentOwnership}=await import('/src/scripts/inboxOwnership.browser.test.ts');
  const {checkPublicationOwnership}=await import('/src/scripts/publicationOwnership.browser.test.ts');
  const {checkInboxCompletion}=await import('/src/scripts/inboxCompletion.browser.test.ts');
  const {checkInboxHistoryMetadata}=await import('/src/scripts/inboxHistory.browser.test.ts');
  const {checkInboxProgress}=await import('/src/scripts/inboxProgress.browser.test.ts');
  let timer;
  try{return await Promise.race([(async()=>({
   script:await checkScriptDocumentOwnership(),inbox:await checkInboxDocumentOwnership(),publication:await checkPublicationOwnership(),completion:await checkInboxCompletion(),history:await checkInboxHistoryMetadata(),progress:await checkInboxProgress(),
  }))(),new Promise((_,reject)=>{
   timer=setTimeout(()=>reject(new Error('Script document ownership contract timed out')),15000);
  })]);}finally{clearTimeout(timer);}
 });
 console.log('PASS production script document ownership: '+JSON.stringify(ownership));
 await ownershipPage.close();
 const camOwnershipPage=await browser.newPage();
 await camOwnershipPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const camOwnership=await camOwnershipPage.evaluate(async()=>{
  const {checkCamDocumentOwnership}=await import('/src/cam/documentOwnership.browser.test.ts');
  let timer;
  try{return await Promise.race([checkCamDocumentOwnership(),new Promise((_,reject)=>{
   timer=setTimeout(()=>reject(new Error('CAM document ownership contract timed out')),15000);
  })]);}finally{clearTimeout(timer);}
 });
 console.log('PASS production CAM document ownership: '+JSON.stringify(camOwnership));
 await camOwnershipPage.close();
 const namedViewPage=await browser.newPage();
 try {
  await namedViewPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
  const namedViews=await namedViewPage.evaluate(async()=>{
   const {checkNamedViewOwnership}=await import('/src/namedViews.browser.test.ts');
   const {checkNamedViewTabEviction}=await import('/src/namedViewTabs.browser.test.ts');
   const {checkNamedViewHistory}=await import('/src/namedViewHistory.browser.test.ts');
   const {checkNamedViewMcpCapture}=await import('/src/namedViewMcp.browser.test.ts');
   return {ownership:await checkNamedViewOwnership(),eviction:await checkNamedViewTabEviction(),history:await checkNamedViewHistory(),mcpCapture:await checkNamedViewMcpCapture()};
  });
  console.log('PASS named view publication and tab retention: '+JSON.stringify(namedViews));
 } finally { await namedViewPage.close(); }
 const drawingPublicationPage=await browser.newPage();
 await drawingPublicationPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const drawingPublication=await drawingPublicationPage.evaluate(async()=>{
  const {checkDrawingProjectionPublication}=await import('/src/drawing/projectionPresentation.browser.test.tsx');
  return checkDrawingProjectionPublication();
 });
 console.log('PASS production drawing projection publication: '+JSON.stringify(drawingPublication));
 await drawingPublicationPage.close();
 const drawingFitPage=await browser.newPage({viewport:{width:1280,height:720}});
 try {
  await drawingFitPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
  const drawingFit=await drawingFitPage.evaluate(async()=>{
   const {checkDrawingSheetFit}=await import('/src/drawing/sheetFit.browser.test.tsx');
   return checkDrawingSheetFit();
  });
  console.log('PASS production drawing sheet fit: '+JSON.stringify(drawingFit));
 } finally { await drawingFitPage.close(); }
 const titleInputs=[];
 for(const recipe of ['garden-bench','d-screw-vise','vertical-axis-turbine','turbine-fit-coupons']) {
  const source=await readFile(new URL(`../../examples/scripts/${recipe}.nbcad.jsonc`,import.meta.url),'utf8');
  const script=JSON.parse(source.replace(/^\s*\/\/.*$/gm,''));
  for(const step of script.steps)if(step.call?.operation==='drawing_create_sheet')titleInputs.push({recipe,arguments:step.call.arguments});
 }
 const titlePage=await browser.newPage({viewport:{width:1280,height:800}});
 try {
  await titlePage.goto(server.resolvedUrls.local[0]+'mcp-contract');
  const titleBlocks=await titlePage.evaluate(async inputs=>{
   const {checkDrawingTitleBlocks}=await import('/src/drawing/titleBlock.browser.test.tsx');
   return checkDrawingTitleBlocks(inputs);
  },titleInputs);
  console.log('PASS production drawing title blocks: '+JSON.stringify(titleBlocks));
 }finally{await titlePage.close();}
 const controlPage=await browser.newPage();
 await controlPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 const controls=await controlPage.evaluate(async()=>{
  const {checkControlDocumentOwnership}=await import('/src/scripts/controlOwnership.browser.test.ts');
  return checkControlDocumentOwnership();
 });
 console.log('PASS production control document ownership: '+JSON.stringify(controls));
 await controlPage.close();
 const palettePage=await browser.newPage();
 await palettePage.goto(server.resolvedUrls.local[0]+'mcp-contract');
 await palettePage.evaluate(async()=>{
  const {mountSketchPaletteContract}=await import('/src/components/SketchPalette.browser.test.tsx');
  window.paletteContract=mountSketchPaletteContract();
 });
 await palettePage.getByRole('checkbox',{name:'Sketch Grid',exact:true}).waitFor();
 const palette=await palettePage.evaluate(async()=>{
  const {inspectUi,operateUi}=await import('/src/uiControl.ts');
  const snapshot=inspectUi();
  const controls=snapshot.surfaces.flatMap(surface=>surface.controls);
  const toggles=controls.filter(control=>control.role==='checkbox');
  if(snapshot.unlabeled_controls.length||toggles.some(control=>typeof control.value!=='boolean'))
   throw new Error('Every palette checkbox must expose its label and actual state');
  const grid=toggles.find(control=>control.label==='Sketch Grid');
  if(!grid||grid.value!==true)throw new Error('Initial grid state missing');
  operateUi({action:'click',target:grid.id});
  const disabled=toggles.find(control=>control.disabled);
  if(!disabled)throw new Error('Unsupported options must stay disabled');
  let blocked=false;
  try{operateUi({action:'click',target:disabled.id});}catch(error){blocked=/disabled/.test(String(error));}
  if(!blocked)throw new Error('MCP accepted a disabled palette control');
  return {toggles:toggles.length,grid:window.paletteContract.grid()};
 });
 assert.equal(palette.grid,false,'MCP click must update the real grid preference');
 const gridCheckbox=palettePage.getByRole('checkbox',{name:'Sketch Grid',exact:true});
 await gridCheckbox.focus();
 await palettePage.keyboard.press('Space');
 await palettePage.waitForFunction(()=>window.paletteContract.grid()===true);
 assert.equal(await gridCheckbox.isChecked(),true,'Keyboard and MCP must share the checked state');
 await palettePage.keyboard.press('Control+s');
 await palettePage.keyboard.press('Control+o');
 await palettePage.keyboard.press('Control+n');
 assert.deepEqual(await palettePage.evaluate(()=>window.paletteContract.fileCommands()),['save','open','new'],
  'Focused native palette checkbox must retain application File shortcut routing');
 const paletteHistory=await palettePage.evaluate(()=>window.paletteContract.checkNativeHistory());
 await palettePage.evaluate(()=>window.paletteContract.unmount());
 await palettePage.close();
 console.log('PASS production sketch palette: '+JSON.stringify(palette));
 console.log('PASS focused palette keyboard and native history routing: '+JSON.stringify(paletteHistory));
 const ribbonPage=await browser.newPage({viewport:{width:900,height:700}});
 try {
  await ribbonPage.goto(server.resolvedUrls.local[0]+'mcp-contract');
  await ribbonPage.evaluate(async()=>{
   const {mountRibbonMenuContract}=await import('/src/components/RibbonMenu.browser.test.tsx');
   window.ribbonMenuContract=mountRibbonMenuContract();
  });
  const flyout=ribbonPage.locator('[data-ribbon-flyout]');
  await ribbonPage.locator('[data-ribbon-menu-id="rectangle"]').hover();
  await flyout.waitFor({state:'visible'});
  const flyoutIsland=await ribbonPage.evaluate(async()=>{
   const {collectNativeViewportOverlayRects}=await import('/src/components/viewport/nativeViewportBridge.ts');
   const panel=document.querySelector('[data-ribbon-flyout]').getBoundingClientRect();
   const menu=document.querySelector('[role="menu"]').getBoundingClientRect();
   const overlap=(a,b)=>Math.max(0,Math.min(a.x+a.width,b.x+b.width)-Math.max(a.x,b.x))
    *Math.max(0,Math.min(a.y+a.height,b.y+b.height)-Math.max(a.y,b.y));
   const islands=collectNativeViewportOverlayRects();
   return {
    overflows:panel.left>=menu.right-2,
    hit:Boolean(document.elementFromPoint(panel.right-24,panel.top+16)?.closest('[data-ribbon-flyout]')),
    covered:islands.reduce((sum,island)=>sum+overlap(panel,island),0)/(panel.width*panel.height),
   };
  });
  assert.equal(flyoutIsland.overflows,true,'The flyout must overflow the portaled menu box that the native cut-out used to mask');
  assert.equal(flyoutIsland.hit,true,'A point beyond the menu box must hit-test to the flyout');
  assert.ok(flyoutIsland.covered>0.99,`Native overlay islands cover only ${(flyoutIsland.covered*100).toFixed(1)}% of the flyout`);
  await ribbonPage.mouse.move(20,600);
  await ribbonPage.waitForTimeout(400);
  assert.equal(await flyout.count(),0,'Leaving the row must unmount the flyout so its island is released with it');
  console.log('PASS production ribbon flyout island: '+JSON.stringify(flyoutIsland));
  const row=id=>ribbonPage.locator(`[data-ribbon-menu-id="${id}"]`);
  const activeRow=()=>ribbonPage.evaluate(()=>document.activeElement?.getAttribute('data-ribbon-menu-id'));
  const flyoutRows=()=>ribbonPage.evaluate(()=>[...document.querySelectorAll('[data-ribbon-flyout] [data-ribbon-menu-id]')]
   .map(element=>element.getAttribute('data-ribbon-menu-id')));
  const tabUntil=async id=>{
   for(let attempt=0;attempt<8;attempt++){
    if(await activeRow()===id)return true;
    await ribbonPage.keyboard.press('Tab');
   }
   return await activeRow()===id;
  };
  // Keyboard focus enters, owns and releases the panel it opened.
  await row('rectangle').focus();
  await ribbonPage.keyboard.press('ArrowRight');
  await flyout.waitFor({state:'visible'});
  assert.equal(await tabUntil('rectCenter'),true,'Tab must enter the open submenu');
  await ribbonPage.keyboard.press('Escape');
  assert.equal(await flyout.count(),0,'Escape from a child must close the submenu');
  assert.equal(await ribbonPage.locator('[role="menu"]').count(),1,'Dismissing a submenu must keep the parent menu open');
  assert.equal(await activeRow(),'rectangle','Escape from a child must restore focus to its parent row');
  await ribbonPage.keyboard.press('ArrowRight');
  await flyout.waitFor({state:'visible'});
  assert.equal(await tabUntil('circle'),true,'Tab must leave the submenu for the next parent row');
  assert.equal(await flyout.count(),0,'Focus leaving a submenu must dismiss the panel it left');
  await ribbonPage.keyboard.press('ArrowRight');
  await flyout.waitFor({state:'visible'});
  assert.deepEqual(await flyoutRows(),['circleCenter','circle2pt','circle3pt'],'The focused row owns the only open panel');
  // A panel whose commands hold focus survives the pointer crossing away.
  await row('arc').hover();
  await flyout.waitFor({state:'visible'});
  await ribbonPage.locator('[data-ribbon-flyout] [data-ribbon-menu-id="arc3pt"]').focus();
  await ribbonPage.mouse.move(20,600);
  await ribbonPage.waitForTimeout(400);
  assert.equal(await flyout.count(),1,'A panel that owns focus must survive pointer leave');
  assert.equal(await activeRow(),'arc3pt','Pointer leave must not drop focused commands');
  console.log('PASS production ribbon flyout focus ownership: '+JSON.stringify({
   focusedMenu:await flyoutRows(),focus:await activeRow(),
  }));
 } finally {
  await ribbonPage.evaluate(()=>window.ribbonMenuContract?.unmount());
  await ribbonPage.close();
 }
} finally {await browser?.close();await server.close();}
