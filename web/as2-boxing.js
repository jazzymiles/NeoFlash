/* Primitive object identity bridge. Rust owns conversion and value formatting;
 * String algorithms reuse the existing shared Rust UTF-16 service. */
(function(global){'use strict';
function create({execute,strings}){
 if(typeof execute!=='function'||typeof strings?.invoke!=='function')throw new TypeError('AS2 primitive boxes require the Rust boxing and String services.');
 const boxes=new WeakMap(),prototypes={};let generation=1,disposed=false;
 function text(units){let result='';for(let at=0;at<units.length;at+=8192)result+=String.fromCharCode(...units.slice(at,at+8192));return result;}
 function units(value){if(value.length>1000000)throw new RangeError('AS2 boxed String exceeds 1000000 UTF-16 units.');return Array.from({length:value.length},(_,i)=>value.charCodeAt(i));}
 function record(value){const r=boxes.get(value);if(!r||disposed||r.generation!==generation)throw new TypeError('This AS2 primitive-box receiver is invalid or belongs to a restarted publication.');return r;}
 function pack(value,target){
  if(value===undefined)return{kind:'Undefined'};if(value===null)return{kind:'Null'};
  if(typeof value==='string')return{kind:'String',value:units(value)};
  if(typeof value==='boolean')return{kind:'Boolean',value};
  if(typeof value==='number')return{kind:'Number',value:Object.is(value,-0)?'-0':String(value)};
  if(boxes.has(value)){const r=record(value);return target==='Boolean'?{kind:'Object'}:r.primitive;}
  if(target==='Boolean'&&(typeof value==='object'||typeof value==='function'))return{kind:'Object'};
  throw new TypeError('AS2 boxed construction accepts primitives or these primitive boxes; arbitrary object coercion is outside this profile.');
 }
 function valueOf(){return record(this).value;}
 function toString(radix){const r=record(this);if(r.primitive.kind==='Number'&&arguments.length&&radix!==10)throw new TypeError('AS2 boxed Number.toString currently supports decimal radix 10.');return r.text;}
 for(const name of ['Number','Boolean','String']){const p=Object.create(Object.prototype);Object.defineProperties(p,{valueOf:{value:valueOf},toString:{value:toString}});prototypes[name]=p;}
 for(const name of ['charAt','charCodeAt','concat','indexOf','lastIndexOf','slice','substr','substring','split','toLowerCase','toUpperCase'])Object.defineProperty(prototypes.String,name,{value:function(...args){const r=record(this);if(r.primitive.kind!=='String')throw new TypeError('AS2 String method requires a String box.');return strings.invoke(name,r.value,args);}});
 for(const p of Object.values(prototypes))Object.freeze(p);
 function construct(name,args){
  if(disposed)throw new TypeError('AS2 boxing service disposed.');if(!Object.hasOwn(prototypes,name))throw new TypeError('Unsupported AS2 primitive box.');
  let data;try{data=JSON.parse(execute(JSON.stringify({target:name,supplied:args.length>0,source:pack(args[0],name)})));}catch(error){if(error instanceof Error)throw error;throw new TypeError(String(error));}
  const primitive=data.primitive,value=primitive.kind==='String'?text(primitive.value):primitive.kind==='Number'?Number(primitive.value):primitive.value;
  const object=Object.create(prototypes[name]);boxes.set(object,{primitive,value,text:text(data.text),length:data.length,generation});
  if(name==='String')Object.defineProperty(object,'length',{get(){return record(this).length;},enumerable:false});
  return object;
 }
 return Object.freeze({create:construct,is:value=>boxes.has(value),kind:value=>record(value).primitive.kind,reset(){generation++;},dispose(){disposed=true;generation++;}});
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Boxing=api;
})(globalThis);
