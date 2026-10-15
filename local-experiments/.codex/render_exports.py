import collections, hashlib, json, pathlib, subprocess, sys
import xml.etree.ElementTree as ET
import ezdxf, matplotlib
from PIL import Image, ImageChops
matplotlib.use('Agg')
from ezdxf.addons.drawing.matplotlib import qsave
from ezdxf.fonts import fonts

root = pathlib.Path(sys.argv[1]).resolve()
renderer = pathlib.Path(sys.argv[2]).resolve()
report_path = root / 'independent-render-review.json'
if report_path.exists():
    raise SystemExit('Preserve prior review evidence')
report = {'ezdxf': ezdxf.__version__, 'matplotlib': matplotlib.__version__, 'svg_renderer': 'resvg 0.45.1 with text and system-fonts', 'artifacts': []}
def audit_item(item):
    entity = getattr(item, 'entity', None)
    return {'code': int(item.code), 'message': item.message,
            'entity': None if entity is None else str(entity)}

for source in sorted(root.glob('*')):
    if source.suffix not in ('.svg', '.dxf'):
        continue
    image = source.with_name(source.stem + '-' + source.suffix[1:] + '.png')
    if image.exists():
        raise SystemExit(f'Preserve prior rendered image: {image}')
    row = {'source': source.name, 'sha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'render': image.name}
    if source.suffix == '.svg':
        tree = ET.parse(source)
        row['text'] = [''.join(e.itertext()) for e in tree.iter() if e.tag.endswith('}text')]
        subprocess.run([str(renderer), str(source), str(image)], check=True)
    else:
        doc = ezdxf.readfile(source)
        audit = doc.audit()
        row['audit_errors'] = [audit_item(e) for e in audit.errors]
        row['audit_fixes'] = [audit_item(e) for e in audit.fixes]
        row['entities'] = dict(collections.Counter(e.dxftype() for e in doc.modelspace()))
        row['text'] = [e.dxf.text for e in doc.modelspace() if e.dxftype() == 'TEXT']
        row['font_styles'] = [{'name': s.dxf.name, 'filename': s.dxf.font, 'extended': s.get_extended_font_data()} for s in doc.styles]
        row['text_font_faces'] = sorted({str(fonts.get_entity_font_face(e, doc)) for e in doc.modelspace() if e.dxftype() == 'TEXT'})
        row['strict_parse_passed'] = not audit.errors and not audit.fixes
        if audit.errors:
            report['artifacts'].append(row)
            continue
        qsave(doc.modelspace(), image, bg='#FFFFFF', fg='#202124', dpi=250, size_inches=(8, 6))
    with Image.open(image) as rendered:
        rgb = rendered.convert('RGB')
        row['image_size'] = list(rgb.size)
        row['rgb_extrema'] = rgb.getextrema()
        row['ink_bounds'] = ImageChops.difference(rgb, Image.new('RGB', rgb.size, 'white')).getbbox()
        row['nonblank_render'] = row['ink_bounds'] is not None
    row['render_sha256'] = hashlib.sha256(image.read_bytes()).hexdigest()
    report['artifacts'].append(row)
report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
if not report['artifacts']:
    raise SystemExit('No export artifacts found')
if any(r.get('strict_parse_passed') is False for r in report['artifacts']):
    raise SystemExit('DXF audit did not pass strictly; inspect retained evidence')
if any(r.get('nonblank_render') is not True for r in report['artifacts']):
    raise SystemExit('Export render is blank or missing; inspect retained evidence')
print(f"Retained {len(report['artifacts'])} independently rendered artifacts")
