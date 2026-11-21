"""Arrows, chevrons, playback, undo and rotation."""
from kit import icon


# ── chevrons and marks ───────────────────────────────────────────────────────────────────────────
@icon('chevdown')
def _(g): g.chevron(8, 10, 'd', 4)


@icon('chevup')
def _(g): g.chevron(8, 6, 'u', 4)


@icon('chevleft')
def _(g): g.chevron(6, 8, 'l', 4)


@icon('chevright')
def _(g): g.chevron(10, 8, 'r', 4)


@icon('submenu')
def _(g): g.chevron(10, 8, 'r', 4)


@icon('expand')
def _(g):
    g.chevron(8, 2.5, 'u', 3)
    g.chevron(8, 13.5, 'd', 3)


@icon('collapse')
def _(g):
    g.axis(8, 8.5)
    g.line(3.5, 8.5, 12.5, 8.5)
    g.chevron(8, 6, 'd', 3)
    g.chevron(8, 11, 'u', 3)


@icon('check')
def _(g): g.poly([(3.5, 8), (6.5, 11), (12.5, 5)])


@icon('x')
def _(g):
    g.line(4, 4, 12, 12)
    g.line(12, 4, 4, 12)


@icon('plus')
def _(g):
    g.axis(8.5, 8.5)
    g.plus(8.5, 8.5, 5)


@icon('minus')
def _(g):
    g.axis(8.5, 8.5)
    g.line(3.5, 8.5, 13.5, 8.5)


@icon('more')
def _(g):
    for x in (3, 8, 13):
        g.dot(x, 8, 1)


@icon('grip')
def _(g):
    for y in (3, 8, 13):
        g.dot(8, y, 1)


@icon('grip-h')
def _(g):
    for y in (6, 10):
        for x in (4, 8, 12):
            g.dot(x, y, 1)


# ── solid triangles and transport ────────────────────────────────────────────────────────────────
@icon('tridown')
def _(g): g.tri(8, 11, 'd', 4, 5)


@icon('triright')
def _(g): g.tri(11, 8, 'r', 4, 5)


@icon('play')
def _(g): g.tri(13, 8, 'r', 5, 8)


@icon('pause')
def _(g):
    g.rect(4, 3, 7, 13, fill=True)
    g.rect(9, 3, 12, 13, fill=True)


@icon('stop')
def _(g): g.rect(4, 4, 12, 12, 1, fill=True)


def _edge(g, flip):
    """Bar and triangle: `first`; mirrored about 8.5 it is `last`."""
    g.axis(8.5, 8)
    with g.flip(8.5, flip):
        g.line(4.5, 3.5, 4.5, 12.5)
        g.tri(7, 8, 'l', 5, 6)


@icon('first')
def _(g): _edge(g, False)


@icon('last')
def _(g): _edge(g, True)


@icon('stepback')
def _(g): g.tri(5, 8, 'l', 5, 6)


@icon('stepfwd')
def _(g): g.tri(11, 8, 'r', 5, 6)


@icon('debug-continue')
def _(g):
    g.axis(8.5, 8)
    g.line(4.5, 3.5, 4.5, 12.5)
    g.tri(13, 8, 'r', 5, 6)


@icon('exec-arrow')
def _(g):
    g.poly([(2, 6), (8, 6), (8, 3), (14, 8), (8, 13), (8, 10), (2, 10)], fill=True)


@icon('breakpoint')
def _(g): g.circle(8, 8, 4.5, fill=True)


# ── debugger steps ───────────────────────────────────────────────────────────────────────────────
@icon('step-into')
def _(g):
    g.axis(8.5, 8.5)
    g.arrow(8.5, 2.5, 8.5, 9.5, 3)
    g.dot(8.5, 13.5, 1.5)


@icon('step-out')
def _(g):
    g.axis(8.5, 8.5)
    g.arrow(8.5, 9.5, 8.5, 2.5, 3)
    g.dot(8.5, 13.5, 1.5)


@icon('step-over')
def _(g):
    g.axis(8.5, 8.5)
    g.arc(8.5, 9.5, 4, 180, 360)
    g.chevron(12.5, 9.5, 'd', 2)
    g.dot(8.5, 13.5, 1.5)


# ── undo, redo, rotation ─────────────────────────────────────────────────────────────────────────
def _undo(g, flip):
    g.axis(8.5, 8.5)
    with g.flip(8.5, flip):
        g.path([('M', 3.5, 6.5), ('L', 10.5, 6.5), ('A', 10.5, 9.5, 3, -90, 90), ('L', 6.5, 12.5)])
        g.chevron(3.5, 6.5, 'l', 2.5)


