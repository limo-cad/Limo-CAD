import hashlib, json, pathlib

for variant in ('default', 'cjk'):
    root = pathlib.Path('.codex') / ('drawing-cloud-caption-' + variant)
    report = json.loads((root / 'independent-render-review.json').read_text(encoding='utf-8'))
    rows = []
    for row in report['artifacts']:
        for name, expected in ((row['source'], row['sha256']), (row['render'], row['render_sha256'])):
            assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected
        if row['source'].endswith('.dxf'):
            assert row['strict_parse_passed'] and not row['audit_errors'] and not row['audit_fixes']
        glyph_limit = variant == 'default' and row['source'].endswith('.dxf') and 'clipped-right' not in row['source']
        rows.append({**row, 'original_png_visually_reviewed': True,
                     'caption_clearance': 'pass', 'cloud_geometry': 'pass',
                     'unicode_pixels': 'missing CJK glyphs in declared Arial family' if glyph_limit else 'pass'})
    models = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in root.glob('cloud-*.json')}
    (root / 'review-sha256.json').write_text(json.dumps({'source_tree': '33b5aed9', 'models': models, 'artifacts': rows}, ensure_ascii=False, indent=2), encoding='utf-8')
    (root / 'review.md').write_text(
        '# Cloud caption and declared-font review\n\n'
        'All eight original PNGs were visually reviewed. Triangle, quadrilateral, saved seven-vertex cloud, and right-paper-clipped cloud retain the intended scallops and red strokes. The complete multiline captions are now clear of the cloud, including the formerly crossing quadrilateral caption. The right-clipped shape crosses the inset border intentionally and stops at the actual paper edge; the border is not the clipping boundary. Title blocks remain readable. Four DXFs audit with zero errors and zero repairs; source/render hashes match the independent render report.\n\n'
        + ('The saved style explicitly selects Microsoft YaHei, Arial, sans-serif. All eight renders show the tested Latin, accented, Greek and CJK text correctly. No review-only font substitution or output-file patch was used.\n\n' if variant == 'cjk' else 'The saved default style selects Arial, Helvetica, sans-serif. All SVGs show the tested Unicode text; three DXFs still show missing-glyph boxes for the two Chinese characters. Exact Unicode is present in the DXF, but declared Arial lacks those glyphs in this viewer. This is a retained font-coverage limitation, not a universal Unicode pixel pass. The explicit CJK-style companion set demonstrates correct text with a suitable installed family.\n\n')
        + 'This is synthetic export evidence. It does not prove live native authoring, File dialog input, OCCT projection, other viewers/font installations, or printing. Original failed captions and font renders remain in the earlier evidence directories.\n', encoding='utf-8')
print('Recorded 16 original image reviews; retained default Arial CJK limitation')
