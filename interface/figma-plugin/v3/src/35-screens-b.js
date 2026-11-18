// NF3 screens B: floating panel and popover screens, motion tween, video, and the Timeline states legend.
// ---- Stage artwork of these screens (document points × k)
// Motion path of the layer "hand", frames 1–60: one dot per frame, the object at frame 30 with its selection box.
SC.motionPath = (f, k) => {
  const pt = s => [(100 + 560 * s) * k, (372 - 175 * (4 * s * (1 - s) + 0.3 * s)) * k];
  for (let i = 0; i < 60; i++) { const p = pt(i / 59); P2.put(f, dot(3, '#66A0FF', { name: 'Frame dot' }), p[0] - 1.5, p[1] - 1.5); }
  const a = pt(0), b = pt(29 / 59), c = pt(1), ghost = P2.put(f, dot(14, '#FF8E4F', { name: 'First frame' }), a[0] - 7, a[1] - 7);
  ghost.opacity = 0.35;
  P2.put(f, rect(6, 6, '#FFFFFF', { name: 'Path end', stroke: 'accent/default' }), c[0] - 3, c[1] - 3);
  P2.put(f, dot(28, '#FF8E4F', { name: 'hand' }), b[0] - 14, b[1] - 14);
  SC.selBox(f, b[0] - 18, b[1] - 18, 36, 36);
};
SC.videoArt = (f, k) => {
  P2.put(f, rect(320 * k, 180 * k, '#3A3550', { name: 'Video — intro.mp4' }), 56 * k, 214 * k);
  P2.put(f, icon('play', 'text/on-accent', 20), (216 - 10) * k, (304 - 10) * k);
  P2.put(f, txt('intro.mp4 · 00:07 / 00:24', 'Mono Small', '#FFFFFF'), 64 * k, 372 * k);
};

// ---- Overlays
SC.easeOpen = (sh, c) => {
  const p = SC.float(P2.ease('Classic tween · hand · frames 1–24', 'Custom', ['0.60', '0.02', '0.40', '0.98'], false, -1));
  abs(sh.root, p, c.X + 200, Math.max(40, c.Y + Math.round((c.H - SC.real(p.height, 100, 568)) / 2)));
};
// Swatches popover beside its anchor, the Fill chip of the Tools rail (rail children: grip, tools, divider, hand / zoom, divider, chips, …).
SC.swatchesOpen = (sh, c) => {
  const g = c.rail.children[5], chip = g.children[1];
  chip.strokes = fillOf('border/focus'); chip.strokeWeight = 2; chip.strokeAlign = 'INSIDE';
  for (const r of ['topLeftRadius', 'topRightRadius', 'bottomLeftRadius', 'bottomRightRadius']) chip.setBoundVariable(r, V('radius/control'));
  const pop = P2.popover('Swatches', {}, P2.swatchKids());
  const bottom = c.Y + c.rail.y + SC.real(g.y, 1, 364) + SC.real(chip.y, 1, 36) + SC.real(chip.height, 10, 28);
  abs(sh.root, pop, c.X + c.rail.x + SC.real(c.rail.width, 20, 40) + 8, Math.max(c.Y + c.R + 8, bottom - SC.real(pop.height, 100, 380)));
};
// View settings popover above its anchor, the last button of the Stage toolbar; right edges aligned.
SC.viewSettingsOpen = (sh, c) => {
  const pop = P2.viewSettings();
  abs(sh.root, pop, c.X + c.W - 14 - SC.real(pop.width, 150, 264), Math.max(c.Y + c.R + 8, c.Y + c.H - 14 - SC.real(c.bar.height, 10, 32) - 8 - SC.real(pop.height, 100, 480)));
};

