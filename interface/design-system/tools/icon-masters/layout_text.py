"""Alignment, distribution, stroke caps and joins, paragraph and text."""
from kit import icon


# ── align: a reference line and two boxes ────────────────────────────────────────────────────────
def _align_edge(g, vertical_line, flip=False, mirror_y=False):
    """Line on the left with two bars growing from it; other edges are transposed / mirrored."""
    def T(x, y):                        # transpose for top / bottom
        return (x, y) if vertical_line else (y, x)

    def M(p):                           # mirror for right / bottom
        x, y = p
        if vertical_line:
            return ((16 - x) if flip else x, y)
        return (x, (16 - y) if mirror_y else y)

    def rect(l, t, r, b):
        (x0, y0), (x1, y1) = M(T(l, t)), M(T(r, b))
        g.rect(min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1), 1)

    a, b = M(T(2.5, 2.5)), M(T(2.5, 13.5))
    g.line(a[0], a[1], b[0], b[1])
    rect(4.5, 4.5, 12.5, 7.5)
    rect(4.5, 9.5, 9.5, 12.5)


@icon('align-left')
def _(g): _align_edge(g, True)


@icon('align-right')
def _(g): _align_edge(g, True, flip=True)


@icon('align-top')
def _(g): _align_edge(g, False)


@icon('align-bottom')
def _(g): _align_edge(g, False, mirror_y=True)


@icon('align-hcenter')
def _(g):
    g.axis(8.5, 8.5)
    for y0, y1 in ((2.5, 4.5), (7.5, 9.5), (12.5, 14.5)):     # the axis shows only outside the boxes
        g.line(8.5, y0, 8.5, y1)
    g.rect(3.5, 4.5, 13.5, 7.5, 1)
    g.rect(5.5, 9.5, 11.5, 12.5, 1)


@icon('align-vmiddle')
def _(g):
    g.axis(8.5, 8.5)
    for x0, x1 in ((2.5, 4.5), (7.5, 9.5), (12.5, 14.5)):
        g.line(x0, 8.5, x1, 8.5)
    g.rect(4.5, 3.5, 7.5, 13.5, 1)
    g.rect(9.5, 5.5, 12.5, 11.5, 1)


# ── distribute: three equal steps ────────────────────────────────────────────────────────────────
def _dist(g, horizontal, side):
    """Three lines 4 px apart with a block on each. side: -1 before the line, 0 centred, 1 after."""
    if g.s == 12:
        # 12 px has no room for 4 px steps: edges step by 3 with thin blocks, centres step by 4.
        with g.native():
            first, step = {-1: 3.5, 0: 1.5, 1: 2.5}[side], (4 if side == 0 else 3)
            for i in range(3):
                c = first + step * i
                lo, hi = {-1: (c - 1.5, c), 0: (c - 1.5, c + 1.5), 1: (c, c + 1.5)}[side]
                if horizontal:
                    g.line(c, 1.5, c, 10.5)
                    g.rect(lo, 4, hi, 8, fill=True)
                else:
                    g.line(1.5, c, 10.5, c)
                    g.rect(4, lo, 8, hi, fill=True)
        return
    g.axis(8.5, 8) if horizontal else g.axis(8, 8.5)
    first = {-1: 5.5, 0: 4.5, 1: 3.5}[side]
    for i in range(3):
        c = first + 4 * i
        lo, hi = {-1: (c - 2.5, c), 0: (c - 1.5, c + 1.5), 1: (c, c + 2.5)}[side]
        if horizontal:
            g.line(c, 2.5, c, 13.5)
            g.rect(lo, 5, hi, 11, fill=True)
        else:
            g.line(2.5, c, 13.5, c)
            g.rect(5, lo, 11, hi, fill=True)


@icon('dist-left')
def _(g): _dist(g, True, 1)


@icon('dist-hcenter')
def _(g): _dist(g, True, 0)


@icon('dist-right')
def _(g): _dist(g, True, -1)


@icon('dist-top')
def _(g): _dist(g, False, 1)


@icon('dist-vmiddle')
def _(g): _dist(g, False, 0)


@icon('dist-bottom')
def _(g): _dist(g, False, -1)


@icon('space-h')
def _(g):
    g.line(2.5, 2.5, 2.5, 13.5)
    g.line(13.5, 2.5, 13.5, 13.5)
    g.rect(5, 4, 7, 12, fill=True)
    g.rect(9, 4, 11, 12, fill=True)


