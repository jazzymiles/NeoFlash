const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const { resolve } = require('node:path');
const { homedir } = require('node:os');
const { spawnSync } = require('node:child_process');

test('published DisplayObject/MovieClip family works through compiled AS2, actual host and Rust/WASM', async () => {
  const root = resolve(__dirname, '../..');
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargo = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
  const directory = fs.mkdtempSync(resolve(root, 'target/display-family-'));
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  let player;
  try {
    const exported = spawnSync(cargo,
      ['run', '--quiet', '--locked', '-p', 'neoflash-export', '--', 'examples/display-family.nflash', '--output', directory],
      { cwd: root, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
    assert.equal(exported.status, 0, exported.error?.message || exported.stderr);
    const context = vm.createContext({ TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response });
    vm.runInContext(fs.readFileSync(resolve(directory, 'bundle.js'), 'utf8'), context);
    const bundle = vm.runInContext('NeoFlashBundle', context);
    vm.runInContext(fs.readFileSync(resolve(directory, 'neoflash_web.js'), 'utf8'), context);
    context.wasmBytes = Buffer.from(bundle.wasm, 'base64');
    const wasm = await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)', context);
    player = new wasm.WebPlayer(JSON.stringify(bundle.project));
    const { host } = createRuntimeHost(root, player, Array.from(bundle.scripts, item => ({
      script: { language: item.language, source: item.source }, code: item.code, location: item.location,
    })), { bundle, context: { measureText: line => ({ width: line.length * 8 }) } });
    host.dispatchScripts();
    const scene = host.proxyFor([]), widget = scene.widget, tile = widget.tile;
    const click = name => { player.click(JSON.stringify([name])); host.dispatchScripts([name]); };
    assert.equal(host.lastTrace(), 'family ready');

    // Identity, hierarchy, geometry and coordinate conversion form one contract.
    click('inspect');
    assert.equal(host.lastTrace(), 'family children 2 identity true bounds 100x80 global 80,164');
    assert.equal(widget.parent, scene); assert.equal(scene.parent, host.stage().stage);
    assert.equal(widget.stage, scene.stage); assert.equal(widget.root, scene);
    click('resize');
    assert.equal(host.lastTrace(), 'resize 200 120 2 1.5');
    widget.visible = false;
    assert.equal(widget.width, 200, 'visibility must not erase geometric bounds');
    widget.visible = true;
    click('rotate');
    assert.equal(host.lastTrace(), 'rotate 45 bounds 226 226');
    const point = widget.localToGlobal({ x: 17, y: 13 }), local = widget.globalToLocal(point);
    assert.ok(Math.abs(local.x - 17) < 1e-9 && Math.abs(local.y - 13) < 1e-9);
    click('identity');
    assert.equal(host.lastTrace(), 'rename true true 1 true');
    assert.equal(widget.renamed, tile); assert.equal(widget.getChildByName('tile'), null);
    assert.throws(() => { widget.width = 50; }, /rotated object/);
    widget.rotation = 0; widget.width = 0; widget.width = 100;
    assert.equal(widget.width, 100, 'local geometry allows recovery from zero scale');

    // Public transport calls are void, with independent child clocks preserved.
    assert.equal(widget.stop(), undefined);
    assert.equal(widget.nextFrame(), undefined); assert.equal(widget.currentFrame, 2);
    assert.equal(widget.gotoAndPlay(1), undefined);
    for (let i = 0; i < 3; i++) { host.broadcastFrame(); player.tick(); host.dispatchScripts(); }
    assert.equal(widget.ticks, 3); assert.equal(scene.currentFrame, 1);
    assert.equal(widget.currentFrame, 4); assert.equal(tile.currentFrame, 4);
    host.stage().handlePointer('pointerdown', { button: 0, buttons: 1 }, widget);
    assert.equal(host.lastTrace(), 'AS2 press widget 15');
    click('callbacks');
    const ticks = widget.ticks;
    host.broadcastFrame(); player.tick(); host.dispatchScripts();
    assert.equal(widget.ticks, ticks); assert.equal(widget.currentFrame, 5);

    // Linked class descriptors can coexist with native members and custom fields.
    let amount = 4;
    Object.defineProperty(widget, 'amount', { get: () => amount, set: value => { amount = Number(value) | 0; } });
    widget.amount = 8.9; assert.equal(widget.amount, 8);
    assert.equal(host.program().type('flash.display.MovieClip').__nfIs(widget), true);
    assert.equal(host.program().type('flash.display.DisplayObject').__nfIs(scene.stage), true);
    // Dynamic objects use the same Rust scene and keep identity through hierarchy edits.
    const Sprite=host.program().type('flash.display.Sprite'),Shape=host.program().type('flash.display.Shape');
    const left=new Sprite(),right=new Sprite(),shape=new Shape();
    left.name='runtimeLeft';right.name='runtimeRight';shape.name='drawing';
    left.x=20;left.y=30;right.x=180;right.scaleX=2;
    assert.equal(left.root,null);assert.equal(left.stage,null);assert.equal(shape.parent,null);
    shape.graphics.beginFill(0x4288cc);shape.graphics.lineStyle(2,0x112233);
    shape.graphics.drawRect(0,0,40,20);shape.graphics.endFill();
    assert.equal(shape.width,42,'detached Graphics contributes bounds before first attachment');
    assert.equal(left.addChild(shape),shape);assert.equal(left.getChildByName('drawing'),shape);
    const identity=shape.toJSON().object_ref,events=[];
    shape.addEventListener('added',event=>events.push([event.type,event.target===shape,event.currentTarget===shape]));
    shape.addEventListener('removed',event=>events.push([event.type,event.target===shape,event.currentTarget===shape]));
    left.addEventListener('removed',event=>events.push(['old-parent',event.target===shape,event.currentTarget===left]));
    const prefix=scene.numChildren;
    scene.addChild(left);scene.addChild(right);
    assert.equal(left.stage,scene.stage);assert.equal(left.root,scene);
    assert.equal(JSON.parse(player.runtime_snapshot_json()).draws.some(item=>item.owner_ref.id===identity.id),true);
    right.addChild(shape);
    assert.equal(shape.parent,right);assert.equal(shape.width,42);assert.equal(right.width,84);
    assert.deepEqual(events,[['removed',true,true],['old-parent',true,true],['added',true,true]]);
    assert.deepEqual(shape.toJSON().object_ref,identity,'reparent must not replace the Rust object');
    right.removeChild(shape);assert.equal(shape.parent,null);assert.equal(shape.stage,null);
    assert.equal(JSON.parse(player.runtime_snapshot_json()).draws.some(item=>item.owner_ref.id===identity.id),false);
    right.addChild(shape);assert.equal(events.at(-1)[0],'added','listeners survive detachment');
    scene.swapChildren(left,right);assert.equal(scene.getChildIndex(right),prefix);
    scene.setChildIndex(left,prefix);assert.equal(scene.getChildAt(prefix),left);
    assert.throws(()=>scene.addChildAt(left,0),/authored|prefix/i);
    assert.throws(()=>scene.removeChild(widget),/authored/i);
    assert.throws(()=>shape.graphics.lineStyle(0),/hairlines/);
    const legacy=scene.createEmptyMovieClip('legacyRuntime',scene.getNextHighestDepth());
    legacy.beginFill(0xaa33bb,100);legacy.moveTo(0,0);legacy.lineTo(20,0);legacy.lineTo(10,20);legacy.endFill();
    assert.equal(legacy.totalFrames,1);assert.equal(legacy.width,20);
    legacy.removeMovieClip();assert.equal(legacy.parent,null);

    assert.equal(host.program().type('flash.geom.Point').__nfIs(point), true);

    // Plain authored TextFields are live display objects backed by Rust overrides.
    const title = scene.title, originalText = title.text;
    title.text = 'Zoe:45\r\nAda:30';
    assert.equal(title.text, 'Zoe:45\rAda:30');
    assert.equal(title.appendText('\nBob:20'), undefined);
    assert.equal(title.replaceText(0, 3, 'Zoë'), undefined);
    title.textColor = 0x48d8b0;
    assert.equal(title.length, 20); assert.equal(title.numLines, 3);
    assert.equal(title.getLineText(0), 'Zoë:45\r'); assert.equal(title.getLineLength(0), 7);
    assert.equal(title.getLineText(2), 'Bob:20'); assert.equal(title.textColor, 0x48d8b0);
    assert.equal(title.textWidth, 48, 'deterministic Canvas measurement service');
    assert.ok(Math.abs(title.textHeight - 97.2) < 1e-9);
    const liveTitle = JSON.parse(player.scene_json()).find(item => item.name === 'title');
    assert.equal(liveTitle.kind.type, 'text_field');
    assert.equal(liveTitle.kind.field.text, 'Zoë:45\nAda:30\nBob:20');
    assert.equal(liveTitle.kind.field.format.color, 0x48d8b0);
    assert.notEqual(bundle.project.symbols[0].timeline.layers[0].keyframes[0].elements[0].kind.text, liveTitle.kind.field.text);
    title.text = 'A😀\nB'; assert.equal(title.length, 5);
    assert.throws(() => title.replaceText(1, 2, 'X'), /Unicode/);
    assert.equal(title.text, 'A😀\rB', 'rejected range must not corrupt text');
    assert.throws(() => { title.htmlText = '<b>pending</b>'; }, /plain-text profile/);
    widget.text = 'custom'; widget.htmlText = 'custom html';
    assert.equal(widget.text, 'custom'); assert.equal(widget.htmlText, 'custom html');
    host.resetProject();
    assert.throws(() => title.text, /removed|replaced/);
    assert.throws(() => shape.x, /removed|replaced/);
    assert.throws(() => shape.graphics.clear(), /removed|replaced/);
    assert.equal(host.proxyFor([]).title.text, originalText);

    // The public linked-symbol workflow exercises the same host with two
    // independent authored namespaces, project constructors and AS2 attachMovie.
    player.free();player=null;
    const linkedDirectory=resolve(directory,'linked');
    const linkedExport=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/linked-movieclips.nflash','--output',linkedDirectory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(linkedExport.status,0,linkedExport.error?.message||linkedExport.stderr);
    const linkedSandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(fs.readFileSync(resolve(linkedDirectory,'bundle.js'),'utf8'),linkedSandbox);
    const linkedBundle=vm.runInContext('NeoFlashBundle',linkedSandbox);
    vm.runInContext(fs.readFileSync(resolve(linkedDirectory,'neoflash_web.js'),'utf8'),linkedSandbox);
    linkedSandbox.wasmBytes=Buffer.from(linkedBundle.wasm,'base64');
    const linkedWasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',linkedSandbox);
    player=new linkedWasm.WebPlayer(JSON.stringify(linkedBundle.project));
    const {host:linked,traceCalls:linkedTraces}=createRuntimeHost(root,player,Array.from(linkedBundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle:linkedBundle,context:{measureText:line=>({width:line.length*8})}});
    linked.dispatchScripts();
    const document=linked.proxyFor([]),a=document.a,b=document.b,classic=document.classic;
    assert.notEqual(a,b);assert.notEqual(a.caption,b.caption);assert.notEqual(a.badge,b.badge);
    assert.deepEqual([a.totalFrames,b.totalFrames,classic.totalFrames],[12,12,12]);
    assert.equal(a.caption.text,'AS3 linked');assert.equal(classic.caption.text,'AS2 attached');
    assert.equal(classic.tag,'init-before-constructor');assert.equal(classic.parent,document);
    assert.equal(linkedTraces.filter(row=>row[0]==='linked constructor').length,2,'each exported class constructor runs exactly once');
    assert.ok(linkedTraces.some(row=>row[0]==='legacy constructor'&&row[1]==='init-before-constructor'));
    assert.ok(linkedTraces.some(row=>row.join(' ')==='linked frame left first'));
    assert.ok(linkedTraces.some(row=>row.join(' ')==='linked frame right second'));
    const firstBadge=a.badge.currentFrame;
    linked.broadcastFrame();player.tick();linked.dispatchScripts();
    assert.equal(a.currentFrame,1);assert.equal(b.currentFrame,8);assert.equal(a.badge.currentFrame,firstBadge+1);
    linked.stage().handleKey('keydown',{key:'ArrowRight',code:'ArrowRight',keyCode:39});
    assert.equal(a.currentFrame,2);assert.equal(b.currentFrame,7);
    linked.stage().handlePointer('click',{},b);assert.equal(b.clicks,1);assert.equal(a.clicks,0);
    b.gotoAndPlay(2);const rightIdentity=b.toJSON().object_ref;
    linked.stage().stage.focus=b;document.removeChild(b);
    assert.equal(b.stage,null);assert.equal(linked.stage().stage.focus,null);
    linked.broadcastFrame();player.tick();linked.dispatchScripts();assert.equal(b.currentFrame,3,'detached timeline continues independently');
    document.addChild(b);assert.equal(b.clicks,1);assert.deepEqual(b.toJSON().object_ref,rightIdentity);
    const semantics=JSON.parse(player.runtime_snapshot_json()).semantics;
    const twoCards=semantics.filter(node=>node.accessibility.name==='Activate linked card');
    assert.equal(twoCards.length,2);assert.notDeepEqual(twoCards[0].owner_ref,twoCards[1].owner_ref);
    assert.deepEqual(twoCards[0].instance_path,twoCards[1].instance_path,'matching source paths belong to different namespaces');
    const legacyButton=semantics.find(node=>node.accessibility.name==='Activate legacy card');
    assert.equal(linked.activateSemantic(legacyButton),true);assert.equal(classic.clicks,1);
    assert.equal(classic.currentFrame,7);assert.equal(classic.root,document);
    assert.ok(linkedTraces.some(row=>row[0]==='legacy linked click'&&row[1]==='classic'&&row[2]===1));
    assert.throws(()=>document.attachMovie('linked card','wrong',document.getNextHighestDepth()),/identifier|linkage|export/i,'display names do not substitute for Export identifiers');
    const oldBadge=a.badge;linked.resetProject();
    assert.throws(()=>oldBadge.x,/removed|replaced/);assert.notEqual(linked.proxyFor([]).a,a);
    assert.equal(linked.proxyFor([]).classic.tag,'init-before-constructor');
    assert.equal(linked.activateSemantic(legacyButton),false,'stale semantic owner cannot activate a recreated namespace');
    linked.disposeFamily();
  } finally {
    if (player) player.free();
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
