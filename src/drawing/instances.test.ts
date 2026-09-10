import type { AssemblySolutionDto, DrawingViewDto, SolidSceneDto } from '../engine/types';
import { drawingInstanceScene, drawingProjectionRequestForView, projectSceneForDrawing, drawingSourceAnchorPoint, drawingSectionSourceExtent } from './projection';
import { drawingAnchorRef, resolveDrawingAnchor } from './annotations';
import { drawingCenterlineEdgeCandidates } from './centerlines';

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const corners = [{x:0,y:0,z:0},{x:20,y:0,z:0},{x:20,y:10,z:0},{x:0,y:10,z:0}];
const scene: SolidSceneDto = { errors: [], bodies: [{
  id:1, name:'Reusable plate', feature_id:1, faces:[],
  mesh:{positions:corners.flatMap((p) => [p.x,p.y,p.z]),normals:[],indices:[0,1,2,0,2,3]},
  edges:corners.map((p,index) => ({id:index+1,key:`edge-${index}`,points:[p,corners[(index+1)%4]],refinable:true})),
}] };
const source = JSON.stringify(scene);
const half = Math.sqrt(0.5);
const solution: AssemblySolutionDto = {
  solved:true,diagnostics:[],body_poses:[],occurrence_poses:[],
  instance_body_poses:[
    {body_id:1,component_id:1,occurrence_id:31,translation:[0,0,0],rotation:[0,0,0,1],visible:true},
    {body_id:1,component_id:1,occurrence_id:32,translation:[100,0,0],rotation:[0,0,half,half],visible:true},
  ],
};
const view: DrawingViewDto = {
  id:1,name:'Assembly top',kind:'top',scope:'assembly',occurrence_ids:[],
  body_ids:[],direction:[0,0,1],up:[0,1,0],position:[50,50],scale:1,
  show_hidden_lines:true,show_tangent_edges:false,parent_view_id:null,alignment:'free',derivation:null,
};
const placed = drawingInstanceScene(scene,solution);
const projection = projectSceneForDrawing(placed,drawingProjectionRequestForView(view,[view],scene,solution));
check(Math.abs(projection.bounds[2]-100)<1e-8,'The repeated rotated occurrence must contribute its placed bounds.');
const copy = projection.anchors.find((anchor) => anchor.occurrence_id===32 && anchor.edge_id===1 && anchor.endpoint==='start');
check(copy,'Projected endpoints must carry their occurrence identity.');
check(Math.abs(copy.model_point[0]-100)<1e-8,'The copy endpoint must be in assembly coordinates.');
const reference = drawingAnchorRef(copy);
check(resolveDrawingAnchor(reference,view,projection)?.anchor===copy,'An associative dimension must resolve the selected occurrence.');
check(resolveDrawingAnchor({...reference,occurrence_id:null},view,projection)===null,'A definition reference must not silently attach to the first repeated instance.');
check(drawingCenterlineEdgeCandidates(scene,view,projection).some((candidate) => candidate.reference.occurrence_id===32),'Line and centerline picks must retain the instance.');

const end = projection.anchors.find((anchor) => anchor.occurrence_id===32 && anchor.edge_id===1 && anchor.endpoint==='end');
check(end,'The placed line needs both endpoint references.');
const section: DrawingViewDto = {...view,id:2,name:'Section',kind:'section',derivation:{
  type:'section',parent_view_id:1,first:reference,second:drawingAnchorRef(end),label:'A',depth:null,hatch_angle_deg:45,hatch_spacing_mm:2,
}};
const sectionRequest = drawingProjectionRequestForView(section,[view,section],scene,solution);
check(Math.abs(sectionRequest.section_plane!.point[0]-100)<1e-8,'The section cutting plane must follow the placed source edge.');
check(Math.abs(Math.abs(sectionRequest.direction[0])-1)<1e-8,'The section direction must follow the rotated source edge.');
check(JSON.stringify(scene)===source,'Drawing presentation must not modify the part definition or mesh.');
const guardedReference = {...reference,topology_signature:'feature:1:connectivity-v1:original'};
const guardedProjection = {...projection,topology_signatures:{'1':guardedReference.topology_signature}};
check(drawingAnchorRef(copy,guardedProjection).topology_signature===guardedReference.topology_signature,'Explicit topology picking must capture the current signature for reassociation.');
check(resolveDrawingAnchor(guardedReference,view,guardedProjection)?.anchor===copy,'An unchanged structural signature must preserve the reference.');
check(resolveDrawingAnchor(reference,view,guardedProjection)===null,'Legacy native references without a captured signature remain unverified.');
check(resolveDrawingAnchor(guardedReference,view,{...guardedProjection,topology_signatures:{'1':'feature:2:connectivity-v1:changed'}})===null,'A reused ordinal after a topology edit must not silently resolve on screen.');
const nativeScene = {...scene,bodies:scene.bodies.map((body) => ({...body,topology_signature:'connectivity-v1:original'}))};
let rejectedSection = false;
try { drawingProjectionRequestForView(section,[view,section],nativeScene,solution); } catch { rejectedSection = true; }
check(rejectedSection,'A derived native view must not use fallback coordinates for an unverified reference.');
const arcScene: SolidSceneDto = {...nativeScene,bodies:[{...nativeScene.bodies[0],edges:[{
  ...scene.bodies[0].edges[0],points:[{x:5,y:9,z:3},{x:5,y:5,z:7}],
  circle:{center:{x:5,y:5,z:3},normal:{x:1,y:0,z:0},reference:{x:0,y:1,z:0},radius:4,closed:false},
}]}]};
const centerReference = {...guardedReference,circle_center:true,fallback_point:[999,999,999] as [number,number,number]};
const sourceCenter = drawingSourceAnchorPoint(centerReference,view,[view],arcScene,guardedProjection,solution);
check(sourceCenter && Math.abs(sourceCenter[0]-95)<1e-8 && Math.abs(sourceCenter[1]-55)<1e-8,
  'An edge-on two-sample arc center must use exact placed topology, even with no projected circle.');
check(drawingSourceAnchorPoint({...centerReference,topology_signature:null},view,[view],arcScene,guardedProjection,solution)===null,'Source markers must reject unverified references.');
check(drawingSourceAnchorPoint({...centerReference,occurrence_id:99},view,[view],arcScene,guardedProjection,solution)===null,'Source markers must reject occurrences absent from the parent projection.');
const extended = drawingSectionSourceExtent([96,55],[100,55],view,projection);
check(extended && Math.abs(extended[0][0]+4)<1e-8 && Math.abs(extended[1][0]-104)<1e-8,
  'Short datum pairs must define full-width cutting-plane indicators outside the parent silhouette.');
console.log('Drawing occurrence placement, topology identity, and derived section checks passed.');
