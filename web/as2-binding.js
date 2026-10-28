/* AS2 field/binding reference bridge. Endpoint parsing, primitive conversion and
 * two-way/cycle policy execute in Rust. No global object prototype is patched.
 * This is an inferred primitive-field profile, not a schema/XML framework. */
(function(global){
  'use strict';
  function create(wasm,options={}){
    if(typeof wasm?.ScriptAS2Binding!=='function'||typeof wasm?.script_as2_binding_value!=='function'||typeof options.EventDispatcher?.initialize!=='function')throw new Error('AS2 bindings require the Rust binding module and AS2 EventDispatcher.');
    const endpoints=new WeakMap(),typedValues=new WeakMap(),fieldStates=new WeakMap(),components=new WeakMap(),bindings=new WeakMap(),records=new Set(),componentRecords=new Set();
    let generation=1,disposed=false;
    function active(value){if(disposed||value.generation!==generation)throw new Error('This AS2 binding belongs to a disposed or restarted publication.');}
    function object(value){if(value===null||(typeof value!=='object'&&typeof value!=='function'))throw new TypeError('Binding components require objects.');return value;}
    function unsupported(name){throw new Error(`${name} requires schema/custom formatter support beyond the current primitive-field binding profile.`);}
    function request(value){try{return JSON.parse(wasm.script_as2_binding_value(JSON.stringify(value)));}catch(error){throw new Error(String(error));}}
    function pack(value){
      if(value===undefined)return{kind:'Undefined'};if(value===null)return{kind:'Null'};
      if(typeof value==='number'){if(!Number.isFinite(value))throw new TypeError('Binding primitive Number values must be finite.');return{kind:'Number',value};}
      if(typeof value==='string')return{kind:'String',value};if(typeof value==='boolean')return{kind:'Boolean',value};
      return{kind:Array.isArray(value)?'Array':typeof value==='function'?'Function':'Object',ref:1};
    }
    function converted(value,target){const result=request({op:'convert',source:pack(value),target});return result.ref?value:result.kind==='Undefined'?undefined:result.kind==='Null'?null:result.value;}
    function read(reference){active(reference);let value=reference.component;for(const name of reference.path){if(value==null)throw new Error(`Binding path cannot read '${name}' through null or undefined.`);value=Reflect.get(Object(value),name,value);}return value;}
    function write(reference,value){active(reference);let receiver=reference.component;for(const name of reference.path.slice(0,-1)){if(receiver==null)throw new Error(`Binding path cannot traverse '${name}'.`);receiver=receiver[name];}object(receiver);const key=reference.path[reference.path.length-1];if(!Reflect.set(receiver,key,value,receiver))throw new Error(`Binding destination field '${key}' is read-only.`);}
    function signal(component,valid,property,errors){if(typeof component?.dispatchEvent==='function')component.dispatchEvent({type:valid?'valid':'invalid',target:component,property,errors});}
    class EndPoint{
      constructor(){endpoints.set(this,{generation,component:null,property:null,location:null,event:null,constant:undefined,hasConstant:false});}
    }
    for(const name of ['component','property','location','event','constant'])Object.defineProperty(EndPoint.prototype,name,{get(){const value=endpoints.get(this);if(!value)throw new TypeError('Expected EndPoint.');active(value);return value[name];},set(next){const value=endpoints.get(this);if(!value)throw new TypeError('Expected EndPoint.');active(value);value[name]=next;if(name==='constant')value.hasConstant=true;}});
    class TypedValue{
      constructor(value,typeName,type=null){if(typeof typeName!=='string'||typeName.length>256)throw new TypeError('TypedValue requires a type name string up to 256 characters.');typedValues.set(this,{generation,value,typeName,type});}
    }
    for(const name of ['value','typeName','type'])Object.defineProperty(TypedValue.prototype,name,{get(){const value=typedValues.get(this);if(!value)throw new TypeError('Expected TypedValue.');active(value);return value[name];},set(next){const value=typedValues.get(this);if(!value)throw new TypeError('Expected TypedValue.');active(value);if(name==='typeName'&&(typeof next!=='string'||next.length>256))throw new TypeError('TypedValue requires a type name string.');value[name]=next;}});
    function fieldState(value){const result=fieldStates.get(value);if(!result)throw new TypeError('Get a DataType from component.getField(property, location).');active(result);return result;}
    function errors(error){return[String(error?.message||error)];}
    class DataType{
      constructor(){throw new TypeError('DataType is obtained with component.getField(property, location).');}
      getTypedValue(requestedType=null){const value=fieldState(this),raw=read(value),target=requestedType===null?value.typeName:requestedType;try{const result=converted(raw,target);return new TypedValue(result,target,null);}catch(_){return null;}}
      getAnyTypedValue(){unsupported('DataType.getAnyTypedValue preferred-type/schema negotiation');}
      getAsString(){return converted(read(fieldState(this)),'String');}
      getAsNumber(){return converted(read(fieldState(this)),'Number');}
      getAsBoolean(){return converted(read(fieldState(this)),'Boolean');}
      setTypedValue(value){return setTyped(fieldState(this),value,false);}
      setAnyTypedValue(value){return setTyped(fieldState(this),value,true);}
      setAsString(value){if(typeof value!=='string')throw new TypeError('setAsString requires String.');setPrimitive(fieldState(this),new TypedValue(value,'String'));}
      setAsNumber(value){if(typeof value!=='number')throw new TypeError('setAsNumber requires Number.');setPrimitive(fieldState(this),new TypedValue(value,'Number'));}
      setAsBoolean(value){if(typeof value!=='boolean')throw new TypeError('setAsBoolean requires Boolean.');setPrimitive(fieldState(this),new TypedValue(value,'Boolean'));}
      get encoder(){unsupported('DataType.encoder');}get formatter(){unsupported('DataType.formatter');}get kind(){unsupported('DataType.kind schema accessor');}
    }
    function setPrimitive(reference,value){const result=setTyped(reference,value,true);if(result)throw new Error(result.join('; '));}
    function setTyped(reference,value,coerce){
      active(reference);const typed=typedValues.get(value);if(!typed)throw new TypeError('setTypedValue requires a TypedValue from this publication.');active(typed);
      try{
        if(typed.type!==null&&typed.type!==undefined)unsupported('Custom TypedValue schema conversion');
        // A declaration and the supplied primitive must agree before conversion.
        const declared=converted(typed.value,typed.typeName);
        if(!coerce&&reference.typeName!==typed.typeName&&reference.typeName!=='Object'&&reference.typeName!=='Null'&&reference.typeName!=='Undefined')return[`Expected ${reference.typeName}, received ${typed.typeName}.`];
        const value=['Undefined','Null'].includes(reference.typeName)?declared:converted(declared,reference.typeName);write(reference,value);return null;
      }catch(error){return errors(error);}
    }
    function getField(property,location=null){
      const component=componentState(this),path=request({op:'path',property,location}),key=JSON.stringify(path);let result=component.fields.get(key);
      if(!result){const reference={generation,component:this,path,typeName:'Undefined'},raw=read(reference);reference.typeName=pack(raw).kind;result=Object.create(DataType.prototype);fieldStates.set(result,reference);component.fields.set(key,result);}
      return result;
    }
    function validateProperty(property){
      const component=componentState(this),value=component.target.getField(property),reference=fieldState(value);let result=null;
      try{converted(read(reference),reference.typeName);}catch(error){result=errors(error);}
      signal(component.target,result===null,property,result);return result;
    }
    function refreshDestinations(){componentState(this);for(const record of [...records]){active(record);if(record.source.component===this)record.target.execute();else if(record.plan.twoWay&&record.destination.component===this)record.target.execute(true);}}
    function refreshFromSources(){componentState(this);for(const record of [...records]){active(record);if(record.destination.component===this)record.target.execute();else if(record.plan.twoWay&&record.source.component===this)record.target.execute(true);}}
    function componentState(value){const record=components.get(value);if(!record)throw new TypeError('Initialize this component with ComponentMixins.initComponent.');active(record);return record;}
    function initComponent(target){
      object(target);if(disposed)throw new Error('The AS2 binding service is disposed.');const old=components.get(target);if(old?.generation===generation)return;
      if(componentRecords.size>=1024)throw new RangeError('A publication supports at most 1024 bound component objects before Restart.');
      for(const name of ['getField','validateProperty','refreshDestinations','refreshFromSources'])if(name in target&&!old)throw new Error(`Binding initialization would replace existing component method '${name}'.`);
      if(typeof target.addEventListener!=='function'||typeof target.removeEventListener!=='function'||typeof target.dispatchEvent!=='function')options.EventDispatcher.initialize(target);
      Object.defineProperties(target,{getField:{value:getField,configurable:true,writable:true},validateProperty:{value:validateProperty,configurable:true,writable:true},refreshDestinations:{value:refreshDestinations,configurable:true,writable:true},refreshFromSources:{value:refreshFromSources,configurable:true,writable:true}});
      const record={generation,target,fields:new Map()};components.set(target,record);componentRecords.add(record);
    }
    function ComponentMixins(){throw new TypeError('Use ComponentMixins.initComponent(target).');}
    ComponentMixins.initComponent=initComponent;
    function endpoint(value,destination){
      object(value);const stored=endpoints.get(value);if(stored)active(stored);
      const constant=stored?stored.hasConstant:Object.hasOwn(value,'constant');
      if(constant){if(destination)throw new TypeError('A binding destination cannot be constant.');return{constant:true,value:value.constant,component:null,property:null,location:null,event:null};}
      const result={constant:false,component:object(value.component),property:value.property,location:value.location??null,event:value.event??null};
      initComponent(result.component);return result;
    }
    class Binding{
      constructor(source,destination,format=null,isTwoWay=false){
        if(disposed)throw new Error('The AS2 binding service is disposed.');if(format!==null&&format!==undefined)unsupported('Custom Binding formatters');if(typeof isTwoWay!=='boolean')throw new TypeError('Binding isTwoWay requires Boolean.');if(records.size>=1024)throw new RangeError('At most 1024 bindings per publication.');
        source=endpoint(source,false);destination=endpoint(destination,true);
        const core=new wasm.ScriptAS2Binding(JSON.stringify({source:{constant:source.constant,property:source.property,location:source.location,event:source.event},destination:{constant:false,property:destination.property,location:destination.location,event:destination.event},twoWay:isTwoWay}));
        const plan=JSON.parse(core.plan()),record={generation,core,plan,source,destination,target:this,subscriptions:[]};
        try{
          record.sourceField=source.constant?null:source.component.getField(source.property,source.location);
          record.destinationField=destination.component.getField(destination.property,destination.location);
          bindings.set(this,record);records.add(record);
          for(const [endpoint,names,reverse]of [[source,plan.sourceEvents,false],[destination,plan.destinationEvents,true]])for(const name of names){const callback=()=>{if(disposed||record.generation!==generation)return;this.execute(reverse);};endpoint.component.addEventListener(name,callback);record.subscriptions.push(()=>endpoint.component.removeEventListener(name,callback));}
          this.execute();
        }catch(error){for(const cleanup of record.subscriptions)cleanup();records.delete(record);core.free();throw error;}
      }
      execute(reverse=false){
        const record=bindings.get(this);if(!record)throw new TypeError('Expected Binding.');active(record);if(typeof reverse!=='boolean')throw new TypeError('Binding.execute reverse requires Boolean.');const direction=record.core.enter(reverse);if(!direction)return null;
        try{
          const source=direction===2?record.destination:record.source,destination=direction===2?record.source:record.destination;
          const sourceField=direction===2?record.destinationField:record.sourceField,destinationField=direction===2?record.sourceField:record.destinationField;
          const raw=source.constant?source.value:read(fieldState(sourceField));
          const result=setTyped(fieldState(destinationField),new TypedValue(raw,pack(raw).kind),true);
          signal(destination.component,result===null,destination.property,result);if(source.component)signal(source.component,result===null,source.property,result);
          return result;
        }catch(error){const result=errors(error);signal(record.destination.component,false,record.destination.property,result);return result;}
        finally{if(!disposed&&record.generation===generation)record.core.leave();}
      }
    }
    for(const [Ctor,table]of [[EndPoint,endpoints],[TypedValue,typedValues],[DataType,fieldStates],[Binding,bindings]]){Ctor.__nfIs=value=>table.has(value);Ctor.__nfInitialize=()=>unsupported(Ctor.name+' native subclassing');}
    ComponentMixins.__nfIs=()=>false;ComponentMixins.__nfInitialize=()=>unsupported('ComponentMixins native subclassing');
    const builtins=Object.freeze({'mx.data.binding.EndPoint':EndPoint,'mx.data.binding.TypedValue':TypedValue,'mx.data.binding.DataType':DataType,'mx.data.binding.Binding':Binding,'mx.data.binding.ComponentMixins':ComponentMixins});
    function reset(){if(disposed)return;for(const record of records){for(const cleanup of record.subscriptions){try{cleanup();}catch(_){}}record.core.free();}records.clear();for(const record of componentRecords)record.fields.clear();componentRecords.clear();generation++;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({builtins,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Binding=api;
})(globalThis);
