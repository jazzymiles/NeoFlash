/* AS2 timeline variables: browser I/O and object-reference bridge only.
 * Rust decodes/validates/group fields; the existing URLLoader owns transport. */
(function(global){
  'use strict';
  function create({wasm,getNet,resolveTarget,fieldsOf,alive,afterCallback=()=>{}}){
    let generation=0;const pending=new Map(),decoder=new TextDecoder('utf-8',{fatal:true});
    function load(source,url,target,method){
      const receiver=resolveTarget(target,source),net=getNet();if(!net)throw new Error('AS2 loadVariables requires the shared browser network service.');
      const request=new net.URLRequest(url);
      if(method!==undefined){if(method!=='GET'&&method!=='POST')throw new TypeError('AS2 loadVariables method must be GET or POST.');request.method=method;const variables=new net.URLVariables();for(const [key,value]of Object.entries(fieldsOf(source)))Object.defineProperty(variables,key,{value,writable:true,enumerable:true,configurable:true});request.data=variables;}
      pending.get(receiver)?.transport.close();const job={generation,transport:null};pending.set(receiver,job);
      const current=()=>job.generation===generation&&pending.get(receiver)===job&&alive(receiver);
      const finish=()=>{if(pending.get(receiver)===job)pending.delete(receiver);};
      try{job.transport=net.loadBytes(request,{
        onComplete(bytes){if(!current()){finish();return;}try{const fields=JSON.parse(wasm.as2_variables_json(decoder.decode(bytes)));for(const [name,value]of Object.entries(fields)){if(name==='__proto__')Object.defineProperty(receiver,name,{value,writable:true,enumerable:true,configurable:true});else if(!Reflect.set(receiver,name,value))throw new TypeError('Cannot assign loaded AS2 variable '+name);}const handler=receiver.onData;if(typeof handler==='function')Reflect.apply(handler,receiver,[]);afterCallback();}finally{finish();}},
        onEvent(event){if(event.type==='ioError'){try{if(current()){const handler=receiver.onData;if(typeof handler==='function')Reflect.apply(handler,receiver,[]);afterCallback();}}finally{finish();}}},
      });}catch(error){finish();throw error;}
    }
    function reset(){generation++;for(const job of pending.values())job.transport?.close();pending.clear();}
    return {load,reset,dispose:reset};
  }
  const api={create};if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Network=api;
})(globalThis);
