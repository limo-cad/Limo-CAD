// Read retained evidence and record checks; does not generate or change images.
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const near = (a,b) => Math.abs(a-b) <= 0.00002;
function linesDxf(text) {
  const rows=text.trimEnd().split(/\r?\n/); const entities=[]; let e;
  for(let i=0;i<rows.length;i+=2){const code=rows[i].trim(), value=rows[i+1]?.trim();if(code==='0'){e={type:value};entities.push(e);}else if(e)e[code]=value;}
  return entities.filter(e=>e.type==='LINE'&&e['8']==='CENTER').map(e=>({points:[[+e['10'],210-+e['20']],[+e['11'],210-+e['21']]],lineweight:+e['370'],linetype:e['6']}));
}
for(const folder of ['drawing-center-exports','drawing-center-exports-fixed']){
  const dir=path.join('.codex',folder), fixed=folder.endsWith('-fixed');
  const audit=JSON.parse(fs.readFileSync(path.join(dir,'independent-render-review.json')));
  const cases=JSON.parse(fs.readFileSync(path.join(dir,'manifest.json'))).cases;
  const items=[];
  for(const name of cases){
    const src=JSON.parse(fs.readFileSync(path.join(dir,name+'.json'))), sheet=src.drawing.sheets[0], view=sheet.views[0], projection=src.projection, annotation=sheet.annotations[0];
    const s=view.scale, b=projection.bounds;
    const points=projection.circles.map(c=>[view.position[0]+(c.center[0]-(b[0]+b[2])/2)*s,view.position[1]-(c.center[1]-(b[1]+b[3])/2)*s]);
    const radii=projection.circles.map(c=>c.radius*s), ext=annotation.extension;
    let expected;
    if(annotation.kind==='center_mark'){
      const [x,y]=points[0], e=radii[0]+ext;
      expected=[[[x-e,y],[x+e,y]],[[x,y-e],[x,y+e]]];
    }else{
      const delta=points[1].map((n,i)=>n-points[0][i]), len=Math.hypot(...delta), u=delta.map(n=>n/len);
      expected=[[points[0].map((n,i)=>n-u[i]*(radii[0]+ext)),points[1].map((n,i)=>n+u[i]*(radii[1]+ext))]];
    }
    const svg=fs.readFileSync(path.join(dir,name+'.svg'),'utf8');
    const svgLines=[...svg.matchAll(/<polyline data-layer="CENTER" points="([^"]+)"[^>]*stroke="([^"]+)" stroke-width="([^"]+)" stroke-dasharray="([^"]+)"/g)].map(m=>({points:m[1].split(' ').map(p=>p.split(',').map(Number)),color:m[2],width:+m[3],dash:m[4].split(',').map(Number)}));
    const dxfLines=linesDxf(fs.readFileSync(path.join(dir,name+'.dxf'),'utf8'));
    const matches=lines=>lines.length===expected.length&&lines.every((l,i)=>l.points.every((p,j)=>p.every((n,k)=>near(n,expected[i][j][k]))));
    if(!matches(svgLines)||!matches(dxfLines))throw Error('Endpoint mismatch '+name);
    for(const format of ['svg','dxf']){
      const file=name+'-'+format+'.png', png=fs.readFileSync(path.join(dir,file));
      const report=audit.artifacts.find(r=>r.source===name+'.'+format);
      const mark=annotation.kind==='center_mark', dxf=format==='dxf';
      const warning=mark?(dxf?'Lower vertical center stroke touches/overlaps view caption; clear at scale 2, tight at scale 1.':'Lower vertical center stroke has only a small visible gap above caption.') : null;
      items.push({file,sha256:sha(path.join(dir,file)),dimensions:[png.readUInt32BE(16),png.readUInt32BE(20)],review_method:'Original PNG independently viewed at original resolution',source_file:name+'.'+format,source_sha256:sha(path.join(dir,name+'.'+format)),model_file:name+'.json',model_sha256:sha(path.join(dir,name+'.json')),same_png_as_original:fixed?sha(path.join(dir,file))===sha(path.join('.codex/drawing-center-exports',file)):null,model_unchanged_from_original:fixed?sha(path.join(dir,name+'.json'))===sha(path.join('.codex/drawing-center-exports',name+'.json')):null,kind:annotation.kind,scale:s,paper_circle_centers:points,paper_circle_radii:radii,extension_paper_mm:ext,expected_center_segments:expected,serialized_center_segments:dxf?dxfLines:svgLines,exact_current_projection_endpoint_check:true,caption_baseline: +(svg.match(/<text[^>]* y="([^"]+)"[^>]*>Synthetic circles/)?.[1]??NaN),audit_errors:report.audit_errors??null,audit_fixes:report.audit_fixes??null,nonblank_render:report.nonblank_render,visible_facts:mark?['Cross centered on first/smaller circle only','Black circle outlines retained','White center opening and teal ring visible','Four arms extend beyond first circle','Complete sheet frame/title and readable scale caption']:['Diagonal passes through both circle centers','Black circle outlines retained','White center openings and teal rings visible','Line extends outward beyond both circles','Complete sheet frame/title and readable scale caption'],style:sheet.style.center,warning,result:!fixed&&dxf?'FAIL: invalid DXF lineweight audit repairs':mark&&dxf?'FAIL: caption collision':mark?'PASS geometry; caption clearance concern':'PASS scoped pixels and geometry'});
    }
  }
  fs.writeFileSync(path.join(dir,'review-sha256.json'),JSON.stringify({reviewer:'series_resume',source:'Synthetic projection fixture, not OCC',all_originals_individually_viewed:true,scope:'CenterMark and two-circle CenterLine, scale 1/2, default/custom dash and width, SVG and graphical DXF',not_proven:['OCC projection','native authoring or File UI','physical input','printing','editable DXF dimensions','unrepresented fonts or sheet layouts'],items},null,2)+'\n');
  const lines=[
    '# Independent center export review', '',
    fixed?'The corrected DXF structure passes all eight independent audits with zero errors and zero repairs. Pixel review still fails the center-mark caption-clearance check; do not treat this set as an unconditional visual pass.':'This original set is retained as failed evidence. All eight DXFs required code 205 lineweight repairs during independent parsing, so they did not pass strict audit. Center-mark caption clearance also fails as described below.', '',
    'All 16 original PNGs in this directory were individually viewed at original resolution. `review-sha256.json` records every filename, SHA-256, source/model hash, image size, exact projected center endpoints, audit record and visual finding. Source JSON and serialized SVG/DXF coordinates were checked independently of rendered appearance.', '',
    'The eight cases use an A4 landscape synthetic projection with circles at model [10,20], radius 5 and [40,40], radius 8. Current projected bounds are [5,15,48,48]. Deliberately stale saved fallback centers/radii are 999; all SVG and DXF center segments match the current projection instead. View scales are 1 and 2. Circle size and center separation scale, while extension remains exactly 4 paper millimetres beyond each relevant radius. Center-mark cross spans are 18 mm and 28 mm respectively. Centerline end extensions use the same 4 mm value along the current center-to-center direction.', '',
    'All centerline images show both black circle outlines, a teal diagonal through both centers, white center openings/rings and outward line extensions. All center-mark images show a centered cross only on the smaller circle, leaving the second circle unmarked. Default dash [8,1.5,1.2,1.5] and custom [5,1,0.8,1] are visibly distinct. Custom width is visibly stronger. SVG retains exact 0.25/0.55 mm widths; '+(fixed?'DXF uses valid nearest discrete lineweights (0.25/0.53 mm for these center styles and 0.35 mm for the 0.36 mm ring).':'original DXF wrongly writes non-enumerated 0.55 mm and ring 0.36 mm lineweights, prompting repairs.')+' The unchanged teal center color contrasts with black projection edges. No alternative color is supplied by this fixture.', '',
    'The frame, note, view scale text and title block remain visible and unclipped in every image. Centerline captions are clear. However, `center-mark-2-default-dxf.png` and `center-mark-2-custom-dxf.png` show the lower vertical stroke touching/crossing the first word of the view caption. Scale-1 DXF mark captions are also tight/touching. Corresponding SVGs have a small visible gap, not robust clearance. At scale 2 the current center is [97,103], radius 10, so the 4 mm extension ends at y=117. The caption baseline is y=119 and text height is 2.5 mm: its DXF cap ink reaches above the center-stroke end. At scale 1 the same issue is y=100.5 versus baseline102.5. The serializer places the automatic caption from projection bounds alone, before accounting for annotation extents. This is reported to root as a concrete collision; no fixture was moved to conceal it.', '',
    'Proposed scoped correction: retain current caption alignment/text and normal baseline, but clear the resolved center-annotation ink extents plus text height and a paper-space gap. Use shared current-circle geometry and aggregate per view within existing budgets. The equivalent native caption currently has a projected-bottom +5 mm baseline (React parity), while export uses +6 mm, so one shared clearance rule should preserve each existing baseline when no center annotation extends into its space. No renderer or product code was changed during this review.', '',
    'These are independently rendered synthetic SVG/graphical DXF artifacts (resvg 0.45.1; ezdxf 1.4.4 and matplotlib 3.11.2). They do not prove OCC projection, native authoring/File export, physical input, printing, editable CAD dimensions, or untested fonts/layouts.', '',
    '## Per-image result', '',
    ...items.map(i=>'- `'+i.file+'`: '+i.result+'.'+(i.warning?' '+i.warning:'')), ''
  ];
  fs.writeFileSync(path.join(dir,'review.md'),lines.join('\n'));
  console.log(folder,JSON.stringify({images:items.length,exact_geometry:items.every(i=>i.exact_current_projection_endpoint_check),audit_fixes:items.reduce((n,i)=>n+(i.audit_fixes?.length??0),0),same_images:items.filter(i=>i.same_png_as_original).length,model_unchanged:fixed?items.every(i=>i.model_unchanged_from_original):null}));
}
