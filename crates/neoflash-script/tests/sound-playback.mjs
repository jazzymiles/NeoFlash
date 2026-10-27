// One ordinary MP3 publication through real WASM and the shipped host. The
// injected AudioContext observes silent scheduling/lifecycle, not MP3 decoding;
// actual browser decoder and silent output evidence is recorded separately.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRequire } from 'node:module';
import { createRuntimeHost } from './host-integration.mjs';

async function until(predicate,label) {
  const deadline=Date.now()+3000;
  while(!predicate()&&Date.now()<deadline)await new Promise(done=>setTimeout(done,5));
  assert.ok(predicate(),label);
}

export async function verifySoundPlayback(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/sound-playback-'));
  const contexts=[],decoded=[];
  let player,host,gesture=false;
  function createContext() {
    const sources=[],gains=[];
    const context={state:'suspended',sampleRate:22050,currentTime:0,destination:{kind:'destination'},sources,gains,
      resume(){this.state='running';return Promise.resolve();},
      close(){this.state='closed';return Promise.resolve();},
      decodeAudioData(buffer){decoded.push(Buffer.from(buffer));return Promise.resolve({numberOfChannels:1,sampleRate:22050,length:15434,duration:15434/22050});},
      createGain(){const node={kind:'gain',gain:{value:1},connect(){},disconnect(){}};gains.push(node);return node;},
      createStereoPanner(){return {kind:'pan',pan:{value:0},connect(){},disconnect(){}};},
      createBufferSource(){
        const source={buffer:null,loop:false,loopStart:0,loopEnd:0,onended:null,started:false,ended:false,stopAt:Infinity,
          connect(node){assert.equal(node.kind,'gain');assert.equal(node.gain.value,0,'zero gain is installed before any source connection');this.gain=node;},
          disconnect(){},
          start(when=0,offset=0){this.started=true;this.startedAt=Math.max(context.currentTime,when);this.offset=offset;assert.equal(this.gain.gain.value,0);},
          stop(when=0){this.stopAt=Math.max(context.currentTime,when);if(this.stopAt===context.currentTime)queueMicrotask(()=>this.finish());},
          finish(){if(this.ended)return;this.ended=true;this.onended?.();},
        };
        sources.push(source);return source;
      },
      advance(seconds){this.currentTime+=seconds;for(const source of sources){if(!source.started||source.ended)continue;const end=source.loop?source.stopAt:Math.min(source.stopAt,source.startedAt+source.buffer.duration-source.offset);if(this.currentTime>=end)source.finish();}},
    };
    contexts.push(context);return context;
  }
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/sound-playback.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox),resource=bundle.project.resources.find(item=>item.path==='audio/original-pulse.mp3');
    const original=Buffer.from(resource.data_base64,'base64');
    assert.equal(resource.media_type,'audio/mpeg');assert.equal(original.length,4727);
    assert.notEqual(original.subarray(0,4).toString(),'RIFF','the resource is MP3, not a WAV shortcut');
    const inspect=createRequire(import.meta.url)(resolve(root,'web/audio-formats.js')).inspect;
    const profile=inspect(original,48000);
    assert.deepEqual([profile.format,profile.sampleRate,profile.channels],['mp3',22050,1]);
    assert.ok(profile.decodedBytes>=15434/22050*48000*4,'preflight reserves resampled PCM before decoding');
    const wav=Buffer.alloc(76);
    wav.write('RIFF',0);wav.writeUInt32LE(68,4);wav.write('WAVEfmt ',8);wav.writeUInt32LE(16,16);
    wav.writeUInt16LE(1,20);wav.writeUInt16LE(1,22);wav.writeUInt32LE(22050,24);wav.writeUInt32LE(44100,28);
    wav.writeUInt16LE(2,32);wav.writeUInt16LE(16,34);wav.write('data',36);wav.writeUInt32LE(32,40);
    const waveProfile=inspect(wav,48000);
    assert.deepEqual([waveProfile.format,waveProfile.samples,waveProfile.sampleRate,waveProfile.channels],['wav',16,22050,1]);
    const html=readFileSync(resolve(directory,'index.html'),'utf8');
    assert.ok(html.includes('id="audio-toggle"'));assert.ok(html.includes('id="audio-status"'));
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,audio:{createContext,isUserGesture:()=>gesture},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    let scene=host.proxyFor([]);
    await until(()=>scene.ready&&scene.legacyReady,'both sound APIs load and decode asynchronously');
    assert.ok(decoded.length>=1);assert.ok(decoded.every(bytes=>bytes.equals(original)),'the actual embedded MP3 bytes reach the decoder boundary');
    assert.equal(scene.sound.bytesLoaded,original.length);assert.equal(scene.legacySound.getBytesTotal(),original.length);
    assert.equal(scene.volume,0);assert.equal(scene.legacySound.getVolume(),0);
    assert.equal(host.program().is(scene.sound,'flash.media.Sound'),true);
    assert.equal(contexts.flatMap(context=>context.sources).length,0,'loading does not autoplay');
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    click('play-button');
    assert.equal(scene.plays,1);assert.equal(scene.completed,0);assert.equal(scene.legacyCompleted,0);
    assert.equal(contexts.flatMap(context=>context.sources).filter(source=>source.started).length,0,'without a gesture playback waits for the explicit enable action');
    gesture=true;await host.audio().resumeFromGesture();
    await until(()=>contexts.flatMap(context=>context.sources).filter(source=>source.started).length===2,'enable starts both pending silent channels');
    let context=contexts.at(-1);
    assert.equal(host.program().is(scene.channel,'flash.media.SoundChannel'),true);
    assert.equal(host.program().is(scene.channel.soundTransform,'flash.media.SoundTransform'),true);
    assert.deepEqual([scene.channel.soundTransform.volume,scene.channel.soundTransform.pan],[0,0.25]);
    assert.equal(scene.legacySound.getPan(),-25);
    const detached=scene.channel.soundTransform;detached.pan=-0.5;
    assert.equal(scene.channel.soundTransform.pan,0.25,'soundTransform getters return values, not a live mutable channel record');
    scene.channel.soundTransform=new (host.audio().SoundTransform)(0,-0.5);
    context.advance(0.2);assert.ok(scene.channel.position>=199&&scene.channel.position<=201);
    context.advance(0.6);
    await until(()=>scene.completed===1&&scene.legacyCompleted===1,'natural completion reaches both dialect callbacks once');
    assert.equal(scene.soundStatus.text.startsWith('Completed 1'),true);assert.equal(scene.legacyStatus.text.startsWith('Completed 1'),true);
    click('play-button');context.advance(0.1);click('stop-button');
    await new Promise(done=>setTimeout(done,0));context.advance(1);
    assert.deepEqual([scene.plays,scene.stopped,scene.completed,scene.legacyCompleted],[2,1,1,1],'explicit stop suppresses natural completion');
    const cancelled=new (host.audio().Sound)();let cancelledCompleted=0;
    cancelled.addEventListener('complete',()=>cancelledCompleted++);
    cancelled.load(new (host.net().URLRequest)('audio/original-pulse.mp3'));cancelled.close();
    await new Promise(done=>setTimeout(done,0));assert.equal(cancelledCompleted,0);
    click('play-button');const oldSound=scene.sound,oldChannel=scene.channel,oldEnded=context.sources.filter(source=>!source.ended).map(source=>source.onended);
    host.resetProject();scene=host.proxyFor([]);
    await until(()=>scene.ready&&scene.legacyReady,'Restart reloads fresh sound owners');
    for(const ended of oldEnded)ended?.();
    assert.deepEqual([scene.plays,scene.completed,scene.legacyCompleted,scene.volume],[0,0,0,0]);
    assert.notEqual(scene.sound,oldSound);assert.throws(()=>oldSound.play(),/previous|session|disposed/i);assert.throws(()=>oldChannel.position,/previous|session|disposed/i);
    assert.ok(contexts.flatMap(context=>context.sources).every(source=>source.gain.gain.value===0),'every source kept its own output gain at zero');
    host.disposeFamily();assert.throws(()=>scene.sound.length,/disposed|session/i);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  console.log('Sound playback: original MP3 transport, AS3/AS2 owners, silent gesture gating, channel transforms, completion/stop/close and Restart passed with an injected audio scheduler.');
}
