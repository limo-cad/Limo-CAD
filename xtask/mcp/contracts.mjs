import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {chromium} from 'playwright';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';

const dispatcher=ts.createSourceFile('dispatch.ts',await readFile(new URL('../../src/ribbon/dispatch.ts',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true);
const dispatched=new Set();
function visit(node){
 if(ts.isSwitchStatement(node)&&node.expression.getText(dispatcher)==='action') {
  for(const clause of node.caseBlock.clauses) if(ts.isCaseClause(clause)&&ts.isStringLiteral(clause.expression)) dispatched.add(clause.expression.text);
 }
 ts.forEachChild(node,visit);
}
visit(dispatcher);

const server=await createServer({configFile:false,optimizeDeps:{noDiscovery:true,entries:[],include:['react','react-dom/client','react/jsx-runtime','react/jsx-dev-runtime']},server:{host:'127.0.0.1',port:0},logLevel:'error',plugins:[{name:'mcp-contract',configureServer(server){server.middlewares.use('/mcp-contract',(_req,res)=>{
 res.setHeader('Content-Type','text/html');
 res.end('<!doctype html><html><body><main data-mcp-surface="test-surface"><button>Run</button><button disabled>Disabled</button><label>Name<input value="old"></label><label>Choice<select><option value="a">A</option><option disabled value="b">B</option></select></label><button id="hidden" hidden>Hidden</button></main></body></html>');
});}}]});
let browser;
try {
 await server.listen();
 browser=await chromium.launch({headless:true});
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
  const modal=document.createElement('div'); modal.setAttribute('aria-modal','true'); modal.innerHTML='<button>Accept</button>'; document.body.append(modal);
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
  for(const tab of [config.SOLID_TAB,config.SKETCH_TAB,config.DRAWING_TAB,config.ASSEMBLY_TAB]) {
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
  for(const tab of [config.SOLID_TAB,config.SKETCH_TAB,config.DRAWING_TAB,config.ASSEMBLY_TAB]){
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
 await scriptPage.close();
} finally {await browser?.close();await server.close();}


