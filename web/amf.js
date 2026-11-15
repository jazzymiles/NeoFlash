/* Original JavaScript value/graph bridge for the shared Rust AMF0/AMF3 codec.
 * No JSON substitution for AMF: JSON is only the lossless tagged WASM ABI.
 * Primitive numbers, arrays (dense + named own entries), records, Date and
 * ByteArray preserve cycles/references. Source-class public fields/getters can
 * flatten to records through program.jsonKeys; registered class aliases retain
 * their type. XML/XMLDocument use the publication XML service. Functions, symbol keys,
 * dictionary/vector/XMLList and externalizable objects are
 * explicit boundaries. Ordinary record accessors are not executed.
 */
(function(global){
  'use strict';
  const MAX_BYTES=16*1024*1024,MAX_VALUES=100000,MAX_DEPTH=128;
  const utf8=new TextEncoder(),dateTime=Date.prototype.getTime;
  function format(value){if(value!==0&&value!==3)throw new RangeError('ObjectEncoding must be AMF0 (0) or AMF3 (3).');return value;}
  function number(value){return Object.is(value,-0)?'-0':String(value);}
  function fromNumber(value){if(typeof value!=='string')throw new TypeError('Invalid AMF numeric graph value.');if(value==='NaN')return NaN;if(value==='Infinity')return Infinity;if(value==='-Infinity')return -Infinity;if(value==='-0')return -0;const result=Number(value);if(!Number.isFinite(result)||value.trim()==='')throw new TypeError('Invalid AMF numeric graph value.');return result;}
  function base64(bytes){let text='';for(let at=0;at<bytes.length;at+=8192)text+=String.fromCharCode(...bytes.subarray(at,at+8192));return global.btoa(text);}
  function unbase64(text){
    if(typeof text!=='string'||text.length>Math.ceil(MAX_BYTES/3)*4||text.length%4||!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(text))throw new TypeError('Invalid bounded AMF byte payload.');
    const value=global.atob(text);if(value.length>MAX_BYTES)throw new RangeError('AMF byte payload exceeds 16 MiB.');return Uint8Array.from(value,char=>char.charCodeAt(0));
  }
  function plain(value){const p=Object.getPrototypeOf(value);if(p===null||p===Object.prototype)return true;const ctor=Object.getOwnPropertyDescriptor(p,'constructor')?.value;return Object.getPrototypeOf(p)===null&&typeof ctor==='function'&&ctor.name==='Object'&&ctor.prototype===p;}
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
        bytes+=utf8.encode(value).length;if(bytes>MAX_BYTES)throw new RangeError('AMF strings/bytes exceed the 16 MiB graph budget.');return value;
      }
      function visit(value,depth){
        if(++visits>MAX_VALUES||depth>MAX_DEPTH)throw new RangeError('AMF graph exceeds 100000 values or 128 levels.');
        if(value===undefined||value===null)return {type:value===null?'null':'undefined'};
        if(typeof value==='boolean')return {type:'bool',value};
        if(typeof value==='number')return {type:'number',value:number(value)};
        if(typeof value==='string')return {type:'string',value:text(value)};
        if(typeof value!=='object')throw new TypeError('AMF cannot serialize functions, Symbols or BigInts in this profile.');
        if(seen.has(value))return {type:'ref',value:seen.get(value)};
        if(options.isUnsupported?.(value))throw new TypeError('AMF display/native objects require an explicit data record; class aliases are not implemented.');
        const index=nodes.length;if(index>=MAX_VALUES)throw new RangeError('AMF graph exceeds 100000 nodes.');seen.set(value,index);nodes.push(null);
        if(binary.ByteArray.__nfIs(value)){const data=binary.toBytes(value);bytes+=data.length;if(bytes>MAX_BYTES)throw new RangeError('AMF strings/bytes exceed 16 MiB.');nodes[index]={type:'bytes',base64:base64(data)};return {type:'ref',value:index};}
        if(xml?.XML?.__nfIs(value)){nodes[index]={type:'xml',text:text(xml.method(value,'toXMLString',true)())};return {type:'ref',value:index};}
        if(xml?.XMLDocument?.__nfIs(value)||xml?.LegacyXML?.__nfIs(value)){nodes[index]={type:'xml_document',text:text(Reflect.apply(xml.XMLNode.prototype.toString,value,[]))};return {type:'ref',value:index};}
        if(xml?.XMLList?.__nfIs(value))throw new TypeError('AMF XMLList serialization is not supported; use an XML value.');
        let milliseconds;try{milliseconds=Reflect.apply(dateTime,value,[]);}catch(_){milliseconds=undefined;}
        if(milliseconds!==undefined){nodes[index]={type:'date',milliseconds:number(milliseconds)};return {type:'ref',value:index};}
        if(Object.getOwnPropertySymbols(value).length)throw new TypeError('AMF symbol-key/native container serialization is not implemented.');
        const array=Array.isArray(value),publicKeys=!array?prog?.jsonKeys?.(value):null;
        const instanceName=!array?prog?.instanceClassName?.(value):null;
        const ctor=!array?Object.getOwnPropertyDescriptor(Object.getPrototypeOf(value)||{},'constructor')?.value:null;
        const alias=!array?((instanceName&&classNameToAlias.get(instanceName))||classToAlias.get(ctor)||null):null;
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
    function materialize(graph,requirements={}){
      if(!graph||!Array.isArray(graph.nodes)||graph.nodes.length>MAX_VALUES)throw new TypeError('Malformed AMF codec graph.');
      const nodes=graph.nodes,values=new Array(nodes.length),byteIndices=[],byteInputs=[],aliased=new Set(),prog=options.getProgram?.();let visits=0;
      function checked(value){if(++visits>MAX_VALUES)throw new RangeError('Decoded AMF graph exceeds 100000 values.');if(!value||typeof value.type!=='string')throw new TypeError('Malformed AMF value.');switch(value.type){case'null':case'undefined':break;case'bool':if(typeof value.value!=='boolean')throw new TypeError('Malformed AMF Boolean.');break;case'number':fromNumber(value.value);break;case'string':if(typeof value.value!=='string')throw new TypeError('Malformed AMF string.');break;case'ref':if(!Number.isInteger(value.value)||value.value<0||value.value>=nodes.length)throw new TypeError('Invalid AMF reference.');break;default:throw new TypeError('Unsupported AMF graph value.');}}
      checked(graph.root);
      // Validate before allocating any ByteArray. A multi-buffer transaction in
      // the binary service rolls back all buffers if the session budget fails.
      for(let i=0;i<nodes.length;i++){
        const node=nodes[i];if(!node||!['object','array','date','bytes','xml','xml_document'].includes(node.type))throw new TypeError('Unsupported AMF node.');
        if(node.type==='date'){values[i]=new Date(fromNumber(node.milliseconds));continue;}
        if(node.type==='bytes'){byteIndices.push(i);byteInputs.push(unbase64(node.base64));continue;}
        if(node.type==='xml'||node.type==='xml_document'){if(typeof node.text!=='string')throw new TypeError('Malformed AMF XML text.');continue;}
        if(!Array.isArray(node.entries)||node.entries.length>MAX_VALUES)throw new TypeError('Malformed AMF object entries.');
        const keys=new Set();
        if(node.type==='array'){if(!Array.isArray(node.dense)||node.dense.length>MAX_VALUES)throw new TypeError('Malformed AMF dense array.');if(node.amf0_length!==undefined&&(!Number.isInteger(node.amf0_length)||node.amf0_length<0||node.amf0_length>MAX_VALUES))throw new RangeError('Malformed AMF0 array length hint.');for(const value of node.dense)checked(value);values[i]=new Array(node.dense.length);}
        else{
          if(node.class_name!==undefined&&node.class_name!==null&&typeof node.class_name!=='string')throw new TypeError('Malformed AMF class alias.');
        }
        for(const entry of node.entries){if(!Array.isArray(entry)||entry.length!==2||typeof entry[0]!=='string'||keys.has(entry[0]))throw new TypeError('Duplicate/malformed AMF object key.');keys.add(entry[0]);if(node.type==='array'&&(entry[0]==='length'||(/^(?:0|[1-9][0-9]*)$/.test(entry[0])&&Number(entry[0])<node.dense.length)))throw new TypeError('AMF associative key conflicts with its dense array.');if(node.type==='array'&&/^(?:0|[1-9][0-9]*)$/.test(entry[0])&&Number(entry[0])>=MAX_VALUES&&Number(entry[0])<4294967295)throw new RangeError('AMF associative array index exceeds 100000 entries.');checked(entry[1]);}
      }
      // No authored constructor runs until every native graph entry validates.
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
          }
        }
        byteValues=binary.fromByteArrays(byteInputs);for(let i=0;i<byteIndices.length;i++)values[byteIndices[i]]=byteValues[i];
        function resolve(value){switch(value.type){case'null':return null;case'undefined':return undefined;case'bool':case'string':return value.value;case'number':return fromNumber(value.value);case'ref':return values[value.value];}}
        for(let i=0;i<nodes.length;i++){
          const node=nodes[i];if(node.type==='array')for(let at=0;at<node.dense.length;at++)values[i][at]=resolve(node.dense[at]);
          if(node.type==='array'||node.type==='object'){
            if(aliased.has(i)){
              for(const[name,value]of node.entries){
                const resolved=resolve(value),slot=prog?.ref?.(values[i],name,null);
                if(slot)slot.set(resolved);else values[i][name]=resolved;
              }
            }else{
              for(const[name,value]of node.entries)Object.defineProperty(values[i],name,{value:resolve(value),enumerable:true,writable:true,configurable:true});
            }
          }
        }
        const value=resolve(graph.root);requirements.acceptValue?.(value);return value;
      }catch(error){
        // Cleanup is secondary to the native/authored failure. All tracked values
        // are newly decoded; services ignore records retired by a nested Restart.
        try{binary.discardByteArrays?.(byteValues);}catch(_){}
        for(let i=xmlValues.length-1;i>=0;i--)try{xmlValues[i][0].disposeXML(xmlValues[i][1]);}catch(_){}
        throw error;
      }
    }
    // wasm-bindgen exposes Rust codec diagnostics as strings. Only this native
    // call boundary converts them to Error; graph accessors/authored exceptions
    // stay outside it, and existing Error identities/subtypes remain intact.
    function codecCall(call){
      try{return call();}catch(error){
        if(error instanceof Error||Object.prototype.toString.call(error)==='[object Error]')throw error;
        throw new Error(typeof error==='string'?error:'Rust AMF codec failed.',{cause:error});
      }
    }
    function encode(value,encoding=3,graphOptions={}){const source=JSON.stringify(graph(value,graphOptions));if(source.length>64*1024*1024)throw new RangeError('AMF graph envelope exceeds 64 MiB.');const mode=format(encoding);return codecCall(()=>options.encode(mode,source));}
    function decode(bytes,encoding=3,requirements={}){
      if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1||bytes.byteLength>MAX_BYTES)throw new RangeError('AMF decode requires a byte view no larger than 16 MiB.');
      const mode=format(encoding),result=JSON.parse(codecCall(()=>options.decode(mode,bytes)));if(!Number.isInteger(result.consumed)||result.consumed<1||result.consumed>bytes.byteLength)throw new TypeError('AMF codec returned an invalid consumed length.');
      if(requirements.complete&&result.consumed!==bytes.byteLength)throw new TypeError('Saved AMF payload contains trailing data.');
      if(requirements.record&&(result.graph?.root?.type!=='ref'||result.graph.nodes?.[result.graph.root.value]?.type!=='object'))throw new TypeError('Saved SharedObject root must be an ordinary data record.');
      return {value:materialize(result.graph,requirements),consumed:result.consumed};
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
      const mode=format(encoding),source=JSON.stringify(graph(value,graphOptions));
      if(source.length>64*1024*1024)throw new RangeError('AMF graph envelope exceeds 64 MiB.');
      const result=codecCall(()=>nativeEncodeSol(mode,name,source));
      if(!ArrayBuffer.isView(result)||result.BYTES_PER_ELEMENT!==1||result.byteLength>MAX_BYTES)throw new RangeError('SOL encoding exceeds the 16 MiB file limit.');
      return result;
    }
    function decodeSol(bytes,requirements={}){
      if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1||bytes.byteLength>MAX_BYTES)throw new RangeError('SOL decode requires a byte view no larger than 16 MiB.');
      const u8=new Uint8Array(bytes.buffer,bytes.byteOffset,bytes.byteLength);
      const nativeDecodeSol=options.decodeSol||global.NeoFlashWasm?.sol_decode;
      if(typeof nativeDecodeSol!=='function')throw new Error('Native SOL decoding is unavailable. Rebuild the shared WASM runtime and re-export this project.');
      const parsed=JSON.parse(codecCall(()=>nativeDecodeSol(u8))),name=solName(parsed.name),mode=format(parsed.format),decoded=parsed.decoded;
      if(!Number.isInteger(decoded?.consumed)||decoded.consumed!==u8.length)throw new TypeError('SOL file contains trailing or incomplete data.');
      if(decoded.graph?.root?.type!=='ref'||decoded.graph.nodes?.[decoded.graph.root.value]?.type!=='object')throw new TypeError('SOL root must be an ordinary data record.');
      requirements.validateName?.(name);
      const mat=materialize(decoded.graph,{...requirements,acceptValue:requirements.accept?value=>requirements.accept({name,encoding:mode,value}):undefined});
      return {name,encoding:mode,objectEncoding:mode,value:mat,data:mat,consumed:decoded.consumed};
    }
    return Object.freeze({encode,decode,encodeSol,decodeSol,registerClassAlias,getClassByAlias,setDynamicPropertyWriterGetter,DynamicPropertyOutput,reset,base64,unbase64});
  }
  const api=Object.freeze({create,DynamicPropertyOutput,base64,unbase64});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAMF=api;
})(globalThis);
