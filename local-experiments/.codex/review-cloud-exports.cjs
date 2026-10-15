const fs=require('fs'),path=require('path'),crypto=require('crypto');
const dir='.codex/drawing-cloud-exports';
const read=n=>fs.readFileSync(path.join(dir,n),'utf8');
const sha=n=>crypto.createHash('sha256').update(fs.readFileSync(path.join(dir,n))).digest('hex');
const report=JSON.parse(read('independent-render-review.json'));
function entities(s){let e;const out=[],r=s.trimEnd().split(/\r?\n/);for(let i=0;i<r.length;i+=2){const c=r[i].trim(),v=r[i+1]?.trim();if(c==='0'){e={type:v};out.push(e);}else if(e)e[c]=v;}return out;}
const observations={
 'cloud-triangle-svg':['PASS scoped pixels','Closed red triangular scallops, two intact revision rows, Greek/accented/CJK glyphs visible, clear of cloud and frame.'],
 'cloud-triangle-dxf':['LIMIT font coverage','Closed red triangular scallops and two rows remain clear. Greek/accented characters visible; Chinese characters render as two missing-glyph squares.'],
 'cloud-quad-svg':['FAIL caption collision','Closed red rectangular scallops. Second revision row crosses the top scallops; Chinese glyphs are present.'],
 'cloud-quad-dxf':['FAIL caption collision; LIMIT font coverage','Second revision row crosses top scallops. Chinese characters also appear as two missing-glyph squares.'],
 'cloud-loaded-seven-svg':['PASS scoped pixels','Seven-vertex saved outline keeps distinct sloping sides and scallops. Two rows and all Unicode characters remain clear of cloud/frame.'],
 'cloud-loaded-seven-dxf':['LIMIT font coverage','Seven-vertex outline and caption clearance agree with SVG. Chinese characters render as two missing-glyph squares; other revision text is legible.'],
 'cloud-clipped-right-svg':['PASS scoped pixels','Left/top/bottom scallops stop at the sheet edge, beyond the inset frame, with no invented right closing side. REV B stays visible inside the sheet.'],
 'cloud-clipped-right-dxf':['PASS scoped pixels','Same open clipped geometry and retained REV B. DXF viewer includes outside paper canvas margin; cloud endpoints stop at x=297 paper mm.']
};
const items=report.artifacts.map(a=>{
 const ext=path.extname(a.source),name=path.basename(a.source,ext),model=name+'.json',doc=JSON.parse(read(model));
 const annotation=doc.drawing.sheets[0].annotations.find(a=>a.kind==='revision_cloud');
 const expected=('REV '+annotation.revision).split(/\r?\n/);
 const svg=read(name+'.svg'),dxf=entities(read(name+'.dxf'));
 const labels=dxf.filter(e=>e.type==='TEXT'&&e['8']==='REVISION');
 if(JSON.stringify(labels.map(e=>e['1']))!==JSON.stringify(expected))throw Error('DXF text changed '+name);
 if(!expected.every(t=>a.text.includes(t)))throw Error('Independent Unicode decode changed '+a.source);
 const segments=dxf.filter(e=>e.type==='LINE'&&e['8']==='REVISION').map(e=>[[+e['10'],210-+e['20']],[+e['11'],210-+e['21']]]);
 const svgSegments=[...svg.matchAll(/<polyline data-layer="REVISION" points="([^"]+)"/g)].map(m=>m[1].split(' ').map(p=>p.split(',').map(Number)));
 if(segments.length!==svgSegments.length||!segments.every((s,i)=>s.every((p,j)=>p.every((v,k)=>Math.abs(v-svgSegments[i][j][k])<0.00002))))throw Error('Format geometry mismatch '+name);
 if(!segments.flat().every(p=>p.every(Number.isFinite)&&p[0]>=0&&p[0]<=297&&p[1]>=0&&p[1]<=210))throw Error('Clip bounds '+name);
 if(!a.nonblank_render||a.audit_errors?.length||a.audit_fixes?.length)throw Error('Audit failure '+a.source);
 if(a.sha256!==sha(a.source)||a.render_sha256!==sha(a.render))throw Error('Source/render hash mismatch '+a.source);
 const png=fs.readFileSync(path.join(dir,a.render)),[result,fact]=observations[path.basename(a.render,'.png')];
 return {file:a.render,sha256:sha(a.render),dimensions:[png.readUInt32BE(16),png.readUInt32BE(20)],source_file:a.source,source_sha256:sha(a.source),model_file:model,model_sha256:sha(model),result,visible_facts:[fact,'White paper, inset frame, title block and fixture note remain legible.'],saved_vertices:annotation.points,exact_decoded_revision_lines:expected,revision_codepoints:[...annotation.revision].map(c=>'U+'+c.codePointAt(0).toString(16).toUpperCase()),dxf_revision_baselines:labels.map(e=>[+e['10'],210-+e['20']]),identical_svg_dxf_segments:true,revision_segment_count:segments.length,dxf_lineweights:[...new Set(dxf.filter(e=>e.type==='LINE'&&e['8']==='REVISION').map(e=>+e['370']))],all_centerlines_inside_sheet:true,clipped_vertices_preserved:annotation.points.some(p=>p[0]>297),dxf_font:'Implicit STANDARD; no font STYLE table or per-TEXT style serialized',audit_errors:a.audit_errors??null,audit_fixes:a.audit_fixes??null,nonblank_render:a.nonblank_render};
});
fs.writeFileSync(path.join(dir,'review-sha256.json'),JSON.stringify({all_originals_individually_viewed:true,review_result:'FAIL inherited multiline caption overlap; DXF CJK pixel coverage limited',render_report_sha256:sha('independent-render-review.json'),manifest_sha256:sha('manifest.json'),items},null,2)+'\n');
const lines=['# Independent revision cloud export review','',
'All eight original PNGs were individually inspected. This set does not pass complete pixel review: the rectangular cloud has an inherited multiline caption collision in both formats, and three DXF renderings lack Chinese glyphs. Preserve these images as failure evidence.','',
'The rectangular source keeps vertices [40,55], [135,55], [135,115], [40,115]. Caption baselines are y=53 and57 mm, so the second row enters the top scallops around y=53.57..55 mm. The SVG and DXF originals both visibly show that overlap. Native cloud placement uses the same baseline and multiline spacing, so this is shared inherited presentation behavior rather than a divergent exporter formula. A shared caption helper can lift earlier rows while preserving the current one-row/last-row baseline; the saved polygon must remain unchanged.','',
'Exact Unicode round trips are confirmed independently: each DXF TEXT entity and ezdxf report retains the saved lines REV B<2> & cω and Café 零件. The SVG has the expected XML escaping. Greek omega and accented e are visible in both formats; Chinese characters render correctly in SVG but become two missing-glyph squares in DXF. The DXF contains no explicit font STYLE table or per-TEXT style, so its implicit STANDARD style depends on the viewer. The current ezdxf/matplotlib rendering path provides no explicit CJK font override. This is a demonstrated font-coverage limit of this rendering setup, not demonstrated serialization corruption; a CJK-capable independent rendering remains required.','',
'All four DXF strict audits have zero errors and zero repairs; all eight renders are nonblank. Every source and render SHA agrees with the independent report. SVG and DXF serialize identical cloud centerline segments within 0.00002 mm rounding, with the fixed red revision layer, exact SVG 0.45 mm width and valid DXF 0.40 mm lineweight. The triangle and loaded seven-vertex outlines remain visibly scalloped and correctly shaped. The clipped-right source still retains x=310 mm vertices; both outputs terminate centerlines at x=297 mm without inventing a closing side. Crossing the inset x=287 mm frame is intended: the clip boundary is the sheet, not the frame. DXF viewer canvas margins differ from the SVG page crop.','',
'`review-sha256.json` records all image/source/model SHA-256 values, sizes, exact decoded text/code points, original vertices, baselines, matching segment counts and per-image outcomes. This is synthetic saved-paper annotation evidence using resvg 0.45.1 and ezdxf 1.4.4/matplotlib 3.11.2. It does not prove OCC projection, native authoring/File export, physical input, printing, editable CAD cloud entities or complete font coverage. No Cargo, GUI or input was run in this review.','',
'## Per-image outcome','',...items.map(i=>'- `'+i.file+'`: '+i.result+'. '+i.visible_facts[0]),''];
fs.writeFileSync(path.join(dir,'review.md'),lines.join('\n'));
console.log(JSON.stringify({images:items.length,caption_failures:items.filter(i=>i.result.startsWith('FAIL')).length,font_limits:items.filter(i=>i.result.includes('font coverage')).length,strict_dxf_audits:4,exact_unicode:true}));
