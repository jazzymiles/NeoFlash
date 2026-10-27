/* Bounded AS3 binary buffers. Original implementation; no external IO.
 * Primary API contracts:
 * https://airsdk.dev/reference/actionscript/3.0/flash/utils/ByteArray.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/utils/Endian.html
 * Cursor clamping, UTF prefix/endian and UTF BOM/NUL behavior cross-checked with
 * Ruffle fe6cfc545405b888ebb478ce1c211d63147ca49b:
 * core/src/avm2/bytearray.rs and globals/flash/utils/byte_array.rs.
 * No upstream code is incorporated and no original-player comparison is claimed.
 *
 * create({maxBytes=16MiB,maxTotalBytes=64MiB,maxArrays=4096}) returns
 * {ByteArray,Endian,reset,dispose}. Limits include backing capacity; initialized
 * arrays remain budgeted until clear/reset/dispose, independently of browser GC.
 * A reset invalidates previous-session arrays and frees their backing storage.
 * Sizes/indices are nonnegative integers, numbers/Booleans/strings use primitive
 * inputs. Integer writes keep low bits; IEEE floats retain NaN/Infinity. No full
 * AS argument coercion or native errorID/EOFError class conformance is claimed.
 * Failed bounds/allocation/UTF validation preserves bytes, length and cursors.
 * writeBytes out-of-source ranges reject (the AIR docs describe clamping, while
 * the pinned implementation rejects); this is an explicit supported boundary.
 * UTF decoding is strict: malformed sequences and unpaired UTF-16 inputs reject.
 * readUTF/readUTFBytes strip UTF-8 BOM and stop their result at NUL, while consuming
 * the complete requested bytes. toString decodes all UTF-8 bytes, preserving NUL;
 * UTF-16 BOM/system code pages are unsupported. readBytes into itself is rejected
 * because its source/destination cursor aliases need a separate legacy contract;
 * writeBytes self-copy uses snapshot semantics and supports overlapping ranges.
 * Native subclass initialization/index enumeration, shared
 * workers, compression and multibyte code pages explicitly reject. This module
 * shares its per-publication lifecycle with the browser runtime. Host-only bulk
 * transfers copy bytes without exposing the backing store or changing cursors.
 */
