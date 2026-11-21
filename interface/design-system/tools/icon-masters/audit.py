"""Geometry audit of icon masters: lopsided arrowheads, near-miss mirror symmetry, non-round circles, kinked curves."""
import sys, os, re, glob, math
TOKEN = re.compile(r'[A-Za-z]|-?\d*\.?\d+(?:e-?\d+)?')
def parse(d):
    toks = TOKEN.findall(d); out = []; i = 0; cmd = None
    while i < len(toks):
        t = toks[i]
        if t.isalpha():
            cmd = t; i += 1
            if cmd in 'Zz': out.append(('Z', [])); continue
        if cmd not in 'MLHVC': raise ValueError('cmd ' + cmd)
        n = {'M': 2, 'L': 2, 'H': 1, 'V': 1, 'C': 6}[cmd]
        out.append((cmd, [float(x) for x in toks[i:i + n]])); i += n
        if cmd == 'M': cmd = 'L'
    return out
def subpaths(svg):
    """-> list of (stroked, closed, segs) ; seg = ('L', p0, p1) | ('C', p0, c1, c2, p1)"""
    res = []
    for tag in re.findall(r'<path[^>]*>', svg):
        stroked = 'stroke="' in tag and 'stroke="none"' not in tag
        d = re.search(r' d="([^"]+)"', tag).group(1)
        cur = start = (0, 0); segs = []
        def flush(closed=False):
            nonlocal segs
            if segs: res.append((stroked, closed, segs))
            segs = []
        for cmd, a in parse(d):
            if cmd == 'M': flush(); cur = start = (a[0], a[1]); continue
            if cmd == 'Z':
                if math.dist(cur, start) > 1e-6: segs.append(('L', cur, start))
                cur = start; flush(True); continue
            if cmd == 'C':
                segs.append(('C', cur, (a[0], a[1]), (a[2], a[3]), (a[4], a[5]))); cur = (a[4], a[5]); continue
            nxt = {'L': lambda: (a[0], a[1]), 'H': lambda: (a[0], cur[1]), 'V': lambda: (cur[0], a[0])}[cmd]()
            if math.dist(cur, nxt) > 1e-6: segs.append(('L', cur, nxt))
            cur = nxt
        flush()
    return res
def sample(seg, n=12):
    if seg[0] == 'L':
        (x0, y0), (x1, y1) = seg[1], seg[2]
        k = max(2, int(math.dist(seg[1], seg[2]) * 3))
        return [(x0 + (x1 - x0) * t / k, y0 + (y1 - y0) * t / k) for t in range(k + 1)]
    p0, c1, c2, p1 = seg[1:]
    pts = []
    for i in range(n + 1):
        t = i / n; u = 1 - t
        pts.append(tuple(u**3 * p0[k] + 3 * u * u * t * c1[k] + 3 * u * t * t * c2[k] + t**3 * p1[k] for k in (0, 1)))
    return pts
def hausdorff(a, b):
    return max(max(min(math.dist(p, q) for q in b) for p in a), max(min(math.dist(p, q) for q in a) for p in b))
def near_mirror(pts):
    """smallest mirror mismatch over axis candidates; returns (axis, coord, mismatch) for x and y mirrors"""
    out = []
    for ax in (0, 1):
        lo = min(p[ax] for p in pts); hi = max(p[ax] for p in pts)
        best = None
        for c in {round((lo + hi) / 2, 3), round((lo + hi)) / 2, math.floor(lo + hi) / 2 + 0.25, math.floor(lo + hi) / 2 - 0.25}:
            m = [((2 * c - p[0], p[1]) if ax == 0 else (p[0], 2 * c - p[1])) for p in pts]
            h = hausdorff(pts, m)
            if best is None or h < best[2]: best = (ax, c, h)
        out.append(best)
    return out
def tangent(seg, end):
    if seg[0] == 'L':
        a, b = seg[1], seg[2]
    elif end == 0:
        a, b = seg[1], (seg[2] if math.dist(seg[1], seg[2]) > 1e-6 else seg[3])
    else:
        a, b = (seg[3] if math.dist(seg[3], seg[4]) > 1e-6 else seg[2]), seg[4]
    d = math.dist(a, b) or 1
    return ((b[0] - a[0]) / d, (b[1] - a[1]) / d)
def audit(svg):
    issues = []
    subs = subpaths(svg)
    allpts = []
    for si, (stroked, closed, segs) in enumerate(subs):
        pts = [p for s in segs for p in sample(s)]
        allpts += pts
        # arrowheads / chevrons: two short straight arms meeting at a vertex
        n = len(segs)
        for i in range(n if closed else n - 1):
            a, b = segs[i], segs[(i + 1) % n]
            if a[0] == 'L' and b[0] == 'L' and stroked and n <= 3 and not closed:
                la, lb = math.dist(a[1], a[2]), math.dist(b[1], b[2])
                if 0.03 < abs(la - lb) < 1.3 and max(la, lb) < 7:
                    issues.append('arms %.2f/%.2f at (%.2f,%.2f)' % (la, lb, a[2][0], a[2][1]))
            # kinks between curve joins
            if a[0] == 'C' or b[0] == 'C':
                t0, t1 = tangent(a, 1), tangent(b, 0)
                ang = math.degrees(math.acos(max(-1, min(1, t0[0] * t1[0] + t0[1] * t1[1]))))
                if 2.0 < ang < 30:
                    issues.append('kink %.0f° at (%.2f,%.2f)' % (ang, a[-1][0], a[-1][1]))
        # circles / ellipses: closed all-curve subpaths
        if closed and all(s[0] == 'C' for s in segs) and len(segs) in (4, 8):
            xs = [p[0] for p in pts]; ys = [p[1] for p in pts]
            w, h = max(xs) - min(xs), max(ys) - min(ys)
            cx, cy = (max(xs) + min(xs)) / 2, (max(ys) + min(ys)) / 2
            if abs(w - h) < 0.6:
                r = (w + h) / 4
                dev = max(abs(math.dist(p, (cx, cy)) - r) for p in pts)
                if abs(w - h) > 0.02 or dev > 0.03:
                    issues.append('circle w%.2f h%.2f dev%.2f' % (w, h, dev))
        # per-subpath near-miss symmetry
        if len(pts) > 3:
            for ax, c, h in near_mirror(pts):
                if 0.03 < h < 0.45:
                    issues.append('sub%d near-%s-mirror %.2f @%.2f' % (si, 'xy'[ax], h, c))
    if allpts:
        for ax, c, h in near_mirror(allpts):
            if 0.03 < h < 0.45:
                issues.append('ICON near-%s-mirror %.2f @%.2f' % ('xy'[ax], h, c))
    return issues
if __name__ == '__main__':
    bad = 0; files = sorted(glob.glob(sys.argv[1] + '/*.svg'))
    for f in files:
        try: iss = audit(open(f).read())
        except Exception as e: iss = ['PARSE ' + str(e)]
        if iss:
            bad += 1; print(os.path.basename(f)[:-4].ljust(30), '; '.join(iss[:6]) + (' …+%d' % (len(iss) - 6) if len(iss) > 6 else ''))
    print('--', bad, 'of', len(files), 'flagged')
