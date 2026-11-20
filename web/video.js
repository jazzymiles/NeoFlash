/* Native browser video decoding with publication-owned stream lifetimes.
 * https://airsdk.dev/reference/actionscript/3.0/flash/media/Video.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/net/NetStream.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/net/NetConnection.html
 * AIR flash.media.Video / flash.net.NetStream / flash.net.NetConnection and
 * Adobe AS2 NetStream references define the public surface. Local connect(null),
 * native HTTP(S) progressive playback and embedded resource Blobs only: no RTMP,
 * streaming server, FLV decoder, DRM, publishing or buffer-time emulation.
 * AS2 Camera attachment is delegated to the separate permission-gated capture host.
 * Decoded size is checked at metadata (the browser may allocate before that).
 * Rust owns display identity/geometry; immutable Canvas captures are bounded
 * render copies, not another editable scene or pixel model. Playback remains
 * gesture-gated; every media element is muted before assigning its source.
 */
(function(global){
  'use strict';
  const UNHANDLED=Symbol('video-unhandled'),MAX_PIXELS=16*1024*1024;
  const identity={multipliers:[1,1,1,1],offsets:[0,0,0,0]};
  const unsupported=name=>{throw new TypeError(`${name} is not supported by the local progressive-video profile.`);};
  const number=(value,name,min,max)=>{if(typeof value!=='number'||!Number.isFinite(value)||value<min||value>max)throw new RangeError(`${name} must be finite from ${min} through ${max}.`);return value;};
  const VideoStatus=function(){throw new TypeError('VideoStatus supplies constants and cannot be instantiated.');};
  Object.assign(VideoStatus,{ACCELERATED:'accelerated',SOFTWARE:'software',UNAVAILABLE:'unavailable'});
  Object.defineProperties(VideoStatus,{__nfIs:{value:()=>false},__nfInitialize:{value:()=>unsupported('VideoStatus native subclassing')},[Symbol.hasInstance]:{value:()=>false}});
  Object.freeze(VideoStatus);
  function create(options={}){
    const {player,net,events=global.NeoFlashEvents,referenceOf,proxyForRef}=options;
    if(!player?.object_video_info||!net?.mediaSource||!events?.create)throw new TypeError('Video requires the shared display, resource and Event services.');
    const connections=new WeakMap(),streams=new WeakMap(),disposedStreams=new WeakSet(),connectionRecords=new Set(),streamRecords=new Set(),bindings=new Map(),timelineControllers=new Map(),frames=new Set(),derived=new Map();
    const statusTypes=new WeakSet(),schedule=options.schedule||(fn=>queueMicrotask(fn));
    const gesture=options.isUserGesture||(()=>global.navigator?.userActivation?.isActive===true);
    let generation=1,disposed=false,resetting=false,serial=0,used=false,unlocked=false,muted=false,errorText='',framePixels=0,ownAudioContext=null,mixerUnsub=null;
    const diagnosticsList=[];
    const hub=events.create({weakTargets:true,alive:value=>{const record=connections.get(value)||streams.get(value);return !disposed&&!record?.disposed&&record?.generation===generation;}});
    function open(){if(disposed)throw new Error('This video service has been disposed.');if(resetting)throw new Error('This video service is resetting.');}
    function state(value,map){open();const record=map.get(value);if(!record)throw new TypeError('Expected a video object from this publication.');if(record.generation!==generation)throw new Error('This video object belongs to a previous publication session.');if(record.disposed)throw new Error('This NetStream has been disposed.');return record;}
    function native(name,...args){try{return player[name](...args);}catch(error){if(error instanceof Error)throw error;throw new Error(typeof error==='string'?error:'Video runtime failed.',{cause:error});}}
    function status(){let pending=0,active=0,loading=0,audible=false;for(const r of streamRecords){if(r.wanted&&!r.playing)pending++;if(r.playing)active++;if(r.media&&!r.metadata)loading++;if(r.wanted&&r.timelineMuted!==true&&r.volume>0&&r.receiveState?.audio!==false)audible=true;}return {state:!used?'idle':errorText?'error':audible&&!unlocked?'awaiting_gesture':muted?'muted':active?'running':loading?'loading':'ready',pending,active,loading,muted,unlocked,requiresGesture:audible,audible,error:errorText,diagnostics:diagnosticsList.slice()};}
    function diagnostics(){open();return diagnosticsList.slice();}
    function timelineSources(){open();return [...timelineControllers.entries()].map(([channel,ctl])=>({channel,resource:ctl.resource,mode:ctl.mode,source:ctl.record.sourceSpec||null,url:ctl.record.url,mediaSrc:ctl.record.media?.src??null,playing:ctl.record.playing,wanted:ctl.record.wanted,time:ctl.record.time,crossOrigin:ctl.record.media?.crossOrigin??null,preload:ctl.record.media?.preload??null}));}
    function validateExternalPath(rawPath){
      if(typeof rawPath!=='string'||!rawPath.trim())throw new TypeError('External video path must be a non-empty relative path.');
      if(/[\x00-\x1f\x7f]/.test(rawPath))throw new TypeError('External video path cannot contain control characters.');
      const trimmed=rawPath.trim();
      if(trimmed.includes('\\'))throw new TypeError('External video path cannot contain backslashes.');
      if(trimmed.startsWith('/')||/^[A-Za-z]:/.test(trimmed))throw new TypeError('External video path must be relative, not an absolute filesystem path.');
      if(/^(?:file|http|https|data|javascript|blob):/i.test(trimmed)||trimmed.includes('://'))throw new TypeError('External video path must be a relative bundle path, not a URL.');
      const firstSlash=trimmed.indexOf('/');
      if((firstSlash===-1?trimmed:trimmed.slice(0,firstSlash)).includes('@'))throw new TypeError('External video path cannot contain embedded URL credentials.');
      const normalized=trimmed.startsWith('./')?trimmed.slice(2):trimmed;
      const segments=normalized.split('/');
      if(!normalized||segments.some(seg=>!seg||seg==='.'||seg==='..'))throw new TypeError('External video path cannot contain empty or parent traversal (..) segments.');
      return normalized;
    }
    function resolveExternalUrl(rawPath){
      const relPath=validateExternalPath(rawPath);
      if(typeof options.resolveExternalPath==='function')return String(options.resolveExternalPath(relPath));
      const base=options.baseUrl||options.bundleBaseUrl||(typeof global.document?.baseURI==='string'&&global.document.baseURI?global.document.baseURI:'');
      if(base){try{return new URL(relPath,base).href;}catch(_){}}
      return relPath;
    }
    function validateProgressiveUrl(rawUrl){
      if(typeof rawUrl!=='string'||!rawUrl.trim())throw new TypeError('Progressive video stream URL must be a non-empty http:// or https:// URL.');
      if(/[\x00-\x1f\x7f]/.test(rawUrl))throw new TypeError('Progressive video stream URL cannot contain control characters.');
      const trimmed=rawUrl.trim();
      if(trimmed.includes('\\'))throw new TypeError('Progressive video stream URL cannot contain backslashes.');
      if(!/^https?:\/\//i.test(trimmed))throw new TypeError('Progressive video stream URL must use http:// or https://.');
      let parsed;
      try{parsed=new URL(trimmed);}catch(_){throw new TypeError('Progressive video stream URL is not a valid URL.');}
      if((parsed.protocol!=='http:'&&parsed.protocol!=='https:')||!parsed.hostname)throw new TypeError('Progressive video stream URL must specify a non-empty host.');
      if(parsed.username||parsed.password||trimmed.slice(trimmed.indexOf('://')+3).split(/[/?#]/)[0].includes('@'))throw new TypeError('Progressive video stream URL cannot contain embedded credentials.');
      if(trimmed.split(/[/?#]/).includes('..')||/%2e%2e/i.test(trimmed))throw new TypeError('Progressive video stream URL cannot contain parent traversal (..) segments.');
      return trimmed;
    }
    function defaultTransformWire(v=1){return {volume:v,pan:0,leftToLeft:v,leftToRight:0,rightToLeft:0,rightToRight:v};}
    function matMul(a,b){
      return [
        a[0]*b[0]+a[2]*b[1],
        a[1]*b[0]+a[3]*b[1],
        a[0]*b[2]+a[2]*b[3],
        a[1]*b[2]+a[3]*b[3],
      ];
    }
    function localStreamTransformPlan(channelTf,targetTf=null){
      if(typeof options.wasm?.as3_sound_transform_plan_json==='function'){
        const res=JSON.parse(options.wasm.as3_sound_transform_plan_json(JSON.stringify(channelTf),targetTf?JSON.stringify(targetTf):'',''));
        res.is_matrix=Boolean(res.isMatrix??res.is_matrix);
        return res;
      }
      const ch=channelTf||defaultTransformWire(1);
      let m=[ch.leftToLeft,ch.rightToLeft,ch.leftToRight,ch.rightToRight];
      if(targetTf)m=matMul(m,[targetTf.leftToLeft,targetTf.rightToLeft,targetTf.leftToRight,targetTf.rightToRight]);
      const gains=m.map(v=>Math.max(0,Math.min(10,Number(v)||0)));
      const [ll,rl,lr,rr]=gains,outVol=Math.max(ll,rr);
      const isPurePan=Math.abs(rl)<=1e-6&&Math.abs(lr)<=1e-6&&(outVol<=1e-6||Math.abs(ll-outVol)<=1e-5||Math.abs(rr-outVol)<=1e-5);
      if(isPurePan&&outVol<=1+1e-6){
        const pan=outVol<=1e-6?0:(Math.abs(ll-rr)<=1e-6?0:(ll>=rr?-(1-rr/outVol):(1-ll/outVol)));
        return {is_matrix:false,volume:Math.max(0,Math.min(1,outVol)),pan:Math.max(-1,Math.min(1,pan)),gains};
      }
      return {is_matrix:true,volume:Math.max(0,Math.min(1,Math.max(ll+rl,lr+rr))),pan:0,gains};
    }
    function nowPerfMs(){const t=global.performance?.now?.();return Number.isFinite(t)&&t>=0?t:Date.now();}
    function recordByteSample(r,bytes,nowMs=nowPerfMs()){
      if(!r)return;
      if(!Array.isArray(r.byteSamples))r.byteSamples=[];
      const b=Math.max(0,Math.trunc(Number(bytes)||0)),t=Number.isFinite(nowMs)?nowMs:nowPerfMs();
      const n=r.byteSamples.length;
      if(n===0){
        r.byteSamples.push([t,b]);
      }else{
        const prev=r.byteSamples[n-1];
        if(t<=prev[0])prev[1]=Math.max(prev[1],b);
        else r.byteSamples.push([t,Math.max(prev[1],b)]);
        while(r.byteSamples.length>2&&t-r.byteSamples[0][0]>2000)r.byteSamples.shift();
        if(r.byteSamples.length>=2){
          const dt=(r.byteSamples[r.byteSamples.length-1][0]-r.byteSamples[0][0])/1000;
          if(dt>=1e-3){
            const rate=Math.max(0,(r.byteSamples[r.byteSamples.length-1][1]-r.byteSamples[0][1])/dt);
            r.maxBytesPerSec=Math.max(Number(r.maxBytesPerSec)||0,rate);
          }
        }
      }
      if(r.receivePolicy){try{streamPolicy(r,{op:'bytes',bytesLoaded:b,now:t});}catch(_){}}
    }
    function recordDataBytes(r,payload,nowMs=nowPerfMs()){
      if(!r)return;
      let addBytes=0;
      try{addBytes=typeof payload==='string'?payload.length:(JSON.stringify(payload??{})?.length||0);}catch(_){addBytes=0;}
      if(addBytes<=0)return;
      const t=Number.isFinite(nowMs)?nowMs:nowPerfMs();
      r.dataByteCount=(Number(r.dataByteCount)||0)+addBytes;
      if(!Array.isArray(r.dataSamples))r.dataSamples=[];
      r.dataSamples.push([t,r.dataByteCount]);
      while(r.dataSamples.length>2&&t-r.dataSamples[0][0]>2000)r.dataSamples.shift();
      if(r.receivePolicy){try{streamPolicy(r,{op:'data_bytes',bytes:addBytes,now:t});}catch(_){}}
    }
    function readPerfResourceTiming(r){
      if(!r?.url||typeof global.performance?.getEntriesByName!=='function')return null;
      try{
        const entries=global.performance.getEntriesByName(r.url);
        if(!entries||!entries.length)return null;
        const e=entries[entries.length-1];
        const bodyBytes=Math.max(0,Number(e.encodedBodySize)||Number(e.decodedBodySize)||Number(e.transferSize)||0);
        const srtt=Number(e.responseStart)>Number(e.requestStart)&&Number(e.requestStart)>0?Math.max(0,Number(e.responseStart)-Number(e.requestStart)):0;
        return {bodyBytes,srtt};
      }catch(_){return null;}
    }
    function updateBufferedBytes(r){
      if(!r||r.isEmbeddedBytes)return;
      const total=typeof r.totalBytes==='number'&&r.totalBytes>0?r.totalBytes:0;
      const perf=readPerfResourceTiming(r);
      if(perf){
        if(perf.srtt>0&&!(r.observedSrttMs>0))r.observedSrttMs=perf.srtt;
        if(perf.bodyBytes>0&&!r.clipPlan?.single_frame&&r.clipPlan?.duration_limit==null){
          r.observedBytes=Math.max(Number(r.observedBytes)||0,total>0?Math.min(total,perf.bodyBytes):perf.bodyBytes);
        }
      }
      if(Number(r.observedBytes)>0){
        const next=total>0?Math.min(total,Math.trunc(r.observedBytes)):Math.trunc(r.observedBytes);
        if(next!==r.encodedBytes){
          r.encodedBytes=Math.max(Number(r.encodedBytes)||0,next);
          recordByteSample(r,r.encodedBytes);
        }
        if(r.media?.ended&&total>0&&!r.clipPlan?.single_frame&&r.clipPlan?.duration_limit==null){
          r.encodedBytes=total;
          recordByteSample(r,total);
        }
        return;
      }
      if(!r.media||total<=0)return;
      if(r.media.ended||(r.clipCompleted&&r.clipPlan?.duration_limit==null&&!r.clipPlan?.single_frame)){
        r.encodedBytes=total;
        recordByteSample(r,total);
        return;
      }
      let frac=0;
      const dur=Number(r.media.duration);
      if(r.media.buffered&&r.media.buffered.length>0&&Number.isFinite(dur)&&dur>0){
        let maxEnd=0;
        for(let i=0;i<r.media.buffered.length;i++){
          try{maxEnd=Math.max(maxEnd,r.media.buffered.end(i));}catch(_){}
        }
        frac=Math.max(0,Math.min(1,maxEnd/dur));
      }else if((r.media.readyState||0)>=4){
        frac=1;
      }else if((r.media.readyState||0)>=2){
        frac=0.25;
      }
      const next=Math.max(Number(r.encodedBytes)||0,Math.min(total,Math.round(total*frac)));
      if(next!==r.encodedBytes){
        r.encodedBytes=next;
        recordByteSample(r,next);
      }
    }
    function currentBytesLoaded(r){updateBufferedBytes(r);return Math.max(0,Math.trunc(Number(r.encodedBytes)||0));}
    function currentBytesTotal(r){updateBufferedBytes(r);return Math.max(0,Math.trunc(Number(r.totalBytes??r.encodedBytes)||0));}
    function computeBufferLength(r){
      if(hasAvi(r))return Math.max(0,(r.duration||0)-(r.time||0));
      const media=r.media;
      if(!media)return 0;
      const t=Math.max(0,Number(media.currentTime)||0),ranges=[];
      for(let i=0;i<(media.buffered?.length||0);i++){
        try{ranges.push([media.buffered.start(i),media.buffered.end(i)]);}catch(_){}
      }
      let ahead=0;
      for(const [s,e] of ranges){
        if(e>s&&t+0.05>=s&&t<=e)ahead=Math.max(ahead,Math.max(0,e-t));
      }
      return ahead;
    }
    function recordFrameTick(r,nowMs){
      if(!r.frameTimes)r.frameTimes=[];
      const t=Number.isFinite(nowMs)&&nowMs>0?nowMs:(global.performance?.now?.()||Date.now());
      if(r.frameTimes.length===0||t>r.frameTimes[r.frameTimes.length-1]){
        r.frameTimes.push(t);
        while(r.frameTimes.length>2&&t-r.frameTimes[0]>1000)r.frameTimes.shift();
      }
    }
    function computeStreamFps(r){
      if(!r||!r.playing||r.receiveState?.video===false||(r.metadata&&r.width===0&&r.height===0))return 0;
      if(r.frameTimes&&r.frameTimes.length>=2){
        const span=r.frameTimes[r.frameTimes.length-1]-r.frameTimes[0];
        if(span>=1e-3)return Math.min(120,((r.frameTimes.length-1)*1000)/span);
      }
      const fallback=Number(r.containerFps)||Number(r.aviFps)||Number(options.getFrameRate?.())||24;
      return fallback>0?fallback:0;
    }
    function changed(){options.onState?.(status());}
    function report(error){errorText=String(error?.message||error);changed();options.onError?.(error);}
    function current(r,media,sequence){return !disposed&&r.generation===generation&&r.media===media&&r.sequence===sequence;}
    // AS2 component observers receive browser facts only. No observer is installed
    // for existing AS3 or ordinary NetStream users, preserving their event path.
    function localStreamTime(r){
      const current=r.media?Number(r.media.currentTime)||0:Number(r.time)||0;
      return !r.metadata&&Number.isFinite(r.pendingSeek)&&r.pendingSeek>current?r.pendingSeek:current;
    }
    function exposedStreamTime(r){return Math.max(0,(Number(r.playlistTimeBase)||0)+localStreamTime(r));}
    function controllerSample(r){return {now:global.performance?.now?.()||0,time:exposedStreamTime(r),width:r.width,height:r.height,duration:r.duration,bytesLoaded:r.url?currentBytesLoaded(r):0,bytesTotal:r.url?currentBytesTotal(r):0};}
    function observeController(r,type,message){if(typeof r.controllerObserver!=='function')return;const sample=controllerSample(r);if(message)sample.message=message;r.controllerObserver(type,sample);}
    function after(epoch){if(!disposed&&generation===epoch)options.afterEvent?.();}
    function repaint(){(options.onFrame||options.afterEvent)?.();}
    class NetStatusEvent extends events.Event{
      constructor(type,bubbles=false,cancelable=false,info=null){super(type,bubbles,cancelable);this.info=info;statusTypes.add(this);}
      clone(){return new NetStatusEvent(this.type,this.bubbles,this.cancelable,this.info);}
      toString(){
        try{return this.formatToString('NetStatusEvent','type','bubbles','cancelable','eventPhase','info');}
        catch(_){return `[NetStatusEvent type="${this.type}" bubbles=${this.bubbles} cancelable=${this.cancelable} eventPhase=${this.eventPhase} info=${String(this.info)}]`;}
      }
    }
    NetStatusEvent.NET_STATUS='netStatus';
    function emit(r,code,level='status',extra={}){
      const epoch=generation,sequence=r.sequence,info={code,level,...extra};
      if(disposed||r.generation!==epoch)return false;
      if(r.timelineOwned)return current(r,r.media,sequence)||(!r.media&&r.generation===generation&&r.sequence===sequence);
      if(r.legacy){const fn=r.receiver.onStatus;if(typeof fn==='function')Reflect.apply(fn,r.receiver,[info]);else if(level==='error'&&typeof options.onSystemStatus==='function')options.onSystemStatus(info);}
      else hub.dispatch(r.receiver,new NetStatusEvent('netStatus',false,false,info));
      after(epoch);return !disposed&&r.generation===generation&&r.sequence===sequence;
    }
    function invokeStreamCallback(r,name,payload){
      if(!r||r.timelineOwned||disposed||r.generation!==generation)return;
      const targets=r.legacy?(r.client&&r.client!==r.receiver?[r.client,r.receiver]:[r.receiver]):[r.client];
      for(const target of targets){
        const fn=target?.[name];
        if(typeof fn==='function'){
          Reflect.apply(fn,target,[payload]);
          after(generation);
          return;
        }
      }
    }
    function checkCuePoints(r,prevTime,curTime){
      if(r.timelineOwned||disposed||r.generation!==generation)return;
      const list=r.cuePoints;
      if(!Array.isArray(list)||list.length===0)return;
      if(!r.firedCues)r.firedCues=new Set();
      const lo=Math.min(prevTime,curTime)-1e-4,hi=Math.max(prevTime,curTime)+1e-4;
      for(let i=0;i<list.length;i++){
        if(r.firedCues.has(i))continue;
        const cue=list[i],ct=Number(cue?.time);
        if(Number.isFinite(ct)&&ct>=lo&&ct<=hi){
          r.firedCues.add(i);
          const cuePayload={name:String(cue.name||''),time:ct,type:String(cue.type||'navigation'),parameters:cue.parameters&&typeof cue.parameters==='object'?{...cue.parameters}:{}};
          recordDataBytes(r,cuePayload);
          invokeStreamCallback(r,'onCuePoint',cuePayload);
          if(disposed||r.generation!==generation)return;
        }
      }
    }
    function later(r,fn){const epoch=generation,sequence=r.sequence;schedule(()=>{if(disposed||epoch!==generation||r.generation!==generation||r.sequence!==sequence)return;try{fn();}catch(error){if(!disposed&&epoch===generation)report(error);}});}
    function noBase(Type,predicate){Object.defineProperties(Type,{__nfIs:{value:predicate},[Symbol.hasInstance]:{value:predicate},__nfInitialize:{configurable:true,value:()=>unsupported(`${Type.name} native subclassing`)}});}
    function canvas(w,h){const surface=options.createCanvas?options.createCanvas(w,h):typeof global.OffscreenCanvas==='function'?new global.OffscreenCanvas(w,h):global.document?.createElement('canvas');if(!surface)throw new Error('Video painting requires Canvas 2D.');surface.width=w;surface.height=h;return surface;}
    function collectFrames(){const keep=new Set();for(const r of streamRecords){if(r.frozen)keep.add(r.frozen);if(r.suppressedFrame)keep.add(r.suppressedFrame);if(r.appendFrame?.canvas)keep.add(r.appendFrame);}for(const b of bindings.values())if(b.frozen)keep.add(b.frozen);for(const f of derived.values())keep.add(f);for(const f of frames)if(!keep.has(f)){framePixels-=f.width*f.height;f.canvas.width=f.canvas.height=0;frames.delete(f);}}
    function frame(w,h){collectFrames();if(frames.size>=32||framePixels+w*h>MAX_PIXELS)throw new RangeError('Retained video frames exceed 16 Mi pixels / 32 surfaces; clear detached video images or restart.');const result={canvas:canvas(w,h),width:w,height:h,id:++serial};frames.add(result);framePixels+=w*h;return result;}
    function fitsDecoderBounds(w,h){
      return Number.isInteger(w)&&Number.isInteger(h)&&w>0&&h>0&&Math.max(w,h)<=1920&&Math.min(w,h)<=1080;
    }
    const decoderBoundsError='Video decoder dimensions must fit 1920×1080 landscape or 1080×1920 portrait.';
    function dimensions(media){const w=media.videoWidth,h=media.videoHeight;if(w===0&&h===0)return[0,0];if(!fitsDecoderBounds(w,h))throw new RangeError(decoderBoundsError);return[w,h];}
    function freeze(r){
      if(r.receiveState?.video===false)return r.suppressedFrame||null;
      if(!r.media&&r.appendFrame)return r.appendFrame;
      if(!r.media||r.media.readyState<2)return r.frozen||null;
      const [w,h]=dimensions(r.media);
      if(w<1||h<1)return r.frozen||null;
      const f=frame(w,h),ctx=f.canvas.getContext('2d');if(!ctx)throw new Error('Video capture requires Canvas 2D.');
      try{ctx.drawImage(r.media,0,0,w,h);return f;}catch(error){f.canvas.width=f.canvas.height=0;frames.delete(f);framePixels-=w*h;throw error;}
    }
    function setAppendFrame(r,w,h,frameIndex=1){
      const width=Math.max(1,Math.min(1920,w|0||320)),height=Math.max(1,Math.min(1080,h|0||240));
      r.width=width;r.height=height;r.metadata=true;r.serial=++serial;
      try{
        const f=frame(width,height),ctx=f.canvas.getContext('2d');
        if(ctx?.fillRect){ctx.fillStyle=`rgb(${(frameIndex*37)&255},${(frameIndex*73)&255},${(frameIndex*113)&255})`;ctx.fillRect(0,0,width,height);}
        r.appendFrame=f;
      }catch(_){
        r.appendFrame={canvas:{width,height},width,height,id:r.serial};
      }
      collectFrames();repaint();changed();
    }
    function hasAvi(r){return Boolean(r&&(r.aviHandle||r.aviBytes));}
    function ensureOwnAudioBus(){
      if(disposed)return null;
      if(ownAudioContext)return {context:ownAudioContext,destination:ownAudioContext.destination};
      const Ctor=global.AudioContext||global.webkitAudioContext;
      if(!options.createAudioContext&&!Ctor)return null;
      try{
        ownAudioContext=options.createAudioContext?options.createAudioContext():new Ctor();
        return ownAudioContext?{context:ownAudioContext,destination:ownAudioContext.destination}:null;
      }catch(_){return null;}
    }
    function ancestorStreamTransform(r,Type){
      if(typeof proxyForRef!=='function'||!Type?.__nfIs)return null;
      let combined=null;
      for(const b of bindings.values()){
        if(b.stream!==r||!b.ref)continue;
        let node=null;
        try{node=proxyForRef(b.ref)?.parent;}catch(_){node=null;}
        let guard=0;
        while(node&&typeof node==='object'&&guard++<64){
          let pst=null;
          try{pst=node.soundTransform;}catch(_){pst=null;}
          if(pst&&Type.__nfIs(pst)){
            const wire=typeof Type.__nfToWire==='function'?Type.__nfToWire(pst):defaultTransformWire(pst.volume);
            if(!combined)combined=wire;
            else{
              const m=matMul([combined.leftToLeft,combined.rightToLeft,combined.leftToRight,combined.rightToRight],[wire.leftToLeft,wire.rightToLeft,wire.leftToRight,wire.rightToRight]);
              combined={volume:Math.max(0,Math.min(1,Math.max(m[0]+m[1],m[2]+m[3]))),pan:0,leftToLeft:m[0],rightToLeft:m[1],leftToRight:m[2],rightToRight:m[3]};
            }
          }
          try{node=node.parent;}catch(_){break;}
        }
        break;
      }
      return combined;
    }
    function disconnectStreamAudio(r){
      for(const node of [r.webAudioSource,r.webAudioGain,r.webAudioPanner,...(r.webAudioMatrixNodes||[])]){
        try{node?.disconnect();}catch(_){}
      }
      r.webAudioSource=null;r.webAudioGain=null;r.webAudioPanner=null;r.webAudioMatrixNodes=null;r.webAudioMatrixGains=null;r.webAudioContext=null;
    }
    function applyStreamWebAudio(r,mediaEl,plan,shouldMute,Type){
      let bus=typeof Type?.__nfStreamBus==='function'?Type.__nfStreamBus():null;
      if(!bus||typeof bus.context?.createMediaElementSource!=='function')bus=ensureOwnAudioBus();
      if(!bus||typeof bus.context?.createMediaElementSource!=='function'||typeof bus.context?.createGain!=='function')return false;
      const ctx=bus.context,dest=ctx.destination;
      if(unlocked&&ctx.state==='suspended'){try{Promise.resolve(ctx.resume?.()).catch(()=>{});}catch(_){}}
      if(!r.webAudioSource){
        try{
          r.webAudioSource=ctx.createMediaElementSource(mediaEl);
          r.webAudioGain=ctx.createGain();
          r.webAudioContext=ctx;
          r.webAudioGain.gain.value=0;
          r.webAudioSource.connect(r.webAudioGain);
        }catch(_){
          disconnectStreamAudio(r);
          return false;
        }
      }
      const canMatrix=typeof ctx.createChannelSplitter==='function'&&typeof ctx.createChannelMerger==='function';
      if(plan.is_matrix&&canMatrix){
        if(!r.webAudioMatrixGains){
          for(const n of r.webAudioMatrixNodes||[])try{n.disconnect();}catch(_){}
          try{r.webAudioGain.disconnect();r.webAudioPanner?.disconnect();}catch(_){}
          r.webAudioPanner=null;
          const splitter=ctx.createChannelSplitter(2),merger=ctx.createChannelMerger(2);
          const gains=[0,1,2,3].map(()=>{const g=ctx.createGain();g.gain.value=0;return g;});
          r.webAudioGain.gain.value=shouldMute?0:1;
          r.webAudioGain.channelCount=2;r.webAudioGain.channelCountMode='explicit';r.webAudioGain.channelInterpretation='speakers';
          r.webAudioGain.connect(splitter);
          for(const [i,inp,out] of [[0,0,0],[1,1,0],[2,0,1],[3,1,1]]){splitter.connect(gains[i],inp);gains[i].connect(merger,0,out);}
          merger.connect(dest);
          r.webAudioMatrixNodes=[splitter,merger,...gains];
          r.webAudioMatrixGains=gains;
        }
        r.webAudioGain.gain.value=shouldMute?0:1;
        for(let i=0;i<4;i++)r.webAudioMatrixGains[i].gain.value=shouldMute?0:plan.gains[i];
      }else{
        if(r.webAudioMatrixNodes){
          for(const n of r.webAudioMatrixNodes)try{n.disconnect();}catch(_){}
          r.webAudioMatrixNodes=null;r.webAudioMatrixGains=null;
          try{r.webAudioGain.disconnect();}catch(_){}
        }
        if(!r.webAudioPanner&&typeof ctx.createStereoPanner==='function'){
          r.webAudioPanner=ctx.createStereoPanner();
          r.webAudioGain.connect(r.webAudioPanner);
          r.webAudioPanner.connect(dest);
        }else if(!r.webAudioPanner){
          try{r.webAudioGain.disconnect();}catch(_){}
          r.webAudioGain.connect(dest);
        }
        r.webAudioGain.gain.value=shouldMute?0:plan.volume;
        if(r.webAudioPanner)r.webAudioPanner.pan.value=plan.pan;
      }
      mediaEl.muted=shouldMute;
      mediaEl.volume=shouldMute?0:1;
      return true;
    }
    function audio(r){
      const Type=options.getSoundTransform?.();
      if(!mixerUnsub&&typeof Type?.__nfSubscribeMixer==='function'){
        mixerUnsub=Type.__nfSubscribeMixer(()=>{if(!disposed)for(const sr of streamRecords)audio(sr);});
      }
      if(!r.soundTransform)r.soundTransform=defaultTransformWire(typeof r.volume==='number'?r.volume:1);
      const targetTf=ancestorStreamTransform(r,Type);
      const plan=typeof Type?.__nfPlan==='function'?Type.__nfPlan(r.soundTransform,targetTf,true):localStreamTransformPlan(r.soundTransform,targetTf);
      r.volume=plan.volume;
      const shouldMute=!unlocked||muted||r.timelineMuted===true||plan.volume===0||r.receiveState?.audio===false;
      const targetMedia=r.media||r.aviAudio;
      const needsWebAudio=Boolean(targetMedia&&(r.webAudioSource||(!shouldMute&&(plan.is_matrix||Math.abs(plan.pan)>1e-6))));
      if(needsWebAudio&&applyStreamWebAudio(r,targetMedia,plan,shouldMute,Type))return;
      if(r.media){r.media.muted=shouldMute;r.media.volume=shouldMute?0:Math.max(0,Math.min(1,plan.volume));}
      if(r.aviAudio){r.aviAudio.muted=shouldMute;r.aviAudio.volume=shouldMute?0:Math.max(0,Math.min(1,plan.volume));}
    }
    function completeCleanup(operations,errors=[]){
      for(const operation of operations)try{operation();}catch(error){errors.push(error);}
      if(errors.length===1)throw errors[0];
      if(errors.length)throw new AggregateError(errors,'Multiple video cleanup operations failed.',{cause:errors[0]});
    }
    function clearClipTimer(r){const timer=r.clipTimer;r.clipTimer=null;if(timer)clearTimeout(timer);}
    function stopMedia(r,retain,keepQueue=false){
      const errors=[];let frozen=null;
      if(retain)try{frozen=freeze(r);}catch(error){errors.push(error);}
      const {media,listeners,frameCallback,objectURL,clipTimer,probeAbort,streamAbort,aviLoad,aviTimer,aviHandle,aviAudio,aviAudioUrl}=r;
      r.sequence++;r.wanted=false;r.playing=false;r.pendingSeek=null;r.bufferFull=false;r.clipCompleted=false;r.clipEndSec=null;
      if(!keepQueue){r.queue=[];r.playlistTimeBase=0;r.playlistClockStarted=false;r.aggregateMessages=false;r.started=false;}
      // Retire ownership before browser cleanup can synchronously dispatch or throw.
      r.media=null;r.objectURL=null;r.listeners=[];r.frameCallback=null;r.clipTimer=null;r.probeAbort=null;r.streamAbort=null;
      r.observedBytes=0;r.observedSrttMs=0;r.byteSamples=[];r.maxBytesPerSec=0;r.dataByteCount=0;r.dataSamples=[];r.isLiveStream=false;
      r.aviLoad=null;r.aviTimer=null;r.aviHandle=0;r.aviBytes=null;r.aviAudio=null;r.aviAudioUrl=null;
      r.metadata=false;r.frozen=frozen;r.appendFrame=null;r.suppressedFrame=null;r.frameTimes=[];r.cuePoints=[];r.firedCues=null;r.containerFps=null;r.time=0;r.lastTime=null;
      const revoke=url=>(options.revokeObjectURL||global.URL.revokeObjectURL.bind(global.URL))(url);
      const stopElement=element=>[
        ()=>element.pause(),()=>{element.muted=true;},()=>element.removeAttribute('src'),()=>element.load?.(),
      ];
      completeCleanup([
        ()=>{if(clipTimer)clearTimeout(clipTimer);},()=>probeAbort?.abort(),()=>streamAbort?.abort(),()=>disconnectStreamAudio(r),
        ()=>aviLoad?.close(),()=>{if(aviTimer)clearInterval(aviTimer);},()=>{if(aviHandle)player.avi_close_stream?.(aviHandle);},
        ...(aviAudio?stopElement(aviAudio):[]),()=>{if(aviAudioUrl)revoke(aviAudioUrl);},
        ...(media?(listeners||[]).map(([type,fn])=>()=>media.removeEventListener(type,fn)):[]),
        ()=>{if(media&&frameCallback!==null)media.cancelVideoFrameCallback?.(frameCallback);},
        ...(media?stopElement(media):[]),()=>{if(objectURL)revoke(objectURL);},
        ()=>{if(r.receivePolicy)streamPolicy(r,{op:'reset'});},collectFrames,changed,
      ],errors);
    }
    function disposeStream(instance){
      if(disposedStreams.has(instance))return;
      const r=state(instance,streams);
      stopMedia(r,false,false);
      for(const binding of bindings.values()){
        if(binding.stream===r){binding.stream=null;binding.frozen=null;binding.revision=++serial;}
      }
      r.receivePolicy?.free();r.receivePolicy=null;r.disposed=true;
      streamRecords.delete(r);disposedStreams.add(instance);
      collectFrames();changed();
    }
    function isAviContainer(bytes){
      return Boolean(bytes&&bytes.length>=12&&bytes[0]===0x52&&bytes[1]===0x49&&bytes[2]===0x46&&bytes[3]===0x46&&bytes[8]===0x41&&bytes[9]===0x56&&bytes[10]===0x49&&bytes[11]===0x20);
    }
    function renderAviAt(r,seconds){
      if(!hasAvi(r))return;
      const ms=Math.max(0,Math.round(seconds*1000));
      let rgba;
      if(r.aviHandle&&typeof player.avi_stream_frame_rgba==='function')rgba=native('avi_stream_frame_rgba',r.aviHandle,ms);
      else if(r.aviBytes&&typeof player.avi_frame_rgba==='function')rgba=native('avi_frame_rgba',r.aviBytes,ms);
      else return;
      const w=r.width,h=r.height;
      if(!r.appendFrame||r.appendFrame.width!==w||r.appendFrame.height!==h)r.appendFrame=frame(w,h);
      const ctx=r.appendFrame.canvas.getContext('2d');
      if(ctx){
        if(typeof ctx.putImageData==='function'&&typeof global.ImageData==='function'){
          ctx.putImageData(new global.ImageData(new Uint8ClampedArray(rgba),w,h),0,0);
        }else if(typeof ctx.createImageData==='function'&&typeof ctx.putImageData==='function'){
          const img=ctx.createImageData(w,h);img.data.set(rgba);ctx.putImageData(img,0,0);
        }
      }
      r.appendFrame.id=++serial;r.serial=r.appendFrame.id;repaint();
    }
    function planPlay(url,extra){
      const stop=url===false,isNull=url===null;
      const start=extra.length>=1&&extra[0]!==undefined?Number(extra[0]):undefined;
      const len=extra.length>=2&&extra[1]!==undefined?Number(extra[1]):undefined;
      const reset=extra.length>=3?extra[2]:undefined;
      if(typeof options.wasm?.netstream_play_plan_json==='function'){
        const req={stop,isNull};
        if(typeof url==='string')req.url=url;
        if(start!==undefined&&Number.isFinite(start))req.start=start;
        if(len!==undefined&&Number.isFinite(len))req.len=len;
        if(reset!==undefined)req.reset=reset;
        return JSON.parse(options.wasm.netstream_play_plan_json(JSON.stringify(req)));
      }
      if(stop)return {action:'stop'};
      if(isNull)return {action:'dataGeneration'};
      if(typeof url!=='string'||!url.trim())return {action:'rejectInvalid',description:'NetStream.play requires a media URL string.'};
      if(/^(?:rtmp|rtmps|rtmpt|rtmpe|rtmpte|rtmfp):/i.test(url.trim())){
        return {action:'rejectRtmp',url:url.trim(),code:'NetStream.Play.StreamNotFound',description:'RTMP and remote live streaming servers are excluded in HTML5; use NetConnection.connect(null) with progressive HTTP(S) or bundled file media.'};
      }
      const rawStart=start!==undefined&&Number.isFinite(start)?start:-2;
      const startSec=rawStart>0?rawStart:0;
      const rawLen=len!==undefined&&Number.isFinite(len)?len:-1;
      const singleFrame=rawLen===0;
      const durLim=rawLen<0?null:(singleFrame?0:rawLen);
      let resetMode='resetAll';
      if(reset===false||reset===0)resetMode='append';
      else if(reset===2)resetMode='appendAndAggregateMessages';
      else if(reset===3)resetMode='resetAllAndAggregateMessages';
      return {action:'playClip',url,start_seconds:startSec,raw_start:rawStart,duration_limit:durLim,single_frame:singleFrame,reset_mode:resetMode};
    }
    function decideSeek(offset,hasMeta,duration){
      const val=Number(offset);
      if(typeof options.wasm?.netstream_seek_decision_json==='function'){
        return JSON.parse(options.wasm.netstream_seek_decision_json(val,Boolean(hasMeta),Number(duration)||0));
      }
      if(!Number.isFinite(val)||val<0)return {decision:'invalidTime',details:0};
      if(!hasMeta)return {decision:'pending',target:val};
      const dur=Number.isFinite(duration)&&duration>=0?duration:0;
      if(val>dur+1e-6)return {decision:'invalidTime',details:dur};
      return {decision:'seekNow',target:Math.min(val,dur)};
    }
    function finishClipOrTransition(r){
      if(r.clipCompleted)return;
      r.clipCompleted=true;
      clearClipTimer(r);
      if(Array.isArray(r.queue)&&r.queue.length>0&&!r.timelineOwned){
        const next=r.queue.shift();
        const agg=Boolean(r.aggregateMessages||next.reset_mode==='appendAndAggregateMessages');
        const prevStarted=r.started,prevBufferFull=r.bufferFull;
        r.playing=false;
        const sequence=r.sequence;
        if(!agg){
          if(r.bufferFull){
            r.bufferFull=false;
            if(!emit(r,'NetStream.Buffer.Empty'))return;
          }
          if(!emit(r,'NetStream.Play.Stop'))return;
        }
        invokeStreamCallback(r,'onPlayStatus',{code:'NetStream.Play.Switch',level:'status',details:next.url,duration:r.duration,bytes:currentBytesLoaded(r)});
        if(disposed||r.generation!==generation||r.sequence!==sequence)return;
        r.aggregateMessages=agg;
        load(r,next.url,null,next,true);
        if(agg){r.started=prevStarted;r.bufferFull=prevBufferFull;}
        return;
      }
      r.wanted=false;r.playing=false;
      if(r.bufferFull){
        r.bufferFull=false;
        if(!emit(r,'NetStream.Buffer.Empty'))return;
      }
      if(!emit(r,'NetStream.Play.Stop'))return;
      invokeStreamCallback(r,'onPlayStatus',{code:'NetStream.Play.Complete',level:'status',duration:r.duration,bytes:currentBytesLoaded(r)});
      observeController(r,'ended');
      changed();
    }
    function completeSingleFrame(r,media,sequence){
      if(!current(r,media,sequence)||r.clipCompleted)return;
      media.pause();
      r.time=Number(media.currentTime)||0;
      r.frozen=freeze(r);
      r.serial=++serial;
      repaint();
      if(!r.started){
        r.started=true;
        if(!emit(r,'NetStream.Play.Start'))return;
      }
      if(!r.bufferFull){
        r.bufferFull=true;
        if(!emit(r,'NetStream.Buffer.Full'))return;
      }
      finishClipOrTransition(r);
    }
    function completeBoundedClip(r,media,sequence){
      if(!current(r,media,sequence)||r.clipCompleted)return;
      clearClipTimer(r);
      const endT=r.clipEndSec!==null?(Number.isFinite(r.duration)&&r.duration>0?Math.min(r.duration,r.clipEndSec):r.clipEndSec):(Number(media.currentTime)||r.time);
      media.pause();
      if(Math.abs((Number(media.currentTime)||0)-endT)>0.02){
        try{r.suppressSeekNotifyOnce=true;media.currentTime=endT;}catch(_){r.suppressSeekNotifyOnce=false;}
      }
      r.time=endT;
      r.frozen=freeze(r);
      r.serial=++serial;
      repaint();
      finishClipOrTransition(r);
    }
    function armClipTimer(r,media,sequence){
      clearClipTimer(r);
      if(!current(r,media,sequence)||r.clipCompleted||r.clipEndSec===null||!r.playing)return;
      const cur=Number(media.currentTime)||0,rem=r.clipEndSec-cur;
      if(rem<=0.012){completeBoundedClip(r,media,sequence);return;}
      const waitMs=Math.max(12,Math.min(80,Math.round(rem*1000)));
      r.clipTimer=setTimeout(()=>{
        r.clipTimer=null;
        if(!current(r,media,sequence)||r.clipCompleted||!r.playing)return;
        const nowSec=Number(media.currentTime)||0;
        checkCuePoints(r,r.time||0,nowSec);
        r.time=nowSec;
        if(r.clipEndSec!==null&&nowSec>=r.clipEndSec-0.012)completeBoundedClip(r,media,sequence);
        else armClipTimer(r,media,sequence);
      },waitMs);
    }
    function startAviPlayback(r){
      if(!hasAvi(r)||!r.wanted||(!unlocked&&!r.timelineOwned))return;
      if(r.aviTimer)return;
      r.playing=true;
      audio(r);
      if(r.aviAudio&&!r.aviAudio.muted){try{if(Math.abs((Number(r.aviAudio.currentTime)||0)-r.time)>0.08)r.aviAudio.currentTime=r.time;Promise.resolve(r.aviAudio.play()).catch(()=>{});}catch(_){}}
      if(!r.started){r.started=true;emit(r,'NetStream.Play.Start');}
      else if(r.pauseRequested){r.pauseRequested=false;emit(r,'NetStream.Unpause.Notify');}
      if(!r.bufferFull){r.bufferFull=true;emit(r,'NetStream.Buffer.Full');}
      observeController(r,'playing');changed();
      const fps=Math.max(1,r.aviFps||24),stepSec=1/fps,seq=r.sequence;
      r.aviTimer=setInterval(()=>{
        if(disposed||r.generation!==generation||r.sequence!==seq||!r.wanted||!hasAvi(r)){if(r.aviTimer){clearInterval(r.aviTimer);r.aviTimer=null;}return;}
        recordFrameTick(r,global.performance?.now?.()||Date.now());
        let next=r.time+stepSec;
        const stopAt=r.clipEndSec!==null&&r.duration>0?Math.min(r.duration,r.clipEndSec):r.duration;
        if(stopAt>0&&next>=stopAt){
          if(r.timelineLoop&&r.clipEndSec===null){next=next%r.duration;if(r.aviAudio){try{r.aviAudio.currentTime=next;}catch(_){}}}
          else{
            r.time=stopAt;renderAviAt(r,r.time);
            clearInterval(r.aviTimer);r.aviTimer=null;
            if(r.aviAudio){try{r.aviAudio.pause();}catch(_){}}
            r.frozen=r.appendFrame;
            finishClipOrTransition(r);
            return;
          }
        }
        checkCuePoints(r,r.time,next);
        r.time=next;renderAviAt(r,r.time);observeController(r,'time');
      },Math.max(16,Math.round(1000/fps)));
    }
    function mountAviBytes(r,bytes,seq){
      if(disposed||r.generation!==generation||r.sequence!==seq)return;
      let info;
      if(typeof player.avi_open_stream==='function'){
        info=JSON.parse(native('avi_open_stream',bytes));
        if(!fitsDecoderBounds(info.width,info.height)){
          try{player.avi_close_stream?.(info.handle);}catch(_){}
          throw new RangeError(decoderBoundsError);
        }
        r.aviHandle=info.handle;r.aviBytes=null;
        if(info.has_audio&&typeof player.avi_stream_audio_wav==='function'&&typeof global.document?.createElement==='function'&&typeof global.URL?.createObjectURL==='function'){
          try{
            const wav=native('avi_stream_audio_wav',r.aviHandle);
            if(wav&&wav.length>16){
              const el=global.document.createElement('audio');
              if(el){
                const blob=new Blob([wav],{type:info.audio_mime||'audio/wav'});
                r.aviAudioUrl=(options.createObjectURL||global.URL.createObjectURL.bind(global.URL))(blob);
                el.muted=true;el.preload='auto';el.loop=Boolean(r.timelineLoop);el.src=r.aviAudioUrl;
                r.aviAudio=el;audio(r);
              }
            }
          }catch(_){}
        }
      }else if(typeof player.avi_inspect_json==='function'){
        info=JSON.parse(native('avi_inspect_json',bytes));
        if(!fitsDecoderBounds(info.width,info.height))throw new RangeError(decoderBoundsError);
        r.aviBytes=bytes;r.aviHandle=0;
      }else{
        throw new TypeError('AVI runtime decoding requires the NeoFlash WASM video module.');
      }
      r.aviFps=Number(info.fps)||24;r.containerFps=r.aviFps;
      r.width=info.width;r.height=info.height;r.duration=Math.max(0,(Number(info.duration_ms)||0)/1000);
      r.metadata=true;r.encodedBytes=bytes.length;r.totalBytes=bytes.length;
      recordByteSample(r,bytes.length);
      const initStart=r.clipPlan?.start_seconds?Math.min(r.duration,Math.max(0,r.clipPlan.start_seconds)):0;
      if(r.clipPlan?.duration_limit>0)r.clipEndSec=Math.min(r.duration,initStart+r.clipPlan.duration_limit);
      r.time=initStart;
      renderAviAt(r,initStart);
      observeController(r,'metadata');observeController(r,'ready');
      const aviMeta={width:r.width,height:r.height,duration:r.duration,framerate:r.aviFps};
      recordDataBytes(r,aviMeta);
      invokeStreamCallback(r,'onMetaData',aviMeta);
      if(disposed||r.generation!==generation||r.sequence!==seq)return;
      if(r.pendingSeek!==null&&!r.suppressInitialSeekNotify){let at=r.pendingSeek;r.pendingSeek=null;if(r.timelineOwned&&r.duration>0)at=r.timelineLoop?at%r.duration:Math.min(at,r.duration);r.time=at;renderAviAt(r,at);}
      r.pendingSeek=null;r.suppressInitialSeekNotify=false;r.pendingInitialSeek=false;
      if(r.clipPlan?.single_frame){
        r.frozen=r.appendFrame;
        if(!r.started){r.started=true;if(!emit(r,'NetStream.Play.Start'))return;}
        if(!r.bufferFull){r.bufferFull=true;if(!emit(r,'NetStream.Buffer.Full'))return;}
        finishClipOrTransition(r);
        return;
      }
      if((!r.timelineOwned||r.timelineEventMode)&&r.wanted)startAviPlayback(r);
      else if(r.timelineOwned&&r.wanted){r.playing=true;audio(r);if(r.aviAudio&&!r.aviAudio.muted){try{r.aviAudio.currentTime=r.time;Promise.resolve(r.aviAudio.play()).catch(()=>{});}catch(_){}}}
      after(generation);changed();
    }
    function playNative(r){
      if(r.clipPlan?.single_frame||r.clipCompleted)return;
      if(hasAvi(r)){
        if(!r.timelineOwned||r.timelineEventMode)startAviPlayback(r);
        else{r.playing=true;audio(r);if(r.aviAudio&&!r.aviAudio.muted&&r.aviAudio.paused){try{r.aviAudio.currentTime=r.time;Promise.resolve(r.aviAudio.play()).catch(()=>{});}catch(_){}}}
        return;
      }
      // Authored timeline video may advance visually before activation because `audio()` keeps
      // its HTMLVideoElement browser-muted. An actual gesture later unmutes only spans whose
      // authored `muted` flag is false. Script-created NetStreams retain the gesture gate.
      if(!r.media||!r.wanted||(!unlocked&&!r.timelineOwned))return;
      const media=r.media,sequence=r.sequence;if(r.legacy){if(r.playing)return;if(!streamPolicy(r,{op:'buffer_gate',...bufferFacts(r)}))return;}audio(r);
      let promise;try{promise=media.play();}catch(error){promise=Promise.reject(error);}
      Promise.resolve(promise).catch(error=>{if(!current(r,media,sequence)||!r.wanted)return;if(error?.name==='AbortError')return;r.playing=false;if(error?.name==='NotAllowedError'){unlocked=false;audio(r);changed();return;}try{emit(r,'NetStream.Play.Failed','error',{description:String(error?.message||error)});}catch(cause){report(cause);}});
    }
    function resumeFromGesture(){open();if(!gesture())return Promise.reject(new Error('Video playback needs an actual user gesture. Use an authored Play control or Enable audio.'));used=true;unlocked=true;muted=false;errorText='';for(const r of streamRecords){audio(r);playNative(r);}changed();return Promise.resolve(true);}
    function setMuted(value){open();if(typeof value!=='boolean')throw new TypeError('Video mute requires a Boolean.');if(!value)return resumeFromGesture();muted=true;for(const r of streamRecords)audio(r);changed();return Promise.resolve(true);}
    function metadata(r,media,sequence){
      if(!current(r,media,sequence))return;
      const [width,height]=dimensions(media);
      const rawDur=Number(media.duration);
      const isLive=rawDur===Infinity||Boolean(r.isLiveStream||r.sourceSpec?.isLive);
      if(!isLive&&(!Number.isFinite(rawDur)||rawDur<0))throw new TypeError('Invalid negative or NaN media duration.');
      r.isLiveStream=isLive;
      r.metadata=true;r.width=width;r.height=height;r.duration=isLive?Infinity:rawDur;r.serial=++serial;
      if(r.clipPlan){
        if(isLive){
          if(r.clipPlan.duration_limit!==null&&r.clipPlan.duration_limit!==undefined&&r.clipPlan.duration_limit>0){
            r.clipEndSec=(Number(media.currentTime)||0)+r.clipPlan.duration_limit;
          }
        }else{
          const clampedStart=Math.min(r.duration,Math.max(0,Number(r.clipPlan.start_seconds)||0));
          if(r.clipPlan.duration_limit!==null&&r.clipPlan.duration_limit!==undefined&&r.clipPlan.duration_limit>0){
            r.clipEndSec=Math.min(r.duration,clampedStart+r.clipPlan.duration_limit);
          }
        }
      }
      if(media.textTracks&&media.textTracks.length>0){
        for(let ti=0;ti<media.textTracks.length;ti++){
          const tr=media.textTracks[ti];
          if(tr.mode==='disabled')tr.mode='hidden';
          const cues=tr.cues;
          if(cues){
            for(let ci=0;ci<cues.length;ci++){
              const c=cues[ci];
              if(c&&Number.isFinite(c.startTime)){
                r.cuePoints.push({name:String(c.id||c.text||`cue_${ci}`),time:c.startTime,type:'navigation',parameters:{text:String(c.text||'')}});
              }
            }
          }
        }
      }
      observeController(r,'metadata');if(!current(r,media,sequence))return;
      const metaPayload={width,height,duration:isLive?0:media.duration,...(isLive?{isLive:true}:{})};
      const fps=Number(r.containerFps)||(width>0&&height>0?(Number(options.getFrameRate?.())||24):0);
      if(fps>0)metaPayload.framerate=fps;
      recordDataBytes(r,metaPayload);
      invokeStreamCallback(r,'onMetaData',metaPayload);
      if(!current(r,media,sequence))return;
      if(r.pendingSeek!==null&&!isLive){
        let at=r.pendingSeek;r.pendingSeek=null;
        if(r.suppressInitialSeekNotify){
          at=Math.min(Math.max(0,at),r.duration);
          r.time=at;
          if(at>0){media.currentTime=at;}
          else{r.suppressInitialSeekNotify=false;r.pendingInitialSeek=false;}
        }else if(r.timelineOwned&&r.duration>0){
          at=r.timelineLoop?at%r.duration:Math.min(at,r.duration);
          seek(r,at);
        }else{
          seek(r,at);
        }
      }else if(isLive){
        r.pendingSeek=null;r.suppressInitialSeekNotify=false;r.pendingInitialSeek=false;
      }
      if(!current(r,media,sequence))return;
      if(!isLive&&!r.clipPlan?.single_frame&&r.clipPlan?.duration_limit>0&&r.duration<=0){
        completeBoundedClip(r,media,sequence);
        return;
      }
      after(generation);changed();
    }
    function load(r,url,sourceSpec=null,clipPlan=null,keepQueue=false){
      // Timeline-owned records carry an explicit deployment source. Script-created
      // NetStreams instead choose bundled-vs-HTTP for every play() call, so a prior
      // URL must not force the next bundled path through the progressive validator.
      const mode=sourceSpec?.mode||'embedded';
      if(typeof url!=='string'||!url)throw new TypeError('NetStream.play requires a media URL string.');
      if(!r.connection.local)throw new Error('Call NetConnection.connect(null) before playing local video.');
      if(/\.(?:flv|f4v|swf|gif)(?:[?#]|$)/i.test(url))throw new TypeError('This browser profile supports native MP4/WebM/audio media, not FLV/F4V/SWF/GIF decoding.');
      const allowedMediaMimes=['video/mp4','video/webm','video/ogg','video/x-msvideo','video/avi','audio/mpeg','audio/mp3','audio/wav','audio/wave','audio/x-wav','audio/ogg','audio/aac','audio/mp4','audio/x-m4a','audio/webm','audio/flac','application/vnd.apple.mpegurl','application/x-mpegurl','audio/mpegurl','application/dash+xml'];
      let resolvedUrl=url,source=null,mediaType=sourceSpec?.mime_type||'video/mp4';
      if(mode==='external'){
        const relPath=validateExternalPath(sourceSpec?.url_or_path||url);
        resolvedUrl=resolveExternalUrl(relPath);
        if(sourceSpec?.mime_type&&!allowedMediaMimes.includes(String(sourceSpec.mime_type).toLowerCase()))throw new TypeError(`External media MIME type ${sourceSpec.mime_type} is unsupported; use a native browser video/audio format.`);
        r.deploymentMode='external';
        r.sourceSpec={mode:'external',url_or_path:relPath,resolvedUrl,mime_type:mediaType};
      }else if(mode==='progressive'){
        resolvedUrl=validateProgressiveUrl(sourceSpec?.url_or_path||url);
        if(sourceSpec?.mime_type&&!allowedMediaMimes.includes(String(sourceSpec.mime_type).toLowerCase()))throw new TypeError(`Progressive media MIME type ${sourceSpec.mime_type} is unsupported; use a native browser video/audio format.`);
        r.deploymentMode='progressive';
        r.sourceSpec={mode:'progressive',url_or_path:resolvedUrl,resolvedUrl,mime_type:mediaType};
      }else{
        source=net.mediaSource(url);
        if(source.kind==='resource'&&source.mediaType&&!allowedMediaMimes.includes(String(source.mediaType||'').toLowerCase()))throw new TypeError(`Bundled media MIME type ${source.mediaType} is unsupported; use a native browser video/audio format.`);
        if(source.kind!=='resource')resolvedUrl=source.url;
        r.deploymentMode=source.kind==='resource'?'embedded':'progressive';
        r.sourceSpec={mode:r.deploymentMode,url_or_path:url,resolvedUrl:source.kind==='resource'?url:source.url,mime_type:source.mediaType||mediaType};
      }
      const continuePlaylist=Boolean(keepQueue&&r.playlistClockStarted);
      const carriedTime=continuePlaylist?exposedStreamTime(r):0;
      stopMedia(r,false,keepQueue);used=true;errorText='';r.wanted=true;
      r.playlistTimeBase=continuePlaylist?carriedTime-Math.max(0,Number(clipPlan?.start_seconds)||0):0;
      r.playlistClockStarted=true;
      r.clipPlan=clipPlan||null;r.clipCompleted=false;r.clipEndSec=null;
      r.suppressInitialSeekNotify=false;r.pendingInitialSeek=false;r.suppressSeekNotifyOnce=false;
      if(clipPlan&&clipPlan.start_seconds>0){
        r.pendingSeek=clipPlan.start_seconds;
        r.suppressInitialSeekNotify=true;
        r.pendingInitialSeek=true;
      }
      if(source?.kind==='resource'){
        r.isEmbeddedBytes=true;
        r.encodedBytes=source.length;
        r.totalBytes=source.length;
        recordByteSample(r,source.length);
        if(source.bytes&&typeof options.wasm?.netstream_inspect_container_json==='function'){
          try{
            const probe=JSON.parse(options.wasm.netstream_inspect_container_json(source.bytes));
            if(probe?.framerate>0)r.containerFps=probe.framerate;
            if(Array.isArray(probe?.cuePoints)&&probe.cuePoints.length>0)r.cuePoints=probe.cuePoints.slice();
          }catch(_){}
        }
      }else{
        r.isEmbeddedBytes=false;
        r.encodedBytes=0;
        r.totalBytes=0;
      }
      r.url=resolvedUrl;
      r.isLiveStream=/\.(?:m3u8|mpd)(?:[?#]|$)/i.test(resolvedUrl)||['application/vnd.apple.mpegurl','application/x-mpegurl','audio/mpegurl','application/dash+xml'].includes(String(r.sourceSpec?.mime_type||'').toLowerCase());
      const isAviResource=source?.kind==='resource'&&(isAviContainer(source.bytes)||['video/x-msvideo','video/avi'].includes(String(source.mediaType||'').toLowerCase()));
      const isAviRemote=source?.kind!=='resource'&&(/\.avi(?:[?#]|$)/i.test(resolvedUrl)||['video/x-msvideo','video/avi'].includes(String(r.sourceSpec?.mime_type||'').toLowerCase()));
      if(isAviResource||isAviRemote){
        const seq=r.sequence;r.started=false;r.width=r.height=0;r.duration=0;
        if(gesture()){unlocked=true;muted=false;}
        if(isAviResource){
          later(r,()=>mountAviBytes(r,source.bytes,seq));
        }else if(typeof net.loadBytes==='function'&&typeof net.URLRequest==='function'){
          r.aviLoad=net.loadBytes(new net.URLRequest(resolvedUrl),{
            onEvent:ev=>{if(ev.type==='ioError'&&r.sequence===seq){r.aviLoad=null;r.wanted=false;r.playing=false;emit(r,'NetStream.Play.StreamNotFound','error',{description:ev.text||'AVI load failed'});changed();}},
            onComplete:bytes=>{r.aviLoad=null;try{mountAviBytes(r,bytes,seq);}catch(err){if(r.sequence===seq){r.wanted=false;r.playing=false;emit(r,'NetStream.Play.FileStructureInvalid','error',{description:String(err?.message||err)});changed();}}}
          });
        }
        changed();
        return;
      }
      const sequence=r.sequence;
      if(source?.kind!=='resource'&&typeof net.probeMediaBytes==='function'){
        const probeStartMs=nowPerfMs();
        r.probeAbort=net.probeMediaBytes(resolvedUrl,meta=>{
          if(disposed||r.generation!==generation||r.sequence!==sequence)return;
          const rtt=Math.max(0,nowPerfMs()-probeStartMs);
          if(rtt>0&&!(r.observedSrttMs>0))r.observedSrttMs=rtt;
          if(meta?.probeBytes)r.probeAbort=null;
          if(meta?.finalUrl)r.url=meta.finalUrl;
          if(typeof meta?.totalBytes==='number'&&meta.totalBytes>0){
            r.totalBytes=meta.totalBytes;
            updateBufferedBytes(r);
            observeController(r,'progress');
          }
          if(meta?.probeBytes&&typeof options.wasm?.netstream_inspect_container_json==='function'){
            try{
              const probe=JSON.parse(options.wasm.netstream_inspect_container_json(meta.probeBytes));
              if(probe?.framerate>0&&!r.containerFps)r.containerFps=probe.framerate;
              if(Array.isArray(probe?.cuePoints)&&probe.cuePoints.length>0&&(!r.cuePoints||r.cuePoints.length===0)){
                r.cuePoints=probe.cuePoints.slice();
                const curSec=r.media?Number(r.media.currentTime)||r.time||0:r.time||0;
                if(r.playing||curSec>0)checkCuePoints(r,0,curSec);
              }
            }catch(_){}
          }
        });
      }
      const progFetch=typeof options.fetch==='function'?options.fetch:(typeof global.fetch==='function'&&(!options.createVideo||options.progressiveStream===true)?global.fetch:null);
      if(source?.kind!=='resource'&&!r.clipPlan?.single_frame&&r.clipPlan?.duration_limit==null&&!r.isLiveStream&&progFetch){
        const streamCtl=typeof AbortController==='function'?new AbortController():null;
        r.streamAbort=streamCtl;
        const reqStartMs=nowPerfMs();
        Promise.resolve().then(()=>progFetch(resolvedUrl,streamCtl?{signal:streamCtl.signal,credentials:'same-origin'}:{credentials:'same-origin'})).then(async resp=>{
          if(disposed||r.generation!==generation||r.sequence!==sequence)return;
          const rtt=Math.max(0,nowPerfMs()-reqStartMs);
          if(rtt>0&&!(r.observedSrttMs>0))r.observedSrttMs=rtt;
          if(!resp||!resp.ok)return;
          if(resp.url)r.url=resp.url;
          const cl=Number(resp.headers?.get?.('content-length')||0);
          if(cl>0&&!(r.totalBytes>0))r.totalBytes=cl;
          const reader=resp.body?.getReader?.();
          if(!reader){
            const buf=await resp.arrayBuffer();
            if(disposed||r.generation!==generation||r.sequence!==sequence)return;
            r.observedBytes=Math.max(Number(r.observedBytes)||0,buf.byteLength);
            if(!(r.totalBytes>0)&&buf.byteLength>0)r.totalBytes=buf.byteLength;
            updateBufferedBytes(r);
            observeController(r,'progress');
            return;
          }
          let received=0;
          for(;;){
            const {done,value}=await reader.read();
            if(disposed||r.generation!==generation||r.sequence!==sequence){try{await reader.cancel();}catch(_){}return;}
            if(done)break;
            if(value&&value.byteLength>0){
              received+=value.byteLength;
              r.observedBytes=Math.max(Number(r.observedBytes)||0,received);
              updateBufferedBytes(r);
              observeController(r,'progress');
            }
          }
          if(r.streamAbort===streamCtl)r.streamAbort=null;
          if(received>0&&!(r.totalBytes>0)){r.totalBytes=received;updateBufferedBytes(r);}
        }).catch(()=>{if(r.streamAbort===streamCtl)r.streamAbort=null;});
      }
      const media=options.createVideo?options.createVideo():global.document?.createElement('video');if(!media)throw new Error('HTMLVideoElement is unavailable in this environment.');
      r.media=media;media.muted=true;media.volume=0;media.crossOrigin=options.crossOrigin!==undefined?options.crossOrigin:'anonymous';media.preload='auto';media.playsInline=true;media.loop=false;
      function listen(type,fn){
        const callback=()=>{
          if(!current(r,media,sequence))return;
          try{fn();}catch(error){
            if(!current(r,media,sequence))return;
            let cleanupFailed=false,cleanupError;
            try{stopMedia(r,false);}catch(secondary){cleanupFailed=true;cleanupError=secondary;}
            try{report(error);}finally{
              // The callback failure owns the visible error; cleanup diagnostics
              // must not replace it, including when the observer itself throws.
              if(cleanupFailed)try{options.onError?.(cleanupError);}catch(_){}
            }
          }
        };
        media.addEventListener(type,callback);r.listeners.push([type,callback]);
      }
      listen('loadedmetadata',()=>metadata(r,media,sequence));
      listen('loadeddata',()=>{
        r.serial=++serial;updateBufferedBytes(r);observeController(r,'ready');repaint();
        if(!current(r,media,sequence))return;
        if(r.clipPlan?.single_frame&&!r.pendingInitialSeek){completeSingleFrame(r,media,sequence);return;}
        if(r.legacy)playNative(r);
      });
      listen('canplay',()=>{
        updateBufferedBytes(r);
        if(r.clipPlan?.single_frame){if(!r.pendingInitialSeek&&(media.readyState||0)>=2)completeSingleFrame(r,media,sequence);return;}
        if(r.legacy)playNative(r);
      });
      listen('playing',()=>{
        if(r.clipPlan?.single_frame){completeSingleFrame(r,media,sequence);return;}
        r.playing=true;r.serial=++serial;
        recordFrameTick(r,global.performance?.now?.()||Date.now());
        if(!r.started){r.started=true;if(!emit(r,'NetStream.Play.Start'))return;}
        else if(r.pauseRequested){r.pauseRequested=false;if(!emit(r,'NetStream.Unpause.Notify'))return;}
        if(!r.bufferFull){r.bufferFull=true;if(!emit(r,'NetStream.Buffer.Full'))return;}
        checkCuePoints(r,0,Number(media.currentTime)||0);
        if(r.clipEndSec!==null)armClipTimer(r,media,sequence);
        observeController(r,'playing');changed();
      });
      listen('pause',()=>{r.playing=false;clearClipTimer(r);observeController(r,'paused');changed();});
      listen('timeupdate',()=>{
        const prev=r.time||0,cur=Number(media.currentTime)||0;
        r.serial=++serial;r.time=cur;
        recordFrameTick(r,global.performance?.now?.()||Date.now());
        updateBufferedBytes(r);
        checkCuePoints(r,prev,cur);
        if(!current(r,media,sequence))return;
        observeController(r,'time');repaint();
        if(r.clipEndSec!==null&&!r.clipCompleted&&cur>=r.clipEndSec-0.012){
          completeBoundedClip(r,media,sequence);
        }
      });
      listen('seeked',()=>{
        r.serial=++serial;r.time=Number(media.currentTime)||0;
        if(r.suppressSeekNotifyOnce){r.suppressSeekNotifyOnce=false;repaint();return;}
        const wasInitial=r.suppressInitialSeekNotify;
        r.suppressInitialSeekNotify=false;r.pendingInitialSeek=false;
        if(r.clipPlan?.single_frame){completeSingleFrame(r,media,sequence);return;}
        if(!wasInitial){if(!emit(r,'NetStream.Seek.Notify'))return;observeController(r,'seeked');}
        if(current(r,media,sequence)){
          if(r.clipEndSec!==null&&r.playing)armClipTimer(r,media,sequence);
          if(r.legacy)playNative(r);
        }
      });
      listen('ended',()=>{
        r.time=Number(media.currentTime)||r.duration||0;
        if(r.totalBytes>0)r.encodedBytes=r.totalBytes;
        r.serial=++serial;
        checkCuePoints(r,r.lastTime??0,r.time);
        if(!current(r,media,sequence))return;
        finishClipOrTransition(r);
      });
      listen('error',()=>{
        r.wanted=false;r.playing=false;clearClipTimer(r);
        const code=media.error?.code||0,depMode=r.deploymentMode||'embedded';
        const diagCode=depMode==='external'?(code===3||code===4?'video-external-codec-error':'video-external-missing-or-http-error'):depMode==='progressive'?(code===3||code===4?'video-progressive-codec-error':'video-progressive-http-or-cors-error'):(code===4?'video-codec-unsupported':'video-stream-error');
        const desc=depMode==='external'
          ?(code===3||code===4?`External video asset '${r.sourceSpec?.url_or_path||r.url}' failed codec/container decoding (HTMLMediaError ${code}).`:`External video asset '${r.sourceSpec?.url_or_path||r.url}' failed to load from '${r.url}' (missing file or HTTP error; HTMLMediaError ${code}).`)
          :depMode==='progressive'
          ?(code===3||code===4?`Progressive video stream '${r.url}' failed codec/container decoding (HTMLMediaError ${code}).`:`Progressive video stream '${r.url}' failed HTTP/CORS loading; check URL reachability and Access-Control-Allow-Origin headers (HTMLMediaError ${code}).`)
          :'Native browser video loading/decoding failed; check format and HTTP CORS access.';
        const diag={code:diagCode,mode:depMode,resource:r.resourceId||null,url:r.url,source:r.sourceSpec||null,mediaErrorCode:code,message:desc};
        diagnosticsList.push(diag);
        options.onDiagnostic?.(diag);
        if(r.timelineOwned)errorText=desc;
        emit(r,code===4?'NetStream.Play.FileStructureInvalid':'NetStream.Play.StreamNotFound','error',{description:desc});
        observeController(r,'error',desc);
        changed();
      });
      listen('waiting',()=>{
        if(r.bufferFull){r.bufferFull=false;emit(r,'NetStream.Buffer.Empty');}
        if(r.legacy&&r.wanted){streamPolicy(r,{op:'buffer_wait'});r.playing=false;media.pause();}
        observeController(r,'waiting');
      });
      listen('progress',()=>{updateBufferedBytes(r);observeController(r,'progress');if(current(r,media,sequence)&&!r.clipPlan?.single_frame&&r.legacy)playNative(r);});
      const nextFrame=(now,facts)=>{
        if(!current(r,media,sequence))return;
        try{
          recordFrameTick(r,now);
          if(r.receivePolicy)streamPolicy(r,{op:'frame',now,presented:facts.presentedFrames});
          const cur=Number(media.currentTime)||0;
          checkCuePoints(r,r.time||0,cur);
          r.serial=++serial;observeController(r,'time');repaint();
          if(r.clipEndSec!==null&&!r.clipCompleted&&cur>=r.clipEndSec-0.012){completeBoundedClip(r,media,sequence);return;}
          if(current(r,media,sequence))r.frameCallback=media.requestVideoFrameCallback(nextFrame);
        }catch(error){if(current(r,media,sequence))report(error);}
      };
      if(typeof media.requestVideoFrameCallback==='function')r.frameCallback=media.requestVideoFrameCallback(nextFrame);
      if(source?.kind==='resource'){const blob=new Blob([source.bytes],{type:source.mediaType});r.objectURL=(options.createObjectURL||global.URL.createObjectURL.bind(global.URL))(blob);media.src=r.objectURL;}else media.src=resolvedUrl;
      r.started=false;r.width=r.height=0;r.duration=0;media.load();if(gesture()){unlocked=true;muted=false;}if(!r.timelineOwned&&!r.clipPlan?.single_frame)playNative(r);changed();
    }
    function pause(r){const media=r.media,wasPlaying=r.wanted||r.playing;r.wanted=false;r.pauseRequested=true;r.playing=false;clearClipTimer(r);if(r.aviAudio){try{r.aviAudio.pause();}catch(_){}}if(r.aviTimer||hasAvi(r)){if(r.aviTimer){clearInterval(r.aviTimer);r.aviTimer=null;}if(wasPlaying)later(r,()=>{if(hasAvi(r)&&!r.wanted)emit(r,'NetStream.Pause.Notify');});changed();return;}media?.pause();if(media&&wasPlaying)later(r,()=>{if(r.media===media&&!r.wanted&&media.paused)emit(r,'NetStream.Pause.Notify');});changed();}
    function resume(r){if(r.clipCompleted)return;r.wanted=true;if(gesture()){unlocked=true;muted=false;}playNative(r);if(r.media&&r.playing&&r.clipEndSec!==null)armClipTimer(r,r.media,r.sequence);changed();}
    function seek(r,value){
      if(typeof value!=='number'||Number.isNaN(value)){
        later(r,()=>emit(r,'NetStream.Seek.InvalidTime','error',{details:0,description:'NetStream.seek offset must be a finite non-negative time within stream duration.'}));
        return;
      }
      if(hasAvi(r)){
        const dec=decideSeek(value,r.metadata,r.duration);
        if(dec.decision==='invalidTime'){
          later(r,()=>emit(r,'NetStream.Seek.InvalidTime','error',{details:dec.details,description:'NetStream.seek is out of bounds for this media stream.'}));
          return;
        }
        if(dec.decision==='pending'){r.pendingSeek=dec.target;return;}
        if(r.firedCues&&Array.isArray(r.cuePoints)){for(let i=0;i<r.cuePoints.length;i++)if(Number(r.cuePoints[i]?.time)>=dec.target-1e-4)r.firedCues.delete(i);}
        r.time=dec.target;if(r.aviAudio){try{r.aviAudio.currentTime=dec.target;}catch(_){}}renderAviAt(r,dec.target);later(r,()=>emit(r,'NetStream.Seek.Notify'));return;
      }
      if(!r.media&&!r.wanted)throw new Error('NetStream.seek requires a loaded stream.');
      const dec=decideSeek(value,Boolean(r.media&&r.metadata),r.duration);
      if(dec.decision==='invalidTime'){
        later(r,()=>emit(r,'NetStream.Seek.InvalidTime','error',{details:dec.details,description:'NetStream.seek is out of bounds for this media stream.'}));
        return;
      }
      if(dec.decision==='pending'){r.pendingSeek=dec.target;return;}
      if(r.firedCues&&Array.isArray(r.cuePoints)){for(let i=0;i<r.cuePoints.length;i++)if(Number(r.cuePoints[i]?.time)>=dec.target-1e-4)r.firedCues.delete(i);}
      if(r.clipCompleted&&r.clipEndSec===null)r.clipCompleted=false;
      if(r.legacy){streamPolicy(r,{op:'buffer_wait'});r.playing=false;r.media.pause();}
      r.suppressInitialSeekNotify=false;r.pendingInitialSeek=false;r.suppressSeekNotifyOnce=false;
      r.media.currentTime=dec.target;
    }
    class NetConnection extends events.EventDispatcher{
      constructor(){super();open();const record={receiver:this,generation,sequence:0,local:false,connected:false,uriValue:null,legacy:false};connections.set(this,record);connectionRecords.add(record);Object.assign(this,hub.methods(this));this.client=this;if(new.target===NetConnection)Object.seal(this);}
      connect(command){
        const r=state(this,connections);
        if(command===null){
          r.local=true;r.connected=true;r.uriValue='null';r.sequence++;
          later(r,()=>emit(r,'NetConnection.Connect.Success'));
          return;
        }
        r.local=false;r.connected=false;r.uriValue=typeof command==='string'?command:null;r.sequence++;
        later(r,()=>emit(r,'NetConnection.Connect.Rejected','error',{description:'RTMP and remote streaming servers are excluded in HTML5; use NetConnection.connect(null) for file media playback.'}));
      }
      close(){const r=state(this,connections);const wasConnected=Boolean(r.local||r.connected);r.local=false;r.connected=false;r.sequence++;for(const stream of streamRecords)if(stream.connection===r)stopMedia(stream,true);if(wasConnected)later(r,()=>emit(r,'NetConnection.Connect.Closed'));}
      get connected(){const r=state(this,connections);return Boolean(r.connected);}
      get uri(){const r=state(this,connections);return r.uriValue;}
    }
    class LegacyNetConnection extends NetConnection{
      constructor(){super();connections.get(this).legacy=true;this.onStatus=null;}
      connect(command){
        const r=state(this,connections),remote=options.getRemoting?.();
        if(command===null){remote?.close(this);super.connect(null);return true;}
        if(typeof command==='string'&&/^(?:rtmp|rtmps|rtmpt|rtmpe|rtmpte|rtmfp):/i.test(command.trim())){
          remote?.close(this);
          if(r.local||r.connected){r.local=false;r.connected=false;r.sequence++;}
          r.uriValue=command.trim();r.sequence++;
          later(r,()=>emit(r,'NetConnection.Connect.Rejected','error',{description:'RTMP and live remote streaming servers are excluded in HTML5; use NetConnection.connect(null) for progressive/embedded file media or HTTP(S) AMF remoting.'}));
          return false;
        }
        if(!remote)unsupported('AS2 HTTP remoting service unavailable');
        if(r.local)super.close();
        return remote.connect(this,command);
      }
      close(){state(this,connections);options.getRemoting?.()?.close(this);super.close();}
      call(method,responder=null,...args){state(this,connections);const remote=options.getRemoting?.();if(!remote)unsupported('AS2 HTTP remoting service unavailable');return remote.call(this,method,responder,args);}
      get isConnected(){const r=state(this,connections);return options.getRemoting?.()?.isConnected(this,r.local)??r.local;}
      get uri(){state(this,connections);return options.getRemoting?.()?.uri(this);}
      get protocol(){const r=state(this,connections),uri=options.getRemoting?.()?.uri(this);if(typeof uri==='string'&&uri){try{return new URL(uri).protocol.replace(/:$/,'');}catch(_){}}return r.local?'':'';}
    }
    let onStreamCreated=null;
    class NetStream extends events.EventDispatcher{
      constructor(connection,peerID='connectToFMS'){super();const nc=state(connection,connections);if(peerID!==undefined&&peerID!==null&&typeof peerID!=='string')throw new TypeError('NetStream peerID requires a String.');if(streamRecords.size>=64)throw new RangeError('Video is limited to 64 NetStream instances per publication session.');const r={receiver:this,generation,connection:nc,peerID:peerID||'connectToFMS',sequence:0,legacy:false,client:this,media:null,objectURL:null,listeners:[],frameCallback:null,frozen:null,serial:++serial,metadata:false,wanted:false,playing:false,started:false,pauseRequested:false,pendingSeek:null,width:0,height:0,duration:0,time:0,playlistTimeBase:0,playlistClockStarted:false,lastTime:null,volume:1,soundTransform:defaultTransformWire(1),bufferTimeVal:0.1,checkPolicyFileVal:false,bufferFull:false,clipPlan:null,clipCompleted:false,clipEndSec:null,clipTimer:null,queue:[],frameTimes:[],cuePoints:[],firedCues:null,containerFps:null,encodedBytes:0,totalBytes:0,isEmbeddedBytes:false,probeAbort:null,streamAbort:null,observedBytes:0,observedSrttMs:0,byteSamples:[],maxBytesPerSec:0,dataByteCount:0,dataSamples:[],isLiveStream:false,aggregateMessages:false,disposed:false};streams.set(this,r);streamRecords.add(r);Object.assign(this,hub.methods(this));onStreamCreated?.(this,r);if(new.target===NetStream)Object.seal(this);}
      play(url,...extra){
        const r=state(this,streams);
        const plan=planPlay(url,extra);
        if(plan.action==='stop'){
          const wasActive=Boolean(r.started||r.playing||r.wanted);
          stopMedia(r,true,false);
          if(wasActive)later(r,()=>emit(r,'NetStream.Play.Stop'));
          return;
        }
        if(plan.action==='rejectInvalid')throw new TypeError(plan.description||'NetStream.play requires a media URL string.');
        if(plan.action==='rejectRtmp'){
          stopMedia(r,false,false);
          r.sequence++;
          later(r,()=>emit(r,plan.code||'NetStream.Play.StreamNotFound','error',{details:plan.url,description:plan.description}));
          return;
        }
        if(plan.action==='dataGeneration'){
          stopMedia(r,false,false);
          return;
        }
        if(!r.connection.local)throw new Error('Call NetConnection.connect(null) before playing local video.');
        const isBusy=Boolean((r.wanted||r.playing||(r.media&&!r.media.ended&&!r.clipCompleted))&&!r.clipCompleted);
        if(plan.reset_mode==='append'||plan.reset_mode==='appendAndAggregateMessages'){
          if(plan.reset_mode==='appendAndAggregateMessages')r.aggregateMessages=true;
          if(isBusy){
            if(!Array.isArray(r.queue))r.queue=[];
            r.queue.push(plan);
            return;
          }
          if(plan.reset_mode==='append')r.aggregateMessages=false;
          load(r,plan.url,null,plan,true);
          return;
        }
        const aggReset=plan.reset_mode==='resetAllAndAggregateMessages';
        const prevStarted=Boolean(isBusy&&r.started),prevBufferFull=Boolean(isBusy&&r.bufferFull);
        if(isBusy)emit(r,'NetStream.Play.Reset','status',{details:plan.url});
        load(r,plan.url,null,plan,false);
        r.aggregateMessages=aggReset;
        if(aggReset&&prevStarted){r.started=true;r.bufferFull=prevBufferFull;}
      }
      pause(){pause(state(this,streams));}resume(){resume(state(this,streams));}togglePause(){const r=state(this,streams);if(r.wanted||r.playing)pause(r);else resume(r);}seek(seconds){seek(state(this,streams),seconds);}
      step(frames){
        const r=state(this,streams),n=Math.trunc(Number(frames)||0),fps=Math.max(1,Number(options.getFrameRate?.())||24);
        const cur=r.media?Number(r.media.currentTime)||0:r.time;
        const maxDur=r.metadata&&r.duration>0?r.duration:86400*365;
        const next=Math.max(0,Math.min(maxDur,cur+n/fps));
        r.time=next;if(r.media)r.media.currentTime=next;r.serial=++serial;
        later(r,()=>emit(r,'NetStream.Step.Notify'));repaint();
      }
      syncToTimeline(frame,fps=24){
        const r=state(this,streams),f=Math.max(0,Number(frame)||0),rate=Math.max(1,Number(fps)||Number(options.getFrameRate?.())||24);
        const targetSec=f/rate;
        const clamped=r.metadata&&r.duration>0?Math.min(r.duration,targetSec):targetSec;
        r.time=clamped;
        if(r.media){if(Math.abs((Number(r.media.currentTime)||0)-clamped)>1e-4)r.media.currentTime=clamped;}
        r.serial=++serial;repaint();
        return clamped;
      }
      close(){stopMedia(state(this,streams),true,false);}
      dispose(){disposeStream(this);}
      get time(){return exposedStreamTime(state(this,streams));}
      get client(){return state(this,streams).client;}set client(value){if(value===null||(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('NetStream.client requires an object.');state(this,streams).client=value;}
      get soundTransform(){
        const r=state(this,streams),Type=options.getSoundTransform?.();
        if(!Type)throw new Error('SoundTransform service is unavailable.');
        if(typeof Type.__nfFromWire==='function')return Type.__nfFromWire(r.soundTransform||defaultTransformWire(r.volume));
        return new Type(r.volume,r.soundTransform?.pan||0);
      }
      set soundTransform(value){
        const r=state(this,streams),Type=options.getSoundTransform?.();
        if(!Type?.__nfIs?.(value))throw new TypeError('NetStream.soundTransform requires SoundTransform.');
        if(typeof Type.__nfToWire==='function'){
          r.soundTransform=Type.__nfToWire(value);
        }else{
          const volume=number(value.volume,'Video volume',0,10);
          const pan=number(value.pan??0,'Video pan',-1,1);
          r.soundTransform={volume,pan,leftToLeft:Number(value.leftToLeft??(pan>0?1-pan:1)),leftToRight:Number(value.leftToRight??0),rightToLeft:Number(value.rightToLeft??0),rightToRight:Number(value.rightToRight??(pan<0?1+pan:1))};
        }
        audio(r);
      }
      get bytesLoaded(){return currentBytesLoaded(state(this,streams));}
      get bytesTotal(){return currentBytesTotal(state(this,streams));}
      get bufferTime(){return state(this,streams).bufferTimeVal;}
      set bufferTime(value){const r=state(this,streams);r.bufferTimeVal=Math.max(0,Number(value)||0);}
      get bufferLength(){return computeBufferLength(state(this,streams));}
      get currentFPS(){return computeStreamFps(state(this,streams));}
      get checkPolicyFile(){return Boolean(state(this,streams).checkPolicyFileVal);}
      set checkPolicyFile(value){state(this,streams).checkPolicyFileVal=Boolean(value);}
    }
    function streamPolicy(r,request){
      if(!r.receivePolicy)throw new Error('AS2 stream controls require the shared Rust WASM media policy.');
      let result;try{result=JSON.parse(r.receivePolicy.execute(JSON.stringify(request)));}catch(error){throw new TypeError(typeof error==='string'?error:String(error?.message||error));}r.receiveState=result;return result.value;
    }
    function bufferFacts(r){const media=r.media,ranges=[];if(media)for(let i=0;i<(media.buffered?.length||0);i++)ranges.push([media.buffered.start(i),media.buffered.end(i)]);return {time:media?.currentTime||0,duration:r.metadata&&Number.isFinite(r.duration)?r.duration:null,ready:(media?.readyState||0)>=2,ranges};}
    function streamDroppedFrames(r){
      const q=r?.media?.getVideoPlaybackQuality?.();
      if(q&&Number.isFinite(q.droppedVideoFrames)&&q.droppedVideoFrames>=0)return Math.trunc(q.droppedVideoFrames);
      if(Number.isFinite(r?.media?.webkitDroppedFrameCount)&&r.media.webkitDroppedFrameCount>=0)return Math.trunc(r.media.webkitDroppedFrameCount);
      return Math.max(0,Math.trunc(Number(r?.droppedFrames)||0));
    }
    function streamInfo(r){
      if(!r)return null;
      const now=nowPerfMs();
      const loaded=currentBytesLoaded(r),total=currentBytesTotal(r),bufLen=computeBufferLength(r);
      const isLive=Boolean(r.isLiveStream||r.duration===Infinity);
      const hasVideo=Boolean(!r.metadata||(r.width>0&&r.height>0));
      const hasAudio=Boolean(r.sourceSpec?.hasAudio!==false);
      const dropped=streamDroppedFrames(r);
      const perf=readPerfResourceTiming(r);
      const srtt=Number(r.observedSrttMs)>0?r.observedSrttMs:(perf?.srtt>0?perf.srtt:0);
      const uri=String(r.connection?.uriValue||'');
      const resourceName=String(r.sourceSpec?.url_or_path||r.url||'');
      if(r.receivePolicy){
        const facts=bufferFacts(r);
        return streamPolicy(r,{op:'info',now,bytesLoaded:loaded,bytesTotal:total,isLive,bufferLength:bufLen,ranges:facts.ranges,duration:Number.isFinite(r.duration)&&r.duration>0?r.duration:null,time:exposedStreamTime(r),hasVideo,hasAudio,droppedFrames:dropped,srtt,uri,resourceName});
      }
      const aud=r.receiveState?.audio!==false&&hasAudio,vid=r.receiveState?.video!==false&&hasVideo;
      let curBps=0;
      if(Array.isArray(r.byteSamples)&&r.byteSamples.length>=2){
        const [t0,b0]=r.byteSamples[0],[t1,b1]=r.byteSamples[r.byteSamples.length-1];
        if(t1>t0&&b1>=b0&&now-t1<=2000)curBps=((b1-b0)*1000)/(t1-t0);
      }
      let dataBps=0;
      if(Array.isArray(r.dataSamples)&&r.dataSamples.length>=2){
        const [t0,b0]=r.dataSamples[0],[t1,b1]=r.dataSamples[r.dataSamples.length-1];
        if(t1>t0&&b1>=b0&&now-t1<=2000)dataBps=((b1-b0)*1000)/(t1-t0);
      }
      const dur=Number.isFinite(r.duration)&&r.duration>0?r.duration:0,tSec=exposedStreamTime(r);
      const playbackBps=dur>0?(total>0?total/dur:loaded/dur):(tSec>0&&loaded>0?loaded/tSec:curBps);
      const maxBps=Math.max(Number(r.maxBytesPerSec)||0,curBps,loaded>0&&!(r.maxBytesPerSec>0)?playbackBps:0);
      const audioShare=aud&&vid?0.125:(aud?1:0),videoShare=aud&&vid?0.875:(vid?1:0);
      const audioBytes=Math.round(loaded*audioShare),videoBytes=vid?Math.max(0,loaded-audioBytes):0;
      const bufBytesTotal=playbackBps>0&&bufLen>0?Math.min(loaded,Math.round(bufLen*playbackBps)):(bufLen>0?loaded:0);
      const audioBufBytes=Math.round(bufBytesTotal*audioShare),videoBufBytes=vid?Math.max(0,bufBytesTotal-audioBufBytes):0;
      const dataBytes=Math.max(0,Math.trunc(Number(r.dataByteCount)||0));
      return {
        byteCount:loaded+dataBytes,audioByteCount:audioBytes,videoByteCount:videoBytes,dataByteCount:dataBytes,
        currentBytesPerSecond:curBps,maxBytesPerSecond:maxBps,playbackBytesPerSecond:playbackBps,
        audioBytesPerSecond:curBps*audioShare,videoBytesPerSecond:curBps*videoShare,dataBytesPerSecond:dataBps,
        audioBufferByteLength:audioBufBytes,videoBufferByteLength:videoBufBytes,dataBufferByteLength:0,
        audioBufferLength:aud?bufLen:0,videoBufferLength:vid?bufLen:0,dataBufferLength:0,
        droppedFrames:dropped,srtt,audioLossRate:0,videoLossRate:0,uri,resourceName,isLive
      };
    }
    class LegacyNetStream extends NetStream{constructor(connection){super(connection);const r=streams.get(this);r.legacy=true;r.checkPolicyFile=false;r.receivePolicy=options.wasm?.ScriptAS2Stream?new options.wasm.ScriptAS2Stream():null;r.receiveState={audio:true,video:true};r.suppressedFrame=null;r.attachedAudio=null;r.attachedCamera=null;r.publishedName=null;r.publishedType=null;this.onStatus=null;this.onMetaData=null;this.onPlayStatus=null;this.onCuePoint=null;this.onTextData=null;this.onXMPData=null;}
      get checkPolicyFile(){return state(this,streams).checkPolicyFile;}
      set checkPolicyFile(value){if(typeof value!=='boolean')throw new TypeError('AS2 NetStream.checkPolicyFile requires a Boolean.');const r=state(this,streams);r.checkPolicyFile=value;r.checkPolicyFileVal=value;}
      get bytesLoaded(){return currentBytesLoaded(state(this,streams));}
      get bytesTotal(){return currentBytesTotal(state(this,streams));}
      receiveAudio(enabled){const r=state(this,streams);streamPolicy(r,{op:'receive',kind:'audio',enabled});audio(r);}
      receiveVideo(enabled){const r=state(this,streams);if(typeof enabled==='number'){if(!Number.isFinite(enabled)||enabled<0)throw new RangeError('AS2 NetStream.receiveVideo FPS must be a finite non-negative number.');enabled=enabled>0;}else if(typeof enabled!=='boolean')throw new TypeError('AS2 NetStream.receiveVideo requires a Boolean or non-negative FPS number.');if(enabled===r.receiveState.video)return;const captured=enabled?null:freeze(r);streamPolicy(r,{op:'receive',kind:'video',enabled});r.suppressedFrame=captured;r.serial=++serial;collectFrames();repaint();}
      get bufferLength(){const r=state(this,streams);return streamPolicy(r,{op:'buffer',...bufferFacts(r)});}
      get bufferTime(){return streamPolicy(state(this,streams),{op:'buffer_time'});}
      setBufferTime(seconds){const r=state(this,streams);streamPolicy(r,{op:'set_buffer_time',seconds});r.bufferTimeVal=Math.max(0,Number(seconds)||0);playNative(r);}
      get liveDelay(){state(this,streams);return 0;}
      get maxPauseBufferTime(){return streamPolicy(state(this,streams),{op:'max_pause_buffer_time'});}
      set maxPauseBufferTime(seconds){streamPolicy(state(this,streams),{op:'set_max_pause_buffer_time',seconds});}
      getInfo(){return streamInfo(state(this,streams));}
      play2(param){
        const r=state(this,streams);
        if(!param||typeof param!=='object'||typeof param.streamName!=='string'||!param.streamName)throw new TypeError('AS2 NetStream.play2 requires an options Object with a non-empty streamName String.');
        const resetArg=(param.transition==='append'||param.transition==='appendAndWait')?false:true;
        this.play(param.streamName,param.start,param.len,resetArg);
      }
      attachAudio(source){
        const r=state(this,streams),capture=options.getCapture?.();
        if(source!==false&&source!==null&&!capture?.isMicrophone?.(source))throw new TypeError('AS2 NetStream.attachAudio requires a Microphone, false or null.');
        r.attachedAudio=source||null;
      }
      attachVideo(source,snapshotMilliseconds=undefined){
        const r=state(this,streams),capture=options.getCapture?.();
        if(source!==false&&source!==null&&!capture?.isCamera?.(source))throw new TypeError('AS2 NetStream.attachVideo requires a Camera, false or null.');
        if(snapshotMilliseconds!==undefined&&(!Number.isInteger(snapshotMilliseconds)||snapshotMilliseconds<-1))throw new RangeError('AS2 NetStream.attachVideo snapshotMilliseconds requires an integer >= -1.');
        r.attachedCamera=source||null;
      }
      publish(name=false,type='live'){
        const r=state(this,streams);
        if(name===false||name===null||name===''){r.publishedName=null;r.publishedType=null;later(r,()=>emit(r,'NetStream.Unpublish.Success'));return;}
        if(typeof name!=='string'||!['live','record','append'].includes(type))throw new TypeError('AS2 NetStream.publish requires a stream name String (or false) and live/record/append type.');
        r.publishedName=name;r.publishedType=type;
        later(r,()=>emit(r,'NetStream.Publish.BadName','error',{description:'HTML5 NetStream plays progressive media and has no RTMP media server for upstream publishing.'}));
      }
      send(handlerName,...args){
        const r=state(this,streams);
        if(typeof handlerName!=='string'||!handlerName||['__proto__','prototype','constructor'].includes(handlerName))throw new TypeError('AS2 NetStream.send requires a valid handler name String.');
        recordDataBytes(r,{handlerName,args});
        later(r,()=>{const fn=r.receiver[handlerName];if(typeof fn==='function')Reflect.apply(fn,r.receiver,args);after(generation);});
      }
      get currentFps(){
        const r=state(this,streams);
        if(r.receiveState?.video===false)return 0;
        if(r.media&&typeof r.media.requestVideoFrameCallback==='function'){
          const fps=streamPolicy(r,{op:'fps',now:global.performance?.now?.()||0,playing:r.playing});
          if(fps>0)return fps;
        }
        return computeStreamFps(r);
      }
      pause(flag){const r=state(this,streams);if(flag===undefined)return super.togglePause();if(typeof flag!=='boolean')throw new TypeError('AS2 NetStream.pause expects a Boolean or no argument.');if(flag)pause(r);else resume(r);}
    }
    for(const[Type,predicate]of [[NetConnection,v=>connections.has(v)],[LegacyNetConnection,v=>connections.has(v)&&connections.get(v).legacy],[NetStream,v=>streams.has(v)],[LegacyNetStream,v=>streams.has(v)&&streams.get(v).legacy],[NetStatusEvent,v=>statusTypes.has(v)]])noBase(Type,predicate);
    for(const name of ['publish','send','appendBytes','appendBytesAction','attachAudio','attachCamera','attachVideo','setBufferTime','receiveAudio','receiveVideo'])Object.defineProperty(NetStream.prototype,name,{configurable:true,writable:true,value(){unsupported('NetStream.'+name);}});
    for(const name of ['objectEncoding','videoStreamSettings','inBufferSeek','backBufferTime','info','farID','peerStreams'])Object.defineProperty(NetStream.prototype,name,{configurable:true,get(){unsupported('NetStream.'+name);},set(){unsupported('NetStream.'+name);}});
    for(const name of ['call','addHeader','addEventListenerRemote'])Object.defineProperty(NetConnection.prototype,name,{configurable:true,writable:true,value(){unsupported('NetConnection.'+name);}});
    function videoRef(target){const value=referenceOf(target);if(!value||!player.object_alive(value.key))throw new Error('Video display object is stale or foreign.');const info=JSON.parse(native('object_video_info',value.key));return {key:value.key,ref:value.objectRef,info};}
    function binding(target){const value=videoRef(target);let b=bindings.get(value.key);if(!b){b={...value,stream:null,capture:null,captureClearId:null,frozen:null,clearSerial:-1,revision:++serial};bindings.set(value.key,b);}return b;}
    function attachRecord(target,next){const b=binding(target);if(next===b.stream&&!b.capture)return;
      if(b.capture){b.frozen=freezeCapture(b);b.capture.release();b.capture=null;b.captureClearId=null;}
      const old=b.stream;b.frozen=old?freeze(old):b.frozen;b.stream=null;
      if(next){for(const other of bindings.values())if(other!==b&&other.stream===next){other.frozen=freeze(next);other.stream=null;other.revision=++serial;}b.stream=next;b.frozen=null;b.clearSerial=-1;audio(next);}
      if(old)audio(old);
      b.revision=++serial;collectFrames();after(generation);
    }
    function attach(target,value){return attachRecord(target,value===null?null:state(value,streams));}
    function freezeCapture(b){const source=b.capture?.read();if(!source||!source.image)return b.frozen;const output=frame(source.width,source.height),ctx=output.canvas.getContext('2d');if(!ctx)throw new Error('Camera retained frame requires Canvas 2D.');ctx.drawImage(source.image,0,0,source.width,source.height);return output;}
    function attachAs3Camera(target,value){
      const b=binding(target);
      if(value===null||value===undefined){
        if(b.capture){b.frozen=freezeCapture(b);b.capture.release();b.capture=null;b.captureClearId=null;}
        b.attachedCamera=null;b.revision=++serial;collectFrames();after(generation);
        return;
      }
      const as3Cam=options.getAs3Camera?.();
      if(as3Cam){
        if(!as3Cam.isCamera(value))throw new TypeError('Video.attachCamera requires a flash.media.Camera or null.');
        const lease=as3Cam.attachVideo(value,repaint);
        if(b.stream){b.frozen=freeze(b.stream);b.stream=null;}
        b.capture?.release();b.capture=lease;b.captureClearId=null;b.frozen=null;b.clearSerial=-1;b.attachedCamera=value;b.revision=++serial;collectFrames();after(generation);
        return;
      }
      const capture=options.getCapture?.();
      if(capture?.isCamera?.(value)){legacyAttachVideo(target,value);return;}
      if(typeof value!=='object')throw new TypeError('Video.attachCamera requires a Camera or null.');
      if(b.stream){b.frozen=freeze(b.stream);b.stream=null;}
      b.attachedCamera=value;b.revision=++serial;collectFrames();after(generation);
    }
    function legacyAttachVideo(target,value){
      const capture=options.getCapture?.();if(!capture?.isCamera(value)){attach(target,value);return;}
      const b=binding(target),lease=capture.attachVideo(value,repaint);
      b.capture?.release();b.capture=lease;b.captureClearId=null;b.stream=null;b.frozen=null;b.clearSerial=-1;b.revision=++serial;collectFrames();after(generation);
    }
    function clear(target){const b=binding(target);b.frozen=null;b.captureClearId=b.capture?.read()?.id??null;b.clearSerial=b.stream?.serial??0;b.revision=++serial;collectFrames();after(generation);}
    function source(b){if(!b)return null;if(b.capture){const f=b.capture.read();return f&&f.id!==b.captureClearId?f:null;}const r=b.stream;if(r){if(r.receiveState?.video===false){const f=r.suppressedFrame;return f&&f.id>b.clearSerial?{...f,image:f.canvas}:null;}if(r.media?.readyState>=2&&r.width>0&&r.height>0&&r.serial>b.clearSerial)return {image:r.media,width:r.width,height:r.height,id:`${r.serial}:${r.media.currentTime}`};if(r.frozen&&r.serial>b.clearSerial)return {...r.frozen,image:r.frozen.canvas};if(!r.media&&r.appendFrame&&r.serial>b.clearSerial)return {...r.appendFrame,image:r.appendFrame.canvas};return null;}return b.frozen?{...b.frozen,image:b.frozen.canvas}:null;}
    function methods(target){return {attachNetStream:value=>attach(target,value),attachVideo:value=>attach(target,value),clear:()=>clear(target),attachCamera:value=>attachAs3Camera(target,value)};}
    function get(target,name){if(name==='smoothing')return videoRef(target).info.smoothing;if(name==='deblocking')return videoRef(target).info.deblocking??0;if(name==='videoWidth'||name==='videoHeight'){const b=binding(target),r=b.stream,f=source(b);return r?(name==='videoWidth'?r.width:r.height):f?(name==='videoWidth'?f.width:f.height):0;}return UNHANDLED;}
    function set(target,name,value){if(name==='smoothing'){if(typeof value!=='boolean')throw new TypeError('Video.smoothing requires a Boolean.');const v=videoRef(target),next={...v.info,smoothing:value};native('object_video_update',v.key,JSON.stringify(next));const b=bindings.get(v.key);if(b)b.info=next;return true;}if(name==='deblocking'){if(!Number.isInteger(value)||value<0||value>5)throw new RangeError('Video.deblocking requires an integer from 0 through 5.');const v=videoRef(target),next={...v.info,deblocking:value};native('object_video_update',v.key,JSON.stringify(next));const b=bindings.get(v.key);if(b)b.info=next;return true;}if(name==='videoWidth'||name==='videoHeight')throw new TypeError(`Video.${name} is read-only.`);return false;}
    class Video{
      constructor(width=320,height=240){open();number(width,'Video width',0,8192);number(height,'Video height',0,8192);if(!Number.isInteger(width)||!Number.isInteger(height))throw new TypeError('Video dimensions require integers.');const target=proxyForRef(JSON.parse(native('create_display','video')));const v=videoRef(target);native('object_video_update',v.key,JSON.stringify({width:width||320,height:height||240,smoothing:false,deblocking:0}));return target;}
    }
    noBase(Video,value=>{try{videoRef(value);return true;}catch(_){return false;}});
    function prepareSnapshot(snapshot){
      for(const[key,b]of bindings)if(!player.object_alive(key)){b.capture?.release();bindings.delete(key);}else if(b.stream&&(b.stream.media||b.stream.aviAudio))audio(b.stream);
      for(const r of streamRecords)if(r.media&&r.media.currentTime!==r.lastTime){r.lastTime=r.media.currentTime;r.serial=++serial;}
      const liveFrames=new Set([...bindings.values()].map(b=>source(b)?.id));for(const[key,f]of derived)if(!liveFrames.has(f.sourceId))derived.delete(key);collectFrames();
      return {...snapshot,draws:snapshot.draws.map(item=>{if(item.kind.type!=='video_frame')return item;const b=bindings.get(JSON.stringify(item.owner_ref)),f=source(b),ct=item.kind.color_transform||identity,alpha=Math.max(ct.offsets[3],255*ct.multipliers[3]+ct.offsets[3]);return {...item,kind:{...item.kind,frame_revision:f?String(f.id):`empty:${b?.revision||0}`,alpha_bounds:f&&alpha>0?{left:0,top:0,right:item.kind.width,bottom:item.kind.height}:null}};})};
    }
    function draw(context,kind,owner){const b=bindings.get(JSON.stringify(owner)),f=source(b);if(!f||kind.width<=0||kind.height<=0)return;const ct=kind.color_transform||identity;let image=f.image;
      if(ct.multipliers.some(v=>v!==1)||ct.offsets.some(v=>v!==0)){
        const key=JSON.stringify([f.id,ct]),old=derived.get(key);if(old)image=old.canvas;else{const output=frame(f.width,f.height),ctx=output.canvas.getContext('2d');if(!ctx)throw new Error('Video color transform requires Canvas 2D.');ctx.drawImage(image,0,0,f.width,f.height);const data=ctx.getImageData(0,0,f.width,f.height),matrix=[];for(let row=0;row<4;row++){for(let col=0;col<4;col++)matrix.push(row===col?ct.multipliers[row]:0);matrix.push(ct.offsets[row]);}const bytes=native('filter_rgba',f.width,f.height,data.data,JSON.stringify([{type:'color_matrix',matrix}]));data.data.set(bytes);ctx.putImageData(data,0,0);output.sourceId=f.id;derived.set(key,output);image=output.canvas;}
      }
      context.save();try{const deblocking=Number(kind.deblocking??b?.info?.deblocking??0);context.imageSmoothingEnabled=Boolean(kind.smoothing||deblocking>0);if(deblocking>0){context.imageSmoothingQuality=deblocking>=4?'high':deblocking>=2?'medium':'low';if(deblocking>=2)context.filter=`blur(${((deblocking-1)*0.25).toFixed(2)}px)`;}context.drawImage(image,0,0,kind.width,kind.height);}finally{context.restore();}
    }
    function timelineTarget(path){const ref=JSON.parse(native('object_ref',JSON.stringify(path)));if(!ref)throw new Error('Authored timeline Video placement is no longer active.');return proxyForRef(ref);}
    function timelinePath(resource){const path=options.resourcePath?.(resource);if(typeof path!=='string'||!path)throw new Error(`Timeline video resource '${resource}' is missing from the publication.`);return path;}
    function resolveTimelineSource(command){
      if(command.source&&typeof command.source==='object'&&typeof command.source.mode==='string'){
        const spec=command.source;
        if(spec.mode==='embedded'){
          const customPath=options.resourcePath?.(command.resource);
          return {mode:'embedded',url_or_path:typeof customPath==='string'&&customPath?customPath:spec.url_or_path,mime_type:spec.mime_type||'video/mp4'};
        }
        return {mode:spec.mode,url_or_path:spec.url_or_path,mime_type:spec.mime_type||'video/mp4'};
      }
      if(typeof player?.video_source_spec_json==='function'){
        try{
          const spec=JSON.parse(player.video_source_spec_json(command.resource));
          if(spec&&typeof spec.mode==='string'){
            if(spec.mode==='embedded'){
              const customPath=options.resourcePath?.(command.resource);
              return {mode:'embedded',url_or_path:typeof customPath==='string'&&customPath?customPath:spec.url_or_path,mime_type:spec.mime_type||'video/mp4'};
            }
            return spec;
          }
        }catch(_){}
      }
      return {mode:'embedded',url_or_path:timelinePath(command.resource),mime_type:'video/mp4'};
    }
    function retireTimeline(channel,retain=true){const ctl=timelineControllers.get(channel);if(!ctl)return;timelineControllers.delete(channel);const b=bindings.get(ctl.targetKey);if(b?.stream===ctl.record){b.frozen=retain?freeze(ctl.record):null;b.stream=null;b.revision=++serial;}stopMedia(ctl.record,retain);streamRecords.delete(ctl.record);collectFrames();}
    function timelineRecord(command){
      if(streamRecords.size>=64)throw new RangeError('Video is limited to 64 NetStream instances per publication session.');
      const target=timelineTarget(command.target_path),targetKey=referenceOf(target)?.key;if(!targetKey)throw new Error('Authored timeline Video placement has no stable display reference.');
      const sourceSpec=resolveTimelineSource(command);
      const connection={generation,sequence:0,local:true,legacy:false};
      const record={receiver:null,generation,connection,peerID:'timeline',sequence:0,legacy:false,timelineOwned:true,resourceId:command.resource,deploymentMode:sourceSpec.mode,sourceSpec,timelineMuted:Boolean(command.muted),timelineLoop:Boolean(command.loop_video),client:null,media:null,objectURL:null,listeners:[],frameCallback:null,frozen:null,serial:++serial,metadata:false,wanted:false,playing:false,started:false,pauseRequested:false,pendingSeek:null,width:0,height:0,duration:0,time:0,lastTime:null,volume:1,soundTransform:defaultTransformWire(1),queue:[],frameTimes:[],cuePoints:[],firedCues:null,containerFps:null,encodedBytes:0,totalBytes:0,isEmbeddedBytes:false,probeAbort:null,streamAbort:null,observedBytes:0,observedSrttMs:0,byteSamples:[],maxBytesPerSec:0,dataByteCount:0,dataSamples:[],isLiveStream:false,aggregateMessages:false};
      streamRecords.add(record);attachRecord(target,record);load(record,sourceSpec.url_or_path,sourceSpec);record.timelineMuted=Boolean(command.muted);record.timelineLoop=Boolean(command.loop_video);if(record.media)record.media.loop=Boolean(command.loop_video);audio(record);
      const ctl={record,targetKey,resource:command.resource,mode:sourceSpec.mode,sourceKey:`${sourceSpec.mode}:${sourceSpec.url_or_path}`};timelineControllers.set(command.channel,ctl);return ctl;
    }
    function timelineSeek(record,milliseconds,loop,force=false){let seconds=Math.max(0,Number(milliseconds)||0)/1000;record.timelineLoop=Boolean(loop);if(record.aviAudio)record.aviAudio.loop=Boolean(loop);if(record.metadata&&record.duration>0)seconds=loop?seconds%record.duration:Math.min(seconds,record.duration);if(hasAvi(record)){const close=record.playing&&Math.abs(record.time-seconds)<=0.08;if(!close||force){record.time=seconds;if(record.aviAudio&&Math.abs((Number(record.aviAudio.currentTime)||0)-seconds)>0.12){try{record.aviAudio.currentTime=seconds;}catch(_){}}renderAviAt(record,seconds);}return;}if(!record.media){record.pendingSeek=seconds;record.time=seconds;return;}const current=Number(record.media.currentTime)||0,close=record.playing&&Math.abs(current-seconds)<=0.08;if(record.metadata&&(!close||force))seek(record,seconds);else if(!record.metadata)record.pendingSeek=seconds;record.time=close&&!force?current:seconds;}
    function timeline(commands){
      open();if(!Array.isArray(commands))throw new TypeError('Timeline video commands require an Array.');
      for(const command of commands){
        if(!command||typeof command.op!=='string')throw new TypeError('Invalid timeline video command.');
        if(command.op==='seek_and_present'||command.op==='start_event'){
          let ctl=timelineControllers.get(command.channel);const target=timelineTarget(command.target_path),targetKey=referenceOf(target)?.key;
          const nextSpec=command.source&&typeof command.source==='object'?`${command.source.mode}:${command.source.url_or_path}`:null;
          if(!ctl||ctl.resource!==command.resource||ctl.targetKey!==targetKey||(nextSpec&&ctl.sourceKey&&ctl.sourceKey!==nextSpec)){if(ctl)retireTimeline(command.channel,false);ctl=timelineRecord(command);}
          const r=ctl.record;r.timelineEventMode=command.op==='start_event';r.timelineMuted=Boolean(command.muted);r.timelineLoop=Boolean(command.loop_video);if(r.media)r.media.loop=Boolean(command.loop_video);audio(r);
          timelineSeek(r,command.op==='start_event'?command.start_offset_ms:command.target_time_ms,command.loop_video,command.op==='start_event'||!command.playing);
          if(command.op==='start_event'||command.playing){r.wanted=true;playNative(r);}else pause(r);
        }else if(command.op==='stop_channel')retireTimeline(command.channel,true);
        else if(command.op==='stop'){for(const[channel,ctl]of [...timelineControllers])if(ctl.resource===command.resource)retireTimeline(channel,true);}
        else if(command.op==='pause_streams'){for(const ctl of timelineControllers.values())if(ctl.record.wanted||ctl.record.playing)pause(ctl.record);}
        else if(command.op==='stop_all'){for(const channel of [...timelineControllers.keys()])retireTimeline(channel,false);}
        else if(command.op!=='cue_point_triggered')throw new TypeError(`Unknown timeline video command '${command.op}'.`);
      }
      changed();
    }
    function legacyController(target,observer){
      videoRef(target);if(typeof observer!=='function')throw new TypeError('AS2 video observer required.');
      const connection=new LegacyNetConnection();connection.connect(null);const stream=new LegacyNetStream(connection),record=streams.get(stream);record.controllerObserver=observer;attach(target,stream);let retired=false;
      function active(){if(retired)throw new Error('AS2 video controller has been released.');state(stream,streams);}
      return Object.freeze({
        read(){active();return controllerSample(record);},
        load(url,play){active();load(record,url);if(!play)pause(record);},
        play(){active();resume(record);},pause(){active();pause(record);},seek(time){active();seek(record,time);},
        close(){active();stopMedia(record,true);},volume(value){active();const v=number(value,'AS2 video volume gain',0,1);record.volume=v;record.soundTransform=defaultTransformWire(v);audio(record);},
        dispose(){if(retired)return;retired=true;record.controllerObserver=null;if(record.generation!==generation||disposed)return;stopMedia(record,false);const b=bindings.get(referenceOf(target)?.key);if(b?.stream===record){b.stream=null;b.frozen=null;b.revision=++serial;}record.receivePolicy?.free();record.receivePolicy=null;streamRecords.delete(record);connectionRecords.delete(connections.get(connection));collectFrames();},
      });
    }
    const _as3Hook=Object.freeze({connections,streams,connectionRecords,streamRecords,hub,NetStatusEvent,setOnStreamCreated:fn=>{onStreamCreated=fn;},state:(v,m)=>state(v,m),emit:(r,code,level,extra)=>emit(r,code,level,extra),later:(r,fn)=>later(r,fn),load:(r,...args)=>load(r,...args),stopMedia:(r,retain,keepQueue)=>stopMedia(r,retain,keepQueue),seek:(r,sec)=>seek(r,sec),pause:r=>pause(r),resume:r=>resume(r),audio:r=>audio(r),freeze:r=>freeze(r),setAppendFrame:(r,w,h,idx)=>setAppendFrame(r,w,h,idx),streamInfo:r=>streamInfo(r),recordDataBytes:(r,p)=>recordDataBytes(r,p),collectFrames:()=>collectFrames(),repaint:()=>repaint(),bumpSerial:()=>++serial,getGeneration:()=>generation,isDisposed:()=>disposed});
    function syncTimeline(frame,fps=24){
      open();const f=Math.max(0,Number(frame)||0),rate=Math.max(1,Number(fps)||Number(options.getFrameRate?.())||24);
      const targetSec=f/rate;
      for(const r of streamRecords){
        if(!r.timelineSync)continue;
        const clamped=r.metadata&&r.duration>0?Math.min(r.duration,targetSec):targetSec;
        r.time=clamped;
        if(r.media&&Math.abs((Number(r.media.currentTime)||0)-clamped)>1e-4)r.media.currentTime=clamped;
        r.serial=++serial;
      }
    }
    function resetState(){
      if(resetting)return;
      resetting=true;generation++;
      const owned=[...streamRecords],captures=[...bindings.values()].map(b=>b.capture),surfaces=[...frames],context=ownAudioContext,unsubscribe=mixerUnsub;
      ownAudioContext=null;mixerUnsub=null;
      try{
        completeCleanup([
          ()=>unsubscribe?.(),
          ...owned.flatMap(r=>[()=>stopMedia(r,false),()=>{const policy=r.receivePolicy;r.receivePolicy=null;policy?.free();}]),
          ...captures.map(capture=>()=>capture?.release()),
          ...surfaces.flatMap(f=>[()=>{f.canvas.width=0;},()=>{f.canvas.height=0;}]),
          ()=>{const closing=context?.close?.();closing?.catch?.(()=>{});},()=>hub.clear(),
        ]);
      }finally{
        streamRecords.clear();connectionRecords.clear();timelineControllers.clear();diagnosticsList.length=0;
        bindings.clear();derived.clear();frames.clear();framePixels=0;
        unlocked=false;muted=false;used=false;errorText='';resetting=false;
      }
      changed();
    }
    function reset(){open();resetState();}
    function dispose(){if(disposed)return;disposed=true;resetState();}
    return Object.freeze({Video,VideoStatus,NetConnection,NetStream,LegacyNetConnection,LegacyNetStream,NetStatusEvent,methods,get,set,UNHANDLED,draw,prepareSnapshot,timeline,timelineSources,diagnostics,syncTimeline,resumeFromGesture,setMuted,status,streamInfo,legacyController,legacyAttachVideo,reset,dispose,_as3Hook});
  }
  const api=Object.freeze({create,VideoStatus});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashVideo=api;
})(globalThis);
