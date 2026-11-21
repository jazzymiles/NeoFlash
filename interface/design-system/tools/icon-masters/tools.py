"""Tools and tool modifiers."""
import math
from kit import icon, fmt, COLOR, FREE


def _both(name, tool=None):
    """Register one drawing as the general icon and as the 20 px Tools palette glyph."""
    def deco(fn):
        icon(name)(fn)
        if tool:
            icon(tool, sizes=(20,))(fn)
        return fn
    return deco


# ── pointers ─────────────────────────────────────────────────────────────────────────────────────
# Classic arrow: the left edge is vertical, the right edge runs at 45°.
ARROW = [(4.5, 2.5), (4.5, 12.5), (7, 10.25), (8.75, 14), (10.25, 13.25), (8.5, 9.6), (11.5, 9.5)]


@_both('select', 'tool-select')
def _(g):
    g.axis(8.5, 8.5)
    g.poly(ARROW, close=True)
    g.poly(ARROW, fill=True)


@_both('subselect', 'tool-subselect')
def _(g):
    g.axis(8.5, 8.5)
    g.poly(ARROW, close=True)


@_both('hand', 'tool-hand')
def _(g):
    if g.s == 12:
        with g.native():                          # same hand, fingers two pixels wide
            g.path([('M', 3.5, 6.5), ('L', 3.5, 3.5), ('A', 4.5, 3.5, 1, 180, 360),
                    ('L', 5.5, 2.5), ('A', 6.5, 2.5, 1, 180, 360),
                    ('L', 7.5, 3.5), ('A', 8.5, 3.5, 1, 180, 360),
                    ('L', 9.5, 7.5), ('C', 9.5, 9.6, 8.4, 10.5, 6.6, 10.5),
                    ('C', 5.1, 10.5, 4.1, 9.8, 3.3, 8.4), ('L', 2.3, 6.8),
                    ('C', 1.95, 6.2, 2.2, 5.7, 2.75, 5.6), ('C', 3.1, 5.5, 3.4, 5.8, 3.5, 6.2)])
            g.line(5.5, 3.5, 5.5, 6)
            g.line(7.5, 3.5, 7.5, 6)
        return
    g.axis(8.5, 8.5)
    g.path([('M', 5.5, 8.5), ('L', 5.5, 4.5), ('A', 6.5, 4.5, 1, 180, 360),          # index
            ('L', 7.5, 3.5), ('A', 8.5, 3.5, 1, 180, 360),                            # middle
            ('L', 9.5, 4.5), ('A', 10.5, 4.5, 1, 180, 360),                           # ring
            ('L', 11.5, 10), ('C', 11.5, 12.75, 10.25, 14.5, 8.25, 14.5),             # palm
            ('C', 6.25, 14.5, 5, 13.4, 4 + FREE, 11.25), ('L', 3 + FREE, 9.25),
            ('C', 2.55, 8.45, 2.9, 7.75, 3.6, 7.6), ('C', 4.1, 7.5, 4.5, 7.8, 4.75, 8.25),
            ('L', 5.5, 9.5)])
    g.line(7.5, 4.5, 7.5, 8)
    g.line(9.5, 4.5, 9.5, 8)


def _lasso(g):
    with g.rotate(-14, 8.5, 7):
        g.ellipse(8.5, 7, 5.25, 3.9)
    g.path([('M', 5.1, 10.1), ('C', 4.8, 12.2, 5.8, 13.9, 7.9, 14.3)])


@icon('lasso')
def _(g): _lasso(g)


@icon('tool-lasso', sizes=(20,))
def _(g): _lasso(g)


# ── drawing tools (drawn upright, shown turned 45°) ──────────────────────────────────────────────
def _pencil(g):
    with g.rotate(45):
        g.poly([(6.5, 1.25), (9.5, 1.25), (9.5, 10.75), (8, 14.5), (6.5, 10.75)], close=True)
        g.line(6.5, 4.25, 9.5, 4.25)


@_both('pencil', 'tool-pencil')
def _(g): _pencil(g)


# Bristles: round shoulders, pointed tip (upright, tip down).
DROP = [('M', 8, 8.5), ('C', 9.7, 8.5, 10.1, 10, 9.7, 11.5), ('C', 9.35, 12.9, 8.7, 14, 8, 15),
        ('C', 7.3, 14, 6.65, 12.9, 6.3, 11.5), ('C', 5.9, 10, 6.3, 8.5, 8, 8.5)]


