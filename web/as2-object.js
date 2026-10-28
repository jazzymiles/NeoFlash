/* AS2 dynamic property adapter. Rust owns watchpoint/reentrancy state; this
 * adapter retains authored function/object identities and invokes accessors.
 * No global JavaScript prototype is modified. */
(function(global){
  'use strict';
  function create(wasm,{isNative=()=>false,isProject=()=>false,globals=()=>({}),nativeRef=()=>null,textVariable=null}={}){
    let kernel=new wasm.ScriptAS2Object(),ids=new WeakMap(),generation=1,syncingText=false;const callbacks=new Map(),textFields=new Map();
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
      if(!token){write(value);syncTextVariables();return value;}
      try{const item=callbacks.get(token),next=item?Reflect.apply(item.callback,receiver,[name,read(),value,item.userData]):value;write(next);syncTextVariables();return value;}finally{kernel.end_write(id,name);}
    }
    function deleted(receiver,name){const id=object(receiver)?ids.get(receiver):0;if(id)kernel.deleted(id,name);}
    function namedGet(receiver,name){const ref=nativeRef(receiver,name);return ref?ref.get():get(receiver,name);}
    function namedOwner(frame,parts,root=null){
      let owner=frame;const name=parts[parts.length-1];
      if(parts.length>1){owner=root?root.get():namedGet(frame,parts[0]);for(let i=1;i<parts.length-1;i++){if(owner==null)return{owner:null,name};owner=namedGet(owner,parts[i]);}}
      return{owner,name};
    }
    function textCommand(value){if(!kernel)throw new Error('AS2 Object service disposed.');return JSON.parse(kernel.text_binding(JSON.stringify(value)));}
    function packedPrimitive(value){
      if(value===undefined)return{kind:'Undefined'};if(value===null)return{kind:'Null'};
      if(typeof value==='string')return{kind:'String',value};if(typeof value==='boolean')return{kind:'Boolean',value};
      if(typeof value==='number'&&Number.isFinite(value))return{kind:'Number',value};
      throw new TypeError('TextField.variable supports finite primitive MovieClip variables; object coercion is not implemented.');
    }
    function textOwner(record){
      const frame=textVariable.parentOf(record.field);if(!frame)return{owner:null,name:record.parts.at(-1)};
      const first=record.parts[0],environment=globals();const root=first==='this'?{get:()=>frame}:Object.hasOwn(environment,first)?{get:()=>environment[first]}:null;
      const ref=namedOwner(frame,record.parts,root);
      if(ref.owner!=null&&!textVariable.isMovieClip(ref.owner))throw new TypeError('TextField.variable must resolve to a MovieClip variable in this profile.');
      return ref;
    }
    function synchronizeText(record,fromField=false){
      if(!kernel||record.generation!==generation)return;
      if(!textVariable.alive(record.field)){textCommand({op:'bind',field:record.id,path:null});textFields.delete(record.id);return;}
      if(textVariable.isHTML?.(record.field))throw new Error('TextField.variable currently supports plain text fields; disable html or detach the binding.');
      const ref=textOwner(record),owner=ref.owner;const exists=owner!=null&&Reflect.has(owner,ref.name);
      const source=exists&&!fromField?packedPrimitive(namedGet(owner,ref.name)):{kind:'Undefined'};
      if(record.generation!==generation)return;
      const plan=textCommand({op:'plan',field:record.id,owner:owner?identity(owner):0,exists,source,text:textVariable.readText(record.field),fromField});
      if(!plan)return;let accepted=false;
      try{
        if(plan.kind==='text')textVariable.writeText(record.field,plan.value);
        else {const native=nativeRef(owner,ref.name);if(native)native.set(plan.value);else set(owner,ref.name,plan.value,()=>owner[ref.name],next=>{if(!Reflect.set(owner,ref.name,next))throw new Error('Bound MovieClip variable is read-only.');});}
        accepted=true;
      }finally{if(kernel&&record.generation===generation)textCommand({op:'finish',field:record.id,accepted});}
    }
    function syncTextVariables(){
      if(!textVariable||!kernel||syncingText||!textFields.size)return;
      syncingText=true;try{for(const record of [...textFields.values()])synchronizeText(record);}finally{syncingText=false;}
    }
    function variableRef(field){
      if(!textVariable?.isText(field))return null;
      return{get(){const id=ids.get(field);return id?textCommand({op:'get',field:id}):null;},set(value){
        if(value!==null&&value!==undefined&&typeof value!=='string')throw new TypeError('TextField.variable requires a String or null.');
        if(!textVariable.alive(field))throw new Error('Cannot bind a removed TextField.');
        if(value&&textVariable.isHTML?.(field))throw new Error('TextField.variable currently supports plain text fields only.');
        const id=identity(field),parts=textCommand({op:'bind',field:id,path:value??null});textFields.delete(id);
        if(parts){const record={id,field,parts,generation};textFields.set(id,record);synchronizeText(record);}
      },has:()=>true,delete:()=>false};
    }
    function textChanged(field){const id=ids.get(field),record=textFields.get(id);if(record){synchronizeText(record,true);syncTextVariables();}}
    function resolve(frame,path,lexical,writing,value){
      if(!writing&&object(path))return path;
      if(typeof path!=='string')throw new TypeError('AS2 eval/set requires a variable path string or an object reference for eval.');
      const parts=JSON.parse(wasm.as2_reference_path_json(path));if(!parts.length){if(writing)throw new TypeError('AS2 set requires a nonempty variable path.');return undefined;}
      const first=parts.shift(),known=lexical?.(first),environment=globals();
      const root=first==='this'?{get:()=>frame}:known??(Object.hasOwn(environment,first)?{get:()=>environment[first]}:null);
      let owner=frame,name=first;
      if(parts.length){const found=namedOwner(frame,[first,...parts],root);owner=found.owner;name=found.name;if(owner==null){if(writing)throw new TypeError('AS2 set target does not exist.');return undefined;}}
      if(writing){
        if(!parts.length&&root){if(!root.set)throw new TypeError('This AS2 named binding cannot be reassigned.');root.set(value);return undefined;}
        if(owner==null)throw new TypeError('AS2 set target does not exist.');
        const ref=nativeRef(owner,name);if(ref){ref.set(value);return undefined;}set(owner,name,value,()=>owner[name],next=>{if(!Reflect.set(owner,name,next))throw new TypeError('Cannot write AS2 named property.');});return undefined;
      }
      return !parts.length&&root?root.get():owner==null?undefined:namedGet(owner,name);
    }
    return {get,set,deleted,resolve,variableRef,textChanged,syncTextVariables,reset(){kernel.reset();generation++;ids=new WeakMap();callbacks.clear();textFields.clear();syncingText=false;},dispose(){kernel?.free();kernel=null;generation++;ids=new WeakMap();callbacks.clear();textFields.clear();syncingText=false;}};
  }
  const api={create};if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Object=api;
})(typeof globalThis!=='undefined'?globalThis:this);