// ---- Timeline states legend
SC.legend = () => {
  const cap = s => txt(s, 'Body', 'text/secondary');
  const head = s => row({ name: 'Caption', minH: 'size/section' }, [txt(s, 'Overline', 'text/tertiary')]);
  const item = (node, s) => row({ name: 'Item', gap: 16 }, [node, cap(s)]);
  const lr = (name, o, s) => item(SC.layer(name, Object.assign({ w: 'width/layer-column', mode: false }, o)), s);
  const fs = (spans, s, o) => item(SC.frames(spans, Object.assign({ w: 240 }, o || {})), s);
  const tg = (node, s) => row({ name: 'Toggle', gap: 12, h: 'size/layer-row' }, [row({ name: 'Glyph', w: 16, h: 16, justify: 'CENTER' }, [node]), cap(s)]);
  const grid = Array.from({ length: 20 }, (x, i) => rect(12, 'fill', (i + 1) % 5 === 0 ? 'timeline/empty-5th' : 'timeline/empty', { name: 'Frame ' + (i + 1) }));
  const onion = row({ name: 'Onion range', w: 84, h: 16, justify: 'SPACE_BETWEEN', fill: 'timeline/onion', fillOpacity: 0.25 }, [rect(3, 'fill', 'timeline/onion'),
    row({ name: 'Playhead head', pad: [0, 4], radius: 3, fill: 'timeline/playhead' }, [txt('8', 'Mono Small', 'text/on-accent')]), rect(3, 'fill', 'timeline/onion')]);
  markGrow(onion, false, true);
  const ruler = col({ name: 'Ruler sample', w: 240 }, [
    row({ name: 'Frame ruler', w: 'fill', h: 'size/timeline-ruler', pad: [0, 0, 0, 48], gap: 6, fill: 'bg/panel', stroke: 'border/subtle', sides: [0, 0, 1, 0], clip: true }, [onion, txt('1.21s', 'Mono Small', 'timeline/playhead')]),
    SC.frames([[20, 'static', { end: true }]])]);
  P2.playhead(ruler, 89.5);
  const layers = col({ name: 'Layer types', gap: 'gap/row' }, [head('Layer types'),
    lr('Layer', {}, 'Normal layer'),
    lr('character', { icon: 'folderopen', chev: true }, 'Folder (expanded) — children indent'),
    lr('background', { icon: 'folder', chev: false }, 'Folder (collapsed)'),
    lr('Guide: path', { icon: 'motionguide', level: 1 }, 'Motion guide — guided layers sit under it'),
    lr('ball', { level: 2 }, 'Guided layer (indent 2)'),
    lr('mask', { icon: 'mask', level: 1 }, 'Mask layer'),
    lr('arm', { icon: 'masked', level: 2 }, 'Masked layer'),
    lr('Guide: sketch', { icon: 'guide' }, 'Guide layer — not exported'),
    lr('hand', { icon: 'masked', level: 2, sel: true }, 'Selected + current layer (pencil)'),
    head('Columns'),
    tg(dot(4, 'text/tertiary'), 'Shown (dot)'), tg(icon('x', 'danger/default', 12), 'Hidden (red X)'),
    tg(dot(4, 'text/tertiary'), 'Unlocked (dot)'), tg(icon('lock', 'text/secondary', 12), 'Locked'),
    tg(rect(8, 8, 'layer/blue', { radius: 2 }), 'Layer colour'), tg(rect(8, 8, null, { radius: 2, stroke: 'layer/blue' }), 'Outline view')]);
  const frames = col({ name: 'Frame states', gap: 'gap/row' }, [head('Frames (12 pt per frame)'),
    fs(grid, 'Empty frames — every 5th frame shaded'),
    fs([[12, 'static', { end: true }]], 'Keyframe ● with content, static frames (grey), span end ▯'),
    fs([[12, 'empty', { key: 'b', end: true }]], 'Blank keyframe ○ — empty frames stay white'),
    fs([[15, 'classic'], [1, 'static']], 'Classic tween — purple with arrow'),
    fs([[15, 'shape'], [1, 'static']], 'Shape tween — green with arrow'),
    fs([[15, 'broken']], 'Broken tween — dashed (missing end keyframe)'),
    fs([[18, 'motion', { props: 2, end: true }]], 'Motion tween — blue, property keyframes ◆'),
    fs([[7, 'static'], [7, 'selected'], [4, 'static', { key: 0, end: true }]], 'Selected frames'),
    fs([[12, 'static', { key: 'a', end: true }]], 'Action keyframe (script on frame)'),
    fs([[20, 'static', { label: 'intro' }]], 'Frame label (red flag)'),
    fs([[20, 'static', { comment: 'loop' }]], 'Frame comment (green //)'),
    fs([[20, 'static', { anchor: 'home' }]], 'Anchor (named anchor)'),
    fs([], 'Folder row — a folder has no frames', { fill: 'timeline/folder' }),
    item(ruler, 'Playhead with frame number; onion-skin range on the ruler')]);
  return col({ name: 'Panel / Timeline — states legend', pad: 'pad/dialog', fill: 'bg/panel', stroke: 'border/subtle', radius: 'radius/panel' },
    [row({ name: 'Columns', gap: 48, align: 'MIN' }, [layers, frames])]);
};

