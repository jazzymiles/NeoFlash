// One command-bus publication combines optional/rest signatures, argument
// activations and receivers across AS3 classes and an AS2 timeline.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyFunctionWorkflow(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/function-workflow-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/function-workflow.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    let scene=host.proxyFor([]);
    const values=()=>[scene.primary.value,scene.secondary.value,scene.legacyOne.value,scene.legacyOther.value];
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    assert.deepEqual(values(),[0,10,0,20]);
    for (const id of ['default-command','batch-command','borrow-command']) {
      assert.equal(scene[id][id+'-symbol-label'].selectable,false,'decorative button captions must not open the native text editor');
    }
    assert.equal(scene.argumentSummary,'5 / argc 0 | 0 / argc 1','only omission invokes a default; explicit undefined still coerces');
    assert.equal(scene.argumentTyped,'7 / argc 2');
    assert.equal(scene.argumentValueType,'number','arguments contains the coerced normal prefix');
    assert.equal(scene.calleeMatches,true,'arguments.callee is the stable public bound method');
    assert.equal(scene.primary.execute.length,2);assert.equal(scene.bus.run.length,0);
    assert.equal(scene.primary.execute,scene.bus.listeners[0]);
    assert.equal(scene.legacyShared,true,'AS2 instances share the same unbound prototype method');
    assert.equal(host.program().is(scene.secondary,'app.ICommand'),true);
    click('default-command');
    assert.deepEqual(values(),[1,12,0,21],'interface implementations retain their own optional default values');
    assert.equal(scene.total,13);assert.equal(scene.primary.last,'tap');assert.equal(scene.secondary.last,'bonus');
    assert.equal(scene.legacyOther.seen,0);
    click('batch-command');
    assert.deepEqual(values(),[4,15,5,21]);assert.equal(scene.total,19);
    assert.equal(scene.legacyOne.seen,2,'AS2 apply passes all supplied values through arguments');
    click('borrow-command');
    assert.deepEqual(values(),[8,15,9,21]);
    assert.equal(scene.boundResult,8);assert.equal(scene.ordinaryResult,103);assert.equal(scene.borrowed.value,103);
    assert.equal(scene.legacyOne.seen,2,'AS2 extra arguments remain accepted by declared methods');
    assert.equal(scene['modern-values'].text,'8 primary / 15 secondary');
    assert.equal(scene['legacy-values'].text,'9 first / 21 other');
    const primary=scene.primary;
    assert.throws(()=>primary.execute(1,'too many',3),/argument|arity/i);
    assert.equal(primary.value,8,'AS3 arity rejection happens before the method mutates its receiver');
    assert.throws(()=>scene.bus.add(),/argument|arity/i);
    assert.equal(scene.bus.listeners.length,2);
    host.resetProject();scene=host.proxyFor([]);
    assert.deepEqual(values(),[0,10,0,20]);assert.notEqual(scene.primary,primary);
    assert.equal(scene.calleeMatches,true);
    click('default-command');assert.deepEqual(values(),[1,12,0,21]);assert.equal(scene.commands,1);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Source functions: published command bus, optional/rest signatures, arguments, bound/dynamic call/apply, AS2 timeline and Restart passed.');
}
