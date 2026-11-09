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
    const connections=new WeakMap(),streams=new WeakMap(),connectionRecords=new Set(),streamRecords=new Set(),bindings=new Map(),timelineControllers=new Map(),frames=new Set(),derived=new Map();
    const statusTypes=new WeakSet(),schedule=options.schedule||(fn=>queueMicrotask(fn));
    const gesture=options.isUserGesture||(()=>global.navigator?.userActivation?.isActive===true);
    let generation=1,disposed=false,serial=0,used=false,unlocked=false,muted=false,errorText='',framePixels=0;
    const diagnosticsList=[];
    const hub=events.create({weakTargets:true,alive:value=>!disposed&&(connections.get(value)||streams.get(value))?.generation===generation});
    function open(){if(disposed)throw new Error('This video service has been disposed.');}
    function state(value,map){open();const record=map.get(value);if(!record)throw new TypeError('Expected a video object from this publication.');if(record.generation!==generation)throw new Error('This video object belongs to a previous publication session.');return record;}
    function native(name,...args){try{return player[name](...args);}catch(error){if(error instanceof Error)throw error;throw new Error(typeof error==='string'?error:'Video runtime failed.',{cause:error});}}
    function status(){let pending=0,active=0,loading=0;for(const r of streamRecords){if(r.wanted&&!r.playing)pending++;if(r.playing)active++;if(r.media&&!r.metadata)loading++;}return {state:!used?'idle':errorText?'error':pending&&!unlocked?'awaiting_gesture':muted?'muted':active?'running':loading?'loading':'ready',pending,active,loading,muted,unlocked,error:errorText,diagnostics:diagnosticsList.slice()};}
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
    function changed(){options.onState?.(status());}
    function report(error){errorText=String(error?.message||error);changed();options.onError?.(error);}
    function current(r,media,sequence){return !disposed&&r.generation===generation&&r.media===media&&r.sequence===sequence;}
    // AS2 component observers receive browser facts only. No observer is installed
    // for existing AS3 or ordinary NetStream users, preserving their event path.
    function controllerSample(r){return {now:global.performance?.now?.()||0,time:r.media?Number(r.media.currentTime)||0:r.time,width:r.width,height:r.height,duration:r.duration,bytesLoaded:r.url?(r.encodedBytes??null):0,bytesTotal:r.url?(r.encodedBytes??null):0};}
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
    function later(r,fn){const epoch=generation,sequence=r.sequence;schedule(()=>{if(disposed||epoch!==generation||r.generation!==generation||r.sequence!==sequence)return;try{fn();}catch(error){if(!disposed&&epoch===generation)report(error);}});}
    function noBase(Type,predicate){Object.defineProperties(Type,{__nfIs:{value:predicate},[Symbol.hasInstance]:{value:predicate},__nfInitialize:{configurable:true,value:()=>unsupported(`${Type.name} native subclassing`)}});}
    function canvas(w,h){const surface=options.createCanvas?options.createCanvas(w,h):typeof global.OffscreenCanvas==='function'?new global.OffscreenCanvas(w,h):global.document?.createElement('canvas');if(!surface)throw new Error('Video painting requires Canvas 2D.');surface.width=w;surface.height=h;return surface;}
    function collectFrames(){const keep=new Set();for(const r of streamRecords){if(r.frozen)keep.add(r.frozen);if(r.suppressedFrame)keep.add(r.suppressedFrame);if(r.appendFrame?.canvas)keep.add(r.appendFrame);}for(const b of bindings.values())if(b.frozen)keep.add(b.frozen);for(const f of derived.values())keep.add(f);for(const f of frames)if(!keep.has(f)){framePixels-=f.width*f.height;f.canvas.width=f.canvas.height=0;frames.delete(f);}}
    function frame(w,h){collectFrames();if(frames.size>=32||framePixels+w*h>MAX_PIXELS)throw new RangeError('Retained video frames exceed 16 Mi pixels / 32 surfaces; clear detached video images or restart.');const result={canvas:canvas(w,h),width:w,height:h,id:++serial};frames.add(result);framePixels+=w*h;return result;}
    function dimensions(media){const w=media.videoWidth,h=media.videoHeight;if(!Number.isInteger(w)||!Number.isInteger(h)||w<1||h<1||w>1920||h>1080)throw new RangeError('Video decoder dimensions must be within 1920×1080.');return[w,h];}
    function freeze(r){
      if(r.receiveState?.video===false)return r.suppressedFrame||null;
      if(!r.media&&r.appendFrame)return r.appendFrame;
      if(!r.media||r.media.readyState<2)return r.frozen||null;
      const [w,h]=dimensions(r.media),f=frame(w,h),ctx=f.canvas.getContext('2d');if(!ctx)throw new Error('Video capture requires Canvas 2D.');
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
    function audio(r){if(!r.media)return;r.media.muted=!unlocked||muted||r.timelineMuted===true||r.volume===0||r.receiveState?.audio===false;r.media.volume=r.volume;}
    function stopMedia(r,retain){
      const frozen=retain?freeze(r):null;
      r.sequence++;r.wanted=false;r.playing=false;r.pendingSeek=null;
      if(r.media){for(const[type,fn]of r.listeners)r.media.removeEventListener(type,fn);if(r.frameCallback!==null)r.media.cancelVideoFrameCallback?.(r.frameCallback);r.media.pause();r.media.muted=true;r.media.removeAttribute('src');r.media.load();}
      if(r.objectURL)(options.revokeObjectURL||global.URL.revokeObjectURL.bind(global.URL))(r.objectURL);
      r.media=null;r.objectURL=null;r.listeners=[];r.frameCallback=null;r.metadata=false;r.frozen=frozen;r.appendFrame=null;r.suppressedFrame=null;if(r.receivePolicy)streamPolicy(r,{op:"reset"});r.time=0;r.lastTime=null;collectFrames();changed();
    }
    function playNative(r){
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
      const [width,height]=dimensions(media);if(!Number.isFinite(media.duration)||media.duration<0)throw new TypeError('Live or unbounded-duration video is unsupported; use a finite media file.');
      r.metadata=true;r.width=width;r.height=height;r.duration=media.duration;r.serial=++serial;
      observeController(r,'metadata');if(!current(r,media,sequence))return;
      const client=r.timelineOwned?null:(r.legacy?r.receiver:r.client),fn=client?.onMetaData;
      if(!current(r,media,sequence))return;
      if(typeof fn==='function')Reflect.apply(fn,client,[{width,height,duration:media.duration}]);
      if(!current(r,media,sequence))return;
      if(r.pendingSeek!==null){let at=r.pendingSeek;r.pendingSeek=null;if(r.timelineOwned&&r.duration>0)at=r.timelineLoop?at%r.duration:Math.min(at,r.duration);seek(r,at);}after(generation);changed();
    }
    function load(r,url,sourceSpec=null){
      const mode=sourceSpec?.mode||r.deploymentMode||'embedded';
      if(typeof url!=='string'||!url)throw new TypeError('NetStream.play requires a media URL string.');
      if(!r.connection.local)throw new Error('Call NetConnection.connect(null) before playing local video.');
      if(/\.(?:flv|f4v|swf|gif)(?:[?#]|$)/i.test(url))throw new TypeError('This browser profile supports native MP4/WebM media, not FLV/F4V/SWF/GIF decoding.');
      let resolvedUrl=url,source=null,mediaType=sourceSpec?.mime_type||'video/mp4';
      if(mode==='external'){
        const relPath=validateExternalPath(sourceSpec?.url_or_path||url);
        resolvedUrl=resolveExternalUrl(relPath);
        if(sourceSpec?.mime_type&&!['video/mp4','video/webm','video/ogg'].includes(String(sourceSpec.mime_type).toLowerCase()))throw new TypeError(`External video MIME type ${sourceSpec.mime_type} is unsupported; use a native browser video format.`);
        r.deploymentMode='external';
        r.sourceSpec={mode:'external',url_or_path:relPath,resolvedUrl,mime_type:mediaType};
      }else if(mode==='progressive'){
        resolvedUrl=validateProgressiveUrl(sourceSpec?.url_or_path||url);
        if(sourceSpec?.mime_type&&!['video/mp4','video/webm','video/ogg'].includes(String(sourceSpec.mime_type).toLowerCase()))throw new TypeError(`Progressive video MIME type ${sourceSpec.mime_type} is unsupported; use a native browser video format.`);
        r.deploymentMode='progressive';
        r.sourceSpec={mode:'progressive',url_or_path:resolvedUrl,resolvedUrl,mime_type:mediaType};
      }else{
        source=net.mediaSource(url);
        if(source.kind==='resource'&&!['video/mp4','video/webm','video/ogg'].includes(source.mediaType))throw new TypeError(`Bundled video MIME type ${source.mediaType} is unsupported; use a native browser video format.`);
        r.deploymentMode=source.kind==='resource'?'embedded':'progressive';
        r.sourceSpec={mode:r.deploymentMode,url_or_path:url,resolvedUrl:source.kind==='resource'?url:source.url,mime_type:source.mediaType||mediaType};
      }
      stopMedia(r,false);used=true;errorText='';r.wanted=true;r.encodedBytes=source?.kind==='resource'?source.length:null;r.url=resolvedUrl;
      const media=options.createVideo?options.createVideo():global.document?.createElement('video');if(!media)throw new Error('HTMLVideoElement is unavailable in this environment.');
      r.media=media;media.muted=true;media.volume=0;media.crossOrigin=options.crossOrigin!==undefined?options.crossOrigin:'anonymous';media.preload='auto';media.playsInline=true;media.loop=false;
      const sequence=r.sequence;
      function listen(type,fn){const callback=()=>{if(!current(r,media,sequence))return;try{fn();}catch(error){if(current(r,media,sequence)){stopMedia(r,false);report(error);}}};media.addEventListener(type,callback);r.listeners.push([type,callback]);}
      listen('loadedmetadata',()=>metadata(r,media,sequence));
      listen('loadeddata',()=>{r.serial=++serial;observeController(r,'ready');repaint();if(current(r,media,sequence)&&r.legacy)playNative(r);});
      listen('canplay',()=>{if(r.legacy)playNative(r);});
      listen('playing',()=>{r.playing=true;r.serial=++serial;if(!r.started){r.started=true;emit(r,'NetStream.Play.Start');}else if(r.pauseRequested){r.pauseRequested=false;emit(r,'NetStream.Unpause.Notify');}observeController(r,'playing');changed();});
      listen('pause',()=>{r.playing=false;observeController(r,'paused');changed();});
      listen('timeupdate',()=>{r.serial=++serial;r.time=media.currentTime;observeController(r,'time');repaint();});
      listen('seeked',()=>{r.serial=++serial;r.time=media.currentTime;emit(r,'NetStream.Seek.Notify');observeController(r,'seeked');if(current(r,media,sequence)&&r.legacy)playNative(r);});
      listen('ended',()=>{r.wanted=false;r.playing=false;r.time=media.currentTime;r.serial=++serial;if(!emit(r,'NetStream.Play.Stop'))return;if(r.legacy&&typeof r.receiver.onPlayStatus==='function'){Reflect.apply(r.receiver.onPlayStatus,r.receiver,[{code:'NetStream.Play.Complete',level:'status'}]);if(!current(r,media,sequence))return;after(generation);}observeController(r,'ended');changed();});
      listen('error',()=>{
        r.wanted=false;r.playing=false;
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
      listen('waiting',()=>{if(r.legacy&&r.wanted){streamPolicy(r,{op:'buffer_wait'});r.playing=false;media.pause();}observeController(r,'waiting');});
      listen('progress',()=>{observeController(r,'progress');if(current(r,media,sequence)&&r.legacy)playNative(r);});
      const nextFrame=(now,facts)=>{if(!current(r,media,sequence))return;try{if(r.receivePolicy)streamPolicy(r,{op:"frame",now,presented:facts.presentedFrames});r.serial=++serial;observeController(r,'time');repaint();if(current(r,media,sequence))r.frameCallback=media.requestVideoFrameCallback(nextFrame);}catch(error){if(current(r,media,sequence))report(error);}};
      if(typeof media.requestVideoFrameCallback==='function')r.frameCallback=media.requestVideoFrameCallback(nextFrame);
      if(source?.kind==='resource'){const blob=new Blob([source.bytes],{type:source.mediaType});r.objectURL=(options.createObjectURL||global.URL.createObjectURL.bind(global.URL))(blob);media.src=r.objectURL;}else media.src=resolvedUrl;
      r.started=false;r.width=r.height=0;r.duration=0;media.load();if(gesture()){unlocked=true;muted=false;}if(!r.timelineOwned)playNative(r);changed();
    }
    function pause(r){const media=r.media,wasPlaying=r.wanted||r.playing;r.wanted=false;r.pauseRequested=true;r.playing=false;media?.pause();if(media&&wasPlaying)later(r,()=>{if(r.media===media&&!r.wanted&&media.paused)emit(r,'NetStream.Pause.Notify');});changed();}
    function resume(r){r.wanted=true;if(gesture()){unlocked=true;muted=false;}playNative(r);changed();}
    function seek(r,value){number(value,'NetStream.seek',0,86400*365);if(!r.media)throw new Error('NetStream.seek requires a loaded stream.');if(!r.metadata){r.pendingSeek=value;return;}if(value>r.duration)throw new RangeError('NetStream.seek is beyond the finite media duration.');if(r.legacy){streamPolicy(r,{op:'buffer_wait'});r.playing=false;r.media.pause();}r.media.currentTime=value;}
    class NetConnection extends events.EventDispatcher{
      constructor(){super();open();const record={receiver:this,generation,sequence:0,local:false,legacy:false};connections.set(this,record);connectionRecords.add(record);Object.assign(this,hub.methods(this));this.client=this;if(new.target===NetConnection)Object.seal(this);}
      connect(command){const r=state(this,connections);if(command!==null)unsupported('NetConnection remote connections');r.local=true;r.sequence++;later(r,()=>emit(r,'NetConnection.Connect.Success'));}
      close(){const r=state(this,connections);r.local=false;r.sequence++;for(const stream of streamRecords)if(stream.connection===r)stopMedia(stream,true);later(r,()=>emit(r,'NetConnection.Connect.Closed'));}
      get connected(){state(this,connections);return false;}get uri(){state(this,connections);return null;}
    }
    class LegacyNetConnection extends NetConnection{
      constructor(){super();connections.get(this).legacy=true;this.onStatus=null;}
      connect(command){const r=state(this,connections),remote=options.getRemoting?.();if(command===null){remote?.close(this);super.connect(null);return true;}if(!remote)unsupported('AS2 HTTP remoting service unavailable');if(r.local)super.close();return remote.connect(this,command);}
      close(){state(this,connections);options.getRemoting?.()?.close(this);super.close();}
      call(method,responder=null,...args){state(this,connections);const remote=options.getRemoting?.();if(!remote)unsupported('AS2 HTTP remoting service unavailable');return remote.call(this,method,responder,args);}
      get isConnected(){const r=state(this,connections);return options.getRemoting?.()?.isConnected(this,r.local)??r.local;}
      get uri(){state(this,connections);return options.getRemoting?.()?.uri(this);}
      get protocol(){const r=state(this,connections),uri=options.getRemoting?.()?.uri(this);if(typeof uri==='string'&&uri){try{return new URL(uri).protocol.replace(/:$/,'');}catch(_){}}return r.local?'':'';}
    }
    let onStreamCreated=null;
    class NetStream extends events.EventDispatcher{
      constructor(connection,peerID='connectToFMS'){super();const nc=state(connection,connections);if(peerID!==undefined&&peerID!==null&&typeof peerID!=='string')throw new TypeError('NetStream peerID requires a String.');if(streamRecords.size>=64)throw new RangeError('Video is limited to 64 NetStream instances per publication session.');const r={receiver:this,generation,connection:nc,peerID:peerID||'connectToFMS',sequence:0,legacy:false,client:this,media:null,objectURL:null,listeners:[],frameCallback:null,frozen:null,serial:++serial,metadata:false,wanted:false,playing:false,started:false,pauseRequested:false,pendingSeek:null,width:0,height:0,duration:0,time:0,lastTime:null,volume:1};streams.set(this,r);streamRecords.add(r);Object.assign(this,hub.methods(this));onStreamCreated?.(this,r);if(new.target===NetStream)Object.seal(this);}
      play(url,...extra){if(extra.length)unsupported('NetStream.play playlist/start/length/reset arguments');load(state(this,streams),url);}
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
      close(){stopMedia(state(this,streams),true);}
      get time(){const r=state(this,streams);return r.media?Number(r.media.currentTime)||0:r.time;}
      get client(){return state(this,streams).client;}set client(value){if(value===null||(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('NetStream.client requires an object.');state(this,streams).client=value;}
      get soundTransform(){const r=state(this,streams),Type=options.getSoundTransform?.();if(!Type)throw new Error('SoundTransform service is unavailable.');return new Type(r.volume,0);}
      set soundTransform(value){const r=state(this,streams),Type=options.getSoundTransform?.();if(!Type?.__nfIs?.(value))throw new TypeError('NetStream.soundTransform requires SoundTransform.');const volume=number(value.volume,'Video volume',0,1);if(value.pan!==0)unsupported('NetStream nonzero stereo pan');r.volume=volume;audio(r);}
      get bytesLoaded(){const r=state(this,streams);if(r.encodedBytes===null)unsupported('NetStream HTTP byte counts (browser-managed progressive buffering)');return r.encodedBytes||0;}
      get bytesTotal(){return this.bytesLoaded;}
    }
    function streamPolicy(r,request){
      if(!r.receivePolicy)throw new Error('AS2 stream controls require the shared Rust WASM media policy.');
      let result;try{result=JSON.parse(r.receivePolicy.execute(JSON.stringify(request)));}catch(error){throw new TypeError(typeof error==='string'?error:String(error?.message||error));}r.receiveState=result;return result.value;
    }
    function bufferFacts(r){const media=r.media,ranges=[];if(media)for(let i=0;i<(media.buffered?.length||0);i++)ranges.push([media.buffered.start(i),media.buffered.end(i)]);return {time:media?.currentTime||0,duration:r.metadata?r.duration:null,ready:(media?.readyState||0)>=2,ranges};}
    class LegacyNetStream extends NetStream{constructor(connection){super(connection);const r=streams.get(this);r.legacy=true;r.checkPolicyFile=false;r.receivePolicy=options.wasm?.ScriptAS2Stream?new options.wasm.ScriptAS2Stream():null;r.receiveState={audio:true,video:true};r.suppressedFrame=null;r.attachedAudio=null;r.attachedCamera=null;r.publishedName=null;r.publishedType=null;this.onStatus=null;this.onMetaData=null;this.onPlayStatus=null;this.onCuePoint=null;this.onTextData=null;this.onXMPData=null;}
      get checkPolicyFile(){return state(this,streams).checkPolicyFile;}
      set checkPolicyFile(value){if(typeof value!=='boolean')throw new TypeError('AS2 NetStream.checkPolicyFile requires a Boolean.');state(this,streams).checkPolicyFile=value;}
      get bytesLoaded(){const r=state(this,streams);return r.encodedBytes||0;}
      get bytesTotal(){return this.bytesLoaded;}
      receiveAudio(enabled){const r=state(this,streams);streamPolicy(r,{op:'receive',kind:'audio',enabled});audio(r);}
      receiveVideo(enabled){const r=state(this,streams);if(typeof enabled==='number'){if(!Number.isFinite(enabled)||enabled<0)throw new RangeError('AS2 NetStream.receiveVideo FPS must be a finite non-negative number.');enabled=enabled>0;}else if(typeof enabled!=='boolean')throw new TypeError('AS2 NetStream.receiveVideo requires a Boolean or non-negative FPS number.');if(enabled===r.receiveState.video)return;const captured=enabled?null:freeze(r);streamPolicy(r,{op:'receive',kind:'video',enabled});r.suppressedFrame=captured;r.serial=++serial;collectFrames();repaint();}
      get bufferLength(){const r=state(this,streams);return streamPolicy(r,{op:'buffer',...bufferFacts(r)});}
      get bufferTime(){return streamPolicy(state(this,streams),{op:'buffer_time'});}
      setBufferTime(seconds){const r=state(this,streams);streamPolicy(r,{op:'set_buffer_time',seconds});playNative(r);}
      get liveDelay(){state(this,streams);return 0;}
      get maxPauseBufferTime(){return streamPolicy(state(this,streams),{op:'max_pause_buffer_time'});}
      set maxPauseBufferTime(seconds){streamPolicy(state(this,streams),{op:'set_max_pause_buffer_time',seconds});}
      getInfo(){
        const r=state(this,streams),bufLen=this.bufferLength,loaded=r.encodedBytes||0,aud=r.receiveState?.audio!==false,vid=r.receiveState?.video!==false;
        return {audioBufferByteLength:aud?loaded:0,videoBufferByteLength:vid?loaded:0,dataBufferByteLength:0,audioBufferLength:aud?bufLen:0,videoBufferLength:vid?bufLen:0,srtt:0,droppedFrames:0,currentBytesPerSecond:0,maxBytesPerSecond:0,playbackBytesPerSecond:0,isLive:false};
      }
      play2(param){
        const r=state(this,streams);
        if(!param||typeof param!=='object'||typeof param.streamName!=='string'||!param.streamName)throw new TypeError('AS2 NetStream.play2 requires an options Object with a non-empty streamName String.');
        load(r,param.streamName);
        if(typeof param.start==='number'&&Number.isFinite(param.start)&&param.start>0)seek(r,param.start);
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
        later(r,()=>{const fn=r.receiver[handlerName];if(typeof fn==='function')Reflect.apply(fn,r.receiver,args);after(generation);});
      }
      get currentFps(){const r=state(this,streams);if(r.media&&typeof r.media.requestVideoFrameCallback!=='function')return 0;return streamPolicy(r,{op:'fps',now:global.performance?.now?.()||0,playing:r.playing});}
      pause(flag){const r=state(this,streams);if(flag===undefined)return super.togglePause();if(typeof flag!=='boolean')throw new TypeError('AS2 NetStream.pause expects a Boolean or no argument.');if(flag)pause(r);else resume(r);}get soundTransform(){unsupported('AS2 NetStream.soundTransform; legacy Sound target mixing');}set soundTransform(_){unsupported('AS2 NetStream.soundTransform; legacy Sound target mixing');}}
    for(const[Type,predicate]of [[NetConnection,v=>connections.has(v)],[LegacyNetConnection,v=>connections.has(v)&&connections.get(v).legacy],[NetStream,v=>streams.has(v)],[LegacyNetStream,v=>streams.has(v)&&streams.get(v).legacy],[NetStatusEvent,v=>statusTypes.has(v)]])noBase(Type,predicate);
    for(const name of ['publish','send','appendBytes','appendBytesAction','attachAudio','attachCamera','attachVideo','setBufferTime','receiveAudio','receiveVideo','dispose'])Object.defineProperty(NetStream.prototype,name,{configurable:true,writable:true,value(){unsupported('NetStream.'+name);}});
    for(const name of ['bufferTime','bufferLength','checkPolicyFile','objectEncoding','videoStreamSettings','inBufferSeek','backBufferTime','currentFPS','info','farID','peerStreams'])Object.defineProperty(NetStream.prototype,name,{configurable:true,get(){unsupported('NetStream.'+name);},set(){unsupported('NetStream.'+name);}});
    for(const name of ['call','addHeader','addEventListenerRemote'])Object.defineProperty(NetConnection.prototype,name,{configurable:true,writable:true,value(){unsupported('NetConnection.'+name);}});
    function videoRef(target){const value=referenceOf(target);if(!value||!player.object_alive(value.key))throw new Error('Video display object is stale or foreign.');const info=JSON.parse(native('object_video_info',value.key));return {key:value.key,ref:value.objectRef,info};}
    function binding(target){const value=videoRef(target);let b=bindings.get(value.key);if(!b){b={...value,stream:null,capture:null,captureClearId:null,frozen:null,clearSerial:-1,revision:++serial};bindings.set(value.key,b);}return b;}
    function attachRecord(target,next){const b=binding(target);if(next===b.stream&&!b.capture)return;
      if(b.capture){b.frozen=freezeCapture(b);b.capture.release();b.capture=null;b.captureClearId=null;}
      const old=b.stream;b.frozen=old?freeze(old):b.frozen;b.stream=null;
      if(next){for(const other of bindings.values())if(other!==b&&other.stream===next){other.frozen=freeze(next);other.stream=null;other.revision=++serial;}b.stream=next;b.frozen=null;b.clearSerial=-1;}b.revision=++serial;collectFrames();after(generation);
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
    function source(b){if(!b)return null;if(b.capture){const f=b.capture.read();return f&&f.id!==b.captureClearId?f:null;}const r=b.stream;if(r){if(r.receiveState?.video===false){const f=r.suppressedFrame;return f&&f.id>b.clearSerial?{...f,image:f.canvas}:null;}if(r.media?.readyState>=2&&r.serial>b.clearSerial)return {image:r.media,width:r.width,height:r.height,id:`${r.serial}:${r.media.currentTime}`};if(!r.media&&r.appendFrame&&r.serial>b.clearSerial)return {...r.appendFrame,image:r.appendFrame.canvas};if(!r.media&&r.frozen&&r.serial>b.clearSerial)return {...r.frozen,image:r.frozen.canvas};return null;}return b.frozen?{...b.frozen,image:b.frozen.canvas}:null;}
    function methods(target){return {attachNetStream:value=>attach(target,value),attachVideo:value=>attach(target,value),clear:()=>clear(target),attachCamera:value=>attachAs3Camera(target,value)};}
    function get(target,name){if(name==='smoothing')return videoRef(target).info.smoothing;if(name==='deblocking')return videoRef(target).info.deblocking??0;if(name==='videoWidth'||name==='videoHeight'){const b=binding(target),r=b.stream,f=source(b);return r?(name==='videoWidth'?r.width:r.height):f?(name==='videoWidth'?f.width:f.height):0;}return UNHANDLED;}
    function set(target,name,value){if(name==='smoothing'){if(typeof value!=='boolean')throw new TypeError('Video.smoothing requires a Boolean.');const v=videoRef(target),next={...v.info,smoothing:value};native('object_video_update',v.key,JSON.stringify(next));const b=bindings.get(v.key);if(b)b.info=next;return true;}if(name==='deblocking'){if(!Number.isInteger(value)||value<0||value>5)throw new RangeError('Video.deblocking requires an integer from 0 through 5.');const v=videoRef(target),next={...v.info,deblocking:value};native('object_video_update',v.key,JSON.stringify(next));const b=bindings.get(v.key);if(b)b.info=next;return true;}if(name==='videoWidth'||name==='videoHeight')throw new TypeError(`Video.${name} is read-only.`);return false;}
    class Video{
      constructor(width=320,height=240){open();number(width,'Video width',0,8192);number(height,'Video height',0,8192);if(!Number.isInteger(width)||!Number.isInteger(height))throw new TypeError('Video dimensions require integers.');const target=proxyForRef(JSON.parse(native('create_display','video')));const v=videoRef(target);native('object_video_update',v.key,JSON.stringify({width:width||320,height:height||240,smoothing:false,deblocking:0}));return target;}
    }
    noBase(Video,value=>{try{videoRef(value);return true;}catch(_){return false;}});
    function prepareSnapshot(snapshot){
      for(const[key,b]of bindings)if(!player.object_alive(key)){b.capture?.release();bindings.delete(key);}
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
      const record={receiver:null,generation,connection,peerID:'timeline',sequence:0,legacy:false,timelineOwned:true,resourceId:command.resource,deploymentMode:sourceSpec.mode,sourceSpec,timelineMuted:Boolean(command.muted),timelineLoop:Boolean(command.loop_video),client:null,media:null,objectURL:null,listeners:[],frameCallback:null,frozen:null,serial:++serial,metadata:false,wanted:false,playing:false,started:false,pauseRequested:false,pendingSeek:null,width:0,height:0,duration:0,time:0,lastTime:null,volume:1};
      streamRecords.add(record);attachRecord(target,record);load(record,sourceSpec.url_or_path,sourceSpec);record.timelineMuted=Boolean(command.muted);record.timelineLoop=Boolean(command.loop_video);if(record.media)record.media.loop=Boolean(command.loop_video);audio(record);
      const ctl={record,targetKey,resource:command.resource,mode:sourceSpec.mode,sourceKey:`${sourceSpec.mode}:${sourceSpec.url_or_path}`};timelineControllers.set(command.channel,ctl);return ctl;
    }
    function timelineSeek(record,milliseconds,loop,force=false){let seconds=Math.max(0,Number(milliseconds)||0)/1000;record.timelineLoop=Boolean(loop);if(record.metadata&&record.duration>0)seconds=loop?seconds%record.duration:Math.min(seconds,record.duration);if(!record.media)throw new Error('Timeline video decoder was retired before seek.');const current=Number(record.media.currentTime)||0,close=record.playing&&Math.abs(current-seconds)<=0.08;if(record.metadata&&(!close||force))seek(record,seconds);else if(!record.metadata)record.pendingSeek=seconds;record.time=close&&!force?current:seconds;}
    function timeline(commands){
      open();if(!Array.isArray(commands))throw new TypeError('Timeline video commands require an Array.');
      for(const command of commands){
        if(!command||typeof command.op!=='string')throw new TypeError('Invalid timeline video command.');
        if(command.op==='seek_and_present'||command.op==='start_event'){
          let ctl=timelineControllers.get(command.channel);const target=timelineTarget(command.target_path),targetKey=referenceOf(target)?.key;
          const nextSpec=command.source&&typeof command.source==='object'?`${command.source.mode}:${command.source.url_or_path}`:null;
          if(!ctl||ctl.resource!==command.resource||ctl.targetKey!==targetKey||(nextSpec&&ctl.sourceKey&&ctl.sourceKey!==nextSpec)){if(ctl)retireTimeline(command.channel,false);ctl=timelineRecord(command);}
          const r=ctl.record;r.timelineMuted=Boolean(command.muted);r.timelineLoop=Boolean(command.loop_video);if(r.media)r.media.loop=Boolean(command.loop_video);audio(r);
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
        close(){active();stopMedia(record,true);},volume(value){active();record.volume=number(value,'AS2 video volume gain',0,1);audio(record);},
        dispose(){if(retired)return;retired=true;record.controllerObserver=null;if(record.generation!==generation||disposed)return;stopMedia(record,false);const b=bindings.get(referenceOf(target)?.key);if(b?.stream===record){b.stream=null;b.frozen=null;b.revision=++serial;}record.receivePolicy?.free();record.receivePolicy=null;streamRecords.delete(record);connectionRecords.delete(connections.get(connection));collectFrames();},
      });
    }
    const _as3Hook=Object.freeze({connections,streams,connectionRecords,streamRecords,hub,NetStatusEvent,setOnStreamCreated:fn=>{onStreamCreated=fn;},state:(v,m)=>state(v,m),emit:(r,code,level,extra)=>emit(r,code,level,extra),later:(r,fn)=>later(r,fn),load:(r,url)=>load(r,url),stopMedia:(r,retain)=>stopMedia(r,retain),seek:(r,sec)=>seek(r,sec),pause:r=>pause(r),resume:r=>resume(r),audio:r=>audio(r),freeze:r=>freeze(r),setAppendFrame:(r,w,h,idx)=>setAppendFrame(r,w,h,idx),collectFrames:()=>collectFrames(),repaint:()=>repaint(),bumpSerial:()=>++serial,getGeneration:()=>generation,isDisposed:()=>disposed});
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
    function reset(){open();for(const r of streamRecords){stopMedia(r,false);r.receivePolicy?.free();r.receivePolicy=null;}streamRecords.clear();connectionRecords.clear();timelineControllers.clear();diagnosticsList.length=0;for(const b of bindings.values())b.capture?.release();bindings.clear();derived.clear();for(const f of frames)f.canvas.width=f.canvas.height=0;frames.clear();framePixels=0;generation++;hub.clear();unlocked=false;muted=false;used=false;errorText='';changed();}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({Video,VideoStatus,NetConnection,NetStream,LegacyNetConnection,LegacyNetStream,NetStatusEvent,methods,get,set,UNHANDLED,draw,prepareSnapshot,timeline,timelineSources,diagnostics,syncTimeline,resumeFromGesture,setMuted,status,legacyController,legacyAttachVideo,reset,dispose,_as3Hook});
  }
  const api=Object.freeze({create,VideoStatus});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashVideo=api;
})(globalThis);
