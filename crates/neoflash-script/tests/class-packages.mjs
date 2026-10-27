// One ordinary publication joins lexical traits, package collaboration,
// inheritance, public serialization and dynamic data to a visible component UI.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyClassPackages(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/class-packages-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/class-packages.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
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
    const values=()=>[scene.modelTotal,scene.modelLocal,scene.classic.counter.read()];
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    assert.deepEqual(values(),[10,100,20]);
    assert.equal(scene.sealedBlocked,true,'dynamic attribute is not inherited by a sealed child');
    assert.equal(scene.privateBlocked,true,'public computed read cannot reach a private slot');
    assert.equal(scene.summary,'0 revisions');
    assert.deepEqual(JSON.parse(scene.serialized),{total:10,local:100},'JSON only traverses public readable traits');
    assert.equal(scene.metadataKeys,'note','fixed public fields are not dynamic enumeration');
    for(const id of ['advance','tag'])assert.equal(scene[id][id+'-label'].selectable,false);
    click('advance');
    assert.deepEqual(values(),[14,102,23],'base and derived private slots remain distinct while protected virtual dispatch doubles the command');
    assert.equal(scene.summary,'1 revisions','same-package collaborator reads internal state');
    assert.deepEqual(JSON.parse(scene.serialized),{total:14,local:102});
    assert.equal(scene.state.note,'step 1');
    click('tag');
    assert.equal(scene.metadataKeys,'note, theme');assert.equal(scene.state.theme,'mint');
    assert.deepEqual(JSON.parse(scene['state-json'].text),{label:'session',safe:'session',note:'step 1',theme:'mint'});
    click('advance');assert.deepEqual(values(),[18,104,26]);assert.equal(scene.steps,2);
    click('tag');assert.equal(scene.metadataKeys,'note');assert.equal(scene.tagged,false);
    assert.equal(scene['model-values'].text,'18 base / 104 derived');
    const state=scene.state;
    host.resetProject();scene=host.proxyFor([]);
    assert.deepEqual(values(),[10,100,20]);assert.notEqual(scene.state,state);
    assert.equal(scene.sealedBlocked,true);assert.equal(scene.privateBlocked,true);
    click('advance');assert.deepEqual(values(),[14,102,23]);assert.equal(scene.steps,1);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Source class/package workflow: lexical private slots, protected dispatch, internal collaboration, public JSON, sealed/dynamic data, AS2 inheritance and Restart passed.');
}
