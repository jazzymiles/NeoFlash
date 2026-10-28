/* AS2 Locale browser transport. Translation tables, language choice and XLIFF
 * parsing are Rust-owned. No editor Strings panel or automatic field replacement. */
(function(global){
  'use strict';
  function create(wasm,options={}){
    if(typeof wasm?.ScriptAS2Locale!=='function'||typeof options.URLLoader!=='function'||typeof options.URLRequest!=='function')throw new Error('AS2 Locale requires Rust Locale and the shared URLLoader.');
    let core=new wasm.ScriptAS2Locale(),callback=null,active=null,generation=1,disposed=false;
    function request(value){if(disposed)throw new Error('This Locale service has been disposed.');try{return JSON.parse(core.execute(JSON.stringify(value)));}catch(error){throw new Error(String(error));}}
    function text(value,name){if(typeof value!=='string')throw new TypeError(`Locale.${name} requires a string.`);return value;}
    function unsupported(name){throw new Error(`Locale.${name} requires automatic TextField replacement, which is unavailable in this nonvisual Locale profile.`);}
    function stop(){const old=active;active=null;if(old?.loader){try{old.loader.close();}catch(_){}for(const [type,listener]of old.listeners)try{old.loader.removeEventListener(type,listener);}catch(_){}}}
    function failLater(operation){queueMicrotask(()=>finish(operation,false));}
    function finish(operation,success){
      if(disposed||operation.generation!==generation||active!==operation)return;
      let detail=null;
      if(success){try{const result=request({op:'complete',token:operation.token,xml:operation.loader.data});success=result.accepted&&result.success;}catch(error){success=false;detail=error;}}
      if(!success)request({op:'failed',token:operation.token});
      const handler=operation.callback;
      stop();
      if(detail)options.onDiagnostic?.(String(detail));
      if(handler)Reflect.apply(handler,Locale,[Boolean(success)]);
    }
    function load(language,custom){
      if(custom!==undefined&&custom!==null&&typeof custom!=='function')throw new TypeError('Locale.loadLanguageXML callback requires a function.');
      const plan=request({op:'begin',language:text(language,'loadLanguageXML')});stop();
      const operation={generation,token:plan.token,callback:custom||callback,loader:null,listeners:[]};active=operation;
      if(!plan.path){failLater(operation);return;}
      const loader=new options.URLLoader();operation.loader=loader;
      const complete=()=>finish(operation,true),failed=()=>finish(operation,false);
      operation.listeners=[['complete',complete],['ioError',failed],['securityError',failed]];
      for(const [type,listener]of operation.listeners)loader.addEventListener(type,listener);
      try{loader.load(new options.URLRequest(plan.path));}catch(error){operation.error=String(error);failLater(operation);}
    }
    class Locale {constructor(){throw new TypeError('mx.lang.Locale exposes static methods; it cannot be constructed.');}}
    Object.assign(Locale,{
      addXMLPath(language,path){request({op:'path',language:text(language,'addXMLPath'),path:text(path,'addXMLPath')});},
      checkXMLStatus(){return request({op:'state'}).loaded;},
      getDefaultLang(){return request({op:'state'}).default;},
      initialize(){const language=typeof options.language==='function'?options.language():options.language;load(request({op:'select',language:language||''}));},
      loadLanguageXML(language,custom){load(language,custom);},
      loadString(id){const result=request({op:'get',id:text(id,'loadString')});return result===null?undefined:result;},
      loadStringEx(id,language){const result=request({op:'get',id:text(id,'loadStringEx'),language:text(language,'loadStringEx')});return result===null?undefined:result;},
      setDefaultLang(language){request({op:'default',language:text(language,'setDefaultLang')});},
      setLoadCallback(next){if(next!==null&&typeof next!=='function')throw new TypeError('Locale.setLoadCallback requires a function or null.');if(disposed)throw new Error('Locale disposed.');callback=next;},
      setString(id,language,value){request({op:'set',id:text(id,'setString'),language:text(language,'setString'),value:text(value,'setString')});},
      addDelayedInstance(){unsupported('addDelayedInstance');},
    });
    Object.defineProperties(Locale,{
      languageCodeArray:{get(){return request({op:'state'}).languages;}},stringIDArray:{get(){return request({op:'state'}).ids;}},
      autoReplace:{get(){return false;},set(value){if(value)unsupported('autoReplace=true');}},
      __nfIs:{value:()=>false},__nfInitialize:{value:()=>{throw new TypeError('mx.lang.Locale native subclassing is unavailable.');}},
    });
    function reset(){if(disposed)return;stop();generation++;callback=null;core.free?.();core=new wasm.ScriptAS2Locale();}
    function dispose(){if(disposed)return;stop();generation++;callback=null;core.free?.();disposed=true;}
    return{builtins:{'mx.lang.Locale':Locale},reset,dispose};
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Locale=api;
})(typeof window==='undefined'?globalThis:window);
