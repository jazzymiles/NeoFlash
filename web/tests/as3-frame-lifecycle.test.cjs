const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const { resolve } = require('node:path');
const { homedir } = require('node:os');
const { spawnSync } = require('node:child_process');

async function exportAndBoot(root, projectJson, tempPrefix) {
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
  const directory = fs.mkdtempSync(resolve(root, `target/${tempPrefix}-`));
  try {
    const sourcePath = resolve(directory, 'project.nflash');
    const outDir = resolve(directory, 'export');
    fs.writeFileSync(sourcePath, JSON.stringify(projectJson));
    const exported = spawnSync(
      cargo,
      ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--', sourcePath, '--output', outDir],
      { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 },
    );
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);

    const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(resolve(outDir, 'bundle.js'), 'utf8'), context);
    const bundle = vm.runInContext('NeoFlashBundle', context);
    vm.runInContext(fs.readFileSync(resolve(outDir, 'neoflash_web.js'), 'utf8'), context);
    context.wasmBytes = Buffer.from(bundle.wasm, 'base64');
    const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', context);
    const player = new wasm.WebPlayer(
      JSON.stringify(bundle.project),
      bundle.scenes ? JSON.stringify(bundle.scenes) : undefined,
    );
    const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
    const compiled = Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source },
      code: item.code,
      location: item.location,
    }));
    const { host, traceCalls } = createRuntimeHost(root, player, compiled, {
      bundle,
      context: { measureText: line => ({ width: line.length * 8 }) },
    });
    return { directory, player, host, traceCalls };
  } catch (error) {
    fs.rmSync(directory, { recursive: true, force: true });
    throw error;
  }
}

function rectElement(id, name = id) {
  return {
    id,
    name,
    transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
    kind: {
      type: 'rectangle',
      width: 20,
      height: 20,
      fill: [255, 0, 0, 255],
      stroke: [0, 0, 0, 0],
      stroke_width: 0,
    },
  };
}

function instElement(id, symbolId, name = id) {
  return {
    id,
    name,
    transform: { x: 0, y: 0, scale_x: 1, scale_y: 1, rotation: 0, alpha: 1 },
    kind: {
      type: 'instance',
      symbol_id: symbolId,
      playback: 'independent',
    },
  };
}

