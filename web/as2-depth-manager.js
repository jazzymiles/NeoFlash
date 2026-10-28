/* AS2 identity adapter. Depth selection and sibling shuffling live in Rust. */
(function(global){
'use strict';
function create({player,referenceOf,display,getRoot,afterEvent}={}){
 let generation=1,serial=0,disposed=false,cache=new WeakMap();
 const methodNames=Object.freeze(['createChildAtDepth','createClassChildAtDepth','setDepthAbove','setDepthBelow','setDepthTo']);
 function target(value){if(disposed)throw new Error('DepthManager has been disposed.');const ref=referenceOf(value);if(!ref||!display.alive(value)||display.descriptor(value).owner!=='movie_clip')throw new TypeError('DepthManager requires a live MovieClip from this publication.');return ref;}
 function command(value,request){const ref=target(value);try{return JSON.parse(player.as2_depth_manager(ref.key,JSON.stringify(request)));}catch(error){throw error instanceof Error?error:new Error(String(error));}}
 function unsupported(name){throw new Error(`DepthManager.${name} requires Flash UIObject or reserved system-layer linkage, outside the current relative MovieClip profile.`);}
 class DepthManager{constructor(){unsupported('constructor');}}
 Object.assign(DepthManager,command(getRoot(),{op:'definitions'}));
 for(const name of ['createClassObjectAtDepth','createObjectAtDepth'])Object.defineProperty(DepthManager,name,{value(){unsupported(name);}});
 Object.defineProperties(DepthManager,{__nfIs:{value:()=>false},__nfInitialize:{value:()=>unsupported('native subclassing')}});
 function methods(value){
  target(value);if(cache.has(value))return cache.get(value);
  const epoch=generation;
  function active(){if(epoch!==generation)throw new Error('DepthManager method belongs to a previous publication session.');target(value);}
  const result={
   createChildAtDepth(identifier,flag,initObject){active();if(typeof identifier!=='string'||!identifier)throw new TypeError('createChildAtDepth requires an existing NeoFlash symbol Export identifier.');const depth=command(value,{op:'plan',flag}).depth;const child=display.methods(value).attachMovie(identifier,`depthChild${++serial}`,depth,initObject);afterEvent?.();return child;},
   createClassChildAtDepth(){active();unsupported('createClassChildAtDepth');},
   setDepthAbove(other){active();command(value,{op:'above',other:target(other).objectRef});afterEvent?.();},
   setDepthBelow(other){active();command(value,{op:'below',other:target(other).objectRef});afterEvent?.();},
   setDepthTo(flag){active();command(value,{op:'set',flag});afterEvent?.();},
  };
  cache.set(value,result);return result;
 }
 function reset(){generation++;serial=0;cache=new WeakMap();}
 function dispose(){reset();disposed=true;}
 return{DepthManager,methods,methodNames,reset,dispose};
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2DepthManager=api;
})(globalThis);
