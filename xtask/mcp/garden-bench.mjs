import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {Client} from '../../mcp-server/client.mjs';

const option = name => process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : undefined;
assert(option('--server'), 'Use --server MCP_EXE [--desktop EXE] [--save FILE] [--out DIRECTORY]');
const client = new Client(option('--server'));
const design = Object.freeze({seatWidth:1200, post:65, postClearance:2, sizeTolerance:0.5, locationTolerance:0.5, sideRail:28, armWidth:100, armThickness:35, armUnderside:630, armRailHeight:65, rearPostFront:380, armFront:-25, armRearClearance:3});
const mirrorX = (left, width) => design.seatWidth - left - width;
const report = {name: 'Crown garden bench — referenced joinery', design, datumConvention:{assembly:'X across seat from left end; Y rearward from seat front; Z upward from finished floor',part:'Each part uses A: broad stock face, B: adjacent long-edge face, C: square end; their intersection is local XYZ zero. See per-part datum axes and grain direction.'}, parts: [], calls: [], checks: []};
let tools, session;
async function call(name, args = {}) {
  const tool = tools.find(t => t.name === name);
  const result = await client.call('cad_interface', {action: 'execute', group: tool.group, operation: name, arguments: args});
  report.calls.push({name, arguments: args});
  return result;
}
const scene = () => call('solid_scene');
const bodyById = async id => (await scene()).bodies.find(b => b.id === id);
function volume(body) {
  const p = body.mesh.positions, ids = body.mesh.indices;
  let sum = 0;
  for (let i = 0; i < ids.length; i += 3) {
    const a = ids[i] * 3, b = ids[i + 1] * 3, c = ids[i + 2] * 3;
    sum += p[a] * (p[b + 1] * p[c + 2] - p[b + 2] * p[c + 1]) +
      p[a + 1] * (p[b + 2] * p[c] - p[b] * p[c + 2]) +
      p[a + 2] * (p[b] * p[c + 1] - p[b + 1] * p[c]);
  }
  return Math.abs(sum / 6);
}
function nearVolume(actual, expected, context) {
  assert(Math.abs(actual - expected) < expected * .02, `${context}: ${actual} vs ${expected} mm³`);
}
const span = (body, axis) => {
  const values = body.mesh.positions.filter((_, i) => i % 3 === axis);
  return Math.max(...values) - Math.min(...values);
};
async function part(name, width, depth, height, {round = 3, crown = false, notches = [], nose = 0} = {}) {
  const before = await scene();
  await call('sketch_begin', {name: `${name} / stock from A-B-C`, plane: {type: 'origin_plane', plane: 'xy'}});
  await call('sketch_add_rectangle_locked', {mode: 'two_point', anchor: {x: 0, y: 0}, corner_hint: {x: width, y: depth}, width_mm: width, height_mm: depth, ctrl_held: true});
  let sketchState = await call('sketch_active');
  assert.equal(sketchState.name, `${name} / stock from A-B-C`);
  const origin = sketchState.entities.find(e => e.kind === 'point' && Math.abs(e.position.x)<1e-8 && Math.abs(e.position.y)<1e-8);
  assert(origin, 'Stock profile must have a vertex at the machining origin');
  await call('sketch_add_constraint',{type:'fix',entity:origin.id});
  sketchState = await call('sketch_active');
  assert.equal(sketchState.dof.value, 0, `${name}: stock profile must have no free motion`);
  await call('sketch_finish');
  const sketch = sketchState.name;
  const extrude = {sketch_name: sketch, profile_indices: [0], operation: 'new_body', extent: {type: 'distance', distance: height}, taper_angle_deg: 0, flip: false, target_body_ids: []};
  const stockUpdate = await call('solid_extrude', extrude);
  const feature = stockUpdate.document.features.at(-1);
  let body = stockUpdate.scene.bodies.find(b => !before.bodies.some(old => old.id === b.id));
  if (crown) {
    // Round the two top edges running through the board thickness. These
    // persisted edge features follow later edits of the extrusion height.
    const ids = body.edges.filter(e => e.points.length >= 2 &&
      e.points.every(p => Math.abs(p.z - height) < 1e-6) &&
      (e.points.every(p => Math.abs(p.x) < 1e-6) || e.points.every(p => Math.abs(p.x - width) < 1e-6))).map(e => e.id);
    assert.equal(ids.length, 2, 'Two crown shoulder edges');
    await call('solid_fillet', {body_id: body.id, edge_ids: ids, radius: 40, tangent_chain: false});
    body = await bodyById(body.id);
  }
  if (nose) {
    const frontEdges=body.edges.filter(e=>e.points.length>=2 && e.points.every(p=>Math.abs(p.y)<1e-6)
      && (e.points.every(p=>Math.abs(p.x)<1e-6)||e.points.every(p=>Math.abs(p.x-width)<1e-6)));
    assert.equal(frontEdges.length,2,'Arm nose requires the two front vertical corners');
    await call('solid_fillet',{body_id:body.id,edge_ids:frontEdges.map(e=>e.id),radius:nose,tangent_chain:false});
    body=await bodyById(body.id);
  }
  if (round) {
    const edges = body.edges.filter(e => e.refinable && (!crown ||
      e.points.every(p => Math.abs(p.y) < 1e-6) || e.points.every(p => Math.abs(p.y - depth) < 1e-6)));
    await call('solid_fillet', {body_id: body.id, edge_ids: edges.map(e => e.id), radius: round, tangent_chain: crown});
    body = await bodyById(body.id);
  }
  let relief;
  if (notches.length) {
    const [start, end] = notches;
    const notchWidth = design.post + 2 * design.postClearance;
    for (const x of [65-design.postClearance, 1070-design.postClearance]) {
      const beforePocket = volume(body);
      const face = body.faces.find(f => f.plane?.normal[2] > .99);
      await call('sketch_begin', {name: `${name} / ${x < 600 ? 'left' : 'right'} post clearance`, plane: {type: 'planar_face', face_id: face.id}, face_origin: 'global_origin_projection'});
      await call('sketch_set_grid_snap', {enabled: false});
      await call('sketch_add_rectangle_locked', {mode: 'two_point', anchor: {x, y: start}, corner_hint: {x: x + notchWidth, y: end}, width_mm: notchWidth, height_mm: end - start, ctrl_held: true});
      const pocket = await call('sketch_active');
      const locator = pocket.entities.find(e => e.kind === 'point' && Math.abs(e.position.x-x)<1e-8 && Math.abs(e.position.y-start)<1e-8);
      assert(locator, 'Post clearance has a locating vertex');
      await call('sketch_add_constraint',{type:'fix',entity:locator.id});
      assert.equal((await call('sketch_active')).dof.value,0,'Post clearances must be fully located');
      const cutSketch = (await call('sketch_active')).name;
      await call('sketch_finish');
      await call('solid_extrude', {sketch_name: cutSketch, profile_indices: [0], operation: 'cut', extent: {type: 'through_all'}, taper_angle_deg: 0, flip: true, target_body_ids: [body.id]});
      body = await bodyById(body.id);
      nearVolume(beforePocket - volume(body), notchWidth * (Math.min(depth, end) - Math.max(0, start)) * height, 'Post clearance pocket removes stock');
    }
  }
  if (crown) {
    const uncutVolume = volume(body);
    const frontFace = body.faces.find(f=>f.plane?.normal[1]<-.99);
    const rearFace = body.faces.find(f=>f.plane?.normal[1]>.99);
    const reference = await call('construction_plane_midplane',{name:'Picket / thickness symmetry plane',first:{type:'planar_face',face_id:frontFace.id},second:{type:'planar_face',face_id:rearFace.id}});
    const midplane = reference.planes.at(-1);
    const toPlane = p => ({x:midplane.basis.u.reduce((sum,v,i)=>sum+v*(p[i]-midplane.basis.origin[i]),0),y:midplane.basis.v.reduce((sum,v,i)=>sum+v*(p[i]-midplane.basis.origin[i]),0)});
    const p1=toPlane([width/2,depth/2,height/2-50]),p2=toPlane([width/2,depth/2,height/2+50]);
    assert(Math.abs(p1.x-p2.x)<1e-8,'Relief axis must be vertical in the machining plane');
    await call('sketch_begin', {name: `${name} / datum-located decorative slot`, plane: {type: 'datum_plane', datum_id:midplane.datum_id}});
    await call('sketch_set_grid_snap', {enabled: false});
    await call('sketch_add_slot', {mode: 'center_to_center', p1, p2, cursor: {x:p1.x+9,y:(p1.y+p2.y)/2}, width_mm: 18});
    const active = await call('sketch_active');
    const line = active.entities.find(e => e.kind === 'line');
    const arc = active.entities.find(e => e.kind === 'arc' && Math.hypot(e.center.x-p1.x,e.center.y-p1.y)<1e-6);
    assert(arc,'Use the locating end of the slot, not an arbitrary topology index');
    await call('sketch_add_dimension', {entities: [line.id], text_pos: {x: -30, y: 0}, value_text: '100'});
    await call('sketch_add_constraint', {type: 'vertical', entity: line.id});
    await call('sketch_add_point', {position:p1});
    const datum = (await call('sketch_active')).entities.at(-1).id;
    // Fix only the locating datum, leaving slot width and length editable.
    await call('sketch_add_constraints', {constraints: [{type: 'fix', entity: datum}, {type: 'center_coincident', point: datum, curve: arc.id}]});
    const constrained = await call('sketch_active');
    assert.equal(constrained.dof.value, 0, 'Relief slot must be fully constrained');
    relief = {sketch: constrained.name, width_constraint: active.dimensions[0].constraint_id,datum_id:midplane.datum_id,datum_name:midplane.name,slot_center_from_bottom_mm:height/2};
    await call('sketch_finish');
    await call('solid_extrude', {sketch_name: relief.sketch, profile_indices: [0], operation: 'cut', extent: {type: 'symmetric',distance:depth+2}, taper_angle_deg: 0, flip: true, target_body_ids: [body.id]});
    body = await bodyById(body.id);
    nearVolume(uncutVolume - volume(body), (18 * 100 + Math.PI * 9 ** 2) * depth, 'Relief slot removes stock');
  }
  const light = name.includes('seat') || name.includes('Seat') || crown || /armrest/i.test(name);
  await call('set_body_appearance', {body_id: body.id, color: light ? {r: 187, g: 126, b: 68, a: 255} : {r: 65, g: 84, b: 77, a: 255}, material_name: light ? 'Oiled timber (visual designation)' : 'Painted timber (visual designation)', filament_type: '', brand: '', color_name: light ? 'Honey timber' : 'Deep green'});
  const component = await call('assembly_create_component', {name, body_ids: [body.id], absorb_promoted_bodies: true});
  const assembly = await call('assembly_document');
  const occurrence = assembly.component_structure.occurrences.find(o => o.component_id === component.id);
  const result = {name, width, depth, height, sketch, dimensions: sketchState.dimensions, extrude, feature, body, component, occurrence, relief};
  const stock=[width,depth,height], grainAxis=stock.indexOf(Math.max(...stock));
  const thicknessAxis=[0,1,2].filter(i=>i!==grainAxis).sort((a,b)=>stock[a]-stock[b])[0];
  const edgeAxis=[0,1,2].find(i=>i!==grainAxis&&i!==thicknessAxis);
  const datums={A:{normal_axis:thicknessAxis,coordinate_mm:0},B:{normal_axis:edgeAxis,coordinate_mm:0},C:{normal_axis:grainAxis,coordinate_mm:0},grain_axis:grainAxis};
  report.parts.push({name,datums, stock_mm: [width, depth, height], sketch, dimensions: sketchState.dimensions, feature_id: feature.id, body_id: body.id, component_id: component.id, relief});
  console.log('Built', name);
  return result;
}
function anchor(body, origin, normal) {
  const face = body.faces.find(f => f.plane && f.plane.normal.every((v,i) => Math.abs(v-normal[i]) < 1e-6)
    && Math.abs(f.plane.origin.reduce((sum,v,i) => sum+(origin[i]-v)*normal[i],0)) < 1e-5);
  assert(face, `Missing mating reference face on body ${body.id}`);
  const secondary = normal[0] ? [0,1,0] : [1,0,0];
  return {body_id:body.id,face_id:face.id,face_key:face.key,kind:'planar_face',
    frame:{origin,primary_axis:normal,secondary_axis:secondary},
    source_surface_frame:{origin:face.plane.origin,primary_axis:face.plane.normal,secondary_axis:face.plane.u}};
}
let root;
const rootPosition = [65, 30, 0];
const expected = [];
async function instances(part, positions) {
  const placed = [];
  for (const [index, position] of positions.entries()) {
    const occurrence = index === 0 ? part.occurrence : await call('assembly_create_occurrence', {component_id: part.component.id, name: `${part.name} ${index + 1}`});
    // Show each new member in its intended place while the remaining parts
    // are machined. Final contact mates replace this unconstrained preview.
    await call('assembly_set_occurrence_pose',{occurrence_id:occurrence.id,local_pose:{translation:position,rotation:[0,0,0,1]}});
    // Assemble the finished machined parts below, using freshly resolved faces.
    expected.push({id: occurrence.id, position, body_id: part.body.id});
    placed.push({part, position, occurrence});
  }
  return placed;
}
async function assemble() {
  const bodies = (await scene()).bodies;
  const placed = new Set([root.occurrence.id]);
  report.mates = [];
  while (placed.size < expected.length) {
    const connection = connections.find(c => placed.has(c.from.occurrence.id) !== placed.has(c.to.occurrence.id));
    assert(connection, 'Every part must connect to the grounded frame through a real joint');
    const parentIsFrom = placed.has(connection.from.occurrence.id);
    const parent = parentIsFrom ? connection.from : connection.to;
    const child = parentIsFrom ? connection.to : connection.from;
    const contact = connection.head.map((v,i)=>v+(i===connection.axis?connection.direction*connection.thickness:0));
    const normal = [0,0,0]; normal[connection.axis] = connection.direction*(parentIsFrom?1:-1);
    await call('assembly_create_joint', {name:`${child.part.name} ${child.occurrence.id} / to ${parent.part.name}`,kind:'rigid',
      connector_a:anchor(bodies.find(b=>b.id===parent.part.body.id),contact.map((v,i)=>v-parent.position[i]),normal),
      connector_b:anchor(bodies.find(b=>b.id===child.part.body.id),contact.map((v,i)=>v-child.position[i]),normal.map(v=>-v)),flipped:false,
      advanced:{connector_a_occurrence_id:parent.occurrence.id,connector_b_occurrence_id:child.occurrence.id}});
    report.mates.push({parent:parent.occurrence.id,child:child.occurrence.id,contact_mm:contact});
    placed.add(child.occurrence.id);
  }
}
const drills = new Map();
const connections = [];
let functionalMembers;
const dimensions = p => [p.width, p.depth, p.height];
function screw(from, to, head, axis, direction, thickness, length, stage) {
  const localHead = head.map((v, i) => v - from.position[i]);
  const sourceSize = dimensions(from.part);
  assert(Math.abs(localHead[axis] - (direction > 0 ? 0 : sourceSize[axis])) < 1e-6, 'Screw head must lie on source face');
  for (let i = 0; i < 3; i++) if (i !== axis) assert(localHead[i] > 5 && localHead[i] < sourceSize[i] - 5, 'Clearance hole must lie within source stock');
  const entry = head.map((v, i) => v + (i === axis ? direction * thickness : 0));
  const localEntry = entry.map((v, i) => v - to.position[i]);
  const size = dimensions(to.part);
  assert(Math.abs(localEntry[axis] - (direction > 0 ? 0 : size[axis])) < 1e-6, 'Joint must bear on receiving member');
  for (let i = 0; i < 3; i++) if (i !== axis) assert(localEntry[i] > 5 && localEntry[i] < size[i] - 5, 'Hole must lie within receiving stock');
  assert(length > thickness && length - thickness < size[axis] - 5, 'Screw must engage without protruding');
  assert.notEqual(axis, size.indexOf(Math.max(...size)), 'Receiving screw must enter side grain');
  connections.push({from, to, head, axis, direction, thickness, length, stage});
  for (const [member, point, diameter, depth] of [[from, head, 5.5, thickness], [to, entry, 3.5, length - thickness + 2]]) {
    const local = point.map((v, i) => v - member.position[i]);
    const countersink = member === from && /seat slat|armrest/i.test(member.part.name);
    const canonical = !countersink && depth === dimensions(member.part)[axis];
    const drillDirection = canonical ? 1 : direction;
    if (canonical) local[axis] = 0;
    const key = JSON.stringify([member.part.body.id, axis, drillDirection, diameter, depth, local[axis], countersink]);
    if (!drills.has(key)) drills.set(key, {part: member.part, axis, direction: drillDirection, diameter, depth, countersink, points: new Map()});
    drills.get(key).points.set(JSON.stringify(local), local);
  }
}
async function machineConnections() {
  const shafts = connections.map(c => c.head.map((v, i) => [Math.min(v, v + (i === c.axis ? c.direction * c.length : 0)), Math.max(v, v + (i === c.axis ? c.direction * c.length : 0))]));
  for (let i = 0; i < shafts.length; i++) for (let j = i + 1; j < shafts.length; j++) {
    const distanceSquared = shafts[i].reduce((sum, interval, axis) => sum + Math.max(0, interval[0] - shafts[j][axis][1], shafts[j][axis][0] - interval[1]) ** 2, 0);
    assert(distanceSquared >= 5.5 ** 2, `Fasteners intersect: ${i}, ${j}`);
  }
  let currentScene = await scene();
  for (const {part, axis, direction, diameter, depth, points, countersink} of drills.values()) {
    const body = currentScene.bodies.find(b => b.id === part.body.id);
    const first = [...points.values()][0];
    const face = body.faces.find(f => f.plane && f.plane.normal[axis] * direction < -.99 && Math.abs(f.plane.origin[axis] - first[axis]) < 1e-5);
    assert(face, `Missing drilling face: ${part.name}`);
    const positions = [...points.values()].map(p => {
      const delta = p.map((v, i) => v - face.plane.origin[i]);
      const dot = v => v.reduce((sum, n, i) => sum + n * delta[i], 0);
      return {position: {x: dot(face.plane.u), y: dot(face.plane.v)}};
    });
    const before = volume(body);
    const update = await call('solid_hole', {body_id: body.id, face_id: face.id, position: positions[0].position, positions,
      diameter, extent: {type: 'distance', depth}, style: countersink ? 'countersink' : 'simple', countersink_diameter: 10, countersink_angle_deg: 90, bottom_style: 'flat', flip: false});
    const r = diameter / 2, h = 5 - r;
    const recess = countersink ? Math.PI * h * (25 + 5 * r - 2 * r * r) / 3 : 0;
    // The committed command already returns the authoritative scene. Avoid
    // two extra live round trips per feature merely to fetch it again.
    currentScene = update.scene;
    nearVolume(before - volume(currentScene.bodies.find(b => b.id === body.id)), positions.length * (Math.PI * r ** 2 * depth + recess), `Drilling ${part.name}`);
    console.log('Drilled', part.name, positions.length, 'holes');
  }
  report.fasteners = connections.map(({from, to, ...detail}) => ({...detail, from: from.occurrence.id, to: to.occurrence.id, nominal_diameter_mm: 5, head_style: /seat slat|armrest/i.test(from.part.name) ? '90 degree countersunk, 10 mm head envelope' : 'washer or pan head'}));
  report.checks.push('mating clearance and pilot bores remove stock', 'side-grain engagement, separated shafts and no screw-tip protrusion');
}
function checkFunction(s, solution) {
  if (!functionalMembers) return;
  for (const drill of drills.values()) {
    const cylinders = s.bodies.find(b => b.id === drill.part.body.id).faces.map(f => f.cylinder).filter(Boolean);
    for (const point of drill.points.values()) assert(cylinders.some(cylinder => {
      const axis = [cylinder.axis.x, cylinder.axis.y, cylinder.axis.z];
      const origin = [cylinder.origin.x, cylinder.origin.y, cylinder.origin.z];
      return Math.abs(Math.abs(axis[drill.axis]) - 1) < 1e-6 && Math.abs(cylinder.radius - drill.diameter / 2) < 1e-6 &&
        origin.every((v, i) => i === drill.axis || Math.abs(v - point[i]) < 1e-5);
    }), `Drilled axis moved or disappeared: ${drill.part.name} at ${point}`);
  }
  const boxes = new Map(expected.map(item => {
    const pose = solution.instance_body_poses.find(p => p.occurrence_id === item.id);
    assert(pose.rotation.slice(0, 3).every(n => Math.abs(n) < 1e-6), 'Bench stock axes must remain aligned');
    const mesh = s.bodies.find(b => b.id === item.body_id).mesh.positions;
    return [item.id, [0, 1, 2].map(axis => {
      const coordinates = mesh.filter((_, i) => i % 3 === axis);
      return [Math.min(...coordinates) + pose.translation[axis], Math.max(...coordinates) + pose.translation[axis]];
    })];
  }));
  const box = member => boxes.get(member.occurrence.id);
  const contact = (a, b, axis) => {
    assert(Math.abs(a[axis][1] - b[axis][0]) < 1e-4, 'Expected bearing faces must meet');
    for (let i = 0; i < 3; i++) if (i !== axis)
      assert(Math.min(a[i][1], b[i][1]) - Math.max(a[i][0], b[i][0]) >= 20, 'Bearing overlap must be at least 20 mm');
  };
  const {pickets, backs, arms, front, armRails, seats, seatSupports, center, centerCleats} = functionalMembers;
  for (const pair of [arms,armRails,front]) {
    const [a,b]=pair.map(box);
    assert(Math.abs(a[0][0]+b[0][1]-design.seatWidth)<1e-5 && Math.abs(a[0][1]+b[0][0]-design.seatWidth)<1e-5,'Left/right stock must be mirrored about the seat center plane');
  }
  for (const arm of arms) assert(Math.abs(design.rearPostFront-box(arm)[1][1]-design.armRearClearance)<1e-5,'Arm end must clear rear post');
  for (const slat of pickets) for (const rail of backs) contact(box(slat), box(rail), 1);
  for (const [i, arm] of arms.entries()) for (const support of [front[i], armRails[i]]) contact(box(support), box(arm), 2);
  for (const seat of seats) for (const rail of seatSupports) contact(box(rail), box(seat), 2);
  for (const support of centerCleats) contact(box(support), box(center), 2);
  // A conservative 20 mm square, 120 mm long driver envelope. Only members
  // installed by that stage can obstruct the tool; the seat precedes arms.
  const stages = ['lower frame', 'frame', 'seat', 'back', 'arms'];
  const installed = new Map();
  for (const c of connections) for (const member of [c.from, c.to]) {
    const id = member.occurrence.id, stage = stages.indexOf(c.stage);
    installed.set(id, Math.min(installed.get(id) ?? Infinity, stage));
  }
  for (const c of connections) {
    const envelope = c.head.map((v, i) => i === c.axis ?
      [Math.min(v, v - c.direction * 120), Math.max(v, v - c.direction * 120)] : [v - 10, v + 10]);
    for (const [id, stock] of boxes) {
      if ([c.from.occurrence.id, c.to.occurrence.id].includes(id) || installed.get(id) > stages.indexOf(c.stage)) continue;
      const name = report.parts.find(p => p.body_id === expected.find(item => item.id === id)?.body_id)?.name;
      assert(!stock.every((range, i) => Math.min(range[1], envelope[i][1]) - Math.max(range[0], envelope[i][0]) > .01), `Driver access obstructed: ${JSON.stringify({connection: c.head, obstruction: name})}`);
    }
  }
}
async function validate(c = client) {
  const s = await c.call('solid_scene');
  assert.deepEqual(s.errors, []);
  const solution = await c.call('assembly_solution');
  assert.equal(solution.solved, true, JSON.stringify(solution.diagnostics));
  assert.deepEqual(solution.diagnostics, []);
  for (const item of expected) {
    const pose = solution.instance_body_poses.find(p => p.occurrence_id === item.id);
    assert(pose && pose.translation.every((n, i) => Math.abs(n - item.position[i]) < 1e-5), JSON.stringify({item, pose}));
  }
  checkFunction(s, solution);
  const interference = await c.call('assembly_interference_check',{});
  assert.equal(interference.exact,true);
  assert.deepEqual(interference.pairs.filter(p=>p.interfering),[], 'Finished occurrence solids must not overlap');
  report.interference = {exact:true,candidate_pairs:interference.pairs.length,overlaps:0};
  const m = JSON.parse(await c.call('cad_project_model'));
  assert(!m.document.history.features.some(f => f.kind === 'import_step'));
  return m;
}
try {
  await client.start(); tools = await client.call('cad_list_all_tools');
  if (option('--desktop')) {
    const launched = await client.call('cad_interface', {action: 'launch', executable: option('--desktop')});
    assert.equal(launched.status, 'ready'); session = launched.session_id;
    report.session_id = session; report.pid = launched.pid;
  }
  await call('cad_set_document_name', {name: 'Crown garden bench — referenced joinery'});
  const minimumNotchClearance = design.postClearance-design.sizeTolerance-design.locationTolerance;
  const minimumArmClearance = design.armRearClearance-2*design.locationTolerance-design.sizeTolerance;
  assert(minimumNotchClearance>=1 && minimumArmClearance>=1,'Tolerance stack must retain positive assembly clearance');
  report.tolerances = {units:'mm',finished_size_plus_minus:design.sizeTolerance,location_plus_minus:design.locationTolerance,post_notch_nominal_side_clearance:design.postClearance,post_notch_minimum_side_clearance:minimumNotchClearance,arm_rear_nominal_clearance:design.armRearClearance,arm_rear_minimum_clearance:minimumArmClearance,scope:'Machining and assembly budget only; moisture movement must be checked for chosen timber.'};
  root = await part('Front leg and arm post', 65, 65, 630, {round: 0});
  await call('assembly_set_occurrence_pose', {occurrence_id: root.occurrence.id, local_pose: {translation: rootPosition, rotation: [0, 0, 0, 1]}});
  await call('assembly_set_occurrence_grounded', {occurrence_id: root.occurrence.id, grounded: true});
  expected.push({id: root.occurrence.id, position: rootPosition, body_id: root.body.id});
  const front = [{part: root, position: rootPosition, occurrence: root.occurrence}, ...(await instances(await part('Right front leg and arm post', 65, 65, 630, {round: 0}), [[1070, 30, 0]]))];
  const rear = [...(await instances(await part('Left rear leg and back post', 65, 65, 790, {round: 0}), [[65, 380, 0]])), ...(await instances(await part('Right rear leg and back post', 65, 65, 790, {round: 0}), [[1070, 380, 0]]))];
  const aprons = await instances(await part('Long apron — face lap', 1070, 28, 155, {round: 0}), [[65, 2, 260], [65, 445, 260]]);
  const sides = [...(await instances(await part('Upper side rail — face lap', 28, 415, 90, {round: 0}), [[37, 30, 325], [1135, 30, 325]])), ...(await instances(await part('Lower side rail — face lap', 28, 415, 90, {round: 0}), [[37, 30, 130], [1135, 30, 130]]))];
  const center = (await instances(await part('Center seat bearer', 28, 415, 90, {round: 0}), [[586, 30, 325]]))[0];
  const centerCleats = [...(await instances(await part('Front center bearer support block', 65, 28, 65, {round: 0}), [[567.5, 30, 260]])), ...(await instances(await part('Rear center bearer support block', 65, 28, 65, {round: 0}), [[567.5, 417, 260]]))];
  const seatSupports = [sides[0], center, sides[1]];
  const stretcher = (await instances(await part('Lower stretcher — bearing on rails', 1126, 90, 35), [[37, 200, 220]]))[0];
  const seats = [
    ...(await instances(await part('Front seat slat — post notches', 1200, 85, 35, {notches: [30-design.postClearance, 90], round: 0}), [[0, 0, 415]])),
    ...(await instances(await part('Second seat slat — post relief', 1200, 85, 35, {notches: [-1, 5+design.postClearance], round: 0}), [[0, 90, 415]])),
    ...(await instances(await part('Seat slat', 1200, 85, 35), [[0, 180, 415], [0, 270, 415]])),
    ...(await instances(await part('Rear seat slat — post notches', 1200, 85, 35, {notches: [20-design.postClearance, 90], round: 0}), [[0, 360, 415]]))];
  const backs = await instances(await part('Back rail — face lap', 1070, 32, 55, {round: 0}), [[65, 445, 480], [65, 445, 735]]);
  const picket = await part('Back picket — editable height', 85, 25, 315, {crown: true, round: 3});
  const pickets = await instances(picket, Array.from({length: 9}, (_, i) => [137.5 + i * 105, 420, 490]));
  const armRails = await instances(await part('Continuous arm support rail', design.sideRail,415,design.armRailHeight,{round:0}), [[37,30,design.armUnderside-design.armRailHeight],[mirrorX(37,design.sideRail),30,design.armUnderside-design.armRailHeight]]);
  const arms = [];
  for (const [index,x] of [35,mirrorX(35,design.armWidth)].entries())
    arms.push(...await instances(await part(`${index?'Right':'Left'} armrest — eased edges`,design.armWidth,design.rearPostFront-design.armRearClearance-design.armFront,design.armThickness,{round:3,nose:18}),[[x,design.armFront,design.armUnderside]]));
  for (const [i, rail] of aprons.entries()) for (const leg of i ? rear : front)
    for (const z of [350, 390]) screw(rail, leg, [leg.position[0] + 32.5, i ? 473 : 2, z], 1, i ? -1 : 1, 28, 65, 'frame');
  for (const [i, rail] of sides.entries()) for (const leg of [front[i % 2], rear[i % 2]])
    for (const z of [rail.position[2] + 40, rail.position[2] + 75]) screw(rail, leg, [i % 2 ? 1163 : 37, leg.position[1] + 32.5, z], 0, i % 2 ? -1 : 1, 28, 65, i >= 2 ? 'lower frame' : 'frame');
  for (const [i, rail] of sides.slice(2).entries()) for (const y of [225, 265])
    screw(stretcher, rail, [i ? 1149 : 51, y, 255], 2, -1, 35, 65, 'lower frame');
  for (const [i, support] of centerCleats.entries()) {
    for (const x of [582.5, 617.5]) screw(aprons[i], support, [x, i ? 473 : 2, 295], 1, i ? -1 : 1, 28, 45, 'frame');
    screw(support, center, [600, support.position[1] + 14, 260], 2, 1, 65, 90, 'frame');
  }
  for (const seat of seats) for (const rail of seatSupports)
    screw(seat, rail, [rail.position[0] + 14, seat.position[1] + 42.5, 450], 2, -1, 35, 65, 'seat');
  for (const rail of backs) {
    for (const leg of rear) for (const x of [leg.position[0] + 20, leg.position[0] + 45])
      screw(rail, leg, [x, 477, rail.position[2] + 27.5], 1, -1, 32, 65, 'back');
    for (const slat of pickets) screw(rail, slat, [slat.position[0] + 42.5, 477, rail.position[2] + 27.5], 1, -1, 32, 50, 'back');
  }
  for (const [i, rail] of armRails.entries()) {
    for (const post of [front[i],rear[i]]) for (const z of [590,615])
      screw(rail,post,[i?1163:37,post.position[1]+32.5,z],0,i?-1:1,design.sideRail,65,'arms');
    for (const y of [130,330])
      screw(arms[i],rail,[rail.position[0]+14,y,design.armUnderside+design.armThickness],2,-1,design.armThickness,65,'arms');
  }
  await machineConnections();
  await assemble();
  functionalMembers = {pickets, backs, arms, front, armRails, seats, seatSupports, center, centerCleats};
  report.checks.push('solved geometry: seating-side back support and seat/arm bearing', '120 mm driver access in assembly order');
  await validate();
  const proofScene = await scene(), proofSolution = await call('assembly_solution');
  const wrongSide = structuredClone(proofSolution);
  wrongSide.instance_body_poses.find(p => p.occurrence_id === pickets[0].occurrence.id).translation[1] = 477;
  assert.throws(() => checkFunction(proofScene, wrongSide), /bearing faces/);
  const unsupportedArm = structuredClone(proofSolution);
  unsupportedArm.instance_body_poses.find(p => p.occurrence_id === armRails[1].occurrence.id).translation[2] -= 10;
  assert.throws(() => checkFunction(proofScene, unsupportedArm), /bearing faces/);
  report.checks.push('functional checks reject reversed picket and lowered arm support');
  const narrowSlotVolume = volume(await bodyById(picket.body.id));
  await call('sketch_edit', {name: picket.relief.sketch});
  await call('sketch_edit_dimension', {constraint_id: picket.relief.width_constraint, text: '20'});
  assert.equal((await call('sketch_active')).dof.value, 0);
  await call('sketch_finish');
  await call('solid_recompute');
  await validate();
  nearVolume(narrowSlotVolume - volume(await bodyById(picket.body.id)), (2 * 100 + Math.PI * (10 ** 2 - 9 ** 2)) * picket.depth, 'Width edit changes the cut');
  report.checks.push('fully constrained datum slot, driving width 18 → 20');
  // Exercise downstream edge features and every repeated back-picket instance.
  await call('solid_edit_extrude', {feature_id: picket.feature.id, extrude: {...picket.extrude, extent: {type: 'distance', distance: 340}}});
  await validate();
  assert(Math.abs(span(await bodyById(picket.body.id), 2) - 340) < 1e-4);
  report.checks.push('native feature history', 'face-referenced assembly poses', 'picket height 315 → 340 with downstream fillets');
  const saved = await call('cad_project_model');
  const finishedBodies = (await scene()).bodies;
  for (const entry of report.parts) {
    entry.quantity = expected.filter(item => item.body_id === entry.body_id).length;
    entry.finished_envelope_mm = [0, 1, 2].map(axis => span(finishedBodies.find(b => b.id === entry.body_id), axis));
    entry.stock_mm = entry.finished_envelope_mm.map(v => Math.round(v * 1000) / 1000);
  }
  const restored = new Client(option('--server'));
  try {
    await restored.start(); await restored.call('cad_load_project_model', {model_json: saved});
    await validate(restored);
    await restored.call('solid_edit_extrude', {feature_id: picket.feature.id, extrude: {...picket.extrude, extent: {type: 'distance', distance: 350}}});
    assert(Math.abs(span((await restored.call('solid_scene')).bodies.find(b => b.id === picket.body.id), 2) - 350) < 1e-4);
    await validate(restored);
    report.checks.push('fresh process restore and subsequent feature edit');
  } finally { restored.close(); }
  if (session) {
    if (option('--save')) {
      const path = resolve(option('--save'));
      assert.equal((await client.call('cad_interface', {action: 'file', command: 'save', path})).status, 'applied');
      assert.equal((await client.call('cad_interface', {action: 'file', command: 'open', path})).status, 'applied');
      await validate();
      // Exercise the file the user will open, not just a model checkpoint.
      const reopenedVolume = volume(await bodyById(picket.body.id));
      await call('sketch_edit', {name: picket.relief.sketch});
      await call('sketch_edit_dimension', {constraint_id: picket.relief.width_constraint, text: '22'});
      assert.equal((await call('sketch_active')).dof.value, 0);
      await call('sketch_finish'); await call('solid_recompute');
      await validate();
      nearVolume(reopenedVolume - volume(await bodyById(picket.body.id)), (2 * 100 + Math.PI * (11 ** 2 - 10 ** 2)) * picket.depth, 'Reopened native file remains editable');
      await call('sketch_edit', {name: picket.relief.sketch});
      await call('sketch_edit_dimension', {constraint_id: picket.relief.width_constraint, text: '20'});
      await call('sketch_finish'); await call('solid_recompute'); await validate();
      assert.equal((await client.call('cad_interface', {action: 'file', command: 'save', path, overwrite: true})).status, 'applied');
      report.checks.push('native .nbcad save/open, width edit 20 → 22 → 20, save');
    }
    assert.equal((await client.call('cad_interface', {action: 'view', view: 'isometric', fit: true})).status, 'applied');
    assert.equal((await client.call('cad_interface', {action: 'window', mode: 'foreground'})).status, 'applied');
  }
  if (option('--out')) {
    await mkdir(resolve(option('--out')), {recursive: true});
    await writeFile(resolve(option('--out'), 'model.json'), saved);
    await writeFile(resolve(option('--out'), 'report.json'), JSON.stringify(report, null, 2));
  }
  console.log('PASS Crown garden bench', report.checks);
} finally { client.close(); }