(function (global) {
  'use strict';
  const MIB=1024*1024,HARD_LIMIT=256*MIB;
  function unsupported(name) {throw new TypeError(`ByteArray.${name} is not implemented in the bounded binary-buffer profile.`);}
  class Endian {constructor(){throw new TypeError('Endian supplies static byte-order constants; construction is not supported.');}}
  Object.defineProperties(Endian,{BIG_ENDIAN:{value:'bigEndian'},LITTLE_ENDIAN:{value:'littleEndian'},__nfIs:{value:()=>false},__nfInitialize:{value:()=>{throw new TypeError('Endian cannot be used as a native base class.');}}});

  function create(options={}) {
    const maxBytes=options.maxBytes??16*MIB,maxTotalBytes=options.maxTotalBytes??64*MIB,maxArrays=options.maxArrays??4096;
    for(const [name,value,limit] of [['maxBytes',maxBytes,HARD_LIMIT],['maxTotalBytes',maxTotalBytes,HARD_LIMIT],['maxArrays',maxArrays,65536]]) {
      if(!Number.isInteger(value)||value<1||value>limit)throw new RangeError(`${name} must be an integer from 1 through ${limit}.`);
    }
    const states=new WeakMap(),records=new Set(),encoder=new TextEncoder();
    const decoder=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true});
    let generation=1,allocated=0,disposed=false,objectCodec=null,defaultEncoding=3;
    function encoding(value){if(value!==0&&value!==3)throw new RangeError('ByteArray.objectEncoding must be AMF0 (0) or AMF3 (3).');return value;}
    function open(){if(disposed)throw new Error('This ByteArray service has been disposed.');}
    function state(receiver){
      open();const record=states.get(receiver);
      if(!record)throw new TypeError('This operation requires a ByteArray from this publication.');
      if(record.generation!==generation)throw new Error('This ByteArray belongs to a previous publication session.');
      return record;
    }
    function size(value,name='size') {
      if(!Number.isInteger(value)||value<0||value>maxBytes)throw new RangeError(`ByteArray ${name} must be an integer from 0 through ${maxBytes}.`);
      return value;
    }
    function number(value){if(typeof value!=='number')throw new TypeError('ByteArray numeric writes require primitive Number values.');return value;}
    function index(key){
      if(typeof key!=='string'||key==='')return null;
      const value=Number(key);
      return Number.isInteger(value)&&value>=0&&String(value)===key?value:null;
    }
    function readEnd(record,offset,count){
      size(offset,'offset');size(count,'length');
      if(offset>record.length||count>record.length-offset)throw new RangeError('ByteArray read exceeds available data.');
      return offset+count;
    }
    function writeEnd(offset,count){
      size(offset,'offset');size(count,'length');
      if(count>maxBytes-offset)throw new RangeError('ByteArray write exceeds the configured buffer limit.');
      return offset+count;
    }
    function reserve(record,end){
      if(end<=record.bytes.length)return;
      const old=record.bytes.length,available=maxTotalBytes-allocated+old;
      if(end>available)throw new RangeError('ByteArray publication allocation limit exceeded; clear unused buffers.');
      const preferred=Math.min(maxBytes,Math.max(end,Math.max(32,old*2)));
      const capacity=preferred<=available?preferred:end;
      const next=new Uint8Array(capacity); // May fail; no state changed yet.
      next.set(record.bytes.subarray(0,record.length));
      record.bytes=next;allocated+=capacity-old;
    }
    function extend(record,end){
      reserve(record,end);
      if(end>record.length){record.bytes.fill(0,record.length,end);record.length=end;}
    }
    function free(record){allocated-=record.bytes.length;record.bytes=new Uint8Array(0);record.length=0;record.position=0;}
    function view(record){return new DataView(record.bytes.buffer,record.bytes.byteOffset,record.bytes.byteLength);}
    function readNumber(receiver,width,method){
      const record=state(receiver),end=readEnd(record,record.position,width);
      const value=view(record)[method](record.position,record.little);
      record.position=end;return value;
    }
    function writeNumber(receiver,value,width,method){
      const record=state(receiver);number(value);
      const end=writeEnd(record.position,width);extend(record,end);
      view(record)[method](record.position,value,record.little);record.position=end;
    }
    function encoded(value,limit){
      if(typeof value!=='string')throw new TypeError('ByteArray UTF writes require a primitive String.');
      if(value.length>limit)throw new RangeError('ByteArray UTF string exceeds the supported byte length.');
      let bytes=0;
      for(let at=0;at<value.length;at++) {
        const unit=value.charCodeAt(at);
        if(unit>=0xd800&&unit<=0xdbff) {
          const low=value.charCodeAt(++at);
          if(!(low>=0xdc00&&low<=0xdfff))throw new TypeError('ByteArray UTF input contains an unsupported unpaired surrogate.');
          bytes+=4;
        } else {
          if(unit>=0xdc00&&unit<=0xdfff)throw new TypeError('ByteArray UTF input contains an unsupported unpaired surrogate.');
          bytes+=unit<0x80?1:unit<0x800?2:3;
        }
        if(bytes>limit)throw new RangeError('ByteArray UTF string exceeds the supported byte length.');
      }
      return encoder.encode(value);
    }
    function decoded(bytes,stopAtNull){
      let start=bytes[0]===0xef&&bytes[1]===0xbb&&bytes[2]===0xbf?3:0,end=bytes.length;
      if(stopAtNull){const nul=bytes.indexOf(0,start);if(nul>=0)end=nul;}
      try {return decoder.decode(bytes.subarray(start,end));}
      catch {throw new TypeError('ByteArray UTF input is not valid UTF-8; legacy replacement/code-page behavior is not implemented.');}
    }
    function writeUtf(receiver,value,prefix){
      const record=state(receiver),bytes=encoded(value,prefix?Math.min(65535,maxBytes):maxBytes);
      const end=writeEnd(record.position,bytes.length+(prefix?2:0));extend(record,end);
      let at=record.position;
      if(prefix){view(record).setUint16(at,bytes.length,record.little);at+=2;}
      record.bytes.set(bytes,at);record.position=end;
    }
    function initialize(target,args){
      open();if(args.length)throw new TypeError('ByteArray construction expects no arguments.');
      if(records.size>=maxArrays)throw new RangeError('ByteArray publication instance limit exceeded.');
      const record={bytes:new Uint8Array(0),length:0,position:0,little:false,objectEncoding:defaultEncoding,generation};
      const proxy=new Proxy(target,{
        get(object,key,receiver){const i=index(key);if(i===null)return Reflect.get(object,key,receiver);const r=state(receiver);size(i,'index');return i<r.length?r.bytes[i]:undefined;},
        set(object,key,value,receiver){
          const i=index(key);if(i===null)return Reflect.set(object,key,value,receiver);
          const r=state(receiver);number(value);const end=writeEnd(i,1);extend(r,end);r.bytes[i]=value;return true;
        },
        has(object,key){const i=index(key);if(i===null)return Reflect.has(object,key);const r=state(proxy);size(i,'index');return i<r.length;},
        deleteProperty(object,key){if(index(key)!==null)unsupported('delete indexed byte');return Reflect.deleteProperty(object,key);},
        defineProperty(object,key,descriptor){if(index(key)!==null)unsupported('define indexed property');return Reflect.defineProperty(object,key,descriptor);},
        getOwnPropertyDescriptor(object,key){
          const i=index(key);if(i===null)return Reflect.getOwnPropertyDescriptor(object,key);
          const r=state(proxy);size(i,'index');return i<r.length?{value:r.bytes[i],writable:true,enumerable:true,configurable:true}:undefined;
        },
        ownKeys(){unsupported('indexed enumeration');},
        preventExtensions(){unsupported('preventExtensions');}
      });
      states.set(target,record);states.set(proxy,record);records.add(record);return proxy;
    }

    class ByteArray {
      constructor(...args){return initialize(this,args);}
      get length(){return state(this).length;}
      set length(value){
        const record=state(this);size(value,'length');reserve(record,value);
        if(value<record.length)record.bytes.fill(0,value,record.length);
        else record.bytes.fill(0,record.length,value);
        record.length=value;record.position=Math.min(record.position,value);
      }
      get position(){return state(this).position;}
      set position(value){state(this).position=size(value,'position');}
      get bytesAvailable(){const record=state(this);return Math.max(0,record.length-record.position);}
      get endian(){return state(this).little?Endian.LITTLE_ENDIAN:Endian.BIG_ENDIAN;}
      set endian(value){
        const record=state(this);
        if(value!==Endian.BIG_ENDIAN&&value!==Endian.LITTLE_ENDIAN)throw new TypeError('ByteArray endian must be Endian.BIG_ENDIAN or Endian.LITTLE_ENDIAN.');
        record.little=value===Endian.LITTLE_ENDIAN;
      }
      clear(){free(state(this));}
      get objectEncoding(){return state(this).objectEncoding;}
      set objectEncoding(value){state(this).objectEncoding=encoding(value);}
      writeObject(value){
        const record=state(this);if(!objectCodec)unsupported('writeObject without the shared Rust AMF codec');
        const position=record.position,version=record.generation,bytes=objectCodec.encode(value,record.objectEncoding);
        if(state(this)!==record||record.generation!==version||record.position!==position)throw new Error('ByteArray changed during AMF serialization.');
        const end=writeEnd(position,bytes.length);extend(record,end);record.bytes.set(bytes,position);record.position=end;
      }
      readObject(){
        const record=state(this);if(!objectCodec)unsupported('readObject without the shared Rust AMF codec');
        const result=objectCodec.decode(record.bytes.subarray(record.position,record.length),record.objectEncoding);
        record.position+=result.consumed;return result.value;
      }
      readBoolean(){return readNumber(this,1,'getUint8')!==0;}
      readByte(){return readNumber(this,1,'getInt8');}
      readUnsignedByte(){return readNumber(this,1,'getUint8');}
      readShort(){return readNumber(this,2,'getInt16');}
      readUnsignedShort(){return readNumber(this,2,'getUint16');}
      readInt(){return readNumber(this,4,'getInt32');}
      readUnsignedInt(){return readNumber(this,4,'getUint32');}
      readFloat(){return readNumber(this,4,'getFloat32');}
      readDouble(){return readNumber(this,8,'getFloat64');}
      writeBoolean(value){if(typeof value!=='boolean')throw new TypeError('ByteArray.writeBoolean requires a primitive Boolean.');writeNumber(this,value?1:0,1,'setUint8');}
      writeByte(value){writeNumber(this,value,1,'setInt8');}
      writeShort(value){writeNumber(this,value,2,'setInt16');}
      writeInt(value){writeNumber(this,value,4,'setInt32');}
      writeUnsignedInt(value){writeNumber(this,value,4,'setUint32');}
      writeFloat(value){writeNumber(this,value,4,'setFloat32');}
      writeDouble(value){writeNumber(this,value,8,'setFloat64');}
      readBytes(bytes,offset=0,length=0){
        const source=state(this),destination=state(bytes);size(offset,'offset');size(length,'length');
        if(source===destination)unsupported('readBytes into the same buffer');
        const count=length===0?Math.max(0,source.length-source.position):length;
        const sourceEnd=readEnd(source,source.position,count),end=writeEnd(offset,count);
        extend(destination,end);
        destination.bytes.set(source.bytes.subarray(source.position,sourceEnd),offset);
        source.position=sourceEnd;
      }
      writeBytes(bytes,offset=0,length=0){
        const destination=state(this),source=state(bytes);size(offset,'offset');size(length,'length');
        const count=length===0?Math.max(0,source.length-offset):length;
        const sourceEnd=readEnd(source,offset,count),end=writeEnd(destination.position,count);
        // TypedArray.set provides overlap-safe copy after any required growth.
        extend(destination,end);
        destination.bytes.set(source.bytes.subarray(offset,sourceEnd),destination.position);
        destination.position=end;
      }
      readUTF(){
        const record=state(this);readEnd(record,record.position,2);
        const count=view(record).getUint16(record.position,record.little),start=record.position+2;
        const end=readEnd(record,start,count),result=decoded(record.bytes.subarray(start,end),true);
        record.position=end;return result;
      }
      readUTFBytes(length){
        const record=state(this),end=readEnd(record,record.position,size(length,'length'));
        const result=decoded(record.bytes.subarray(record.position,end),true);record.position=end;return result;
      }
      writeUTF(value){writeUtf(this,value,true);}
      writeUTFBytes(value){writeUtf(this,value,false);}
      toString(){const record=state(this);return decoded(record.bytes.subarray(0,record.length),false);}
    }
    Object.defineProperties(ByteArray,{__nfIs:{value:value=>states.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native ByteArray subclass initialization requires indexed proxy construction and is not implemented.');}}});
    for(const name of ['shareable'])Object.defineProperty(ByteArray.prototype,name,{get(){unsupported(name);},set(){unsupported(name);}});
    Object.defineProperty(ByteArray,'defaultObjectEncoding',{get(){open();return defaultEncoding;},set(value){open();defaultEncoding=encoding(value);}});
    for(const name of ['compress','uncompress','deflate','inflate','readMultiByte','writeMultiByte','atomicCompareAndSwapIntAt','atomicCompareAndSwapLength','toBase64','toHexString','writeBase64','writeHexString','writeRandomBytes'])Object.defineProperty(ByteArray.prototype,name,{value(){unsupported(name);}});
    for(const name of ['createFromBase64','createFromHexString'])Object.defineProperty(ByteArray,name,{value(){unsupported(name);}});
    // Host-only copies for resource/network adapters. No mutable backing storage
    // escapes, and transfers never change an existing ByteArray cursor.
    function fromBytes(bytes){
      open();if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1)throw new TypeError('ByteArray bulk input requires a byte view.');
      size(bytes.byteLength,'bulk byte length');
      const result=new ByteArray(),record=state(result);
      try {extend(record,bytes.byteLength);record.bytes.set(new Uint8Array(bytes.buffer,bytes.byteOffset,bytes.byteLength));return result;}
      catch(error){free(record);records.delete(record);throw error;}
    }
    function fromByteArrays(chunks){
      open();if(!Array.isArray(chunks)||chunks.length>maxArrays-records.size)throw new RangeError('AMF ByteArrays exceed the publication instance budget.');
      const result=[];
      try{for(const chunk of chunks)result.push(fromBytes(chunk));return result;}
      catch(error){for(const value of result){const record=states.get(value);free(record);records.delete(record);record.generation=-1;}throw error;}
    }
    function setObjectCodec(codec){if(!codec||typeof codec.encode!=='function'||typeof codec.decode!=='function')throw new TypeError('Expected the shared AMF value codec.');objectCodec=codec;}
    function toBytes(receiver){const record=state(receiver);return record.bytes.slice(0,record.length);}
    // Trusted synchronous native adapters only. The callback receives a copy,
    // never the backing allocation. A partial pixel read can commit its consumed
    // prefix before reporting EOF; failed native calls leave the cursor intact.
    function readAvailable(receiver,consume){
      const record=state(receiver),start=record.position;
      const result=consume(record.bytes.slice(Math.min(start,record.length),record.length),record.little);
      const count=result?.consumed;
      if(!Number.isSafeInteger(count)||count<0||count>Math.max(0,record.length-start))throw new Error('Invalid native ByteArray consumed length.');
      record.position=start+count;return result;
    }
    function writeFromHost(receiver,produce){
      const record=state(receiver),bytes=typeof produce==='function'?produce(record.little):produce;
      if(!ArrayBuffer.isView(bytes)||bytes.BYTES_PER_ELEMENT!==1)throw new TypeError('Native ByteArray write requires a byte view.');
      const end=writeEnd(record.position,bytes.byteLength);extend(record,end);
      record.bytes.set(new Uint8Array(bytes.buffer,bytes.byteOffset,bytes.byteLength),record.position);record.position=end;
    }
    function reset(){open();for(const record of records)free(record);records.clear();generation++;defaultEncoding=3;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({ByteArray,Endian,fromBytes,fromByteArrays,toBytes,readAvailable,writeFromHost,setObjectCodec,reset,dispose});
  }
  const api=Object.freeze({create});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;
  else global.NeoFlashByteArray=api;
})(globalThis);