@icon('undo')
def _(g): _undo(g, False)


@icon('redo')
def _(g): _undo(g, True)


def _turn(g, cx, cy, r, flip, k=(4.5, 3.5), arm=2.5):
    """Open circle with a corner arrowhead: clockwise; mirrored it turns counter-clockwise.

    The corner of the head sits on the circle at (cx + k[0], cy - k[1]) and its arms run up and left.
    """
    import math
    with g.flip(cx, flip):
        kx, ky = cx + k[0], cy - k[1]
        tail = 360 - math.degrees(math.atan2(k[1], k[0]))
        g.path([('A', cx, cy, r, 0, tail), ('L', kx, ky)])
        g.corner(kx, ky, -1, -1, arm)


@icon('rotate-cw')
def _(g): _turn(g, 8, 8, 5.5, False)


@icon('rotate-ccw')
def _(g): _turn(g, 8, 8, 5.5, True)


@icon('refresh')
def _(g):
    # Run again: two half turns chasing each other (the lower one is the upper one turned 180°).
    g.path([('A', 8, 8, 5.5, 180, 322), ('L', 12.5, 4.5)])
    g.corner(12.5, 4.5, -1, -1, 2.5)
    g.path([('A', 8, 8, 5.5, 0, 142), ('L', 3.5, 11.5)])
    g.corner(3.5, 11.5, 1, 1, 2.5)


@icon('reset-transform')
def _(g):
    _turn(g, 8, 8, 5.5, True)
    g.rect(6.5, 6.5, 9.5, 9.5)


@icon('history')
def _(g):
    g.axis(8.5, 8.5)
    _turn(g, 8.5, 8.5, 5, True, k=(4, 3))
    g.poly([(8.5, 5.5), (8.5, 8.5), (10.5, 10)])


@icon('recent')
def _(g):
    g.axis(8.5, 8.5)
    g.circle(8.5, 8.5, 6)
    g.poly([(8.5, 5), (8.5, 8.5), (11, 10)])


def _bend(g, x0, y0, x1, y1, rad, head):
    """Half of a loop: up from (x0, y0), round the corner, along y1 to the arrow tip (x1, y1)."""
    g.path([('M', x0, y0), ('L', x0, y1 + rad), ('A', x0 + rad, y1 + rad, rad, 180, 270), ('L', x1, y1)])
    g.chevron(x1, y1, 'r', head)


@icon('loop')
def _(g):
    g.axis(8.5, 8.5)
    _bend(g, 3.5, 7.5, 12.5, 4.5, 2, 2)
    # the lower half is the upper one turned 180° about (8.5, 8.5)
    g.path([('M', 13.5, 9.5), ('L', 13.5, 10.5), ('A', 11.5, 10.5, 2, 0, 90), ('L', 4.5, 12.5)])
    g.chevron(4.5, 12.5, 'l', 2)


@icon('color-swap')
def _(g):
    g.axis(8.5, 8)
    g.path([('M', 4.5, 6.5), ('L', 4.5, 5), ('A', 6, 5, 1.5, 180, 270), ('L', 11.5, 3.5)])
    g.chevron(11.5, 3.5, 'r', 1.5)
    g.path([('M', 12.5, 9.5), ('L', 12.5, 11), ('A', 11, 11, 1.5, 0, 90), ('L', 5.5, 12.5)])
    g.chevron(5.5, 12.5, 'l', 1.5)


# ── transfer ─────────────────────────────────────────────────────────────────────────────────────
@icon('upload')
def _(g):
    g.axis(8.5, 8.5)
    g.arrow(8.5, 10.5, 8.5, 2.5, 3)
    g.line(3.5, 13.5, 13.5, 13.5)


@icon('embed')
def _(g):
    g.axis(8.5, 8.5)
    g.arrow(8.5, 2.5, 8.5, 10.5, 3)
    g.line(3.5, 13.5, 13.5, 13.5)


@icon('publish')
def _(g):
    g.axis(8.5, 8.5)
    g.arrow(8.5, 10.5, 8.5, 2.5, 3)
    g.poly([(3.5, 9.5), (3.5, 13.5), (13.5, 13.5), (13.5, 9.5)])


@icon('external')
def _(g):
    g.corner(13.5, 2.5, -1, 1, 4)
    g.line(13.5, 2.5, 7.5, 8.5)
    g.poly([(11.5, 9.5), (11.5, 13.5), (2.5, 13.5), (2.5, 4.5), (6.5, 4.5)])


@icon('detach')
def _(g):
    g.rect(2.5, 5.5, 10.5, 13.5, 1)
    g.poly([(5.5, 2.5), (13.5, 2.5), (13.5, 10.5)])
