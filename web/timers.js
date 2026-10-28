/* Session-owned AS3 Timer family. Original implementation; primary contracts:
 * https://airsdk.dev/reference/actionscript/3.0/flash/utils/Timer.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/events/TimerEvent.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/utils/package.html#getTimer()
 * TIMER-before-TIMER_COMPLETE ordering was cross-checked with Ruffle Timer.as,
 * revision fe6cfc545405b888ebb478ce1c211d63147ca49b. No upstream code is copied.
 *
 * create({wasm,events,now,schedule,cancel,onError,afterEvent,requestRender,maxRunning})
 * returns Timer/TimerEvent/getTimer, callback timer functions, legacy and lifecycle.
 * Callback IDs/liveness are owned by Rust ScriptTimerIds; callbacks/targets stay
 * at this browser boundary. Both dialects reuse the same Timer scheduler.
 * now() is monotonic milliseconds. schedule(callback,ms) must be asynchronous
 * and one-shot; cancel(handle) cancels it. Defaults use browser clock/timeouts.
 * afterEvent drains host script/render work; requestRender queues a render request
 * for TimerEvent.updateAfterEvent. Both hooks are optional until used.
 *
 * Bounded profile: finite numeric delays 0..2147483647, scheduled at >=1ms;
 * integer repeat counts 0..2147483647, <=1024 running timers by default. A delayed
 * wake emits one tick, with no historical Flash frame-rate/catch-up guarantee.
 * Browser background throttling applies. Full AS coercion/errorID fidelity,
 * count overflow or timer-GC conformance is not claimed.
 * Publication reset invalidates old objects and rebases getTimer; per-Timer reset
 * only stops/zeros its own count. Completed finite timers need reset to run again.
 */
