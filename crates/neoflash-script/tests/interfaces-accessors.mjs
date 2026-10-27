// One model/component publication exercises nominal contracts and actual property
// descriptors together. Runtime/WASM evidence; visual browser QA is separate.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyInterfacesAccessors(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/interfaces-accessors-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/interfaces-accessors.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
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
    const scene=host.proxyFor([]),model=scene.model,other=scene.other,program=host.program();
    assert.equal(host.lastTrace(),'interfaces ready 2 2 7');
    assert.equal(scene.status.text,'bounded:2 | other:7');
    for (const name of ['app.ICounter','app.IReadable','app.IResettable']) {
      assert.equal(program.is(model,name),true,'base class implements the complete inherited interface closure');
      assert.equal(program.cast(name,other),other);
      assert.equal(program.is({value:2,increment(){}},name),false,'interface identity is nominal');
    }
    assert.equal(program.cast('app.ICounter',null),null);
    assert.throws(()=>program.cast('app.ICounter',{}),/value required/);
    const ICounter=program.type('app.ICounter');
    assert.equal(ICounter(model),model);assert.equal(model instanceof ICounter,true);
    assert.throws(()=>new ICounter(),/cannot be constructed/);
    assert.throws(()=>program.construct('app.ICounter'),/cannot be constructed/);
    assert.throws(()=>{scene.model={};},/value required/);
    assert.equal(scene.model,model,'failed interface-slot assignment preserves the old value');
    assert.equal(model.accept(other),other);assert.equal(model.accept(null),null);
    assert.throws(()=>model.accept({}),/value required/);
    model.reads=0;model.writes=0;
    assert.equal(model.post(),2);
    assert.deepEqual([model.stored,model.reads,model.writes],[3,1,1],'postfix evaluates inherited getter and overridden setter once');
    let conversions=0;
    model.value={valueOf(){conversions++;return 8.8;}};
    assert.equal(conversions,1);assert.equal(model.stored,8,'setter argument coerces before clamping and super delegation');
    model.value=50;assert.equal(model.stored,9);
    model.command=4;assert.equal(model.stored,4,'inherited write-only setter dispatches to the actual receiver');
    assert.throws(()=>model.command,/write-only/);
    assert.throws(()=>{model.label='replace';},/read-only/);
    const increment=model.increment;
    assert.equal(increment,model.increment);assert.equal(increment(1),5,'declared methods remain bound');
    assert.equal(other.value,7);
    const Counter=program.type('app.Counter');
    assert.equal(Counter.created,2);
    Counter.created={valueOf(){conversions++;return 6.9;}};
    assert.equal(Counter.created,6);assert.equal(conversions,2,'static accessors coerce through their declared setter');
    model.reset();
    host.stage().handlePointer('click',{},scene);
    assert.equal(host.lastTrace(),'interfaces click 1 1 7');
    assert.equal(scene.status.text,'bounded:1 | other:7');
    host.resetProject();
    const fresh=host.proxyFor([]);
    assert.notEqual(fresh.model,model);
    assert.equal(host.lastTrace(),'interfaces ready 2 2 7');
    assert.equal(host.program().is(model,'app.ICounter'),false,'old publication identities do not leak into a restarted program');
    host.stage().handlePointer('click',{},fresh);
    assert.equal(host.lastTrace(),'interfaces click 1 3 7');
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Interfaces/accessors: published typed components, inherited contracts/properties, coercion, method binding, input and Restart passed.');
}
