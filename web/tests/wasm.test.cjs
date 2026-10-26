// Runs against the actual compiled Rust WASM module. Build with scripts/build-web.sh first.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const runtimeDir = path.resolve(__dirname, '../../target/web-runtime');
const context = vm.createContext({TextEncoder, TextDecoder, WebAssembly, console, URL, Request, Response});
vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'),'utf8'), context);
const wasm = vm.runInContext('NeoFlashWasm',context);
context.wasmBytes = fs.readFileSync(path.join(runtimeDir,'neoflash_web_bg.wasm'));
const ready = vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', context);
const transform = x => ({x,y:0,scale_x:1,scale_y:1,rotation:0,alpha:1});
const script = source => ({language:'java_script',source});
const shape = x => ({id:'shape',name:'disc',transform:transform(x),kind:{type:'ellipse',width:20,height:20,fill:[220,0,0,255],stroke:[0,0,0,0],stroke_width:0},scripts:[{event:'click',script:script('trace("leaf")')}]});
const instance = (id,x) => ({id,name:id,transform:transform(x),kind:{type:'instance',symbol_id:'clip',playback:'independent'},scripts:[{event:'click',script:script('this.rotation += 15;')}]});
const key = (frame,elements,label='',tween=null) => ({frame,elements,label,tween});
function project() {
  return {format_version:1,name:'WASM verification',width:320,height:200,fps:24,background:[255,255,255,255],root:'root',symbols:[
    {id:'root',name:'Scene',kind:'movie_clip',timeline:{frame_count:4,layers:[{id:'root-layer',name:'Instances',visible:true,locked:false,keyframes:[key(0,[instance('left',0),instance('right',100)],'start'),key(3,[instance('left',0),instance('right',100)],'end')]}],scripts:[{frame:0,script:script('globalThis.__neverExecute = true;')}] }},
    {id:'clip',name:'Nested clip',kind:'movie_clip',timeline:{frame_count:4,layers:[{id:'clip-layer',name:'Drawing',visible:true,locked:false,keyframes:[key(0,[shape(0)],'', 'linear'),key(3,[shape(30)])]}],scripts:[]}},
  ]};
}
test('WASM executes shared independent clocks, labels, hierarchy and transform overrides',async()=>{
  await ready;
  const player=new wasm.WebPlayer(JSON.stringify(project()));
  assert.equal(player.current_frame(),0);
  assert.equal(player.resolve_frame('[]','end'),3);
  assert.equal(JSON.parse(player.child_path('[]','right'))[0],'right');
  assert.equal(player.control('[]','stop',0),true);
  assert.equal(player.control('["left"]','stop',0),true);
  player.tick();
  assert.equal(player.current_frame(),0);
  assert.equal(player.clip_frame('["left"]'),0);
  assert.equal(player.clip_frame('["right"]'),1);
  let scene=JSON.parse(player.scene_json());
  assert.equal(scene.find(item=>item.instance_path[0]==='left').transform[4],0);
  assert.equal(scene.find(item=>item.instance_path[0]==='right').transform[4],110);
  assert.equal(player.set_property('["right"]','x',150),true);
  scene=JSON.parse(player.scene_json());
  assert.equal(scene.find(item=>item.instance_path[0]==='right').transform[4],160);
  assert.equal(player.control('["right"]','goto_and_stop',3),true);
  assert.equal(player.clip_frame('["right"]'),3);
  assert.equal(player.control('["right"]','goto_and_stop',4),false);
  player.set_playing(false); player.reset();
  assert.equal(player.is_playing(),true);
  player.free();
});
test('WASM queues source without executing it and bubbles click from leaf to instance',async()=>{
  await ready;
  const player=new wasm.WebPlayer(JSON.stringify(project()));
  const startup=JSON.parse(player.scripts_json());
  assert.equal(startup.length,1);
  assert.equal(player.is_dispatch_current(JSON.stringify(startup[0])),true);
  assert.equal(globalThis.__neverExecute,undefined);
  player.click('["right","shape"]');
  const events=JSON.parse(player.scripts_json());
  assert.deepEqual(events.map(event=>event.target_path),[['right','shape'],['right']]);
  assert.ok(events.every(event=>event.event==='click'));
  player.free();
});
test('WASM rejects malformed or unsupported project versions',async()=>{
  await ready;
  const invalid=project(); invalid.format_version=987;
  assert.throws(()=>new wasm.WebPlayer(JSON.stringify(invalid)),/unsupported version/);
});
test('WASM changes instance lifetime when a placement disappears and returns', async()=>{
  await ready;
  const data=project();
  data.symbols[0].timeline.layers[0].keyframes.splice(1,0,key(1,[]),key(2,[instance('left',0),instance('right',100)]));
  const player=new wasm.WebPlayer(JSON.stringify(data));
  const before=player.lifetime_id('["left"]');
  assert.notEqual(before,'');
  player.tick();
  assert.equal(player.lifetime_id('["left"]'),'');
  player.tick();
  assert.notEqual(player.lifetime_id('["left"]'),'');
  assert.notEqual(player.lifetime_id('["left"]'),before);
  player.free();
});
test('WASM preserves curved compound paths and nested affine transforms',async()=>{
  await ready;
  const data=project();
  const geometry={type:'path',commands:[
    {op:'move_to',x:0,y:0},{op:'quadratic_to',cx:50,cy:100,x:100,y:0},{op:'close'},
    {op:'move_to',x:30,y:10},{op:'cubic_to',cx1:40,cy1:30,cx2:60,cy2:30,x:70,y:10},{op:'close'},
  ],fill_rule:'evenodd',fill:[255,100,50,255],stroke:[0,0,0,255],stroke_width:3};
  data.symbols[1].timeline.layers[0].keyframes.forEach(key=>{key.elements[0].kind=geometry});
  const right=data.symbols[0].timeline.layers[0].keyframes[0].elements[1];
  right.transform={...right.transform,rotation:90,scale_x:2,scale_y:3};
  const player=new wasm.WebPlayer(JSON.stringify(data));
  const item=JSON.parse(player.scene_json()).find(item=>item.instance_path[0]==='right');
  assert.deepEqual(item.kind,geometry);
  assert.ok(Math.abs(item.transform[0])<1e-10);
  assert.equal(item.transform[1],2);
  assert.equal(item.transform[2],-3);
  assert.equal(item.transform[4],100);
  player.free();
});
test('WASM exposes independent authored targets and validates activation lifetime/disabled state',async()=>{
 await ready;const data=project();
 const meta=name=>({name,description:'Counter',role:'button',tab_order:0,disabled:false});
 for(const keyframe of data.symbols[0].timeline.layers[0].keyframes){keyframe.elements[0].accessibility=meta('Left counter');keyframe.elements[1].accessibility=meta('Right counter');}
 data.symbols[0].timeline.layers[0].keyframes.splice(1,0,key(1,[]),key(2,[{...instance('left',0),accessibility:meta('Left counter')},{...instance('right',100),accessibility:{...meta('Right counter'),disabled:true}}]));
 const player=new wasm.WebPlayer(JSON.stringify(data));player.scripts_json();
 let nodes=JSON.parse(player.accessibility_json());assert.deepEqual(nodes.map(n=>n.accessibility.name),['Left counter','Right counter']);
 const old=nodes[0];assert.equal(player.activate_accessible(JSON.stringify(old.instance_path),old.lifetime_id),true);
 assert.equal(JSON.parse(player.scripts_json()).filter(x=>x.event==='click').length,1);
 player.tick();assert.equal(JSON.parse(player.accessibility_json()).length,0);
 player.tick();nodes=JSON.parse(player.accessibility_json());assert.equal(nodes.length,2);
 assert.equal(player.activate_accessible(JSON.stringify(old.instance_path),old.lifetime_id),false);
 const replacement=nodes.find(n=>n.accessibility.name==='Left counter');assert.notEqual(replacement.lifetime_id,old.lifetime_id);
 assert.equal(player.activate_accessible(JSON.stringify(replacement.instance_path),replacement.lifetime_id),true);
 const disabled=nodes.find(n=>n.accessibility.disabled);assert.equal(player.activate_accessible(JSON.stringify(disabled.instance_path),disabled.lifetime_id),false);
 player.free();
});
