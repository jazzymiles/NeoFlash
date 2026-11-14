const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const REPO_ROOT = path.resolve(__dirname, '../..');
const WASM_JS_PATH = path.join(REPO_ROOT, 'target/web-runtime/neoflash_web.js');
const WASM_BIN_PATH = path.join(REPO_ROOT, 'target/web-runtime/neoflash_web_bg.wasm');

let cachedWasm = null;
function loadWasm() {
  if (cachedWasm) return cachedWasm;
  assert.ok(
    fs.existsSync(WASM_JS_PATH) && fs.existsSync(WASM_BIN_PATH),
    `Expected built WASM runtime at ${WASM_JS_PATH}. Run ./scripts/build-web.sh first.`
  );
  const wasmApi = new Function(fs.readFileSync(WASM_JS_PATH, 'utf8') + '\nreturn NeoFlashWasm;')();
  wasmApi.initSync({ module: fs.readFileSync(WASM_BIN_PATH) });
  cachedWasm = wasmApi;
  return cachedWasm;
}

function makeMultiSceneAndClickProject() {
  const btnSym = {
    id: 'btn_sym',
    name: 'GateButton',
    folder: '',
    kind: 'button',
    timeline: {
      frame_count: 1,
      layers: [
        {
          id: 'bl',
          name: 'Up',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: '',
              tween: null,
              elements: [
                {
                  id: 'br',
                  name: '',
                  transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                  kind: {
                    type: 'path',
                    commands: [
                      { op: 'move_to', x: 0, y: 0 },
                      { op: 'line_to', x: 100, y: 0 },
                      { op: 'line_to', x: 100, y: 50 },
                      { op: 'line_to', x: 0, y: 50 },
                      { op: 'close' },
                    ],
                    fill_rule: 'nonzero',
                    fill: [56, 189, 248, 255],
                    stroke: [255, 255, 255, 255],
                    stroke_width: 2,
                  },
                },
              ],
            },
          ],
        },
      ],
      scripts: [],
      guides: [],
    },
  };

  const rootSym = {
    id: 'root',
    name: 'FlattenedScenes',
    folder: '',
    kind: 'movie_clip',
    timeline: {
      frame_count: 4,
      layers: [
        {
          id: 'l_main',
          name: 'Stage',
          visible: true,
          locked: false,
          keyframes: [
            {
              frame: 0,
              label: 'intro_start',
              tween: null,
              elements: [
                {
                  id: 'btn_inst',
                  name: 'actionBtn',
                  transform: { x: 40, y: 40, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
                  kind: { type: 'instance', symbol_id: 'btn_sym', playback: 'independent' },
                  scripts: [
                    {
                      event: 'click',
                      script: { language: 'action_script3', source: 'trace("clicked");' },
                    },
                  ],
                },
              ],
            },
            {
              frame: 2,
              label: 'finale_start',
              tween: null,
              elements: [],
            },
          ],
        },
      ],
      scripts: [
        {
          frame: 0,
          script: { language: 'action_script3', source: 'trace("scene1");' },
        },
        {
          frame: 2,
          script: { language: 'action_script3', source: 'trace("scene2");' },
        },
      ],
      guides: [],
    },
  };

  return {
    project: {
      format_version: 6,
      name: 'Publish Gate WASM Contract',
      width: 640,
      height: 360,
      fps: 24,
      background: [15, 23, 42, 255],
      root: 'root',
      symbols: [btnSym, rootSym],
      library_folders: [],
      sources: [],
      resources: [],
      scenes: [],
    },
    scenes: [
      { name: 'Intro', start: 0, frames: 2 },
      { name: 'Finale', start: 2, frames: 2 },
    ],
  };
}