@icon('space-v')
def _(g):
    g.line(2.5, 2.5, 13.5, 2.5)
    g.line(2.5, 13.5, 13.5, 13.5)
    g.rect(4, 5, 12, 7, fill=True)
    g.rect(4, 9, 12, 11, fill=True)


@icon('match-both')
def _(g):
    g.rect(2.5, 2.5, 9.5, 9.5, 1)
    g.rect(6.5, 6.5, 13.5, 13.5, 1)


@icon('match-h')
def _(g):
    g.rect(2.5, 2.5, 6.5, 13.5, 1)
    g.rect(9.5, 2.5, 13.5, 13.5, 1)


@icon('match-w')
def _(g):
    g.rect(2.5, 2.5, 13.5, 6.5, 1)
    g.rect(2.5, 9.5, 13.5, 13.5, 1)




# ── flip ─────────────────────────────────────────────────────────────────────────────────────────
@icon('flip-h')
def _(g):
    g.axis(8.5, 8.5)
    g.dashes(8.5, 2.5, 8.5, 14.5, 3 if g.s == 12 else 4)
    g.poly([(6.5, 4.5), (6.5, 12.5), (2.5, 12.5)], close=True)
    tri = [(10.5, 4.5), (10.5, 12.5), (14.5, 12.5)]
    g.poly(tri, close=True)
    g.poly(tri, fill=True)


@icon('flip-v')
def _(g):
    g.axis(8.5, 8.5)
    g.dashes(2.5, 8.5, 14.5, 8.5, 3 if g.s == 12 else 4)
    g.poly([(4.5, 6.5), (12.5, 6.5), (12.5, 2.5)], close=True)
    tri = [(4.5, 10.5), (12.5, 10.5), (12.5, 14.5)]
    g.poly(tri, close=True)
    g.poly(tri, fill=True)


# ── stroke caps and joins ────────────────────────────────────────────────────────────────────────
def _cap(g, kind):
    g.axis(8, 8.5)
    g.line(11.5, 2.5, 11.5, 4.5)                  # the end of the path, shown outside the stroke only
    g.line(11.5, 12.5, 11.5, 14.5)
    if kind == 'none':
        g.poly([(2, 6), (11.5, 6), (11.5, 11), (2, 11)], fill=True)
    elif kind == 'square':
        g.rect(2, 6, 14, 11, fill=True)
    else:
        g.path([('M', 2, 6), ('L', 11.5, 6), ('A', 11.5, 8.5, 2.5, -90, 90), ('L', 2, 11)], fill=True)


@icon('cap-none')
def _(g): _cap(g, 'none')


@icon('cap-round')
def _(g): _cap(g, 'round')


@icon('cap-square')
def _(g): _cap(g, 'square')


@icon('join-miter')
def _(g): g.poly([(2, 14), (2, 2), (14, 2), (14, 6), (6, 6), (6, 14)], fill=True)


@icon('join-bevel')
def _(g): g.poly([(2, 14), (2, 6), (6, 2), (14, 2), (14, 6), (6, 6), (6, 14)], fill=True)


@icon('join-round')
def _(g):
    g.path([('M', 2, 14), ('L', 2, 6), ('A', 6, 6, 4, 180, 270), ('L', 14, 2), ('L', 14, 6), ('L', 6, 6), ('L', 6, 14)], fill=True)


# ── paragraph ────────────────────────────────────────────────────────────────────────────────────
ROWS = (3.5, 6.5, 9.5, 12.5)


def _rows(g, spans):
    for y, (x0, x1) in zip(ROWS, spans):
        g.line(x0, y, x1, y)


@icon('talign-left')
def _(g): _rows(g, [(2.5, 13.5), (2.5, 9.5), (2.5, 13.5), (2.5, 7.5)])


@icon('talign-right')
def _(g):
    with g.flip(8):
        _rows(g, [(2.5, 13.5), (2.5, 9.5), (2.5, 13.5), (2.5, 7.5)])


@icon('talign-center')
def _(g): _rows(g, [(2.5, 13.5), (4.5, 11.5), (2.5, 13.5), (5.5, 10.5)])


@icon('talign-justify')
def _(g): _rows(g, [(2.5, 13.5), (2.5, 13.5), (2.5, 13.5), (2.5, 8.5)])


@icon('talign-justify-all')
def _(g): _rows(g, [(2.5, 13.5)] * 4)


