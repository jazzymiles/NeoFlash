/* AS2-only browser connections. Rust owns wire validation, AMF packet framing,
 * pending-message state, retries/ACK deduplication and connection policy. */
(function(global){'use strict';
function create(options={}){
 const{wasm,amf,net,binary}=options;
 if(!wasm?.ScriptAS2Connections||!wasm?.as2_remoting_request||!amf?.encode||!net?.loadBytes)throw new Error('AS2 connections require the Rust connection/AMF codec and shared transport.');
 const localStates=new WeakMap(),locals=new Map(),remoteStates=new WeakMap(),remotes=new Set();let kernel=null,bus=null,generation=1,disposed=false,ticker=null;
 const empty=new Uint8Array();
 function now(){return global.performance.now();}
 function active(record){return !disposed&&record.generation===generation;}
 function state(map,value){const r=map.get(value);if(!r||!active(r))throw new Error('This AS2 connection belongs to a restarted publication.');return r;}
 function ensure(){
  if(disposed)throw new Error('AS2 connections disposed.');if(kernel)return;
  const url=new URL(options.url||global.location.href);if(!['http:','https:'].includes(url.protocol))throw new Error('AS2 browser connections require an HTTP(S) publication.');
  kernel=new wasm.ScriptAS2Connections(JSON.stringify({origin:url.origin,domain:url.hostname,clientNonce:global.crypto.randomUUID()}));
 }
 function command(request,bytes=empty){ensure();try{return JSON.parse(kernel.execute(JSON.stringify({...request,now:now()}),bytes));}catch(e){throw new Error(String(e));}}
 function report(error){if(options.onError)options.onError(error);else throw error;}
 function invoke(record,target,name,arg){if(!active(record))return;try{const handler=target?.[name];if(typeof handler==='function')Reflect.apply(handler,target,[arg]);if(active(record))options.afterEvent?.();}catch(e){report(e);}}
 function localStatus(event,operation='send',description=''){const r=locals.get(event.owner);if(r&&active(r))invoke(r,r.receiver,'onStatus',{level:event.level,operation,description,messageId:event.id});}
 function post(wire){if(disposed||!bus)return;bus.postMessage(wire);const born=generation;queueMicrotask(()=>{if(!disposed&&generation===born)receive(wire);});}
 function receive(wire){
  if(typeof wire!=='string'||!kernel||disposed)return;let result;try{result=command({op:'incoming',wire});}catch(error){options.onDiagnostic?.(String(error));return;}
  if(!result.event)return;
  if(result.kind==='ack'){localStatus(result.event);return;}
  const event=result.event,r=locals.get(event.owner);if(!r||!active(r))return;
  // ACK means the named receiver got the command, not that its method existed
  // or succeeded. This is the legacy LocalConnection.onStatus distinction.
  post(event.ack);if(event.duplicate)return;
  let args;try{args=amf.decode(amf.unbase64(event.body),0,{complete:true}).value;}catch(error){report(error);return;}
  try{const handler=r.receiver[event.method];if(typeof handler==='function')Reflect.apply(handler,r.receiver,args);if(active(r))options.afterEvent?.();}catch(error){report(error);}
 }
 function transport(){
  if(!global.BroadcastChannel||!global.navigator?.locks?.request)throw new Error('LocalConnection requires BroadcastChannel and Web Locks on a secure browser origin (localhost is supported).');
  if(!bus){bus=new global.BroadcastChannel('neoflash.as2.localconnection.v1');bus.onmessage=event=>receive(event.data);}
 }
 function arm(){if(ticker!==null)return;const born=generation;ticker=global.setInterval(()=>{if(disposed||!kernel||generation!==born)return;try{const result=command({op:'tick'});if(!result.pending){global.clearInterval(ticker);ticker=null;}for(const wire of result.outgoing)post(wire);for(const event of result.statuses){if(disposed||generation!==born)return;localStatus(event,'send','No named receiver acknowledged the command within two seconds.');}}catch(error){global.clearInterval(ticker);ticker=null;report(error);}},100);}
 function release(record){record.claimToken++;record.release?.();record.release=null;}
 class LocalConnection{
  constructor(){transport();const id=command({op:'localNew'}),r={id,generation,receiver:this,claimToken:0,release:null};localStates.set(this,r);locals.set(id,r);this.onStatus=null;}
  domain(){state(localStates,this);return command({op:'domain'});}
  get isPerUser(){const r=state(localStates,this);return command({op:'user',owner:r.id});}
  set isPerUser(value){const r=state(localStates,this);command({op:'user',owner:r.id,set:true,value});}
  connect(name){
   const r=state(localStates,this);if(typeof name!=='string')return false;
   let plan;try{plan=command({op:'connect',owner:r.id,name});}catch(_){return false;}if(!plan.accepted)return false;
   const token=++r.claimToken;
   global.navigator.locks.request('neoflash.as2.localconnection:'+plan.name,{mode:'exclusive',ifAvailable:true},lock=>{
    if(!active(r)||r.claimToken!==token)return;
    const accepted=!!lock;command({op:'claim',owner:r.id,name:plan.name,accepted});
    if(!accepted){localStatus({owner:r.id,level:'error'},'connect','The receiver name is owned by another browser context.');return;}
    return new Promise(resolve=>{r.release=resolve;localStatus({owner:r.id,level:'status'},'connect','The browser exclusively reserved the receiver name.');if(!active(r)||r.claimToken!==token){r.release=null;resolve();}});
   }).catch(error=>{if(!active(r)||r.claimToken!==token)return;command({op:'claim',owner:r.id,name:plan.name,accepted:false});localStatus({owner:r.id,level:'error'},'connect',String(error));});
   // Browser Web Locks are asynchronous. true acknowledges a valid reservation
   // request; the explicit connect status reports actual cross-context ownership.
   return true;
  }
  close(){const r=state(localStates,this);release(r);command({op:'close',owner:r.id});}
  send(name,method,...args){const r=state(localStates,this);if(typeof name!=='string'||typeof method!=='string')return false;let plan;try{plan=command({op:'send',owner:r.id,name,method},amf.encode(args,0));}catch(_){return false;}post(plan.wire);arm();return true;}
 }
 for(const name of ['allowDomain','allowInsecureDomain'])Object.defineProperty(LocalConnection.prototype,name,{get(){throw new Error('LocalConnection.'+name+' requires cross-origin Flash transport, which this same-origin browser profile does not implement.');},set(){throw new Error('LocalConnection.'+name+' cannot enable cross-origin browser messaging; use one publication origin.');}});
 Object.defineProperties(LocalConnection,{__nfIs:{value:value=>localStates.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native LocalConnection subclasses are unsupported; attach receiving methods to an instance.');}}});
 function remote(receiver,allocate=false){let r=remoteStates.get(receiver);if(r){if(!active(r))throw new Error('NetConnection belongs to a restarted publication.');return r;}if(!allocate)return null;const id=command({op:'remoteNew'});r={id,receiver,generation,operations:new Set()};remoteStates.set(receiver,r);remotes.add(r);return r;}
 function stopRemote(record){for(const operation of [...record.operations]){operation.active=false;try{operation.handle?.close();}catch(_){}record.operations.delete(operation);}}
 const remoting={
  connect(receiver,uri){
   if(typeof uri!=='string')throw new TypeError('NetConnection remoting endpoint requires an HTTP(S) URL.');const url=new URL(uri,options.url||global.location.href);if(!['http:','https:'].includes(url.protocol)||url.username||url.password)throw new Error('NetConnection supports HTTP(S) AMF0 remoting without URL credentials; RTMP is unavailable.');
   const r=remote(receiver,true);stopRemote(r);return command({op:'remoteConnect',owner:r.id,uri:url.href});
  },
  close(receiver){const r=remote(receiver);if(!r)return;stopRemote(r);command({op:'remoteClose',owner:r.id});},
  isConnected(receiver,localFlag=false){const r=remote(receiver);if(!r)return !!localFlag;const s=command({op:'remoteState',owner:r.id});return s.uri?false:!!localFlag;},
  uri(receiver){const r=remote(receiver);return r?command({op:'remoteState',owner:r.id}).uri??undefined:undefined;},
  call(receiver,method,responder,args){
   const r=remote(receiver);if(!r)throw new Error('NetConnection.call requires an AMF HTTP(S) endpoint; local video connections have no remote server.');if(typeof method!=='string')throw new TypeError('NetConnection.call requires a remote method name.');if(responder!==null&&(typeof responder!=='object'&&typeof responder!=='function'))throw new TypeError('NetConnection.call responder requires an Object or null.');
   const body=amf.encode(args,0),plan=command({op:'remoteBegin',owner:r.id});let bytes;
   try{bytes=wasm.as2_remoting_request(method,plan.response,body);}catch(error){command({op:'remoteFinish',owner:r.id,id:plan.id});throw new Error(String(error));}
   let request,buffer;
   try{request=new net.URLRequest(plan.uri);request.method='POST';request.contentType='application/x-amf';buffer=binary.fromBytes(bytes);request.data=buffer;}
   catch(error){command({op:'remoteFinish',owner:r.id,id:plan.id});throw error;}
   const operation={active:true,handle:null,generation};r.operations.add(operation);
   function finish(){if(!operation.active||!active(r))return false;operation.active=false;r.operations.delete(operation);return command({op:'remoteFinish',owner:r.id,id:plan.id});}
   function failed(description){if(!finish())return;invoke(r,receiver,'onStatus',{code:'NetConnection.Call.Failed',level:'error',description:String(description),details:plan.uri});}
   let failure=null;try{operation.handle=net.loadBytes(request,{onEvent(event){if(event.type==='ioError')failed(event.text||'HTTP AMF request failed.');},onComplete(responseBytes){
    if(!operation.active||!active(r))return;let result,value;
    try{result=JSON.parse(wasm.as2_remoting_response(responseBytes,plan.response));value=amf.decode(amf.unbase64(result.body),0,{complete:true}).value;}catch(error){failed(error);return;}
    if(!finish())return;if(responder!==null)invoke(r,responder,result.kind==='result'?'onResult':'onStatus',value);
   }});}catch(error){failure=error;}buffer.clear();if(failure)failed(failure);
  }
 };
 function reset(){if(disposed)return;for(const record of locals.values())release(record);for(const record of remotes)stopRemote(record);if(ticker!==null)global.clearInterval(ticker);ticker=null;bus?.close();bus=null;if(kernel)command({op:'reset'});locals.clear();remotes.clear();generation++;}
 function dispose(){if(disposed)return;reset();kernel?.free();kernel=null;disposed=true;}
 return{LocalConnection,remoting,reset,dispose};
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Connections=api;
})(globalThis);
