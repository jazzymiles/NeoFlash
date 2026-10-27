// Original bounded AS3 dense Vector adapter. AIR Vector reference and Adobe
// avmplus Vector.as/VectorImpl.as/VectorClass-impl.h at
// 858d034a3bd3a54d9b70909386435cf4aec81d21 define the common collection profile.
// No upstream code is copied. No global prototype or mutable registry is changed.
// Primitive/class tokens, rather than names, determine invariant element types.
// Historical error IDs, prototype extension, structural callback mutation and
// sort comparator order are outside this initial profile.
const __nfVectorBrand = Symbol.for('neoflash.as3.vector.instance.v1');
const __nfVectorTypeBrand = Symbol.for('neoflash.as3.vector.type.v1');
const __nfVectorTypes = new Map();
const __nfVectorDescriptorOf = Object.getOwnPropertyDescriptor;
const __nfVectorApply = Reflect.apply;
const __nfVectorArrayIs = Array.isArray;
const __nfVectorLimit = 1000000;
const __nfVectorArray = Object.freeze(Object.fromEntries(
    ['slice','push','pop','shift','unshift','splice','reverse','sort'].map(name=>[name,Array.prototype[name]])
));
function __nfVectorError(kind, message) { return new __nfErrorTypes[kind]('Vector: ' + message); }
function __nfVectorUnsupported(message) { throw __nfVectorError('TypeError','unsupported operation: '+message); }
function __nfVectorIs(value) {
    if(value===null||(typeof value!=='object'&&typeof value!=='function'))return false;
    const tag=__nfVectorDescriptorOf(value,__nfVectorBrand);
    return !!tag&&Object.hasOwn(tag,'value')&&tag.value?.version===1;
}
function __nfVectorState(value) {
    if(!__nfVectorIs(value))throw __nfVectorError('TypeError','expected a typed Vector.');
    return __nfVectorDescriptorOf(value,__nfVectorBrand).value;
}
function __nfVectorSameToken(a,b,depth=0) {
    if(a===b)return true;
    if(depth>=64||typeof a!=='function'||typeof b!=='function')return false;
    const left=__nfVectorDescriptorOf(a,__nfVectorTypeBrand),right=__nfVectorDescriptorOf(b,__nfVectorTypeBrand);
    return !!left&&!!right&&__nfVectorSameToken(left.value.token,right.value.token,depth+1);
}
function __nfVectorCast(value,type) {
    if(value==null)return null;
    if(type.__nfIs(value))return value;
    throw __nfVectorError('TypeError','invariant element type mismatch. Convert explicitly with the destination Vector type.');
}
function __nfVectorLength(value) {
    if(typeof value!=='number'||!Number.isInteger(value)||value<0||value>__nfVectorLimit)
        throw __nfVectorError('RangeError','length must be an integer from 0 through 1000000.');
    return value;
}
function __nfVectorPosition(value, fallback) {
    if(value===undefined)return fallback;
    if(typeof value!=='number')__nfVectorUnsupported('positions currently require numeric arguments.');
    return Number.isNaN(value)?0:Math.trunc(value);
}
function __nfVectorClamp(index,length) { return index<0?Math.max(0,length+index):Math.min(length,index); }
function __nfVectorMutable(state,structural=false) {
    if(state.lock) __nfVectorUnsupported('reentrant mutation during coercion or sorting.');
    if(structural&&state.iterating)__nfVectorUnsupported('length-changing mutation during a Vector callback.');
}
function __nfVectorConvertValues(state,values) {
    // Coercion is transactional for this operation. A converter may not mutate
    // its destination, preventing callback reentry from making indexes stale.
    const result=[];state.lock++;
    try {for(const value of values)result.push(state.element.coerce(value));}
    finally {state.lock--;}
    return result;
}
function __nfVectorNew(element,type,values,fixed=false) {
    __nfVectorLength(values.length);
    const target=Object.create(type.prototype);
    const state={version:1,element,type,values,fixed,methods:new Map(),lock:0,iterating:0,proxy:null};
    Object.defineProperty(target,__nfVectorBrand,{value:state});
    const proxy=new Proxy(target,{
        get(object,key,receiver){
            if(key===__nfVectorBrand)return state;
            if(key===Symbol.toStringTag)return 'Vector.<'+element.name+'>';
            if(key===Symbol.toPrimitive)return undefined;
            if(key==='toJSON')return undefined; // JSON traverses vectors as arrays.
            if(key===Symbol.iterator)return function*(){const count=state.values.length;for(let i=0;i<count&&i<state.values.length;i++)yield state.values[i];};
            return __nfVectorRef(receiver,key).get();
        },
        set(_target,key,value,receiver){__nfVectorRef(receiver,key).set(value);return true;},
        has(_target,key){if(key===__nfVectorBrand)return true;return __nfVectorRef(proxy,key).has();},
        deleteProperty(_target,key){if(key===__nfVectorBrand)return false;return __nfVectorRef(proxy,key).delete();},
        ownKeys(){return [...__nfVectorKeys(proxy),__nfVectorBrand];},
        getOwnPropertyDescriptor(object,key){
            if(key===__nfVectorBrand)return __nfVectorDescriptorOf(object,key);
            const index=__nfVectorIndex(key);
            if(index!==null&&index>=0&&index<state.values.length)return {value:state.values[index],writable:true,enumerable:true,configurable:true};
            return undefined;
        },
        defineProperty(){__nfVectorUnsupported('defineProperty or dynamic fields on Vector.');},
        setPrototypeOf(){__nfVectorUnsupported('changing Vector prototypes.');},
        preventExtensions(){__nfVectorUnsupported('sealing/freezing the JavaScript representation of Vector.');},
    });
    state.proxy=proxy;
    return proxy;
}
function __nfVectorType(descriptor) {
    if(!descriptor||typeof descriptor.coerce!=='function'||typeof descriptor.name!=='string'||descriptor.token==null)
        throw __nfVectorError('TypeError','invalid element descriptor.');
    if(__nfVectorTypes.has(descriptor.token))return __nfVectorTypes.get(descriptor.token);
    const element=Object.freeze({token:descriptor.token,coerce:descriptor.coerce,defaultValue:descriptor.defaultValue,name:descriptor.name});
    const type=function(...args){
        if(new.target){
            if(new.target!==type)__nfVectorUnsupported('native Vector subclass construction.');
            if(args.length>2)__nfVectorUnsupported('constructor expects length and optional fixed Boolean.');
            const length=__nfVectorLength(args[0]===undefined?0:args[0]);
            const fixed=args[1]===undefined?false:args[1];
            if(typeof fixed!=='boolean')__nfVectorUnsupported('fixed currently requires a Boolean.');
            const values=new Array(length);for(let i=0;i<length;i++)values[i]=element.defaultValue;
            return __nfVectorNew(element,type,values,fixed);
        }
        if(args.length!==1)__nfVectorUnsupported('conversion expects one Array or Vector.');
        const source=args[0];
        if(type.__nfIs(source))return source;
        const input=__nfVectorIs(source)?__nfVectorState(source).values:__nfVectorArrayIs(source)?source:null;
        if(input===null)__nfVectorUnsupported('conversion requires an Array or Vector; array-like objects are pending.');
        __nfVectorLength(input.length);
        const values=[];for(let i=0,count=input.length;i<count;i++)values.push(element.coerce(input[i]));
        return __nfVectorNew(element,type,values);
    };
    Object.defineProperties(type,{
        [__nfVectorTypeBrand]:{value:element},
        __nfIs:{value:value=>__nfVectorIs(value)&&__nfVectorSameToken(__nfVectorState(value).element.token,element.token)},
        [Symbol.hasInstance]:{value:value=>type.__nfIs(value)},
        __nfInitialize:{value:()=>__nfVectorUnsupported('native Vector subclass initialization.')},
    });
    Object.freeze(type.prototype);
    __nfVectorTypes.set(element.token,Object.freeze(type));
    return type;
}
function __nfVectorIndex(key) {
    if(typeof key==='number')return key;
    if(typeof key==='string'&&/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/.test(key))return Number(key);
    return null;
}
function __nfVectorKeys(value) {return Array.from({length:__nfVectorState(value).values.length},(_,i)=>String(i));}
function __nfVectorRef(value,key) {
    const state=__nfVectorState(value);
    if(typeof key!=='string'&&typeof key!=='number'&&typeof key!=='symbol')__nfVectorUnsupported('object/implicit property-key coercion.');
    const index=__nfVectorIndex(key);
    const valid=index!==null&&Number.isInteger(index)&&index>=0;
    const has=()=>index!==null?valid&&index<state.values.length
        :key==='length'||key==='fixed'||key==='constructor'||__nfVectorMethodNames.has(key);
    return {
        has,
        get(){
            if(index!==null){if(!valid||index>=state.values.length)throw __nfVectorError('RangeError','index is outside the Vector bounds.');return state.values[index];}
            if(key==='length')return state.values.length;
            if(key==='fixed')return state.fixed;
            if(key==='constructor')return state.type;
            if(__nfVectorMethodNames.has(key)){
                if(!state.methods.has(key))state.methods.set(key,(...args)=>__nfVectorMethod(state,key,args));
                return state.methods.get(key);
            }
            throw __nfVectorError('ReferenceError','unknown or unsupported member '+String(key)+'.');
        },
        set(next){
            if(index!==null){
                if(!valid||index>state.values.length)throw __nfVectorError('RangeError','assignment must replace an element or append at length.');
                const append=index===state.values.length;
                __nfVectorMutable(state,append);
                if(append&&state.fixed)throw __nfVectorError('RangeError','cannot append to a fixed Vector.');
                if(append)__nfVectorLength(index+1);
                const converted=__nfVectorConvertValues(state,[next])[0];state.values[index]=converted;
            }else if(key==='length'){
                __nfVectorMutable(state,true);
                if(state.fixed)throw __nfVectorError('RangeError','cannot assign length on a fixed Vector.');
                const length=__nfVectorLength(next),old=state.values.length;state.values.length=length;
                for(let i=old;i<length;i++)state.values[i]=state.element.defaultValue;
            }else if(key==='fixed'){
                __nfVectorMutable(state,true);
                if(typeof next!=='boolean')__nfVectorUnsupported('fixed currently requires a Boolean.');
                state.fixed=next;
            }else throw __nfVectorError('ReferenceError','Vector members are sealed; cannot assign '+String(key)+'.');
            return next; // Assignment expression keeps the original RHS.
        },
        // Flash's deleteproperty reports true without creating a dense-array hole.
        delete(){return true;},
    };
}
const __nfVectorMethodNames=new Set(['push','pop','shift','unshift','slice','splice','concat','reverse',
    'indexOf','lastIndexOf','sort','map','filter','every','some','forEach','join','toString','toLocaleString',
    'hasOwnProperty','propertyIsEnumerable','valueOf']);
