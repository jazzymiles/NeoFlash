/* Portable data loading; original implementation, browser transport semantics.
 * Primary contracts: https://airsdk.dev/reference/actionscript/3.0/flash/net/URLLoader.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/net/URLRequest.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/net/URLVariables.html
 * https://open-flash.github.io/mirrors/as2-language-reference/LoadVars.html
 *
 * create({events,binary,resources,fetch,baseURL,schedule,onError,onAfterDispatch})
 * owns all active operations. schedule(callback) must be asynchronous and one-shot.
 * Embedded GETs work offline; network GET/POST uses browser HTTP(S), CORS and
 * same-origin credentials. No filesystem, desktop credentials or Flash policy
 * file emulation. Text/form data is UTF-8; malformed UTF-8/form escapes diagnose.
 * Limits: 16 MiB/response or body, 64 MiB active response bytes, 32 concurrent
 * operations, 1024 form pairs/headers capped at64. No native subclassing, URLStream,
 * multipart/FormData, file upload, authentication prompts, automatic retry,
 * timeout configuration, or historical security/errorID/redirect-order claim.
 * Navigation uses an injected synchronous navigate({url,method,target,fields})
 * browser adapter. sendToURL shares request policy but never reads the response
 * or dispatches loader events; reset aborts pending sends. Neither promises
 * delivery across page unload. Navigation supports HTTP(S) GET/form POST only.
 */
