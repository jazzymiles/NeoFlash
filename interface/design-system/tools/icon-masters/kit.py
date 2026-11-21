"""NF icon masters — construction kit.

Every icon is drawn from exact primitives on the 16 px grid and re-fitted to the 12 and 20 px grids.
Nothing is hinted point by point: a coordinate is mapped as "axis + whole steps", so a shape that is
symmetric about its axis at 16 px is symmetric at every size, circles stay circles and arrowheads keep
equal arms.

Grid rules (owner decision 2026-11-07, checked by crates/neoflash-ui/tests/icon_masters.rs):
  12 / 16 px — 1 pt strokes, straight strokes on half pixels, fill edges on whole pixels;
  20 px      — 1.5 pt strokes on .25 / .75, fill edges on half points.

Authoring coordinates are always 16 px space. Stroke axes sit on n + 0.5, fill edges on n.
An icon whose axis of symmetry carries a stroke (plus, arrow, text cursor) uses the axis 8.5.
"""
import math

COLOR = '#9AA3B6'
KAPPA = 0.5522847498


def fmt(v):
    s = ('%.3f' % v).rstrip('0').rstrip('.')
    return '0' if s in ('-0', '') else s


class Frame:
    """An axis in 16 px space and its position on the target grid."""

    def __init__(self, a, target):
        self.a, self.t = a, target


