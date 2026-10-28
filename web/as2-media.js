/* AS2 video component reference/transport adapter. All state, cue ordering,
 * seek policy, event throttling and aspect sizing execute in owned Rust. */
(function(global){
 'use strict';
 function create(wasm,options={}){
  if(typeof wasm?.ScriptAS2Media!=='function'||typeof options.video?.legacyController!=='function'||typeof options.EventDispatcher?.initialize!=='function')throw new Error('AS2 media components require Rust policy, video transport and EventDispatcher.');
  const definitionsCore=new wasm.ScriptAS2Media(),definitions=JSON.parse(definitionsCore.execute(JSON.stringify({op:'definitions'}))).value;definitionsCore.free?.();
  const records=new WeakMap(),targets=new WeakMap(),activeRecords=new Set(),cueResults=new WeakMap(),videoErrors=new WeakSet();let generation=1,disposed=false;
  class VideoError extends Error{constructor(code,message=''){if(!Number.isInteger(code))throw new TypeError('VideoError code requires an integer.');super(String(message));this.name='VideoError';Object.defineProperty(this,'code',{value:code,enumerable:true});videoErrors.add(this);}}
  Object.assign(VideoError,definitions.errors);
  function failure(error){const text=String(error?.message||error),match=/^(\d+)\|(.*)$/s.exec(text);return match?new VideoError(Number(match[1]),match[2]):error instanceof Error?error:new Error(text);}
  function state(receiver){const r=records.get(receiver);if(!r)throw new TypeError('Use VideoPlayer.attach(authoredVideo) or FLVPlayback.attach(authoredVideo).');if(disposed||r.generation!==generation||r.retired)throw new Error('This AS2 video controller belongs to a previous publication session.');return r;}
  function unsupported(name){throw new Error(`${name} is unavailable in the progressive single-viewport AS2 component profile. See docs/AS2-MEDIA-BATCH.md.`);}
  function invoke(r,request){try{return JSON.parse(r.core.execute(JSON.stringify(request)));}catch(error){throw failure(error);}}
  const playerEvents=new Set(['close','complete','cuePoint','metadataReceived','playheadUpdate','progress','ready','resize','rewind','stateChange']);
  function cue(r,point){return {name:point.name,time:point.time,type:'actionscript',parameters:r.parameters.get(point.id)};}
  function resultCue(r,value){if(value===null)return null;const array=value.points.map(point=>cue(r,point)),result={...array[value.index],array,index:value.index};cueResults.set(result,{record:r,cueEpoch:r.cueEpoch,points:value.points,index:value.index});return result;}
  function publish(r,result,revision){
   for(const effect of result.effects){if(r.retired||r.generation!==generation||r.revision!==revision)return;switch(effect.op){
    case'load':try{r.transport.load(effect.url,effect.play);}catch(error){operate(r,{op:'observe',kind:'error',sample:{...r.transport.read(),message:String(error?.message||error)}});return;}break;
    case'play':r.transport.play();break;case'pause':r.transport.pause();break;case'seek':r.transport.seek(effect.time);break;case'close':r.transport.close();break;case'volume':r.transport.volume(effect.value);break;
    case'size':r.target._width=effect.width;r.target._height=effect.height;break;
   }}
   for(const event of result.events){
    if(r.retired||r.generation!==generation||r.revision!==revision)return;
    const emitted={...event};if(event.type==='cuePoint')emitted.info=cue(r,event.info);
    if(r.component)r.component.dispatchEvent({...emitted,target:r.component,vp:0});
    if(r.retired||r.generation!==generation||r.revision!==revision)return;
    if(playerEvents.has(event.type))r.player.dispatchEvent({...emitted,target:r.player});
   }
   if(result.events.length||result.effects.length)options.afterEvent?.();
  }
  function operate(r,request){const result=invoke(r,request),revision=++r.revision;publish(r,result,revision);return result.value;}
  function sample(r){const result=invoke(r,{op:'observe',kind:'sample',sample:r.transport.read()});return result.state;}
  function load(r,url,totalTime,isLive,play){
   if(typeof url!=='string')throw new VideoError(VideoError.INVALID_CONTENT_PATH,'Content path requires a string.');
   if(totalTime!==undefined&&totalTime!==null)unsupported('Manual totalTime override; duration comes from native metadata');
   if(isLive!==undefined&&isLive!==false)unsupported('Live video');
   const result=invoke(r,{op:'load',url,play});r.parameters.clear();r.cueEpoch++;publish(r,result,++r.revision);
  }
  class VideoPlayer{
   constructor(){throw new TypeError('Use VideoPlayer.attach(authoredVideo); Flash component linkage/constructor semantics are not available.');}
   static attach(target){return attach(target,false);}
   load(url,totalTime,isLive){load(state(this),url,totalTime,isLive,false);}
   play(url,totalTime,isLive){const r=state(this);if(url!==undefined)load(r,url,totalTime,isLive,true);else operate(r,{op:'play'});}
   pause(){operate(state(this),{op:'pause'});}
   stop(){operate(state(this),{op:'stop'});}
   close(){operate(state(this),{op:'close'});}
   seek(seconds){operate(state(this),{op:'seek',time:seconds});}
   setSize(width,height){operate(state(this),{op:'size',width,height});}
   setScale(x,y){const r=state(this);operate(r,{op:'scale',x,y,width:r.target._width,height:r.target._height,oldX:r.target._xscale,oldY:r.target._yscale});}
  }
  class FLVPlayback extends VideoPlayer{
   static attach(target){return attach(target,true);}
   getVideoPlayer(index=0){const r=state(this);if(index!==0)unsupported('Multiple video players');return r.player;}
   closeVideoPlayer(index=0){state(this);if(index===0)throw new VideoError(VideoError.DELETE_DEFAULT_PLAYER,'The default video player cannot be deleted; call stop() or getVideoPlayer(0).close().');unsupported('Multiple video players');}
   seekSeconds(time){this.seek(time);}
   seekPercent(percent){operate(state(this),{op:'seek',time:percent,percent:true});}
   addASCuePoint(value,name,parameters){const r=state(this);if(value!==null&&typeof value==='object'){parameters=value.parameters;name=value.name;value=value.time;}const result=operate(r,{op:'cue_add',time:value,name});const point=result.points[result.index];r.parameters.set(point.id,parameters);return resultCue(r,result);}
   removeASCuePoint(value){const r=state(this);return resultCue(r,operate(r,{op:'cue_remove',...query(value)}));}
   findCuePoint(value,type='all'){const r=state(this);return resultCue(r,invoke(r,{op:'cue_find',...query(value),type}).value);}
   findNearestCuePoint(value,type='all'){const r=state(this);return resultCue(r,invoke(r,{op:'cue_find',...query(value),type,nearest:true}).value);}
   findNextCuePointWithName(value){const r=state(this),previous=cueResults.get(value);if(!previous||previous.record!==r||previous.cueEpoch!==r.cueEpoch)throw new VideoError(VideoError.ILLEGAL_CUE_POINT,'Use a cue object returned by this controller.');return resultCue(r,invoke(r,{op:'cue_next',points:previous.points,index:previous.index}).value);}
  }
  function query(value){if(typeof value==='number')return{time:value};if(typeof value==='string')return{name:value};if(value!==null&&typeof value==='object')return{time:value.time??null,name:value.name??null};throw new VideoError(VideoError.ILLEGAL_CUE_POINT,'Cue lookup requires a name, time or cue object.');}
  const readOnly=['state','stateResponsive','connected','metadata','videoWidth','videoHeight','isRTMP','isLive'];
  for(const name of readOnly)Object.defineProperty(VideoPlayer.prototype,name,{get(){const value=sample(state(this))[name];return name==='metadata'&&value===null?undefined:value;},set(){throw new TypeError(`VideoPlayer.${name} is read-only in this profile.`);}});
  for(const name of ['bytesLoaded','bytesTotal'])Object.defineProperty(VideoPlayer.prototype,name,{get(){const value=sample(state(this))[name];if(value===null)unsupported('HTTP progressive byte counts; the browser owns buffering');return value;}});
  for(const name of ['autoRewind','autoSize','maintainAspectRatio','volume','playheadUpdateInterval'])Object.defineProperty(VideoPlayer.prototype,name,{get(){return sample(state(this))[name];},set(value){operate(state(this),{op:'setting',name,value});}});
  Object.defineProperties(VideoPlayer.prototype,{
   playheadTime:{get(){return sample(state(this)).playheadTime;},set(value){this.seek(value);}},
   totalTime:{get(){return sample(state(this)).totalTime;},set(){unsupported('Manual totalTime override');}},
   url:{get(){return sample(state(this)).url;},set(value){const r=state(this);load(r,value,undefined,undefined,sample(r).autoPlay);}},
   width:{get(){return state(this).target._width;},set(value){const r=state(this);this.setSize(value,r.target._height);}},height:{get(){return state(this).target._height;},set(value){const r=state(this);this.setSize(r.target._width,value);}},
   scaleX:{get(){return state(this).target._xscale;},set(value){this.setScale(value,state(this).target._yscale);}},scaleY:{get(){return state(this).target._yscale;},set(value){this.setScale(state(this).target._xscale,value);}},
   x:{get(){return state(this).target._x;},set(value){state(this).target._x=value;}},y:{get(){return state(this).target._y;},set(value){state(this).target._y=value;}},visible:{get(){return state(this).target._visible;},set(value){if(typeof value!=='boolean')throw new TypeError('visible requires Boolean.');state(this).target._visible=value;}},
   version:{get(){state(this);return 'NeoFlash AS2 media 0.1';}},
  });
  for(const name of ['metadataLoaded','buffering','paused','playing','stopped','preferredWidth','preferredHeight','playheadPercentage'])Object.defineProperty(FLVPlayback.prototype,name,{get(){return sample(state(this))[name];},set(value){if(name==='playheadPercentage')return this.seekPercent(value);throw new TypeError(`FLVPlayback.${name} is read-only.`);}});
  Object.defineProperties(FLVPlayback.prototype,{
   autoPlay:{get(){return sample(state(this)).autoPlay;},set(value){operate(state(this),{op:'setting',name:'autoPlay',value});}},
   contentPath:{get(){return this.url;},set(value){this.url=value;}},
  });
  for(const name of ['bufferTime','idleTimeout','ncMgr','transform','progressInterval'])Object.defineProperty(VideoPlayer.prototype,name,{get(){state(this);unsupported('VideoPlayer.'+name);},set(){state(this);unsupported('VideoPlayer.'+name);}});
  for(const name of ['activeVideoPlayerIndex','visibleVideoPlayerIndex','cuePoints','bitrate','skin','skinAutoHide','seekToPrevOffset','scrubbing','playButton','pauseButton','playPauseButton','stopButton','muteButton','backButton','forwardButton','seekBar','seekBarInterval','seekBarScrubTolerance','volumeBar','volumeBarInterval','volumeBarScrubTolerance','bufferingBar','bufferingBarHidesAndDisablesOthers'])Object.defineProperty(FLVPlayback.prototype,name,{get(){state(this);unsupported('FLVPlayback.'+name);},set(){state(this);unsupported('FLVPlayback.'+name);}});
  for(const name of ['bringVideoPlayerToFront','setFLVCuePointEnabled','isFLVCuePointEnabled','seekToNavCuePoint','seekToNextNavCuePoint','seekToPrevNavCuePoint'])Object.defineProperty(FLVPlayback.prototype,name,{value(){state(this);unsupported('FLVPlayback.'+name);}});
  Object.defineProperty(VideoPlayer,'version',{value:'NeoFlash AS2 media 0.1'});
  Object.assign(VideoPlayer,definitions.states);for(const[name,value]of Object.entries(definitions.states))if(name!=='RESIZING')FLVPlayback[name]=value;Object.assign(FLVPlayback,definitions.cueTypes);
  for(const[Type,predicate]of [[VideoPlayer,value=>records.has(value)],[FLVPlayback,value=>records.has(value)&&records.get(value).component===value],[VideoError,value=>videoErrors.has(value)]])Object.defineProperties(Type,{__nfIs:{value:predicate},__nfInitialize:{value:()=>unsupported(Type.name+' native subclassing')}});
  function attach(target,component){
   if(disposed)throw new Error('AS2 video component service disposed.');if(!target||(typeof target!=='object'&&typeof target!=='function'))throw new TypeError('attach requires an authored Video instance.');
   const previous=targets.get(target);if(previous&&!previous.retired&&previous.generation===generation){if(component&&previous.component)return previous.component;if(!component)return previous.player;throw new Error('This Video already has a VideoPlayer controller; attach FLVPlayback first.');}
   const core=new wasm.ScriptAS2Media(),r={core,target,parameters:new Map(),generation,retired:false,revision:0,cueEpoch:0,transport:null,player:Object.create(VideoPlayer.prototype),component:component?Object.create(FLVPlayback.prototype):null};
   try{invoke(r,{op:'init',width:target._width,height:target._height});r.transport=options.video.legacyController(target,(kind,sample)=>{if(!r.retired&&r.generation===generation)operate(r,{op:'observe',kind,sample});});}catch(error){core.free?.();throw failure(error);}
   for(const receiver of [r.player,r.component].filter(Boolean)){records.set(receiver,r);options.EventDispatcher.initialize(receiver);}
   targets.set(target,r);activeRecords.add(r);return r.component||r.player;
  }
  function reset(){if(disposed)return;for(const r of activeRecords){r.retired=true;r.transport.dispose();r.core.free?.();r.parameters.clear();}activeRecords.clear();generation++;}
  function dispose(){if(disposed)return;reset();disposed=true;}
  return{builtins:{'mx.video.VideoPlayer':VideoPlayer,'mx.video.FLVPlayback':FLVPlayback,'mx.video.VideoError':VideoError},reset,dispose};
 }
 const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Media=api;
})(typeof window==='undefined'?globalThis:window);
