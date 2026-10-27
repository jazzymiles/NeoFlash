// One ordinary resource-to-report workflow through exported scripts and real WASM.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyDataProcessing(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/data-processing-'));
  let player,host;
  async function ready(){
    const deadline=Date.now()+3000;
    while(!host.proxyFor([]).ready&&Date.now()<deadline)await new Promise(done=>setTimeout(done,5));
    assert.equal(host.proxyFor([]).ready,true,'embedded JSON finished loading');
  }
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/data-processing.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();await ready();
    let scene=host.proxyFor([]);
    assert.equal(scene.rows.length,3);assert.equal(scene.rows[0].hours,6,'JSON reviver converts input hours');
    assert.equal(scene.totalHours,13);
    assert.deepEqual(JSON.parse(scene.serialized),{count:3,hours:13,summary:'3 items / 13 hours'},'serialization includes the declared public getter');
    assert.equal(host.program().is(scene.codePattern,'RegExp'),true);
    assert.equal(host.program().is(scene.lastDate,'Date'),true);
    assert.equal(scene.dataStatus.text.includes('WEB #104 / 2026-10-27'),true);
    assert.equal(scene.dataStatus.text.includes('Build dashboard / 6 h'),true,'regex replacement normalizes input whitespace');
    assert.equal(scene.legacyStatus.text.includes('UTC 2026-10-27'),true);
    const firstDate=scene.lastDate,firstPattern=scene.codePattern,legacyDate=scene.legacyDate;
    const click=()=>{player.click(JSON.stringify(['advance-button']));host.dispatchScripts(['advance-button']);};
    click();
    assert.equal(scene.shift,1);assert.equal(scene.dataStatus.text.includes('WEB #104 / 2026-10-28'),true);
    assert.equal(scene.legacyStatus.text.includes('UTC 2026-10-28'),true);
    assert.equal(legacyDate.getTime(),Date.UTC(2026,9,28,9,30));
    assert.equal(scene.lastDate.getTime()-firstDate.getTime(),86400000);
    assert.equal(scene.codePattern,firstPattern,'the same regex is reused across calls');
    assert.equal(scene.totalHours,13);assert.equal(host.lastTrace(),'data pipeline 1 3 13 3 items / 13 hours');
    host.resetProject();await ready();scene=host.proxyFor([]);
    assert.equal(scene.shift,0);assert.notEqual(scene.codePattern,firstPattern);assert.notEqual(scene.legacyDate,legacyDate);
    assert.equal(scene.dataStatus.text.includes('WEB #104 / 2026-10-27'),true);
    click();assert.equal(scene.shift,1);assert.equal(host.lastTrace(),'data pipeline 1 3 13 3 items / 13 hours');
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Data processing: embedded JSON/reviver, RegExp captures/replacement, AS3/AS2 UTC dates, class getter report and Restart passed.');
}