function __nfVectorMethod(state,name,args) {
    const values=state.values,proxy=state.proxy,make=items=>__nfVectorNew(state.element,state.type,items);
    const arity=max=>{if(args.length>max)__nfVectorUnsupported(name+' excess arguments.');};
    const unfixed=()=>{__nfVectorMutable(state,true);if(state.fixed)throw __nfVectorError('RangeError',name+' cannot change a fixed Vector.');};
    switch(name){
        case 'valueOf':arity(0);return proxy;
        case 'hasOwnProperty':case 'propertyIsEnumerable':arity(1);{const index=__nfVectorIndex(args[0]);return index!==null&&Number.isInteger(index)&&index>=0&&index<values.length;}
        case 'push':case 'unshift':{
            unfixed();__nfVectorLength(values.length+args.length);
            const items=__nfVectorConvertValues(state,args);
            if(name==='push'){for(const item of items)values.push(item);}
            else state.values=items.concat(values);
            return state.values.length;
        }
        case 'pop':case 'shift':{
            arity(0);unfixed();
            if(!values.length)return typeof state.element.defaultValue==='number'?0:undefined;
            return __nfVectorApply(__nfVectorArray[name],values,[]);
        }
        case 'reverse':arity(0);__nfVectorMutable(state);__nfVectorApply(__nfVectorArray.reverse,values,[]);return proxy;
        case 'slice':{
            arity(2);const start=__nfVectorClamp(__nfVectorPosition(args[0],0),values.length),end=__nfVectorClamp(__nfVectorPosition(args[1],values.length),values.length);
            return make(__nfVectorApply(__nfVectorArray.slice,values,[start,end]));
        }
        case 'splice':{
            if(!args.length)__nfVectorUnsupported('splice requires a start position.');
            const start=__nfVectorClamp(__nfVectorPosition(args[0],0),values.length);
            const count=Math.max(0,Math.min(values.length-start,__nfVectorPosition(args[1],values.length-start)));
            const change=args.length-2;const insert=Math.max(0,change),length=values.length-count+insert;
            __nfVectorMutable(state,length!==values.length);__nfVectorLength(length);
            if(state.fixed&&length!==values.length)throw __nfVectorError('RangeError','splice would change a fixed Vector length.');
            const items=__nfVectorConvertValues(state,args.slice(2));
            const removed=values.slice(start,start+count);
            state.values=values.slice(0,start).concat(items,values.slice(start+count));
            return make(removed);
        }
        case 'concat':{
            let length=values.length;const sources=[];
            for(const value of args){if(!state.type.__nfIs(value))throw __nfVectorError('TypeError','concat requires Vectors with the same invariant element type.');const part=__nfVectorState(value).values;length+=part.length;__nfVectorLength(length);sources.push(part);}
            const result=values.slice();for(const part of sources)for(const item of part)result.push(item);return make(result);
        }
        case 'indexOf':case 'lastIndexOf':{
            arity(2);const sought=state.element.coerce(args[0]);
            const start=__nfVectorClamp(__nfVectorPosition(args[1],name==='indexOf'?0:values.length),values.length);
            if(name==='indexOf'){for(let i=start;i<values.length;i++)if(values[i]===sought)return i;}
            else for(let i=Math.min(start,values.length-1);i>=0;i--)if(values[i]===sought)return i;
            return -1;
        }
        case 'join':case 'toString':case 'toLocaleString':{
            arity(name==='join'?1:0);const separator=name==='join'&&args[0]!==undefined?String(args[0]):',';
            let result='';for(let i=0;i<values.length;i++){
                const item=values[i],part=name==='toLocaleString'?(item==null?'':String(item.toLocaleString())):String(item);
                if(result.length+part.length+(i?separator.length:0)>__nfVectorLimit)throw __nfVectorError('RangeError','string result exceeds 1000000 UTF-16 code units.');
                result+=(i?separator:'')+part;
            }return result;
        }
        case 'sort':{
            arity(1);__nfVectorMutable(state);
            if(typeof args[0]!=='function'&&args[0]!==undefined&&(Number(args[0])&8))__nfVectorUnsupported('RETURNINDEXEDARRAY sort behavior on typed Vectors.');
            const candidate=values.slice();state.lock++;
            try{const sorted=__nfArraySort(candidate,args,true,false);if(sorted!==0)state.values=candidate;}
            finally{state.lock--;}
            return proxy;
        }
        case 'map':case 'filter':case 'every':case 'some':case 'forEach':{
            arity(2);const callback=args[0],thisObject=args[1]===undefined?null:args[1];
            if(typeof callback!=='function')throw __nfVectorError('TypeError',name+' requires a function.');
            const count=values.length,result=[];state.iterating++;
            try{for(let index=0;index<count;index++){
                const item=state.values[index],answer=__nfVectorApply(callback,thisObject,[item,index,proxy]);
                if(name==='every'&&!answer)return false;
                if(name==='some'&&answer)return true;
                if(name==='filter'&&answer)result.push(item);
                if(name==='map')result.push(state.element.coerce(answer));
            }}finally{state.iterating--;}
            if(name==='every')return true;if(name==='some')return false;if(name==='forEach')return undefined;
            return make(result);
        }
        default:__nfVectorUnsupported(name);
    }
}
