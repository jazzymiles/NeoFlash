/* AS2 local devices. Browser APIs supply device facts, frames and PCM; Rust owns
 * identity, constraints, gain conversion, motion/RMS and activity hysteresis. */
(function(global){
'use strict';
function create(wasm,options={}){
 if(typeof wasm?.ScriptAS2Capture!=='function')throw new Error('AS2 capture requires the Rust capture module.');
 const kernel=new wasm.ScriptAS2Capture(),mediaDevices=options.mediaDevices??global.navigator?.mediaDevices;
 const now=options.now||(()=>global.performance?.now?.()||0),gesture=options.isUserGesture||(()=>global.navigator?.userActivation?.isActive===true);
 const records=new Map(),identities=new WeakMap(),audioTargets=new Map();let generation=1,disposed=false,discovery=null;
 const schedule=options.schedule||((fn)=>global.setInterval(fn,100)),cancel=options.cancel||((id)=>global.clearInterval(id));
 function open(){if(disposed)throw new Error('AS2 capture service has been disposed.');}
 function raw(request){open();try{return JSON.parse(kernel.execute(JSON.stringify({now:now(),...request})));}catch(error){throw error instanceof Error?error:new TypeError(String(error));}}
 function state(value,kind){open();const r=identities.get(value);if(!r||r.generation!==generation||kind&&r.kind!==kind)throw new TypeError('Expected a live '+(kind||'capture')+' object from this publication.');return r;}
 function report(error){if(options.onError)options.onError(error);else global.console?.error(error);}
 function invoke(fn,receiver,args){try{Reflect.apply(fn,receiver,args);}catch(error){report(error);}}
 function publish(r,result){
  if(r.generation!==generation||disposed)return;const epoch=generation;r.state=result.state;
  for(const event of result.events||[]){if(epoch!==generation||disposed)return;const fn=r.receiver[event.type==='status'?'onStatus':'onActivity'];if(typeof fn==='function')invoke(fn,r.receiver,[event.type==='status'?event.info:event.active]);}
  if(epoch===generation&&(result.events?.length))options.afterEvent?.();
 }
 function command(r,request){const result=raw({id:r.id,...request});publish(r,result);return result;}
 function snapshot(r){return command(r,{op:'state'}).state;}
 function unsupported(name){throw new TypeError(name+' requires a Flash encoder/server facility unavailable in local browser capture.');}
 function cleanup(r,notify=false){
  r.sequence++;if(r.timer!==null)cancel(r.timer);r.timer=null;
  if(r.video){if(r.frameCallback!==null)r.video.cancelVideoFrameCallback?.(r.frameCallback);r.video.pause();r.video.srcObject=null;r.video=null;}r.frameCallback=null;
  for(const [target,name,fn]of r.listeners)target.removeEventListener?.(name,fn);r.listeners=[];
  for(const track of r.stream?.getTracks?.()||[])track.stop();r.stream=null;r.track=null;
  for(const node of [r.source,r.gain,r.analyser])try{node?.disconnect();}catch(_){}r.source=r.gain=r.analyser=null;
  if(r.surface){r.surface.width=r.surface.height=0;r.surface=null;}
  r.pending=null;r.lastMotion=-Infinity;r.serial++;
  if(!disposed&&r.generation===generation){const result=raw({op:'stop',id:r.id});if(notify)publish(r,result);else r.state=result.state;}
 }
 function settings(r){const facts=r.track?.getSettings?.()||{};const values={...facts,label:r.track?.label||r.state.name};if(r.kind==='camera'){values.width=values.width||r.video?.videoWidth;values.height=values.height||r.video?.videoHeight;if(!values.width||!values.height)return;}command(r,{op:'settings',settings:values});}
 function valid(r,sequence){return !disposed&&r.generation===generation&&r.sequence===sequence;}
 function fail(r,error,sequence){if(!valid(r,sequence))return;cleanup(r);command(r,{op:'status',status:error?.name==='NotAllowedError'||error?.name==='PermissionDeniedError'?'denied':'error',description:String(error?.message||error)});}
 function activitySample(r){
  for(const token of [...r.leases])if(token.alive&&!token.alive())token.release();
  if(!r.leases.size||!r.stream)return;
  try{
   if(r.kind==='microphone'&&r.analyser){const samples=new Float32Array(r.analyser.fftSize);r.analyser.getFloatTimeDomainData(samples);publish(r,JSON.parse(kernel.audio_samples(r.id,now(),samples)));}
   else if(r.video?.readyState>=2){if(!r.surface){r.surface=options.createCanvas?options.createCanvas(32,24):typeof global.OffscreenCanvas==='function'?new global.OffscreenCanvas(32,24):global.document?.createElement('canvas');if(!r.surface)throw new Error('Camera motion sampling requires Canvas2D.');r.surface.width=32;r.surface.height=24;}const ctx=r.surface.getContext('2d',{willReadFrequently:true});if(!ctx)throw new Error('Camera motion sampling requires Canvas2D.');ctx.drawImage(r.video,0,0,32,24);publish(r,JSON.parse(kernel.video_pixels(r.id,now(),ctx.getImageData(0,0,32,24).data)));}
  }catch(error){fail(r,error,r.sequence);}
 }
 function start(r){
  if(r.stream||r.pending)return;
  if(typeof mediaDevices?.getUserMedia!=='function')throw new Error('Browser capture is unavailable; use a secure context with MediaDevices support.');
  if(!gesture())throw new Error('Attach a camera or microphone from an explicit user click before requesting permission.');
  let bus=null;if(r.kind==='microphone'){const audio=options.getAudio?.();if(!audio?.captureBus)throw new Error('Microphone playback requires the shared AudioContext service.');bus=audio.captureBus();}
  const requested=command(r,{op:'request',gesture:true}).effect,sequence=++r.sequence;if(bus?.ready)Promise.resolve(bus.ready).then(ok=>{if(ok===false)fail(r,new Error('Shared audio playback was not enabled.'),sequence);},error=>fail(r,error,sequence));
  let request;try{request=mediaDevices.getUserMedia(r.kind==='camera'?{video:requested.constraints,audio:false}:{video:false,audio:requested.constraints});}catch(error){fail(r,error,sequence);return;}
  r.pending=Promise.resolve(request).then(stream=>{
   if(!valid(r,sequence)||!r.leases.size){for(const track of stream?.getTracks?.()||[])track.stop();return;}
   r.pending=null;r.stream=stream;r.track=(r.kind==='camera'?stream.getVideoTracks():stream.getAudioTracks())[0];if(!r.track)throw new Error('Browser returned no requested capture track.');
   const ended=()=>{if(!valid(r,sequence))return;cleanup(r);command(r,{op:'status',status:'ended',description:'The browser capture track ended.'});};r.track.addEventListener?.('ended',ended);r.listeners.push([r.track,'ended',ended]);
   if(r.kind==='camera'){
    const video=options.createVideo?options.createVideo():global.document?.createElement('video');if(!video)throw new Error('Camera preview requires HTMLVideoElement.');r.video=video;video.muted=true;video.playsInline=true;video.autoplay=true;
    const metadata=()=>{if(valid(r,sequence)){try{settings(r);}catch(error){fail(r,error,sequence);}}};video.addEventListener?.('loadedmetadata',metadata);r.listeners.push([video,'loadedmetadata',metadata]);
    video.srcObject=stream;settings(r);command(r,{op:'status',status:'allowed'});if(!valid(r,sequence))return;
    const frame=(timestamp,facts)=>{if(!valid(r,sequence)||r.video!==video)return;try{command(r,{op:'frame',now:timestamp,presented:facts.presentedFrames});r.serial++;for(const token of r.leases)token.onFrame?.();if(valid(r,sequence))r.frameCallback=video.requestVideoFrameCallback(frame);}catch(error){fail(r,error,sequence);}};
    if(typeof video.requestVideoFrameCallback==='function')r.frameCallback=video.requestVideoFrameCallback(frame);
    Promise.resolve(video.play()).catch(error=>fail(r,error,sequence));
   }else{
    r.source=bus.context.createMediaStreamSource(stream);r.gain=bus.context.createGain();r.analyser=bus.context.createAnalyser();r.analyser.fftSize=1024;r.gain.gain.value=snapshot(r).gainMultiplier;r.source.connect(r.gain);r.gain.connect(r.analyser);r.analyser.connect(bus.destination);settings(r);command(r,{op:'status',status:'allowed'});
   }
   if(valid(r,sequence)){r.timer=schedule(()=>{activitySample(r);if(r.kind==='camera'&&r.video&&typeof r.video.requestVideoFrameCallback!=='function'){r.serial++;for(const token of r.leases)token.onFrame?.();}});}
  }).catch(error=>fail(r,error,sequence));
 }
 function acquire(r,{onFrame,alive}={}){
  const token={onFrame,alive,released:false,release(){if(token.released)return;token.released=true;r.leases.delete(token);if(!r.leases.size)cleanup(r,true);}};
  r.leases.add(token);try{start(r);}catch(error){r.leases.delete(token);throw error;}return token;
 }
 function apply(r,request){
  if(r.track&&['mode','rate','echo'].includes(request.op)&&typeof r.track.applyConstraints!=='function')throw new Error('This browser track cannot apply capture constraints.');
  const result=command(r,request),effect=result.effect;
  if(effect?.type==='gain'&&r.gain)r.gain.gain.value=effect.value;
  if(effect?.type==='constraints'&&r.track){const sequence=r.sequence;Promise.resolve(r.track.applyConstraints(effect.constraints)).then(()=>{if(valid(r,sequence))settings(r);}).catch(error=>fail(r,error,sequence));}
 }
 function get(kind,index=0){if(!Number.isInteger(index)||index<0)return null;const id=raw({op:'get',kind,index}).value;if(id===null)return null;return facade(id,kind).receiver;}
 function facade(id,kind){
  let r=records.get(id);if(r)return r;
  const receiver=Object.create(kind==='camera'?Camera.prototype:Microphone.prototype);receiver.onActivity=null;receiver.onStatus=null;
  r={id,kind,receiver,generation,sequence:0,state:null,stream:null,track:null,pending:null,leases:new Set(),listeners:[],video:null,frameCallback:null,surface:null,source:null,gain:null,analyser:null,timer:null,serial:0,lastMotion:-Infinity};records.set(id,r);identities.set(receiver,r);r.state=raw({op:'state',id}).state;return r;
 }
 async function refresh(callback){
  open();if(callback!==undefined&&typeof callback!=='function')throw new TypeError('Capture.refresh extension requires a callback function.');const epoch=generation;
  if(!discovery)discovery=(async()=>{if(typeof mediaDevices?.enumerateDevices!=='function')throw new Error('Device discovery requires a secure browser with MediaDevices.');const devices=await mediaDevices.enumerateDevices();if(disposed||generation!==epoch)return false;const facts=devices.filter(d=>d.kind==='videoinput'||d.kind==='audioinput').map(d=>({kind:d.kind==='videoinput'?'camera':'microphone',key:d.deviceId||'',label:d.label||''}));raw({op:'discover',devices:facts});return true;})().finally(()=>{if(generation===epoch)discovery=null;});
  let ok=false,errorText='';try{ok=await discovery;}catch(error){errorText=String(error?.message||error);}
  if(generation===epoch&&!disposed&&callback){invoke(callback,null,[ok,errorText]);if(generation===epoch&&!disposed)options.afterEvent?.();}return ok;
 }
 class Camera{
  constructor(){throw new TypeError('Use Camera.get(index) after device discovery; Camera is not constructible.');}
  static get(index){return get('camera',index===undefined?0:index);}static get names(){return raw({op:'names',kind:'camera'}).value;}static refresh(callback){void refresh(callback);}
  setMode(width=160,height=120,fps=15,favorArea=true){apply(state(this,'camera'),{op:'mode',width,height,fps,favorArea});}
  setMotionLevel(level=50,timeout=2000){apply(state(this,'camera'),{op:'threshold',level,timeout});}
 }
 class Microphone{
  constructor(){throw new TypeError('Use Microphone.get(index) after device discovery; Microphone is not constructible.');}
  static get(index){return get('microphone',index===undefined?0:index);}static get names(){return raw({op:'names',kind:'microphone'}).value;}static refresh(callback){void refresh(callback);}
  setGain(value){apply(state(this,'microphone'),{op:'gain',value});}
  setRate(value){if(mediaDevices?.getSupportedConstraints?.().sampleRate!==true)throw new Error('This browser does not expose microphone sampleRate constraints.');apply(state(this,'microphone'),{op:'rate',value});}
  setSilenceLevel(level,timeout=2000){apply(state(this,'microphone'),{op:'threshold',level,timeout});}
  setUseEchoSuppression(value){if(mediaDevices?.getSupportedConstraints?.().echoCancellation!==true)throw new Error('This browser does not expose echo cancellation constraints.');apply(state(this,'microphone'),{op:'echo',value});}
 }
 for(const [Type,kind,names]of [[Camera,'camera',['activityLevel','currentFps','fps','height','index','motionLevel','motionTimeOut','muted','name','width']],[Microphone,'microphone',['activityLevel','gain','index','muted','name','rate','silenceLevel','silenceTimeOut','useEchoSuppression']]]){
  for(const name of names)Object.defineProperty(Type.prototype,name,{get(){const r=state(this,kind);if(name==='currentFps'&&r.video&&typeof r.video.requestVideoFrameCallback!=='function')throw new Error('Camera.currentFps requires actual presented-frame callbacks in this browser.');if(name==='rate'&&r.stream&&r.track?.getSettings?.().sampleRate===undefined)throw new Error('Browser did not report the actual microphone sample rate.');if(name==='useEchoSuppression'&&r.stream&&r.track?.getSettings?.().echoCancellation===undefined)throw new Error('Browser did not report its actual echo cancellation setting.');return snapshot(r)[name];},set(){throw new TypeError(Type.name+'.'+name+' is read-only.');}});
  Object.defineProperties(Type,{__nfIs:{value:value=>identities.get(value)?.kind===kind&&identities.get(value).generation===generation},__nfInitialize:{value:()=>unsupported(Type.name+' subclassing')}});
 }
 for(const name of ['setQuality','setKeyFrameInterval','setLoopback'])Camera.prototype[name]=function(){state(this,'camera');unsupported('Camera.'+name);};
 for(const name of ['bandwidth','quality','keyFrameInterval','loopback'])Object.defineProperty(Camera.prototype,name,{get(){state(this,'camera');unsupported('Camera.'+name);}});
 for(const name of ['setCodec','setEncodeQuality','setFramesPerPacket'])Microphone.prototype[name]=function(){state(this,'microphone');unsupported('Microphone.'+name);};
 for(const name of ['codec','encodeQuality','framesPerPacket'])Object.defineProperty(Microphone.prototype,name,{get(){state(this,'microphone');unsupported('Microphone.'+name);}});
 function attachVideo(camera,onFrame){const r=state(camera,'camera'),token=acquire(r,{onFrame});return Object.freeze({read(){if(token.released||r.generation!==generation||!r.video||r.video.readyState<2)return null;const s=snapshot(r);return{image:r.video,width:s.width,height:s.height,id:`capture:${generation}:${r.id}:${r.serial}`};},release:token.release});}
 function attachAudio(target,microphone){open();const ref=options.referenceOf?.(target);if(!ref||!options.display?.alive(target)||options.display.descriptor(target).owner!=='movie_clip')throw new TypeError('attachAudio requires a live MovieClip.');const old=audioTargets.get(target);if(microphone===null||microphone===false){old?.release();audioTargets.delete(target);return;}const r=state(microphone,'microphone');if(old?.device===r)return;const next=acquire(r,{alive:()=>options.display.alive(target)});next.device=r;old?.release();audioTargets.set(target,next);}
 function reset(){if(disposed)return;for(const r of records.values()){for(const token of r.leases)token.released=true;r.leases.clear();cleanup(r);}generation++;records.clear();audioTargets.clear();raw({op:'reset'});discovery=null;void refresh();}
 function dispose(){if(disposed)return;for(const r of records.values()){r.leases.clear();cleanup(r);}generation++;records.clear();audioTargets.clear();disposed=true;kernel.free();}
 const changed=()=>{void refresh();};mediaDevices?.addEventListener?.('devicechange',changed);
 const ready=refresh();
 return{Camera,Microphone,ready,refresh,attachAudio,attachVideo,isCamera:value=>Camera.__nfIs(value),isMicrophone:value=>Microphone.__nfIs(value),reset,dispose(){mediaDevices?.removeEventListener?.('devicechange',changed);dispose();}};
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Capture=api;
})(globalThis);
