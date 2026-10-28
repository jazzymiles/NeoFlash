/* AS2-only browser facilities and reference bridge. Capability policy, query
 * encoding, locale normalization and toggle state are Rust. Ordered event
 * listener identities reuse ScriptBroadcaster. No AS3 classes are replaced.
 * Pinned references and partial limits: docs/AS2-PLATFORM-BATCH.md. */
(function(global){
  'use strict';
  function create(wasm, options = {}) {
    if(typeof wasm?.ScriptAS2Platform !== 'function' || typeof wasm?.ScriptBroadcaster !== 'function') {
      throw new Error('AS2 platform requires the Rust platform and broadcaster modules; rebuild this HTML5 export.');
    }
    const host = options.global || global, document = options.document || host.document;
    const navigator = host.navigator || {}, screen = host.screen || {};
    const audio = document?.createElement?.('audio'), video = document?.createElement?.('video');
    const facts = {
      language:String(navigator.language || ''), platform:String(navigator.userAgentData?.platform || navigator.platform || 'unknown'),
      protocol:String(host.location?.protocol || ''), screenWidth:Number(screen.width || 0), screenHeight:Number(screen.height || 0),
      colorDepth:Number(screen.colorDepth || 0), audio:typeof host.Audio === 'function',
      mp3:!!audio?.canPlayType?.('audio/mpeg'), video:typeof video?.canPlayType === 'function',
      composition:typeof host.CompositionEvent === 'function', printing:typeof host.print === 'function'
    };
    const core = new wasm.ScriptAS2Platform(JSON.stringify(facts));
    const capabilities = Object.freeze(JSON.parse(core.capabilities_json()));
    const records = new Set(), targets = new WeakMap();
    let generation = 1, disposed = false;
    function active(){if(disposed)throw new Error('The AS2 platform publication has been disposed.');}
    function object(value){if(value===null||(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('AS2 event targets/listeners require objects or functions.');return value;}
    function name(value){if(typeof value!=='string'||value.length===0||value.length>256)throw new TypeError('AS2 event names require 1–256 characters.');return value;}
    function record(target){active();const value=targets.get(target);if(!value||value.generation!==generation)throw new Error('Initialize this AS2 EventDispatcher target in the current publication.');return value;}
    function eventState(target,type,allocate){
      const current=record(target);let state=current.events.get(type);
      if(!state&&allocate){
        if(current.events.size>=128)throw new RangeError('AS2 EventDispatcher supports at most 128 event types per target.');
        state={core:new wasm.ScriptBroadcaster(1024),identities:new WeakMap(),references:new Map()};current.events.set(type,state);
      }
      return state;
    }
    function addEventListener(type,listener){
      name(type);object(listener);const state=eventState(this,type,true),id=state.core.add(state.identities.get(listener)||0);
      state.identities.set(listener,id);state.references.set(id,listener);
    }
    function removeEventListener(type,listener){
      name(type);const state=eventState(this,type,false);if(!state||listener===null||(typeof listener!=='object'&&typeof listener!=='function'))return;
      const id=state.identities.get(listener)||0;if(state.core.remove(id))state.references.delete(id);
    }
    function dispatchEvent(event){
      object(event);const type=name(event.type),current=record(this),state=eventState(this,type,false);
      if(event.target===undefined)event.target=this;
      if(!state)return;
      const length=state.core.len();
      for(let index=0;index<length;index++){
        if(disposed||current.generation!==generation||targets.get(this)!==current)return;
        const listener=state.references.get(state.core.at(index));if(!listener)continue;
        const callback=typeof listener==='function'?listener:listener[type];
        if(typeof callback==='function')Reflect.apply(callback,typeof listener==='function'?this:listener,[event]);
      }
    }
    function freeRecord(value){for(const state of value.events.values()){state.core.free();state.references.clear();}value.events.clear();records.delete(value);}
    function initialize(target){
      active();object(target);const previous=targets.get(target);
      if((!previous||previous.generation!==generation)&&records.size>=1024)throw new RangeError('This publication supports at most 1024 AS2 EventDispatcher targets.');
      Object.defineProperties(target,{
        addEventListener:{value:addEventListener,writable:true,configurable:true},
        removeEventListener:{value:removeEventListener,writable:true,configurable:true},
        dispatchEvent:{value:dispatchEvent,writable:true,configurable:true}
      });
      if(previous&&previous.generation===generation)freeRecord(previous);
      const value={generation,events:new Map()};targets.set(target,value);records.add(value);
    }
    function EventDispatcher(){throw new TypeError('Use mx.events.EventDispatcher.initialize(target) in this AS2 profile.');}
    Object.defineProperties(EventDispatcher,{initialize:{value:initialize},__nfIs:{value:()=>false}});
    function Delegate(){throw new TypeError('Use mx.utils.Delegate.create(receiver, method).');}
    Object.defineProperties(Delegate,{
      create:{value(receiver,method){
        active();object(receiver);if(typeof method!=='function')throw new TypeError('Delegate.create requires a function method.');
        const created=generation;
        return function(...args){active();if(created!==generation)throw new Error('This AS2 Delegate belongs to a restarted publication.');return Reflect.apply(method,receiver,args);};
      }},__nfIs:{value:()=>false}
    });
    const imeListeners=new wasm.ScriptBroadcaster(1024),imeReferences=new Map();
    let imeIdentities=new WeakMap();
    function unsupported(api,reason){throw new Error(`${api} is unavailable in the HTML5 publication: ${reason}`);}
    const IME={
      ALPHANUMERIC_FULL:'ALPHANUMERIC_FULL',ALPHANUMERIC_HALF:'ALPHANUMERIC_HALF',CHINESE:'CHINESE',
      JAPANESE_HIRAGANA:'JAPANESE_HIRAGANA',JAPANESE_KATAKANA_FULL:'JAPANESE_KATAKANA_FULL',
      JAPANESE_KATAKANA_HALF:'JAPANESE_KATAKANA_HALF',KOREAN:'KOREAN',UNKNOWN:'UNKNOWN',
      addListener(listener){active();object(listener);const id=imeListeners.add(imeIdentities.get(listener)||0);imeIdentities.set(listener,id);imeReferences.set(id,listener);},
      removeListener(listener){active();if(listener===null||(typeof listener!=='object'&&typeof listener!=='function'))return false;const id=imeIdentities.get(listener)||0,removed=imeListeners.remove(id);if(removed)imeReferences.delete(id);return removed;},
      doConversion(){unsupported('System.IME.doConversion','the browser does not expose OS conversion control.');},
      getConversionMode(){unsupported('System.IME.getConversionMode','the browser does not expose the OS conversion mode.');},
      getEnabled(){unsupported('System.IME.getEnabled','the browser does not expose whether the OS IME is enabled.');},
      setCompositionString(){unsupported('System.IME.setCompositionString','browser composition text cannot be replaced programmatically.');},
      setConversionMode(){unsupported('System.IME.setConversionMode','the browser does not expose OS conversion control.');},
      setEnabled(){unsupported('System.IME.setEnabled','the browser does not expose OS IME enable/disable control.');}
    };
    function composition(event){
      if(disposed)return;const current=generation,length=imeListeners.len();
      try{
        for(let index=0;index<length;index++){
          if(disposed||current!==generation)return;
          const listener=imeReferences.get(imeListeners.at(index));
          if(listener&&typeof listener.onIMEComposition==='function')Reflect.apply(listener.onIMEComposition,listener,[String(event.data||'')]);
        }
      }catch(error){if(options.onError)options.onError(error);else throw error;}
    }
    const security=Object.freeze({
      get sandboxType(){active();return core.sandbox_type();},
      allowDomain(){unsupported('System.security.allowDomain','browser same-origin/CORS policy cannot be relaxed by ActionScript.');},
      allowInsecureDomain(){unsupported('System.security.allowInsecureDomain','browser mixed-content policy cannot be relaxed by ActionScript.');},
      loadPolicyFile(){unsupported('System.security.loadPolicyFile','Flash crossdomain.xml files do not grant browser CORS permissions.');}
    });
    const System=Object.freeze({
      capabilities,security,IME:Object.freeze(IME),
      get exactSettings(){return true;},set exactSettings(value){if(value!==true)unsupported('System.exactSettings','storage is isolated by the exact browser origin.');},
      get useCodepage(){return false;},set useCodepage(value){if(value!==false)unsupported('System.useCodepage','text uses Unicode; the browser OS codepage is unavailable.');},
      setClipboard(){unsupported('System.setClipboard','synchronous clipboard writes require an explicit browser permission adapter.');},
      showSettings(){unsupported('System.showSettings','use browser site permissions; Flash Player settings panels do not exist.');}
    });
    function observeKeyEvent(event){active();if(typeof event?.getModifierState==='function'){core.key_toggle(20,!!event.getModifierState('CapsLock'));core.key_toggle(144,!!event.getModifierState('NumLock'));}}
    function reset(){active();for(const value of [...records])freeRecord(value);imeListeners.reset();imeReferences.clear();imeIdentities=new WeakMap();generation++;core.reset();}
    function dispose(){if(disposed)return;reset();disposed=true;document?.removeEventListener?.('compositionend',composition);imeListeners.free();core.free();}
    document?.addEventListener?.('compositionend',composition);
    function getVersion(){active();return capabilities.version;}
    function updateAfterEvent(){active();if(typeof options.requestRender!=='function')throw new Error('AS2 updateAfterEvent requires the publication render-request hook.');options.requestRender();}
    return Object.freeze({System,Delegate,EventDispatcher,getVersion,updateAfterEvent,observeKeyEvent,isToggled(code){active();return core.is_toggled(Number(code)>>>0);},reset,dispose});
  }
  const api=Object.freeze({create});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Platform=api;
})(globalThis);