(function (global) {
  'use strict';
  const MAX_INT=2147483647;
  function create(options={}) {
    const events=options.events||global.NeoFlashEvents;
    if(!events?.Event||!events?.EventDispatcher||typeof events.create!=='function')throw new TypeError('Timer requires the NeoFlash Event facade.');
    const now=options.now||(()=>global.performance?.now?.()??Date.now());
    const schedule=options.schedule||((callback,delay)=>global.setTimeout(callback,delay));
    const cancel=options.cancel||(handle=>global.clearTimeout(handle));
    const maxRunning=options.maxRunning??1024;
    if(!Number.isInteger(maxRunning)||maxRunning<1||maxRunning>10000)throw new RangeError('Timer running limit must be an integer from 1 through 10000.');
    const states=new WeakMap(),eventInstances=new WeakSet(),running=new Set(),callbacks=new Map();
    let callbackIds=null;
    let generation=1,disposed=false;
    function clock(){const value=now();if(typeof value!=='number'||!Number.isFinite(value))throw new RangeError('Timer clock must return finite milliseconds.');return value;}
    let origin=clock();
    function open(){if(disposed)throw new Error('This Timer service has been disposed.');}
    function state(receiver){
      open();const record=states.get(receiver);
      if(!record)throw new TypeError('This operation requires a Timer from the current player.');
      if(record.generation!==generation)throw new Error('This Timer belongs to a previous publication session. Create a new Timer after Restart.');
      return record;
    }
    const hub=events.create({weakTargets:true,alive:target=>!disposed&&states.get(target)?.generation===generation});
    function delayValue(value){
      if(typeof value!=='number'||!Number.isFinite(value)||value<0||value>MAX_INT)throw new RangeError('Timer delay currently requires a finite number from 0 through 2147483647 milliseconds.');
      return value;
    }
    function repeatValue(value){
      if(!Number.isInteger(value)||value<0||value>MAX_INT)throw new RangeError('Timer repeatCount currently requires an integer from 0 through 2147483647.');
      return value;
    }
    function cancelPending(record){
      const pending=record.pending;record.pending=null;
      if(pending)cancel(pending.handle);
    }
    function stop(record){cancelPending(record);record.running=false;running.delete(record);}
    function report(error){if(options.onError)options.onError(error);else throw error;}
    function arm(record){
      const pending={handle:null};record.pending=pending;
      try {pending.handle=schedule(()=>fire(record,pending),Math.max(1,record.delay));}
      catch(error){stop(record);throw error;}
    }
    function fire(record,pending){
      if(disposed||record.generation!==generation||!record.running||record.pending!==pending)return;
      record.pending=null;
      try {
        if(record.count===MAX_INT)throw new RangeError('Timer count overflow is outside the current supported profile; reset this Timer.');
        record.count++;
        hub.dispatch(record.receiver,new TimerEvent(TimerEvent.TIMER));
        // A publication reset/dispose inside a listener must cancel the old turn.
        if(disposed||record.generation!==generation)return;
        if(record.repeat!==0&&record.count>=record.repeat){
          stop(record);
          hub.dispatch(record.receiver,new TimerEvent(TimerEvent.TIMER_COMPLETE));
        }
        if(disposed||record.generation!==generation)return;
        options.afterEvent?.();
        // A listener can stop/reset/change delay; never schedule a second job.
        if(record.running&&!record.pending)arm(record);
      } catch(error){stop(record);report(error);}
    }
    function initialize(receiver,args){
      open();
      if(args.length<1||args.length>2)throw new TypeError('Timer expects a delay and optional repeatCount.');
      if(states.has(receiver))throw new Error('Timer base initialization may only run once per instance.');
      const delay=delayValue(args[0]),repeat=repeatValue(args.length>1?args[1]:0);
      states.set(receiver,{receiver,delay,repeat,count:0,running:false,pending:null,generation});
      Object.assign(receiver,hub.methods(receiver));
    }
    class Timer extends events.EventDispatcher {
      constructor(delay,repeatCount=0){super();initialize(this,[delay,repeatCount]);}
      get delay(){return state(this).delay;}
      set delay(value){const next=delayValue(value),record=state(this);record.delay=next;if(record.running){cancelPending(record);arm(record);}}
      get repeatCount(){return state(this).repeat;}
      set repeatCount(value){const next=repeatValue(value),record=state(this);record.repeat=next;if(next!==0&&next<=record.count)stop(record);}
      get currentCount(){return state(this).count;}
      get running(){return state(this).running;}
      start(){
        const record=state(this);
        if(record.running||(record.repeat!==0&&record.count>=record.repeat))return;
        if(running.size>=maxRunning)throw new RangeError(`This player supports at most ${maxRunning} concurrently running Timers.`);
        record.running=true;running.add(record);arm(record);
      }
      stop(){stop(state(this));}
      reset(){const record=state(this);stop(record);record.count=0;}
    }
    Timer.__nfIs=value=>states.has(value);
    Timer.__nfInitialize=(receiver,args=[])=>initialize(receiver,args);
    class TimerEvent extends events.Event {
      constructor(type,bubbles=false,cancelable=false){super(type,bubbles,cancelable);open();eventInstances.add(this);}
      clone(){return new TimerEvent(this.type,this.bubbles,this.cancelable);}
      updateAfterEvent(){
        open();if(typeof options.requestRender!=='function')throw new Error('TimerEvent.updateAfterEvent requires the host render-request hook.');
        options.requestRender();
      }
      toString(){return `[TimerEvent type="${this.type}" bubbles=${this.bubbles} cancelable=${this.cancelable} eventPhase=${this.eventPhase}]`;}
    }
    Object.defineProperties(TimerEvent,{TIMER:{value:'timer'},TIMER_COMPLETE:{value:'timerComplete'},
      __nfIs:{value:value=>eventInstances.has(value)},
      __nfInitialize:{value:(receiver,args=[])=>{open();events.Event.__nfInitialize(receiver,args);eventInstances.add(receiver);}}});
    function ids(){
      open();
      if(!callbackIds){
        if(typeof options.wasm?.ScriptTimerIds!=='function')throw new Error('setInterval/setTimeout require the Rust ScriptTimerIds module. Rebuild this HTML5 export.');
        callbackIds=new options.wasm.ScriptTimerIds(maxRunning);
      }
      return callbackIds;
    }
    function clearCallback(id){
      if(typeof id!=='number')throw new TypeError('clearInterval/clearTimeout require a numeric timer ID.');
      const registry=ids();
      if(!registry.clear(id))return;
      const binding=callbacks.get(id);callbacks.delete(id);
      if(binding)binding.timer.stop();
    }
    function createCallback(repeating,legacy,args){
      open();
      let receiver,method,callback,delay,parameters;
      if(legacy&&args[0]!==null&&typeof args[0]==='object'&&typeof args[1]==='string'){
        if(args.length<3)throw new TypeError('AS2 object timer expects object, methodName, delay and optional callback arguments.');
        [receiver,method,delay]=args;parameters=args.slice(3);
      }else{
        if(args.length<2||typeof args[0]!=='function')throw new TypeError('setInterval/setTimeout expect a function, delay and optional callback arguments.');
        [callback,delay]=args;parameters=args.slice(2);
      }
      if(typeof delay!=='number')throw new TypeError('Callback timer delay requires numeric milliseconds.');
      if(parameters.length>1024)throw new RangeError('Callback timers support at most 1024 arguments.');
      const registry=ids(),id=registry.register(delay,repeating),ownerGeneration=generation;
      let timer;
      try{
        timer=new Timer(delay,repeating?0:1);
        callbacks.set(id,{timer});
        timer.addEventListener(TimerEvent.TIMER,()=>{
          if(!registry.fire(id))return;
          if(!repeating)callbacks.delete(id);
          try{
            // AS2 method names are resolved at delivery, so replacement between
            // registration and the next callback observes the new function.
            const fn=method===undefined?callback:receiver[method];
            if(typeof fn!=='function')throw new TypeError(`Callback timer method '${method}' is not callable.`);
            Reflect.apply(fn,receiver,parameters);
          }catch(error){
            if(!disposed&&ownerGeneration===generation){registry.clear(id);callbacks.delete(id);timer.stop();}
            throw error;
          }
        });
        timer.start();return id;
      }catch(error){registry.clear(id);callbacks.delete(id);if(timer)timer.stop();throw error;}
    }
    const setInterval=(...args)=>createCallback(true,false,args);
    const setTimeout=(...args)=>createCallback(false,false,args);
    const legacy=Object.freeze({setInterval:(...args)=>createCallback(true,true,args),setTimeout:(...args)=>createCallback(false,true,args),clearInterval:clearCallback,clearTimeout:clearCallback});
    function clearCallbacks(){
      callbackIds?.reset();callbacks.clear();
    }
    function reset(){
      open();const nextOrigin=clock();
      clearCallbacks();
      for(const record of [...running])stop(record);
      generation++;hub.clear();origin=nextOrigin;
    }
    function dispose(){
      if(disposed)return;
      clearCallbacks();
      for(const record of [...running])stop(record);
      generation++;hub.clear();callbackIds?.free();callbackIds=null;disposed=true;
    }
    function getTimer(){
      open();const elapsed=clock()-origin;
      if(elapsed<0||!Number.isFinite(elapsed))throw new RangeError('Timer clock must advance monotonically within the publication session.');
      return Math.trunc(elapsed)|0;
    }
    return Object.freeze({Timer,TimerEvent,getTimer,setInterval,setTimeout,clearInterval:clearCallback,clearTimeout:clearCallback,legacy,reset,dispose});
  }
  const api=Object.freeze({create});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;
  else global.NeoFlashTimers=api;
})(globalThis);
