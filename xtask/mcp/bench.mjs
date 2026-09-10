import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {Client} from '../../mcp-server/client.mjs';
import {workshop} from './workshop.mjs';

const option=name=>process.argv.includes(name)?process.argv[process.argv.indexOf(name)+1]:undefined;
assert(option('--server'),'cargo xtask test-mcp bench --server MCP_EXE [--session UUID] [--out DIRECTORY]');
const client=new Client(option('--server'));
let session=option('--session');
const report={name:'Garden workshop bench',calls:[],parts:[],checks:[]};
let tools;
async function call(name,args={}) {
  const start=performance.now();let result;
  const operation=tools?.find(t=>t.name===name);
  if(operation&&name!=='cad_interface') {
    result=await client.call('cad_interface',{action:'execute',group:operation.group,operation:name,arguments:args});
  } else result=await client.call(name,args);
  report.calls.push({name,arguments:args,elapsed_ms:Math.round(performance.now()-start)});
  return result;
}
const model=async()=>JSON.parse(await call('cad_project_model'));
const scene=()=>call('solid_scene');
async function board(name,width,depth,height) {
  const before=await scene();
  await call('sketch_begin',{plane:{type:'origin_plane',plane:'xy'}});
  await call('sketch_add_rectangle_locked',{mode:'two_point',anchor:{x:0,y:0},corner_hint:{x:width,y:depth},width_mm:width,height_mm:depth,ctrl_held:true});
  await call('sketch_finish');
  const sketch=(await model()).sketches.at(-1).name;
  await call('solid_extrude',{sketch_name:sketch,profile_indices:[0],operation:'new_body',extent:{type:'distance',distance:height},taper_angle_deg:0,flip:false,target_body_ids:[]});
  const body=(await scene()).bodies.find(b=>!before.bodies.some(old=>old.id===b.id));
  assert(body,`${name} must create a body`);
  await call('assembly_create_component',{name,body_ids:[body.id],absorb_promoted_bodies:true});
  const assembly=await call('assembly_document');
  const component=assembly.component_structure.definitions.find(c=>c.name===name);
  const occurrence=assembly.component_structure.occurrences.find(o=>o.component_id===component.id);
  const part={name,width,depth,height,body,component,occurrence};report.parts.push({name,width,depth,height});return part;
}
function connector(part,top,origin) {
  const face=part.body.faces.find(f=>f.plane&&f.plane.normal[2]*(top?1:-1)>0.99);
  assert(face,`${part.name} horizontal connector face`);
  return {body_id:part.body.id,face_id:face.id,face_key:face.key,kind:'planar_face',
    frame:{origin,primary_axis:face.plane.normal,secondary_axis:[1,0,0]},
    source_surface_frame:{origin:face.plane.origin,primary_axis:face.plane.normal,secondary_axis:face.plane.u}};
}
try {
  await client.start();tools=await client.call('cad_list_all_tools');
  if(option('--desktop')) {
    assert(!session,'Choose --desktop or --session');
    const launched=await call('cad_interface',{action:'launch',executable:option('--desktop')});
    assert.equal(launched.status,'ready',JSON.stringify(launched));
    session=launched.session_id;report.session_id=session;report.pid=launched.pid;
    console.log('Live bench session',session);
  }
  if(session){await client.call('cad_attach',{session_id:session});await call('cad_interface',{action:'inspect',session_id:session,pace_ms:Number(option('--pace')??0)});}
  assert.equal((await model()).document.history.features.length,0,'Use a new empty document for the bench');
  if(option('--workshop')==='all') await workshop(call,JSON.stringify(await model()),report);
  const slat=await board('Seat slat',1200,85,35);
  await call('assembly_set_occurrence_pose',{occurrence_id:slat.occurrence.id,local_pose:{translation:[0,0,415],rotation:[0,0,0,1]}});
  await call('assembly_set_occurrence_grounded',{occurrence_id:slat.occurrence.id,grounded:true});
  let jointCount=0;const expectedPoses=[];slat.instances=[slat.occurrence];
  async function place(part,positions,topOfPart,sameFace=false,skipFirst=false){
    for(const [i,[x,y]] of positions.entries()) {
      if(skipFirst&&i===0)continue;
      let occurrence=part.occurrence;
      if(i>0) {
        const before=await call('assembly_document');
        await call('assembly_create_occurrence',{component_id:part.component.id,name:`${part.name} ${i+1}`});
        occurrence=(await call('assembly_document')).component_structure.occurrences.find(o=>!before.component_structure.occurrences.some(old=>old.id===o.id));
      }
      await call('assembly_create_joint',{name:`${part.name} mount ${i+1}`,kind:'rigid',
        connector_a:connector(slat,sameFace,[x,y,sameFace?35:0]),connector_b:connector(part,true,[0,0,part.height]),
        flipped:sameFace,
        grounded_occurrence_id:slat.occurrence.id,
        advanced:{connector_a_occurrence_id:slat.occurrence.id,connector_b_occurrence_id:occurrence.id}});
      jointCount++;
      if(part===slat)slat.instances.push(occurrence);
      expectedPoses.push({occurrence_id:occurrence.id,translation:[x,y,topOfPart-part.height]});
    }
  }
  await place(slat,[[0,0],[0,90],[0,180],[0,270],[0,360]],450,true,true);
  const leg=await board('Leg',60,60,415);await place(leg,[[60,35],[1080,35],[60,340],[1080,340]],415);
  const apron=await board('Long apron',1080,25,90);await place(apron,[[60,50],[60,365]],415);
  const rail=await board('Side rail',25,305,90);await place(rail,[[78,60],[1097,60]],415);
  // Attach posts to the rear slat and boards to a post using real vertical
  // faces. Rigid joints have no motion coordinate; linear_offset_mm belongs
  // to movable joints and cannot serve as an assembly placement shortcut.
  function side(part,back,origin) {
    const face=part.body.faces.find(f=>f.plane&&f.plane.normal[1]*(back?1:-1)>0.99);
    assert(face);
    return {body_id:part.body.id,face_id:face.id,face_key:face.key,kind:'planar_face',
      frame:{origin,primary_axis:face.plane.normal,secondary_axis:[1,0,0]},
      source_surface_frame:{origin:face.plane.origin,primary_axis:face.plane.normal,secondary_axis:face.plane.u}};
  }
  async function mount(parent,parentOccurrence,part,occurrence,a,b,flipped,translation) {
    await call('assembly_create_joint',{name:`${part.name} mount ${occurrence.id}`,kind:'rigid',
      connector_a:side(parent,true,a),connector_b:side(part,flipped,b),flipped,
      advanced:{connector_a_occurrence_id:parentOccurrence.id,connector_b_occurrence_id:occurrence.id}});
    jointCount++;expectedPoses.push({occurrence_id:occurrence.id,translation});
  }
  const post=await board('Back post',50,40,740);
  for(const [i,x] of [65,1085].entries()) {
    let occurrence=post.occurrence;
    if(i){
      await call('assembly_create_occurrence',{component_id:post.component.id,name:'Back post 2'});
      occurrence=(await call('assembly_document')).component_structure.occurrences.at(-1);
    }
    await mount(slat,slat.instances.at(-1),post,occurrence,[x,85,0],[0,40,415],true,[x,405,0]);
  }
  for(const [name,z] of [['Back board',550],['Upper back board',655]]) {
    const part=await board(name,1200,25,85);
    await mount(post,post.occurrence,part,part.occurrence,[0,40,z],[65,0,0],false,[0,445,z]);
  }
  const assembly=await call('assembly_document');const solved=await call('assembly_solution');
  assert.equal(solved.solved,true);assert.equal(solved.diagnostics.length,0,JSON.stringify(solved.diagnostics));
  assert.equal(assembly.joints.length,jointCount);
  assert.equal(assembly.component_structure.occurrences.length,jointCount+1);
  for(const expected of expectedPoses){
    const pose=solved.instance_body_poses.find(p=>p.occurrence_id===expected.occurrence_id);
    assert(pose,`Missing instance pose ${expected.occurrence_id}`);
    assert(pose.translation.every((n,i)=>Math.abs(n-expected.translation[i])<1e-5),`Misplaced bench part: ${JSON.stringify({pose,expected})}`);
  }
  const original=await model();assert(original.sketches.length===report.parts.length);
  assert(!original.document.history.features.some(f=>f.kind==='import_step'));
  await call('solid_recompute');
  assert.equal((await scene()).errors.length,0);
  report.checks.push('native sketch/extrude provenance','repeated component occurrences','rigid joint solution','history replay');
  if(option('--workshop')==='all') {
    const called=new Set(report.calls.map(c=>c.name));
    const required=tools.filter(t=>t.group.startsWith('sketch/') ||
      (t.mutates&&['solid/build','solid/refine','solid/repeat','solid/body'].includes(t.group)));
    const missing=required.filter(t=>!called.has(t.name)).map(t=>t.name);
    report.coverage={required:required.length,executed:required.length-missing.length,missing};
    // Discovery is informational: new tools may have focused native Rust
    // regressions or recipes without duplicating them in this legacy workshop.
    // Every executed scenario above still asserts its geometry and edit behavior.
  }
  if(session) {
    assert.equal((await call('cad_interface',{action:'view',session_id:session,view:'isometric',fit:true})).status,'applied');
    if(option('--save')) assert.equal((await call('cad_interface',{action:'file',session_id:session,command:'save',path:resolve(option('--save'))})).status,'applied');
  }
  if(option('--out')) {
    const out=resolve(option('--out'));await mkdir(out,{recursive:true});
    await writeFile(resolve(out,'bench.model.json'),JSON.stringify(await model(),null,2));
  }
  console.log(`PASS bench: ${report.parts.length} native parts, ${jointCount+1} occurrences, ${jointCount} rigid joints`);
  report.status='passed';
} catch(error) {
  report.status='failed';report.error=String(error);throw error;
} finally {
  client.close();
  if(option('--out')) {
    const out=resolve(option('--out'));await mkdir(out,{recursive:true});
    await writeFile(resolve(out,'bench-report.json'),JSON.stringify(report,null,2));
  }
}
