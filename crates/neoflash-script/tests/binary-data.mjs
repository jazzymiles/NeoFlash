// One ordinary AS3 publication, actual WASM and the shipped host. Numeric/UTF
// data and copy semantics are exercised together; browser rendering is separate QA.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyBinaryData(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/binary-data-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/binary-data.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
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
    const scene=host.proxyFor([]),oldPacket=scene.packet;
    assert.equal(scene.packetBytes,50);
    assert.equal(scene.result,'NeoFlash π 🦀 / done');
    assert.equal(scene.status.text,scene.result,'decoded data reaches the actual TextField override');
    assert.equal(scene.measurements.text,'Values: true, -7, -1024, -1234567, 4275878552, 1.25');
    assert.equal(scene.lifecycle.text,'Copy kept destination position 7 | remaining bytes 0');
    assert.equal(host.lastTrace(),'binary ready 50 NeoFlash π 🦀 / done 0 0');
    assert.equal(host.program().is(scene.packet,'flash.utils.ByteArray'),true);
    assert.equal(host.program().is({},'flash.utils.ByteArray'),false);
    assert.deepEqual([scene.packet[0],scene.packet[1],scene.packet[2],scene.packet[3]],[78,70,66,49]);
    const end=scene.decoded.position;
    assert.throws(()=>scene.decoded.readInt(),/available data/);
    assert.equal(scene.decoded.position,end,'failed read keeps the cursor');
    scene.packet.position=0;scene.decoded.position=5;
    const original=[scene.decoded.length,scene.decoded[0],scene.decoded.position];
    assert.throws(()=>scene.packet.readBytes(scene.decoded,16*1024*1024-1,2),/buffer limit/);
    assert.equal(scene.packet.position,0);
    assert.deepEqual([scene.decoded.length,scene.decoded[0],scene.decoded.position],original,'failed transfer is atomic for both buffers');
    const ByteArray=host.binary().ByteArray,overlap=new ByteArray();
    overlap.writeUTFBytes('abcd');overlap.position=1;overlap.writeBytes(overlap,0,3);
    assert.equal(overlap.toString(),'aabc');overlap[7]=0x1ff;
    assert.equal(overlap.position,4);assert.deepEqual([overlap[4],overlap[5],overlap[6],overlap[7]],[0,0,0,255]);
    overlap.clear();overlap.writeShort(2);overlap.writeByte(0xff);overlap.position=0;
    assert.throws(()=>overlap.readUTF(),/available data/);assert.equal(overlap.position,0);
    overlap.position=2;assert.throws(()=>overlap.readUTFBytes(1),/valid UTF-8/);assert.equal(overlap.position,2);
    assert.throws(()=>overlap.writeUTF('\ud800'),/unpaired surrogate/);assert.equal(overlap.position,2);
    assert.throws(()=>overlap.readObject(),/AMF|marker|truncated|available/i);
    assert.equal(overlap.position,2,'failed AMF decoding keeps the cursor');
    assert.throws(()=>overlap.compress(),/not implemented/);
    assert.equal(overlap.objectEncoding,3);
    host.resetProject();
    assert.throws(()=>oldPacket.length,/previous publication|disposed/);
    assert.equal(host.lastTrace(),'binary ready 50 NeoFlash π 🦀 / done 0 0');
    const fresh=host.proxyFor([]).packet;
    assert.notEqual(fresh,oldPacket);assert.equal(fresh.position,50);
    host.disposeFamily();assert.throws(()=>fresh.length,/disposed/);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Binary data: published numeric/UTF packet, copying/indexing, atomic failures, live text, Restart and disposal passed.');
}
