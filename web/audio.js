/* Publication-owned, complete-file Sound playback over Web Audio.
 * Original implementation; primary API contracts:
 * https://airsdk.dev/reference/actionscript/3.0/flash/media/Sound.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/media/SoundChannel.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/media/SoundTransform.html
 * https://open-flash.github.io/mirrors/as2-language-reference/Sound.html
 * Loading reuses URLLoader's portable resources/HTTP/CORS policy. MP3 and WAV
 * preflight reserves PCM before browser decoding; browser codec support still
 * determines success. No streaming, ID3, sampleData/extract, channel matrix,
 * MovieClip sound hierarchy or native timeline audio is implied. AS2 stop and
 * transforms apply to this adapter's own channels, not a document-wide mixer.
 */
(function(global){
  'use strict';
  const fail=name=>{throw new TypeError(`${name} is not supported by the complete-file audio profile.`);};
  function message(error){try{if(typeof error?.message==='string')return error.message;return String(error);}catch(_){return '<unprintable audio error>';}}
  function number(value,name,min,max){if(typeof value!=='number'||!Number.isFinite(value)||value<min||value>max)throw new RangeError(`${name} requires a finite number from ${min} through ${max}.`);return value;}
  function create(options={}){
    const {events=global.NeoFlashEvents,net,binary}=options;
    const formats=options.formats||global.NeoFlashAudioFormats||(typeof module!=='undefined'&&module.exports?require('./audio-formats.js'):null);
    if(!events?.create||!net?.URLLoader||!binary?.toBytes||!formats?.inspect)throw new TypeError('Sound requires the shared Event, resource, ByteArray and audio-format services.');
    const maxDecodedBytes=options.maxDecodedBytes??128*1024*1024,maxChannels=options.maxChannels??32,maxSounds=options.maxSounds??256;
    if(!Number.isSafeInteger(maxDecodedBytes)||maxDecodedBytes<1||maxDecodedBytes>128*1024*1024||!Number.isInteger(maxChannels)||maxChannels<1||maxChannels>32||!Number.isInteger(maxSounds)||maxSounds<1||maxSounds>256)throw new RangeError('Audio budgets exceed the supported 128 MiB PCM, 32 channels and 256 Sound objects.');
    const sounds=new WeakMap(),channels=new WeakMap(),transforms=new WeakMap(),legacy=new WeakMap();
    const soundRecords=new Set(),playing=new Set(),decodes=new Set();
    let generation=1,disposed=false,context=null,master=null,unlocked=false,muted=false,resuming=null,decodedBytes=0,used=false,lastError='';
    const isUserGesture=options.isUserGesture||(()=>global.navigator?.userActivation?.isActive===true);
    function open(){if(disposed)throw new Error('This audio service has been disposed.');}
    function state(value,table){open();const record=table.get(value);if(!record)throw new TypeError('Expected an audio object from this publication.');if(record.generation!==generation)throw new Error('This audio object belongs to a previous publication session.');return record;}
    const hub=events.create({weakTargets:true,alive:target=>!disposed&&(sounds.get(target)||channels.get(target))?.generation===generation});
    function status(){
      let loading=0,loaded=0,pending=0,active=0;
      for(const record of soundRecords){if(record.load)loading++;if(record.buffer)loaded++;}
      for(const record of playing){if(record.started)active++;else pending++;}
      return {state:!used?'idle':lastError?'error':pending&&(!unlocked||context?.state!=='running')?'awaiting_gesture':muted?'muted':active?'running':loading?'loading':'ready',pending,active,loading,loaded,muted,unlocked,decodedBytes,error:lastError};
    }
    function changed(){options.onState?.(status());}
    function report(error){lastError=message(error);changed();if(options.onError)options.onError(error);else throw error;}
    function emit(receiver,event){
      const epoch=generation;hub.dispatch(receiver,event);
      if(!disposed&&generation===epoch)options.afterEvent?.();
      return !disposed&&generation===epoch;
    }
    function ensureContext(){
      open();if(context)return context;
      const Ctor=global.AudioContext||global.webkitAudioContext;
      if(!options.createContext&&!Ctor)throw new Error('This browser does not provide Web Audio.');
      const next=options.createContext?options.createContext():new Ctor();
      if(!next||typeof next.decodeAudioData!=='function'||typeof next.createStereoPanner!=='function')throw new Error('Sound requires AudioContext decoding, gain and stereo panning support.');
      context=next;master=context.createGain();master.gain.value=0;master.connect(context.destination);return context;
    }
    class SoundTransform {
      constructor(volume=1,pan=0){transforms.set(this,{volume:number(volume,'SoundTransform.volume',0,1),pan:number(pan,'SoundTransform.pan',-1,1)});Object.seal(this);}
      get volume(){return transform(this).volume;}set volume(value){transform(this).volume=number(value,'SoundTransform.volume',0,1);}
      get pan(){return transform(this).pan;}set pan(value){transform(this).pan=number(value,'SoundTransform.pan',-1,1);}
    }
    function transform(value){const record=transforms.get(value);if(!record)throw new TypeError('Expected SoundTransform.');return record;}
    function transformCopy(value){const record=transform(value);return {volume:record.volume,pan:record.pan};}
    for(const name of ['leftToLeft','leftToRight','rightToLeft','rightToRight'])Object.defineProperty(SoundTransform.prototype,name,{get(){fail('SoundTransform.'+name);},set(){fail('SoundTransform.'+name);}});
    function position(record){
      if(!record.started)return record.position;
      const elapsed=Math.max(0,context.currentTime-record.startedAt),segment=record.sound.buffer.duration-record.offset;
      return elapsed>=record.duration?record.sound.buffer.duration*1000:(record.offset+(segment?elapsed%segment:0))*1000;
    }
    function disconnect(record){for(const node of [record.source,record.gain,record.panner])try{node?.disconnect();}catch(_){}record.source=record.gain=record.panner=null;}
    function stop(record){
      if(record.stopped)return;record.position=position(record);record.stopped=true;playing.delete(record);
      const source=record.source;if(source){source.onended=null;try{source.stop();}catch(_){}}
      record.started=false;disconnect(record);changed();
    }
    function completed(record,source){
      if(disposed||record.generation!==generation||record.stopped||record.source!==source)return;
      record.position=record.sound.buffer.duration*1000;record.started=false;record.stopped=true;playing.delete(record);disconnect(record);changed();
      try{emit(record.receiver,new events.Event('soundComplete'));}catch(error){if(!disposed&&record.generation===generation)report(error);}
    }
    function startChannel(record){
      if(record.stopped||record.started||!record.sound.buffer||!unlocked||context?.state!=='running')return;
      const buffer=record.sound.buffer;
      if(record.offset>buffer.duration){stop(record);throw new RangeError('Sound startTime is beyond the decoded duration.');}
      const source=context.createBufferSource(),gain=context.createGain(),panner=context.createStereoPanner();
      gain.gain.value=record.transform.volume;panner.pan.value=record.transform.pan;
      // Set every level before any source is connected. QA uses volume zero or
      // a permanently silent injected destination; no OS volume changes occur.
      gain.connect(panner);panner.connect(master);source.buffer=buffer;source.loop=record.loops>0;source.loopStart=record.offset;source.loopEnd=buffer.duration;source.connect(gain);
      record.source=source;record.gain=gain;record.panner=panner;record.startedAt=context.currentTime;record.started=true;record.duration=(buffer.duration-record.offset)*(record.loops+1);
      source.onended=()=>completed(record,source);
      try{source.start(record.startedAt,record.offset);source.stop(record.startedAt+record.duration);}
      catch(error){stop(record);throw error;}
    }
    function flush(){for(const record of [...playing]){if(disposed||record.generation!==generation)break;startChannel(record);}changed();}
    function resumeFromGesture(){
      open();if(!isUserGesture())return Promise.reject(new Error('Audio needs an actual user gesture. Use Enable audio or an authored Play control.'));
      const ctx=ensureContext(),epoch=generation;used=true;lastError='';
      if(resuming)return resuming;
      resuming=Promise.resolve(ctx.resume()).then(()=>{
        if(disposed||epoch!==generation||context!==ctx)return false;
        if(ctx.state!=='running')throw new Error('The browser has not enabled audio. Use the audio control again.');
        unlocked=true;muted=false;master.gain.value=1;flush();return true;
      }).catch(error=>{if(disposed||epoch!==generation||context!==ctx)return false;throw error;}).finally(()=>{if(epoch===generation){resuming=null;changed();}});
      return resuming;
    }
    function setMuted(value){open();if(typeof value!=='boolean')throw new TypeError('Audio mute requires a Boolean.');if(!value)return resumeFromGesture();muted=true;if(master)master.gain.value=0;changed();return Promise.resolve(true);}
    class SoundChannel extends events.EventDispatcher {
      constructor(){super();fail('direct SoundChannel construction; use Sound.play()');}
      get position(){return position(state(this,channels));}
      get soundTransform(){const value=state(this,channels).transform;return new SoundTransform(value.volume,value.pan);}
      set soundTransform(value){const record=state(this,channels);record.transform=transformCopy(value);if(record.gain)record.gain.gain.value=record.transform.volume;if(record.panner)record.panner.pan.value=record.transform.pan;}
      stop(){stop(state(this,channels));}
      get leftPeak(){fail('SoundChannel.leftPeak');}get rightPeak(){fail('SoundChannel.rightPeak');}
    }
    function play(sound,startTime=0,loops=0,soundTransform=null){
      number(startTime,'Sound startTime',0,2147483647);if(!Number.isInteger(loops)||loops<0||loops>10000)throw new RangeError('Sound loops requires an integer from 0 through 10000.');
      const value=soundTransform===null?{volume:1,pan:0}:transformCopy(soundTransform);
      if(!sound.buffer&&!sound.load)throw new Error('Load a Sound before calling play().');
      if(sound.buffer&&startTime>sound.buffer.duration*1000)throw new RangeError('Sound startTime is beyond the decoded duration.');
      if(playing.size>=maxChannels)throw new RangeError(`This publication supports at most ${maxChannels} active or pending sound channels.`);
      ensureContext();used=true;
      const receiver=Object.create(SoundChannel.prototype),record={receiver,generation,sound,offset:startTime/1000,loops,transform:value,position:startTime,started:false,stopped:false,source:null,gain:null,panner:null};
      channels.set(receiver,record);Object.assign(receiver,hub.methods(receiver));Object.seal(receiver);playing.add(record);
      startChannel(record);changed();
      if((!unlocked||context.state!=='running')&&isUserGesture())resumeFromGesture().catch(error=>{if(!disposed&&record.generation===generation)report(error);});
      return receiver;
    }
    function releaseReservation(token){if(token.reserved){decodedBytes-=token.reserved;token.reserved=0;}}
    const current=token=>!disposed&&token.generation===generation&&token.sound.load===token;
    function cancelLoad(record){const token=record.load;if(!token)return;record.load=null;token.loader.close();if(!token.decoding)releaseReservation(token);for(const channel of [...playing])if(channel.sound===record&&!record.buffer)stop(channel);changed();}
    function ioFailure(record,message){
      for(const channel of [...playing])if(channel.sound===record&&!record.buffer)stop(channel);
      lastError=message;changed();
      return emit(record.receiver,new net.IOErrorEvent('ioError',false,false,message));
    }
    async function decode(token){
      try{
        const bytes=binary.toBytes(token.loader.data);token.loader.data.clear();
        if(!current(token))return;
        const ctx=ensureContext(),profile=formats.inspect(bytes,ctx.sampleRate);
        if(!Number.isSafeInteger(profile.decodedBytes)||profile.decodedBytes<1||profile.decodedBytes>maxDecodedBytes-decodedBytes)throw new RangeError('Decoded audio would exceed the publication PCM budget.');
        token.reserved=profile.decodedBytes;decodedBytes+=token.reserved;token.decoding=true;decodes.add(token);
        const buffer=await ctx.decodeAudioData(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength));
        if(!current(token))return;
        const actual=buffer.length*buffer.numberOfChannels*4;
        if(!Number.isSafeInteger(actual)||actual<1||actual>token.reserved||!Number.isFinite(buffer.duration)||buffer.duration<=0)throw new RangeError('Decoded audio exceeds its preflight PCM reservation.');
        decodedBytes-=token.reserved-actual;token.reserved=0;token.sound.buffer=buffer;token.sound.bytes=actual;token.sound.load=null;token.committed=true;changed();
        if(emit(token.sound.receiver,new events.Event('complete'))&&token.generation===generation)flush();
      }catch(error){
        if(token.committed){if(!disposed&&token.generation===generation)report(error);return;}
        if(!current(token))return;
        token.sound.load=null;releaseReservation(token);changed();
        try{ioFailure(token.sound,message(error));}catch(callbackError){if(!disposed&&token.generation===generation)report(callbackError);}
      }finally{decodes.delete(token);token.decoding=false;releaseReservation(token);}
    }
    function load(record,request,loaderContext){
      if(loaderContext!==null&&loaderContext!==undefined)fail('SoundLoaderContext');
      if(!net.URLRequest.__nfIs(request))throw new TypeError('Sound.load requires URLRequest.');
      if(request.method!=='GET'||request.data!==null)fail('Sound loading methods other than GET without request data');
      if(record.buffer)throw new Error('This Sound already contains decoded audio. Create a new Sound for another resource.');
      cancelLoad(record);used=true;lastError='';
      const loader=new net.URLLoader();loader.dataFormat='binary';record.url=request.url;
      const token={sound:record,loader,generation,reserved:0};record.load=token;
      for(const name of ['open','progress','httpStatus'])loader.addEventListener(name,event=>{if(!current(token))return;record.loadedBytes=loader.bytesLoaded;record.total=loader.bytesTotal;emit(record.receiver,event.clone());});
      loader.addEventListener('ioError',event=>{if(!current(token))return;record.loadedBytes=loader.bytesLoaded;record.total=loader.bytesTotal;record.load=null;changed();ioFailure(record,event.text);});
      loader.addEventListener('complete',()=>{if(!current(token))return;record.loadedBytes=loader.bytesLoaded;record.total=loader.bytesTotal;void decode(token);});
      try{loader.load(request);}catch(error){record.load=null;throw error;}changed();
    }
    class Sound extends events.EventDispatcher {
      constructor(request=null,loaderContext=null){
        super();open();if(soundRecords.size>=maxSounds)throw new RangeError(`This publication supports at most ${maxSounds} Sound objects before Restart.`);
        const record={receiver:this,generation,load:null,buffer:null,bytes:0,loadedBytes:0,total:0,url:null};sounds.set(this,record);soundRecords.add(record);Object.assign(this,hub.methods(this));Object.seal(this);
        if(request!==null)load(record,request,loaderContext);else if(loaderContext!==null)fail('SoundLoaderContext');
      }
      load(request,loaderContext=null){load(state(this,sounds),request,loaderContext);}
      close(){cancelLoad(state(this,sounds));}
      play(startTime=0,loops=0,soundTransform=null){return play(state(this,sounds),startTime,loops,soundTransform);}
      get length(){return (state(this,sounds).buffer?.duration||0)*1000;}
      get bytesLoaded(){return state(this,sounds).loadedBytes;}get bytesTotal(){return state(this,sounds).total;}get url(){return state(this,sounds).url;}
      get isBuffering(){return !!state(this,sounds).load;}
      get id3(){fail('Sound.id3');}get isURLInaccessible(){fail('Sound.isURLInaccessible historical sandbox state');}
      extract(){fail('Sound.extract');}loadCompressedDataFromByteArray(){fail('Sound.loadCompressedDataFromByteArray');}loadPCMFromByteArray(){fail('Sound.loadPCMFromByteArray');}
    }
    function legacyCallback(record,name,args){
      if(disposed||record.generation!==generation)return;
      const fn=record.receiver[name];if(disposed||record.generation!==generation)return;
      if(typeof fn==='function')Reflect.apply(fn,record.receiver,args);
    }
    class LegacySound {
      constructor(...args){
        open();if(args.length)fail('AS2 Sound target/MovieClip hierarchy');
        const sound=new Sound(),record={receiver:this,generation,sound,volume:100,pan:0,channel:null,owned:new Set()};legacy.set(this,record);
        sound.addEventListener('complete',()=>legacyCallback(record,'onLoad',[true]));sound.addEventListener('ioError',()=>legacyCallback(record,'onLoad',[false]));
      }
      loadSound(url,isStreaming=false){const record=state(this,legacy);if(isStreaming!==false)fail('AS2 streaming Sound');record.sound.load(new net.URLRequest(url));}
      start(offset=0,loops=1){
        const record=state(this,legacy);number(offset,'AS2 Sound secondOffset',0,2147483.647);if(!Number.isInteger(loops)||loops<1||loops>10001)throw new RangeError('AS2 Sound loops requires a total play count from 1 through 10001.');
        const channel=record.sound.play(offset*1000,loops-1,new SoundTransform(record.volume/100,record.pan/100));record.channel=channel;record.owned.add(channel);
        channel.addEventListener('soundComplete',()=>{record.owned.delete(channel);legacyCallback(record,'onSoundComplete',[]);});
      }
      stop(...args){if(args.length)fail('AS2 Sound.stop linkage identifiers');const record=state(this,legacy);for(const channel of record.owned)channel.stop();record.owned.clear();}
      getVolume(){return state(this,legacy).volume;}
      setVolume(value){const record=state(this,legacy);record.volume=number(value,'AS2 Sound volume',0,100);for(const channel of record.owned)channel.soundTransform=new SoundTransform(record.volume/100,record.pan/100);}
      getPan(){return state(this,legacy).pan;}
      setPan(value){const record=state(this,legacy);record.pan=number(value,'AS2 Sound pan',-100,100);for(const channel of record.owned)channel.soundTransform=new SoundTransform(record.volume/100,record.pan/100);}
      get position(){const record=state(this,legacy);return record.channel?record.channel.position:0;}
      get duration(){return state(this,legacy).sound.length;}
      getBytesLoaded(){return state(this,legacy).sound.bytesLoaded;}getBytesTotal(){return state(this,legacy).sound.bytesTotal;}
      attachSound(){fail('AS2 attachSound library linkage; use a portable resource path with loadSound');}
      getTransform(){fail('AS2 Sound channel-matrix transforms');}setTransform(){fail('AS2 Sound channel-matrix transforms');}get id3(){fail('AS2 Sound.id3');}
    }
    for(const[Ctor,name,table]of [[Sound,'Sound',sounds],[SoundChannel,'SoundChannel',channels],[SoundTransform,'SoundTransform',transforms],[LegacySound,'AS2 Sound',legacy]]){
      Ctor.__nfIs=value=>table.has(value);Ctor.__nfInitialize=()=>fail(name+' native subclassing');Object.defineProperty(Ctor,Symbol.hasInstance,{value:Ctor.__nfIs});
    }
    function reset(){
      open();if(master)master.gain.value=0;
      for(const record of [...playing])stop(record);
      for(const record of soundRecords){cancelLoad(record);record.buffer=null;record.bytes=0;}
      const old=context;context=null;if(master){try{master.disconnect();}catch(_){}}master=null;
      // decodeAudioData has no abort API. Keep reservations for unsettled native
      // decodes across Restart so a new session cannot overlap unbudgeted PCM.
      generation++;hub.clear();soundRecords.clear();playing.clear();decodedBytes=[...decodes].reduce((sum,token)=>sum+token.reserved,0);unlocked=false;muted=false;used=false;lastError='';resuming=null;
      try{old?.close()?.catch(()=>{});}catch(_){}changed();
    }
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({Sound,SoundChannel,SoundTransform,LegacySound,resumeFromGesture,setMuted,status,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAudio=api;
})(globalThis);
