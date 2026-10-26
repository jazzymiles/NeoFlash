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
