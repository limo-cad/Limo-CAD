import hashlib
import json
from pathlib import Path

root = Path(__file__).parent
annotations = root / 'ci-linux-annotations-c78d1c5b'
images = sorted(annotations.glob('*/evidence/*.png'))
assert len(images) == 36, len(images)
(annotations / 'review.md').write_text('''Reviewed all 36 original native captures from run 36348817288, commit c78d1c5b: 14 chamfer and 22 revision-cloud images, at fixed 100% and 200% scale. All four fixture reports pass. Real chamfer picks, paper placement, label dragging, edited setback/angle and Greek text are legible. Loaded seven-vertex clouds, staging, triangle/quad creation, translation, deletion and restoration match the report. The complete 200% window fits the private display.

Observed limitations: Place chamfer note is clipped in these images (shortened to Place note in the next source change; fresh pixels pending). The prescribed quad reaches the title-block border and crosses it after dragging; this is not automatic layout validation. These old captures do not validate the new multiline cloud caption helper or HoleNote. No hardware DPI transition, macOS, Windows, or Wayland proof is inferred.
''', encoding='utf-8')
(annotations / 'review-sha256.json').write_text(json.dumps({str(p.relative_to(annotations)): hashlib.sha256(p.read_bytes()).hexdigest() for p in images}, indent=2), encoding='utf-8')

mac = root / 'ci-macos-ime-native-36349702501'
evidence = mac / 'native-input-macos/native-input-macos-japanese'
report = json.loads((evidence / 'ime-result.json').read_text(encoding='utf-8'))
assert report['exact_project_unchanged']
assert report['cleanup']['result'] == 'passed'
events = report['cancelled']['ui']['ime_diagnostics']['events']
assert [e['value'] for e in events if e['kind'] == 'commit'] == ['はる']
reviewed = [evidence / name for name in ['ime-preedit.png', 'ime-committed.png', 'ime-cancelled.png']]
(mac / 'review.md').write_text('''Run 36349702501 at 09860f92 passed actual Bevy Japanese composition using CoreGraphics virtual keys and the owned Winit/AppKit view. Independently observed target input context is Japanese, focused, and IME-enabled. Retained events show nonempty preedit, exactly one commit of はる, a second composition, and cancellation after two Escapes. Exact project data is unchanged, the modal remains open, and the prior input-source enabled set and US selection are restored.

Reviewed the three original preedit, committed, and cancelled native captures. Japanese text is legible in the focused Project name field. Candidate-popup pixels/placement, physical keyboard and monitor/DPI transitions remain unvalidated. No local desktop input was used for this review. The earlier failed run is retained; this successful run includes bounded passive waiting for the actual target context and correct full document/window/epoch ownership checks.
''', encoding='utf-8')
(mac / 'review-sha256.json').write_text(json.dumps({str(p.relative_to(mac)): hashlib.sha256(p.read_bytes()).hexdigest() for p in reviewed + [evidence / 'ime-result.json']}, indent=2), encoding='utf-8')
print('Recorded 36 annotation and 3 macOS IME original-image reviews.')
