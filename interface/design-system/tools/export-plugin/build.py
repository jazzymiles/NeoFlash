"""Inject exports/manifest.json into code.src.js -> code.js (run after editing the manifest)."""
import json, os

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.normpath(os.path.join(here, '..', '..'))
items = json.load(open(os.path.join(root, 'exports', 'manifest.json')))
keep = [{k: i[k] for k in ('id', 'group', 'theme', 'png', 'svg')} for i in items]
src = open(os.path.join(here, 'code.src.js')).read()
out = src.replace('/*ITEMS*/[]', json.dumps(keep, ensure_ascii=False))
open(os.path.join(here, 'code.js'), 'w').write(out)
print(f'code.js: {len(keep)} items')
