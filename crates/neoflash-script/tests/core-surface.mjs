// AS3 core API surface: every probe in core-surface.json runs in one exported frame
// script through the real compiler, exporter, host proxies and Rust/WASM Player.
// Presence probes back "partial" catalogue reviews; exact constant values back
// "working". Behaviour beyond each probe is covered by the family workflows.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

const helpers=['flash.display.Sprite','flash.display.Shape','flash.display.BitmapData','flash.geom.Point','flash.geom.Rectangle','flash.geom.Matrix','flash.geom.ColorTransform','flash.events.Event','flash.filters.BlurFilter','flash.text.TextFormat','flash.text.TextField','flash.utils.ByteArray'];
function frameScript(probes){
  const imports=[...new Set([...helpers,...probes.map(p=>p.owner)])].filter(owner=>owner.includes('.'));
  return [...imports.map(owner=>`import ${owner};`),'var out:String = ""; var __o:*; var __r:*; var __v:*;',
    ...probes.map((p,i)=>`__r = "unset"; try { ${p.code} } catch (__e:*) { __r = "throw:" + __e; } out += "${i}\\t" + __r + "\\n";`),
    'this.probeOut = out;'].join('\n');
}
function passes(expect,value){
  if(expect==='ok')return value==='ok';
  if(expect==='function')return value==='function';
  if(expect==='throws')return value.startsWith('throw:');
  if(expect==='notundefined')return value!=='undefined'&&value!=='unset'&&!value.startsWith('throw:');
  if(expect.startsWith('eq:'))return value===expect.slice(3);
  if(expect.startsWith('not:'))return value!==expect.slice(4)&&value!=='unset'&&!value.startsWith('throw:');
  throw new Error(`unknown expectation ${expect}`);
}
export async function verifyCoreSurface(root,cargo){
  const fixture=JSON.parse(readFileSync(resolve(root,'crates/neoflash-script/tests/core-surface.json'),'utf8'));
  const probes=[...fixture.probes.map(p=>({...p,label:p.catalogId})),...fixture.controls.map(p=>({...p,label:`control ${p.owner}: ${p.code}`}))];
  const project={format_version:2,name:'Core surface',width:320,height:240,fps:24,background:[255,255,255,255],root:'scene',
    symbols:[{id:'scene',name:'Scene',kind:'movie_clip',timeline:{frame_count:1,layers:[],scripts:[{frame:0,script:{language:'action_script3',source:frameScript(probes)}}]}}],sources:[]};
  const directory=mkdtempSync(resolve(root,'target/core-surface-'));let player,host;
  try{
    const file=resolve(directory,'core-surface.nflash');writeFileSync(file,JSON.stringify(project));
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--',file,'--output',resolve(directory,'out')],{cwd:root,encoding:'utf8',maxBuffer:64*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const out=resolve(directory,'out'),sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(out,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(out,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),
      {bundle,context:{measureText:text=>({width:String(text).length*6,actualBoundingBoxAscent:8,actualBoundingBoxDescent:2})}}));
    host.dispatchScripts();
    const rows=String(host.proxyFor([]).probeOut||'').trim().split('\n').filter(Boolean);
    assert.equal(rows.length,probes.length,`probe output incomplete; last trace ${host.lastTrace()}`);
    const failed=rows.map(row=>{const [index,...rest]=row.split('\t');return [probes[+index],rest.join('\t')];})
      .filter(([probe,value])=>!passes(probe.expect,value)).map(([probe,value])=>`${probe.label}: expected ${probe.expect}, got ${value}`);
    assert.deepEqual(failed,[],`${failed.length} core surface probes failed`);
  }finally{host?.stopTimers();host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
}
