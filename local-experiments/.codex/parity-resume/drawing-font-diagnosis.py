"""Read-only DXF font diagnosis; override only the in-memory renderer document."""
import hashlib, json, pathlib
import ezdxf, matplotlib
matplotlib.use('Agg')
from matplotlib import font_manager
from fontTools.ttLib import TTFont
from ezdxf.addons.drawing.matplotlib import qsave

root = pathlib.Path(__file__).parent
source = root / 'drawing-advanced-dash-fixed' / 'advanced-mm.dxf'
output = root / 'drawing-advanced-dash-fixed' / 'diagnostic-segoe-symbol-mm.png'
report_path = root / 'drawing-font-diagnosis.json'
assert not output.exists() and not report_path.exists(), 'Preserve previous evidence'
before = hashlib.sha256(source.read_bytes()).hexdigest()
chars = [0x2316, 0x2300, 0x24c2, 0x24c1]
report = {'source': str(source), 'source_sha256': before, 'fonts': []}
for family in ['Arial', 'DejaVu Sans', 'Segoe UI Symbol']:
    path = font_manager.findfont(font_manager.FontProperties(family=family))
    cmap = TTFont(path).getBestCmap()
    report['fonts'].append({'family': family, 'resolved_path': path,
        'sha256': hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest(),
        'glyphs': {f'U+{c:04X}': c in cmap for c in chars}})
doc = ezdxf.readfile(source)
values = [e.dxf.text for e in doc.modelspace() if e.dxftype() == 'TEXT']
report['parsed_unicode_present'] = {f'U+{c:04X}': any(chr(c) in v for v in values) for c in chars}
assert all(report['parsed_unicode_present'].values())
doc.styles.get('STANDARD').set_extended_font_data('Segoe UI Symbol')
qsave(doc.modelspace(), output, bg='#FFFFFF', fg='#202124', dpi=250, size_inches=(8, 6))
assert hashlib.sha256(source.read_bytes()).hexdigest() == before
report['diagnostic_render'] = str(output)
report['diagnostic_render_sha256'] = hashlib.sha256(output.read_bytes()).hexdigest()
report['limitation'] = 'Diagnostic only: saved Arial unchanged. DXF has no font embedding/fallback guarantee.'
report_path.write_text(json.dumps(report, indent=2), encoding='utf-8')
print(report_path)