def _brush(g):
    with g.rotate(45):
        g.line(8, 0.5, 8, 8.5)
        g.path(DROP, close=True)


@_both('brush', 'tool-brush')
def _(g): _brush(g)


@icon('brushes')
def _(g):
    with g.rotate(45):
        g.rect(6.5, 0.5, 9.5, 7)
        g.path([('M', 6.5, 7), ('L', 9.5, 7), ('C', 10.6, 8.6, 10.6, 10.6, 10, 12), ('C', 9.5, 13.3, 8.8, 14.5, 8, 15.5),
                ('C', 7.2, 14.5, 6.5, 13.3, 6, 12), ('C', 5.4, 10.6, 5.4, 8.6, 6.5, 7)], close=True)


def _dropper(g):
    with g.rotate(45):
        g.rect(6.5, 0.5, 9.5, 4, 1.25)
        g.line(5.25, 4.25, 10.75, 4.25)
        g.poly([(6.5, 4.25), (6.5, 11.5), (8, 14.75), (9.5, 11.5), (9.5, 4.25)])


@_both('eyedropper', 'tool-eyedropper')
def _(g): _dropper(g)


@icon('eyedropper16', sizes=(12, 16))
def _(g): _dropper(g)


def _eraser(g):
    g.line(7.5, 13.5, 13.5, 13.5)
    with g.rotate(45, 7.5, 7.5):
        g.rect(4.5, 2, 10.5, 13, 1)
        g.line(4.5, 8.5, 10.5, 8.5)


@_both('eraser', 'tool-eraser')
def _(g): _eraser(g)


@icon('mod-eraser-mode')
def _(g): _eraser(g)


def _nib(g, cx, top, w, h):
    """Pen nib pointing down: shoulders at 42 % of the height, a slit from the hole to the tip."""
    mid = top + h * 0.42
    g.poly([(cx, top), (cx + w, mid), (cx, top + h), (cx - w, mid)], close=True)
    g.circle(cx, mid, 1)
    g.line(cx, mid + 1, cx, top + h)


@icon('pen')
def _(g):
    g.axis(8.5, 8)
    _nib(g, 8.5, 2.5, 4, 11.5)


@icon('tool-pen', sizes=(20,))
def _(g):
    g.axis(8.5, 8)
    _nib(g, 8.5, 1.5, 4, 10)
    g.line(6, 14.5, 11, 14.5)


def _small_nib(g):
    with g.rotate(-135, 6.5, 9.5):
        g.poly([(6.5, 15), (9.25, 10.5), (6.5, 4), (3.75, 10.5)], close=True)
        g.line(6.5, 4, 6.5, 9.5)
        g.circle(6.5, 10.5, 0.9)


@icon('tool-pen-add')
def _(g):
    _small_nib(g)
    g.plus(12.5, 3.5, 2)


@icon('tool-pen-remove')
def _(g):
    _small_nib(g)
    g.line(10.5, 3.5, 14.5, 3.5)


@icon('tool-pen-convert')
def _(g):
    # Convert anchor point: a point with its two handles on top of a corner.
    g.axis(8.5, 8)
    g.rect(7.5, 2.5, 9.5, 4.5)
    g.line(4.5, 3.5, 7.5, 3.5)
    g.line(9.5, 3.5, 12.5, 3.5)
    g.dot(3.5, 3.5, 1)
    g.dot(13.5, 3.5, 1)
    g.line(7.75, 4.5, 4.5, 13.5)
    g.line(9.25, 4.5, 12.5, 13.5)


def _bucket(g):
    with g.at(7, 8.5):
        g.poly([(2.5, 8.5), (7, 4), (11.5, 8.5), (7, 13)], close=True)
        g.line(2.5, 8.5, 11.5, 8.5)               # the paint level
    g.path([('M', 13.5, 9.5), ('C', 12.2, 11.4, 12.1, 13.5, 13.5, 13.5), ('C', 14.9, 13.5, 14.8, 11.4, 13.5, 9.5)], close=True)


@_both('bucket', 'tool-bucket')
def _(g): _bucket(g)


