"""Files, objects, panels, status, view, colour, timeline."""
import math
from kit import icon, FREE


# ── basic shapes ─────────────────────────────────────────────────────────────────────────────────
@icon('rect')
def _(g): g.rect(2.5, 3.5, 13.5, 12.5)


@icon('oval')
def _(g): g.ellipse(8, 8, 5.5, 4.5)


@icon('line')
def _(g): g.line(3, 13, 13, 3)


@icon('skew')
def _(g): g.poly([(5.5, 3.5), (13.5, 3.5), (10.5, 12.5), (2.5, 12.5)], close=True)


@icon('symbol')
def _(g):
    g.poly([(8, 2.5), (13.5, 8), (8, 13.5), (2.5, 8)], close=True)
    g.poly([(8, 5.5), (10.5, 8), (8, 10.5), (5.5, 8)], close=True)


@icon('graphic')
def _(g):
    g.poly([(2.5, 13.5), (7, 6.5), (11.5, 13.5)], close=True)
    g.circle(11.5, 4.5, 2)


@icon('button-symbol')
def _(g):
    g.axis(8, 8.5)
    g.rect(2.5, 5.5, 13.5, 11.5, 3)
    g.line(6.5, 8.5, 9.5, 8.5)


@icon('movieclip')
def _(g):
    g.rect(2.5, 3.5, 13.5, 12.5, 2)
    g.tri(11, 8, 'r', 2, 4)


# ── timeline frames ──────────────────────────────────────────────────────────────────────────────
@icon('frame')
def _(g):
    g.rect(4.5, 2.5, 11.5, 13.5, 1)
    g.dot(8, 8, 1.5)


@icon('frame-add')
def _(g):
    g.axis(8, 8.5)
    g.rect(1.5, 4.5, 6.5, 12.5, 1)
    g.plus(11.5, 8.5, 3)


@icon('keyframe-add')
def _(g):
    g.axis(8, 8.5)
    g.circle(4.5, 8.5, 3, fill=True)
    g.plus(11.5, 8.5, 3)


@icon('blankkey-add')
def _(g):
    g.axis(8, 8.5)
    g.circle(4.5, 8.5, 3)
    g.plus(11.5, 8.5, 3)


@icon('keyframe-remove')
def _(g):
    g.axis(8, 8.5)
    g.circle(4.5, 8.5, 3, fill=True)
    g.line(8.5, 8.5, 14.5, 8.5)


@icon('centerframe')
def _(g):
    g.axis(8, 8.5)
    g.rect(6.5, 3.5, 9.5, 13.5, 1)
    g.line(2.5, 8.5, 4.5, 8.5)
    g.line(11.5, 8.5, 13.5, 8.5)


@icon('markers')
def _(g):
    g.poly([(5.5, 2.5), (2.5, 2.5), (2.5, 13.5), (5.5, 13.5)])
    g.poly([(10.5, 2.5), (13.5, 2.5), (13.5, 13.5), (10.5, 13.5)])


@icon('onion')
def _(g):
    g.circle(5.5, 8, 3.5)
    g.circle(10.5, 8, 3.5)


@icon('onionoutline')
def _(g):
    g.dash_arc(5.5, 8, 3.5, 6, 0, 26)
    g.circle(10.5, 8, 3.5)


@icon('editmulti')
def _(g):
    g.rect(5.5, 2.5, 13.5, 10.5, 1)
    g.rect(2.5, 5.5, 10.5, 13.5, 1)


@icon('tween-classic')
def _(g):
    g.axis(8, 8.5)
    g.rect(2.5, 5.5, 13.5, 11.5, 2)
    g.arrow(5.5, 8.5, 10.5, 8.5, 1.5)


@icon('tween-motion')
def _(g):
    g.path([('M', 2.5, 12.5), ('C', 4.5, 4.5, 11.5, 4.5, 13.5, 12.5)])
    g.poly([(8, 4), (10.5, 6.5), (8, 9), (5.5, 6.5)], fill=True)


@icon('tween-shape')
def _(g):
    g.axis(8, 8.5)
    g.poly([(1.5, 11.5), (4, 5.5), (6.5, 11.5)], close=True)
    g.line(7.5, 8.5, 8.5, 8.5)
    g.circle(12, 8.5, 2.5)


