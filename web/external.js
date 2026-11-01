/* Original synchronous web-container bridge. References:
 * https://airsdk.dev/reference/actionscript/3.0/flash/external/ExternalInterface.html
 * https://open-flash.github.io/mirrors/as2-language-reference/flash/external/ExternalInterface.html
 * Named dotted functions only (no eval); callbacks live on the publication's
 * canvas DOM element. Values cross as copied data, not live native objects.
 * This is a bounded HTML5 bridge, not the browser-plugin XML wire protocol.
 * Cycles/native values/accessor records are explicit boundaries. Public source
 * class slots can flatten through the program's existing JSON metadata.
 */
(function(global){
  'use strict';
  const MAX_VALUES=16384,MAX_DEPTH=64,MAX_STRING_UNITS=2*1024*1024,MAX_CALLBACKS=256,MAX_CALL_DEPTH=32;
  const identifier=/^[$_\p{ID_Start}][$_\u200c\u200d\p{ID_Continue}]*$/u;
  const forbidden=new Set(['__proto__','prototype','constructor']);
  function names(value,callback=false){
    if(typeof value!=='string'||!value||value.length>1024)throw new TypeError('ExternalInterface requires a nonempty function name of at most 1024 characters.');
    const parts=value.split('.');
    if(parts.length>32||(callback&&parts.length!==1)||parts.some(part=>!identifier.test(part)||forbidden.has(part)))throw new TypeError('ExternalInterface accepts named functions and dotted host paths only; prototype paths and expression evaluation are unsupported.');
    return parts;
  }
  function plain(value){const p=Object.getPrototypeOf(value);if(p===null||p===Object.prototype)return true;const ctor=Object.getOwnPropertyDescriptor(p,'constructor')?.value;return Object.getPrototypeOf(p)===null&&typeof ctor==='function'&&ctor.name==='Object'&&ctor.prototype===p;}
  function create(options={}){
    const host=options.hostObject??null,hostGlobal=options.hostGlobal??null,callbacks=new Map(),forms=new Map();
    let generation=1,disposed=false,exceptions=false,depth=0,navigationSequence=0;
    function available(){return !disposed&&host!==null&&hostGlobal!==null;}
    function current(token){if(disposed||token!==generation)throw new Error('This ExternalInterface call belongs to a retired publication session.');}
    function diagnostic(error){try{if(options.onDiagnostic)options.onDiagnostic(error);else global.console?.warn?.('NeoFlash ExternalInterface:',error);}catch(_){/* Diagnostics must not alter the bridge result. */}}
    function exception(error,enabled){
      const cause=options.unwrapError?options.unwrapError(error):error;
      if(enabled){if(cause instanceof Error)throw cause;throw new Error('ExternalInterface callback failed: '+String(cause),{cause});}
      diagnostic(cause);return null;
    }
    function copy(value,token){
      const seen=new Map(),active=new Set();let count=0,units=0;
      function text(value){units+=value.length;if(units>MAX_STRING_UNITS)throw new RangeError('ExternalInterface data exceeds 2 Mi UTF-16 string units.');return value;}
      function visit(value,level){
        current(token);if(++count>MAX_VALUES||level>MAX_DEPTH)throw new RangeError('ExternalInterface data exceeds 16384 values or 64 levels.');
        if(value===null||value===undefined||typeof value==='boolean'||typeof value==='number')return value;
        if(typeof value==='string')return text(value);
        if(typeof value!=='object')throw new TypeError('ExternalInterface supports data values, not functions, Symbols or BigInts.');
        if(active.has(value))throw new TypeError('ExternalInterface cyclic values are unsupported; pass an acyclic data record.');
        if(seen.has(value))return seen.get(value);
        if(options.isUnsupported?.(value)||Object.getOwnPropertySymbols(value).length)throw new TypeError('ExternalInterface native/display values require an explicit plain data record.');
        const array=Array.isArray(value),publicKeys=array?null:options.getProgram?.()?.jsonKeys?.(value);
        if(!array&&publicKeys==null&&!plain(value))throw new TypeError('ExternalInterface cannot marshal this native object; use primitive values, arrays or plain/public data records.');
        if(array&&value.length>MAX_VALUES)throw new RangeError('ExternalInterface array exceeds 16384 entries.');
        const result=array?new Array(value.length):{},keys=publicKeys??Object.keys(value);
        if(keys.length>MAX_VALUES)throw new RangeError('ExternalInterface record exceeds 16384 properties.');
        seen.set(value,result);active.add(value);
        for(const key of keys){
          text(key);const descriptor=Object.getOwnPropertyDescriptor(value,key);let entry;
          if(publicKeys!=null)entry=value[key];
          else {if(!descriptor)continue;if(!Object.hasOwn(descriptor,'value'))throw new TypeError('ExternalInterface ordinary records must contain data properties; accessors are unsupported.');entry=descriptor.value;}
          current(token);Object.defineProperty(result,key,{value:visit(entry,level+1),writable:true,enumerable:true,configurable:true});
        }
        active.delete(value);return result;
      }
      return visit(value,0);
    }
    function boundary(legacy,operation){
      if(depth>=MAX_CALL_DEPTH)throw new RangeError('ExternalInterface synchronous call depth exceeds 32.');
      // AVM1 does not support recursive ExternalInterface crossings. AS3 has a
      // bounded synchronous stack; no global mutable caller scope is installed.
      if(legacy&&depth){diagnostic(new Error('Recursive AS2 ExternalInterface calls are unsupported.'));return null;}
      depth++;try{return operation();}finally{depth--;}
    }
    function call(legacy,name,...args){
      const parts=names(name);if(!available())return null;
      const token=generation,enabled=!legacy&&exceptions;
      return boundary(legacy,()=>{
        const input=copy(args,token);let receiver=hostGlobal,method;
        try{
          for(let i=0;i<parts.length;i++){
            if(receiver===null||receiver===undefined)return null;
            method=Reflect.get(Object(receiver),parts[i]);current(token);
            if(i<parts.length-1)receiver=method;
          }
        }catch(error){current(token);return exception(error,enabled);}
        if(typeof method!=='function')return null;
        let result;try{result=Reflect.apply(method,receiver,input);}catch(error){current(token);return exception(error,enabled);}
        current(token);return copy(result,token);
      });
    }
    function addCallback(legacy,name,receiver,method){
      names(name,true);
      if(!available()){if(legacy)return false;throw new Error('ExternalInterface is unavailable without a live publication DOM host.');}
      const previous=callbacks.get(name),descriptor=Object.getOwnPropertyDescriptor(host,name);
      const owned=previous&&descriptor?.value===previous.wrapper;
      if(method===null&&!legacy){if(owned)delete host[name];callbacks.delete(name);return;}
      if(typeof method!=='function')throw new TypeError('ExternalInterface callback must be a function.');
      if(!owned&&name in host)throw new TypeError(`ExternalInterface callback '${name}' collides with an existing host property.`);
      if(!previous&&callbacks.size>=MAX_CALLBACKS)throw new RangeError('ExternalInterface supports at most 256 callbacks.');
      const token=generation,record={wrapper:null};
      record.wrapper=function(...args){
        current(token);if(callbacks.get(name)!==record)throw new Error('This ExternalInterface callback was removed or replaced.');
        const enabled=!legacy&&exceptions;
        return boundary(legacy,()=>{
          const input=copy(args,token);let result,failure,threw=false;
          try{result=Reflect.apply(method,legacy?receiver:null,input);}catch(error){failure=error;threw=true;}
          current(token);
          // Rendering belongs to this generation, even if user code threw after
          // changing display state. Stale callbacks never publish a new frame.
          options.afterCallback?.();current(token);
          if(threw)return exception(failure,enabled);
          return copy(result,token);
        });
      };
      Object.defineProperty(host,name,{value:record.wrapper,writable:false,enumerable:false,configurable:true});callbacks.set(name,record);
      if(legacy)return true;
    }
    function staticFacade(legacy){
      const facade=function(){throw new TypeError('ExternalInterface is a static class and cannot be constructed or extended.');};
      const properties={available:{get:available},call:{value:(...args)=>call(legacy,...args)},addCallback:{value:legacy?(name,instance,method)=>addCallback(true,name,instance,method):(name,closure)=>addCallback(false,name,null,closure)},__nfIs:{value:()=>false},__nfInitialize:{value:()=>{throw new TypeError('ExternalInterface native subclassing is unsupported.');}}};
      if(!legacy)Object.assign(properties,{objectID:{get:()=>available()?String(host.id||''):null},marshallExceptions:{get:()=>exceptions,set:value=>{if(disposed)throw new Error('ExternalInterface was disposed.');if(typeof value!=='boolean')throw new TypeError('marshallExceptions requires Boolean.');exceptions=value;}}});
      Object.defineProperties(facade,properties);return Object.freeze(facade);
    }
    function navigate(prepared){
      if(disposed)throw new Error('Browser navigation belongs to a disposed publication.');
      if(typeof options.navigate==='function')return options.navigate(prepared);
      if(!hostGlobal)throw new Error('Browser navigation is unavailable in this host.');
      const {url,method,fields}=prepared;let target=prepared.target;
      if(method==='GET'){
        if(target==='_self'||target==='_parent'||target==='_top'){
          const destination=target==='_self'?hostGlobal:hostGlobal[target==='_top'?'top':'parent'];
          if(typeof destination?.location?.assign!=='function')throw new Error('Browser location navigation is unavailable.');
          destination.location.assign(url);return;
        }
        if(typeof hostGlobal.open!=='function'||!hostGlobal.open(url,target))throw new Error('Browser blocked this window navigation. Invoke it from a user gesture and allow popups for this publication.');
        return;
      }
      const document=hostGlobal.document,submit=hostGlobal.HTMLFormElement?.prototype?.submit;
      if(method!=='POST'||!document?.body||typeof document.createElement!=='function'||typeof submit!=='function')throw new Error('Browser form navigation is unavailable.');
      const form=document.createElement('form'),append=form.appendChild.bind(form),remove=form.remove.bind(form);
      form.method='post';form.action=url;form.acceptCharset='UTF-8';form.enctype='application/x-www-form-urlencoded';form.hidden=true;
      if(target==='_blank')target=`neoflash_navigation_${generation}_${++navigationSequence}_${Date.now()}`;
      form.target=target;
      // Capture native methods before named controls can shadow form members.
      for(const[name,value]of fields){const input=document.createElement('input');input.type='hidden';input.name=name;input.value=value;append(input);}
      if(!['_self','_parent','_top'].includes(target)){
        if(typeof hostGlobal.open!=='function'||!hostGlobal.open('',target))throw new Error('Browser blocked the form navigation window. Submit it from a user gesture and allow popups for this publication.');
      }
      document.body.appendChild(form);
      try{Reflect.apply(submit,form,[]);}catch(error){remove();throw error;}
      // Removing a form during submit can cancel its queued navigation. Keep
      // it connected through this task, and retire cleanup on Restart/dispose.
      const timer=global.setTimeout(()=>{forms.delete(form);remove();},0);forms.set(form,{timer,remove});
    }
    function fscommand(command,args=''){
      if(disposed)throw new Error('Browser fscommand belongs to a disposed publication.');
      const cmd=command===undefined||command===null?'':String(command);
      const param=args===undefined||args===null?'':String(args);
      if(!cmd||['quit','fullscreen','allowscale','showmenu','exec','trapallkeys'].includes(cmd.toLowerCase()))return;
      if(!hostGlobal)return;
      const token=generation;
      const prefix=host?.id&&identifier.test(String(host.id))&&!forbidden.has(String(host.id))?`${host.id}_DoFSCommand`:null;
      const fn=(prefix&&typeof hostGlobal[prefix]==='function'?hostGlobal[prefix]:null)||(typeof hostGlobal.DoFSCommand==='function'?hostGlobal.DoFSCommand:null);
      if(!fn)return;
      boundary(true,()=>{
        try{Reflect.apply(fn,hostGlobal,[cmd,param]);}catch(error){current(token);exception(error,false);return null;}
        current(token);options.afterCallback?.();return null;
      });
    }
    function reset(){
      generation++;exceptions=false;
      for(const[name,record]of callbacks){if(Object.getOwnPropertyDescriptor(host,name)?.value===record.wrapper)delete host[name];}
      callbacks.clear();
      for(const{timer,remove}of forms.values()){global.clearTimeout(timer);remove();}forms.clear();
    }
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({ExternalInterface:staticFacade(false),LegacyExternalInterface:staticFacade(true),navigate,fscommand,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashExternal=api;
})(globalThis);