@icon('autoformat')
def _(g): _rows(g, [(2.5, 13.5), (5.5, 13.5), (5.5, 11.5), (2.5, 9.5)])


def _margin(g):
    g.axis(8, 8.5)
    g.line(2.5, 3.5, 2.5, 13.5)
    g.line(5.5, 5.5, 13.5, 5.5)
    g.line(5.5, 8.5, 13.5, 8.5)
    g.line(5.5, 11.5, 10.5, 11.5)


@icon('margin-left')
def _(g): _margin(g)


@icon('margin-right')
def _(g):
    with g.flip(8):
        _margin(g)


@icon('indent')
def _(g):
    g.axis(8, 8.5)
    for y in (4.5, 8.5, 12.5):
        g.line(7.5, y, 13.5, y)
    g.chevron(5, 8.5, 'r', 2.5)


@icon('line-spacing')
def _(g):
    g.axis(8, 8.5)
    for y in (4.5, 8.5, 12.5):
        g.line(7.5, y, 13.5, y)
    g.line(3.5, 3.5, 3.5, 13.5)
    g.chevron(3.5, 3.5, 'u', 1.5)
    g.chevron(3.5, 13.5, 'd', 1.5)


@icon('letter-spacing')
def _(g):
    _A(g, 6, 3.5, 10.5, 2.5, 8.5, 1.5)
    g.line(12.5, 3.5, 12.5, 10.5)
    g.line(2.5, 13.5, 13.5, 13.5)
    g.chevron(2.5, 13.5, 'l', 1.5)
    g.chevron(13.5, 13.5, 'r', 1.5)


# ── text ─────────────────────────────────────────────────────────────────────────────────────────
def _A(g, cx, top, bottom, half, bar_y, bar_half):
    """Capital A: legs and crossbar are fitted symmetrically about its own axis."""
    with g.at(cx, None):
        g.poly([(cx - half, bottom), (cx, top), (cx + half, bottom)])
        g.line(cx - bar_half, bar_y, cx + bar_half, bar_y)


def _T(g, cx, top, bottom, half):
    g.line(cx - half, top, cx + half, top)
    g.line(cx, top, cx, bottom)


@icon('text')
def _(g):
    g.axis(8.5, 8.5)
    _T(g, 8.5, 3.5, 13.5, 5)
    g.line(6.5, 13.5, 10.5, 13.5)


@icon('font')
def _(g):
    _A(g, 5.5, 3.5, 13.5, 3.5, 10.5, 2)
    g.line(10.5, 5.5, 10.5, 13.5)
    g.circle(12.5, 11.5, 2)


@icon('selectable')
def _(g):
    g.axis(8, 8.5)
    _A(g, 5.5, 4.5, 12.5, 3, 9.5, 1.5)
    g.line(11.5, 3.5, 11.5, 13.5)
    g.line(10, 3.5, 13, 3.5)
    g.line(10, 13.5, 13, 13.5)


def _two(g, x, base):
    """Small digit 2 whose baseline is y = base and left edge x."""
    g.path([('A', x + 1.5, base - 3.5, 1.5, 180, 405), ('L', x, base), ('L', x + 3, base)])


@icon('subscript')
def _(g):
    g.line(2.5, 3.5, 8.5, 11.5)
    g.line(8.5, 3.5, 2.5, 11.5)
    _two(g, 10.5, 13.5)


@icon('superscript')
def _(g):
    g.line(2.5, 5.5, 8.5, 13.5)
    g.line(8.5, 5.5, 2.5, 13.5)
    _two(g, 10.5, 7.5)


@icon('text-underline')
def _(g):
    # U: two stems and a half circle, then the rule under it.
    g.path([('M', 4.5, 2.5), ('L', 4.5, 6.5), ('A', 8, 6.5, 3.5, 180, 0), ('L', 11.5, 2.5)])
    g.line(3.5, 13.5, 12.5, 13.5)


@icon('text-strikethrough')
def _(g):
    # S: two equal bowls that meet on the strike line, so the letter is point-symmetric about its centre.
    if g.s == 12:
        with g.native():
            g.arc(6, 4.5, 2.25, -38, -270, ry=2)
            g.arc(6, 8.5, 2.25, -90, 142, ry=2)
            g.line(1.5, 6.5, 10.5, 6.5)
        return
    g.axis(8, 8.5)
    g.arc(8, 5.75, 3, -38, -270, ry=2.75)
    g.arc(8, 11.25, 3, -90, 142, ry=2.75)
    g.line(2.5, 8.5, 13.5, 8.5)


