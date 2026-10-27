// Original AS3 Dictionary adapter, with cross-unit branding and no global patch.
// https://airsdk.dev/reference/actionscript/3.0/flash/utils/Dictionary.html
// Pinned primitive-key profile: Ruffle fe6cfc545405b888ebb478ce1c211d63147ca49b
// core/src/avm2/{activation.rs,object/dictionary_object.rs,object/script_object.rs}.
// Object/function keys retain identity. Primitive keys use the VM's string-name
// path (1/'1', NaN/'NaN', null/'null' share slots); canonical uint28 names enumerate
// as numbers. Number-to-string formatting uses the JS backend, not a Flash oracle.
// Weak values reside ONLY in a WeakMap; enumeration records contain WeakRef keys,
// never a strong value/key fallback. Collection timing is browser-dependent.
// Snapshot enumeration skips deleted/collected keys, observes current values,
// ignores later insertions; historical hash-table order/mutation is not claimed.
const __nfDictionaryBrand=Symbol.for('neoflash.as3.dictionary.v1');
const __nfDictionaryDescriptor=Object.getOwnPropertyDescriptor;
const __nfDictionaryLimit=100000, __nfDictionaryKeyUnits=1000000;
let __nfDictionaryFacade;
function __nfDictionaryError(message){throw new __nfTypeError('Dictionary: '+message);}
function __nfDictionaryIs(value){
    if(value===null||(typeof value!=='object'&&typeof value!=='function'))return false;
    const descriptor=__nfDictionaryDescriptor(value,__nfDictionaryBrand);
    return !!descriptor&&Object.hasOwn(descriptor,'value')&&descriptor.value?.version===1;
}
function __nfDictionaryState(value){if(!__nfDictionaryIs(value))__nfDictionaryError('expected an AS3 Dictionary.');return __nfDictionaryDescriptor(value,__nfDictionaryBrand).value;}
function __nfDictionaryObjectKey(key){return key!==null&&(typeof key==='object'||typeof key==='function');}
function __nfDictionaryPrimitive(key){
    if(typeof key==='symbol'||typeof key==='bigint')__nfDictionaryError('Symbol and BigInt keys are outside the ActionScript profile.');
    const name=__nfString(key);if(name.length>__nfDictionaryKeyUnits)__nfDictionaryError('key text exceeds 1000000 UTF-16 units.');return name;
}
function __nfDictionaryEnumerant(name){return /^(?:0|[1-9]\d*)$/.test(name)&&Number(name)<=268435455?Number(name):name;}
function __nfDictionaryPrune(state){
    if(!state.weak)return;
    for(const [token,entry]of state.entries)if(entry.weak&&!entry.weak.deref())state.entries.delete(token);
}
function __nfDictionaryRecord(state,key,object){return (object?state.objects:state.primitives).get(key);}
function __nfDictionaryRef(value,key){
    const state=__nfDictionaryState(value),object=__nfDictionaryObjectKey(key);
    if(!object)key=__nfDictionaryPrimitive(key);
    const store=object?state.objects:state.primitives;
    return {
        get(){const record=store.get(key);return record?record.value:object?undefined:Reflect.get(state.target,key,value);},
        set(next){
            const record=store.get(key);if(record){record.value=next;return next;}
            if(state.entries.size>=__nfDictionaryLimit)__nfDictionaryPrune(state);
            if(state.entries.size>=__nfDictionaryLimit)__nfDictionaryError('entry count exceeds 100000.');
            if(!object&&state.keyUnits+key.length>__nfDictionaryKeyUnits)__nfDictionaryError('total primitive key text exceeds 1000000 UTF-16 units.');
            if(!Number.isSafeInteger(state.nextToken))__nfDictionaryError('entry identity counter is exhausted.');
            const token=state.nextToken++;
            store.set(key,{token,value:next});
            state.entries.set(token,object?(state.weak?{weak:new WeakRef(key)}:{object:key}):{primitive:key});
            if(!object)state.keyUnits+=key.length;return next;
        },
        delete(){const record=store.get(key);if(record){store.delete(key);state.entries.delete(record.token);if(!object)state.keyUnits-=key.length;}return true;},
        has(){return store.has(key)||(!object&&Reflect.has(state.target,key));},
    };
}
function __nfDictionaryEnumerate(value,values){
    const state=__nfDictionaryState(value);__nfDictionaryPrune(state);
    const snapshot=[...state.entries];
    return (function*(){
        for(const [token,entry]of snapshot){
            if(!state.entries.has(token))continue;
            const object=Object.hasOwn(entry,'object')||Object.hasOwn(entry,'weak');
            const key=entry.weak?entry.weak.deref():object?entry.object:entry.primitive;
            if(object&&key===undefined){state.entries.delete(token);continue;}
            const record=__nfDictionaryRecord(state,key,object);
            if(!record||record.token!==token)continue;
            yield values?record.value:object?key:__nfDictionaryEnumerant(key);
        }
    })();
}
function __nfDictionaryCreate(as3,args){
    if(!as3)__nfDictionaryError('Dictionary is not an AS2 built-in.');
    if(args.length>1)__nfDictionaryError('constructor accepts only weakKeys.');
    const weak=Boolean(args[0]);
    if(weak&&(typeof WeakRef!=='function'||typeof WeakMap!=='function'))__nfDictionaryError('weakKeys requires browser WeakRef and WeakMap support; strong fallback is not used.');
    const target=Object.create(__nfDictionaryClass(true).prototype);
    const state={version:1,target,weak,objects:weak?new WeakMap():new Map(),primitives:new Map(),entries:new Map(),nextToken:1,keyUnits:0};
    Object.defineProperty(target,__nfDictionaryBrand,{value:state});
    let proxy;
    proxy=new Proxy(target,{
        get(target,key){if(key===__nfDictionaryBrand)return state;if(key===Symbol.toStringTag)return 'Dictionary';if(typeof key==='symbol')return Reflect.get(target,key,proxy);return __nfDictionaryRef(proxy,key).get();},
        set(target,key,value){if(typeof key==='symbol')__nfDictionaryError('Symbol properties are unsupported.');__nfDictionaryRef(proxy,key).set(value);return true;},
        has(target,key){return typeof key==='symbol'?Reflect.has(target,key):__nfDictionaryRef(proxy,key).has();},
        deleteProperty(target,key){return typeof key==='symbol'?Reflect.deleteProperty(target,key):__nfDictionaryRef(proxy,key).delete();},
        ownKeys(){return [__nfDictionaryBrand,...state.primitives.keys()];},
        getOwnPropertyDescriptor(target,key){const record=typeof key==='string'?state.primitives.get(key):undefined;return record?{value:record.value,writable:true,enumerable:true,configurable:true}:Reflect.getOwnPropertyDescriptor(target,key);},
        defineProperty(){__nfDictionaryError('custom property descriptors are unsupported; use indexed assignment.');},
        setPrototypeOf(){__nfDictionaryError('prototype replacement is unsupported.');},
        preventExtensions(){__nfDictionaryError('sealing or freezing a dynamic Dictionary is unsupported.');},
    });
    return proxy;
}
function __nfDictionaryClass(as3){
    if(!as3)__nfDictionaryError('Dictionary is not an AS2 built-in.');
    if(!__nfDictionaryFacade){
        const Type=function(...args){
            if(new.target)return __nfDictionaryCreate(true,args);
            if(args.length!==1)__nfDictionaryError('cast expects exactly one value.');
            if(args[0]==null)return null;if(__nfDictionaryIs(args[0]))return args[0];__nfDictionaryError('cannot cast this value.');
        };
        Object.defineProperties(Type,{__nfIs:{value:__nfDictionaryIs},[Symbol.hasInstance]:{value:__nfDictionaryIs},__nfInitialize:{value:()=>__nfDictionaryError('native subclassing is unsupported.')}});
        Object.defineProperties(Type.prototype,{
            toJSON:{value:function(){return 'Dictionary';},writable:true,configurable:true},
            setPropertyIsEnumerable:{value:function(){__nfDictionaryState(this);},writable:true,configurable:true},
        });
        __nfDictionaryFacade=Type;
    }
    return __nfDictionaryFacade;
}
