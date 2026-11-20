/* Original JavaScript value/graph bridge for the shared Rust AMF0/AMF3 codec.
 * No JSON substitution for AMF: JSON is only the lossless tagged WASM ABI.
 * Primitive numbers, arrays (dense + named own entries), records, Date,
 * ByteArray, Vector.<int|uint|Number|*|Object|T>, Dictionary and IExternalizable
 * preserve cycles/references. Source-class public fields/getters flatten to
 * records through program.jsonKeys; registered class aliases retain their type.
 */
(function(global){
  'use strict';
  const MAX_BYTES=16*1024*1024,MAX_VALUES=100000,MAX_DEPTH=128;
  const utf8=new TextEncoder(),dateTime=Date.prototype.getTime;
  const vectorBrand=Symbol.for('neoflash.as3.vector.instance.v1');
  const vectorTypeBrand=Symbol.for('neoflash.as3.vector.type.v1');
  const dictionaryBrand=Symbol.for('neoflash.as3.dictionary.v1');
  const externalizableBrand=Symbol.for('neoflash.externalizable');
  function format(value){if(value!==0&&value!==3)throw new RangeError('ObjectEncoding must be AMF0 (0) or AMF3 (3).');return value;}
  function number(value){return Object.is(value,-0)?'-0':String(value);}
  function fromNumber(value){if(typeof value!=='string')throw new TypeError('Invalid AMF numeric graph value.');if(value==='NaN')return NaN;if(value==='Infinity')return Infinity;if(value==='-Infinity')return -Infinity;if(value==='-0')return -0;const result=Number(value);if(!Number.isFinite(result)||value.trim()==='')throw new TypeError('Invalid AMF numeric graph value.');return result;}
  function base64(bytes){let text='';for(let at=0;at<bytes.length;at+=8192)text+=String.fromCharCode(...bytes.subarray(at,at+8192));return global.btoa(text);}
  function unbase64(text){
    if(typeof text!=='string'||text.length>Math.ceil(MAX_BYTES/3)*4||text.length%4||!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(text))throw new TypeError('Invalid bounded AMF byte payload.');
    const value=global.atob(text);if(value.length>MAX_BYTES)throw new RangeError('AMF byte payload exceeds 16 MiB.');return Uint8Array.from(value,char=>char.charCodeAt(0));
  }
  function plain(value){const p=Object.getPrototypeOf(value);if(p===null||p===Object.prototype)return true;const ctor=Object.getOwnPropertyDescriptor(p,'constructor')?.value;return Object.getPrototypeOf(p)===null&&typeof ctor==='function'&&ctor.name==='Object'&&ctor.prototype===p;}

  const standaloneVectorTypes=new Map();
  function createVector(elemName='Object',initialValues=[],fixed=false,elemCtor=null){
    const coerce=elemName==='int'?(v=>Number(v)|0)
      :elemName==='uint'?(v=>Number(v)>>>0)
      :elemName==='Number'?(v=>Number(v))
      :elemName==='Boolean'?(v=>Boolean(v))
      :elemName==='String'?(v=>v==null?null:String(v))
      :(elemCtor?v=>{if(v==null)return null;if(elemCtor.__nfIs?elemCtor.__nfIs(v):v instanceof elemCtor)return v;throw new TypeError(`Vector.<${elemName}> element type mismatch.`);}:(v=>v===undefined?null:v));
    const defaultValue=(elemName==='int'||elemName==='uint'||elemName==='Number')?0:elemName==='Boolean'?false:null;
    const tokenKey=elemCtor||elemName;
    let Type=standaloneVectorTypes.get(tokenKey);
    if(!Type){
      const element=Object.freeze({token:elemCtor||Symbol.for('neoflash.amf.vec.'+elemName),coerce,defaultValue,name:elemName});
      Type=function(len=0,isFixed=false){
        const arr=new Array(Number(len)|0).fill(defaultValue);
        return createVector(elemName,arr,Boolean(isFixed),elemCtor);
      };
      Object.defineProperty(Type,vectorTypeBrand,{value:element});
      standaloneVectorTypes.set(tokenKey,Type);
    }
    const element=Object.getOwnPropertyDescriptor(Type,vectorTypeBrand).value;
    const values=Array.from(initialValues,v=>coerce(v));
    const target=Object.create(Type.prototype);
    const state={version:1,element,type:Type,values,fixed:Boolean(fixed),methods:new Map(),lock:0,iterating:0,proxy:null};
    Object.defineProperty(target,vectorBrand,{value:state});
    const proxy=new Proxy(target,{
      get(obj,key){
        if(key===vectorBrand)return state;
        if(key===Symbol.toStringTag)return `Vector.<${elemName}>`;
        if(key===Symbol.iterator)return function*(){for(let i=0;i<state.values.length;i++)yield state.values[i];};
        if(key==='length')return state.values.length;
        if(key==='fixed')return state.fixed;
        if(key==='push')return (...items)=>{if(state.fixed)throw new RangeError('Vector is fixed.');for(const it of items)state.values.push(coerce(it));return state.values.length;};
        if(typeof key==='string'&&/^(?:0|[1-9]\d*)$/.test(key)){const idx=Number(key);if(idx>=state.values.length)throw new RangeError('Index out of bounds.');return state.values[idx];}
        return Reflect.get(obj,key);
      },
      set(_obj,key,val){
        if(key==='fixed'){state.fixed=Boolean(val);return true;}
        if(key==='length'){if(state.fixed)throw new RangeError('Vector is fixed.');const next=Number(val)|0,old=state.values.length;state.values.length=next;for(let i=old;i<next;i++)state.values[i]=defaultValue;return true;}
        if(typeof key==='string'&&/^(?:0|[1-9]\d*)$/.test(key)){const idx=Number(key);if(idx>state.values.length||(idx===state.values.length&&state.fixed))throw new RangeError('Index out of bounds.');state.values[idx]=coerce(val);return true;}
        throw new ReferenceError(`Cannot set ${String(key)} on Vector.`);
      },
    });
    state.proxy=proxy;
    return proxy;
  }

  function createDictionary(weakKeys=false){
    const weak=Boolean(weakKeys);
    const target={};
    const state={version:1,target,weak,objects:weak?new WeakMap():new Map(),primitives:new Map(),entries:new Map(),nextToken:1,keyUnits:0};
    Object.defineProperty(target,dictionaryBrand,{value:state});
    const isObjKey=k=>k!==null&&(typeof k==='object'||typeof k==='function');
    let proxy;
    proxy=new Proxy(target,{
      get(obj,key){
        if(key===dictionaryBrand)return state;
        if(key===Symbol.toStringTag)return 'Dictionary';
        if(typeof key==='symbol')return Reflect.get(obj,key,proxy);
        if(isObjKey(key))return state.objects.get(key)?.value;
        const rec=state.primitives.get(String(key));
        return rec?rec.value:Reflect.get(obj,String(key),proxy);
      },
      set(_obj,key,val){
        if(typeof key==='symbol')throw new TypeError('Symbol keys are unsupported on Dictionary.');
        const objK=isObjKey(key),k=objK?key:String(key),store=objK?state.objects:state.primitives;
        const existing=store.get(k);
        if(existing){existing.value=val;return true;}
        const token=state.nextToken++;
        store.set(k,{token,value:val});
        state.entries.set(token,objK?(weak?{weak:new WeakRef(k)}:{object:k}):{primitive:k});
        return true;
      },
      has(_obj,key){
        if(typeof key==='symbol')return Reflect.has(target,key);
        return isObjKey(key)?state.objects.has(key):state.primitives.has(String(key));
      },
      deleteProperty(_obj,key){
        if(typeof key==='symbol')return false;
        const objK=isObjKey(key),k=objK?key:String(key),store=objK?state.objects:state.primitives;
        const rec=store.get(k);
        if(rec){store.delete(k);state.entries.delete(rec.token);}
        return true;
      },
    });
    Object.defineProperties(target,{
      get:{value(k){return isObjKey(k)?state.objects.get(k)?.value:state.primitives.get(String(k))?.value;},enumerable:false},
      set:{value(k,v){
        const objK=isObjKey(k),nk=objK?k:String(k),store=objK?state.objects:state.primitives;
        const existing=store.get(nk);
        if(existing){existing.value=v;return proxy;}
        const token=state.nextToken++;
        store.set(nk,{token,value:v});
        state.entries.set(token,objK?(weak?{weak:new WeakRef(nk)}:{object:nk}):{primitive:nk});
        return proxy;
      },enumerable:false},
      hasKey:{value(k){return isObjKey(k)?state.objects.has(k):state.primitives.has(String(k));},enumerable:false},
    });
    return proxy;
  }

  function vectorStateOf(value){
    if(value===null||(typeof value!=='object'&&typeof value!=='function'))return null;
    const d=Object.getOwnPropertyDescriptor(value,vectorBrand);
    return d&&Object.hasOwn(d,'value')&&d.value?.version===1?d.value:null;
  }
  function dictionaryStateOf(value){
    if(value===null||(typeof value!=='object'&&typeof value!=='function'))return null;
    const d=Object.getOwnPropertyDescriptor(value,dictionaryBrand);
    return d&&Object.hasOwn(d,'value')&&d.value?.version===1?d.value:null;
  }
  function dictionarySetEntry(dict,key,val){
    const st=dictionaryStateOf(dict);
    if(!st)throw new TypeError('Expected Dictionary instance.');
    const objK=key!==null&&(typeof key==='object'||typeof key==='function');
    const store=objK?st.objects:st.primitives;
    const nk=objK?key:String(key);
    const existing=store.get(nk);
    if(existing){existing.value=val;return;}
    const token=st.nextToken++;
    store.set(nk,{token,value:val});
    st.entries.set(token,objK?(st.weak?{weak:new WeakRef(nk)}:{object:nk}):{primitive:nk});
    if(!objK&&typeof st.keyUnits==='number')st.keyUnits+=nk.length;
  }

  const dynamicOutputs=new WeakMap();
  class DynamicPropertyOutput {
    constructor(){
      dynamicOutputs.set(this,new Map());
    }
    writeDynamicProperty(name,value){
      const map=dynamicOutputs.get(this);
      if(!map)throw new TypeError('DynamicPropertyOutput requires a valid instance.');
      if(name===null||name===undefined)throw new TypeError('Error #2007: Parameter name must be non-null.');
      map.set(String(name),value);
    }
  }
  Object.defineProperties(DynamicPropertyOutput,{
    __nfIs:{value:v=>dynamicOutputs.has(v)},
    __nfInitialize:{value:target=>{if(!dynamicOutputs.has(target))dynamicOutputs.set(target,new Map());return target;}},
  });

  function create(options={}){
    const {binary}=options;
    if(typeof options.encode!=='function'||typeof options.decode!=='function'||!binary?.fromByteArrays)throw new TypeError('AMF requires the shared Rust codec and ByteArray service.');
    const aliasToClass=new Map(),classToAlias=new Map(),classNameToAlias=new Map();
    let dynamicWriterGetter=typeof options.getDynamicPropertyWriter==='function'?options.getDynamicPropertyWriter:null;
    function setDynamicPropertyWriterGetter(fn){
      dynamicWriterGetter=typeof fn==='function'?fn:null;
    }
    function registerClassAlias(aliasName,classObject){
      if(aliasName==null||classObject==null)throw new TypeError('Error #2007: Parameter aliasName and classObject must be non-null.');
      const prog=options.getProgram?.(),alias=String(aliasName);
      if(typeof classObject!=='function'&&!prog?.isClass?.(classObject))throw new TypeError('registerClassAlias requires a Class constructor.');
      aliasToClass.set(alias,classObject);
      classToAlias.set(classObject,alias);
      const qname=prog?.className?.(classObject);
      if(typeof qname==='string'&&qname)classNameToAlias.set(qname.replace('::','.'),alias);
    }
    function getClassByAlias(aliasName){
      if(aliasName==null)throw new TypeError('Error #2007: Parameter aliasName must be non-null.');
      const alias=String(aliasName);
      if(!aliasToClass.has(alias)){
        const error=new ReferenceError(`Error #1014: Class ${alias} could not be found.`);
        error.errorID=1014;
        throw error;
      }
      return aliasToClass.get(alias);
    }
    function reset(){
      aliasToClass.clear();
      classToAlias.clear();
      classNameToAlias.clear();
    }
    function graph(value,graphOptions={}){
      const seen=new Map(),nodes=[],prog=options.getProgram?.(),xml=options.getXML?.();let visits=0,bytes=0;
      const dynWriter=dynamicWriterGetter?.()||null;
      function text(value){
        if(typeof value!=='string'||value.length>MAX_BYTES)throw new RangeError('AMF strings exceed the 16 MiB graph budget.');
        for(let i=0;i<value.length;i++){const c=value.charCodeAt(i);if(c>=0xd800&&c<=0xdbff){const low=value.charCodeAt(++i);if(!(low>=0xdc00&&low<=0xdfff))throw new TypeError('AMF strings require complete Unicode characters.');}else if(c>=0xdc00&&c<=0xdfff)throw new TypeError('AMF strings require complete Unicode characters.');}
        bytes+=utf8.encode(value).length;if(bytes>MAX_BYTES)throw new RangeError('AMF strings/bytes exceed 16 MiB graph budget.');return value;
      }
      function visit(value,depth){
        if(++visits>MAX_VALUES||depth>MAX_DEPTH)throw new RangeError('AMF graph exceeds 100000 values or 128 levels.');
        if(value===undefined||value===null)return {type:value===null?'null':'undefined'};
        if(typeof value==='boolean')return {type:'bool',value};
        if(typeof value==='number')return {type:'number',value:number(value)};
        if(typeof value==='string')return {type:'string',value:text(value)};
        if(typeof value!=='object'&&typeof value!=='function')throw new TypeError('AMF cannot serialize Symbols or BigInts in this profile.');
        if(typeof value==='function'&&!vectorStateOf(value)&&!dictionaryStateOf(value))throw new TypeError('AMF cannot serialize functions, Symbols or BigInts in this profile.');
        if(seen.has(value))return {type:'ref',value:seen.get(value)};
        if(options.isUnsupported?.(value))throw new TypeError('AMF display/native objects require an explicit data record; class aliases are not implemented.');
        const index=nodes.length;if(index>=MAX_VALUES)throw new RangeError('AMF graph exceeds 100000 nodes.');seen.set(value,index);nodes.push(null);
        if(binary.ByteArray.__nfIs(value)){const data=binary.toBytes(value);bytes+=data.length;if(bytes>MAX_BYTES)throw new RangeError('AMF strings/bytes exceed 16 MiB.');nodes[index]={type:'bytes',base64:base64(data)};return {type:'ref',value:index};}
        if(xml?.XML?.__nfIs(value)){nodes[index]={type:'xml',text:text(xml.method(value,'toXMLString',true)())};return {type:'ref',value:index};}
        if(xml?.XMLDocument?.__nfIs(value)||xml?.LegacyXML?.__nfIs(value)){nodes[index]={type:'xml_document',text:text(Reflect.apply(xml.XMLNode.prototype.toString,value,[]))};return {type:'ref',value:index};}
        if(xml?.XMLList?.__nfIs(value))throw new TypeError('AMF XMLList serialization is not supported; use an XML value.');

        const vecState=vectorStateOf(value);
        if(vecState){
          const vals=vecState.values||[],fixed=Boolean(vecState.fixed),elemName=String(vecState.element?.name||'Object');
          if(vals.length>MAX_VALUES)throw new RangeError('AMF Vector exceeds 100000 entries.');
          if(elemName==='int'){
            bytes+=vals.length*4;if(bytes>MAX_BYTES)throw new RangeError('AMF Vector bytes exceed 16 MiB.');
            nodes[index]={type:'vector_int',fixed,values:vals.map(v=>Number(v)|0)};
          }else if(elemName==='uint'){
            bytes+=vals.length*4;if(bytes>MAX_BYTES)throw new RangeError('AMF Vector bytes exceed 16 MiB.');
            nodes[index]={type:'vector_uint',fixed,values:vals.map(v=>Number(v)>>>0)};
          }else if(elemName==='Number'){
            bytes+=vals.length*8;if(bytes>MAX_BYTES)throw new RangeError('AMF Vector bytes exceed 16 MiB.');
            nodes[index]={type:'vector_number',fixed,values:vals.map(v=>number(Number(v)))};
          }else{
            let wireType='';
            if(elemName==='*')wireType='*';
            else if(elemName==='Object')wireType='';
            else if(['String','Boolean','Date','RegExp','XML','Array','Function','Class','int','uint','Number'].includes(elemName)||elemName.startsWith('Vector.<'))wireType=elemName;
            else wireType=classNameToAlias.get(elemName)||(vecState.element?.token&&classToAlias.get(vecState.element.token))||elemName;
            nodes[index]={type:'vector_object',fixed,type_name:text(wireType),values:vals.map(v=>visit(v,depth+1))};
          }
          return {type:'ref',value:index};
        }

        const dictState=dictionaryStateOf(value);
        if(dictState){
          const pairs=[];
          for(const [tok,entry] of dictState.entries){
            const isObj=Object.hasOwn(entry,'object')||Object.hasOwn(entry,'weak');
            const rawKey=entry.weak?entry.weak.deref():isObj?entry.object:entry.primitive;
            if(isObj&&rawKey===undefined)continue;
            const rec=(isObj?dictState.objects:dictState.primitives).get(isObj?rawKey:entry.primitive);
            if(!rec||rec.token!==tok)continue;
            const keyVal=isObj?rawKey:(/^(?:0|[1-9]\d*)$/.test(rawKey)&&Number(rawKey)<=268435455?Number(rawKey):rawKey);
            pairs.push([visit(keyVal,depth+1),visit(rec.value,depth+1)]);
            if(pairs.length>MAX_VALUES)throw new RangeError('AMF Dictionary exceeds 100000 entries.');
          }
          nodes[index]={type:'dictionary',weak_keys:Boolean(dictState.weak),entries:pairs};
          return {type:'ref',value:index};
        }

        let milliseconds;try{milliseconds=Reflect.apply(dateTime,value,[]);}catch(_){milliseconds=undefined;}
        if(milliseconds!==undefined){nodes[index]={type:'date',milliseconds:number(milliseconds)};return {type:'ref',value:index};}

        const array=Array.isArray(value);
        const instanceName=!array?prog?.instanceClassName?.(value):null;
        const ctor=!array?Object.getOwnPropertyDescriptor(Object.getPrototypeOf(value)||{},'constructor')?.value:null;
        const alias=!array?((instanceName&&classNameToAlias.get(instanceName))||classToAlias.get(ctor)||null):null;
        let progIsExt=false;
        if(!array&&prog?.is){try{progIsExt=Boolean(prog.is(value,'flash.utils.IExternalizable'));}catch(_){progIsExt=false;}}
        const isExt=!array&&Boolean(progIsExt||value?.[externalizableBrand]===true||ctor?.[externalizableBrand]===true);
        if(isExt){
          if(!alias)throw new TypeError('IExternalizable serialization requires registerClassAlias for its class.');
          if(typeof value.writeExternal!=='function')throw new TypeError(`IExternalizable class ${alias} must implement writeExternal(output).`);
          const tempBa=new binary.ByteArray();
          tempBa.objectEncoding=graphOptions.encoding??3;
          if(graphOptions.endian)tempBa.endian=graphOptions.endian;
          try{
            value.writeExternal(tempBa);
            const data=binary.toBytes(tempBa);
            bytes+=data.length;
            if(bytes>MAX_BYTES)throw new RangeError('AMF strings/bytes exceed 16 MiB.');
            nodes[index]={type:'externalizable',class_name:text(alias),base64:base64(data)};
            return {type:'ref',value:index};
          }finally{
            try{binary.discardByteArrays?.([tempBa]);}catch(_){}
          }
        }

        if(Object.getOwnPropertySymbols(value).length)throw new TypeError('AMF symbol-key/native container serialization is not implemented.');
        const publicKeys=!array?prog?.jsonKeys?.(value):null;
        if(!array&&publicKeys==null&&alias==null&&!plain(value))throw new TypeError('AMF serialization of this native/class object is unsupported; use a plain data record.');
        function read(key){
          if(publicKeys!=null)return value[key];
          const descriptor=Object.getOwnPropertyDescriptor(value,key);
          if(descriptor){
            if(!Object.hasOwn(descriptor,'value')){
              if(alias!=null&&typeof descriptor.get==='function')return value[key];
              throw new TypeError('AMF ordinary records require data properties; accessors are unsupported.');
            }
            return descriptor.value;
          }
          if(alias!=null){
            let proto=Object.getPrototypeOf(value);
            while(proto&&proto!==Object.prototype&&Object.getPrototypeOf(proto)!==null){
              const pd=Object.getOwnPropertyDescriptor(proto,key);
              if(pd&&typeof pd.get==='function'&&typeof pd.set==='function')return value[key];
              proto=Object.getPrototypeOf(proto);
            }
          }
          return undefined;
        }
        const entries=[],dense=[];
        if(array){if(value.length>MAX_VALUES)throw new RangeError('AMF array exceeds 100000 entries.');for(let i=0;i<value.length&&Object.hasOwn(value,String(i));i++)dense.push(visit(read(String(i)),depth+1));}
        const useDynWriter=dynWriter&&typeof dynWriter.writeDynamicProperties==='function'&&publicKeys==null&&!(depth===0&&graphOptions.ignoreRootDynamicWriter);
        if(useDynWriter){
          const output=new DynamicPropertyOutput();
          dynWriter.writeDynamicProperties(value,output);
          const customMap=dynamicOutputs.get(output);
          if(customMap.size>MAX_VALUES)throw new RangeError('AMF object has too many properties.');
          for(const[key,propVal]of customMap.entries()){
            if(array&&/^(?:0|[1-9][0-9]*)$/.test(key)&&Number(key)<dense.length)continue;
            entries.push([text(key),visit(propVal,depth+1)]);
          }
        }else{
          let keys=publicKeys??Object.keys(value);
          if(publicKeys==null&&alias!=null){
            const keySet=new Set(keys);
            let proto=Object.getPrototypeOf(value);
            while(proto&&proto!==Object.prototype&&Object.getPrototypeOf(proto)!==null){
              for(const k of Object.getOwnPropertyNames(proto)){
                if(k==='constructor'||k==='__proto__'||keySet.has(k))continue;
                const pd=Object.getOwnPropertyDescriptor(proto,k);
                if(pd&&typeof pd.get==='function'&&typeof pd.set==='function'){keySet.add(k);keys.push(k);}
              }
              proto=Object.getPrototypeOf(proto);
            }
          }
          if(keys.length>MAX_VALUES)throw new RangeError('AMF object has too many properties.');
          for(const key of keys){if(array&&/^(?:0|[1-9][0-9]*)$/.test(key)&&Number(key)<dense.length)continue;entries.push([text(key),visit(read(key),depth+1)]);}
        }
        nodes[index]=array?{type:'array',dense,entries,amf0_length:value.length}:(alias?{type:'object',class_name:text(alias),entries}:{type:'object',entries});return {type:'ref',value:index};
      }
      return {root:visit(value,0),nodes};
    }

    function makeProgramVector(prog,elemSpec,len){
      const lookup=qname=>prog?.resolveClass?.(qname)||prog?.type?.(qname)||null;
      if(prog){
        try{
          const VecCtor=lookup(`Vector.<${elemSpec}>`);
          if(typeof VecCtor==='function')return new VecCtor(len,false);
        }catch(_){}
        if(elemSpec!=='Object'&&elemSpec!=='*'){
          try{
            const VecObj=lookup('Vector.<Object>');
            if(typeof VecObj==='function')return new VecObj(len,false);
          }catch(_){}
        }
      }
      const aliasCtor=(elemSpec&&!['Object','*','int','uint','Number','String','Boolean'].includes(elemSpec))?aliasToClass.get(elemSpec)||null:null;
      return createVector(elemSpec||'Object',new Array(len),false,aliasCtor);
    }

    function makeProgramDictionary(prog,weakKeys){
      if(prog){
        try{
          const DictCtor=prog?.resolveClass?.('flash.utils.Dictionary')||prog?.type?.('flash.utils.Dictionary');
          if(typeof DictCtor==='function')return new DictCtor(Boolean(weakKeys));
        }catch(_){}
      }
      return createDictionary(Boolean(weakKeys));
    }

    function materialize(graph,requirements={},extInstances=[]){
      if(!graph||!Array.isArray(graph.nodes)||graph.nodes.length>MAX_VALUES)throw new TypeError('Malformed AMF codec graph.');
      const nodes=graph.nodes,values=new Array(nodes.length),byteIndices=[],byteInputs=[],aliased=new Set(),prog=options.getProgram?.();let visits=0;
      function checked(value){if(++visits>MAX_VALUES)throw new RangeError('Decoded AMF graph exceeds 100000 values.');if(!value||typeof value.type!=='string')throw new TypeError('Malformed AMF value.');switch(value.type){case'null':case'undefined':break;case'bool':if(typeof value.value!=='boolean')throw new TypeError('Malformed AMF Boolean.');break;case'number':fromNumber(value.value);break;case'string':if(typeof value.value!=='string')throw new TypeError('Malformed AMF string.');break;case'ref':if(!Number.isInteger(value.value)||value.value<0||value.value>=nodes.length)throw new TypeError('Invalid AMF reference.');break;default:throw new TypeError('Unsupported AMF graph value.');}}
      checked(graph.root);
      const validNodeTypes=new Set(['object','array','date','bytes','xml','xml_document','vector_int','vector_uint','vector_number','vector_object','dictionary','externalizable']);
      for(let i=0;i<nodes.length;i++){
        const node=nodes[i];if(!node||!validNodeTypes.has(node.type))throw new TypeError('Unsupported AMF node.');
        if(node.type==='date'){values[i]=new Date(fromNumber(node.milliseconds));continue;}
        if(node.type==='bytes'){byteIndices.push(i);byteInputs.push(unbase64(node.base64));continue;}
        if(node.type==='xml'||node.type==='xml_document'){if(typeof node.text!=='string')throw new TypeError('Malformed AMF XML text.');continue;}
        if(node.type==='vector_int'||node.type==='vector_uint'||node.type==='vector_number'){
          if(!Array.isArray(node.values)||node.values.length>MAX_VALUES)throw new TypeError('Malformed AMF Vector.');
          if(node.type==='vector_number')for(const s of node.values)fromNumber(s);
          continue;
        }
        if(node.type==='vector_object'){
          if(!Array.isArray(node.values)||node.values.length>MAX_VALUES)throw new TypeError('Malformed AMF Vector.');
          for(const v of node.values)checked(v);
          continue;
        }
        if(node.type==='dictionary'){
          if(!Array.isArray(node.entries)||node.entries.length>MAX_VALUES)throw new TypeError('Malformed AMF Dictionary.');
          for(const pair of node.entries){if(!Array.isArray(pair)||pair.length!==2)throw new TypeError('Malformed AMF Dictionary entry.');checked(pair[0]);checked(pair[1]);}
          continue;
        }
        if(node.type==='externalizable'){
          if(typeof node.class_name!=='string'||!node.class_name)throw new TypeError('Malformed AMF externalizable class name.');
          unbase64(node.base64||'');
          continue;
        }
        if(!Array.isArray(node.entries)||node.entries.length>MAX_VALUES)throw new TypeError('Malformed AMF object entries.');
        const keys=new Set();
        if(node.type==='array'){if(!Array.isArray(node.dense)||node.dense.length>MAX_VALUES)throw new TypeError('Malformed AMF dense array.');if(node.amf0_length!==undefined&&(!Number.isInteger(node.amf0_length)||node.amf0_length<0||node.amf0_length>MAX_VALUES))throw new RangeError('Malformed AMF0 array length hint.');for(const value of node.dense)checked(value);values[i]=new Array(node.dense.length);}
        else if(node.class_name!==undefined&&node.class_name!==null&&typeof node.class_name!=='string')throw new TypeError('Malformed AMF class alias.');
        for(const entry of node.entries){if(!Array.isArray(entry)||entry.length!==2||typeof entry[0]!=='string'||keys.has(entry[0]))throw new TypeError('Duplicate/malformed AMF object key.');keys.add(entry[0]);if(node.type==='array'&&(entry[0]==='length'||(/^(?:0|[1-9][0-9]*)$/.test(entry[0])&&Number(entry[0])<node.dense.length)))throw new TypeError('AMF associative key conflicts with its dense array.');if(node.type==='array'&&/^(?:0|[1-9][0-9]*)$/.test(entry[0])&&Number(entry[0])>=MAX_VALUES&&Number(entry[0])<4294967295)throw new RangeError('AMF associative array index exceeds 100000 entries.');checked(entry[1]);}
      }

      const xmlValues=[];let byteValues=[];
      try{
        for(let i=0;i<nodes.length;i++){
          const node=nodes[i];
          if(node.type==='object'){
            const Ctor=node.class_name?aliasToClass.get(node.class_name):null;
            if(Ctor){values[i]=prog?.isClass?.(Ctor)?prog.constructClass(Ctor,[]):new Ctor();aliased.add(i);}else values[i]={};
          }else if(node.type==='xml'||node.type==='xml_document'){
            const xml=options.getXML?.(),Ctor=node.type==='xml'?xml?.XML:(requirements.legacyXML?xml?.LegacyXML:xml?.XMLDocument);
            if(typeof Ctor!=='function')throw new Error('AMF XML decoding requires the publication XML service. Re-export this project.');
            if(node.type==='xml')values[i]=new Ctor(node.text);
            else values[i]=new Ctor();
            xmlValues.push([xml,values[i]]);
            if(node.type==='xml_document')Reflect.apply(Ctor.prototype.parseXML,values[i],[node.text]);
          }else if(node.type==='vector_int'){
            const vec=makeProgramVector(prog,'int',node.values.length);
            for(let at=0;at<node.values.length;at++)vec[at]=node.values[at]|0;
            if(node.fixed)vec.fixed=true;
            values[i]=vec;
          }else if(node.type==='vector_uint'){
            const vec=makeProgramVector(prog,'uint',node.values.length);
            for(let at=0;at<node.values.length;at++)vec[at]=node.values[at]>>>0;
            if(node.fixed)vec.fixed=true;
            values[i]=vec;
          }else if(node.type==='vector_number'){
            const vec=makeProgramVector(prog,'Number',node.values.length);
            for(let at=0;at<node.values.length;at++)vec[at]=fromNumber(node.values[at]);
            if(node.fixed)vec.fixed=true;
            values[i]=vec;
          }else if(node.type==='vector_object'){
            const rawType=String(node.type_name||'');
            let elemSpec='Object';
            if(rawType==='*')elemSpec='*';
            else if(rawType===''||rawType==='Object')elemSpec='Object';
            else if(['String','Boolean','Date','RegExp','XML','Array','Function','Class','int','uint','Number'].includes(rawType)||rawType.startsWith('Vector.<'))elemSpec=rawType;
            else{
              const Ctor=aliasToClass.get(rawType);
              const qn=Ctor&&prog?.className?.(Ctor);
              elemSpec=qn?qn.replace('::','.'):(Ctor?rawType:'Object');
            }
            values[i]=makeProgramVector(prog,elemSpec,node.values.length);
          }else if(node.type==='dictionary'){
            values[i]=makeProgramDictionary(prog,node.weak_keys);
          }else if(node.type==='externalizable'){
            if(Number.isInteger(node.ext_token)&&node.ext_token>=0&&node.ext_token<extInstances.length){
              values[i]=extInstances[node.ext_token];
            }else{
              const Ctor=getClassByAlias(node.class_name);
              const inst=prog?.isClass?.(Ctor)?prog.constructClass(Ctor,[]):new Ctor();
              if(typeof inst?.readExternal!=='function')throw new TypeError(`Class ${node.class_name} does not implement IExternalizable.readExternal.`);
              const tempBa=binary.fromBytes(unbase64(node.base64||''));
              tempBa.position=0;tempBa.objectEncoding=requirements.encoding??3;
              if(requirements.endian)tempBa.endian=requirements.endian;
              try{inst.readExternal(tempBa);}finally{try{binary.discardByteArrays?.([tempBa]);}catch(_){}}
              values[i]=inst;
            }
          }
        }
        byteValues=binary.fromByteArrays(byteInputs);for(let i=0;i<byteIndices.length;i++)values[byteIndices[i]]=byteValues[i];
        function resolve(value){switch(value.type){case'null':return null;case'undefined':return undefined;case'bool':case'string':return value.value;case'number':return fromNumber(value.value);case'ref':return values[value.value];}}
        for(let i=0;i<nodes.length;i++){
          const node=nodes[i];
          if(node.type==='array')for(let at=0;at<node.dense.length;at++)values[i][at]=resolve(node.dense[at]);
          if(node.type==='array'||node.type==='object'){
            if(aliased.has(i)){
              for(const[name,value]of node.entries){
                const resolved=resolve(value),slot=prog?.ref?.(values[i],name,null);
                if(slot)slot.set(resolved);else values[i][name]=resolved;
              }
            }else{
              for(const[name,value]of node.entries)Object.defineProperty(values[i],name,{value:resolve(value),enumerable:true,writable:true,configurable:true});
            }
          }else if(node.type==='vector_object'){
            const vec=values[i];
            for(let at=0;at<node.values.length;at++)vec[at]=resolve(node.values[at]);
            if(node.fixed)vec.fixed=true;
          }else if(node.type==='dictionary'){
            const dict=values[i];
            for(const [kVal,vVal] of node.entries)dictionarySetEntry(dict,resolve(kVal),resolve(vVal));
          }
        }
        const value=resolve(graph.root);requirements.acceptValue?.(value);return value;
      }catch(error){
        try{binary.discardByteArrays?.(byteValues);}catch(_){}
        for(let i=xmlValues.length-1;i>=0;i--)try{xmlValues[i][0].disposeXML(xmlValues[i][1]);}catch(_){}
        throw error;
      }
    }

    function codecCall(call,getCaptured){
      try{return call();}catch(error){
        const captured=getCaptured?.();
        if(captured)throw captured;
        if(error instanceof Error||Object.prototype.toString.call(error)==='[object Error]')throw error;
        throw new Error(typeof error==='string'?error:'Rust AMF codec failed.',{cause:error});
      }
    }

    function makeExtReader(mode,requirements,extInstances,setCaptured){
      const prog=options.getProgram?.();
      return (className,slice)=>{
        try{
          const Ctor=getClassByAlias(className);
          const inst=prog?.isClass?.(Ctor)?prog.constructClass(Ctor,[]):new Ctor();
          if(typeof inst?.readExternal!=='function')throw new TypeError(`Class ${className} does not implement IExternalizable.readExternal.`);
          const raw=Uint8Array.from(slice);
          const tempBa=binary.fromBytes(raw);
          tempBa.position=0;
          tempBa.objectEncoding=mode;
          if(requirements?.endian)tempBa.endian=requirements.endian;
          try{
            inst.readExternal(tempBa);
            const consumed=tempBa.position;
            const token=extInstances.length;
            extInstances.push(inst);
            return JSON.stringify({consumed,token});
          }finally{
            try{binary.discardByteArrays?.([tempBa]);}catch(_){}
          }
        }catch(err){
          setCaptured(err);
          throw err;
        }
      };
    }

    function encode(value,encoding=3,graphOptions={}){const mode=format(encoding),source=JSON.stringify(graph(value,{...graphOptions,encoding:mode}));if(source.length>64*1024*1024)throw new RangeError('AMF graph envelope exceeds 64 MiB.');return codecCall(()=>options.encode(mode,source));}
    function decode(bytes,encoding=3,requirements={}){
      if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1||bytes.byteLength>MAX_BYTES)throw new RangeError('AMF decode requires a byte view no larger than 16 MiB.');
      const mode=format(encoding),extInstances=[];
      let capturedError=null;
      const nativeDecodeExt=options.decodeExt||global.NeoFlashWasm?.amf_decode_ext;
      const rawJson=typeof nativeDecodeExt==='function'
        ?codecCall(()=>nativeDecodeExt(mode,bytes,makeExtReader(mode,requirements,extInstances,e=>{capturedError=e;})),()=>capturedError)
        :codecCall(()=>options.decode(mode,bytes));
      const result=JSON.parse(rawJson);
      if(!Number.isInteger(result.consumed)||result.consumed<1||result.consumed>bytes.byteLength)throw new TypeError('AMF codec returned an invalid consumed length.');
      if(requirements.complete&&result.consumed!==bytes.byteLength)throw new TypeError('Saved AMF payload contains trailing data.');
      if(requirements.record&&(result.graph?.root?.type!=='ref'||result.graph.nodes?.[result.graph.root.value]?.type!=='object'))throw new TypeError('Saved SharedObject root must be an ordinary data record.');
      return {value:materialize(result.graph,{...requirements,encoding:mode},extInstances),consumed:result.consumed};
    }
    function solName(name){
      if(typeof name!=='string'||!name)throw new TypeError('SOL name requires a non-empty String.');
      for(let i=0;i<name.length;i++){
        const c=name.charCodeAt(i);
        if(c>=0xd800&&c<=0xdbff){const low=name.charCodeAt(++i);if(!(low>=0xdc00&&low<=0xdfff))throw new TypeError('SOL names require complete Unicode characters.');}
        else if(c>=0xdc00&&c<=0xdfff)throw new TypeError('SOL names require complete Unicode characters.');
      }
      if(utf8.encode(name).length>65535)throw new RangeError('SOL name exceeds 65535 UTF-8 bytes.');
      return name;
    }
    function encodeSol(name,value,encoding=3,graphOptions={}){
      solName(name);
      const nativeEncodeSol=options.encodeSol||global.NeoFlashWasm?.sol_encode;
      if(typeof nativeEncodeSol!=='function')throw new Error('Native SOL encoding is unavailable. Rebuild the shared WASM runtime and re-export this project.');
      const mode=format(encoding),source=JSON.stringify(graph(value,{...graphOptions,encoding:mode}));
      if(source.length>64*1024*1024)throw new RangeError('AMF graph envelope exceeds 64 MiB.');
      const result=codecCall(()=>nativeEncodeSol(mode,name,source));
      if(!ArrayBuffer.isView(result)||result.BYTES_PER_ELEMENT!==1||result.byteLength>MAX_BYTES)throw new RangeError('SOL encoding exceeds the 16 MiB file limit.');
      return result;
    }
    function decodeSol(bytes,requirements={}){
      if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1||bytes.byteLength>MAX_BYTES)throw new RangeError('SOL decode requires a byte view no larger than 16 MiB.');
      const u8=new Uint8Array(bytes.buffer,bytes.byteOffset,bytes.byteLength);
      const nativeDecodeSol=options.decodeSol||global.NeoFlashWasm?.sol_decode;
      const nativeDecodeSolExt=options.decodeSolExt||global.NeoFlashWasm?.sol_decode_ext;
      if(typeof nativeDecodeSol!=='function'&&typeof nativeDecodeSolExt!=='function')throw new Error('Native SOL decoding is unavailable. Rebuild the shared WASM runtime and re-export this project.');
      const extInstances=[];
      let capturedError=null;
      const rawJson=(typeof nativeDecodeSolExt==='function'&&options.decodeSol!==undefined)
        ?codecCall(()=>nativeDecodeSolExt(u8,makeExtReader(3,requirements,extInstances,e=>{capturedError=e;})),()=>capturedError)
        :codecCall(()=>nativeDecodeSol(u8));
      const parsed=JSON.parse(rawJson),name=solName(parsed.name),mode=format(parsed.format),decoded=parsed.decoded;
      if(!Number.isInteger(decoded?.consumed)||decoded.consumed!==u8.length)throw new TypeError('SOL file contains trailing or incomplete data.');
      if(decoded.graph?.root?.type!=='ref'||decoded.graph.nodes?.[decoded.graph.root.value]?.type!=='object')throw new TypeError('SOL root must be an ordinary data record.');
      requirements.validateName?.(name);
      const mat=materialize(decoded.graph,{...requirements,encoding:mode,acceptValue:requirements.accept?value=>requirements.accept({name,encoding:mode,value}):undefined},extInstances);
      return {name,encoding:mode,objectEncoding:mode,value:mat,data:mat,consumed:decoded.consumed};
    }
    return Object.freeze({encode,decode,encodeSol,decodeSol,registerClassAlias,getClassByAlias,setDynamicPropertyWriterGetter,DynamicPropertyOutput,createVector,createDictionary,externalizableBrand,reset,base64,unbase64});
  }
  const api=Object.freeze({create,DynamicPropertyOutput,createVector,createDictionary,externalizableBrand,base64,unbase64});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAMF=api;
})(globalThis);
