/* Numeric host conversions and entropy only; all Math calculations run in Rust. */
(function(global){
  'use strict';
  const arities={abs:1,acos:1,asin:1,atan:1,atan2:2,ceil:1,cos:1,exp:1,floor:1,log:1,pow:2,round:1,sin:1,sqrt:1,tan:1,random:0};
  const constants=['E','LN10','LN2','LOG10E','LOG2E','PI','SQRT1_2','SQRT2'];
  function create(wasm,{random=()=>Math.random()}={}) {
    if(typeof wasm?.as_math_value!=='function')throw new Error('Rust Math bindings are missing; rebuild and re-export this project.');
    function call(as3,name,args) {
      if(args.length>1000000)throw new RangeError('Math argument count exceeds 1000000.');
      let values;
      if(!as3)values=args.length===0?[]:[Number(args[0]),Number(args[1])];
      else if(name==='min'||name==='max'){
        values=[];
        // Stop coercing after NaN, matching the inspected AVM2 implementation.
        for(const arg of args){const value=Number(arg);values.push(value);if(Number.isNaN(value))break;}
      } else values=Array.from({length:arities[name]},(_,i)=>Number(args[i]));
      try{return wasm.as_math_value(as3,name,values,name==='random'?random():0);}
      catch(error){throw new RangeError(String(error));}
    }
    function family(as3){
      const value={};
      for(const name of [...Object.keys(arities),'min','max'])Object.defineProperty(value,name,{value:(...args)=>call(as3,name,args)});
      for(const name of constants)Object.defineProperty(value,name,{value:wasm.as_math_constant(name)});
      return Object.freeze(value);
    }
    return Object.freeze({as2:family(false),as3:family(true),randomInt(...args){
      if(args.length>1)throw new TypeError('AS2 random expects at most one upper bound.');
      try{return wasm.as_math_value(false,'randomInt',[Number(args[0])],random());}
      catch(error){throw new RangeError(String(error));}
    }});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashMath=api;
})(globalThis);