class G:
    def __init__(self, size):
        self.s = size
        self.k = size / 16.0
        self.w = 1.5 if size == 20 else 1.0
        self.step = 0.5 if size == 20 else 1.0
        self.stroke_phase = 0.25 if size == 20 else 0.5   # stroke axes ≡ phase (mod step)
        self.strokes, self.fills, self.extra = [], [], []
        self.fx = [Frame(8.0, size / 2.0)]
        self.fy = [Frame(8.0, size / 2.0)]
        self.mx = None                            # mirror axis (16 px space) while drawing a mirrored twin
        self.tf = None                            # rotation (cos, sin, cx, cy) while drawing turned artwork
        self.nat = False                          # coordinates are given in the target grid

    # ── mapping ────────────────────────────────────────────────────────────────────────────────
    def _map(self, v, frames):
        if self.s == 16 or self.tf is not None or self.nat:
            return v                              # turned artwork is fitted as a whole in _emit
        f = frames[-1]
        d = v - f.a
        t = d * self.k
        frac = v % 1.0
        if abs(frac - 0.5) < 1e-6:
            phase = self.stroke_phase
        elif frac < 1e-6 or frac > 1 - 1e-6:
            phase = 0.0
        else:
            return f.t + t                       # free point (diagonal end, curve handle)
        if abs(d) < 1e-9:
            return f.t
        r0 = (phase - f.t) % self.step            # offsets from the axis that land on the phase
        if min(r0, self.step - r0) > 1e-6 and abs(r0 - self.step / 2) > 1e-6:
            return f.t + t                        # axis itself is off the grid: keep proportions
        if r0 > self.step - 1e-6:
            r0 = 0.0
        m = abs(t)
        n = math.floor((m - r0) / self.step + 0.5 + 1e-9)
        if n < 0:
            n = 0
        val = r0 + n * self.step
        if val < 1e-9:
            val = self.step                       # never collapse onto the axis
        return f.t + (val if d > 0 else -val)

    def X(self, x):
        return self._map(x, self.fx)

    def Y(self, y):
        return self._map(y, self.fy)

    def _fx(self, x):
        return x if self.mx is None else 2 * self.mx - x

    def _fa(self, a):
        return a if self.mx is None else 180.0 - a

    def P(self, x, y):
        return (self.X(self._fx(x)), self.Y(y))

    class _Flip:
        def __init__(self, g, c):
            self.g, self.c = g, c

        def __enter__(self):
            self.prev, self.g.mx = self.g.mx, self.c
            return self.g

        def __exit__(self, *a):
            self.g.mx = self.prev

    def flip(self, c=8.0, on=True):
        """Draw the mirror image about x = c: a mirrored twin is exact by construction."""
        return G._Flip(self, c if on else None)

    def L(self, v, lo=None):
        """A length that is not tied to the grid (corner radius, dot radius)."""
        if self.tf is not None or self.nat:
            return max(v, lo) if lo else v
        r = v * self.k
        if self.s != 16:
            q = self.step / 2
            r = max(q, round(r / q) * q)
        return max(r, lo) if lo else r

    def axis(self, ax=8.0, ay=8.0):
        """Declare the icon's axes of symmetry (16 px space). 8 = between pixels, 8.5 = on a stroke."""
        self.fx = [Frame(ax, self._axis_target(ax))]
        self.fy = [Frame(ay, self._axis_target(ay))]
        return self

    def _axis_target(self, a):
        c = self.s / 2.0
        if self.s == 16:
            return a
        e = a - 8.0
        if abs(e) < 1e-9:
            return c
        if abs(abs(e) - 0.5) < 1e-9:              # stroke on the axis: nearest stroke position to the centre
            return c + math.copysign(self.stroke_phase, e)
        base = [Frame(8.0, c)]
        return self._map(a, base)

    class _At:
        def __init__(self, g, x, y):
            self.g, self.x, self.y = g, x, y

        def __enter__(self):
            g = self.g
            if self.x is not None:
                self.x = g._fx(self.x)
            tx = g.X(self.x) if self.x is not None else None
            ty = g.Y(self.y) if self.y is not None else None
            if tx is not None:
                g.fx.append(Frame(self.x, tx))
            if ty is not None:
                g.fy.append(Frame(self.y, ty))
            return g

        def __exit__(self, *a):
            if self.x is not None:
                self.g.fx.pop()
            if self.y is not None:
                self.g.fy.pop()

    def at(self, x=None, y=None):
        """Local axis: coordinates inside are fitted symmetrically about (x, y)."""
        return G._At(self, x, y)

    # ── path plumbing ──────────────────────────────────────────────────────────────────────────
    @staticmethod
    def _d(sub):
        out = []
        for c in sub:
            if c[0] == 'Z':
                out.append('Z')
            else:
                out.append(c[0] + ' ' + ' '.join(fmt(v) for v in c[1:]))
        return ' '.join(out)

    def _emit(self, sub, fill):
        if self.tf is not None:
            c, s_, cx, cy, px, py = self.tf
            k = self.k

            def tp(x, y):                           # turn about the pivot, then scale about its fitted position
                dx, dy = x - cx, y - cy
                return (px + (dx * c - dy * s_) * k, py + (dx * s_ + dy * c) * k)
            out = []
            for cmd in sub:
                pts = []
                for i in range(1, len(cmd), 2):
                    pts += tp(cmd[i], cmd[i + 1])
                out.append((cmd[0],) + tuple(pts))
            sub = out
        (self.fills if fill else self.strokes).append(self._d(sub))

    class _Rot:
        def __init__(self, g, deg, cx, cy):
            self.g, self.v = g, (math.cos(math.radians(deg)), math.sin(math.radians(deg)), cx, cy)

        def __enter__(self):
            g = self.g
            self.prev = g.tf
            g.tf = self.v + (g.X(self.v[2]), g.Y(self.v[3]))   # pivot fitted before turning
            return self.g

        def __exit__(self, *a):
            self.g.tf = self.prev

    class _Native:
        def __init__(self, g):
            self.g = g

        def __enter__(self):
            self.prev, self.g.nat = self.g.nat, True
            return self.g

        def __exit__(self, *a):
            self.g.nat = self.prev

    def native(self):
        """Coordinates inside are already on this size's grid (a hand-fitted 12 or 20 px variant)."""
        return G._Native(self)

    def rotate(self, deg, cx=8.0, cy=8.0):
        """Draw upright, show turned by `deg` clockwise about (cx, cy). Turned artwork is scaled, not snapped."""
        return G._Rot(self, deg, cx, cy)

    def raw(self, svg):
        """Verbatim element for the rare icon that needs a gradient."""
        self.extra.append(svg)

    # ── primitives (16 px coordinates) ─────────────────────────────────────────────────────────
    def line(self, x0, y0, x1, y1):
        a, b = self.P(x0, y0), self.P(x1, y1)
        self._emit([('M',) + a, ('L',) + b], False)

    def poly(self, pts, close=False, fill=False):
        m = [self.P(*p) for p in pts]
        sub = [('M',) + m[0]] + [('L',) + p for p in m[1:]]
        if close or fill:
            sub.append(('Z',))
        self._emit(sub, fill)

    def _rrect(self, l, t, r, b, rad):
        if rad <= 1e-6:
            return [('M', l, t), ('L', r, t), ('L', r, b), ('L', l, b), ('Z',)]
        rad = min(rad, (r - l) / 2, (b - t) / 2)
        k = rad * (1 - KAPPA)
        return [('M', l + rad, t), ('L', r - rad, t), ('C', r - k, t, r, t + k, r, t + rad),
                ('L', r, b - rad), ('C', r, b - k, r - k, b, r - rad, b),
                ('L', l + rad, b), ('C', l + k, b, l, b - k, l, b - rad),
                ('L', l, t + rad), ('C', l, t + k, l + k, t, l + rad, t), ('Z',)]

    def rect(self, l, t, r, b, rad=0.0, fill=False):
        L_, T_, R_, B_ = self.X(self._fx(l)), self.Y(t), self.X(self._fx(r)), self.Y(b)
        if L_ > R_:
            L_, R_ = R_, L_
        self._emit(self._rrect(L_, T_, R_, B_, self.L(rad) if rad else 0.0), fill)

    def _ellipse(self, cx, cy, rx, ry):
        kx, ky = rx * KAPPA, ry * KAPPA
        return [('M', cx + rx, cy), ('C', cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry),
                ('C', cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy),
                ('C', cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry),
                ('C', cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy), ('Z',)]

    def _round_geom(self, cx, cy, r):
        """Centre and radii on the target grid: the four extremes land exactly where straight lines at
        cx ± r and cy ± r land, so a line that meets the curve at an extreme still meets it at every size."""
        cx = self._fx(cx)
        l, rr = self.X(cx - r), self.X(cx + r)
        t, b = self.Y(cy - r), self.Y(cy + r)
        return (l + rr) / 2, (t + b) / 2, (rr - l) / 2, (b - t) / 2

    def _circle_geom(self, cx, cy, r):
        """A full circle stays a circle: one radius, taken from the horizontal extremes."""
        x, y, rx, _ = self._round_geom(cx, cy, r)
        return x, y, rx

    def circle(self, cx, cy, r, fill=False, hole=False):
        c = self._circle_geom(cx, cy, r)
        sub = self._ellipse(c[0], c[1], c[2], c[2])
        if hole:                                   # opposite winding: cuts a hole in a filled shape
            sub = self._reverse(sub)
        self._emit(sub, fill or hole)

    @staticmethod
    def _reverse(sub):
        pts = [(sub[0][1], sub[0][2])]
        segs = []
        for c in sub[1:]:
            if c[0] == 'L':
                segs.append(('L', pts[-1], (c[1], c[2])))
                pts.append((c[1], c[2]))
            elif c[0] == 'C':
                segs.append(('C', pts[-1], (c[1], c[2]), (c[3], c[4]), (c[5], c[6])))
                pts.append((c[5], c[6]))
        out = [('M',) + pts[-1]]
        for seg in reversed(segs):
            if seg[0] == 'L':
                out.append(('L',) + seg[1])
            else:
                out.append(('C',) + seg[3] + seg[2] + seg[1])
        out.append(('Z',))
        return out

    def dash_arc(self, cx, cy, r, n, a0=0.0, gap=24.0):
        """Circle drawn as n equal dashes; `gap` in degrees between dash ends (round caps close part of it)."""
        x, y, rad = self._circle_geom(cx, cy, r)
        step = 360.0 / n
        for i in range(n):
            s0, s1 = a0 + i * step + gap / 2, a0 + (i + 1) * step - gap / 2
            a = math.radians(s0)
            self._emit([('M', x + rad * math.cos(a), y + rad * math.sin(a))] + self._arc_cmds(x, y, rad, s0, s1), False)

    def dash_rect(self, l, t, r, b, n=3):
        """Rectangle of dashes: every side has n equal dashes and the corners are solid."""
        self.dashes(l, t, r, t, n)
        self.dashes(r, t, r, b, n)
        self.dashes(r, b, l, b, n)
        self.dashes(l, b, l, t, n)

    def ellipse(self, cx, cy, rx, ry, fill=False):
        cx = self._fx(cx)
        l, rr, t, b = self.X(cx - rx), self.X(cx + rx), self.Y(cy - ry), self.Y(cy + ry)
        self._emit(self._ellipse((l + rr) / 2, (t + b) / 2, (rr - l) / 2, (b - t) / 2), fill)

    def dot(self, cx, cy, r=1.0):
        """Filled dot. Free radius; the centre follows the grid."""
        x, y = self.P(cx, cy)
        if self.nat or self.tf is not None:
            rad = r
        else:
            rad = r * self.k if self.s != 12 else max(r * self.k, 0.75)
        self._emit(self._ellipse(x, y, rad, rad), True)

    @staticmethod
    def _arc_cmds(cx, cy, r, a0, a1, ry=None):
        """Cubic segments of an arc; angles in degrees, 0 = +x, 90 = +y (down), a1 may be < a0."""
        ry = r if ry is None else ry
        n = max(1, int(math.ceil(abs(a1 - a0) / 90.0 - 1e-9)))
        da = math.radians(a1 - a0) / n
        h = 4.0 / 3.0 * math.tan(da / 4.0)
        out = []
        a = math.radians(a0)
        for _ in range(n):
            b = a + da
            x0, y0 = cx + r * math.cos(a), cy + ry * math.sin(a)
            x1, y1 = cx + r * math.cos(b), cy + ry * math.sin(b)
            out.append(('C', x0 - h * r * math.sin(a), y0 + h * ry * math.cos(a),
                        x1 + h * r * math.sin(b), y1 - h * ry * math.cos(b), x1, y1))
            a = b
        return out

    def arc(self, cx, cy, r, a0, a1, ry=None):
        """Open arc of a circle (or ellipse with ry)."""
        a0, a1 = self._fa(a0), self._fa(a1)
        if ry is None:
            x, y, rad, rady = self._round_geom(cx, cy, r)
        else:
            cx = self._fx(cx)
            l, rr, t, b = self.X(cx - r), self.X(cx + r), self.Y(cy - ry), self.Y(cy + ry)
            x, y, rad, rady = (l + rr) / 2, (t + b) / 2, (rr - l) / 2, (b - t) / 2
        a = math.radians(a0)
        start = ('M', x + rad * math.cos(a), y + rady * math.sin(a))
        self._emit([start] + self._arc_cmds(x, y, rad, a0, a1, rady), False)

    def path(self, cmds, fill=False, close=False):
        """Free path: [('M', x, y), ('L', x, y), ('C', x1, y1, x2, y2, x, y), ('A', cx, cy, r, a0, a1)].

        Anchors follow the grid; a curve handle keeps its offset from its anchor, so tangents survive.
        'A' continues from the current point along a circle arc (centre and radius fitted as a circle).
        """
        sub, cur16 = [], None
        fx, flipped = self._fx, []
        for c in cmds:                              # mirror first, then fit
            if c[0] in 'ML':
                flipped.append((c[0], fx(c[1]), c[2]))
            elif c[0] == 'C':
                flipped.append(('C', fx(c[1]), c[2], fx(c[3]), c[4], fx(c[5]), c[6]))
            elif c[0] == 'A':
                flipped.append(('A', fx(c[1]), c[2], c[3], self._fa(c[4]), self._fa(c[5])))
            else:
                flipped.append(c)
        cmds, saved, self.mx = flipped, self.mx, None
        try:
            self._path(cmds, sub, fill, close)
        finally:
            self.mx = saved

    def _path(self, cmds, sub, fill, close):
        cur16 = None
        for c in cmds:
            if c[0] in 'ML':
                p = self.P(c[1], c[2])
                sub.append((c[0],) + p)
                cur16, cur = (c[1], c[2]), p
            elif c[0] == 'C':
                p = self.P(c[5], c[6])
                h1 = (cur[0] + (c[1] - cur16[0]) * self.k, cur[1] + (c[2] - cur16[1]) * self.k)
                h2 = (p[0] + (c[3] - c[5]) * self.k, p[1] + (c[4] - c[6]) * self.k)
                sub.append(('C',) + h1 + h2 + p)
                cur16, cur = (c[5], c[6]), p
            elif c[0] == 'A':
                x, y, rad, rady = self._round_geom(c[1], c[2], c[3])
                a = math.radians(c[4])
                sp = (x + rad * math.cos(a), y + rady * math.sin(a))
                if not sub:
                    sub.append(('M',) + sp)
                elif math.hypot(sp[0] - cur[0], sp[1] - cur[1]) > 1e-6:
                    sub.append(('L',) + sp)
                sub += self._arc_cmds(x, y, rad, c[4], c[5], rady)
                e = math.radians(c[5])
                cur = (x + rad * math.cos(e), y + rady * math.sin(e))
                cur16 = (c[1] + c[3] * math.cos(e), c[2] + c[3] * math.sin(e))
            elif c[0] == 'Z':
                sub.append(('Z',))
        if (close or fill) and sub[-1][0] != 'Z':
            sub.append(('Z',))
        self._emit(sub, fill)

    # ── composites ─────────────────────────────────────────────────────────────────────────────
    def chevron(self, x, y, d, arm=3.0):
        """Two equal 45° arms meeting at the tip (x, y); d = direction the tip points: r l u d."""
        if self.mx is not None:
            d = {'r': 'l', 'l': 'r'}.get(d, d)
        dx, dy = {'r': (1, 0), 'l': (-1, 0), 'u': (0, -1), 'd': (0, 1)}[d]
        with self.at(x, y):
            x = self._fx(x)
            tx, ty = self.X(x), self.Y(y)
            a = abs(self.X(x + arm) - tx)            # one arm length for both axes
        if dx:
            sub = [('M', tx - dx * a, ty - a), ('L', tx, ty), ('L', tx - dx * a, ty + a)]
        else:
            sub = [('M', tx - a, ty - dy * a), ('L', tx, ty), ('L', tx + a, ty - dy * a)]
        self._emit(sub, False)

    def corner(self, x, y, sx, sy, arm=3.0):
        """Corner arrowhead: two equal arms from (x, y), one along x (sx = ±1), one along y (sy = ±1)."""
        if self.mx is not None:
            sx = -sx
        with self.at(x, y):
            x = self._fx(x)
            tx, ty = self.X(x), self.Y(y)
            a = abs(self.X(x + arm) - tx)
        self._emit([('M', tx + sx * a, ty), ('L', tx, ty), ('L', tx, ty + sy * a)], False)

    def arrow(self, x0, y0, x1, y1, head=2.0):
        """Axis-aligned shaft from (x0, y0) to the tip (x1, y1) with an equal-armed head."""
        self.line(x0, y0, x1, y1)
        d = ('r' if x1 > x0 else 'l') if abs(y1 - y0) < 1e-9 else ('d' if y1 > y0 else 'u')
        self.chevron(x1, y1, d, head)

    def dashes(self, x0, y0, x1, y1, n, gap=1.0):
        """n equal dashes on an axis-aligned line, first and last touching the ends."""
        (ax, ay), (bx, by) = self.P(x0, y0), self.P(x1, y1)
        length = math.hypot(bx - ax, by - ay)
        g = gap * self.w + self.w                  # round caps eat one stroke width of the gap
        dash = (length - g * (n - 1)) / n
        ux, uy = (bx - ax) / length, (by - ay) / length
        for i in range(n):
            s = i * (dash + g)
            self._emit([('M', ax + ux * s, ay + uy * s), ('L', ax + ux * (s + dash), ay + uy * (s + dash))], False)

    def tri(self, x, y, d, half, depth):
        """Filled isosceles triangle: tip at (x, y) pointing d, base `half` each side, `depth` long."""
        dx, dy = {'r': (1, 0), 'l': (-1, 0), 'u': (0, -1), 'd': (0, 1)}[d]
        if dx:
            self.poly([(x, y), (x - dx * depth, y - half), (x - dx * depth, y + half)], fill=True)
        else:
            self.poly([(x, y), (x - half, y - dy * depth), (x + half, y - dy * depth)], fill=True)

    def plus(self, x, y, arm):
        with self.at(x, y):
            self.line(x - arm, y, x + arm, y)
            self.line(x, y - arm, x, y + arm)

    # ── output ─────────────────────────────────────────────────────────────────────────────────
    def svg(self):
        s = self.s
        out = ['<svg width="%d" height="%d" viewBox="0 0 %d %d" fill="none" xmlns="http://www.w3.org/2000/svg">' % (s, s, s, s)]
        if self.strokes:
            out.append('<path d="%s" stroke-width="%s" stroke-linecap="round" stroke-linejoin="round" stroke="%s"/>'
                       % (' '.join(self.strokes), fmt(self.w), COLOR))
        if self.fills:
            out.append('<path d="%s" fill="%s"/>' % (' '.join(self.fills), COLOR))
        out += self.extra
        out.append('</svg>')
        return '\n'.join(out) + '\n'


FREE = 1e-4      # add to a coordinate to keep it off the grid (free-form anchors)

REGISTRY = {}


def icon(name, sizes=(12, 16, 20)):
    def deco(fn):
        REGISTRY[name] = (fn, sizes)
        return fn
    return deco


def render(name, size):
    fn, _ = REGISTRY[name]
    g = G(size)
    fn(g)
    return g.svg()