def _wand(g):
    with g.rotate(45, 6.5, 9.5):
        g.rect(5.5, 4.5, 7.5, 14.5)
    g.plus(12.5, 3.5, 2)


@icon('wand')
def _(g): _wand(g)


@icon('mod-magic-wand')
def _(g):
    # Classic magic wand: the wand is one ray of the sparkle at its tip.
    g.line(2.5, 13.5, 10.4, 5.6)
    g.line(11.5, 1.5, 11.5, 3)
    g.line(11.5, 6, 11.5, 7.5)
    g.line(8.5, 4.5, 10, 4.5)
    g.line(13, 4.5, 14.5, 4.5)
    g.line(12.6, 3.4, 13.6, 2.4)
    g.line(10.4, 3.4, 9.4, 2.4)
    g.line(12.6, 5.6, 13.6, 6.6)


# ── pencil modes ─────────────────────────────────────────────────────────────────────────────────
def _straighten(g): g.poly([(2.5, 12.5), (6.5, 4.5), (9.5, 10.5), (13.5, 3.5)])


def _smooth(g):
    g.path([('M', 2.5, 10.5), ('C', 5, 2.5, 6.5, 2.5, 8, 8), ('C', 9.5, 13.5, 11, 13.5, 13.5, 5.5)])


@icon('pencil-straighten')
def _(g): _straighten(g)


@icon('tool-straighten')
def _(g): _straighten(g)


@icon('pencil-smooth')
def _(g): _smooth(g)


@icon('tool-smooth')
def _(g): _smooth(g)


@icon('pencil-ink')
def _(g):
    g.poly([(2.5, 10), (4, 7), (5.5, 10), (7, 6.5), (8.5, 10), (10, 6.5), (11.5, 10), (13.5, 7)])


@icon('tool-ink')
def _(g):
    g.axis(8.5, 8.5)
    g.rect(5.5, 7.5, 11.5, 13.5)
    g.poly([(7.5, 7.5), (7.5, 5.5), (9.5, 5.5), (9.5, 7.5)])
    g.path([('M', 8.5, 5.5), ('C', 8.5, 4.2, 9.4, 3.5, 11, 3.5)])


@icon('tool-spray')
def _(g):
    g.rect(3.5, 5.5, 9.5, 13.5, 1.25)
    g.poly([(5.5, 5.5), (5.5, 3.5), (7.5, 3.5), (7.5, 5.5)])
    for x, y in ((11, 3), (13, 5), (11.5, 7), (13.5, 9)):
        g.dot(x, y, 1)


# ── skeleton, 3D, decorative ─────────────────────────────────────────────────────────────────────
def _edge(g, a, ra, b, rb):
    """Line between two circles, from rim to rim."""
    d = math.hypot(b[0] - a[0], b[1] - a[1])
    ux, uy = (b[0] - a[0]) / d, (b[1] - a[1]) / d
    g.line(a[0] + ux * ra, a[1] + uy * ra, b[0] - ux * rb, b[1] - uy * rb)


@icon('armature')
def _(g):
    a, b, c = (4, 12), (8.5, 4.5), (13, 11)
    for p in (a, b, c):
        g.circle(p[0], p[1], 1.5)
    _edge(g, a, 1.5, b, 1.5)
    _edge(g, b, 1.5, c, 1.5)


@icon('tool-bone')
def _(g):
    a, b = (4.5, 11.5), (12, 4)
    g.circle(a[0], a[1], 2)
    g.circle(b[0], b[1], 1.5)
    ax = math.atan2(b[1] - a[1], b[0] - a[0])
    tip = (b[0] - 1.5 * math.cos(ax), b[1] - 1.5 * math.sin(ax))
    for s in (1, -1):
        t = ax + s * math.radians(72)
        g.line(a[0] + 2 * math.cos(t), a[1] + 2 * math.sin(t), tip[0], tip[1])


@icon('tool-bind')
def _(g):
    g.circle(5, 11, 2)
    g.line(6.4, 9.6, 10, 6)
    g.rect(10, 2, 14, 6, fill=True)


@icon('tool-puppet')
def _(g):
    g.axis(8.5, 8.5)
    g.circle(8.5, 5, 2.5)
    g.dot(8.5, 5, 1)
    g.line(8.5, 7.5, 8.5, 11)
    g.poly([(3.5, 14), (8.5, 11), (13.5, 14)])