async function nf3BuildScreensB() {
  const page = await nfMaster(), out = [];
  SC.board(page, out, 'Screen / Editor — Ease panel', 'Panel / Ease floats over the Stage as its own window (radius/panel, popover shadow) while a classic tween span is selected on the Timeline and Properties shows the frame with the Ease field it edits.',
    SC.shell({ inspector: P1.propsFrame(), bottom: SC.timeline(SC.rows([[24, 'selected', { arrow: true }], [96, 'static', { end: true }]]), { head: 12 }) }), SC.easeOpen);
  SC.board(page, out, 'Screen / Editor — Swatches', 'Popover / Swatches opens beside its anchor, the Fill chip of the Tools rail, which takes the focus ring; it is bottom-aligned to the chip and kept below the ruler.',
    SC.shell(), SC.swatchesOpen);
  SC.board(page, out, 'Screen / Editor — View settings', 'Popover / View settings (Rulers, guides & grid) opens above the last button of the Stage toolbar with the right edges aligned.',
    SC.shell(), SC.viewSettingsOpen);
  SC.board(page, out, 'Screen / Editor — motion tween', 'A motion tween span (blue, two ◆ property keyframes) on the layer “hand”; its motion path with one dot per frame is drawn on the Stage with the object selected at the playhead, and Properties is the motion tween panel instead of the v2.25 floating Properties window.',
    SC.shell({ inspector: P1.propsMotion(), stageArt: SC.art(1, { over: SC.motionPath }), bottom: SC.timeline(SC.rows([[60, 'motion', { props: 2, end: true }]]), { head: 30 }) }));
  SC.board(page, out, 'Screen / Editor — video on the timeline', 'An embedded video instance is selected on the Stage; its video layer shows the film-strip span on the Timeline and Properties is the Video panel (cue points are a section of the panel, not a second tab).',
    SC.shell({ inspector: P1.propsVideo(), stageArt: SC.art(1, { over: SC.videoArt, sel: [56, 214, 320, 180] }),
      bottom: SC.timeline([['Orbit instances', { color: '#FF8E4F' }, [[120, 'static', { end: true }]]], ['intro.mp4', { icon: 'video', sel: true, mode: false }, [[96, 'video']]],
        ['Typography', { color: 'layer/green' }, [[120, 'static', { end: true }]]]], { head: 12 }) }));
  SC.board(page, out, 'Screen / Editor — Video edit workspace', 'Workspace “Video edit”: two-column Tools, the right dock split into Properties | Scene and Library | Project (the Library list scrolls inside its group), and the Timeline in Preview rows of 40 pt with thumbnails for the title clips, the film strip of intro.mp4 and the waveform of its audio layer.',
    SC.shell({ tools: 2, inspector: [SC.retab(S.propsDocument(), ['Properties', 'Scene']), SC.retab(P2.library(), ['Library', 'Project'])],
      bottom: SC.timeline([['Titles', {}, [[36, 'static', { thumb: true, end: true }], [48, 'static', { thumb: true, end: true }], [36, 'static', { thumb: true, end: true }]]],
        ['intro.mp4', { icon: 'video', mode: false }, [[96, 'video']]], ['intro.mp4 · audio', { icon: 'sound', mode: false }, [[96, 'sound', { h: 26 }]]]], { h: 40, head: 30 }) }),
    sh => SC.setProp(sh.bar.children[4], 'Value', 'Video edit'));
  out.push(nfBoard(page, 'Panel / Timeline — states legend', 'Every layer kind, column toggle, frame span and marker of the Timeline, drawn with the v3 layer row (size/layer-row, width/layer-column) and 12 pt frames; colours come from the timeline/* tokens only (static, classic-tween, shape-tween, motion-tween, empty, empty-5th, selected, marker, label, comment, folder, onion, playhead).',
    [SC.legend()], { noteW: 900 }));
  nfArrange(page); SC.purge();
  return out.map(b => ({ id: b.id, name: b.name, w: Math.round(b.width), h: Math.round(b.height) }));
}