@icon('ease')
def _(g):
    g.path([('M', 3, 12.5), ('C', 9, 12.5, 7, 3.5, 13, 3.5)])
    g.dot(3, 12.5, 1.25)
    g.dot(13, 3.5, 1.25)


@icon('motion-editor')
def _(g):
    g.axis(8.5, 8)
    g.poly([(2.5, 2.5), (2.5, 13.5), (14.5, 13.5)])
    g.path([('M', 4.5, 11.5), ('C', 7, 11.5, 6.5, 4.5, 8.5, 4.5), ('C', 10.5, 4.5, 10, 11.5, 12.5, 11.5)])


@icon('motion-path')
def _(g):
    g.path([('A', 13, 13, 10, 180, 270)])
    g.dot(3, 13, 1.5)
    g.dot(13, 3, 1.5)


def _guide_arc(g, cx, cy, r):
    for a0, a1 in ((196, 232), (252, 288), (308, 344)):
        g.arc(cx, cy, r, a0, a1)


@icon('motionguide')
def _(g):
    _guide_arc(g, 8, 11, 5)
    g.dot(3, 11, 1.5)
    g.dot(13, 11, 1.5)


@icon('guideplus')
def _(g):
    _guide_arc(g, 6.5, 9, 4)
    g.dot(2.5, 9, 1.25)
    g.plus(11.5, 11.5, 2.5)


@icon('scene')
def _(g):
    g.rect(2.5, 5.5, 13.5, 13.5, 1)
    g.line(2.5, 8.5, 13.5, 8.5)
    g.line(5, 8.5, 7, 5.5)
    g.line(9, 8.5, 11, 5.5)


# ── layers and files ─────────────────────────────────────────────────────────────────────────────
PAGE = [(4.5, 2.5), (9.5, 2.5), (12.5, 5.5), (12.5, 13.5), (4.5, 13.5)]


def _page(g):
    g.axis(8.5, 8)
    g.poly(PAGE, close=True)
    g.poly([(9.5, 2.5), (9.5, 5.5), (12.5, 5.5)])


@icon('layer')
def _(g): _page(g)


@icon('new-file')
def _(g):
    _page(g)
    g.plus(8.5, 9.5, 2)


@icon('file-code')
def _(g):
    _page(g)
    if g.s == 12:
        with g.native():
            g.chevron(4.75, 7.5, 'l', 1)
            g.chevron(8.25, 7.5, 'r', 1)
        return
    g.chevron(6.25, 10, 'l', 1.25)                # a clear pixel between the two brackets
    g.chevron(10.75, 10, 'r', 1.25)


@icon('sound-file')
def _(g):
    _page(g)
    g.dot(7.75, 11, 1.25)
    g.poly([(8.5, 11), (8.5, 7.5), (10.5, 8.5)])


@icon('layerplus')
def _(g):
    g.axis(8.5, 8.5)
    g.poly([(7.5, 13.5), (3.5, 13.5), (3.5, 2.5), (8.5, 2.5), (11.5, 5.5), (11.5, 7.5)])
    g.plus(11.5, 11.5, 2.5)


@icon('layers')
def _(g):
    with g.at(None, 5.5):
        g.poly([(8, 2.5), (13.5, 5.5), (8, 8.5), (2.5, 5.5)], close=True)
    g.poly([(2.5, 8.5), (8, 11.5), (13.5, 8.5)])
    g.poly([(2.5, 11), (8, 14), (13.5, 11)])


FOLDER = [(2.5, 3.5), (6.5, 3.5), (8, 5.5), (13.5, 5.5), (13.5, 12.5), (2.5, 12.5)]


@icon('folder')
def _(g): g.poly(FOLDER, close=True)


def _folder_open(g):
    g.poly([(2.5, 12.5), (2.5, 3.5), (6.5, 3.5), (8, 5.5), (12.5, 5.5), (12.5, 7.5)])
    g.poly([(2.5, 12.5), (4.5, 7.5), (14.5, 7.5), (12.5, 12.5)], close=True)