@icon('tool-deco')
def _(g):
    if g.s == 12:
        with g.native():
            g.dot(6, 6, 0.75)
            for x, y in ((6, 3), (9, 6), (6, 9), (3, 6)):
                g.circle(x, y, 1.5)
        return
    g.circle(8, 8, 1.5)
    for x, y in ((8, 4), (12, 8), (8, 12), (4, 8)):
        g.circle(x, y, 1.5)


@icon('tool-polystar')
def _(g):
    pts = []
    for i in range(10):
        r = 5.9 if i % 2 == 0 else 2.5
        a = math.radians(-90 + 36 * i)
        pts.append((8 + r * math.cos(a), 8.4 + r * math.sin(a)))
    g.poly(pts, close=True)


@icon('tool-3d-rotate')
def _(g):
    g.circle(8, 8, 5.5)                           # a ball, its equator and the direction it turns
    g.ellipse(8, 8, 5.5, 2)
    g.chevron(9.5, 10, 'r', 1.5)


@icon('tool-3d-translate')
def _(g):
    g.axis(8.5, 8.5)
    g.arrow(8.5, 9, 8.5, 3, 2)                    # up: on the grid
    for deg in (120, 240):                        # the other two axes are the same arrow, turned
        with g.rotate(deg, 8.5, 9):
            g.line(8.5, 9, 8.5, 3)
            g.poly([(6.5, 5), (8.5, 3), (10.5, 5)])


@icon('tool-razor')
def _(g):
    g.axis(8.5, 8)
    g.poly([(2.5, 4.5), (13.5, 4.5), (11.5, 11.5), (2.5, 11.5)], close=True)
    g.line(2.5, 6.5, 12.9, 6.5)
    g.dot(6, 9, 1)
    g.line(8.5, 1.5, 8.5, 2.5)
    g.line(8.5, 13.5, 8.5, 14.5)


@icon('tool-oval-primitive')
def _(g):
    g.ellipse(8, 8, 5.5, 4.5)
    g.dot(8, 8, 1)
    g.dot(13.5, 8, 1.25)


@icon('tool-rect-primitive')
def _(g):
    g.rect(2.5, 4.5, 13.5, 11.5, 3)
    g.dot(5.5, 7, 1)


@icon('tool-snap')
def _(g):
    g.path([('M', 3.5, 2.5), ('L', 3.5, 8), ('A', 8, 8, 4.5, 180, 0), ('L', 12.5, 2.5), ('L', 9.5, 2.5),
            ('L', 9.5, 8), ('A', 8, 8, 1.5, 0, 180), ('L', 6.5, 2.5)], close=True)
    g.line(3.5, 5.5, 6.5, 5.5)
    g.line(9.5, 5.5, 12.5, 5.5)


@icon('trace-bitmap')
def _(g):
    g.rect(2.5, 2.5, 8.5, 8.5)
    g.line(5.5, 2.5, 5.5, 8.5)
    g.line(2.5, 5.5, 8.5, 5.5)
    g.path([('A', 13.5, 13.5, 6, 180, 270)])
    g.dot(7.5, 13.5, 1.25)
    g.dot(13.5, 7.5, 1.25)


@icon('debug')
def _(g):
    g.axis(8.5, 8.5)
    g.rect(5.5, 5.5, 11.5, 13.5, 3)
    g.line(8.5, 7.5, 8.5, 13.5)
    for y, dx in ((8.5, 2.5), (11.5, 2)):
        g.line(5.5, y, 5.5 - dx, y)
        g.line(11.5, y, 11.5 + dx, y)
    g.line(6.5, 5.5, 5.5, 3.5)
    g.line(10.5, 5.5, 11.5, 3.5)


@icon('filter')
def _(g): g.poly([(2.5, 3.5), (13.5, 3.5), (9.5, 8.5), (9.5, 13), (6.5, 11.5), (6.5, 8.5)], close=True)


