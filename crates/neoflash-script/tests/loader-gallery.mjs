// One ordinary publication: image resources/bytes, owned content, legacy
// replacement and transport lifecycle. Browser pixels are accepted separately.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';
async function until(predicate,label){
  const deadline=Date.now()+4000;
  while(!predicate()&&Date.now()<deadline)await new Promise(done=>setTimeout(done,5));
  assert.ok(predicate(),label);
}
export async function verifyLoaderGallery(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/loader-gallery-'));
  const fixture=JSON.parse(readFileSync(resolve(root,'examples/loader-gallery.nflash'),'utf8'));
  const encoded=Buffer.from(fixture.resources[1].data_base64,'base64'),held=[],requests=[];
  const server=createServer((request,response)=>{requests.push(request.url);if(request.url==='/held.png'){held.push(response);return;}response.writeHead(200,{'Content-Type':'image/png'});response.end(encoded);});
  let player,host;
  try{
    await new Promise((done,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',done);});
    const endpoint=`http://127.0.0.1:${server.address().port}`;
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/loader-gallery.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,net:{fetch,baseURL:endpoint+'/index.html'},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();const scene=host.proxyFor([]);
    assert.equal(scene.loads,0,'embedded loads remain asynchronous');
    await until(()=>scene.loads===1&&scene.byteLoads===1&&scene.legacyLoads===1&&scene.errors===1,'initial gallery settles');
    assert.deepEqual([scene.order,scene.legacyOrder,scene.cancelled,requests.length],['IC','CI',0,0]);
    const loader=scene.picture,info=loader.contentLoaderInfo,retained=loader.content,data=retained.bitmapData;
    assert.equal(info.loader,loader);assert.equal(info.content,retained);assert.equal(loader.getChildAt(0),retained);
    assert.deepEqual([info.width,info.height,info.contentType,loader.numChildren],[96,64,'image/png',1]);
    assert.equal(host.program().is(loader,'flash.display.Loader'),true);assert.equal(host.program().is(info,'flash.display.LoaderInfo'),true);
    assert.equal(host.program().is(retained,'flash.display.Bitmap'),true);
    assert.equal(data.getPixel32(10,10),0xff29b7a7);assert.equal(scene.bytesPicture.content.bitmapData.getPixel32(10,10),0xffe0974d);
    assert.equal(scene.legacyTarget.numChildren,1,'authored placeholder children are replaced');
    assert.equal(scene.legacyTarget.getChildByName('obsolete-caption'),null);
    assert.equal(scene.legacyTarget.getChildAt(0).bitmapData.getPixel32(10,10),0xff29b7a7);
    assert.throws(()=>loader.removeChild(retained),/Loader|managed|owned/i);
    const advance=()=>{player.click(JSON.stringify(['cycle-button']));host.dispatchScripts(['cycle-button']);};
    advance();await until(()=>scene.loads===2&&scene.byteLoads===2&&scene.legacyLoads===2,'swap settles');
    assert.equal(scene.picture,loader);assert.notEqual(loader.content,retained);assert.equal(retained.parent,null);
    assert.equal(data.getPixel32(10,10),0xff29b7a7,'unloading must not silently dispose retained image pixels');
    assert.equal(loader.content.bitmapData.getPixel32(10,10),0xffe0974d);assert.equal(scene.order,'ICIC');assert.equal(scene.legacyOrder,'CICI');
    const second=loader.content;advance();
    assert.deepEqual([scene.phase,loader.numChildren,loader.content,scene.bytesPicture.content,scene.legacyTarget.numChildren],[2,0,null,null,0]);
    assert.equal(second.parent,null);assert.equal(second.bitmapData.getPixel32(10,10),0xffe0974d);
    scene.addChild(retained);assert.equal(retained.parent,scene);scene.removeChild(retained);
    advance();await until(()=>scene.loads===3&&scene.byteLoads===3&&scene.legacyLoads===3,'restore settles');
    loader.load(new (host.net().URLRequest)(endpoint+'/image.png'));await until(()=>scene.loads===4,'HTTP image load settles');
    assert.equal(loader.content.bitmapData.getPixel32(10,10),0xffe0974d);assert.equal(requests[0],'/image.png');
    // AS2 MovieClip image aliases share the existing Rust loader transaction.
    const movie=(name,...args)=>host.legacy().nativeRef(scene.legacyTarget,name).get()(...args);
    scene.legacyTarget.imageQuery='movie-alias';
    movie('loadMovie',endpoint+'/movie.png','GET');
    await until(()=>movie('getBytesLoaded')===encoded.length&&scene.legacyTarget.numChildren===1,'MovieClip.loadMovie GET with actual image bytes');
    assert.equal(movie('getBytesTotal'),encoded.length);
    assert.ok(requests.some(url=>url.startsWith('/movie.png?')&&url.includes('imageQuery=movie-alias')));
    movie('unloadMovie');assert.equal(scene.legacyTarget.numChildren,0);assert.equal(movie('getBytesLoaded'),0);
    assert.throws(()=>host.legacy().nativeRef(scene,'unloadMovie').get()(),/root/,'image aliases cannot replace the publication root');
    movie('loadMovie',fixture.resources[0].path);
    await until(()=>movie('getBytesLoaded')>0&&scene.legacyTarget.numChildren===1,'MovieClip reload keeps target alive');
    loader.load(new (host.net().URLRequest)(endpoint+'/held.png'));await until(()=>held.length===1,'cancel starts a real pending response');loader.close();held[0].end(encoded);
    loader.load(new (host.net().URLRequest)(endpoint+'/held.png'));await until(()=>held.length===2,'Restart has a pending response');
    assert.equal(scene.loads,4,'closed response cannot publish an image or complete callback');
    const oldInfo=loader.contentLoaderInfo;host.resetProject();held[1].end(encoded);const fresh=host.proxyFor([]);
    await until(()=>fresh.loads===1&&fresh.byteLoads===1&&fresh.legacyLoads===1&&fresh.errors===1,'fresh session loads only its images');
    assert.deepEqual([fresh.order,fresh.legacyOrder,fresh.cancelled],['IC','CI',0]);
    assert.throws(()=>loader.content,/previous|removed|replaced|stale|disposed/i);assert.throws(()=>oldInfo.content,/previous|removed|replaced|stale|disposed/i);
    assert.throws(()=>data.getPixel32(10,10),/previous|disposed/i);
  }finally{host?.disposeFamily();player?.free();server.closeAllConnections();await new Promise(done=>server.close(done));rmSync(directory,{recursive:true,force:true});}
  console.log('Loader gallery: resource/bytes/HTTP PNG, owned content and retained pixels, AS2 replacement/MovieClip aliases with GET variables and real bytes, event order, unload/cancel and Restart passed.');
}