@icon('folderopen')
def _(g): _folder_open(g)


@icon('open')
def _(g):
    # Open / import a file: an arrow going into the folder.
    g.poly(FOLDER, close=True)
    g.arrow(8.5, 6.5, 8.5, 10.5, 1.5)


@icon('folderplus')
def _(g):
    g.poly([(7.5, 12.5), (2.5, 12.5), (2.5, 3.5), (6.5, 3.5), (8, 5.5), (13.5, 5.5), (13.5, 7.5)])
    g.plus(11.5, 11.5, 2.5)


@icon('save')
def _(g):
    g.poly([(2.5, 2.5), (10.5, 2.5), (13.5, 5.5), (13.5, 13.5), (2.5, 13.5)], close=True)
    g.poly([(5.5, 2.5), (5.5, 5.5), (9.5, 5.5), (9.5, 2.5)])
    g.poly([(4.5, 13.5), (4.5, 9.5), (11.5, 9.5), (11.5, 13.5)])


@icon('copy')
def _(g):
    g.rect(5.5, 5.5, 13.5, 13.5, 1)
    g.poly([(2.5, 10.5), (2.5, 2.5), (10.5, 2.5)])


@icon('paste')
def _(g):
    g.rect(3.5, 3.5, 12.5, 13.5, 1)
    g.rect(6.5, 2.5, 9.5, 4.5)


@icon('trash')
def _(g):
    g.line(2.5, 4.5, 13.5, 4.5)
    g.poly([(6.5, 4.5), (6.5, 2.5), (9.5, 2.5), (9.5, 4.5)])
    g.poly([(3.5, 4.5), (4.5, 13.5), (11.5, 13.5), (12.5, 4.5)])
    g.line(6.5, 7.5, 6.5, 10.5)
    g.line(9.5, 7.5, 9.5, 10.5)


@icon('bookmark')
def _(g): g.poly([(4.5, 2.5), (11.5, 2.5), (11.5, 13.5), (8, 10.5), (4.5, 13.5)], close=True)


@icon('flag')
def _(g):
    g.line(3.5, 2.5, 3.5, 13.5)
    g.poly([(4, 3), (13, 3), (11, 6), (13, 9), (4, 9)], fill=True)


@icon('film')
def _(g):
    g.rect(2.5, 2.5, 13.5, 13.5, 1)
    g.line(5.5, 2.5, 5.5, 13.5)
    g.line(10.5, 2.5, 10.5, 13.5)
    for y in (6.5, 9.5):
        g.line(2.5, y, 5.5, y)
        g.line(10.5, y, 13.5, y)


@icon('video')
def _(g):
    g.rect(1.5, 4.5, 10.5, 11.5, 1.5)
    g.poly([(10.5, 7), (14.5, 4.5), (14.5, 11.5), (10.5, 9)])


@icon('bitmap')
def _(g):
    g.rect(2.5, 3.5, 13.5, 12.5, 1)
    g.dot(5.5, 6.5, 1)
    g.poly([(4.5, 12.5), (9.5, 7.5), (13.5, 11.5)])


@icon('bitmap-fill')
def _(g):
    g.axis(8.5, 8.5)
    g.rect(3.5, 3.5, 13.5, 13.5, 1)
    g.line(8.5, 3.5, 8.5, 13.5)
    g.line(3.5, 8.5, 13.5, 8.5)
    g.dot(6, 6, 1)


@icon('grid')
def _(g):
    g.rect(2.5, 2.5, 13.5, 13.5, 1)
    for v in (6.5, 9.5):
        g.line(v, 2.5, v, 13.5)
        g.line(2.5, v, 13.5, v)


@icon('package')
def _(g):
    g.axis(8.5, 8)
    g.poly([(8.5, 2), (13.5, 5), (13.5, 11), (8.5, 14), (3.5, 11), (3.5, 5)], close=True)
    g.poly([(3.5, 5), (8.5, 8), (13.5, 5)])
    g.line(8.5, 8, 8.5, 14)


@icon('class')
def _(g):
    g.rect(2.5, 2.5, 13.5, 13.5, 2)
    g.arc(8, 8, 2.5, 50, 310)


