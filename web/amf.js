/* Original JavaScript value/graph bridge for the shared Rust AMF0/AMF3 codec.
 * No JSON substitution for AMF: JSON is only the lossless tagged WASM ABI.
 * Primitive numbers, arrays (dense + named own entries), records, Date and
 * ByteArray preserve cycles/references. Source-class public fields/getters can
 * flatten to records through program.jsonKeys; class aliases/native objects,
 * functions, symbol keys, dictionary/vector/XML and externalizable objects are
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
  function create(options={}){
    const {binary}=options;
    if(typeof options.encode!=='function'||typeof options.decode!=='function'||!binary?.fromByteArrays)throw new TypeError('AMF requires the shared Rust codec and ByteArray service.');
    function graph(value){
      const seen=new Map(),nodes=[];let visits=0,bytes=0;
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
        let milliseconds;try{milliseconds=Reflect.apply(dateTime,value,[]);}catch(_){milliseconds=undefined;}
        if(milliseconds!==undefined){nodes[index]={type:'date',milliseconds:number(milliseconds)};return {type:'ref',value:index};}
        if(Object.getOwnPropertySymbols(value).length)throw new TypeError('AMF symbol-key/native container serialization is not implemented.');
        const array=Array.isArray(value),publicKeys=!array?options.getProgram?.()?.jsonKeys?.(value):null;
        if(!array&&publicKeys==null&&!plain(value))throw new TypeError('AMF serialization of this native/class object is unsupported; use a plain data record.');
        const keys=publicKeys??Object.keys(value);if(keys.length>MAX_VALUES)throw new RangeError('AMF object has too many properties.');
        function read(key){const descriptor=Object.getOwnPropertyDescriptor(value,key);if(publicKeys!=null)return value[key];if(!descriptor)return undefined;if(!Object.hasOwn(descriptor,'value'))throw new TypeError('AMF ordinary records require data properties; accessors are unsupported.');return descriptor.value;}
        const entries=[],dense=[];
        if(array){if(value.length>MAX_VALUES)throw new RangeError('AMF array exceeds 100000 entries.');for(let i=0;i<value.length;i++)dense.push(visit(read(String(i)),depth+1));}
        for(const key of keys){if(array&&/^(?:0|[1-9][0-9]*)$/.test(key)&&Number(key)<value.length)continue;entries.push([text(key),visit(read(key),depth+1)]);}
        nodes[index]=array?{type:'array',dense,entries}:{type:'object',entries};return {type:'ref',value:index};
      }
      return {root:visit(value,0),nodes};
    }
    function materialize(graph){
      if(!graph||!Array.isArray(graph.nodes)||graph.nodes.length>MAX_VALUES)throw new TypeError('Malformed AMF codec graph.');
      const nodes=graph.nodes,values=new Array(nodes.length),byteIndices=[],byteInputs=[];let visits=0;
      function checked(value){if(++visits>MAX_VALUES)throw new RangeError('Decoded AMF graph exceeds 100000 values.');if(!value||typeof value.type!=='string')throw new TypeError('Malformed AMF value.');switch(value.type){case'null':case'undefined':break;case'bool':if(typeof value.value!=='boolean')throw new TypeError('Malformed AMF Boolean.');break;case'number':fromNumber(value.value);break;case'string':if(typeof value.value!=='string')throw new TypeError('Malformed AMF string.');break;case'ref':if(!Number.isInteger(value.value)||value.value<0||value.value>=nodes.length)throw new TypeError('Invalid AMF reference.');break;default:throw new TypeError('Unsupported AMF graph value.');}}
      checked(graph.root);
      // Validate before allocating any ByteArray. A multi-buffer transaction in
      // the binary service rolls back all buffers if the session budget fails.
      for(let i=0;i<nodes.length;i++){
        const node=nodes[i];if(!node||!['object','array','date','bytes'].includes(node.type))throw new TypeError('Unsupported AMF node.');
        if(node.type==='date'){values[i]=new Date(fromNumber(node.milliseconds));continue;}
        if(node.type==='bytes'){byteIndices.push(i);byteInputs.push(unbase64(node.base64));continue;}
        if(!Array.isArray(node.entries)||node.entries.length>MAX_VALUES)throw new TypeError('Malformed AMF object entries.');
        const keys=new Set();
        if(node.type==='array'){if(!Array.isArray(node.dense)||node.dense.length>MAX_VALUES)throw new TypeError('Malformed AMF dense array.');for(const value of node.dense)checked(value);values[i]=new Array(node.dense.length);}else values[i]={};
        for(const entry of node.entries){if(!Array.isArray(entry)||entry.length!==2||typeof entry[0]!=='string'||keys.has(entry[0]))throw new TypeError('Duplicate/malformed AMF object key.');keys.add(entry[0]);if(node.type==='array'&&(entry[0]==='length'||(/^(?:0|[1-9][0-9]*)$/.test(entry[0])&&Number(entry[0])<node.dense.length)))throw new TypeError('AMF associative key conflicts with its dense array.');if(node.type==='array'&&/^(?:0|[1-9][0-9]*)$/.test(entry[0])&&Number(entry[0])>=MAX_VALUES&&Number(entry[0])<4294967295)throw new RangeError('AMF associative array index exceeds 100000 entries.');checked(entry[1]);}
      }
      const byteValues=binary.fromByteArrays(byteInputs);for(let i=0;i<byteIndices.length;i++)values[byteIndices[i]]=byteValues[i];
      function resolve(value){switch(value.type){case'null':return null;case'undefined':return undefined;case'bool':case'string':return value.value;case'number':return fromNumber(value.value);case'ref':return values[value.value];}}
      for(let i=0;i<nodes.length;i++){
        const node=nodes[i];if(node.type==='array')for(let at=0;at<node.dense.length;at++)values[i][at]=resolve(node.dense[at]);
        if(node.type==='array'||node.type==='object')for(const[name,value]of node.entries)Object.defineProperty(values[i],name,{value:resolve(value),enumerable:true,writable:true,configurable:true});
      }
      return resolve(graph.root);
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
    function encode(value,encoding=3){const source=JSON.stringify(graph(value));if(source.length>64*1024*1024)throw new RangeError('AMF graph envelope exceeds 64 MiB.');const mode=format(encoding);return codecCall(()=>options.encode(mode,source));}
    function decode(bytes,encoding=3,requirements={}){
      if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1||bytes.byteLength>MAX_BYTES)throw new RangeError('AMF decode requires a byte view no larger than 16 MiB.');
      const mode=format(encoding),result=JSON.parse(codecCall(()=>options.decode(mode,bytes)));if(!Number.isInteger(result.consumed)||result.consumed<1||result.consumed>bytes.byteLength)throw new TypeError('AMF codec returned an invalid consumed length.');
      if(requirements.complete&&result.consumed!==bytes.byteLength)throw new TypeError('Saved AMF payload contains trailing data.');
      if(requirements.record&&(result.graph?.root?.type!=='ref'||result.graph.nodes?.[result.graph.root.value]?.type!=='object'))throw new TypeError('Saved SharedObject root must be an ordinary data record.');
      return {value:materialize(result.graph),consumed:result.consumed};
    }
    return Object.freeze({encode,decode,base64,unbase64});
  }
  const api=Object.freeze({create,base64,unbase64});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAMF=api;
})(globalThis);
