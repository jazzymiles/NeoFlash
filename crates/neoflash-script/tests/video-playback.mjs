// One ordinary publication through real WASM/host. Injected media elements
// observe transport and asynchronous playback ownership, not MP4 decoding.
// Root's browser acceptance supplies the native decoder/rendering evidence.
import assert from 'node:assert/strict';
import { runCargo } from '../../../scripts/test-cargo.cjs';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { createServer } from 'node:http';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

async function until(predicate,label){
  const deadline=Date.now()+5000;
  while(!predicate()&&Date.now()<deadline)await new Promise(done=>setTimeout(done,5));
  assert.ok(predicate(),label);
}
function mediaBoundary(){
  const elements=[],blobs=new Map(),loads=[],draws=[],revoked=[];
  let serial=0;
  function createCanvas(width,height){
    const canvas={width,height};let matrix=[1,0,0,1,0,0];const stack=[];
    const context={canvas,globalAlpha:1,globalCompositeOperation:'source-over',imageSmoothingEnabled:false,
      save(){stack.push([matrix.slice(),this.globalAlpha]);},restore(){[matrix,this.globalAlpha]=stack.pop();},
      getTransform(){const[a,b,c,d,e,f]=matrix;return{a,b,c,d,e,f};},setTransform(...args){matrix=args.length===1?[args[0].a,args[0].b,args[0].c,args[0].d,args[0].e,args[0].f]:args;},
      clearRect(){},drawImage(source,...args){draws.push({canvas,source,args});canvas.frame={source,time:source.currentTime??source.frame?.time};},
      createImageData(w,h){return{width:w,height:h,data:new Uint8ClampedArray(w*h*4)};},putImageData(){},
      getImageData(x,y,w,h){return{width:w,height:h,data:new Uint8ClampedArray(w*h*4)};},
    };
    canvas.getContext=()=>context;return canvas;
  }
  function createVideo(){
    class Media extends EventTarget{
      constructor(){super();this.muted=false;this.volume=1;this.paused=true;this.ended=false;this.readyState=0;this.videoWidth=0;this.videoHeight=0;this.duration=NaN;this._time=0;this._src='';this.loadGeneration=0;this.frames=new Map();this.callbacks=[];this.buffered={length:0,start:()=>0,end:()=>4};}
      addEventListener(type,fn,options){this.callbacks.push([type,fn]);super.addEventListener(type,fn,options);}
      get src(){return this._src;}set src(value){if(value){assert.equal(this.muted,true,'media is muted before any source assignment');assert.equal(this.volume,0);}this._src=value;}
      get currentTime(){return this._time;}set currentTime(value){this._time=value;this.ended=false;this.dispatchEvent(new Event('seeking'));queueMicrotask(()=>{this.dispatchEvent(new Event('seeked'));this.frame();});}
      setAttribute(name,value){if(name==='src')this.src=value;else this[name]=value;}
      removeAttribute(name){if(name==='src')this._src='';else delete this[name];}
      canPlayType(){return'probably';}
      load(){
        const generation=++this.loadGeneration,url=this.src;this.readyState=0;if(!url)return;
        (async()=>{const bytes=blobs.has(url)?Buffer.from(await blobs.get(url).arrayBuffer()):Buffer.from(await(await fetch(url)).arrayBuffer());if(generation!==this.loadGeneration)return;loads.push({element:this,url,bytes});this.videoWidth=320;this.videoHeight=180;this.duration=4;this.buffered={length:1,start:()=>0,end:()=>4};this.readyState=1;this.dispatchEvent(new Event('loadedmetadata'));this.readyState=2;this.dispatchEvent(new Event('loadeddata'));this.dispatchEvent(new Event('canplay'));this.frame();})().catch(error=>{this.error=error;this.dispatchEvent(new Event('error'));});
      }
      async play(){if(this.readyState<2)await new Promise(resolve=>this.addEventListener('loadeddata',resolve,{once:true}));this.paused=false;this.ended=false;queueMicrotask(()=>{this.dispatchEvent(new Event('play'));this.dispatchEvent(new Event('playing'));this.frame();});}
      pause(){const changed=!this.paused;this.paused=true;if(changed)this.dispatchEvent(new Event('pause'));}
      requestVideoFrameCallback(callback){const id=++serial;this.frames.set(id,callback);return id;}
      cancelVideoFrameCallback(id){this.frames.delete(id);}
      frame(){const pending=[...this.frames.values()];this.frames.clear();for(const callback of pending)callback(0,{mediaTime:this._time,presentedFrames:1,width:320,height:180});}
      advance(seconds){if(this.paused)return;this._time=Math.min(4,this._time+seconds);this.dispatchEvent(new Event('timeupdate'));this.frame();if(this._time===4){this.paused=true;this.ended=true;this.dispatchEvent(new Event('ended'));}}
    }
    const media=new Media();elements.push(media);return media;
  }
  return{elements,loads,draws,revoked,createVideo,createCanvas,
    createObjectURL(blob){const url='blob:neoflash-video-'+(++serial);blobs.set(url,blob);return url;},
    revokeObjectURL(url){revoked.push(url);blobs.delete(url);},
  };
}
export async function verifyVideoPlayback(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/video-playback-'));const media=mediaBoundary();let player,host,timelinePlayer,timelineHost,server;let requests=0;
  try{
    const exported=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/video-playback.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    const resource=bundle.project.resources.find(resource=>resource.path==='media/motion.mp4'),original=Buffer.from(resource.data_base64,'base64');
    assert.equal(resource.media_type,'video/mp4');assert.equal(original.subarray(4,8).toString(),'ftyp');
    server=createServer((request,response)=>{requests++;assert.equal(request.url,'/video-fixture.mp4');response.writeHead(200,{'Content-Type':'video/mp4','Content-Length':original.length});response.end(original);});
    await new Promise((done,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',done);});const baseURL=`http://127.0.0.1:${server.address().port}/index.html`;
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,net:{baseURL},video:{...media,isUserGesture:()=>true},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();let scene=host.proxyFor([]);await until(()=>scene.ready&&scene.legacyReady,'both local connections create their stream after success');
    assert.equal(scene.playButton.caption.selectable,false);assert.equal(scene.httpButton.caption.selectable,false);
    assert.equal(scene.stream.soundTransform.volume,0);assert.equal(media.loads.length,0,'startup does not load or autoplay');
    assert.equal(host.program().is(scene.video,'flash.media.Video'),true);assert.equal(host.program().is(scene.stream,'flash.net.NetStream'),true);
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    click('play-button');await until(()=>scene.metadataCount===1&&scene.legacyMetadata===1,'both embedded streams deliver metadata');await until(()=>media.elements.filter(item=>!item.paused).length===2,'both streams reach playing state');
    assert.deepEqual([scene.duration,scene.legacyDuration,scene.video.videoWidth,scene.video.videoHeight],[4,4,320,180]);
    assert.ok(media.loads.every(load=>load.bytes.equals(original)),'original MP4 bytes reach the native media boundary');assert.equal(requests,0);
    const paint=()=>{const snapshot=host.video().prepareSnapshot(JSON.parse(player.runtime_snapshot_json()));const views=snapshot.draws.filter(item=>item.kind.type==='video_frame');const context=media.createCanvas(1000,660).getContext('2d');for(const item of views)host.video().draw(context,item.kind,item.owner_ref);return views;};
    assert.equal(paint().filter(item=>item.kind.alpha_bounds).length,2,'both authoritative Video placements have decoded-frame content');
    assert.ok(media.draws.some(draw=>media.elements.includes(draw.source)),'Video renderer consumes the injected native media surface');
    media.elements.forEach(element=>element.advance(.5));assert.equal(scene.stream.time,.5);assert.equal(scene.legacyStream.time,.5);
    click('pause-button');await until(()=>media.elements.every(item=>item.paused),'pause reaches both dialect adapters');const paused=scene.stream.time;media.elements.forEach(element=>element.advance(.5));assert.equal(scene.stream.time,paused);
    click('seek-button');await until(()=>scene.stream.time===2&&scene.legacyStream.time===2,'seek uses seconds and preserves paused state');assert.ok(media.elements.every(item=>item.paused));
    click('resume-button');await until(()=>media.elements.filter(item=>!item.paused).length===2,'resume uses AS3 resume and AS2 pause(false)');media.elements.forEach(element=>element.advance(2));await until(()=>scene.completed===1&&scene.legacyCompleted===1,'native completion produces one status per stream');
    click('play-button');await until(()=>media.loads.length>=4,'new play replaces media ownership');await until(()=>media.elements.filter(item=>!item.paused).length===2,'replacement play starts');media.elements.forEach(element=>element.advance(.75));
    click('close-button');assert.equal(scene.stream.time,0);assert.equal(scene.legacyStream.time,0);assert.equal(scene.completed,1,'explicit close is not natural completion');
    assert.equal(paint().filter(item=>item.kind.alpha_bounds).length,2,'close retains the last image for both attached views');
    assert.ok(media.draws.some(draw=>draw.source.frame?.time===.75),'retained frame Canvas is consumed after close');
    scene.video.clear();assert.equal(paint().filter(item=>item.kind.alpha_bounds).length,1,'Video.clear blanks only its image without modifying the other view');
    click('http-button');await until(()=>requests>0&&scene.legacyMetadata>=3,'optional HTTP source reaches a real local server');assert.ok(media.loads.at(-1).bytes.equals(original));
    const oldStream=scene.stream,oldVideo=scene.video,oldCallbacks=media.elements.flatMap(element=>element.callbacks.filter(([name])=>name==='ended'||name==='loadedmetadata').map(([,fn])=>fn));
    host.resetProject();scene=host.proxyFor([]);await until(()=>scene.ready&&scene.legacyReady,'Restart establishes fresh local streams');
    for(const callback of oldCallbacks)callback(new Event('ended'));
    assert.deepEqual([scene.plays,scene.legacyPlays,scene.metadataCount,scene.legacyMetadata,scene.completed,scene.legacyCompleted],[0,0,0,0,0,0]);
    assert.notEqual(scene.stream,oldStream);assert.notEqual(scene.video,oldVideo);assert.throws(()=>oldStream.play('media/motion.mp4'),/session|disposed|previous|stale/i);
    assert.ok(media.loads.every(load=>load.bytes.equals(original)),'AS2 plays only the original silent AAC fixture; every src assignment was separately checked muted/zero-volume');assert.ok(media.revoked.length>0,'owned Blob URLs are released');

    // A Finder/import-style authored span has no script-created NetStream. The real published
    // host must drain Rust VideoCommands, bind the full placement path and start its visual
    // decoder browser-muted before user activation.
    const timelineMedia=mediaBoundary(),authoredVideo={id:'timeline-video',name:'timelineVideo',transform:{x:0,y:0,scale_x:1,scale_y:1,rotation:0,alpha:1},kind:{type:'video',width:320,height:180},scripts:[]};
    const timelineProject={format_version:9,name:'Published imported video',width:320,height:180,fps:10,background:[0,0,0,255],root:'root',resources:[resource],symbols:[{id:'root',name:'Scene',kind:'movie_clip',timeline:{frame_count:4,layers:[{id:'video-layer',name:'Video',visible:true,locked:false,keyframes:[{frame:0,label:'',tween:null,elements:[authoredVideo],video:{resource:resource.id,sync:'stream',start_offset_ms:0,loop_video:false,muted:false}},{frame:2,label:'',tween:null,elements:[]}]}],scripts:[]}}]};
    timelinePlayer=new wasm.WebPlayer(JSON.stringify(timelineProject));
    const timelineBundle={...bundle,project:timelineProject,scripts:[],program:null};
    ({host:timelineHost}=createRuntimeHost(root,timelinePlayer,[],{bundle:timelineBundle,video:{...timelineMedia,isUserGesture:()=>false},context:{measureText:line=>({width:line.length*8})}}));
    timelineHost.dispatchScripts();
    await until(()=>timelineMedia.loads.length===1&&!timelineMedia.elements[0].paused,'published authored span starts its native decoder');
    assert.equal(timelineMedia.elements[0].muted,true,'pre-activation authored video remains browser-muted');
    assert.ok(timelineMedia.loads[0].bytes.equals(original),'published authored span receives the original imported MP4 bytes');
    timelinePlayer.tick();timelineHost.dispatchScripts();
    assert.equal(timelineMedia.elements[0].currentTime,.1,'Rust timeline time reaches the native decoder');
    timelinePlayer.set_playing(false);timelineHost.dispatchScripts();assert.equal(timelineMedia.elements[0].paused,true,'transport Pause freezes authored video');
    timelinePlayer.control('[]','goto_and_stop',3);timelineHost.dispatchScripts();assert.ok(timelineMedia.revoked.length>=1,'leaving the span retires its Blob URL');
    timelineHost.resetProject();await until(()=>timelineMedia.loads.length===2,'Restart creates a fresh authored decoder');assert.equal(timelineMedia.elements[1].currentTime,0);
    timelineHost.disposeFamily();timelineHost=null;timelinePlayer.free();timelinePlayer=null;

    host.disposeFamily();assert.throws(()=>scene.stream.time,/session|disposed|previous|stale/i);
  }finally{timelineHost?.disposeFamily();timelinePlayer?.free();host?.disposeFamily();player?.free();if(server)await new Promise(resolve=>server.close(resolve));rmSync(directory,{recursive:true,force:true});}
  console.log('Video playback: ordinary export/actual WASM, AS3 dynamic + AS2 authored Video, embedded bytes + local HTTP transport, silent metadata/play/pause/seek/close, completion and Restart passed with injected media events.');
}