@icon('callstack')
def _(g):
    for t in (2.5, 6.5, 10.5):                    # three stack frames
        g.rect(3.5, t, 12.5, t + 3, 1.5)


@icon('tree')
def _(g):
    # A parent node and two children hanging from its trunk.
    g.rect(2.5, 2.5, 7.5, 4.5)
    g.poly([(4.5, 4.5), (4.5, 12.5), (8.5, 12.5)])
    g.line(4.5, 8.5, 8.5, 8.5)
    g.rect(8.5, 7.5, 13.5, 9.5)
    g.rect(8.5, 11.5, 13.5, 13.5)


# ── panels and code ──────────────────────────────────────────────────────────────────────────────
@icon('sidebar')
def _(g):
    g.rect(2.5, 3.5, 13.5, 12.5, 1.5)
    g.line(6.5, 3.5, 6.5, 12.5)


@icon('desktop')
def _(g):
    g.axis(8.5, 8)
    g.rect(3.5, 2.5, 13.5, 10.5, 1)
    g.line(8.5, 10.5, 8.5, 13.5)
    g.line(5.5, 13.5, 11.5, 13.5)


def _prompt(g, rad):
    g.rect(2.5, 3.5, 13.5, 12.5, rad)
    g.chevron(6.5, 8, 'r', 1.5)
    g.line(8.5, 10.5, 10.5, 10.5)


@icon('terminal')
def _(g): _prompt(g, 0)


@icon('snippet')
def _(g):
    # A reusable piece of code: a pair of braces.
    for flip in (False, True):
        with g.flip(8, flip):
            g.path([('M', 6.5, 2.5), ('C', 5.1, 2.5, 4.5, 3.2, 4.5, 4.5), ('L', 4.5, 6.25),
                    ('C', 4.5, 7.4, 3.8, 8, 2.5 + FREE, 8 + FREE),
                    ('C', 3.8, 8, 4.5, 8.6, 4.5, 9.75), ('L', 4.5, 11.5),
                    ('C', 4.5, 12.8, 5.1, 13.5, 6.5, 13.5)])


@icon('code')
def _(g):
    g.chevron(2.5, 8, 'l', 3.5)
    g.chevron(13.5, 8, 'r', 3.5)


@icon('html')
def _(g):
    g.chevron(2, 8, 'l', 3)
    g.chevron(14, 8, 'r', 3)
    g.line(9.5, 3, 6.5, 13)


@icon('codehint')
def _(g):
    g.poly([(2.5, 2.5), (13.5, 2.5), (13.5, 10.5), (7.5, 10.5), (4.5, 13.5), (4.5, 10.5), (2.5, 10.5)], close=True)
    g.line(5.5, 6.5, 10.5, 6.5)


@icon('comment-line')
def _(g):
    g.line(7, 2.5, 4, 13.5)
    g.line(12, 2.5, 9, 13.5)


@icon('comment-block')
def _(g):
    g.line(6, 2.5, 3, 13.5)
    c, s = 2.5 * math.cos(math.radians(30)), 2.5 * math.sin(math.radians(30))
    g.line(11.5, 3.5, 11.5, 8.5)
    g.line(11.5 - c, 6 - s, 11.5 + c, 6 + s)
    g.line(11.5 - c, 6 + s, 11.5 + c, 6 - s)


@icon('uncomment')
def _(g):
    g.line(5.5, 2.5, 2.5, 13.5)
    g.line(9.5, 2.5, 6.5, 13.5)
    g.line(10.5, 9.5, 13.5, 12.5)
    g.line(13.5, 9.5, 10.5, 12.5)


@icon('variable')
def _(g):
    g.arc(9, 8, 6.5, 150, 210)
    g.arc(7, 8, 6.5, -30, 30)
    g.line(6.5, 6.5, 9.5, 9.5)
    g.line(9.5, 6.5, 6.5, 9.5)


