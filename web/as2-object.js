/* AS2 dynamic property adapter. Rust owns watchpoint/reentrancy state; this
 * adapter retains authored function/object identities and invokes accessors.
 * No global JavaScript prototype is modified. */
(function(global){
  'use strict';
  function create(wasm,{isNative=()=>false,isProject=()=>false,globals=()=>({}),nativeRef=()=>null}={}){
    let kernel=new wasm.ScriptAS2Object(),ids=new WeakMap();const callbacks=new Map();
    const object=value=>value!==null&&(typeof value==='object'||typeof value==='function');
    function eligible(value){if(!object(value)||isNative(value)||Array.isArray(value))return false;const p=Object.getPrototypeOf(value);return p===null||p===Object.prototype||typeof value==='function'||isProject(value);}
    function identity(value){let id=ids.get(value);if(!id){id=kernel.new_id();ids.set(value,id);}return id;}
    function descriptor(value,name){for(let current=value;current;current=Object.getPrototypeOf(current)){const found=Object.getOwnPropertyDescriptor(current,name);if(found)return found;}return null;}
    const methods={
      watch(name,callback,userData){
        if(!eligible(this)||typeof callback!=='function')return false;name=String(name);
        const property=descriptor(this,name);if(property&&(!Object.hasOwn(property,'value')||property.writable===false))return false;
        const id=identity(this),old=kernel.watched(id,name),token=kernel.watch(id,name);if(!token)return false;
        if(old)callbacks.delete(old);callbacks.set(token,{callback,userData});return true;
      },
      unwatch(name){if(!object(this))return false;const id=ids.get(this);if(!id)return false;const token=kernel.unwatch(id,String(name));if(token)callbacks.delete(token);return token!==0;},
      addProperty(name,getter,setter){
        if(!eligible(this)||typeof getter!=='function'||(setter!==null&&typeof setter!=='function'))return false;name=String(name);
        const own=Object.getOwnPropertyDescriptor(this,name);if(own?.configurable===false||!Object.isExtensible(this)&&!own)return false;
        if(!kernel.add_accessor(identity(this),name))return false;
        Object.defineProperty(this,name,{enumerable:true,configurable:true,get:getter,set:setter===null?undefined:setter});return true;
      },
      isPropertyEnumerable(name){return object(this)&&Object.prototype.propertyIsEnumerable.call(this,String(name));},
    };
    function get(receiver,name,read=()=>receiver[name],has=()=>Reflect.has(Object(receiver),name)){
      if(has())return read();
      if(object(receiver)&&Object.hasOwn(methods,name))return methods[name];
      if(name==='__resolve'||!eligible(receiver))return undefined;
      const resolver=receiver.__resolve;if(typeof resolver!=='function')return undefined;
      const id=identity(receiver);if(!kernel.begin_resolve(id,name))return undefined;
      try{return Reflect.apply(resolver,receiver,[name]);}finally{kernel.end_resolve(id,name);}
    }
    function set(receiver,name,value,read,write){
      const id=object(receiver)?ids.get(receiver):0,token=id?kernel.begin_write(id,name):0;
      if(!token){write(value);return value;}
      try{const item=callbacks.get(token),next=item?Reflect.apply(item.callback,receiver,[name,read(),value,item.userData]):value;write(next);return value;}finally{kernel.end_write(id,name);}
    }
    function deleted(receiver,name){const id=object(receiver)?ids.get(receiver):0;if(id)kernel.deleted(id,name);}
    function namedGet(receiver,name){const ref=nativeRef(receiver,name);return ref?ref.get():get(receiver,name);}
    function resolve(frame,path,lexical,writing,value){
      if(!writing&&object(path))return path;
      if(typeof path!=='string')throw new TypeError('AS2 eval/set requires a variable path string or an object reference for eval.');
      const parts=JSON.parse(wasm.as2_reference_path_json(path));if(!parts.length){if(writing)throw new TypeError('AS2 set requires a nonempty variable path.');return undefined;}
      const first=parts.shift(),known=lexical?.(first),environment=globals();
      const root=first==='this'?{get:()=>frame}:known??(Object.hasOwn(environment,first)?{get:()=>environment[first]}:null);
      let owner=frame,name=first;
      if(parts.length){owner=root?root.get():namedGet(frame,first);while(parts.length>1){if(owner==null){if(writing)throw new TypeError('AS2 set target does not exist.');return undefined;}owner=namedGet(owner,parts.shift());}name=parts[0];}
      if(writing){
        if(!parts.length&&root){if(!root.set)throw new TypeError('This AS2 named binding cannot be reassigned.');root.set(value);return undefined;}
        if(owner==null)throw new TypeError('AS2 set target does not exist.');
        const ref=nativeRef(owner,name);if(ref){ref.set(value);return undefined;}set(owner,name,value,()=>owner[name],next=>{if(!Reflect.set(owner,name,next))throw new TypeError('Cannot write AS2 named property.');});return undefined;
      }
      return !parts.length&&root?root.get():owner==null?undefined:namedGet(owner,name);
    }
    return {get,set,deleted,resolve,reset(){kernel.reset();ids=new WeakMap();callbacks.clear();},dispose(){kernel?.free();kernel=null;ids=new WeakMap();callbacks.clear();}};
  }
  const api={create};if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Object=api;
})(typeof globalThis!=='undefined'?globalThis:this);
