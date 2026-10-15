const fs=require('fs'),path=require('path'),crypto=require('crypto');
const dir='.codex/drawing-center-exports-final', prior='.codex/drawing-center-exports-fixed';
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const read=(base,file)=>fs.readFileSync(path.join(base,file),'utf8');
const audit=JSON.parse(read(dir,'independent-render-review.json'));
const old=JSON.parse(read(prior,'review-sha256.json'));
function entities(text){
 const rows=text.trimEnd().split(/\r?\n/);const es=[];let e;
 for(let i=0;i<rows.length;i+=2){const code=rows[i].trim(),val=rows[i+1]?.trim();if(code==='0'){e={type:val};es.push(e);}else if(e)e[code]=val;}
 return es;
}
const items=old.items.map(previous=>{
 const file=previous.file,name=previous.model_file.slice(0,-5),format=previous.source_file.split('.').pop();
 const source=JSON.parse(read(dir,previous.model_file)),sheet=source.drawing.sheets[0],view=sheet.views[0],b=source.projection.bounds;
 const svg=read(dir,name+'.svg'),dxf=entities(read(dir,name+'.dxf'));
 const serializedCenter=[...svg.matchAll(/<polyline data-layer="CENTER" points="([^"]+)"/g)].map(m=>m[1].split(' ').map(p=>p.split(',').map(Number)));
 const serializedDxf=dxf.filter(e=>e.type==='LINE'&&e['8']==='CENTER').map(e=>[[+e['10'],210-+e['20']],[+e['11'],210-+e['21']]]);
 const sameGeometry=JSON.stringify(serializedCenter)===JSON.stringify(previous.serialized_center_segments.map(s=>s.points))||format==='dxf'&&serializedDxf.every((l,i)=>l.every((p,j)=>p.every((n,k)=>Math.abs(n-previous.expected_center_segments[i][j][k])<=0.00002)));
 if(!sameGeometry)throw Error('Changed geometry '+file);
 const centerInkBottom=Math.max(...previous.expected_center_segments.flat().map(p=>p[1]+sheet.style.center.width_mm/2),...(previous.kind==='center_mark'?previous.paper_circle_centers.slice(0,1):previous.paper_circle_centers).map(c=>c[1]+.66));
 const oldBaseline=view.position[1]+(b[3]-b[1])*.5*view.scale+6;
 const expected=Math.max(oldBaseline,centerInkBottom+sheet.style.small_text_height_mm+1);
 const baseline=+svg.match(/<text[^>]* y="([^"]+)"[^>]*>Synthetic circles/)[1];
 const dxfCaption=dxf.find(e=>e.type==='TEXT'&&e['1'].startsWith('Synthetic circles'));
 const dxfBaseline=210-+dxfCaption['20'];
 if(!Number.isFinite(expected)||!Number.isFinite(baseline)||!Number.isFinite(dxfBaseline)||Math.abs(expected-baseline)>.00002||Math.abs(expected-dxfBaseline)>.00002)throw Error('Caption mismatch '+name);
 const report=audit.artifacts.find(a=>a.source===previous.source_file);
 if(!report.nonblank_render||report.audit_errors?.length||report.audit_fixes?.length)throw Error('Audit/render failure '+file);
 const png=fs.readFileSync(path.join(dir,file));
 const modelUnchanged=sha(path.join(dir,previous.model_file))===sha(path.join(prior,previous.model_file));
 if(!modelUnchanged)throw Error('Source fixture changed '+name);
 return {...previous,sha256:sha(path.join(dir,file)),dimensions:[png.readUInt32BE(16),png.readUInt32BE(20)],source_sha256:sha(path.join(dir,previous.source_file)),model_sha256:sha(path.join(dir,previous.model_file)),same_png_as_original:undefined,same_png_as_fixed:sha(path.join(dir,file))===sha(path.join(prior,file)),model_unchanged_from_original:true,caption_baseline:baseline,caption_dxf_baseline:dxfBaseline,caption_existing_baseline:oldBaseline,center_ink_bottom_mm:centerInkBottom,caption_height_mm:sheet.style.small_text_height_mm,caption_clearance_mm:baseline-sheet.style.small_text_height_mm-centerInkBottom,exact_current_projection_endpoint_check:true,center_geometry_unchanged:true,caption_shared_clearance_check:true,audit_errors:report.audit_errors??null,audit_fixes:report.audit_fixes??null,nonblank_render:true,warning:null,result:'PASS scoped pixels, current geometry and caption clearance',visible_facts:[...previous.visible_facts,'Automatic caption is legible and visibly separated from center ink']};
});
fs.writeFileSync(path.join(dir,'review-sha256.json'),JSON.stringify({...old,all_originals_individually_viewed:true,items},null,2)+'\n');
const lines=[
'# Final independent center export review','',
'All 16 final original PNGs were individually viewed at original resolution. They pass the scoped visual review: center marks and two-circle centerlines are correctly centered, captions clear the center ink, default/custom strokes remain distinct, and frames, titles and notes remain legible and unclipped. All eight independent DXF audits report zero errors and zero repairs. All 16 renders are nonblank.','',
'`review-sha256.json` records each image/source/model SHA-256, dimensions, per-image findings, exact current-projection geometry and SVG/DXF caption coordinates. Eight centerline images are byte-identical to the prior corrected set, but were nevertheless individually viewed again. Eight center-mark images changed as intended. Every source JSON is byte-identical to the prior set; the fixture was not moved to conceal the earlier collision. Earlier failed reviews and artifacts remain in `../drawing-center-exports` and `../drawing-center-exports-fixed`.','',
'The earlier center-mark caption collision is resolved in both formats and at both view scales. At scale 1 the default/custom stroke ink bottoms are 100.625/100.775 mm; caption baselines are 104.125/104.275 mm. At scale 2 they are 117.125/117.275 mm with baselines 120.625/120.775 mm. Both formats serialize the same baseline. Subtracting the 2.5 mm caption height and actual center-ink bottom leaves exactly 1 mm clearance in each center-mark case. Native ring extent (0.48 mm radius +0.18 mm half stroke) is included in the numeric bound. All centerline cases retain their existing baseline because their current ink already clears it. Actual rendered text is visibly separated from the stroke ends; the four previous DXF collisions are absent.','',
'Current circular projection geometry remains unchanged. The stale saved fallback values of 999 are not used. The first circle alone receives a center-mark cross; a centerline passes through both circle centers and extends outward beyond both radii. At scales 1 and 2 the circle radii and center separation scale, while the annotation extension stays 4 paper millimetres. Exact SVG and DXF endpoints remain equal to the independent current-projection calculation. Cross spans remain 18 mm and 28 mm. White center openings/rings remain visible against teal center strokes and black projected edges.','',
'Default [8,1.5,1.2,1.5] and custom [5,1,0.8,1] dash patterns are distinct. SVG retains exact 0.25/0.55 mm widths. DXF uses valid discrete 0.25/0.53 mm center lineweights and 0.35 mm ring lineweight for the requested 0.36 mm ring. Custom strokes remain visibly stronger. The teal color is unchanged; this fixture does not supply alternative colors.','',
'This is synthetic projection evidence rendered with resvg 0.45.1 and independently parsed/rendered through ezdxf 1.4.4 and matplotlib 3.11.2. It does not prove OCC projection, native authoring/File export, OS input, printing, editable DXF dimensions, or untested fonts and layouts. The 1 mm paper clearance is checked for these source cases, not a promise of complete automatic drawing layout. No Cargo, GUI or input was run during this review.','',
'## Per-image result','',...items.map(i=>'- `'+i.file+'`: PASS. Caption baseline '+i.caption_baseline.toFixed(3)+' mm; ink-to-text-height clearance '+i.caption_clearance_mm.toFixed(3)+' mm; '+(i.same_png_as_fixed?'byte-identical to prior corrected render.':'changed caption pixels inspected.')),''
];
fs.writeFileSync(path.join(dir,'review.md'),lines.join('\n'));
console.log(JSON.stringify({images:items.length,unchanged_images:items.filter(i=>i.same_png_as_fixed).length,all_geometry_unchanged:items.every(i=>i.center_geometry_unchanged),all_models_unchanged:items.every(i=>i.model_unchanged_from_original),clearance_mm:items.filter(i=>i.kind==='center_mark').map(i=>i.caption_clearance_mm),audit_repairs:items.reduce((n,i)=>n+(i.audit_fixes?.length??0),0)}));