test('AS3 construction, broadcast phase order, re-entrant goto, detached clips, retirement and FrameLabel through exported HTML5/WASM', async () => {
  const root = resolve(__dirname, '../..');
  const project = {
    format_version: 6,
    name: 'AS3Lifecycle',
    width: 400,
    height: 300,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    document_class: 'Main',
    scenes: [],
    resources: [],
    library_folders: [],
    sources: [
      {
        id: 'src-main',
        path: 'Main.as',
        language: 'action_script3',
        source: `package {
            import flash.display.MovieClip;
            import flash.events.Event;
            import flash.events.EventPhase;
            public dynamic class Main extends MovieClip {
              public var panel:PanelClip;
              public var retiringBox:MovieClip;
              public var badgeCount:int = 7;
              public function Main() {
                this.addEventListener(Event.ADDED, this.onAdded);
                this.addEventListener(Event.ADDED_TO_STAGE, this.onAddedToStage);
                this.addEventListener(Event.FRAME_CONSTRUCTED, this.onFrameConstructed);
                this.addEventListener(Event.EXIT_FRAME, this.onExitFrame);
                this.addEventListener(Event.ENTER_FRAME, this.onEnterFrame);
                this.addEventListener(Event.RENDER, this.onRender);
                trace("Main.preSuper panel=" + (this.panel === null));
                super();
                trace("Main.postSuper panel=" + (this.panel !== null) + " count=" + this.badgeCount);
              }
              private function onAdded(e:Event):void {
                trace("ADDED:" + e.target.name + ":phase=" + e.eventPhase);
              }
              private function onAddedToStage(e:Event):void {
                trace("ADDED_TO_STAGE:" + e.target.name);
              }
              private function onFrameConstructed(e:Event):void {
                trace("FRAME_CONSTRUCTED:f=" + this.currentFrame);
              }
              private function onExitFrame(e:Event):void {
                trace("EXIT_FRAME:f=" + this.currentFrame);
              }
              private function onEnterFrame(e:Event):void {
                trace("ENTER_FRAME:f=" + this.currentFrame);
              }
              private function onRender(e:Event):void {
                trace("RENDER:f=" + this.currentFrame);
              }
            }
          }`,
      },
      {
        id: 'src-panel',
        path: 'PanelClip.as',
        language: 'action_script3',
        source: `package {
            import flash.display.MovieClip;
            public class PanelClip extends MovieClip {
              public function PanelClip() {
                trace("Panel.preSuper");
                super();
                trace("Panel.postSuper");
              }
            }
          }`,
      },
      {
        id: 'src-detached',
        path: 'DetachedClip.as',
        language: 'action_script3',
        source: `package {
            import flash.display.MovieClip;
            import flash.events.Event;
            public class DetachedClip extends MovieClip {
              public function DetachedClip() {
                this.addEventListener(Event.ADDED, function(e:Event):void {
                  trace("Detached.ADDED:" + e.target.name);
                });
                this.addEventListener(Event.ADDED_TO_STAGE, function(e:Event):void {
                  trace("Detached.ADDED_TO_STAGE:" + e.target.name);
                });
                this.addEventListener(Event.ENTER_FRAME, function(e:Event):void {
                  trace("Detached.ENTER_FRAME");
                });
                trace("Detached.preSuper");
                super();
                trace("Detached.beforeGoto");
                this.gotoAndStop(2);
                trace("Detached.afterGoto");
              }
            }
          }`,
      },
    ],
    symbols: [
      {
        id: 'root',
        name: 'Root',
        kind: 'movie_clip',
        folder: '',
        timeline: {
          frame_count: 3,
          layers: [
            {
              id: 'l-root-panel',
              name: 'PanelLayer',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  elements: [instElement('panel-inst', 'panel-sym', 'panel')],
                  label: 'start',
                },
              ],
            },
            {
              id: 'l-root',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  elements: [instElement('retire-inst', 'retire-sym', 'retiringBox')],
                  label: '',
                },
                {
                  frame: 1,
                  elements: [],
                  label: 'target',
                },
                {
                  frame: 2,
                  elements: [],
                  label: 'done',
                },
              ],
            },
          ],
          scripts: [
            {
              frame: 0,
              script: {
                language: 'action_script3',
                source: `trace("Script:Main:f1 panel=" + (this.panel !== null) + " retiring=" + (this.retiringBox !== null));
                  this.retiringBox.addEventListener(Event.REMOVED, function(e:Event):void {
                    trace("REMOVED:" + e.target.name);
                  });
                  this.retiringBox.addEventListener(Event.REMOVED_FROM_STAGE, function(e:Event):void {
                    trace("REMOVED_FROM_STAGE:" + e.target.name);
                  });
                  var lbls:Array = this.currentLabels;
                  lbls[1].addEventListener(Event.FRAME_LABEL, function(e:Event):void {
                    trace("FRAME_LABEL:" + e.target.name);
                  });
                  this.stage.invalidate();
                  var _forceLink:DetachedClip;`,
              },
            },
            {
              frame: 1,
              script: {
                language: 'action_script3',
                source: `trace("Script:Main:f2:beforeInner retiringBox=" + this.retiringBox);
                  this.panel.gotoAndStop(2);
                  trace("Script:Main:f2:afterInner panelFrame=" + this.panel.currentFrame);
                  this.stop();`,
              },
            },
          ],
        },
      },
      {
        id: 'panel-sym',
        name: 'PanelSym',
        class_name: 'PanelClip',
        linkage_id: 'PanelClip',
        kind: 'movie_clip',
        folder: '',
        timeline: {
          frame_count: 3,
          layers: [
            {
              id: 'l-panel',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [
                {
                  frame: 0,
                  elements: [rectElement('panel-shape', 'panelShape')],
                  label: '',
                },
              ],
            },
          ],
          scripts: [
            {
              frame: 0,
              script: {
                language: 'action_script3',
                source: `trace("Script:Panel:f1");`,
              },
            },
            {
              frame: 1,
              script: {
                language: 'action_script3',
                source: `trace("Script:Panel:f2:beforeNext"); this.nextFrame(); trace("Script:Panel:f2:afterNext f=" + this.currentFrame);`,
              },
            },
            {
              frame: 2,
              script: {
                language: 'action_script3',
                source: `trace("Script:Panel:f3");`,
              },
            },
          ],
        },
      },
      {
        id: 'retire-sym',
        name: 'RetireSym',
        kind: 'movie_clip',
        folder: '',
        timeline: {
          frame_count: 1,
          layers: [{ id: 'l-retire', name: 'L', visible: true, locked: false, keyframes: [{ frame: 0, elements: [rectElement('r-box', 'rBox')], label: '' }] }],
          scripts: [],
        },
      },
      {
        id: 'detached-sym',
        name: 'DetachedSym',
        class_name: 'DetachedClip',
        linkage_id: 'DetachedClip',
        kind: 'movie_clip',
        folder: '',
        timeline: {
          frame_count: 2,
          layers: [{ id: 'l-det', name: 'L', visible: true, locked: false, keyframes: [{ frame: 0, elements: [rectElement('d-dot', 'dot')], label: '' }] }],
          scripts: [
            {
              frame: 0,
              script: { language: 'action_script3', source: `trace("Detached.SHOULD_BE_SUPERSEDED_f1");` },
            },
            {
              frame: 1,
              script: { language: 'action_script3', source: `trace("Detached.Script:f2");` },
            },
          ],
        },
      },
    ],
  };

  let handle;
  try {
    handle = await exportAndBoot(root, project, 'as3-frame-lifecycle');
    const { player, host, traceCalls } = handle;
    const flat = () => traceCalls.map(v => v.join(' '));

    // Startup: no ENTER_FRAME! Parent pre-super -> child construction & ADDED -> Parent post-super ->
    // Parent ADDED & ADDED_TO_STAGE -> FRAME_CONSTRUCTED -> FrameScripts (Main before Panel) -> EXIT_FRAME -> RENDER.
    host.dispatchScripts();
    assert.deepEqual(flat(), [
      'Main.preSuper panel=true',
      'Panel.preSuper',
      'ADDED:panelShape:phase=3',
      'Panel.postSuper',
      'ADDED:panel:phase=3',
      'ADDED:rBox:phase=3',
      'ADDED:retiringBox:phase=3',
      'Main.postSuper panel=true count=7',
      'ADDED:Root:phase=2',
      'ADDED_TO_STAGE:Root',
      'FRAME_CONSTRUCTED:f=1',
      'Script:Main:f1 panel=true retiring=true',
      'Script:Panel:f1',
      'EXIT_FRAME:f=1',
      'RENDER:f=1',
    ]);

    traceCalls.length = 0;

    // Tick 1: ENTER_FRAME first -> timeline advance to Frame 2 -> Construct (retire retiringBox -> REMOVED/REMOVED_FROM_STAGE) ->
    // FRAME_CONSTRUCTED -> FrameLabel.frameLabel ('target') -> Main Frame 2 script -> nested inner panel.gotoAndStop(2) ->
    // nested nextFrame() -> EXIT_FRAME -> return back to Main Frame 2 -> EXIT_FRAME.
    host.broadcastFrame();
    player.tick();
    host.dispatchScripts();
    assert.deepEqual(flat(), [
      'ENTER_FRAME:f=1',
      'REMOVED:rBox',
      'REMOVED:retiringBox',
      'REMOVED_FROM_STAGE:retiringBox',
      'FRAME_CONSTRUCTED:f=2',
      'FRAME_LABEL:target',
      'Script:Main:f2:beforeInner retiringBox=null',
      'FRAME_CONSTRUCTED:f=2',
      'Script:Panel:f2:beforeNext',
      'FRAME_CONSTRUCTED:f=2',
      'Script:Panel:f3',
      'EXIT_FRAME:f=2',
      'Script:Panel:f2:afterNext f=3',
      'EXIT_FRAME:f=2',
      'Script:Main:f2:afterInner panelFrame=3',
      'EXIT_FRAME:f=2',
    ]);

    // Verify detached MovieClip construction with inner gotoAndStop(2) inside constructor,
    // superseding detached Frame 1 script, detached ENTER_FRAME broadcast, and ADDED_TO_STAGE on addChild.
    traceCalls.length = 0;
    const main = host.proxyFor([]);
    const DetachedClip = host.program().type('DetachedClip');
    const detached = new DetachedClip();
    assert.deepEqual(flat(), [
      'Detached.preSuper',
      'Detached.ADDED:dot',
      'Detached.beforeGoto',
      'FRAME_CONSTRUCTED:f=2',
      'Detached.Script:f2',
      'EXIT_FRAME:f=2',
      'Detached.afterGoto',
    ]);
    assert.equal(detached.currentFrame, 2);

    traceCalls.length = 0;
    host.broadcastFrame();
    player.tick();
    host.dispatchScripts();
    assert.ok(flat().includes('Detached.ENTER_FRAME'), 'detached MovieClip receives ENTER_FRAME');
    assert.ok(!flat().includes('Detached.SHOULD_BE_SUPERSEDED_f1'), 'constructor gotoAndStop(2) superseded frame 1 script');

    traceCalls.length = 0;
    main.addChild(detached);
    assert.deepEqual(flat(), [
      `Detached.ADDED:${detached.name}`,
      `ADDED:${detached.name}:phase=3`,
      `Detached.ADDED_TO_STAGE:${detached.name}`,
    ]);
  } finally {
    if (handle?.host) {
      handle.host.stopTimers();
      handle.host.disposeFamily();
    }
    await new Promise(resolve => setTimeout(resolve, 0));
    if (handle?.player) handle.player.free();
    if (handle?.directory) fs.rmSync(handle.directory, { recursive: true, force: true });
  }
});

