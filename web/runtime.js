/* NeoFlash HTML5 host. Timeline evaluation and display-list construction run in shared Rust/WASM. */
(function (global) {
  'use strict';

  function localPoint(matrix, x, y) {
    const [a, b, c, d, tx, ty] = matrix;
    const determinant = a * d - b * c;
    if (!Number.isFinite(determinant) || Math.abs(determinant) < 1e-12) return null;
    return [(d * (x - tx) - c * (y - ty)) / determinant, (-b * (x - tx) + a * (y - ty)) / determinant];
  }
  function hitShape(kind, x, y, measureText, testPath) {
    if (kind.type === 'rectangle') return x >= 0 && y >= 0 && x <= kind.width && y <= kind.height;
    if (kind.type === 'ellipse') {
      if (kind.width <= 0 || kind.height <= 0) return false;
      return Math.pow((x - kind.width / 2) / (kind.width / 2), 2) + Math.pow((y - kind.height / 2) / (kind.height / 2), 2) <= 1;
    }
    if (kind.type === 'text') {
      const lines = kind.text.split('\n');
      return x >= 0 && y >= 0 && y <= lines.length * kind.font_size * 1.2 &&
        x <= Math.max(0, ...lines.map(line => measureText(line, kind)));
    }
    if (kind.type === 'path') return testPath(kind, x, y);
    return false;
  }
  function oneBasedFrame(value, total, resolveLabel) {
    if (typeof value === 'string') {
      const label = resolveLabel(value);
      if (label >= 0) return label;
      if (!/^\d+$/.test(value)) throw new Error(`Frame label '${value}' does not exist.`);
      value = Number(value);
    }
    if (!Number.isInteger(value) || value < 1 || value > total) throw new Error(`Frame must be between 1 and ${total}.`);
    return value - 1;
  }
  function color(rgba) { return `rgba(${rgba[0]},${rgba[1]},${rgba[2]},${rgba[3] / 255})`; }
  function sourceKey(script) { return script.language + '\u0000' + script.source; }
  const helpers = { localPoint, hitShape, oneBasedFrame, sourceKey };
  if (typeof module !== 'undefined' && module.exports) module.exports = helpers;
  if (typeof document === 'undefined') return;

  const errorElement = document.getElementById('error');
  let failed = false;
  function fail(error) {
    failed = true;
    document.getElementById('loading').style.display = 'none';
    errorElement.style.display = 'block';
    errorElement.textContent = 'NeoFlash could not run this project.\n\n' + (error && error.stack || String(error));
    console.error(error);
  }

  async function boot() {
    const bundle = global.NeoFlashBundle;
    if (!bundle || typeof NeoFlashWasm !== 'function') throw new Error('The HTML5 runtime is incomplete. Re-export this project from NeoFlash.');
    const bytes = Uint8Array.from(atob(bundle.wasm), character => character.charCodeAt(0));
    await NeoFlashWasm({ module_or_path: bytes });
    const project = bundle.project;
    const player = new NeoFlashWasm.WebPlayer(JSON.stringify(project));
    const canvas = document.getElementById('stage');
    const context = canvas.getContext('2d');
    if (!context) throw new Error('This browser does not provide Canvas 2D.');
    if (!global.NeoFlashPaths) throw new Error('The vector path runtime is missing. Re-export this project.');
    const paths = new global.NeoFlashPaths.PathCache();
    const status = document.getElementById('status');
    const toggle = document.getElementById('toggle');
    const traceElement = document.getElementById('trace');
    const viewport = document.getElementById('viewport');
    const semanticLayer = new global.NeoFlashAccessibility.SemanticLayer(document.getElementById('semantic-layer'), node => {
      if (failed) return;
      try {
        if (player.activate_accessible(JSON.stringify(node.instance_path), node.lifetime_id)) { dispatchScripts(node.instance_path); render(); }
      } catch (error) { fail(error); }
    });
    const preview = new global.NeoFlashPreview.Preview(viewport, document.getElementById('stage-space'), document.getElementById('stage-surface'), canvas, project, () => render());
    const functions = new Map();
    const proxies = new Map();
    let scene = [];
    let lastTime = null;
    let accumulated = 0;
    let lastTrace = '';
    const bindings = ['event', '_root', '_parent', 'trace', 'play', 'stop', 'gotoAndPlay', 'gotoAndStop', 'nextFrame', 'prevFrame'];
    for (const script of bundle.scripts) {
      try {
        functions.set(sourceKey(script), {
          fn: new Function(...bindings, '"use strict";\nreturn (function () {\n' + script.code + '\n}).call(this);\n//# sourceURL=neoflash-user-script.js'),
          location: script.location,
        });
      } catch (error) { throw new Error(`Script syntax error in ${script.location}: ${error.message}`); }
    }
    for (const warning of bundle.warnings) console.warn('NeoFlash:', warning);

    const propertyMap = {
      x: 'x', y: 'y', rotation: 'rotation', alpha: 'alpha', scaleX: 'scale_x', scaleY: 'scale_y',
      _x: 'x', _y: 'y', _rotation: 'rotation', _alpha: 'alpha', _xscale: 'scale_x', _yscale: 'scale_y',
    };
    const percentage = new Set(['_alpha', '_xscale', '_yscale']);
    function proxyFor(path) {
      const key = JSON.stringify(path);
      const lifetime = player.lifetime_id(key);
      const cached = proxies.get(key);
      if (cached && cached.lifetime === lifetime) return cached.proxy;
      if (!lifetime) {
        if (cached) return cached.proxy;
        throw new Error(`Display object ${key} is no longer active.`);
      }
      const custom = Object.create(null);
      function assertAlive() {
        if (player.lifetime_id(key) !== lifetime) throw new Error(`Display object ${key} was removed or replaced.`);
      }
      const target = Object.create(null);
      const methods = {
        play: () => { assertAlive(); return player.control(key, 'play', 0); },
        stop: () => { assertAlive(); return player.control(key, 'stop', 0); },
        gotoAndPlay: value => goto('goto_and_play', value),
        gotoAndStop: value => goto('goto_and_stop', value),
        nextFrame: () => goto('goto_and_stop', Math.min(player.clip_frame(key) + 2, player.clip_frame_count(key))),
        prevFrame: () => goto('goto_and_stop', Math.max(player.clip_frame(key), 1)),
        trace: (...values) => trace(...values),
      };
      function goto(command, value) {
        assertAlive();
        const total = player.clip_frame_count(key);
        if (!total) throw new Error('Timeline controls apply to symbols and the root scene.');
        const frame = oneBasedFrame(value, total, label => player.resolve_frame(key, label));
        if (!player.control(key, command, frame)) throw new Error(`Cannot control timeline at ${key}.`);
      }
      const proxy = new Proxy(target, {
        get(_target, name) {
          if (name === Symbol.toStringTag) return 'NeoFlashDisplayObject';
          if (name === 'toString') return () => `[NeoFlash ${key}]`;
          if (name === 'toJSON') return () => ({ path });
          if (typeof name !== 'string') return undefined;
          if (name === '_root' || name === 'root') return proxyFor([]);
          if (name === '_parent' || name === 'parent') return path.length ? proxyFor(path.slice(0, -1)) : null;
          if (name === '_currentframe' || name === 'currentFrame') return player.clip_frame(key) + 1;
          if (name === '_totalframes' || name === 'totalFrames') return player.clip_frame_count(key);
          if (Object.hasOwn(methods, name)) return methods[name];
          if (Object.hasOwn(propertyMap, name)) {
            assertAlive();
            const value = player.property(key, propertyMap[name]);
            return percentage.has(name) ? value * 100 : value;
          }
          if (Object.hasOwn(custom, name)) return custom[name];
          const child = JSON.parse(player.child_path(key, name));
          return child ? proxyFor(child) : undefined;
        },
        set(_target, name, value) {
          if (typeof name !== 'string') return false;
          if (Object.hasOwn(propertyMap, name)) {
            assertAlive();
            if (name.startsWith('_')) value = Number(value);
            if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error(`${name} must be a finite number.`);
            if (!player.set_property(key, propertyMap[name], percentage.has(name) ? value / 100 : value)) {
              throw new Error(`Cannot change ${name} on inactive display object ${key}.`);
            }
          } else if (['currentFrame', 'totalFrames', '_currentframe', '_totalframes', '_root', '_parent', 'root', 'parent'].includes(name)) {
            throw new Error(`${name} is read-only.`);
          } else custom[name] = value;
          return true;
        },
      });
      proxies.set(key, { lifetime, proxy });
      return proxy;
    }
    function trace(...values) {
      console.log('[NeoFlash]', ...values);
      lastTrace = values.map(value => typeof value === 'string' ? value : String(value)).join(' ').slice(0, 160);
    }
    function dispatchScripts(clickTarget = null) {
      const clickTargetScope = clickTarget ? proxyFor(clickTarget) : null;
      let count = 0;
      for (;;) {
        const events = JSON.parse(player.scripts_json());
        if (!events.length) break;
        for (const item of events) {
          if (++count > 4096 || player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive gotoAndPlay/gotoAndStop calls.');
          if (!item.script.source.trim() || !player.is_dispatch_current(JSON.stringify(item))) continue;
          const compiled = functions.get(sourceKey(item.script));
          if (!compiled) throw new Error('A script is missing from the compiled export. Re-export the project.');
          const scope = proxyFor(item.target_path);
          const event = Object.freeze({
            type: typeof item.event === 'string' ? item.event : 'frame',
            target: item.event === 'click' && clickTargetScope ? clickTargetScope : scope,
            currentTarget: scope,
          });
          try {
            compiled.fn.call(scope, event, proxyFor([]), scope.parent, trace,
              scope.play, scope.stop, scope.gotoAndPlay, scope.gotoAndStop, scope.nextFrame, scope.prevFrame);
          } catch (error) {
            throw new Error(`${compiled.location}: ${error.message}`, { cause: error });
          }
        }
      }
      if (player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive timeline controls.');
    }
    function resize() { preview.resize(); }
    function font(kind) { return `${kind.font_size}px ${JSON.stringify(kind.font_family)}, sans-serif`; }
    function render() {
      scene = JSON.parse(player.scene_json());
      context.setTransform(canvas.width / project.width, 0, 0, canvas.height / project.height, 0, 0);
      context.clearRect(0, 0, project.width, project.height);
      context.fillStyle = color(project.background);
      context.fillRect(0, 0, project.width, project.height);
      for (const item of scene) {
        const kind = item.kind;
        context.save();
        context.transform(...item.transform);
        context.globalAlpha = Math.max(0, Math.min(1, item.alpha));
        if (kind.type === 'rectangle' || kind.type === 'ellipse') {
          context.beginPath();
          if (kind.type === 'rectangle') context.rect(0, 0, kind.width, kind.height);
          else context.ellipse(kind.width / 2, kind.height / 2, Math.max(0, kind.width / 2), Math.max(0, kind.height / 2), 0, 0, Math.PI * 2);
          context.fillStyle = color(kind.fill);
          context.fill();
          if (kind.stroke_width > 0 && kind.stroke[3]) {
            context.strokeStyle = color(kind.stroke);
            context.lineWidth = kind.stroke_width;
            context.stroke();
          }
        } else if (kind.type === 'path') {
          const path = paths.get(kind.commands);
          if (kind.fill[3] > 0) {
            context.fillStyle = color(kind.fill);
            context.fill(path, kind.fill_rule || 'nonzero');
          }
          if (kind.stroke_width > 0 && kind.stroke[3] > 0) {
            context.strokeStyle = color(kind.stroke);
            context.lineWidth = kind.stroke_width;
            context.lineCap = 'round';
            context.lineJoin = 'round';
            context.stroke(path);
          }
        } else if (kind.type === 'text') {
          context.font = font(kind);
          context.textBaseline = 'top';
          context.fillStyle = color(kind.color);
          kind.text.split('\n').forEach((line, index) => context.fillText(line, 0, index * kind.font_size * 1.2));
        } else throw new Error(`Unsupported display primitive: ${kind.type}`);
        context.restore();
      }
      const targetCount = semanticLayer.update(JSON.parse(player.accessibility_json()), scene, project, preview.view.scale, (line, kind) => { context.font = font(kind); return context.measureText(line).width; });
      if (targetCount) canvas.setAttribute('aria-hidden', 'true'); else canvas.removeAttribute('aria-hidden');
      status.textContent = `${player.current_frame() + 1} / ${player.total_frames()} · ${project.fps} fps`;
      if (traceElement.textContent !== lastTrace) traceElement.textContent = lastTrace;
      toggle.textContent = player.is_playing() ? 'Pause' : 'Play';
    }
    function hitAt(event) {
      const rect = canvas.getBoundingClientRect();
      const x = (event.clientX - rect.left) * project.width / rect.width;
      const y = (event.clientY - rect.top) * project.height / rect.height;
      for (let index = scene.length - 1; index >= 0; index--) {
        const item = scene[index];
        if (item.alpha <= 0) continue;
        const local = localPoint(item.transform, x, y);
        if (!local) continue;
        if (hitShape(item.kind, ...local, (line, kind) => {
          context.font = font(kind);
          return context.measureText(line).width;
        }, (kind, x, y) => global.NeoFlashPaths.contains(context, kind, x, y, paths.get(kind.commands)))) return item.instance_path;
      }
      return null;
    }
    canvas.addEventListener('click', event => {
      if (failed || preview.panMode || preview.suppressClick) return;
      try {
        const target = hitAt(event);
        if (target) { player.click(JSON.stringify(target)); dispatchScripts(target); render(); }
      } catch (error) { fail(error); }
    });
    canvas.addEventListener('pointermove', event => { canvas.style.cursor = preview.panMode ? 'grab' : (hitAt(event) ? 'pointer' : 'default'); });
    toggle.addEventListener('click', () => {
      player.set_playing(!player.is_playing());
      lastTime = null;
      accumulated = 0;
      render();
    });
    document.getElementById('restart').addEventListener('click', () => {
      failed = false;
      errorElement.style.display = 'none';
      proxies.clear(); lastTrace = ''; accumulated = 0; lastTime = null;
      try { player.reset(); dispatchScripts(); render(); } catch (error) { fail(error); }
    });
    global.addEventListener('resize', () => { resize(); render(); });
    document.addEventListener('visibilitychange', () => { lastTime = null; accumulated = 0; });
    document.getElementById('zoom').addEventListener('change', event => preview.setMode(event.target.value));
    const panButton = document.getElementById('pan');
    panButton.addEventListener('click', () => { preview.setPan(!preview.panMode); panButton.setAttribute('aria-pressed', String(preview.panMode)); canvas.style.cursor = preview.panMode ? 'grab' : 'default'; });
    new ResizeObserver(() => { resize(); render(); }).observe(viewport);
    resize();
    dispatchScripts();
    render();
    document.getElementById('loading').style.display = 'none';
    function animate(now) {
      if (!failed && !document.hidden) {
        try {
          if (lastTime !== null && player.is_playing()) {
            accumulated += Math.min(now - lastTime, 250);
            const interval = 1000 / project.fps;
            let ticks = 0;
            while (accumulated >= interval && ticks++ < 12) {
              accumulated -= interval;
              player.tick();
              dispatchScripts();
            }
            // Avoid a runaway catch-up after a suspended browser tab.
            if (ticks >= 12) accumulated = 0;
          }
          lastTime = now;
          render();
        } catch (error) { fail(error); }
      }
      global.requestAnimationFrame(animate);
    }
    global.requestAnimationFrame(animate);
    global.NeoFlashPlayer = Object.freeze({ root: proxyFor([]), pause: () => player.set_playing(false), resume: () => player.set_playing(true), restart: () => document.getElementById('restart').click() });
  }
  boot().catch(fail);
})(globalThis);
