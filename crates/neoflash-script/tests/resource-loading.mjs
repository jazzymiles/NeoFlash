// One published data pipeline: embedded offline resources, actual asynchronous
// callbacks and a local HTTP POST/abort flow. Node fetch is not browser CORS QA.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { createServer } from 'node:http';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

async function until(predicate,label) {
  const deadline=Date.now()+3000;
  while(!predicate()&&Date.now()<deadline)await new Promise(done=>setTimeout(done,5));
  assert.ok(predicate(),label);
}

export async function verifyResourceLoading(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/resource-loading-'));
  const requests=[],held=[];
  const server=createServer((request,response)=>{
    const chunks=[];
    request.on('data',chunk=>chunks.push(chunk));
    request.on('end',()=>{
      const record={url:request.url,method:request.method,headers:request.headers,body:Buffer.concat(chunks).toString('utf8')};
      requests.push(record);
      if(request.url==='/slow'){held.push(response);return;}
      response.writeHead(200,{'content-type':'application/x-www-form-urlencoded'});
      response.end(record.body);
    });
  });
  let player,host;
  try {
    await new Promise((done,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',done);});
    const endpoint=`http://127.0.0.1:${server.address().port}`;
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/resource-loading.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    assert.equal(bundle.project.format_version,3);
    assert.equal(bundle.project.resources.length,3);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,net:{fetch,baseURL:'file:///resource-loading/index.html'},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    const scene=host.proxyFor([]);
    assert.equal(scene.readyCount,0,'embedded loads must not complete inline');
    assert.equal(scene.legacyDone,false);
    await until(()=>scene.readyCount===3&&scene.errorCount===1&&scene.legacyDone,'embedded AS3/AS2 resources must settle');
    assert.equal(scene.status.text,'Text: Portable data says hello — π');
    assert.equal(scene.measurements.text,'Form: NeoFlash | Limassol | tags: rust / web');
    assert.equal(scene.packetInfo.text,'Binary: 50 bytes | NeoFlash π 🦀 | done');
    assert.equal(scene.legacyStatus.text,'AS2 LoadVars: NeoFlash | Limassol | loaded: true');
    assert.equal(scene.cancelledCount,0);
    assert.ok(scene.progressCount>=1);
    assert.deepEqual([scene.binaryLoader.bytesLoaded,scene.binaryLoader.bytesTotal,scene.packet.bytesAvailable],[50,50,0]);
    assert.equal(requests.length,0,'offline embedded paths do not call the HTTP transport');
    assert.equal(host.program().is(scene.textLoader,'flash.net.URLLoader'),true);
    assert.equal(host.program().is(scene.formLoader.data,'flash.net.URLVariables'),true);
    scene.loadEcho(endpoint+'/echo');
    await until(()=>scene.echoDone,'AS3 POST response must complete through the emitted listener');
    assert.equal(scene.networkStatus.text,'HTTP 200: NeoFlash | rust / web');
    assert.equal(requests[0].method,'POST');
    assert.equal(requests[0].headers['x-neoflash-example'],'data-pipeline');
    const form=new URLSearchParams(requests[0].body);
    assert.equal(form.get('name'),'NeoFlash');assert.deepEqual(form.getAll('tag'),['rust','web']);
    scene.postLegacy(endpoint+'/echo');
    await until(()=>scene.legacyEchoDone,'AS2 sendAndLoad must complete with its actual receiver');
    assert.equal(scene.legacyStatus.text,'AS2 POST: Legacy NeoFlash | loaded: true');
    assert.equal(requests[1].method,'POST');
    assert.equal(requests[1].headers['x-neoflash-legacy'],'data-pipeline');
    assert.equal(new URLSearchParams(requests[1].body).get('name'),'Legacy NeoFlash');
    scene.echoDone=false;
    scene.loadEcho(endpoint+'/slow');
    let cancelled=0;
    scene.echoLoader.addEventListener('complete',()=>cancelled++);
    scene.echoLoader.addEventListener('ioError',()=>cancelled++);
    await until(()=>held.length===1,'network cancellation starts an actual request');
    scene.echoLoader.close();
    held[0].end('name=late&tag=late');
    const oldLoader=scene.textLoader,oldPacket=scene.packet;
    scene.loadEcho(endpoint+'/slow');
    let stale=0;
    scene.echoLoader.addEventListener('complete',()=>stale++);
    scene.echoLoader.addEventListener('ioError',()=>stale++);
    await until(()=>held.length===2,'Restart race has a pending transport');
    host.resetProject();
    held[1].end('name=obsolete&tag=obsolete');
    const fresh=host.proxyFor([]);
    await until(()=>fresh.readyCount===3&&fresh.errorCount===1&&fresh.legacyDone,'fresh publication must load its own resources');
    assert.equal(cancelled,0);assert.equal(stale,0,'cancelled and previous-session callbacks stay silent');
    assert.equal(fresh.echoDone,false);assert.equal(fresh.cancelledCount,0);
    assert.throws(()=>oldLoader.data,/previous publication|disposed/);
    assert.throws(()=>oldPacket.length,/previous publication|disposed/);
    assert.equal(fresh.packetInfo.text,'Binary: 50 bytes | NeoFlash π 🦀 | done');
    const live=fresh.textLoader,request=new (host.net().URLRequest)('data/welcome.txt');host.disposeFamily();
    assert.throws(()=>live.load(request),/disposed/);
  } finally {
    host?.disposeFamily();player?.free();
    server.closeAllConnections();await new Promise(done=>server.close(done));
    rmSync(directory,{recursive:true,force:true});
  }
  console.log('Resources/loading: portable text/form/binary, AS3/AS2 callbacks, actual local HTTP POST, cancellation and Restart passed.');
}
