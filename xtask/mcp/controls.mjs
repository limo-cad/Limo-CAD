import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {Client} from '../../mcp-server/client.mjs';

const args=process.argv.slice(2);
const option=name=>args.includes(name)?args[args.indexOf(name)+1]:undefined;
const binary=option('--server');
assert(binary,'Use --server /path/to/nbcad-mcp --session UUID [--model model.json] [--out report.json]');
const c=new Client(binary);
const report={views:[],operations:[]};
try {
 await c.start();
 const id=option('--session');
 assert(id,'Use --session UUID for an active disposable test document');
 await c.call('cad_attach',{session_id:id});
 const generation=async()=> (await c.call('cad_list_sessions')).session_details.find(s=>s.session_id===id).heartbeat.generation;
 const mutate=async(name,args)=>{
  const submitted=await c.call('cad_submit',{name,arguments:args,base_generation:await generation()});
  const applied=await c.call('cad_await_apply',{seq:submitted.seq,timeout_ms:10000});
  assert.equal(applied.status,'applied',JSON.stringify(applied));
  await c.call('cad_refresh');
  report.operations.push({name,status:applied.status});
 };
 if (option('--model')) await mutate('cad_load_project_model',{model_json:await readFile(option('--model'),'utf8')});
 const original=await c.call('cad_project_model');
 const beforeGeneration=await generation();
 const home=await c.call('cad_interface',{action:'view',view:'isometric',fit:false});
 const fitted=await c.call('cad_interface',{action:'view',view:'current',fit:true});
 assert.equal(home.status,'applied');assert.equal(fitted.status,'applied');
 for(const key of ['position','target']) for(let axis=0;axis<3;axis++)
  assert(Math.abs(home.camera[key][axis]-fitted.camera[key][axis])<1e-6,'ISO must already frame the visible geometry without a second Fit');

 const directions={top:[0,0,1],bottom:[0,0,-1],front:[0,-1,0],back:[0,1,0],left:[-1,0,0],right:[1,0,0]};
 for (const view of ['current',...Object.keys(directions),'isometric']) {
  const result=await c.call('cad_interface',{action:'view',view,fit:view!=='current'});
  assert.equal(result.status,'applied',JSON.stringify(result));
  assert(result.camera.position.every(Number.isFinite));
  if(directions[view]) {
   const offset=result.camera.position.map((n,i)=>n-result.camera.target[i]);
   const length=Math.hypot(...offset);
   directions[view].forEach((n,i)=>assert(Math.abs(offset[i]/length-n)<1e-5,JSON.stringify(result)));
  }
  assert.equal(await generation(),beforeGeneration,'Camera changed engine generation');
  report.views.push({view,...result});
  console.log('PASS view',view);
 }
 await c.call('cad_refresh');
 assert.deepEqual(JSON.parse(await c.call('cad_project_model')),JSON.parse(original),'Camera changed model');
 const base=JSON.parse(original), joint=base.assembly.joints[0];
 assert(joint,'Test document needs a joint');
 try {
  await mutate('assembly_set_joint_enabled',{joint_id:joint.id,enabled:false});
  let model=JSON.parse(await c.call('cad_project_model'));
  assert.deepEqual(model.assembly.joints[0],{...joint,enabled:false});
  await mutate('assembly_set_joint_enabled',{joint_id:joint.id,enabled:true});
  const movable={...joint,kind:'revolute',enabled:true,limits:null,angle_limits:null,linear_limits:null};
  await mutate('assembly_update_joint',{joint:movable});
  await mutate('assembly_set_joint_motion',{joint_id:joint.id,angle_offset_deg:15,linear_offset_mm:0});
  model=JSON.parse(await c.call('cad_project_model'));
  assert.deepEqual(model.assembly.joints[0],{...movable,angle_offset_deg:15,linear_offset_mm:0});
  await mutate('assembly_delete_joint',{joint_id:joint.id});
  model=JSON.parse(await c.call('cad_project_model'));
  assert(!model.assembly.joints.some(j=>j.id===joint.id));
 } finally {
  await mutate('cad_load_project_model',{model_json:original});
 }
 const solution=await c.call('assembly_solution');
 assert.equal(solution.solved,true);
 assert.equal(solution.diagnostics.length,0);
 if (option('--out')) await writeFile(option('--out'),JSON.stringify(report,null,2));
 console.log('PASS live camera and joint controls; original assembly restored');
} finally {c.close();}