@icon('keyboard')
def _(g):
    g.rect(1.5, 4.5, 14.5, 11.5, 1.5)
    if g.s == 12:
        with g.native():
            for x in (3.75, 6, 8.25):
                g.dot(x, 5.25, 0.6)
            g.line(4.5, 7.5, 7.5, 7.5)
        return
    for x in (4.25, 6.75, 9.25, 11.75):
        g.dot(x, 7, 0.6)
    g.line(5.5, 9.5, 10.5, 9.5)


@icon('info-panel')
def _(g):
    # The Info panel: "i" in a panel frame (the round badge is the message icon).
    g.axis(8.5, 8.5)
    g.rect(3.5, 3.5, 13.5, 13.5, 2)
    g.line(8.5, 8, 8.5, 11)
    g.dot(8.5, 5.75, 0.75)


# ── status badges (circle 13 px on the 8.5 axis) ─────────────────────────────────────────────────
def _badge(g):
    g.axis(8.5, 8.5)
    g.circle(8.5, 8.5, 6)


@icon('info')
def _(g):
    _badge(g)
    g.line(8.5, 7.5, 8.5, 11.5)
    g.dot(8.5, 5.25, 0.75)


@icon('help')
def _(g):
    _badge(g)
    g.path([('A', 8.5, 6.5, 2, 180, 405), ('L', 8.5, 9.4), ('L', 8.5, 9.75)])
    g.dot(8.5, 11.75, 0.75)


@icon('error')
def _(g):
    _badge(g)
    g.line(6.5, 6.5, 10.5, 10.5)
    g.line(10.5, 6.5, 6.5, 10.5)


@icon('warning')
def _(g):
    g.axis(8.5, 8)
    g.poly([(8.5, 2.5), (14.5, 13.5), (2.5, 13.5)], close=True)
    g.line(8.5, 6.5, 8.5, 9.5)
    g.dot(8.5, 11.5, 0.75)


@icon('globe')
def _(g):
    _badge(g)
    g.ellipse(8.5, 8.5, 2.5, 6)
    g.line(2.5, 8.5, 14.5, 8.5)


@icon('colorwheel')
def _(g):
    _badge(g)
    g.line(8.5, 8.5, 8.5, 2.5)
    c, s = 6 * math.cos(math.radians(30)), 6 * math.sin(math.radians(30))
    g.line(8.5, 8.5, 8.5 - c, 8.5 + s)
    g.line(8.5, 8.5, 8.5 + c, 8.5 + s)


@icon('target')
def _(g):
    g.axis(8.5, 8.5)
    g.circle(8.5, 8.5, 4)
    g.dot(8.5, 8.5, 1.5)
    for a, b in ((2.5, 4.5), (12.5, 14.5)):
        g.line(8.5, a, 8.5, b)
        g.line(a, 8.5, b, 8.5)


@icon('joint')
def _(g):
    g.axis(8.5, 8.5)
    g.circle(8.5, 8.5, 2)
    for a, b in ((2.5, 6.5), (10.5, 14.5)):
        g.line(8.5, a, 8.5, b)
        g.line(a, 8.5, b, 8.5)


@icon('origin')
def _(g):
    g.line(3.5, 3.5, 13.5, 3.5)
    g.line(3.5, 3.5, 3.5, 13.5)
    g.dot(3.5, 3.5, 2)


@icon('anchor')
def _(g):
    g.axis(8.5, 8.5)
    g.circle(8.5, 3.5, 1)
    g.line(8.5, 4.5, 8.5, 13.5)
    g.line(6.5, 6.5, 10.5, 6.5)
    g.arc(8.5, 8.5, 5, 10, 170)


@icon('access')
def _(g):
    g.axis(8.5, 8.5)
    g.dot(8.5, 3, 1.5)
    g.line(3.5, 6.5, 13.5, 6.5)
    g.line(8.5, 6.5, 8.5, 10)
    g.poly([(5.5, 14), (8.5, 10), (11.5, 14)])


@icon('speed')
def _(g):
    g.arc(8, 10.5, 5.5, 180, 360)
    g.line(8, 10.5, 11, 6.5)


# ── view ─────────────────────────────────────────────────────────────────────────────────────────
def _lens(g):
    g.axis(7.5, 7.5)
    g.circle(7.5, 7.5, 5)
    d = 5 * math.cos(math.radians(45))
    g.line(7.5 + d, 7.5 + d, 14, 14)