test('AS3 multi-scene gotoAndStop executes target scene frame script and FrameLabel synchronously without breaking AS2 deferred goto', async () => {
  const root = resolve(__dirname, '../..');
  const project = {
    format_version: 6,
    name: 'SceneAndMixedLifecycle',
    width: 300,
    height: 200,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'scene-a',
    scenes: ['scene-a', 'scene-b'],
    resources: [],
    library_folders: [],
    sources: [],
    symbols: [
      {
        id: 'scene-a',
        name: 'SceneA',
        kind: 'movie_clip',
        folder: '',
        timeline: {
          frame_count: 2,
          layers: [
            {
              id: 'la',
              name: 'LA',
              visible: true,
              locked: false,
              keyframes: [{ frame: 0, elements: [], label: 'startA' }],
            },
          ],
          scripts: [
            {
              frame: 0,
              script: {
                language: 'action_script3',
                source: `this.currentLabels;
                  this.scenes[1].labels[1].addEventListener(Event.FRAME_LABEL, function(e:Event):void {
                    trace("SCENE_B_LABEL:" + e.target.name);
                  });
                  trace("AS3.beforeSceneGoto");
                  this.gotoAndStop("destB", "SceneB");
                  trace("AS3.afterSceneGoto scene=" + this.currentScene.name + " frame=" + this.currentFrame);`,
              },
            },
          ],
        },
      },
      {
        id: 'scene-b',
        name: 'SceneB',
        kind: 'movie_clip',
        folder: '',
        timeline: {
          frame_count: 3,
          layers: [
            {
              id: 'lb',
              name: 'LB',
              visible: true,
              locked: false,
              keyframes: [
                { frame: 0, elements: [instElement('legacyClip', 'as2-clip')], label: 'entryB' },
                { frame: 1, elements: [], label: 'destB' },
              ],
            },
          ],
          scripts: [
            {
              frame: 1,
              script: {
                language: 'action_script3',
                source: `trace("AS3.SceneB.f2"); this.prevFrame(); trace("AS3.SceneB.f2.afterPrev sceneFrame=" + this.currentFrame);`,
              },
            },
            {
              frame: 0,
              script: {
                language: 'action_script3',
                source: `trace("AS3.SceneB.f1");`,
              },
            },
          ],
        },
      },
      {
        id: 'as2-clip',
        name: 'LegacyClip',
        kind: 'movie_clip',
        folder: '',
        timeline: {
          frame_count: 2,
          layers: [{ id: 'l2', name: 'L2', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], label: '' }] }],
          scripts: [
            {
              frame: 0,
              script: {
                language: 'action_script2',
                source: `trace("AS2.f1.before"); gotoAndStop(2); trace("AS2.f1.after");`,
              },
            },
            {
              frame: 1,
              script: {
                language: 'action_script2',
                source: `trace("AS2.f2");`,
              },
            },
          ],
        },
      },
    ],
  };

  let handle;
  try {
    handle = await exportAndBoot(root, project, 'as3-scene-mixed-lifecycle');
    const { host, traceCalls } = handle;
    host.dispatchScripts();
    assert.deepEqual(traceCalls.map(v => v.join(' ')), [
      'AS3.beforeSceneGoto',
      'SCENE_B_LABEL:destB',
      'AS3.SceneB.f2',
      'AS3.SceneB.f1',
      'AS2.f1.before',
      'AS2.f1.after',
      'AS2.f2',
      'AS3.SceneB.f2.afterPrev sceneFrame=1',
      'AS3.afterSceneGoto scene=SceneB frame=1',
    ]);
  } finally {
    if (handle?.host) {
      handle.host.stopTimers();
      handle.host.disposeFamily();
    }
    await new Promise(resolve => setTimeout(resolve, 0));
    if (handle?.player) handle.player.free();
    if (handle?.directory) fs.rmSync(handle.directory, { recursive: true, force: true });
  }
});