# ── tool modifiers ───────────────────────────────────────────────────────────────────────────────
# These follow the glyphs the editor paints for the same options (ui_icons.rs, ToolModifierGlyph),
# which are the classic Flash ones. The editor adds the menu corner itself.
@icon('mod-object-drawing')
def _(g):
    # An object in its bounding box.
    g.rect(2.5, 2.5, 13.5, 13.5)
    for x in (1, 12):
        for y in (1, 12):
            g.rect(x, y, x + 3, y + 3, fill=True)
    g.circle(8, 8, 3.5)


@icon('mod-gap-size')
def _(g):
    # A contour that is not closed: the gap the bucket may bridge.
    g.arc(8, 8, 5, -10, 280)


# Brush modes: a square object and a round blob of paint.
SQ = (2.5, 2.5, 10.5, 10.5)
BLOB = (10, 10, 3.5)


def _blob_outside(g, edge):
    """The part of the blob outside the square, cut along x = y = edge.

    10.5 is the outline's own axis (the blob slides under the line), 11 leaves the line as a clear channel.
    """
    d = edge - BLOB[0]
    a = math.degrees(math.atan2(math.sqrt(BLOB[2] ** 2 - d * d), d))
    with g.at(BLOB[0], BLOB[1]):
        g.path([('A', BLOB[0], BLOB[1], BLOB[2], -a, 90 + a), ('L', edge, edge)], fill=True)


def _blob_inside(g):
    """The quarter of the blob that lies on the square."""
    with g.at(BLOB[0], BLOB[1]):
        g.path([('M', 10, 10), ('L', 6.5, 10), ('A', BLOB[0], BLOB[1], BLOB[2], 180, 270)], fill=True)


@icon('mod-brush-normal')
def _(g):
    # Paint covers lines and fills: the blob lies on top.
    g.rect(*SQ)
    with g.at(BLOB[0], BLOB[1]):
        g.circle(*BLOB, fill=True)


@icon('mod-brush-paint-fills')
def _(g):
    # Paint covers fills, lines stay: the outline shows through the blob.
    g.poly([(10.5, 6), (10.5, 2.5), (2.5, 2.5), (2.5, 10.5), (6, 10.5)])
    _blob_inside(g)
    _blob_outside(g, 11)


@icon('mod-brush-paint-behind')
def _(g):
    # Paint goes behind the object.
    g.rect(*SQ)
    _blob_outside(g, 10.5)


@icon('mod-brush-paint-selection')
def _(g):
    # Paint lands only in the selection.
    g.dash_rect(SQ[0], SQ[1], SQ[2], SQ[3], 3)
    _blob_inside(g)


@icon('mod-brush-paint-inside')
def _(g):
    # Paint stays inside the shape where the stroke began.
    g.rect(2.5, 2.5, 12.5, 12.5)
    g.circle(7.5, 7.5, 2.5, fill=True)


@icon('mod-brush-size')
def _(g):
    g.dot(3, 8, 1)
    g.circle(6.5, 8, 1.5, fill=True)
    g.circle(11.5, 8, 2.5, fill=True)


@icon('mod-brush-shape')
def _(g):
    g.circle(5, 8, 3, fill=True)
    g.rect(9, 5, 14, 11, fill=True)


@icon('mod-eraser-mode')
def _(g):
    # What the eraser removes: the outline and the fill of an object.
    g.rect(2.5, 2.5, 12.5, 12.5)
    g.rect(5, 5, 10, 10, fill=True)


@icon('mod-eraser-shape')
def _(g):
    g.circle(5, 8, 2.5)
    g.rect(9.5, 5.5, 13.5, 10.5)


@icon('mod-eraser-size')
def _(g):
    g.circle(4, 8, 1.5)
    g.circle(10.5, 8, 3)


@icon('mod-faucet')
def _(g):
    g.path([('M', 2.5, 4.5), ('L', 8.5, 4.5), ('A', 8.5, 7.5, 3, 270, 360), ('L', 11.5, 9.5), ('L', 8.5, 9.5),
            ('L', 8.5, 7.5), ('L', 2.5, 7.5)], close=True)
    g.line(5.5, 4.5, 5.5, 2.5)
    g.line(3.5, 2.5, 7.5, 2.5)
    g.dot(10, 12.5, 1)


@icon('mod-lasso-polygon')
def _(g):
    # Lasso drawn with straight segments.
    g.poly([(2.5, 11.5), (4.5, 3), (13, 4), (13.5, 10.5), (8, 13.5)], close=True)