@icon('search')
def _(g): _lens(g)


@icon('tool-zoom', sizes=(12, 16, 20))
def _(g): _lens(g)


@icon('zoomin')
def _(g):
    _lens(g)
    g.plus(7.5, 7.5, 2)


@icon('zoomout')
def _(g):
    _lens(g)
    g.line(5.5, 7.5, 9.5, 7.5)


@icon('find-replace')
def _(g):
    g.axis(6.5, 8.5)
    g.circle(6.5, 8.5, 4)
    d = 4 * math.cos(math.radians(45))
    g.line(6.5 + d, 8.5 + d, 12.5, 14.5)
    g.corner(13.5, 2.5, -1, 1, 3)
    g.line(13.5, 2.5, 10.75, 5.25)


@icon('fit')
def _(g):
    for sx in (1, -1):
        for sy in (1, -1):
            x, y = 8 - sx * 5.5, 8 - sy * 5.5
            g.poly([(x, y + sy * 3), (x, y), (x + sx * 3, y)])


@icon('eye')
def _(g):
    g.path([('M', 1.5, 8), ('C', 4, 4, 12, 4, 14.5, 8), ('C', 12, 12, 4, 12, 1.5, 8)], close=True)
    g.circle(8, 8, 2, fill=True)


@icon('eyeoff')
def _(g):
    g.path([('M', 1.5, 8), ('C', 4, 4, 12, 4, 14.5, 8), ('C', 12, 12, 4, 12, 1.5, 8)], close=True)
    g.line(3, 13, 13, 3)


@icon('ruler')
def _(g):
    g.axis(8.5, 8)
    g.rect(2.5, 5.5, 14.5, 10.5, 1)
    g.line(5.5, 5.5, 5.5, 7.5)
    g.line(8.5, 5.5, 8.5, 8.5)
    g.line(11.5, 5.5, 11.5, 7.5)


@icon('guide')
def _(g):
    with g.rotate(-45):
        g.rect(1.5, 5.5, 14.5, 10.5, 1)
        g.line(5, 5.5, 5, 7.5)
        g.line(8, 5.5, 8, 8.5)
        g.line(11, 5.5, 11, 7.5)


@icon('guides')
def _(g):
    # Two guides crossing: a solid cross where they meet, even dashes running away from it.
    g.plus(5.5, 10.5, 2)
    g.line(5.5, 2.5, 5.5, 3.5)
    g.line(5.5, 5.5, 5.5, 6.5)
    g.line(9.5, 10.5, 10.5, 10.5)
    g.line(12.5, 10.5, 13.5, 10.5)


def _magnet(g, x0, x1, top, cy):
    """Horseshoe open to the top."""
    r = (x1 - x0) / 2.0
    g.path([('M', x0, top), ('L', x0, cy), ('A', x0 + r, cy, r, 180, 0), ('L', x1, top)])


@icon('snap-grid')
def _(g):
    g.line(2.5, 5.5, 8.5, 5.5)
    g.line(5.5, 2.5, 5.5, 8.5)
    _magnet(g, 8.5, 13.5, 8.5, 11)


@icon('snap-guides')
def _(g):
    g.dashes(3.5, 2.5, 3.5, 13.5, 4)
    _magnet(g, 7.5, 13.5, 4.5, 9.5)


@icon('select-all')
def _(g):
    g.dash_rect(2.5, 2.5, 13.5, 13.5, 3)
    g.rect(5, 5, 11, 11, 1, fill=True)


@icon('masked')
def _(g):
    g.dash_rect(2.5, 2.5, 13.5, 13.5, 3)
    g.circle(8, 8, 2.5)


@icon('mask')
def _(g):
    g.rect(2.5, 2.5, 13.5, 13.5, 2)
    g.circle(8, 8, 2.5, fill=True)


@icon('mask-geometry')
def _(g): g.circle(8, 8, 5, fill=True)


@icon('mask-alpha')
def _(g):
    g.circle(8, 8, 5.5)
    g.path([('A', 8, 8, 5.5, -90, 90)], fill=True)


