// One ordinary publication with real WASM and host bindings. The injected clock
// makes Timer lifecycle assertions deterministic; browser scheduling is separate QA.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyGeometryTimer(root, cargo) {
  const directory=mkdtempSync(resolve(root,'target/geometry-timer-'));
  let player,host,time=0,nextId=0;
  const pending=new Map();
  const timers={now:()=>time,schedule(callback,delay){const id=++nextId;pending.set(id,{callback,due:time+delay});return id;},cancel:id=>pending.delete(id)};
  function advance(ms){
    const end=time+ms;
    for(let budget=0;budget<100;budget++){
      const first=[...pending.entries()].sort((a,b)=>a[1].due-b[1].due)[0];
      if(!first||first[1].due>end){time=end;return;}
      pending.delete(first[0]);time=first[1].due;first[1].callback();
    }
    assert.fail('unexpected unbounded timer scheduling');
  }
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/geometry-timer.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,timers,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    const scene=host.proxyFor([]),oldTimer=scene.timer;
    assert.equal(host.lastTrace(),'geometry timer ready 0 0');
    assert.equal(pending.size,1);
    for(let tick=1;tick<=4;tick++){
      advance(scene.timer.delay);
      assert.equal(scene.ticks,tick);assert.equal(scene.marker.x,180+140*tick);assert.equal(scene.marker.y,220+36*tick);
    }
    assert.equal(host.lastTrace(),'geometry complete 4 4 false');assert.equal(pending.size,0);
    const program=host.program(),Point=program.type('flash.geom.Point'),Rectangle=program.type('flash.geom.Rectangle');
    const world=scene.marker.localToGlobal(new Point(0,0)),bounds=scene.marker.getBounds(scene);
    assert.equal(program.is(world,'flash.geom.Point'),true);assert.ok(bounds instanceof Rectangle);
    assert.deepEqual([bounds.width,bounds.height],[32,32]);
    const inverse=scene.mapping.clone();inverse.invert();
    const restored=inverse.transformPoint(world);
    assert.ok(Math.abs(restored.x-4)<1e-9&&Math.abs(restored.y)<1e-9);
    assert.equal(program.type('flash.utils.getTimer')(),1600);
    host.resetProject();
    assert.equal(host.lastTrace(),'geometry timer ready 0 0');
    assert.equal(host.program().type('flash.utils.getTimer')(),0);
    assert.throws(()=>oldTimer.start(),/previous publication session|disposed/);
    advance(400);assert.equal(host.proxyFor([]).ticks,1);assert.equal(pending.size,1);
    host.disposeFamily();assert.equal(pending.size,0);advance(2000);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Geometry/Timer: published typed coordinates/bounds, four ticks, completion, clock, Restart and disposal passed.');
}