@icon('mod-slice-merge-fills')
def _(g):
    # A cutting contour with its corner points.
    pts = [(2.5, 11.5), (6, 3.5), (13.5, 5.5), (10.5, 13)]
    g.poly(pts, close=True)
    for x, y in pts:
        g.dot(x, y, 1.25)


@icon('mod-touch-selects')
def _(g):
    # A marquee that only touches a shape still selects it.
    g.rect(7.5, 2.5, 13.5, 8.5)
    g.dash_rect(2.5, 6.5, 9.5, 13.5, 3)


@icon('mod-settings')
def _(g):
    # Magic wand settings: the wand and two sliders.
    g.line(2.5, 12.5, 8.4, 6.6)
    g.line(9.5, 2.5, 9.5, 4)
    g.line(11, 5.5, 12.5, 5.5)
    g.line(6.5, 5.5, 8, 5.5)
    g.line(9.5, 7, 9.5, 8.5)
    g.line(8.5, 11.5, 14.5, 11.5)
    g.line(8.5, 13.5, 14.5, 13.5)
    g.dot(10.5, 11.5, 1)
    g.dot(12.5, 13.5, 1)


# ── gradient transform keeps its gradient fill ───────────────────────────────────────────────────
@icon('gradient-transform')
def _(g):
    l, t, r, b = g.X(2.5), g.Y(4.5), g.X(13.5), g.Y(11.5)
    x0, x1, y0, y1 = g.X(4.5), g.X(11.5), g.Y(2.5), g.Y(13.5)
    box = 'M %s %s L %s %s L %s %s L %s %s Z' % tuple(fmt(v) for v in (l, t, r, t, r, b, l, b))
    bars = 'M %s %s L %s %s M %s %s L %s %s' % tuple(fmt(v) for v in (x0, y0, x0, y1, x1, y0, x1, y1))
    gid = 'nf_gradient_transform_%d' % g.s
    g.raw('<path d="%s" fill="url(#%s)" stroke-width="%s" stroke-linecap="round" stroke-linejoin="round" stroke="%s"/>'
          % (box, gid, fmt(g.w), COLOR))
    g.raw('<path d="%s" stroke-width="%s" stroke-linecap="round" stroke-linejoin="round" stroke="%s"/>' % (bars, fmt(g.w), COLOR))
    g.raw('<defs>\n<linearGradient id="%s" x1="%s" y1="%s" x2="%s" y2="%s" gradientUnits="userSpaceOnUse">\n'
          '<stop stop-color="#5D677A"/>\n<stop offset="1" stop-color="#5D677A" stop-opacity="0.08"/>\n</linearGradient>\n</defs>'
          % (gid, fmt(l), fmt(t), fmt(r), fmt(t)))


# ── 20 px Tools palette glyphs that have no 16 px twin ───────────────────────────────────────────
@icon('tool-line', sizes=(20,))
def _(g):
    g.line(4.2, 11.8, 11.8, 4.2)
    g.dot(3.5, 12.5, 1.25)
    g.dot(12.5, 3.5, 1.25)


@icon('tool-oval', sizes=(20,))
def _(g): g.ellipse(8, 8, 5.5, 4.5)


@icon('tool-rect', sizes=(20,))
def _(g): g.rect(2.5, 3.5, 13.5, 12.5, 1)


@icon('tool-text', sizes=(20,))
def _(g):
    g.axis(8.5, 8.5)
    g.poly([(3.5, 4.5), (3.5, 3.5), (13.5, 3.5), (13.5, 4.5)])
    g.line(8.5, 3.5, 8.5, 13.5)
    g.line(6.5, 13.5, 10.5, 13.5)


@icon('tool-transform', sizes=(20,))
def _(g):
    g.rect(4.5, 4.5, 11.5, 11.5)
    for x in (3, 10):
        for y in (3, 10):
            g.rect(x, y, x + 3, y + 3, fill=True)
    g.circle(8, 8, 1)


@icon('tool-gradient', sizes=(20,))
def _(g):
    g.rect(2.5, 4.5, 10.5, 13.5, 1)
    g.line(4.5, 7, 4.5, 11)
    g.line(6.5, 9, 11.9, 3.6)
    g.circle(12.75, 2.75, 1.25, fill=True)
