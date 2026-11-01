/* AS2-only browser connections. Rust owns wire validation, AMF packet framing,
 * pending-message state, retries/ACK deduplication and connection policy. */
(function(global){'use strict';
function create(options={}){
 const{wasm,amf,net,binary}=options;
 if(!wasm?.ScriptAS2Connections||!wasm?.as2_remoting_request||!amf?.encode||!net?.loadBytes)throw new Error('AS2 connections require the Rust connection/AMF codec and shared transport.');
 const localStates=new WeakMap(),locals=new Map(),remoteStates=new WeakMap(),remotes=new Set(),xmlStates=new WeakMap(),xmlSockets=new Set();let kernel=null,bus=null,generation=1,disposed=false,ticker=null;
 const empty=new Uint8Array(),encoder=new TextEncoder(),decoder=new TextDecoder();
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
  try{
   const allow=event.insecure&&typeof r.receiver.allowInsecureDomain==='function'?r.receiver.allowInsecureDomain:r.receiver.allowDomain;
   if(typeof allow==='function'&&!Reflect.apply(allow,r.receiver,[event.domain]))return;
  }catch(error){report(error);return;}
  let args;try{args=amf.decode(amf.unbase64(event.body),0,{complete:true}).value;}catch(error){report(error);return;}
  try{const handler=r.receiver[event.method];if(typeof handler==='function')Reflect.apply(handler,r.receiver,args);if(active(r))options.afterEvent?.();}catch(error){report(error);}
 }
 function transport(){
  const BC=options.BroadcastChannel||global.BroadcastChannel,locks=options.locks||global.navigator?.locks;
  if(!BC||!locks?.request)throw new Error('LocalConnection requires BroadcastChannel and Web Locks on a secure browser origin (localhost is supported).');
  if(!bus){bus=new BC('neoflash.as2.localconnection.v1');bus.onmessage=event=>receive(event.data);}
  return locks;
 }
 function arm(){if(ticker!==null)return;const born=generation;ticker=global.setInterval(()=>{if(disposed||!kernel||generation!==born)return;try{const result=command({op:'tick'});if(!result.pending){global.clearInterval(ticker);ticker=null;}for(const wire of result.outgoing)post(wire);for(const event of result.statuses){if(disposed||generation!==born)return;localStatus(event,'send','No named receiver acknowledged the command within two seconds.');}}catch(error){global.clearInterval(ticker);ticker=null;report(error);}},100);}
 function release(record){record.claimToken++;record.release?.();record.release=null;}
 class LocalConnection{
  constructor(){const locks=transport();const id=command({op:'localNew'}),r={id,generation,receiver:this,claimToken:0,release:null,locks};localStates.set(this,r);locals.set(id,r);this.onStatus=null;this.allowDomain=null;this.allowInsecureDomain=null;}
  domain(){state(localStates,this);return command({op:'domain'});}
  get isPerUser(){const r=state(localStates,this);return command({op:'user',owner:r.id});}
  set isPerUser(value){const r=state(localStates,this);command({op:'user',owner:r.id,set:true,value});}
  connect(name){
   const r=state(localStates,this);if(typeof name!=='string')return false;
   let plan;try{plan=command({op:'connect',owner:r.id,name});}catch(_){return false;}if(!plan.accepted)return false;
   const token=++r.claimToken;
   r.locks.request('neoflash.as2.localconnection:'+plan.name,{mode:'exclusive',ifAvailable:true},lock=>{
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
 Object.defineProperties(LocalConnection,{__nfIs:{value:value=>localStates.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native LocalConnection subclasses are unsupported; attach receiving methods to an instance.');}}});
 function stopXmlSocket(r,notifyClose=false){
  r.token++;if(r.timer!==null){global.clearTimeout?.(r.timer);r.timer=null;}
  const sock=r.socket;r.socket=null;const wasOpen=r.open;r.open=false;
  if(sock){sock.onopen=sock.onmessage=sock.onerror=sock.onclose=null;try{sock.close?.();}catch(_){}}
  if(!disposed&&kernel&&r.generation===generation)try{command({op:'xmlClose',owner:r.id});}catch(_){}
  if(notifyClose&&wasOpen&&active(r))invoke(r,r.receiver,'onClose',undefined);
 }
 class XMLSocket{
  constructor(){
   const id=command({op:'xmlNew'}),r={id,generation,receiver:this,socket:null,timer:null,token:0,open:false};
   xmlStates.set(this,r);xmlSockets.add(r);this.onConnect=null;this.onClose=null;this.onXML=null;
  }
  get timeout(){const r=state(xmlStates,this);return command({op:'xmlTimeout',owner:r.id});}
  set timeout(value){const r=state(xmlStates,this);if(!Number.isInteger(value)||value<0)throw new RangeError('XMLSocket.timeout requires a non-negative integer.');command({op:'xmlTimeout',owner:r.id,set:true,value});}
  onData(src){
   state(xmlStates,this);const LegacyXML=options.getXML?.();
   const doc=LegacyXML?new LegacyXML(String(src??'')):String(src??'');
   if(typeof this.onXML==='function')Reflect.apply(this.onXML,this,[doc]);
  }
  connect(host,port){
   const r=state(xmlStates,this);
   if(host!==null&&host!==undefined&&typeof host!=='string')return false;
   if(!Number.isInteger(port)||port<0||port>65535)return false;
   stopXmlSocket(r,false);
   let plan;try{plan=command({op:'xmlConnect',owner:r.id,host:host==null?'':host,port});}catch(_){return false;}
   if(!plan.accepted)return false;
   const token=++r.token,factory=options.createXmlSocket||(typeof global.WebSocket==='function'?(h,p)=>new global.WebSocket((new URL(options.url||global.location.href).protocol==='https:'?'wss://':'ws://')+h+':'+p):null);
   if(!factory){queueMicrotask(()=>{if(!active(r)||r.token!==token)return;command({op:'xmlOpen',owner:r.id,ok:false});invoke(r,this,'onConnect',false);});return true;}
   let sock;try{sock=factory(plan.host,plan.port);}catch(_){queueMicrotask(()=>{if(!active(r)||r.token!==token)return;command({op:'xmlOpen',owner:r.id,ok:false});invoke(r,this,'onConnect',false);});return true;}
   r.socket=sock;
   if(typeof global.setTimeout==='function'&&plan.timeout>0){
    r.timer=global.setTimeout(()=>{if(!active(r)||r.token!==token||r.open)return;stopXmlSocket(r,false);command({op:'xmlOpen',owner:r.id,ok:false});invoke(r,this,'onConnect',false);},plan.timeout);
    r.timer?.unref?.();
   }
   sock.onopen=()=>{if(!active(r)||r.token!==token)return;if(r.timer!==null){global.clearTimeout?.(r.timer);r.timer=null;}r.open=true;command({op:'xmlOpen',owner:r.id,ok:true});invoke(r,this,'onConnect',true);};
   sock.onmessage=ev=>{
    if(!active(r)||r.token!==token||!r.open)return;
    const raw=ev&&typeof ev==='object'&&'data'in ev?ev.data:ev;
    const bytes=typeof raw==='string'?encoder.encode(raw):raw instanceof Uint8Array?raw:raw instanceof ArrayBuffer?new Uint8Array(raw):encoder.encode(String(raw??''));
    let res;try{res=command({op:'xmlFeed',owner:r.id},bytes);}catch(error){stopXmlSocket(r,true);report(error);return;}
    for(const frame of res.frames)if(active(r)&&r.token===token)invoke(r,this,'onData',frame);
   };
   sock.onerror=()=>{if(!active(r)||r.token!==token)return;if(!r.open){stopXmlSocket(r,false);command({op:'xmlOpen',owner:r.id,ok:false});invoke(r,this,'onConnect',false);}};
   sock.onclose=()=>{if(!active(r)||r.token!==token)return;if(r.open)stopXmlSocket(r,true);else{stopXmlSocket(r,false);command({op:'xmlOpen',owner:r.id,ok:false});invoke(r,this,'onConnect',false);}};
   return true;
  }
  send(object){
   const r=state(xmlStates,this);if(!r.open||!r.socket)throw new Error('XMLSocket.send requires an open connection.');
   const text=object===null||object===undefined?'':String(object);
   const framed=amf.unbase64(command({op:'xmlSend',owner:r.id},encoder.encode(text)).wire);
   r.socket.send(decoder.decode(framed));
  }
  close(){const r=state(xmlStates,this);stopXmlSocket(r,false);}
 }
 Object.defineProperties(XMLSocket,{__nfIs:{value:value=>xmlStates.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native XMLSocket subclasses are unsupported; attach callbacks to an XMLSocket instance.');}},[Symbol.hasInstance]:{value:value=>xmlStates.has(value)}});
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
 function reset(){if(disposed)return;for(const record of locals.values())release(record);for(const record of remotes)stopRemote(record);for(const record of xmlSockets)stopXmlSocket(record,false);if(ticker!==null)global.clearInterval(ticker);ticker=null;bus?.close();bus=null;if(kernel)command({op:'reset'});locals.clear();remotes.clear();xmlSockets.clear();generation++;}
 function dispose(){if(disposed)return;reset();kernel?.free();kernel=null;disposed=true;}
 return{LocalConnection,XMLSocket,remoting,reset,dispose};
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Connections=api;
})(globalThis);
