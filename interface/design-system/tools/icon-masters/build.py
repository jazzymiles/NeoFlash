#!/usr/bin/env python3
"""Build the NF icon masters from their construction sources.

  python3 interface/design-system/tools/icon-masters/build.py            # write exports/icons/{12,16,20}
  python3 interface/design-system/tools/icon-masters/build.py --check    # fail if the exports differ
  python3 interface/design-system/tools/icon-masters/build.py --out DIR  # write somewhere else (proofs)

Every icon is a function in arrows.py, layout_text.py, objects.py or tools.py, drawn with the primitives
of kit.py on the 16 px grid. The 12 and 20 px masters come from the same function. grid20.txt lists the
icons that have a 20 px master (the Tools palette and its neighbours).
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import kit  # noqa: E402
import arrows, layout_text, objects, tools  # noqa: E402,F401,E401

EXPORTS = os.path.normpath(os.path.join(HERE, '..', '..', 'exports', 'icons'))


def main(argv):
    check = '--check' in argv
    out = argv[argv.index('--out') + 1] if '--out' in argv else EXPORTS
    grid20 = set(open(os.path.join(HERE, 'grid20.txt')).read().split())
    written, stale = 0, []
    for name, (fn, sizes) in sorted(kit.REGISTRY.items()):
        for size in sizes:
            if size == 20 and name not in grid20:
                continue
            path = os.path.join(out, str(size), name + '.svg')
            svg = kit.render(name, size)
            if check:
                if not os.path.exists(path) or open(path).read() != svg:
                    stale.append('%d/%s' % (size, name))
                continue
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, 'w') as f:
                f.write(svg)
            written += 1
    if check:
        expected = {'%d/%s' % (s, n) for n, (_, sizes) in kit.REGISTRY.items() for s in sizes if s != 20 or n in grid20}
        present = {'%s/%s' % (s, f[:-4]) for s in ('12', '16', '20') for f in os.listdir(os.path.join(out, s)) if f.endswith('.svg')}
        stale += sorted('%s (no source)' % p for p in present - expected)
        if stale:
            print('icon masters differ from their sources:\n  ' + '\n  '.join(stale))
            return 1
        print('icon masters match their sources: %d files' % len(expected))
        return 0
    print('%d files, %d icons' % (written, len(kit.REGISTRY)))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
