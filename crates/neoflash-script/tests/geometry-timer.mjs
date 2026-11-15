// One ordinary publication with real WASM and host bindings. The injected clock
// makes Timer lifecycle assertions deterministic; browser scheduling is separate QA.
import assert from 'node:assert/strict';
import { runCargo } from '../../../scripts/test-cargo.cjs';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRequire } from 'node:module';
import { createRuntimeHost } from './host-integration.mjs';

const require=createRequire(import.meta.url);
function verifyTimerFailureRecovery(root,wasm){
  const jobs=new Map(),cancelled=[],failures=[],diagnostics=[];
  let time=0,nextId=0,cancelFails=false;
  const cleanupError=new Error('native timeout cancellation failed');
  const service=require(resolve(root,'web/timers.js')).create({
    events:require(resolve(root,'web/events.js')),wasm,now:()=>time,
    schedule(callback){const id=++nextId;jobs.set(id,callback);return id;},
    cancel(id){cancelled.push(id);if(cancelFails)throw cleanupError;jobs.delete(id);},
    onError:error=>failures.push(error),onDiagnostic:error=>diagnostics.push(error),
  });
  const fireNext=()=>{const [id,callback]=jobs.entries().next().value;jobs.delete(id);callback();};
  const original=new Error('authored Timer listener failure');
  let called=0;
  const timer=new service.Timer(10);
  timer.addEventListener('timer',()=>{called++;timer.delay=20;cancelFails=true;throw original;});
  timer.start();
  fireNext();
  assert.equal(failures[0],original,'failed cancellation cannot replace the original callback error');
  assert.equal(diagnostics[0],cleanupError,'secondary cleanup failure remains observable');
  assert.equal(timer.running,false,'state retires before native cancellation');
  for(const callback of jobs.values())callback();jobs.clear();
  assert.equal(called,1,'a native callback left queued by failed cancellation cannot run again');

  cancelFails=false;
  const second=new service.Timer(10),third=new service.Timer(10);
  for(const next of [second,third]){next.addEventListener('timer',()=>called++);next.start();}
  const callbacksBeforeReset=[...jobs.values()],cancelsBeforeReset=cancelled.length;
  cancelFails=true;time=100;
  assert.throws(()=>service.reset(),error=>error instanceof AggregateError&&error.cause===cleanupError);
  assert.equal(cancelled.length-cancelsBeforeReset,2,'reset attempts cancellation for every running timer');
  assert.throws(()=>second.start(),/previous publication session/);
  callbacksBeforeReset.forEach(callback=>callback());jobs.clear();
  assert.equal(called,1,'reset invalidates all old callbacks even if native cancellation fails');

  cancelFails=false;
  const id=service.setInterval(()=>called++,10);
  assert.equal(service.getTimer(),0);
  fireNext();assert.equal(called,2,'new callbacks work after failed reset');
  service.clearInterval(id);
  assert.equal(jobs.size,0);
  const finalTimer=new service.Timer(10);finalTimer.start();
  const afterDispose=[...jobs.values()];cancelFails=true;
  assert.throws(()=>service.dispose(),error=>error===cleanupError);
  afterDispose.forEach(callback=>callback());jobs.clear();
  assert.throws(()=>finalTimer.start(),/disposed/);
  assert.equal(called,2);
  service.dispose();
}

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
    const exported=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/geometry-timer.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
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
    const Matrix3D=program.type('flash.geom.Matrix3D'),PerspectiveProjection=program.type('flash.geom.PerspectiveProjection');
    const m3d=new Matrix3D();
    m3d.appendTranslation(12,34,0);
    scene.marker.transform.matrix3D=m3d;
    assert.equal(program.is(scene.marker.transform.matrix3D,'flash.geom.Matrix3D'),true);
    assert.equal(scene.marker.x,12);assert.equal(scene.marker.y,34);
    const rel3d=scene.marker.transform.getRelativeMatrix3D(scene);
    assert.equal(program.is(rel3d,'flash.geom.Matrix3D'),true);
    scene.marker.transform.matrix3D=null;
    const pp=new PerspectiveProjection();
    pp.fieldOfView=60;
    scene.marker.transform.perspectiveProjection=pp;
    assert.equal(scene.marker.transform.perspectiveProjection.fieldOfView,60);
    const AccessibilityProperties=program.type('flash.accessibility.AccessibilityProperties');
    const acc=new AccessibilityProperties();
    acc.name='Marker';acc.description='Moving marker';
    scene.marker.accessibilityProperties=acc;
    assert.equal(scene.marker.accessibilityProperties.name,'Marker');
    const ContextMenu=program.type('flash.ui.ContextMenu'),ContextMenuItem=program.type('flash.ui.ContextMenuItem');
    const cm=new ContextMenu();
    cm.hideBuiltInItems();
    assert.equal(cm.builtInItems.play,false);
    const cmi=new ContextMenuItem('Inspect',true,true,true);
    cm.customItems=[cmi];
    scene.contextMenu=cm;
    assert.equal(scene.contextMenu.customItems.length,1);
    assert.equal(scene.requestSoftKeyboard(),true);
    assert.ok(host.stage().stage.stageVideos.length>=0);
    assert.ok(host.stage().stage.stage3Ds.length>=0);
    host.resetProject();
    assert.equal(host.lastTrace(),'geometry timer ready 0 0');
    assert.equal(host.program().type('flash.utils.getTimer')(),0);
    assert.throws(()=>oldTimer.start(),/previous publication session|disposed/);
    advance(400);assert.equal(host.proxyFor([]).ticks,1);assert.equal(pending.size,1);
    host.disposeFamily();assert.equal(pending.size,0);advance(2000);
    verifyTimerFailureRecovery(root,wasm);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Geometry/Timer: published typed coordinates/bounds, four ticks, completion, clock, Restart and disposal passed.');
}
