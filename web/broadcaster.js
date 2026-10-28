/* AS2 broadcaster facade: Rust owns ordered listener identities. The host only
 * binds object identity, callback arguments and dynamic method references.
 * Pinned MIT provenance: docs/RUNTIME-EASING-PROVENANCE.md. */
(function(global){
  'use strict';
  function create(wasm){
    if(typeof wasm?.ScriptBroadcaster!=='function')throw new Error('AsBroadcaster requires the Rust ScriptBroadcaster module; rebuild this HTML5 export.');
    const states=new WeakMap(),records=new Set();let generation=1,disposed=false;
    function state(receiver){const record=states.get(receiver);if(disposed||!record||record.generation!==generation)throw new Error('AsBroadcaster requires a target initialized in the current publication.');return record;}
    function object(value){if(value===null||(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('AsBroadcaster currently requires object/function listeners and targets.');return value;}
    function addListener(listener){
      object(listener);const record=state(this),known=record.identities.get(listener)||0;let id;
      try{id=record.core.add(known);}catch(error){throw new RangeError(String(error));}
      record.identities.set(listener,id);record.references.set(id,listener);return true;
    }
    function removeListener(listener){
      const record=state(this);if(listener===null||(typeof listener!=='object'&&typeof listener!=='function'))return false;
      const id=record.identities.get(listener)||0,removed=record.core.remove(id);
      if(removed)record.references.delete(id);return removed;
    }
    function broadcastMessage(name,...args){
      const record=state(this);if(name===undefined)return undefined;
      if(typeof name!=='string')throw new TypeError('AsBroadcaster event names currently require strings.');
      if(args.length>1024)throw new RangeError('AsBroadcaster supports at most 1024 event arguments.');
      const length=record.core.len();
      for(let index=0;index<length;index++){
        if(disposed||record.generation!==generation||states.get(this)!==record)return true;
        const listener=record.references.get(record.core.at(index));if(!listener)continue;
        const callback=name===''&&typeof listener==='function'?listener:listener[name];
        if(typeof callback==='function')Reflect.apply(callback,listener,args);
      }
      return true;
    }
    function initialize(target){
      object(target);if(disposed)throw new Error('AsBroadcaster service is disposed.');
      const previous=states.get(target);
      if((!previous||previous.generation!==generation)&&records.size>=1024)throw new RangeError('This publication supports at most 1024 initialized broadcasters.');
      const record={core:new wasm.ScriptBroadcaster(1024),references:new Map(),identities:new WeakMap(),generation};
      try{
        Object.defineProperties(target,{
          addListener:{value:addListener,writable:true,configurable:true},
          removeListener:{value:removeListener,writable:true,configurable:true},
          broadcastMessage:{value:broadcastMessage,writable:true,configurable:true},
          _listeners:{configurable:true,get(){const current=state(this);return Object.freeze(Array.from(current.core.listeners(),id=>current.references.get(id)));},set(){throw new TypeError('Direct _listeners replacement is unsupported; use addListener/removeListener.');}}
        });
      }catch(error){record.core.free();throw error;}
      if(previous&&previous.generation===generation){previous.core.free();previous.references.clear();records.delete(previous);}
      states.set(target,record);records.add(record);
    }
    function AsBroadcaster(){throw new TypeError('Use AsBroadcaster.initialize(target); construction is outside the current profile.');}
    Object.defineProperties(AsBroadcaster,{initialize:{value:initialize},addListener:{value:addListener},removeListener:{value:removeListener},broadcastMessage:{value:broadcastMessage},__nfIs:{value:()=>false}});
    function reset(){if(disposed)return;for(const record of records){record.core.free();record.references.clear();}records.clear();generation++;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({AsBroadcaster,reset,dispose});
  }
  const api=Object.freeze({create});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashBroadcaster=api;
})(globalThis);