test('WASM WebPlayer handles multi-scene goto, ObjectScript click dispatch, and Restart reset cleanly', () => {
  const wasm = loadWasm();
  const { project, scenes } = makeMultiSceneAndClickProject();
  const player = new wasm.WebPlayer(JSON.stringify(project), JSON.stringify(scenes));

  assert.equal(player.scene_count('[]'), 2);
  assert.equal(player.total_frames(), 4);
  assert.equal(player.current_frame(), 0);

  const initialScripts = JSON.parse(player.scripts_json());
  assert.ok(initialScripts.some((s) => s.event && s.event.frame === 0));

  const nodes = JSON.parse(player.display_nodes_json());
  const btnNode = nodes.find((n) => n.name === 'actionBtn');
  assert.ok(btnNode && Array.isArray(btnNode.path), 'expected actionBtn display node');
  const btnRefJson = player.object_ref(JSON.stringify(btnNode.path));
  assert.ok(JSON.parse(btnRefJson), 'expected actionBtn object_ref');

  const firstLifetime = player.lifetime_id(JSON.stringify(btnNode.path));
  assert.ok(firstLifetime.length > 0);

  assert.equal(player.object_click(btnRefJson), true);
  const clickDispatches = JSON.parse(player.scripts_json());
  assert.equal(clickDispatches.length, 1);
  assert.equal(clickDispatches[0].event, 'click');
  assert.deepEqual(clickDispatches[0].target_path, btnNode.path);

  const scene2Frame = player.resolve_goto(
    '[]',
    JSON.stringify({ frame: 'finale_start', scene: 'Finale', as2: false })
  );
  assert.equal(scene2Frame, 2);
  assert.equal(player.control('[]', 'goto_and_stop', scene2Frame), true);
  assert.equal(player.current_frame(), 2);
  const scene2Scripts = JSON.parse(player.scripts_json());
  assert.ok(scene2Scripts.some((s) => s.event && s.event.frame === 2));

  // Restart reset must rewind to frame 0, re-arm initial frame script, and rotate lifetimes.
  player.reset();
  assert.equal(player.current_frame(), 0);
  const resetNodes = JSON.parse(player.display_nodes_json());
  const resetBtn = resetNodes.find((n) => n.name === 'actionBtn');
  assert.ok(resetBtn && Array.isArray(resetBtn.path));
  assert.notEqual(player.lifetime_id(JSON.stringify(resetBtn.path)), firstLifetime);
  const resetScripts = JSON.parse(player.scripts_json());
  assert.ok(resetScripts.some((s) => s.event && s.event.frame === 0));
});

test('NeoFlashNet preserves resourceAliases for transcoded AVI->MP4 across root and child mounts', () => {
  const events = require('../events.js');
  const binary = require('../byte-array.js').create();
  const NeoFlashNet = require('../net.js');
  const sampleBytes = Uint8Array.from([0x00, 0x00, 0x00, 0x18]);
  const sampleB64 = Buffer.from(sampleBytes).toString('base64');
  const net = NeoFlashNet.create({
    events,
    binary,
    resources: [
      { id: 'vid_mp4', path: 'media/clip.mp4', media_type: 'video/mp4', byteLength: sampleBytes.length, data_base64: sampleB64 },
    ],
    resourceAliases: {
      'media/clip.avi': 'media/clip.mp4',
    },
  });

  const resolved = net.mediaSource('media/clip.avi');
  assert.equal(resolved.kind, 'resource');
  assert.equal(resolved.mediaType, 'video/mp4');
  assert.deepEqual(Array.from(resolved.bytes), Array.from(sampleBytes));

  const childResources = [
    { id: 'c_mp4', path: 'child/media/sub.mp4', media_type: 'video/mp4', byteLength: sampleBytes.length, data_base64: sampleB64 },
  ];
  const added = net.registerResources(childResources, null, { 'child/media/sub.avi': 'child/media/sub.mp4' });
  const childResolved = net.mediaSource('child/media/sub.avi');
  assert.equal(childResolved.kind, 'resource');
  assert.equal(childResolved.mediaType, 'video/mp4');

  net.unregisterResources(added);
  assert.throws(
    () => net.mediaSource('child/media/sub.avi'),
    /Media is not bundled/
  );
});

test('NeoFlashRuntime maps thrown errors inside #include and class scripts back to authored locations', () => {
  const helpers = require('../runtime.js');
  const scriptMeta = {
    source_url: 'neoflash-script-0.js',
    code: 'function blowUpNow() {\n  throw new Error("boom-gate");\n}\nblowUpNow();\n',
    source_locations: [
      { generated_line: 1, generated_column: 1, source: 'includes/fault_as3.as', line: 1, column: 1 },
      { generated_line: 2, generated_column: 3, source: 'includes/fault_as3.as', line: 2, column: 3 },
      { generated_line: 4, generated_column: 1, source: 'Main / frame 1', line: 2, column: 1 },
    ],
  };

  const fn = helpers.scriptFunction(scriptMeta, []);
  let caught = null;
  try {
    fn.call({});
  } catch (err) {
    caught = err;
  }
  assert.ok(caught, 'expected script throw');
  const bodyOffset = helpers.functionBodyLineOffset();
  const frames = helpers.mappedErrorFrames(caught, [scriptMeta], bodyOffset);
  assert.ok(frames.length >= 1, 'expected at least one mapped stack frame');
  const top = helpers.sourceLocationText(frames[0]);
  assert.ok(
    top.startsWith('includes/fault_as3.as:2:'),
    `expected includes/fault_as3.as:2:*, got ${top}`
  );
});
