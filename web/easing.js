/* Flash easing facades. Mathematical behavior is implemented in shared Rust.
 * Adaptation provenance: docs/RUNTIME-EASING-PROVENANCE.md. */
(function(global){
  'use strict';
  const transitions={Back:'Back',Bounce:'Bounce',Elastic:'Elastic',None:'Linear',Regular:'Quadratic',Strong:'Quintic'};
  const motion={Back:'Back',Bounce:'Bounce',Circular:'Circular',Cubic:'Cubic',Elastic:'Elastic',Exponential:'Exponential',Linear:'Linear',Quadratic:'Quadratic',Quartic:'Quartic',Quintic:'Quintic',Sine:'Sine'};
  function create(wasm,options={}){
    if(typeof wasm?.as_easing_value!=='function')throw new Error('Rust easing bindings are missing; rebuild and re-export this project.');
    const builtins={},descriptors=new WeakMap();
    for(const [namespace,types] of [['mx.transitions.easing',transitions],['fl.transitions.easing',transitions],['fl.motion.easing',motion]]) {
      for(const [name,family] of Object.entries(types)) {
        const Type=function(){throw new TypeError(namespace+'.'+name+' exposes static easing functions; instances are unsupported.');};
        const methods=family==='Linear'?['easeNone','easeIn','easeOut','easeInOut']:['easeIn','easeOut','easeInOut'];
        for(const mode of methods) {
          const fn=function(t,b,c,d,p1=NaN,p2=NaN){
            try{return wasm.as_easing_value(family,mode,Number(t),Number(b),Number(c),Number(d),Number(p1),Number(p2));}
            catch(error){throw new RangeError(String(error));}
          };
          descriptors.set(fn,{family,mode});Object.defineProperty(Type,mode,{value:fn,enumerable:true});
        }
        Object.defineProperty(Type,'__nfIs',{value:()=>false});
        builtins[namespace+'.'+name]=Object.freeze(Type);
      }
    }

    const customStates=new WeakMap();
    function customPoints(receiver){
      const data=customStates.get(receiver);if(!data)throw new TypeError('CustomEase requires a native instance.');
      if(!Array.isArray(data.points)||data.points.length>191)throw new RangeError('CustomEase.points requires an array of at most 191 control/anchor Points.');
      const values=[];
      for(const point of data.points){if(point===null||typeof point!=='object')throw new TypeError('CustomEase.points requires Point objects.');values.push(Number(point.x),Number(point.y));}
      return values;
    }
    class CustomEase {
      constructor(xml=null){
        if(xml!==null&&xml!==undefined)throw new TypeError('CustomEase XML construction is not implemented; assign a programmatic points array.');
        customStates.set(this,{points:[],target:''});
        const fn=(t,b,c,d)=>{try{return wasm.as_custom_ease_value(new Float64Array(customPoints(this)),Number(t),Number(b),Number(c),Number(d));}catch(error){throw new RangeError(String(error));}};
        descriptors.set(fn,{family:'Custom',mode:'custom',owner:this});
        Object.defineProperty(this,'getValue',{value:fn});
      }
      get points(){return customStates.get(this).points;}set points(value){if(!Array.isArray(value))throw new TypeError('CustomEase.points requires an Array.');customStates.get(this).points=value;}
      get target(){return customStates.get(this).target;}set target(value){customStates.get(this).target=String(value);}
    }
    Object.defineProperties(CustomEase,{__nfIs:{value:value=>customStates.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native CustomEase subclass initialization is not supported; use composition.');}}});
    builtins['fl.motion.CustomEase']=CustomEase;
    const states=new WeakMap(),records=new Set();let generation=1,disposed=false;
    const events=options.events||global.NeoFlashEvents;
    const kinds={motionStart:'onMotionStarted',motionStop:'onMotionStopped',motionResume:'onMotionResumed',motionChange:'onMotionChanged',motionFinish:'onMotionFinished',motionLoop:'onMotionLooped'};
    const clock=()=>options.timers.getTimer();
    const hub=events?.create({weakTargets:true,alive:receiver=>!disposed&&states.get(receiver)?.generation===generation});
    function state(receiver){const r=states.get(receiver);if(disposed||!r||r.generation!==generation)throw new Error('Tween belongs to a previous publication; create a new instance after Restart.');return r;}
    function snapshot(receiver){return JSON.parse(state(receiver).core.snapshot());}
    function curve(fn){const value=descriptors.get(fn);if(!value)throw new TypeError('Tween requires a supported Flash easing function or CustomEase.getValue; arbitrary callbacks are not yet implemented.');return value.family==='Custom'?{family:'Custom',mode:'custom',customPoints:customPoints(value.owner)}:value;}
    function target(value){if(value===null||(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('Tween requires a target object.');return value;}
    function property(value){value=String(value);if(!value||['__proto__','constructor','prototype'].includes(value))throw new TypeError('Tween requires an ordinary target property name.');return value;}
    function syncTimer(r){if(disposed||r.generation!==generation)return;const value=JSON.parse(r.core.snapshot());r.timer.delay=1000/value.fps;if(value.isPlaying)r.timer.start();else r.timer.stop();}
    function dispatch(r,type,value){
      if(disposed||r.generation!==generation)return;
      if(r.legacy){
        const name=kinds[type],callback=r.receiver[name];if(typeof callback==='function')callback.call(r.receiver,r.receiver);
        for(const listener of [...r.listeners]){
          if(disposed||r.generation!==generation)return;
          if(r.listeners.has(listener)&&typeof listener[name]==='function')listener[name].call(listener,r.receiver);
        }
      }else hub.dispatch(r.receiver,new TweenEvent(type,value.time,value.position));
    }
    function apply(r,result){
      r.target[r.prop]=result.state.position;
      for(const type of result.events)dispatch(r,type,result.state);
      syncTimer(r);
    }
    function command(receiver,operation,args=[]){
      const r=state(receiver);let output;
      try{output=JSON.parse(r.core.command(operation,JSON.stringify(args),clock()));}
      catch(error){throw new RangeError(String(error));}
      apply(r,output);
    }
    function initialize(receiver,args,legacy){
      if(disposed)throw new Error('Tween service is disposed.');
      if(!options.timers?.Timer||!hub||typeof wasm.ScriptTween!=='function')throw new Error('Tween requires the current Rust runtime, Timer and Event services.');
      if(records.size>=2048)throw new RangeError('This publication supports at most 2048 Tween instances before Restart.');
      const [obj,prop,fn,begin,finish,duration,useSeconds=false]=args,c=curve(fn);
      const boundTarget=target(obj),boundProperty=property(prop);let core;
      try{core=new wasm.ScriptTween(JSON.stringify({begin:Number(begin),finish:Number(finish),duration:Number(duration),fps:Number(options.getFrameRate?.()??24),useSeconds:Boolean(useSeconds),...c}),clock());}
      catch(error){throw new RangeError(String(error));}
      const r={receiver,core,target:boundTarget,prop:boundProperty,fn,generation,legacy,listeners:new Set(),timer:new options.timers.Timer(1000/(options.getFrameRate?.()??24))};
      states.set(receiver,r);records.add(r);
      if(!legacy)Object.assign(receiver,hub.methods(receiver));
      r.timer.addEventListener(options.timers.TimerEvent.TIMER,()=>command(receiver,'tick'));
      const value=JSON.parse(core.snapshot());apply(r,{state:value,events:['motionChange','motionStart']});
    }
    const Base=events?.EventDispatcher||class{};
    class Tween extends Base {
      constructor(...args){super();initialize(this,args,false);}
      continueTo(finish,duration){command(this,'continueTo',duration===undefined?[Number(finish)]:[Number(finish),Number(duration)]);}
      rewind(time=0){command(this,'rewind',[Number(time)]);}
      get obj(){return state(this).target;}set obj(value){state(this).target=target(value);}
      get prop(){return state(this).prop;}set prop(value){state(this).prop=property(value);}
      get func(){return state(this).fn;}set func(value){const c=curve(value);command(this,'set:curve',[c.family,c.mode,c.customPoints||[]]);state(this).fn=value;}
      get isPlaying(){return snapshot(this).isPlaying;}
      toString(){return '[Tween]';}
    }
    for(const name of ['start','stop','resume','fforward','nextFrame','prevFrame','yoyo'])Tween.prototype[name]=function(){command(this,name);};
    for(const name of ['begin','finish','duration','time','position','FPS','looping','useSeconds']){
      const key=name==='FPS'?'fps':name;
      Object.defineProperty(Tween.prototype,name,{get(){return snapshot(this)[key];},set(value){command(this,'set:'+key,[name==='looping'||name==='useSeconds'?Boolean(value):Number(value)]);}});
    }
    class LegacyTween {
      constructor(...args){initialize(this,args,true);}
      addListener(listener){if(listener===null||typeof listener!=='object')throw new TypeError('Tween listener must be an object.');const list=state(this).listeners;if(list.size>=1024)throw new RangeError('Tween listener limit reached.');list.add(listener);}
      removeListener(listener){return state(this).listeners.delete(listener);}
    }
    for(const name of Object.getOwnPropertyNames(Tween.prototype))if(name!=='constructor')Object.defineProperty(LegacyTween.prototype,name,Object.getOwnPropertyDescriptor(Tween.prototype,name));
    class TweenEvent extends (events?.Event||class{}) {
      constructor(type,time,position,bubbles=false,cancelable=false){super(type,bubbles,cancelable);this.time=Number(time);this.position=Number(position);}
      clone(){return new TweenEvent(this.type,this.time,this.position,this.bubbles,this.cancelable);}
      toString(){return '[TweenEvent type="'+this.type+'" time='+this.time+' position='+this.position+']';}
    }
    for(const [constant,type] of Object.entries({MOTION_START:'motionStart',MOTION_STOP:'motionStop',MOTION_RESUME:'motionResume',MOTION_CHANGE:'motionChange',MOTION_FINISH:'motionFinish',MOTION_LOOP:'motionLoop'}))Object.defineProperty(TweenEvent,constant,{value:type});
    for(const Type of [Tween,LegacyTween]){
      Object.defineProperty(Type,'__nfIs',{value:value=>states.has(value)&&(states.get(value).legacy===(Type===LegacyTween))});
      Object.defineProperty(Type,'__nfInitialize',{value:()=>{throw new TypeError('Native Tween subclass initialization is not supported; use composition.');}});
    }
    Object.defineProperty(TweenEvent,'__nfIs',{value:value=>value instanceof TweenEvent});
    builtins['fl.transitions.Tween']=Tween;builtins['mx.transitions.Tween']=LegacyTween;builtins['fl.transitions.TweenEvent']=TweenEvent;
    function reset(){if(disposed)return;for(const r of records){r.timer.stop();r.core.free();}records.clear();generation++;hub?.clear();}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({builtins:Object.freeze(builtins),describe:fn=>descriptors.get(fn)||null,reset,dispose});
  }
  const api=Object.freeze({create});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashEasing=api;
})(globalThis);