(function(global){
  'use strict';
  const MAX_BYTES=16*1024*1024,MAX_ACTIVE_BYTES=64*1024*1024,MAX_OPERATIONS=32,MAX_PAIRS=1024;
  const unsupported=name=>{throw new TypeError(`${name} is not supported in the portable data-loading profile.`);};
  function text(value,name){if(typeof value!=='string')throw new TypeError(`${name} requires a string.`);return value;}
  const encoder=new TextEncoder(),decoder=new TextDecoder('utf-8',{fatal:true});
  function utf8(value,name){text(value,name);const bytes=encoder.encode(value);if(bytes.length>MAX_BYTES)throw new RangeError(`${name} exceeds 16 MiB.`);return bytes;}
  function primitive(value){if(value===null)return 'null';if(value===undefined)return 'undefined';if(!['string','number','boolean'].includes(typeof value))throw new TypeError('Form values require strings, numbers, Booleans or arrays of these values.');return String(value);}
  const escape=value=>encodeURIComponent(primitive(value)).replace(/%20/g,'+').replace(/[!'()*]/g,c=>'%'+c.charCodeAt(0).toString(16).toUpperCase());
  function parseForm(source){
    utf8(source,'URL-encoded data');const result=[];
    if(!source)return result;
    for(const pair of source.split('&')){
      if(!pair)continue;
      if(result.length>=MAX_PAIRS)throw new RangeError('Form data exceeds 1024 name/value pairs.');
      const split=pair.indexOf('=');if(split<0)throw new TypeError('URL-encoded data requires name=value pairs.');
      try {result.push([decodeURIComponent(pair.slice(0,split).replace(/\+/g,' ')),decodeURIComponent(pair.slice(split+1).replace(/\+/g,' '))]);}
      catch(_){throw new TypeError('URL-encoded data contains malformed UTF-8 or percent escapes.');}
    }
    return result;
  }
  function mergeFields(target,pairs){
    const grouped=new Map();for(const[key,value]of pairs){if(grouped.has(key)){const old=grouped.get(key);grouped.set(key,Array.isArray(old)?[...old,value]:[old,value]);}else grouped.set(key,value);}
    for(const[key,value]of grouped)Object.defineProperty(target,key,{value,writable:true,configurable:true,enumerable:true});
  }
  function formString(target,skip=new Set()){
    const pairs=[];
    for(const key of Object.keys(target)){
      if(skip.has(key))continue;
      const descriptor=Object.getOwnPropertyDescriptor(target,key);
      if(!Object.hasOwn(descriptor,'value'))throw new TypeError('Form serialization currently requires enumerable data properties; accessors are unsupported.');
      for(const value of Array.isArray(descriptor.value)?descriptor.value:[descriptor.value]){
        if(pairs.length>=MAX_PAIRS)throw new RangeError('Form data exceeds 1024 name/value pairs.');
        pairs.push(escape(key)+'='+escape(value));
      }
    }
    const result=pairs.join('&');utf8(result,'Form body');return result;
  }
  function noBase(Constructor,name,predicate){
    Object.defineProperties(Constructor,{__nfIs:{value:predicate},__nfInitialize:{value:()=>unsupported(`${name} native subclassing`)}});
  }
  function create(options={}){
    const events=options.events||global.NeoFlashEvents,binary=options.binary;
    if(!events?.create||!binary?.fromBytes||!binary?.toBytes)throw new TypeError('Data loading requires the shared Event and ByteArray services.');
    const schedule=options.schedule||(callback=>global.queueMicrotask(callback));
    const fetchTransport=options.fetch||(typeof global.fetch==='function'?global.fetch.bind(global):null);
    const baseURL=options.baseURL||global.location?.href||null;
    const requests=new WeakSet(),headers=new WeakSet(),variables=new WeakSet(),loaders=new WeakMap(),legacy=new WeakMap();
    const active=new Set(),resources=new Map(),resourceCache=new Map();
    let generation=1,disposed=false,activeBytes=0;
    let resourceBytes=0;
    if((options.resources||[]).length>1024)throw new RangeError('A publication supports at most 1024 portable resources.');
    for(const resource of options.resources||[]){
      const encoded=text(resource.data_base64,'Resource base64'),length=encoded.length/4*3-(encoded.endsWith('==')?2:encoded.endsWith('=')?1:0);
      if(!Number.isInteger(length)||length<0||length>MAX_BYTES||resourceBytes+length>MAX_ACTIVE_BYTES)throw new RangeError('Portable resources exceed their 16 MiB each / 64 MiB total limit.');
      if(resources.has(resource.path))throw new TypeError('Duplicate portable resource path.');
      resources.set(resource.path,{...resource,byteLength:length});resourceBytes+=length;
    }
    const hub=events.create({weakTargets:true,alive:target=>!disposed&&loaders.get(target)?.generation===generation});
    function open(){if(disposed)throw new Error('This data-loading service has been disposed.');}
    function state(receiver,table=loaders){open();const record=table.get(receiver);if(!record)throw new TypeError('Expected a loader from this publication.');if(record.generation!==generation)throw new Error('This loader belongs to a previous publication session.');return record;}
    function report(error){if(options.onError)options.onError(error);else throw error;}
    class CallbackFailure {constructor(cause){this.cause=cause;}}
    function current(operation){return !disposed&&operation.generation===generation&&operation.record.operation===operation;}
    function notify(operation,event){
      if(!current(operation))return false;
      try {if(operation.record.raw)operation.record.raw.onEvent?.(event);else hub.dispatch(operation.record.receiver,event);if(current(operation)&&!operation.record.raw)options.onAfterDispatch?.();}
      catch(error){throw new CallbackFailure(error);}
      return current(operation);
    }
    function callback(operation,name,args,resolved){
      if(!current(operation))return false;
      try {const fn=arguments.length===4?resolved:operation.record.receiver[name];if(!current(operation))return false;if(typeof fn==='function')Reflect.apply(fn,operation.record.receiver,args);if(current(operation))options.onAfterDispatch?.();}
      catch(error){throw new CallbackFailure(error);}
      return current(operation);
    }
    function release(operation){activeBytes-=operation.retained;operation.retained=0;active.delete(operation);if(operation.record.operation===operation)operation.record.operation=null;}
    function cancel(record){const operation=record.operation;if(!operation)return;record.operation=null;operation.controller.abort();operation.reader?.cancel().catch(()=>{});release(operation);}
    function retained(operation,count){if(count>MAX_BYTES||activeBytes-operation.retained+count>MAX_ACTIVE_BYTES)throw new RangeError('Response exceeds the 16 MiB response or 64 MiB active-response limit.');activeBytes+=count-operation.retained;operation.retained=count;}
    class URLRequestHeader {constructor(name='',value=''){this.name=text(name,'Header name');this.value=text(value,'Header value');headers.add(this);Object.seal(this);}}
    class URLRequest {constructor(url=null){this.url=url===null?null:text(url,'Request URL');this.method='GET';this.data=null;this.contentType='application/x-www-form-urlencoded';this.requestHeaders=[];requests.add(this);Object.seal(this);}}
    class URLRequestMethod {constructor(){unsupported('URLRequestMethod construction');}}
    Object.assign(URLRequestMethod,{GET:'GET',POST:'POST'});Object.freeze(URLRequestMethod);
    class URLLoaderDataFormat {constructor(){unsupported('URLLoaderDataFormat construction');}}
    Object.assign(URLLoaderDataFormat,{TEXT:'text',BINARY:'binary',VARIABLES:'variables'});Object.freeze(URLLoaderDataFormat);
    class URLVariables {
      constructor(source=null){variables.add(this);if(source!==null)mergeFields(this,parseForm(source));}
      decode(source){mergeFields(this,parseForm(source));}
      toString(){return formString(this);}
    }
    noBase(URLRequest,'URLRequest',value=>requests.has(value));noBase(URLRequestHeader,'URLRequestHeader',value=>headers.has(value));noBase(URLVariables,'URLVariables',value=>variables.has(value));
    const progressEvents=new WeakSet(),ioEvents=new WeakSet(),statusEvents=new WeakSet();
    class ProgressEvent extends events.Event {
      constructor(type,bubbles=false,cancelable=false,bytesLoaded=0,bytesTotal=0){super(type,bubbles,cancelable);this.bytesLoaded=bytesLoaded;this.bytesTotal=bytesTotal;progressEvents.add(this);}
      clone(){return new ProgressEvent(this.type,this.bubbles,this.cancelable,this.bytesLoaded,this.bytesTotal);}
    }
    ProgressEvent.PROGRESS='progress';noBase(ProgressEvent,'ProgressEvent',value=>progressEvents.has(value));
    class IOErrorEvent extends (events.ErrorEvent || events.Event) {
      constructor(type,bubbles=false,cancelable=false,message='',id=0){super(type,bubbles,cancelable,message,id);this.text=text(message,'IOErrorEvent text');if(!events.ErrorEvent)Object.defineProperty(this,'errorID',{value:id,enumerable:true});ioEvents.add(this);}
      clone(){return new IOErrorEvent(this.type,this.bubbles,this.cancelable,this.text,this.errorID);}
    }
    IOErrorEvent.IO_ERROR='ioError';noBase(IOErrorEvent,'IOErrorEvent',value=>ioEvents.has(value));
    class HTTPStatusEvent extends events.Event {
      constructor(type,bubbles=false,cancelable=false,status=0,redirected=false){super(type,bubbles,cancelable);Object.defineProperties(this,{status:{value:status,enumerable:true},redirected:{value:Boolean(redirected),enumerable:true}});this.responseURL='';this.responseHeaders=[];statusEvents.add(this);}
      clone(){const event=new HTTPStatusEvent(this.type,this.bubbles,this.cancelable,this.status,this.redirected);event.responseURL=this.responseURL;event.responseHeaders=this.responseHeaders.slice();return event;}
    }
    HTTPStatusEvent.HTTP_STATUS='httpStatus';noBase(HTTPStatusEvent,'HTTPStatusEvent',value=>statusEvents.has(value));
    function headerPairs(input){
      if(!Array.isArray(input)||input.length>64)throw new TypeError('requestHeaders requires an Array of at most 64 URLRequestHeader objects.');
      const result=[];
      for(const header of input){
        if(!headers.has(header))throw new TypeError('requestHeaders entries must be URLRequestHeader objects.');
        const name=text(header.name,'Header name'),value=text(header.value,'Header value');
        if(!/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(name)||/[\r\n\0]/.test(value))throw new TypeError('Invalid HTTP header name or value.');
        if(/^(?:accept-charset|accept-encoding|access-control-request-.*|connection|content-length|cookie2?|date|dnt|expect|host|keep-alive|origin|permissions-policy|referer|te|trailer|transfer-encoding|upgrade|via)$/i.test(name)||/^(?:proxy-|sec-)/i.test(name))throw new TypeError(`Header '${name}' is controlled by the browser and cannot be supplied by project code.`);
        if(name.length+value.length>8192)throw new RangeError('HTTP header exceeds 8192 characters.');result.push([name,value]);
      }
      return result;
    }
    function snapshot(request){
      if(!requests.has(request))throw new TypeError('load requires a URLRequest.');
      let url=text(request.url,'Request URL');if(!url||url.length>8192)throw new RangeError('Request URL must contain 1 through 8192 characters.');
      const method=request.method;if(method!=='GET'&&method!=='POST')unsupported(`URLRequest method ${method}`);
      const requestHeaders=headerPairs(request.requestHeaders),contentType=text(request.contentType,'contentType');
      if(/[\r\n\0]/.test(contentType))throw new TypeError('Invalid contentType.');
      let body=null;
      if(request.data!==null&&request.data!==undefined){
        if(binary.ByteArray.__nfIs(request.data))body=binary.toBytes(request.data);
        else body=utf8(variables.has(request.data)?formString(request.data):text(request.data,'Request data'),'Request body');
      }
      if(body&&body.byteLength>MAX_BYTES)throw new RangeError('Request body exceeds 16 MiB.');
      if(method==='GET'&&body){
        if(binary.ByteArray.__nfIs(request.data))throw new TypeError('GET data must be URLVariables or an encoded string; binary GET bodies are unsupported.');
        const fragment=url.indexOf('#'),suffix=fragment<0?'':url.slice(fragment);if(fragment>=0)url=url.slice(0,fragment);
        url+=(url.includes('?')?'&':'?')+decoder.decode(body)+suffix;body=null;
      }
      if(method==='POST'&&body&&!requestHeaders.some(([name])=>name.toLowerCase()==='content-type'))requestHeaders.push(['Content-Type',contentType]);
      if(url.length>8192)throw new RangeError('GET URL data exceeds 8192 characters; use POST for larger form data.');
      return {url,method,body,headers:requestHeaders};
    }
    function httpURL(source){
      let url;try{url=new URL(source,baseURL||undefined);}catch(_){throw new TypeError('Request requires an absolute HTTP(S) URL or an HTTP(S) publication base URL.');}
      if(!['http:','https:'].includes(url.protocol))throw new TypeError('Only browser HTTP(S) requests and navigation are supported; filesystem and scripting URLs are unavailable.');
      if(url.username||url.password)throw new TypeError('Credentials embedded in request URLs are unsupported; use an explicit permitted request header for data loading.');
      if(url.href.length>8192)throw new RangeError('Resolved request URL exceeds 8192 characters.');
      return url.href;
    }
    function navigationTarget(value){
      if(value===null||value===undefined||value==='')return '_blank';
      const target=text(value,'Browser window target');
      if(target.length>256||/[\s\x00-\x1f\x7f<>]/.test(target))throw new TypeError('Browser target requires a bounded window name without whitespace/control characters.');
      const lower=target.toLowerCase();
      if(['_self','_blank','_parent','_top'].includes(lower))return lower;
      if(target.startsWith('_'))throw new TypeError('Unknown reserved browser target. Use _self, _blank, _parent, _top or a named window.');
      return target;
    }
    function prepareNavigation(request,target=null){
      open();const prepared=snapshot(request),url=httpURL(prepared.url),window=navigationTarget(target);
      const formType=/^application\/x-www-form-urlencoded(?:\s*;\s*charset\s*=\s*(?:UTF-8|"UTF-8"))?\s*$/i;
      // HTML navigation cannot attach custom HTTP headers. A form's own
      // Content-Type is the sole allowed transport-generated header.
      if(prepared.headers.some(([name,value])=>prepared.method!=='POST'||name.toLowerCase()!=='content-type'||!formType.test(value)))throw new TypeError('Browser navigation cannot send custom headers or a non-form Content-Type. Use URLLoader or sendToURL.');
      let fields=[];
      if(prepared.method==='POST'){
        if(binary.ByteArray.__nfIs(request.data))throw new TypeError('Browser POST navigation requires URLVariables or a URL-encoded string, not ByteArray.');
        if(!formType.test(request.contentType))throw new TypeError('Browser POST navigation supports application/x-www-form-urlencoded UTF-8 only.');
        fields=parseForm(prepared.body?decoder.decode(prepared.body):'').map(pair=>Object.freeze(pair));
      }
      return Object.freeze({url,method:prepared.method,target:window,fields:Object.freeze(fields)});
    }
    function navigatePrepared(prepared){
      if(typeof options.navigate!=='function')throw new Error('Browser navigation is unavailable in this host.');
      // Do not defer: the actual browser operation must retain the caller's
      // user-activation context. Popup/sandbox policy belongs to that browser.
      options.navigate(prepared);
    }
    function navigateToURL(request,target=null){navigatePrepared(prepareNavigation(request,target));}
    function getURL(owner,url,target='_self',method=null){
      open();const request=new URLRequest(url);
      if(method!==null&&method!==undefined&&method!==''){
        request.method=method;
        const values=typeof options.legacyVariables==='function'?options.legacyVariables(owner):owner;
        if(values===null||typeof values!=='object')throw new TypeError('getURL with a method requires an enumerable source-variable object.');
        request.data=formString(values);
      }
      navigatePrepared(prepareNavigation(request,target===null||target===undefined||target===''?'_self':target));
    }
    async function runSend(operation,prepared){
      try{
        const response=await fetchTransport(prepared.url,{method:prepared.method,headers:prepared.headers,body:prepared.body,signal:operation.controller.signal,credentials:'same-origin',mode:'cors'});
        // Success/error HTTP status and response bytes are deliberately ignored.
        // Cancel the stream without allocating an ArrayBuffer/ByteArray.
        try{await response.body?.cancel();}catch(_){}
      }catch(error){
        if(current(operation)){
          try{if(options.onSendError)options.onSendError(error);else global.console?.warn?.('sendToURL transport failed; no response is exposed.',error);}
          catch(callbackError){report(callbackError);}
        }
      }finally{operation.controller.abort();release(operation);}
    }
    function sendToURL(request){
      open();const prepared=snapshot(request);prepared.url=httpURL(prepared.url);
      if(!fetchTransport)throw new Error('This browser does not provide fetch.');
      if(active.size>=MAX_OPERATIONS)throw new RangeError('At most 32 resource/network operations may run concurrently.');
      const record={generation,operation:null},operation={record,generation,controller:new AbortController(),retained:0,reader:null};
      // Sending retains the request bytes until the browser accepts/completes
      // its request, within the same shared in-flight byte budget as loaders.
      retained(operation,prepared.body?.byteLength||0);record.operation=operation;active.add(operation);
      try{schedule(()=>{if(current(operation))void runSend(operation,prepared);});}catch(error){cancel(record);throw error;}
    }
    function resourceFor(url){
      if(/^[A-Za-z][A-Za-z0-9+.-]*:/.test(url)||url.startsWith('/'))return null;
      let path;try{path=decodeURIComponent(url.split(/[?#]/,1)[0]).replace(/^(?:\.\/)+/,'');}catch(_){throw new TypeError('Resource URL contains an invalid percent escape.');}
      if(path.includes('\\')||path.split('/').some(part=>part==='..'))throw new TypeError('Resource paths cannot traverse directories or use backslashes.');
      return resources.get(path)||null;
    }
    function decodeResource(resource){
      if(resourceCache.has(resource.path))return resourceCache.get(resource.path);
      const raw=global.atob(resource.data_base64);if(raw.length!==resource.byteLength)throw new TypeError('Portable resource base64 is malformed.');
      const bytes=Uint8Array.from(raw,char=>char.charCodeAt(0));resourceCache.set(resource.path,bytes);return bytes;
    }
    function mediaSource(value){
      open();const source=text(value,'Media URL');if(!source||source.length>8192)throw new RangeError('Media URL must contain 1 through 8192 characters.');
      const resource=resourceFor(source);
      if(resource)return {kind:'resource',bytes:decodeResource(resource).slice(),mediaType:resource.media_type,length:resource.byteLength};
      let url;try{url=new URL(source,baseURL||undefined);}catch(_){throw new TypeError('Media is not bundled and no HTTP(S) base URL is available.');}
      if(!['http:','https:'].includes(url.protocol))throw new TypeError('Media loading supports bundled resources and browser HTTP(S), not filesystem/RTMP URLs.');
      if(url.username||url.password)throw new TypeError('Credentials embedded in media URLs are unsupported.');
      // The native media element owns progressive HTTP buffering/range requests.
      // Its bytes are not fetched or falsely charged as a complete URLLoader file.
      return {kind:'http',url:url.href};
    }
    async function read(operation,request){
      const resource=resourceFor(request.url),record=operation.record;
      if(resource){
        if(request.method!=='GET')throw new TypeError('Bundled resources are immutable and support GET only; POST requires an HTTP(S) endpoint.');
        const bytes=decodeResource(resource);retained(operation,bytes.length);record.total=bytes.length;
        if(!operation.legacy&&!notify(operation,new events.Event('open')))return null;
        record.loadedBytes=bytes.length;
        if(!operation.legacy&&!notify(operation,new ProgressEvent('progress',false,false,bytes.length,bytes.length)))return null;
        return bytes;
      }
      let url;try{url=new URL(request.url,baseURL||undefined);}catch(_){throw new TypeError(`Resource '${request.url}' is not bundled and no HTTP(S) base URL is available.`);}
      if(!['http:','https:'].includes(url.protocol))throw new TypeError(`Resource '${request.url}' is not bundled. Only browser HTTP(S) loading is supported; filesystem URLs are unavailable.`);
      if(url.username||url.password)throw new TypeError('Credentials embedded in request URLs are unsupported; use an explicit permitted request header.');
      if(!fetchTransport)throw new Error('This browser does not provide fetch.');
      const response=await fetchTransport(url.href,{method:request.method,headers:request.headers,body:request.body,signal:operation.controller.signal,credentials:'same-origin',mode:'cors'});
      if(!current(operation))return null;
      if(response.type==='opaque')throw new Error('The browser did not expose this response; configure ordinary HTTP CORS access.');
      if(operation.legacy){if(!callback(operation,'onHTTPStatus',[response.status]))return null;}
      else {
        const event=new HTTPStatusEvent('httpStatus',false,false,response.status,response.redirected);event.responseURL=response.url||url.href;
        response.headers?.forEach?.((value,name)=>event.responseHeaders.push(new URLRequestHeader(name,value)));
        if(!notify(operation,event))return null;
      }
      if(!response.ok)throw new Error(`HTTP ${response.status} while loading ${url.href}.`);
      const declared=response.headers?.get?.('content-length'),compressed=response.headers?.get?.('content-encoding');
      record.total=!compressed&&declared&&/^\d+$/.test(declared)?Number(declared):0;
      if(!Number.isSafeInteger(record.total)||record.total>MAX_BYTES)throw new RangeError('Declared response size exceeds 16 MiB.');
      if(!operation.legacy&&!notify(operation,new events.Event('open')))return null;
      const chunks=[];
      if(response.body?.getReader){
        const reader=response.body.getReader();operation.reader=reader;
        for(;;){
          const {done,value}=await reader.read();if(!current(operation))return null;if(done)break;
          retained(operation,record.loadedBytes+value.byteLength);record.loadedBytes+=value.byteLength;chunks.push(value);
          if(!operation.legacy&&!notify(operation,new ProgressEvent('progress',false,false,record.loadedBytes,record.total)))return null;
        }
      }else{
        const bytes=new Uint8Array(await response.arrayBuffer());if(!current(operation))return null;
        retained(operation,bytes.length);record.loadedBytes=bytes.length;chunks.push(bytes);
        if(!operation.legacy&&!notify(operation,new ProgressEvent('progress',false,false,record.loadedBytes,record.total)))return null;
      }
      record.total=record.loadedBytes;
      const result=new Uint8Array(record.loadedBytes);let offset=0;for(const chunk of chunks){result.set(chunk,offset);offset+=chunk.length;}return result;
    }
    function defaultLegacyData(operation,source){
      const record=operation.record;
      if(source!==undefined){mergeFields(record.receiver,parseForm(source));record.loaded=true;}else record.loaded=false;
      callback(operation,'onLoad',[record.loaded]);
    }
    function legacyData(operation,source){
      let handler;try{handler=operation.record.receiver.onData;}catch(error){throw new CallbackFailure(error);}
      if(!current(operation))return;
      if(typeof handler==='function')callback(operation,'onData',[source],handler);else defaultLegacyData(operation,source);
    }
    async function run(operation,request){
      try{
        const bytes=await read(operation,request);if(bytes===null||!current(operation))return;
        if(operation.record.raw){
          try {operation.record.raw.onComplete(bytes);}catch(error){throw new CallbackFailure(error);}
        }else if(operation.legacy)legacyData(operation,decoder.decode(bytes));
        else{
          const record=operation.record;
          record.data=operation.format==='binary'?binary.fromBytes(bytes):operation.format==='variables'?new URLVariables(decoder.decode(bytes)):decoder.decode(bytes);
          notify(operation,new events.Event('complete'));
        }
      }catch(error){
        if(error instanceof CallbackFailure){if(disposed||operation.generation!==generation)return;if(current(operation))cancel(operation.record);report(error.cause);return;}
        if(!current(operation))return;
        try {if(operation.legacy)legacyData(operation,undefined);else notify(operation,new IOErrorEvent('ioError',false,false,error instanceof Error?error.message:String(error)));}
        catch(callbackError){if(disposed||operation.generation!==generation)return;if(current(operation))cancel(operation.record);report(callbackError instanceof CallbackFailure?callbackError.cause:callbackError);}
      }finally{operation.controller.abort();release(operation);}
    }
    function start(record,request,legacyMode=false){
      const prepared=snapshot(request);cancel(record);
      if(active.size>=MAX_OPERATIONS)throw new RangeError('At most 32 resource/network loads may run concurrently.');
      record.loadedBytes=0;record.total=0;if(legacyMode)record.loaded=false;else record.data=null;
      const operation={record,generation,legacy:legacyMode,format:record.format,controller:new AbortController(),retained:0,reader:null};
      record.operation=operation;active.add(operation);
      try {schedule(()=>{if(current(operation))void run(operation,prepared);});}catch(error){cancel(record);throw error;}
    }
    class URLLoader extends events.EventDispatcher {
      constructor(request=null){super();open();loaders.set(this,{receiver:this,generation,operation:null,data:null,format:'text',loadedBytes:0,total:0});Object.assign(this,hub.methods(this));Object.seal(this);if(request!==null)this.load(request);}
      get data(){return state(this).data;}set data(value){state(this).data=value;}
      get dataFormat(){return state(this).format;}set dataFormat(value){if(!['text','binary','variables'].includes(value))throw new TypeError('dataFormat requires text, binary or variables.');state(this).format=value;}
      get bytesLoaded(){return state(this).loadedBytes;}get bytesTotal(){return state(this).total;}
      load(request){start(state(this),request);}
      close(){cancel(state(this));}
    }
    noBase(URLLoader,'URLLoader',value=>loaders.has(value));
    // Host-only shared transport: consume complete bytes without allocating a
    // user ByteArray record. The same URL validation, budgets and cancellation
    // govern URLLoader, image loaders and any future binary consumers.
    function loadBytes(request,callbacks){
      open();if(!callbacks||typeof callbacks.onComplete!=='function')throw new TypeError('Raw transport requires an onComplete callback.');
      const record={receiver:null,generation,operation:null,data:null,format:'binary',loadedBytes:0,total:0,raw:callbacks};
      start(record,request);return Object.freeze({close:()=>cancel(record)});
    }
    const legacyReserved=new Set(['onLoad','onData','onHTTPStatus','loaded','contentType']);
    class LoadVars {
      constructor(){open();legacy.set(this,{receiver:this,generation,operation:null,loaded:undefined,loadedBytes:undefined,total:undefined,contentType:'application/x-www-form-urlencoded',headers:[]});}
      get contentType(){return state(this,legacy).contentType;}set contentType(value){state(this,legacy).contentType=text(value,'contentType');}
      get loaded(){return state(this,legacy).loaded;}set loaded(value){state(this,legacy).loaded=value;}
      decode(source){state(this,legacy);mergeFields(this,parseForm(source));}
      toString(){state(this,legacy);return formString(this,legacyReserved);}
      getBytesLoaded(){return state(this,legacy).loadedBytes;}getBytesTotal(){return state(this,legacy).total;}
      load(url){const request=new URLRequest(url);start(state(this,legacy),request,true);return true;}
      sendAndLoad(url,target,method='POST'){
        const source=state(this,legacy),destination=state(target,legacy),request=new URLRequest(url);request.method=method;
        request.data=formString(this,legacyReserved);request.contentType=source.contentType;request.requestHeaders=source.headers.slice();start(destination,request,true);return true;
      }
      addRequestHeader(name,value){const record=state(this,legacy),next=record.headers.slice();if(Array.isArray(name)){if(name.length%2)throw new TypeError('Header array requires alternating names and values.');for(let i=0;i<name.length;i+=2)next.push(new URLRequestHeader(name[i],name[i+1]));}else next.push(new URLRequestHeader(name,value));headerPairs(next);record.headers=next;}
      send(url,target,method='POST'){
        const record=state(this,legacy);
        if(arguments.length===0)return false;
        if(target===undefined)return true; // Documented AS2 missing-target no-op.
        const request=new URLRequest(url);request.method=method;request.data=formString(this,legacyReserved);
        request.contentType=record.contentType;request.requestHeaders=record.headers.slice();
        navigateToURL(request,target);return true;
      }
    }
    noBase(LoadVars,'LoadVars',value=>legacy.has(value));
    function reset(){open();for(const operation of [...active])cancel(operation.record);generation++;hub.clear();resourceCache.clear();}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({URLRequest,URLRequestHeader,URLRequestMethod,URLVariables,URLLoader,URLLoaderDataFormat,ProgressEvent,IOErrorEvent,HTTPStatusEvent,LoadVars,loadBytes,mediaSource,prepareNavigation,navigateToURL,sendToURL,getURL,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashNet=api;
})(globalThis);