@icon('text-orientation-horizontal')
def _(g):
    g.axis(8.5, 8.5)
    _T(g, 6.5, 2.5, 8.5, 3)
    g.arrow(3.5, 12.5, 13.5, 12.5, 2)


def _vertical(g, up, mirror=False):
    """T with the reading arrow beside it (on the right; mirrored: on the left)."""
    if g.s == 12:
        with g.native():                          # 12 px: narrower T and a smaller head keep them apart
            m = (lambda x: 12 - x) if mirror else (lambda x: x)
            g.line(m(1.5), 2.5, m(5.5), 2.5)
            g.line(m(3.5), 2.5, m(3.5), 9.5)
            if up:
                g.arrow(m(9.5), 10.5, m(9.5), 1.5, 1.5)
            else:
                g.arrow(m(9.5), 1.5, m(9.5), 10.5, 1.5)
        return
    g.axis(8.5, 8.5)
    with g.flip(8.5, mirror):
        _T(g, 5.5, 3.5, 12.5, 3)                  # one clear pixel between the bar and the arrowhead
        if up:
            g.arrow(12.5, 13.5, 12.5, 2.5, 2)
        else:
            g.arrow(12.5, 2.5, 12.5, 13.5, 2)


@icon('text-orientation-vertical-ltr')
def _(g): _vertical(g, True)


@icon('text-orientation-vertical-rtl')
def _(g): _vertical(g, False, mirror=True)


@icon('text-orientation')
def _(g): _vertical(g, False)


def _direction(g):
    g.line(2.5, 3.5, 10.5, 3.5)
    g.line(2.5, 6.5, 8.5, 6.5)
    g.arrow(2.5, 11.5, 13.5, 11.5, 2)


@icon('text-direction-ltr')
def _(g): _direction(g)


@icon('text-direction-rtl')
def _(g):
    with g.flip(8):
        _direction(g)


@icon('text-bullets')
def _(g):
    g.axis(8, 8.5)
    for y in (3.5, 8.5, 13.5):
        g.dot(3.5, y, 1)
        g.line(6.5, y, 13.5, y)


def _valign(g, bars, rows):
    """A long bar is the edge the text is held against; short rows are the text."""
    for y in bars:
        g.line(2.5, y, 13.5, y)
    for y in rows:
        g.line(4.5, y, 11.5, y)


@icon('text-valign-top')
def _(g): _valign(g, (2.5,), (5.5, 8.5))


@icon('text-valign-bottom')
def _(g): _valign(g, (13.5,), (7.5, 10.5))


@icon('text-valign-center')
def _(g):
    g.axis(8, 8.5)
    _valign(g, (8.5,), (5.5, 11.5))


@icon('text-valign-justify')
def _(g): _valign(g, (2.5, 13.5), (6.5, 9.5))


@icon('text-columns')
def _(g):
    g.rect(2.5, 2.5, 6.5, 13.5)
    g.rect(9.5, 2.5, 13.5, 13.5)


@icon('text-gutter')
def _(g):
    # Two columns and the distance between them.
    if g.s == 12:
        with g.native():
            g.line(1.5, 2.5, 1.5, 9.5)
            g.line(10.5, 2.5, 10.5, 9.5)
            g.line(3.5, 6.5, 8.5, 6.5)
            g.chevron(3.5, 6.5, 'l', 1.5)
            g.chevron(8.5, 6.5, 'r', 1.5)
        return
    g.axis(8, 8.5)
    g.rect(1.5, 3.5, 3.5, 13.5)
    g.rect(12.5, 3.5, 14.5, 13.5)
    g.line(5.5, 8.5, 10.5, 8.5)
    g.chevron(5.5, 8.5, 'l', 1.5)
    g.chevron(10.5, 8.5, 'r', 1.5)


@icon('text-border')
def _(g):
    g.rect(2.5, 3.5, 13.5, 12.5, 2)
    g.line(5.5, 6.5, 10.5, 6.5)
    g.line(5.5, 9.5, 8.5, 9.5)


@icon('text-background')
def _(g):
    g.rect(2.5, 2.5, 13.5, 13.5, 2)
    _A(g, 8, 5, 11, 2.5, 9.5, 1.5)
