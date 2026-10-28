/* AS2-only display transition bridge. Rust owns progress/easing/transform math. */
(function(global){'use strict';
function create(options={}){
 const{wasm,timers,easing,EventDispatcher,stage}=options;
 if(!wasm?.ScriptAS2Transition||!timers?.Timer||!easing?.describe||!EventDispatcher?.initialize)throw new Error('AS2 transitions require Rust transition/easing, Timer and EventDispatcher.');
 const managers=new WeakMap(),byContent=new WeakMap(),effects=new WeakMap(),types=new WeakMap(),records=new Set();let generation=1,disposed=false;
 const properties=['_x','_y','_xscale','_yscale','_alpha','_rotation'];
 function state(target){const r=managers.get(target);if(disposed||!r||r.generation!==generation)throw new Error('TransitionManager belongs to a restarted publication.');return r;}
 function active(r){return !disposed&&r.generation===generation;}
 function content(value){if(!options.isMovieClip?.(value))throw new TypeError('TransitionManager requires a live MovieClip.');return value;}
 function bounds(rect){return{x:rect.x,y:rect.y,width:rect.width,height:rect.height};}
 function appearance(target){const result={};for(const name of properties)result[name]=target[name];result._width=target._width;result._height=target._height;const inner=target.getBounds(target),outer=target.getBounds(target.parent);result._innerBounds={xMin:inner.x,yMin:inner.y,xMax:inner.x+inner.width,yMax:inner.y+inner.height};result._outerBounds={xMin:outer.x,yMin:outer.y,xMax:outer.x+outer.width,yMax:outer.y+outer.height};result.colorTransform=target.transform.colorTransform;return result;}
 function snapshot(r){const result={...r.base,_innerBounds:{...r.base._innerBounds},_outerBounds:{...r.base._outerBounds}};const c=r.base.colorTransform;result.colorTransform=new c.constructor(c.redMultiplier,c.greenMultiplier,c.blueMultiplier,c.alphaMultiplier,c.redOffset,c.greenOffset,c.blueOffset,c.alphaOffset);return result;}
 function release(r,entry){r.jobs.delete(entry.kind);entry.timer.stop();entry.core.free();entry.finished=true;}
 function patch(target,values){for(const[name,value]of Object.entries(values))target[name]=value;}
 function tick(r,entry,notify=false){if(!active(r)||entry.finished)return;try{
  const value=JSON.parse(entry.core.sample(timers.getTimer()));patch(r.content,value.patch);
  if(value.done){release(r,entry);if(![...r.jobs.values()].some(v=>v.direction===entry.direction)){
    if(entry.direction===Transition.OUT&&r.jobs.size===0){r.content._visible=false;const saved={};for(const name of properties)saved[name]=r.base[name];patch(r.content,saved);}
    r.manager.dispatchEvent({type:entry.direction===Transition.IN?'allTransitionsInDone':'allTransitionsOutDone',target:r.manager});
  }}if(notify)options.afterEvent?.();
 }catch(error){if(!entry.finished)release(r,entry);if(options.onError)options.onError(error);else throw error;}}
 class Transition{constructor(){throw new TypeError('Create transitions with TransitionManager.start/startTransition.');}}
 Object.assign(Transition,{IN:1,OUT:2});
 const builtins={'mx.transitions.Transition':Transition};
 for(const name of ['Fade','Fly','Rotate','Zoom','Squeeze']){const Type=class extends Transition{};types.set(Type,name);Object.defineProperty(Type,'__nfIs',{value:value=>effects.get(value)?.kind===name});builtins['mx.transitions.'+name]=Type;}
 class TransitionManager{
  constructor(target){if(disposed)throw new Error('Transition service disposed.');target=content(target);if(records.size>=256)throw new RangeError('At most 256 TransitionManagers per publication.');if(byContent.has(target))throw new Error('This MovieClip already has a TransitionManager; use TransitionManager.start or its existing manager.');const r={manager:this,content:target,base:appearance(target),jobs:new Map(),generation};managers.set(this,r);byContent.set(target,this);records.add(r);EventDispatcher.initialize(this);}
  static start(target,params){target=content(target);let manager=byContent.get(target);if(!manager||managers.get(manager)?.generation!==generation)manager=new TransitionManager(target);return manager.startTransition(params);}
  get content(){return state(this).content;}
  set content(value){const r=state(this);value=content(value);if(value===r.content)return;if(r.jobs.size)throw new Error('Cannot replace transition content while effects are active.');const other=byContent.get(value);if(other&&managers.get(other)?.generation===generation)throw new Error('Target already has a TransitionManager.');byContent.delete(r.content);r.content=value;r.base=appearance(value);byContent.set(value,this);}
  get contentAppearance(){return snapshot(state(this));}
  toString(){state(this);return '[TransitionManager]';}
  startTransition(params){
   const r=state(this);content(r.content);if(!params||typeof params!=='object')throw new TypeError('Transition parameters require an object.');const kind=types.get(params.type);if(!kind)throw new TypeError('Use Fade, Fly, Rotate, Zoom or Squeeze; mask/Photo transitions are not implemented.');
   if(r.jobs.size===0)r.base=appearance(r.content);
   const curve=easing.describe(params.easing);if(!curve||curve.family==='Custom')throw new TypeError('Transition easing requires a built-in Flash easing function.');
   const parent=r.content.parent;const corners=[{x:0,y:0},{x:stage.stageWidth,y:0},{x:0,y:stage.stageHeight},{x:stage.stageWidth,y:stage.stageHeight}].map(p=>parent&&parent!==stage?parent.globalToLocal(p):p);
   const config={kind,direction:Number(params.direction),duration:Number(params.duration),family:curve.family,mode:curve.mode,base:r.base,outer:bounds(r.content.getBounds(parent)),viewport:corners};
   for(const name of ['startPoint','dimension','degrees','ccw'])if(params[name]!==undefined)config[name]=params[name];
   const core=new wasm.ScriptAS2Transition(JSON.stringify(config),timers.getTimer());const channels=kind==='Fade'?['_alpha']:kind==='Fly'?['_x','_y']:kind==='Rotate'?['_rotation']:kind==='Squeeze'?[params.dimension===1?'_yscale':'_xscale']:['_xscale','_yscale'];
   for(const old of r.jobs.values())if(old.kind!==kind&&old.channels.some(p=>channels.includes(p))){core.free();throw new Error('Concurrent transitions cannot write the same display property; finish or replace that effect first.');}
   const previous=r.jobs.get(kind);if(previous)release(r,previous);const transition=Object.create(params.type.prototype),timer=new timers.Timer(1000/(options.getFrameRate?.()||24));
   const entry={core,timer,kind,direction:config.direction,channels,transition,finished:false};effects.set(transition,entry);r.jobs.set(kind,entry);r.content._visible=true;timer.addEventListener(timers.TimerEvent.TIMER,()=>tick(r,entry));tick(r,entry,true);if(!entry.finished)timer.start();return transition;
  }
 }
 Object.defineProperties(Transition,{__nfIs:{value:value=>effects.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Custom Transition subclasses are unsupported.');}}});
 Object.defineProperties(TransitionManager,{__nfIs:{value:value=>managers.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native TransitionManager subclassing is unsupported.');}}});builtins['mx.transitions.TransitionManager']=TransitionManager;
 function reset(){if(disposed)return;for(const r of records){for(const e of [...r.jobs.values()])release(r,e);byContent.delete(r.content);}records.clear();generation++;}
 function dispose(){if(disposed)return;reset();disposed=true;}
 return{builtins,reset,dispose};
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Transitions=api;
})(globalThis);
