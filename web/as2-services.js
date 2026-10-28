/* AS2 SOAP and nonvisual connectors. Rust owns WSDL, SOAP encoding/decoding,
 * validation and log formatting. Browser code connects transport and references. */
(function(global){
'use strict';
function create(options={}){
 const {wasm,net,LegacyXML,EventDispatcher,ComponentMixins}=options;
 if(typeof wasm?.as2_service_command!=='function'||!net?.loadBytes||!LegacyXML||!EventDispatcher?.initialize)throw new Error('AS2 services require the Rust SOAP kernel, XML, network and EventDispatcher.');
 const services=new WeakMap(),calls=new WeakMap(),pending=new WeakMap(),connectors=new WeakMap(),logs=new WeakMap(),records=new Set(),transfers=new Set();
 const decoder=new TextDecoder('utf-8',{fatal:true});let generation=1,disposed=false;
 function core(input){try{return JSON.parse(wasm.as2_service_command(JSON.stringify(input)));}catch(e){throw new Error(String(e));}}
 function state(map,target){const r=map.get(target);if(disposed||!r||r.generation!==generation)throw new Error('This AS2 service belongs to a disposed or restarted publication.');return r;}
 function allocate(target,map,value){if(disposed)throw new Error('AS2 services disposed.');const r={generation,...value};map.set(target,r);return r;}
 function live(r){return !disposed&&r.generation===generation;}
 function report(error){if(options.onError)options.onError(error);else throw error;}
 function callback(target,name,value){try{const fn=target[name];if(typeof fn==='function')Reflect.apply(fn,target,[value]);options.afterEvent?.();}catch(e){report(e);}}
 function later(fn){const born=generation;queueMicrotask(()=>{if(!disposed&&generation===born)fn();});}
 function fault(code,error){return{faultcode:code,faultstring:String(error?.message||error),detail:'',element:null,faultactor:''};}
 function xml(source,ignoreWhite=false){const out=new LegacyXML();out.ignoreWhite=ignoreWhite;out.parseXML(source);if(out.status!==0)throw new Error(`Invalid XML document (status ${out.status}).`);return out;}
 function firstElement(doc){for(const node of doc.childNodes)if(node.nodeType===1)return node;return null;}
 function unpackFault(value){return{...value,element:value.element?xml(value.element):null};}
 function load(request,onComplete,onFault,acceptHttpError=false){
  const token={generation,handle:null,status:200,url:request.url};transfers.add(token);
  function finish(){transfers.delete(token);return live(token);}
  try{token.handle=net.loadBytes(request,{acceptHttpError,onEvent(event){if(!live(token))return;if(event.type==='httpStatus'){token.status=event.status;token.url=event.responseURL||token.url;}if(event.type==='ioError'&&finish())onFault(fault('Server.Connection',event.text||'Network request failed.'));},onComplete(bytes){if(!finish())return;let source;try{source=decoder.decode(bytes);}catch(e){onFault(fault('Client.Encoding',e));return;}onComplete(source,token.status,token.url);}});}catch(e){transfers.delete(token);later(()=>onFault(fault('Server.Connection',e)));}return token;
 }
 function cancel(token){if(!token)return;transfers.delete(token);token.generation=0;try{token.handle?.close();}catch(_){};}
 function date(){const d=new Date();return[d.getMonth()+1,d.getDate(),d.getHours(),d.getMinutes(),d.getSeconds()];}
 class Log{
  constructor(level=1,name=''){allocate(this,logs,{level:1,name:''});this.level=level;this.name=name;}
  get level(){return state(logs,this).level;}set level(value){if(!Number.isInteger(value)||value<0||value>3)throw new TypeError('Log level requires a Log constant.');state(logs,this).level=value;}
  get name(){return state(logs,this).name;}set name(value){if(typeof value!=='string'||value.length>1024)throw new TypeError('Log name requires a string up to 1024 characters.');state(logs,this).name=value;}
  getDateString(){state(logs,this);return core({op:'log',level:1,debug:false,name:'',message:'',date:date()}).date;}
  logInfo(message){log(this,message,false);}logDebug(message){log(this,message,true);}
 }
 Object.assign(Log,{NONE:0,BRIEF:1,VERBOSE:2,DEBUG:3});
 function log(target,message,debug){const r=state(logs,target);if(typeof message!=='string')throw new TypeError('Log message requires String.');const result=core({op:'log',level:r.level,debug,name:r.name,message,date:date()});if(result)callback(target,'onLog',result.message);}
 class SOAPCall{
  constructor(){throw new TypeError('SOAPCall is obtained through WebService.getCall(operation).');}
  get concurrency(){return state(calls,this).concurrency;}set concurrency(value){if(![0,1,2].includes(value))throw new TypeError('Use a SOAPCall concurrency constant.');state(calls,this).concurrency=value;}
  get doDecoding(){return state(calls,this).decoding;}set doDecoding(value){state(calls,this).decoding=Boolean(value);}
  get doLazyDecoding(){state(calls,this);return false;}set doLazyDecoding(value){state(calls,this);if(value)throw new Error('Lazy SOAP encoded arrays are outside the literal scalar service profile.');}
 }
 Object.assign(SOAPCall,{MULTIPLE_CONCURRENCY:0,SINGLE_CONCURRENCY:1,LAST_CONCURRENCY:2});
 class PendingCall{
  constructor(){throw new TypeError('PendingCall is returned by a WebService operation.');}
  get myCall(){return state(pending,this).call;}
  get request(){return state(pending,this).request;}
  get response(){return state(pending,this).response;}
  getOutputParameter(index){const r=state(pending,this);if(!Number.isInteger(index)||index<0)return undefined;return r.output[index];}
  getOutputParameterByName(name){return state(pending,this).output.find(v=>v.name===name);}
  getOutputParameters(){return state(pending,this).output.slice();}
  getOutputValue(index){return this.getOutputParameter(index)?.value;}
  getOutputValues(){return state(pending,this).output.map(v=>v.value);}
 }
 function makeCall(service,operation){const value=Object.create(SOAPCall.prototype);allocate(value,calls,{service,operation,concurrency:0,decoding:true,active:new Set()});return value;}
 function completeFailure(target,value){const p=state(pending,target);if(p.finished)return;p.finished=true;p.active?.delete(target);callback(target,'onFault',value);}
 function invoke(service,name,params){
  const s=state(services,service),target=Object.create(PendingCall.prototype);allocate(target,pending,{call:null,request:null,response:null,output:[],finished:false,token:null,active:null});
  if(s.error){later(()=>completeFailure(target,s.error));return target;}
  if(!s.descriptor){if(s.queue.length>=128)throw new RangeError('At most 128 SOAP calls may wait for WSDL.');s.queue.push({target,name,params});return target;}
  begin(service,target,name,params);return target;
 }
 function begin(service,target,name,params){
  const s=state(services,service),p=state(pending,target),call=s.callMap.get(name);
  if(!call){later(()=>completeFailure(target,fault('Client.NoSuchMethod',`Unknown WSDL operation '${name}'.`)));return;}
  const c=state(calls,call);p.call=call;p.active=c.active;
  if(c.concurrency===SOAPCall.SINGLE_CONCURRENCY&&c.active.size){later(()=>completeFailure(target,fault('Client.Concurrency','A call to this operation is already active.')));return;}
  if(c.concurrency===SOAPCall.LAST_CONCURRENCY)for(const prior of [...c.active]){const old=state(pending,prior);cancel(old.token);old.finished=true;c.active.delete(prior);}
  let plan;try{plan=core({op:'request',operation:c.operation,params});p.request=xml(plan.xml);}catch(e){later(()=>completeFailure(target,fault('Client.InvalidParams',e)));return;}
  c.active.add(target);const decoding=c.decoding;
  const request=new net.URLRequest(s.endpoint);request.method='POST';request.contentType='text/xml; charset=utf-8';request.data=plan.xml;request.requestHeaders=[new net.URLRequestHeader('SOAPAction',`"${plan.action}"`)];
  s.log?.logDebug(`Sending SOAP operation ${name}`);if(!live(p))return;
  p.token=load(request,(source,status)=>{
   if(p.finished||!live(p))return;let result;
   try{p.response=xml(source);result=core({op:'response',operation:c.operation,xml:source,status});if(result.fault){completeFailure(target,unpackFault(result.fault));return;}p.output=result.output.map(v=>({name:v.name,value:v.value,element:firstElement(xml(v.xml))}));}
   catch(e){completeFailure(target,fault('Client.Response',e));return;}
   p.finished=true;c.active.delete(target);s.log?.logDebug(`Received SOAP operation ${name}`);if(!live(p))return;callback(target,'onResult',decoding?p.output[0]?.value:p.response);
  },error=>{if(!p.finished&&live(p))completeFailure(target,error);},true);
 }
 class WebService{
  constructor(uri,logObject=null){
   if(typeof uri!=='string'||!uri)throw new TypeError('WebService requires a WSDL URL.');if(logObject!==null)state(logs,logObject);
   if(records.size>=128)throw new RangeError('At most 128 service/connector instances per publication.');
   const target=this,s=allocate(target,services,{descriptor:null,endpoint:null,queue:[],callMap:new Map(),methods:new Map(),log:logObject,error:null,token:null,uri});
   function dynamicName(key){return typeof key==='string'&&key!=='then'&&key!=='onLoad'&&key!=='onFault'&&!key.startsWith('__');}
   const proxy=new Proxy(target,{
    // AS2's shared Object resolver checks property existence before reading it.
    // Dynamic WSDL operations must be visible through both paths, including
    // calls queued while the descriptor is still loading.
    has(object,key){return Reflect.has(object,key)||dynamicName(key);},
    get(object,key,receiver){if(Reflect.has(object,key)||!dynamicName(key))return Reflect.get(object,key,receiver);if(!s.methods.has(key))s.methods.set(key,(...params)=>invoke(proxy,key,params));return s.methods.get(key);}
   });
   services.set(proxy,s);records.add(s);s.target=proxy;
   later(()=>{
    s.log?.logInfo('Loading WSDL');if(!live(s))return;s.token=load(new net.URLRequest(uri),(source,status,responseURL)=>{
     if(!live(s))return;try{s.descriptor=core({op:'wsdl',xml:source});s.endpoint=new URL(s.descriptor.endpoint,new URL(responseURL,global.location?.href)).href;for(const op of s.descriptor.operations)s.callMap.set(op.name,makeCall(proxy,op));}
     catch(e){failed(fault('Client.WSDL',e));return;}
     s.log?.logInfo('WSDL ready');if(!live(s))return;callback(proxy,'onLoad',xml(source));if(!live(s))return;for(const queued of s.queue.splice(0)){if(!live(s))return;begin(proxy,queued.target,queued.name,queued.params);}
    },failed);
   });
   function failed(error){if(!live(s))return;s.error=error;callback(proxy,'onFault',error);if(!live(s))return;for(const queued of s.queue.splice(0)){if(!live(s))return;completeFailure(queued.target,error);}}
   return proxy;
  }
  getCall(name){const s=state(services,this);if(!s.descriptor)throw new Error('Wait for WebService.onLoad before getCall().');const call=s.callMap.get(name);if(!call)throw new Error(`Unknown WSDL operation '${name}'.`);return call;}
 }
 function event(target,type,extra={}){try{target.dispatchEvent({type,target,...extra});options.afterEvent?.();}catch(e){report(e);}}
 function status(target,code,data=null){event(target,'status',{code,data});}
 function initConnector(target,kind){if(records.size>=128)throw new RangeError('At most 128 service/connector instances per publication.');const r=allocate(target,connectors,{kind,target,count:0,service:null,serviceURL:null,loading:[],URL:'',WSDLURL:'',operation:'',direction:'send/receive',ignoreWhite:false,multipleSimultaneousAllowed:false,suppressInvalidCalls:true,params:kind==='xml'?null:[],results:null});records.add(r);EventDispatcher.initialize(target);ComponentMixins?.initComponent(target);}
 function gather(target,r){if(!r.multipleSimultaneousAllowed&&r.count){status(target,'CallAlreadyInProgress');return false;}target.refreshFromSources?.();if(!live(r))return false;event(target,'send');if(!live(r))return false;const errors=target.validateProperty?.('params');if(errors){status(target,'InvalidParams',errors);if(r.suppressInvalidCalls)return false;}return true;}
 function busy(target,r,delta){r.count+=delta;status(target,'StatusChange',{callsInProgress:r.count});}
 function connectorFailure(target,r,value){if(!live(r))return;busy(target,r,-1);if(!live(r))return;status(target,r.kind==='xml'?'Fault':'WebServiceFault',value);}
 function connectorResult(target,r,value,sendOnly=false){if(!live(r))return;busy(target,r,-1);if(!live(r))return;if(!sendOnly){r.results=value;target.refreshDestinations?.();event(target,'result');}}
 class XMLConnector{
  constructor(){initConnector(this,'xml');}
  trigger(){const r=state(connectors,this);if(!gather(this,r))return;const direction=r.direction||'send/receive';if(!['send','receive','send/receive'].includes(direction)){status(this,'Fault',fault('XMLConnector.Direction','Use send, receive or send/receive.'));return;}
   let source=null;if(direction!=='receive'){
    if(!LegacyXML.__nfIs?.(r.params)){status(this,'Fault',fault('XMLConnector.Not.XML','params requires an AS2 XML document.'));return;}
    try{if(r.params.status!==0)throw new Error('params has an XML parsing error.');source=r.params.toString();core({op:'validateXml',xml:source});}catch(e){status(this,'Fault',fault('XMLConnector.Parse.Error',e));return;}
   }
   const request=new net.URLRequest(r.URL);if(source!==null){request.method='POST';request.contentType='text/xml; charset=utf-8';request.data=source;}const ignore=r.ignoreWhite;busy(this,r,1);if(!live(r))return;
   load(request,(data)=>{if(!live(r))return;if(direction==='send'){connectorResult(this,r,null,true);return;}let result;try{core({op:'validateXml',xml:data});result=xml(data,ignore);}catch(e){connectorFailure(this,r,fault('XMLConnector.Results.Parse.Error',e));return;}connectorResult(this,r,result);},error=>connectorFailure(this,r,error));
  }
 }
 class WebServiceConnector{
  constructor(){initConnector(this,'service');}
  trigger(){const r=state(connectors,this);if(!gather(this,r))return;const args=r.params;if(!Array.isArray(args)&&!(args&&typeof args==='object')){status(this,'InvalidParams');if(r.suppressInvalidCalls)return;status(this,'WebServiceFault',fault('Client.InvalidParams','params requires an Array or named Object.'));return;}
   busy(this,r,1);if(!live(r))return;const job={operation:r.operation,params:args};
   if(r.service&&r.serviceURL===r.WSDLURL){const s=state(services,r.service);if(s.error){connectorFailure(this,r,s.error);return;}if(s.descriptor){runConnector(this,r,job);return;}r.loading.push(job);return;}
   if(r.loading.length){connectorFailure(this,r,fault('Client.WSDL','Cannot change WSDLURL while loading.'));return;}
   let service;try{service=new WebService(r.WSDLURL);}catch(e){connectorFailure(this,r,fault('Client.WSDL',e));return;}r.service=service;r.serviceURL=r.WSDLURL;r.loading.push(job);
   service.onLoad=()=>{for(const next of r.loading.splice(0))runConnector(this,r,next);};service.onFault=value=>{for(const ignored of r.loading.splice(0))connectorFailure(this,r,value);};
  }
 }
 function runConnector(target,r,job){if(!live(r))return;const call=invoke(r.service,job.operation,job.params);call.onResult=value=>connectorResult(target,r,value);call.onFault=value=>connectorFailure(target,r,value);}
 for(const C of [XMLConnector,WebServiceConnector])for(const name of C===XMLConnector?['URL','direction','ignoreWhite','multipleSimultaneousAllowed','params','results','suppressInvalidCalls']:['WSDLURL','operation','multipleSimultaneousAllowed','params','results','suppressInvalidCalls'])Object.defineProperty(C.prototype,name,{get(){return state(connectors,this)[name];},set(value){const r=state(connectors,this);if(['URL','WSDLURL','operation','direction'].includes(name)){if(typeof value!=='string'||value.length>8192)throw new TypeError(`${name} requires a string up to 8192 characters.`);}else if(['ignoreWhite','multipleSimultaneousAllowed','suppressInvalidCalls'].includes(name))value=Boolean(value);r[name]=value;}});
 const classes={WebService,SOAPCall,PendingCall,Log,XMLConnector,WebServiceConnector};for(const[name,C]of Object.entries(classes)){const map=name==='WebService'?services:name==='SOAPCall'?calls:name==='PendingCall'?pending:name==='Log'?logs:connectors;Object.defineProperties(C,{__nfIs:{value:value=>map.has(value)},__nfInitialize:{value:()=>{throw new Error(`${name} native subclassing is outside this service profile.`);}}});}
 const builtins={'mx.services.WebService':WebService,'mx.services.SOAPCall':SOAPCall,'mx.services.PendingCall':PendingCall,'mx.services.Log':Log,'mx.data.components.XMLConnector':XMLConnector,'mx.data.components.WebServiceConnector':WebServiceConnector};
 function reset(){if(disposed)return;for(const token of [...transfers])cancel(token);generation++;records.clear();}
 function dispose(){if(disposed)return;reset();disposed=true;}
 return{builtins,reset,dispose};
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Services=api;
})(globalThis);
