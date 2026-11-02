/* Publication-owned, complete-file Sound playback over Web Audio.
 * Original implementation; primary API contracts:
 * https://airsdk.dev/reference/actionscript/3.0/flash/media/Sound.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/media/SoundChannel.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/media/SoundTransform.html
 * https://open-flash.github.io/mirrors/as2-language-reference/Sound.html
 * Loading reuses URLLoader's portable resources/HTTP/CORS policy. MP3 and WAV
 * preflight reserves PCM before browser decoding; browser codec support still
 * determines success. AS2 metadata/channel matrices use the shared Rust core.
 * No streaming, AS3 ID3, sampleData/extract, AS3 channel matrix or
 * MovieClip sound hierarchy is implied. AS2 stop and transforms apply to this
 * adapter's own channels, not a document-wide mixer.
 * Timeline (frame) sounds: the Rust Player decides which sound starts, stops or
 * seeks and at which offset (SoundCommand); `timeline()` only decodes each
 * embedded resource once and schedules Web Audio sources on the shared bus.
 */
(function(global){
  'use strict';
  const fail=name=>{throw new TypeError(`${name} is not supported by the complete-file audio profile.`);};
  function message(error){try{if(typeof error?.message==='string')return error.message;return String(error);}catch(_){return '<unprintable audio error>';}}
  function number(value,name,min,max){if(typeof value!=='number'||!Number.isFinite(value)||value<min||value>max)throw new RangeError(`${name} requires a finite number from ${min} through ${max}.`);return value;}
  function create(options={}){
    const {events=global.NeoFlashEvents,net,binary,player,wasm}=options;
    const formats=options.formats||global.NeoFlashAudioFormats||(typeof module!=='undefined'&&module.exports?require('./audio-formats.js'):null);
    if(!events?.create||!net?.URLLoader||!binary?.toBytes||!formats?.inspect)throw new TypeError('Sound requires the shared Event, resource, ByteArray and audio-format services.');
    const maxDecodedBytes=options.maxDecodedBytes??128*1024*1024,maxChannels=options.maxChannels??32,maxSounds=options.maxSounds??256;
    if(!Number.isSafeInteger(maxDecodedBytes)||maxDecodedBytes<1||maxDecodedBytes>128*1024*1024||!Number.isInteger(maxChannels)||maxChannels<1||maxChannels>32||!Number.isInteger(maxSounds)||maxSounds<1||maxSounds>256)throw new RangeError('Audio budgets exceed the supported 128 MiB PCM, 32 channels and 256 Sound objects.');
    const sounds=new WeakMap(),channels=new WeakMap(),transforms=new WeakMap(),legacy=new WeakMap(),id3Infos=new WeakSet(),loaderContexts=new WeakMap();
    const soundRecords=new Set(),playing=new Set(),decodes=new Set();
    let generation=1,disposed=false,context=null,master=null,unlocked=false,muted=false,resuming=null,decodedBytes=0,used=false,lastError='',mixerBufferTime=0,mixerTransform={volume:1,pan:0,leftToLeft:1,leftToRight:0,rightToLeft:0,rightToRight:1};
    class ID3Info {
      constructor(){
        this.album=null;this.artist=null;this.comment=null;this.genre=null;this.songName=null;this.track=null;this.year=null;
        id3Infos.add(this);
      }
    }
    function createId3Info(data=null){
      const info=new ID3Info();
      if(data){
        info.album=data.album??null;info.artist=data.artist??null;info.comment=data.comment??null;
        info.genre=data.genre??null;info.songName=data.songName??null;info.track=data.track??null;info.year=data.year??null;
      }
      return info;
    }
    class SoundLoaderContext {
      constructor(bufferTime=1000,checkPolicyFile=false){
        loaderContexts.set(this,{bufferTime:Number(bufferTime),checkPolicyFile:Boolean(checkPolicyFile)});
      }
      get bufferTime(){const r=loaderContexts.get(this);if(!r)throw new TypeError('Expected SoundLoaderContext.');return r.bufferTime;}
      set bufferTime(value){const r=loaderContexts.get(this);if(!r)throw new TypeError('Expected SoundLoaderContext.');r.bufferTime=Number(value);}
      get checkPolicyFile(){const r=loaderContexts.get(this);if(!r)throw new TypeError('Expected SoundLoaderContext.');return r.checkPolicyFile;}
      set checkPolicyFile(value){const r=loaderContexts.get(this);if(!r)throw new TypeError('Expected SoundLoaderContext.');r.checkPolicyFile=Boolean(value);}
    }
    const isUserGesture=options.isUserGesture||(()=>global.navigator?.userActivation?.isActive===true);
    function open(){if(disposed)throw new Error('This audio service has been disposed.');}
    function state(value,table){open();const record=table.get(value);if(!record)throw new TypeError('Expected an audio object from this publication.');if(record.generation!==generation)throw new Error('This audio object belongs to a previous publication session.');return record;}
    const hub=events.create({weakTargets:true,alive:target=>!disposed&&(sounds.get(target)||channels.get(target))?.generation===generation});
    function status(){
      let loading=0,loaded=0,pending=0,active=0;
      for(const record of soundRecords){if(record.load)loading++;if(record.buffer)loaded++;}
      for(const record of playing){if(record.started)active++;else pending++;}
      for(const voice of voices){if(voice.source)active++;else pending++;}
      pending+=blockedEvents;
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
    function panGains(volume,pan){
      return {
        leftToLeft:pan<=0?volume:volume*(1-pan),
        leftToRight:0,
        rightToLeft:0,
        rightToRight:pan>=0?volume:volume*(1+pan),
      };
    }
    class SoundTransform {
      constructor(volume=1,pan=0){
        const v=number(volume,'SoundTransform.volume',0,1),p=number(pan,'SoundTransform.pan',-1,1);
        transforms.set(this,{volume:v,pan:p,...panGains(v,p)});Object.seal(this);
      }
      get volume(){return transform(this).volume;}
      set volume(value){const r=transform(this);r.volume=number(value,'SoundTransform.volume',0,1);Object.assign(r,panGains(r.volume,r.pan));}
      get pan(){return transform(this).pan;}
      set pan(value){const r=transform(this);r.pan=number(value,'SoundTransform.pan',-1,1);Object.assign(r,panGains(r.volume,r.pan));}
    }
    function transform(value){const record=transforms.get(value);if(!record)throw new TypeError('Expected SoundTransform.');return record;}
    function transformCopy(value){const r=transform(value);return {volume:r.volume,pan:r.pan,leftToLeft:r.leftToLeft,leftToRight:r.leftToRight,rightToLeft:r.rightToLeft,rightToRight:r.rightToRight};}
    for(const name of ['leftToLeft','leftToRight','rightToLeft','rightToRight'])Object.defineProperty(SoundTransform.prototype,name,{
      get(){return transform(this)[name];},
      set(value){
        const r=transform(this);
        r[name]=number(value,'SoundTransform.'+name,0,1);
        const leftOut=r.leftToLeft+r.rightToLeft,rightOut=r.leftToRight+r.rightToRight;
        r.volume=Math.max(0,Math.min(1,Math.max(leftOut,rightOut)));
        if(r.leftToRight===0&&r.rightToLeft===0&&r.volume>0)r.pan=Math.max(-1,Math.min(1,(r.rightToRight-r.leftToLeft)/r.volume));
      }
    });
    function position(record){
      if(!record.started)return record.position;
      const elapsed=Math.max(0,context.currentTime-record.startedAt),segment=record.sound.buffer.duration-record.offset;
      return elapsed>=record.duration?record.sound.buffer.duration*1000:(record.offset+(segment?elapsed%segment:0))*1000;
    }
    function disconnect(record){for(const node of [record.source,record.gain,record.panner,...(record.matrixNodes||[])])try{node?.disconnect();}catch(_){}record.source=record.gain=record.panner=null;record.matrixNodes=null;record.matrixGains=null;}
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
    function connectMatrix(record,gain,result){
      if(typeof context.createChannelSplitter!=='function'||typeof context.createChannelMerger!=='function')throw new Error('AS2 Sound.setTransform requires Web Audio channel splitter/merger support.');
      const splitter=context.createChannelSplitter(2),merger=context.createChannelMerger(2),gains=result.gains.map(value=>{const node=context.createGain();node.gain.value=value;return node;});
      // Only graph wiring lives here; Rust computes all matrix coefficients.
      // Web Audio duplicates mono into two logical inputs before the matrix.
      gain.disconnect();record.panner?.disconnect();record.panner=null;
      gain.gain.value=1;gain.channelCount=2;gain.channelCountMode='explicit';gain.channelInterpretation='speakers';gain.connect(splitter);
      for(const [i,input,output] of [[0,0,0],[1,1,0],[2,0,1],[3,1,1]]){splitter.connect(gains[i],input);gains[i].connect(merger,0,output);}
      merger.connect(master);record.matrixNodes=[splitter,merger,...gains];record.matrixGains=gains;
    }
    function startChannel(record){
      if(record.stopped||record.started||!record.sound.buffer||!unlocked||context?.state!=='running')return;
      const buffer=record.sound.buffer;
      if(record.offset>buffer.duration){stop(record);throw new RangeError('Sound startTime is beyond the decoded duration.');}
      const source=context.createBufferSource(),gain=context.createGain(),panner=record.matrix?null:context.createStereoPanner();
      gain.gain.value=record.matrix?1:record.transform.volume;if(panner)panner.pan.value=record.transform.pan;
      // Set every level before any source is connected. QA uses volume zero or
      // a permanently silent injected destination; no OS volume changes occur.
      if(record.matrix)connectMatrix(record,gain,record.matrix);
      else{gain.connect(panner);panner.connect(master);}
      source.buffer=buffer;source.loop=record.loops>0;source.loopStart=record.offset;source.loopEnd=buffer.duration;source.connect(gain);
      record.source=source;record.gain=gain;record.panner=panner;record.startedAt=context.currentTime;record.started=true;record.duration=(buffer.duration-record.offset)*(record.loops+1);
      source.onended=()=>completed(record,source);
      try{source.start(record.startedAt,record.offset);source.stop(record.startedAt+record.duration);}
      catch(error){stop(record);throw error;}
    }
    function flush(){blockedEvents=0;flushVoices();for(const record of [...playing]){if(disposed||record.generation!==generation)break;startChannel(record);}changed();}
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
    // Private AS2 capture transport into the existing publication mute/output bus.
    function captureBus(){open();if(!isUserGesture())throw new Error('Microphone playback needs an actual user gesture.');const ctx=ensureContext();return {context:ctx,destination:master,ready:resumeFromGesture()};}
    function setMuted(value){open();if(typeof value!=='boolean')throw new TypeError('Audio mute requires a Boolean.');if(!value)return resumeFromGesture();muted=true;if(master)master.gain.value=0;changed();return Promise.resolve(true);}
    function unpackCompressed(packed){
      const view=new DataView(packed.buffer,packed.byteOffset,packed.byteLength);
      const metaLen=view.getUint32(0,true);
      const meta=JSON.parse(new TextDecoder().decode(packed.subarray(4,4+metaLen)));
      return {meta,pcm:packed.slice(4+metaLen)};
    }
    function createPcmAudioBuffer(pcm,sampleCount,sampleRate=44100){
      const duration=sampleCount/sampleRate;
      const view=new DataView(pcm.buffer,pcm.byteOffset,pcm.byteLength);
      const ch0=new Float32Array(sampleCount),ch1=new Float32Array(sampleCount);
      for(let i=0;i<sampleCount;i++){
        ch0[i]=view.getFloat32(i*8,true);
        ch1[i]=view.getFloat32(i*8+4,true);
      }
      try{
        const ctx=ensureContext();
        if(typeof ctx?.createBuffer==='function'&&sampleCount>0){
          const buf=ctx.createBuffer(2,sampleCount,sampleRate);
          if(typeof buf.getChannelData==='function'){
            buf.getChannelData(0).set(ch0);
            if(buf.numberOfChannels>1)buf.getChannelData(1).set(ch1);
          }
          return buf;
        }
      }catch(_){}
      return {numberOfChannels:2,sampleRate,length:sampleCount,duration,getChannelData(ch){return ch===1?ch1:ch0;}};
    }
    class SoundChannel extends events.EventDispatcher {
      constructor(){super();fail('direct SoundChannel construction; use Sound.play()');}
      get position(){return position(state(this,channels));}
      get soundTransform(){const value=state(this,channels).transform,st=new SoundTransform(value.volume,value.pan);Object.assign(transforms.get(st),value);return st;}
      set soundTransform(value){const record=state(this,channels);record.transform=transformCopy(value);if(record.gain)record.gain.gain.value=record.transform.volume;if(record.panner)record.panner.pan.value=record.transform.pan;}
      stop(){stop(state(this,channels));}
      get leftPeak(){
        const r=state(this,channels),t=r.transform;
        if(r.stopped)return 0;
        if(r.sound.pcmBytes&&typeof wasm?.as3_sound_channel_peaks_json==='function'){
          return JSON.parse(wasm.as3_sound_channel_peaks_json(r.sound.pcmBytes,position(r),JSON.stringify(t),JSON.stringify(mixerTransform))).leftPeak;
        }
        return Math.max(0,Math.min(1,t.leftToLeft+t.rightToLeft));
      }
      get rightPeak(){
        const r=state(this,channels),t=r.transform;
        if(r.stopped)return 0;
        if(r.sound.pcmBytes&&typeof wasm?.as3_sound_channel_peaks_json==='function'){
          return JSON.parse(wasm.as3_sound_channel_peaks_json(r.sound.pcmBytes,position(r),JSON.stringify(t),JSON.stringify(mixerTransform))).rightPeak;
        }
        return Math.max(0,Math.min(1,t.rightToRight+t.leftToRight));
      }
    }
    function play(sound,startTime=0,loops=0,soundTransform=null,matrix=null){
      number(startTime,'Sound startTime',0,2147483647);if(!Number.isInteger(loops)||loops<0||loops>10000)throw new RangeError('Sound loops requires an integer from 0 through 10000.');
      const value=soundTransform===null?{volume:1,pan:0,leftToLeft:1,leftToRight:0,rightToLeft:0,rightToRight:1}:transformCopy(soundTransform);
      if(!sound.buffer&&!sound.load)throw new Error('Load a Sound before calling play().');
      if(sound.buffer&&startTime>sound.buffer.duration*1000)throw new RangeError('Sound startTime is beyond the decoded duration.');
      if(playing.size>=maxChannels)throw new RangeError(`This publication supports at most ${maxChannels} active or pending sound channels.`);
      ensureContext();used=true;
      const receiver=Object.create(SoundChannel.prototype),record={receiver,generation,sound,offset:startTime/1000,loops,transform:value,matrix,position:startTime,started:false,stopped:false,source:null,gain:null,panner:null};
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
        const metadata=token.sound.legacyOwner&&typeof wasm?.as2_sound_metadata==='function'?JSON.parse(wasm.as2_sound_metadata(bytes)):null;
        if(typeof wasm?.as3_sound_inspect_id3_json==='function'){
          try{const id3=JSON.parse(wasm.as3_sound_inspect_id3_json(bytes));if(id3)token.sound.id3Info=createId3Info(id3);}catch(_){}
        }
        if(typeof wasm?.as3_sound_decode_compressed==='function'){
          try{token.sound.pcmBytes=unpackCompressed(wasm.as3_sound_decode_compressed(bytes)).pcm;}catch(_){}
        }
        if(!Number.isSafeInteger(profile.decodedBytes)||profile.decodedBytes<1||profile.decodedBytes>maxDecodedBytes-decodedBytes)throw new RangeError('Decoded audio would exceed the publication PCM budget.');
        token.reserved=profile.decodedBytes;decodedBytes+=token.reserved;token.decoding=true;decodes.add(token);
        const buffer=await ctx.decodeAudioData(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength));
        if(!current(token))return;
        const actual=buffer.length*buffer.numberOfChannels*4;
        if(!Number.isSafeInteger(actual)||actual<1||actual>token.reserved||!Number.isFinite(buffer.duration)||buffer.duration<=0)throw new RangeError('Decoded audio exceeds its preflight PCM reservation.');
        decodedBytes-=token.reserved-actual;token.reserved=0;token.sound.buffer=buffer;token.sound.bytes=actual;token.sound.load=null;token.committed=true;
        if(!token.sound.pcmBytes&&typeof buffer.getChannelData==='function'){
          const len=buffer.length>>>0,c0=buffer.getChannelData(0),c1=buffer.numberOfChannels>1?buffer.getChannelData(1):c0;
          const pcm=new Uint8Array(len*8),dv=new DataView(pcm.buffer);
          for(let i=0;i<len;i++){dv.setFloat32(i*8,c0[i]||0,true);dv.setFloat32(i*8+4,c1[i]||0,true);}
          token.sound.pcmBytes=pcm;
        }
        changed();
        if(metadata){
          for(const warning of metadata.warnings)console.warn('NeoFlash AS2 Sound:',warning);
          const owner=token.sound.legacyOwner;
          for(const snapshot of metadata.snapshots){
            if(disposed||token.generation!==generation||token.sound.loadEpoch!==token.loadEpoch)return;
            owner.id3=snapshot;legacyCallback(owner,'onID3',[]);
            if(disposed||token.generation!==generation||token.sound.loadEpoch!==token.loadEpoch)return;
            options.afterEvent?.();
          }
        }
        if(disposed||token.generation!==generation||token.sound.loadEpoch!==token.loadEpoch)return;
        if(emit(token.sound.receiver,new events.Event('complete'))&&token.generation===generation)flush();
      }catch(error){
        if(token.committed){if(!disposed&&token.generation===generation)report(error);return;}
        if(!current(token))return;
        token.sound.load=null;releaseReservation(token);changed();
        try{ioFailure(token.sound,message(error));}catch(callbackError){if(!disposed&&token.generation===generation)report(callbackError);}
      }finally{decodes.delete(token);token.decoding=false;releaseReservation(token);}
    }
    function load(record,request,loaderContext){
      if(loaderContext!==null&&loaderContext!==undefined&&!loaderContexts.has(loaderContext))throw new TypeError('Sound.load loaderContext requires SoundLoaderContext or null.');
      if(!net.URLRequest.__nfIs(request))throw new TypeError('Sound.load requires URLRequest.');
      if(request.method!=='GET'||request.data!==null)fail('Sound loading methods other than GET without request data');
      if(record.buffer)throw new Error('This Sound already contains decoded audio. Create a new Sound for another resource.');
      const lc=loaderContext?loaderContexts.get(loaderContext):null;
      record.bufferTime=lc?lc.bufferTime:mixerBufferTime;
      record.checkPolicyFile=lc?Boolean(lc.checkPolicyFile):false;
      cancelLoad(record);used=true;lastError='';
      const loader=new net.URLLoader();loader.dataFormat='binary';record.url=request.url;
      const token={sound:record,loader,generation,reserved:0,loadEpoch:record.loadEpoch=(record.loadEpoch||0)+1};record.load=token;
      for(const name of ['open','progress','httpStatus'])loader.addEventListener(name,event=>{if(!current(token))return;record.loadedBytes=loader.bytesLoaded;record.total=loader.bytesTotal;emit(record.receiver,event.clone());});
      loader.addEventListener('ioError',event=>{if(!current(token))return;record.loadedBytes=loader.bytesLoaded;record.total=loader.bytesTotal;record.load=null;changed();ioFailure(record,event.text);});
      loader.addEventListener('complete',()=>{if(!current(token))return;record.loadedBytes=loader.bytesLoaded;record.total=loader.bytesTotal;void decode(token);});
      try{loader.load(request);}catch(error){record.load=null;throw error;}changed();
    }
    class Sound extends events.EventDispatcher {
      constructor(request=null,loaderContext=null){
        super();open();if(soundRecords.size>=maxSounds)throw new RangeError(`This publication supports at most ${maxSounds} Sound objects before Restart.`);
        if(loaderContext!==null&&loaderContext!==undefined&&!loaderContexts.has(loaderContext))throw new TypeError('Sound constructor loaderContext requires SoundLoaderContext or null.');
        const record={receiver:this,generation,load:null,buffer:null,pcmBytes:null,extractPosition:0,id3Info:createId3Info(),bytes:0,loadedBytes:0,total:0,url:null};sounds.set(this,record);soundRecords.add(record);Object.assign(this,hub.methods(this));Object.seal(this);
        if(request!==null)load(record,request,loaderContext);
      }
      load(request,loaderContext=null){load(state(this,sounds),request,loaderContext);}
      close(){cancelLoad(state(this,sounds));}
      play(startTime=0,loops=0,soundTransform=null){return play(state(this,sounds),startTime,loops,soundTransform);}
      get length(){return (state(this,sounds).buffer?.duration||0)*1000;}
      get bytesLoaded(){return state(this,sounds).loadedBytes;}get bytesTotal(){return state(this,sounds).total;}get url(){return state(this,sounds).url;}
      get isBuffering(){return !!state(this,sounds).load;}
      get id3(){return state(this,sounds).id3Info;}
      get isURLInaccessible(){state(this,sounds);return false;}
      extract(target,length,startPosition=-1){
        const record=state(this,sounds);
        binary.toBytes(target);
        const requested=Math.floor(Number(length));
        if(!Number.isFinite(requested)||requested<0)throw new RangeError('Sound.extract length must be nonnegative.');
        if(record.pcmBytes&&typeof wasm?.as3_sound_extract_f32==='function'){
          const out=wasm.as3_sound_extract_f32(record.pcmBytes,requested,Number(startPosition),record.extractPosition||0,true);
          const view=new DataView(out.buffer,out.byteOffset,out.byteLength);
          const extractedFrames=Math.floor(view.getFloat64(0,true));
          record.extractPosition=Math.floor(view.getFloat64(8,true));
          for(let i=0;i<extractedFrames*2;i++)target.writeFloat(view.getFloat32(16+i*4,true));
          return extractedFrames;
        }
        const totalFrames=record.pcmBytes?(record.pcmBytes.byteLength>>>3):(record.buffer?Math.floor(record.buffer.duration*44100):0);
        const startFrame=startPosition<0?(record.extractPosition||0):Math.floor(Number(startPosition));
        if(startFrame<0||startFrame>=totalFrames||requested===0){
          record.extractPosition=Math.max(0,totalFrames);
          return 0;
        }
        const extractedFrames=Math.min(requested,Math.max(0,totalFrames-startFrame));
        record.extractPosition=startFrame+extractedFrames;
        for(let i=0;i<extractedFrames*2;i++)target.writeFloat(0);
        return extractedFrames;
      }
      loadCompressedDataFromByteArray(bytes,bytesLength){
        const record=state(this,sounds),raw=binary.toBytes(bytes),pos=Number(bytes.position)||0,len=Math.trunc(Number(bytesLength));
        if(!Number.isFinite(len)||len<=0||pos+len>raw.byteLength)throw new RangeError('Sound.loadCompressedDataFromByteArray bytesLength exceeds available bytes.');
        const slice=raw.subarray(pos,pos+len);
        bytes.position=pos+len;
        if(typeof wasm?.as3_sound_decode_compressed!=='function')throw new Error('Rust compressed audio decoder is unavailable.');
        let unpacked;try{unpacked=unpackCompressed(wasm.as3_sound_decode_compressed(slice));}catch(err){const e=new Error(message(err));e.name='IOError';throw e;}
        const pcm=unpacked.pcm,sampleCount=pcm.byteLength>>>3;
        cancelLoad(record);
        record.pcmBytes=pcm;record.extractPosition=0;record.bytes=pcm.byteLength;record.loadedBytes=len;record.total=len;
        record.buffer=createPcmAudioBuffer(pcm,sampleCount,44100);
        if(unpacked.meta?.id3){
          record.id3Info=createId3Info(unpacked.meta.id3);
          emit(this,new events.Event('id3'));
        }else if(typeof wasm?.as3_sound_inspect_id3_json==='function'){
          try{const id3=JSON.parse(wasm.as3_sound_inspect_id3_json(slice));if(id3){record.id3Info=createId3Info(id3);if([id3.album,id3.artist,id3.comment,id3.genre,id3.songName,id3.track,id3.year].some(v=>v!==null&&v!==undefined))emit(this,new events.Event('id3'));}}catch(_){}
        }
      }
      loadPCMFromByteArray(bytes,samples,format='float',stereo=true,sampleRate=44100){
        const record=state(this,sounds),raw=binary.toBytes(bytes),pos=Number(bytes.position)||0,count=Math.trunc(Number(samples));
        if(!Number.isFinite(count)||count<0)throw new RangeError('Sound.loadPCMFromByteArray samples must be nonnegative.');
        if(typeof stereo!=='boolean')throw new TypeError('Sound.loadPCMFromByteArray stereo requires a Boolean.');
        const fmt=String(format);
        if(fmt!=='float'&&fmt!=='short'){const err=new RangeError(`Unsupported PCM format: ${fmt}`);err.name='ArgumentError';throw err;}
        const bytesPerFrame=(fmt==='short'?2:4)*(stereo?2:1),byteLen=count*bytesPerFrame;
        if(pos+byteLen>raw.byteLength){const err=new Error('End of file was encountered.');err.name='EOFError';throw err;}
        const slice=raw.subarray(pos,pos+byteLen);
        bytes.position=pos+byteLen;
        if(typeof wasm?.as3_sound_decode_pcm!=='function')throw new Error('Rust PCM audio decoder is unavailable.');
        let pcm;try{pcm=wasm.as3_sound_decode_pcm(slice,count,fmt,stereo,Number(sampleRate),bytes.endian==='littleEndian');}catch(err){const e=new RangeError(message(err));e.name='ArgumentError';throw e;}
        const sampleCount=pcm.byteLength>>>3;
        cancelLoad(record);
        record.pcmBytes=pcm;record.extractPosition=0;record.bytes=pcm.byteLength;record.loadedBytes=byteLen;record.total=byteLen;
        record.buffer=createPcmAudioBuffer(pcm,sampleCount,44100);
      }
    }
    function legacyCallback(record,name,args){
      if(disposed||record.generation!==generation)return;
      const fn=record.receiver[name];if(disposed||record.generation!==generation)return;
      if(typeof fn==='function')Reflect.apply(fn,record.receiver,args);
    }
    function legacyMatrix(record,patch={}){
      if(typeof wasm?.as2_sound_transform!=='function')throw new Error('AS2 Sound transforms require the current Rust runtime; re-export this publication.');
      const base=record.matrix?{state:record.matrix.state}:{pan:record.pan,volume:record.volume};
      return JSON.parse(wasm.as2_sound_transform(JSON.stringify({...base,...patch})));
    }
    function applyLegacyMatrix(record,result){
      record.matrix=result;record.volume=result.state.volume;record.pan=result.pan;
      for(const channel of record.owned){
        const value=state(channel,channels);value.matrix=result;
        if(value.started&&!value.matrixGains)connectMatrix(value,value.gain,result);
        if(value.matrixGains)for(let i=0;i<4;i++)value.matrixGains[i].gain.value=result.gains[i];
      }
    }
    class LegacySound {
      constructor(...args){
        open();if(args.length>1)throw new TypeError('AS2 Sound accepts at most one optional target MovieClip.');
        const target=args.length===0?null:(typeof options.resolveSoundTarget==='function'?options.resolveSoundTarget(args[0]):(args[0]??null));
        const sound=new Sound(),record={receiver:this,target,generation,sound,volume:100,pan:0,matrix:null,id3:undefined,channel:null,owned:new Set(),checkPolicyFile:false,streaming:false,attachedId:null};legacy.set(this,record);sounds.get(sound).legacyOwner=record;
        sound.addEventListener('complete',()=>{if(record.streaming){try{this.start(0,1);}catch(_){}}legacyCallback(record,'onLoad',[true]);});sound.addEventListener('ioError',()=>legacyCallback(record,'onLoad',[false]));
      }
      loadSound(url,isStreaming=false){const record=state(this,legacy);if(typeof isStreaming!=='boolean')throw new TypeError('AS2 Sound.loadSound isStreaming requires a Boolean.');stopLegacy(record);const sound=state(record.sound,sounds);cancelLoad(sound);decodedBytes-=sound.bytes;sound.bytes=0;sound.buffer=null;record.id3=undefined;record.channel=null;record.attachedId=null;record.streaming=isStreaming;record.sound.load(new net.URLRequest(url));}
      start(offset=0,loops=1){
        const record=state(this,legacy);number(offset,'AS2 Sound secondOffset',0,2147483.647);if(!Number.isInteger(loops)||loops<1||loops>10001)throw new RangeError('AS2 Sound loops requires a total play count from 1 through 10001.');
        const channel=play(state(record.sound,sounds),offset*1000,loops-1,new SoundTransform(record.volume/100,record.pan/100),record.matrix);record.channel=channel;record.owned.add(channel);
        channel.addEventListener('soundComplete',()=>{record.owned.delete(channel);legacyCallback(record,'onSoundComplete',[]);});
      }
      stop(...args){
        const record=state(this,legacy);
        if(args.length>1)throw new TypeError('AS2 Sound.stop accepts at most one optional linkage id.');
        if(args.length===1&&args[0]!==undefined){
          if(typeof args[0]!=='string')throw new TypeError('AS2 Sound.stop linkage id requires a String.');
          const wanted=args[0];
          if(record.target!==null){if(record.attachedId===wanted)stopLegacy(record);}
          else{for(const item of soundRecords)if(item.legacyOwner&&item.legacyOwner.generation===generation&&item.legacyOwner.attachedId===wanted)stopLegacy(item.legacyOwner);}
          return;
        }
        stopLegacy(record);
      }
      getVolume(){return state(this,legacy).volume;}
      setVolume(value){const record=state(this,legacy);number(value,'AS2 Sound volume',0,100);if(record.matrix){applyLegacyMatrix(record,legacyMatrix(record,{volume:value}));return;}record.volume=value;for(const channel of record.owned)channel.soundTransform=new SoundTransform(record.volume/100,record.pan/100);}
      getPan(){return state(this,legacy).pan;}
      setPan(value){const record=state(this,legacy);number(value,'AS2 Sound pan',-100,100);if(record.matrix){applyLegacyMatrix(record,legacyMatrix(record,{pan:value}));return;}record.pan=value;for(const channel of record.owned)channel.soundTransform=new SoundTransform(record.volume/100,record.pan/100);}
      get position(){const record=state(this,legacy);return record.channel?record.channel.position:0;}
      get duration(){return state(this,legacy).sound.length;}
      getBytesLoaded(){return state(this,legacy).sound.bytesLoaded;}getBytesTotal(){return state(this,legacy).sound.bytesTotal;}
      attachSound(id){
        const record=state(this,legacy);if(typeof id!=='string')throw new TypeError('AS2 attachSound requires a bundled audio resource ID.');
        if(typeof player?.as2_sound_resource!=='function')throw new Error('AS2 attachSound requires the current Rust runtime; re-export this publication.');
        const path=JSON.parse(player.as2_sound_resource(id));if(path===null)throw new Error(`No bundled audio resource has ID ${id}. Use a project resource ID; FLA/SWF linkage import is not available.`);
        stopLegacy(record);const sound=state(record.sound,sounds);cancelLoad(sound);decodedBytes-=sound.bytes;sound.bytes=0;sound.buffer=null;record.id3=undefined;record.channel=null;record.attachedId=id;record.streaming=false;
        record.sound.load(new net.URLRequest(path));
      }
      getTransform(){const record=state(this,legacy),value=(record.matrix||legacyMatrix(record)).state;return {ll:value.ll,lr:value.lr,rl:value.rl,rr:value.rr};}
      setTransform(value){const record=state(this,legacy);if(value===null||typeof value!=='object')throw new TypeError('AS2 Sound.setTransform requires an object.');const patch={};for(const key of ['ll','lr','rl','rr'])if(Object.prototype.hasOwnProperty.call(value,key)){patch[key]=value[key];if(typeof patch[key]!=='number'||!Number.isFinite(patch[key]))throw new TypeError('AS2 Sound transform coefficients must be finite numbers.');}applyLegacyMatrix(record,legacyMatrix(record,{patch}));}
      get id3(){return state(this,legacy).id3;}
      get checkPolicyFile(){return state(this,legacy).checkPolicyFile;}set checkPolicyFile(value){const record=state(this,legacy);if(typeof value!=='boolean')throw new TypeError('AS2 Sound.checkPolicyFile requires a Boolean.');record.checkPolicyFile=value;}
    }
    for(const[Ctor,name,table]of [[Sound,'Sound',sounds],[SoundChannel,'SoundChannel',channels],[SoundTransform,'SoundTransform',transforms],[ID3Info,'ID3Info',id3Infos],[SoundLoaderContext,'SoundLoaderContext',loaderContexts],[LegacySound,'AS2 Sound',legacy]]){
      Ctor.__nfIs=value=>table.has(value);Ctor.__nfInitialize=()=>fail(name+' native subclassing');Object.defineProperty(Ctor,Symbol.hasInstance,{value:Ctor.__nfIs});
    }
    function stopLegacy(record){for(const channel of record.owned)channel.stop();record.owned.clear();}
    function stopAllLegacySounds(){open();for(const record of soundRecords)if(record.legacyOwner)stopLegacy(record.legacyOwner);}
    // AS3 SoundMixer.stopAll(): every scripted channel of this publication (timeline voices via timeline()).
    function stopAllChannels(){open();for(const record of [...playing])stop(record);}
    class SoundMixer {
      constructor(){throw new TypeError('SoundMixer cannot be instantiated.');}
      static stopAll(){stopAllChannels();options.onStopAll?.();}
      static areSoundsInaccessible(){open();return false;}
      static get bufferTime(){open();return mixerBufferTime;}
      static set bufferTime(value){open();mixerBufferTime=Math.trunc(Number(value))||0;}
      static get soundTransform(){open();const st=new SoundTransform(mixerTransform.volume,mixerTransform.pan);Object.assign(transforms.get(st),mixerTransform);return st;}
      static set soundTransform(value){open();mixerTransform=transformCopy(value);}
      static computeSpectrum(outputArray,FFTMode=false,stretchFactor=0){
        open();binary.toBytes(outputArray);
        const windowFrames=Math.max(512,Math.min(4096,256*(Math.max(1,Math.trunc(Number(stretchFactor))||1))));
        const mixed=new Uint8Array(windowFrames*8),mixedView=new DataView(mixed.buffer);
        let hasMixed=false;
        const mll=mixerTransform.leftToLeft,mrl=mixerTransform.rightToLeft,mlr=mixerTransform.leftToRight,mrr=mixerTransform.rightToRight;
        for(const ch of playing){
          const pcm=ch.sound?.pcmBytes;
          if(!pcm||ch.stopped)continue;
          const totalFrames=pcm.byteLength>>>3;
          if(totalFrames<=0)continue;
          const offsetFrames=Math.max(0,Math.floor((position(ch)/1000)*44100));
          const srcView=new DataView(pcm.buffer,pcm.byteOffset,pcm.byteLength);
          const t=ch.transform||{leftToLeft:1,rightToLeft:0,leftToRight:0,rightToRight:1};
          for(let i=0;i<windowFrames;i++){
            const idx=offsetFrames+i;
            if(idx>=totalFrames)break;
            const rawL=srcView.getFloat32(idx*8,true),rawR=srcView.getFloat32(idx*8+4,true);
            const chL=rawL*t.leftToLeft+rawR*t.rightToLeft,chR=rawL*t.leftToRight+rawR*t.rightToRight;
            const outL=chL*mll+chR*mrl,outR=chL*mlr+chR*mrr;
            mixedView.setFloat32(i*8,Math.max(-1,Math.min(1,mixedView.getFloat32(i*8,true)+outL)),true);
            mixedView.setFloat32(i*8+4,Math.max(-1,Math.min(1,mixedView.getFloat32(i*8+4,true)+outR)),true);
          }
          hasMixed=true;
        }
        let pcmSlice=mixed;
        if(!hasMixed){
          let pcm=null;
          for(const rec of soundRecords){if(rec.pcmBytes){pcm=rec.pcmBytes;break;}}
          pcmSlice=pcm?pcm:new Uint8Array(0);
        }
        let specBytes;
        if(typeof wasm?.as3_sound_compute_spectrum==='function'){
          specBytes=wasm.as3_sound_compute_spectrum(pcmSlice,Boolean(FFTMode),Math.trunc(Number(stretchFactor))||0,true);
        }else{
          specBytes=new Uint8Array(2048);
        }
        outputArray.position=0;
        const view=new DataView(specBytes.buffer,specBytes.byteOffset,specBytes.byteLength);
        for(let i=0;i<512;i++)outputArray.writeFloat(view.getFloat32(i*4,true));
        outputArray.position=0;
      }
    }
    SoundMixer.__nfIs=()=>false;
    SoundMixer.__nfInitialize=()=>{throw new TypeError('SoundMixer cannot be subclassed.');};
    Object.defineProperty(SoundMixer,Symbol.hasInstance,{value:()=>false});
    // Timeline sounds. Voices are scheduled from Rust SoundCommands; buffers are decoded once
    // per resource and session. Before the browser allows audio, streams wait and resume at
    // their timeline position on Enable audio; Event/Start sounds of that period are skipped.
    const resourcePaths=new Map((options.resources||[]).map(resource=>[resource.id,resource.path]));
    const buffers=new Map(),voices=new Set(),now=options.now||(()=>(global.performance?.now?.()??Date.now())/1000);
    let blockedEvents=0;
    function timelineBuffer(id){
      let entry=buffers.get(id);if(entry)return entry;
      entry={buffer:null,error:null};buffers.set(id,entry);
      const epoch=generation,path=resourcePaths.get(id);
      try{
        if(!path)throw new Error(`Timeline sound resource ${id} is not embedded in this publication. Re-export the project.`);
        const bytes=net.mediaSource(path).bytes,ctx=ensureContext(),profile=formats.inspect(bytes,ctx.sampleRate);
        if(!Number.isSafeInteger(profile.decodedBytes)||profile.decodedBytes<1||profile.decodedBytes>maxDecodedBytes-decodedBytes)throw new RangeError('Decoded audio would exceed the publication PCM budget.');
        decodedBytes+=profile.decodedBytes;
        Promise.resolve(ctx.decodeAudioData(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength))).then(buffer=>{
          if(disposed||epoch!==generation)return;
          if(!buffer||!Number.isFinite(buffer.duration)||buffer.duration<=0)throw new RangeError('Timeline sound decoded to no audio.');
          entry.buffer=buffer;flushVoices();changed();
        }).catch(error=>{if(disposed||epoch!==generation)return;entry.error=error;dropVoices(id);report(error);});
      }catch(error){entry.error=error;report(error);}
      return entry;
    }
    function dropVoices(id){for(const voice of [...voices])if(voice.resource===id)stopVoice(voice);}
    function stopVoice(voice){
      voices.delete(voice);const source=voice.source;voice.source=null;
      if(source){source.onended=null;try{source.stop();}catch(_){}}
      for(const node of voice.nodes)try{node.disconnect();}catch(_){}
      voice.nodes=[];changed();
    }
    function connectEnvelope(voice,source,start,length){
      // Rust supplies the envelope points; this only maps them onto per-channel gain automation.
      const points=voice.envelope.length?voice.envelope:[[0,1,1]];
      if(points.length===1&&points[0][1]===1&&points[0][2]===1){source.connect(master);return [];}
      if(typeof context.createChannelSplitter!=='function'||typeof context.createChannelMerger!=='function'){source.connect(master);return [];}
      const input=context.createGain(),splitter=context.createChannelSplitter(2),merger=context.createChannelMerger(2),gains=[context.createGain(),context.createGain()];
      input.channelCount=2;input.channelCountMode='explicit';input.channelInterpretation='speakers';
      source.connect(input);input.connect(splitter);
      gains.forEach((gain,channel)=>{
        splitter.connect(gain,channel);gain.connect(merger,0,channel);
        const origin=start-voice.offset;
        points.forEach(([position,left,right],index)=>{
          const level=channel?right:left,time=Math.max(0,origin+position*length);
          if(index===0)gain.gain.setValueAtTime(level,time);else gain.gain.linearRampToValueAtTime(level,time);
        });
      });
      merger.connect(master);return [input,splitter,merger,...gains];
    }
    function startVoice(voice){
      if(voice.source||!unlocked||context?.state!=='running')return;
      const entry=timelineBuffer(voice.resource);if(!entry.buffer)return;
      const buffer=entry.buffer,duration=buffer.duration;
      if(voice.stream)voice.offset+=Math.max(0,now()-voice.requestedAt);
      else if(now()-voice.requestedAt>1){stopVoice(voice);return;}
      const segStart=Math.min(duration,Math.max(0,voice.inPoint||0));
      const segEnd=voice.outPoint!==null?Math.min(duration,voice.outPoint):duration;
      const segDuration=segEnd-segStart;
      if(!(segDuration>0)){stopVoice(voice);return;}
      const total=voice.loopForever?Infinity:segDuration*voice.loops;
      if(voice.offset>=total){stopVoice(voice);return;}
      const source=context.createBufferSource(),start=context.currentTime;
      source.buffer=buffer;source.loop=voice.loopForever||voice.loops>1;
      if(source.loop&&(segStart>0||segEnd<duration)){source.loopStart=segStart;source.loopEnd=segEnd;}
      voice.nodes=connectEnvelope(voice,source,start,Number.isFinite(total)?total:segDuration);
      voice.source=source;source.onended=()=>{if(voice.source===source)stopVoice(voice);};
      source.start(start,segStart+(voice.offset%segDuration));
      if(Number.isFinite(total))source.stop(start+total-voice.offset);
      changed();
    }
    function flushVoices(){for(const voice of [...voices]){if(disposed)break;try{startVoice(voice);}catch(error){stopVoice(voice);report(error);}}}
    // Decode the document's timeline sounds ahead of their first keyframe.
    function preloadTimeline(ids){open();if(!Array.isArray(ids))throw new TypeError('Timeline sound preload requires resource IDs.');for(const id of ids.slice(0,maxSounds))timelineBuffer(String(id));}
    function timeline(commands){
      open();if(!Array.isArray(commands))throw new TypeError('Timeline sound commands must be an array.');
      for(const command of commands){
        const op=command?.op;
        if(op==='stop_all'){for(const voice of [...voices])stopVoice(voice);blockedEvents=0;continue;}
        if(op==='stop'){for(const voice of [...voices])if(!voice.stream&&voice.resource===command.resource)stopVoice(voice);continue;}
        if(op==='stream_stop'){for(const voice of [...voices])if(voice.channel===command.channel)stopVoice(voice);continue;}
        if(op!=='play'&&op!=='stream_start')throw new TypeError(`Unknown timeline sound command ${op}.`);
        if(voices.size>=maxChannels)continue;
        const stream=op==='stream_start';used=true;
        if(stream)for(const voice of [...voices])if(voice.channel===command.channel)stopVoice(voice);
        if(!stream&&command.sync==='start'&&[...voices].some(voice=>voice.resource===command.resource))continue;
        if(!stream&&(!unlocked||context?.state!=='running')){blockedEvents=1;timelineBuffer(command.resource);changed();continue;}
        const inPoint=typeof command.in_point==='number'&&Number.isFinite(command.in_point)?Math.max(0,command.in_point):0;
        const outPoint=typeof command.out_point==='number'&&Number.isFinite(command.out_point)?Math.max(0,command.out_point):null;
        const voice={resource:command.resource,channel:stream?command.channel:null,stream,offset:stream?Number(command.offset)||0:0,
          loops:Math.max(1,command.loops|0),loopForever:command.loop_forever===true,inPoint,outPoint,envelope:Array.isArray(command.envelope)?command.envelope:[],requestedAt:now(),source:null,nodes:[]};
        voices.add(voice);
        try{startVoice(voice);}catch(error){stopVoice(voice);report(error);}
      }
      changed();
    }
    function reset(){
      open();if(master)master.gain.value=0;
      for(const voice of [...voices])stopVoice(voice);buffers.clear();blockedEvents=0;
      for(const record of [...playing])stop(record);
      for(const record of soundRecords){cancelLoad(record);record.buffer=null;record.pcmBytes=null;record.bytes=0;}
      const old=context;context=null;if(master){try{master.disconnect();}catch(_){}}master=null;
      // decodeAudioData has no abort API. Keep reservations for unsettled native
      // decodes across Restart so a new session cannot overlap unbudgeted PCM.
      generation++;hub.clear();soundRecords.clear();playing.clear();decodedBytes=[...decodes].reduce((sum,token)=>sum+token.reserved,0);unlocked=false;muted=false;used=false;lastError='';resuming=null;mixerBufferTime=0;mixerTransform={volume:1,pan:0,leftToLeft:1,leftToRight:0,rightToLeft:0,rightToRight:1};
      try{old?.close()?.catch(()=>{});}catch(_){}changed();
    }
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({Sound,SoundChannel,SoundTransform,ID3Info,SoundLoaderContext,SoundMixer,LegacySound,stopAllLegacySounds,stopAllChannels,timeline,preloadTimeline,captureBus,resumeFromGesture,setMuted,status,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAudio=api;
})(globalThis);