@icon('mask-invert')
def _(g):
    g.rect(2, 2, 14, 14, 2, fill=True)
    g.circle(8, 8, 2.5, hole=True)


def _sun(g, core, fill):
    g.axis(8.5, 8.5)
    g.circle(8.5, 8.5, core, fill=fill)
    for i in range(8):
        a = math.radians(45 * i)
        c, s = math.cos(a), math.sin(a)
        g.line(8.5 + 4.5 * c, 8.5 + 4.5 * s, 8.5 + 6 * c, 8.5 + 6 * s)


@icon('settings')
def _(g): _sun(g, 2.5, False)


@icon('mask-luma')
def _(g): _sun(g, 2.5, True)


# ── locks and links ──────────────────────────────────────────────────────────────────────────────
def _lock(g, l, t, r, b, shackle_end=360):
    cx, rad = (l + r) / 2.0, (r - l) / 2.0 - 2
    g.rect(l, t, r, b, 1)
    g.path([('M', cx - rad, t), ('L', cx - rad, t - 2.5), ('A', cx, t - 2.5, rad, 180, shackle_end)]
           + ([('L', cx + rad, t)] if shackle_end == 360 else []))


@icon('lock')
def _(g): _lock(g, 3.5, 7.5, 12.5, 13.5)


@icon('unlock')
def _(g): _lock(g, 3.5, 7.5, 12.5, 13.5, 335)


@icon('lock-fill')
def _(g):
    _lock(g, 2.5, 8.5, 9.5, 13.5)
    g.dashes(13.5, 2.5, 13.5, 13.5, 4)
    g.line(11.5, 2.5, 13.5, 2.5)
    g.line(11.5, 13.5, 13.5, 13.5)


@icon('lock-guides')
def _(g):
    g.dashes(3.5, 2.5, 3.5, 13.5, 4)
    _lock(g, 6.5, 8.5, 13.5, 13.5)


@icon('link')
def _(g):
    with g.rotate(-45):
        g.path([('M', 6.5, 5.5), ('L', 5, 5.5), ('A', 5, 8, 2.5, 270, 90), ('L', 6.5, 10.5)])
        g.path([('M', 9.5, 5.5), ('L', 11, 5.5), ('A', 11, 8, 2.5, -90, 90), ('L', 9.5, 10.5)])
        g.line(6, 8, 10, 8)


# ── transform ────────────────────────────────────────────────────────────────────────────────────
@icon('transform')
def _(g):
    g.rect(4.5, 4.5, 11.5, 11.5)
    for x in (3, 10):
        for y in (3, 10):
            g.rect(x, y, x + 3, y + 3, fill=True)


@icon('scale')
def _(g):
    g.rect(2.5, 8.5, 7.5, 13.5, 1)
    g.corner(13.5, 2.5, -1, 1, 4)
    g.line(13.5, 2.5, 9, 7)


@icon('to-stage')
def _(g):
    g.rect(2.5, 3.5, 13.5, 12.5, 1.5)
    g.rect(5, 6, 11, 10, fill=True)


@icon('stamp')
def _(g):
    g.rect(3.5, 6.5, 12.5, 13.5)
    g.rect(6.5, 2.5, 9.5, 6.5)


@icon('angle')
def _(g):
    g.poly([(10.5, 3.5), (2.5, 13.5), (13.5, 13.5)])
    g.arc(2.5, 13.5, 5.5, 360, 308.7)


@icon('tilt')
def _(g):
    # A stylus leaning over the tablet and the angle between them.
    g.line(2.5, 13.5, 13.5, 13.5)
    with g.rotate(25, 4.5, 13):
        g.poly([(3, 2.5), (6, 2.5), (6, 10), (4.5, 13), (3, 10)], close=True)
    g.arc(4.5, 13, 7, 318, 351)


@icon('pressure')
def _(g):
    g.line(2.5, 13.5, 13.5, 13.5)
    with g.rotate(45, 8, 6.5):
        g.poly([(6.5, 1), (9.5, 1), (9.5, 8), (8, 11), (6.5, 8)], close=True)


@icon('fold-closed')
def _(g):
    g.axis(8.5, 8.5)
    g.rect(3.5, 3.5, 13.5, 13.5, 2)
    g.plus(8.5, 8.5, 2.5)


@icon('fold-open')
def _(g):
    g.axis(8.5, 8.5)
    g.rect(3.5, 3.5, 13.5, 13.5, 2)
    g.line(6, 8.5, 11, 8.5)


@icon('flow-extend')
def _(g):
    g.rect(2.5, 4.5, 13.5, 11.5, 1.5)
    g.line(6.5, 4.5, 6.5, 11.5)
    g.line(9.5, 4.5, 9.5, 11.5)


@icon('flow-reflect')
def _(g): g.poly([(2.5, 11.5), (5.25, 4.5), (8, 11.5), (10.75, 4.5), (13.5, 11.5)])


@icon('flow-repeat')
def _(g):
    g.poly([(3.5, 11.5), (6.5, 4.5), (6.5, 11.5), (9.5, 4.5), (9.5, 11.5), (12.5, 4.5), (12.5, 11.5)])


@icon('pin')
def _(g):
    with g.rotate(45):
        g.line(5, 2, 11, 2)
        g.poly([(6.5, 2), (6.5, 6), (4.5, 9), (11.5, 9), (9.5, 6), (9.5, 2)])
        g.line(8, 9, 8, 14.5)


@icon('scissors')
def _(g):
    g.circle(4.5, 11.5, 2)
    g.circle(11.5, 11.5, 2)
    g.line(5.9, 10.1, 12.5, 2.5)
    g.line(10.1, 10.1, 3.5, 2.5)


# ── colour ───────────────────────────────────────────────────────────────────────────────────────
@icon('swatches')
def _(g):
    g.rect(2.5, 2.5, 6.5, 6.5, 1)
    g.rect(9, 2, 14, 7, 1.5, fill=True)
    g.rect(2, 9, 7, 14, 1.5, fill=True)
    g.rect(9.5, 9.5, 13.5, 13.5, 1)


@icon('color-default')
def _(g):
    g.rect(6.5, 6.5, 13.5, 13.5, 1)
    g.rect(2, 2, 9, 9, 1.5, fill=True)


@icon('color-none')
def _(g):
    g.rect(2.5, 2.5, 13.5, 13.5)
    g.line(13.5, 2.5, 2.5, 13.5)


@icon('nocolor')
def _(g):
    g.rect(2.5, 2.5, 13.5, 13.5, 2)
    g.line(12.9, 3.1, 3.1, 12.9)


@icon('gradient')
def _(g): g.rect(3, 3, 13, 13, fill=True)


# ── sound and controls ───────────────────────────────────────────────────────────────────────────
@icon('sound')
def _(g):
    g.poly([(2.5, 6.5), (5.5, 6.5), (9.5, 3.5), (9.5, 12.5), (5.5, 9.5), (2.5, 9.5)], close=True)
    g.arc(9, 8, 4, -42, 42)


@icon('waveform')
def _(g):
    if g.s == 12:
        with g.native():
            for i, h in enumerate((1, 3, 4.5, 2, 1)):
                x = 1.5 + 2 * i
                g.line(x, 6 - h, x, 6 + h)
        return
    g.axis(8.5, 8)
    for i, h in enumerate((1, 3, 5, 2, 4, 2, 1)):
        x = 2.5 + 2 * i
        g.line(x, 8 - h, x, 8 + h)


@icon('mixer')
def _(g):
    g.axis(8.5, 8)
    for x, y in ((3.5, 10.5), (8.5, 5.5), (13.5, 9.5)):
        g.line(x, 2.5, x, 13.5)
        g.dot(x, y, 2)


def _sliders(g, flip):
    with g.flip(8, flip):
        g.line(2.5, 4.5, 8.5, 4.5)
        g.line(12.5, 4.5, 13.5, 4.5)
        g.circle(10.5, 4.5, 2)
        g.line(2.5, 11.5, 3.5, 11.5)
        g.line(7.5, 11.5, 13.5, 11.5)
        g.circle(5.5, 11.5, 2)


@icon('customize')
def _(g): _sliders(g, False)


@icon('view-settings')
def _(g): _sliders(g, True)
