// Shared reference/enumeration bridge. Expressions are evaluated once by the
// emitter; this helper preserves key identity before any ordinary String coercion.
// Ordinary enumeration is an explicit bounded JS-backed own/inherited-enumerable
// profile, not historical AVM iteration order. Fixed project traits are filtered
// by the program registry. Native objects with hidden storage diagnose.
const __nfContainerLimit=1000000;
function __nfContainerKey(key){
    if(typeof key==='symbol'||typeof key==='bigint')throw new __nfTypeError('ActionScript property keys do not support Symbol or BigInt in this profile.');
    const name=__nfString(key);if(name.length>__nfContainerLimit)throw new __nfTypeError('Property key exceeds 1000000 UTF-16 units.');return name;
}
function __nfContainerProgram(){return typeof __nfProgram!=='undefined'?__nfProgram:null;}
function __nfContainerRef(receiver,key,as3){
    if(receiver==null)throw new __nfTypeError('Cannot access a property on null or undefined.');
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.ref(receiver,key,as3);
    if(__nfDictionaryIs(receiver)){if(!as3)throw new __nfTypeError('Dictionary access requires AS3.');return __nfDictionaryRef(receiver,key);}
    if(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver)){if(!as3)throw new __nfTypeError('Vector access requires AS3.');return __nfVectorRef(receiver,key);}
    const object=Object(receiver),program=__nfContainerProgram();
    // Capture the key expression once, but retain its value: ordinary property
    // conversion belongs to each read/write operation, after the assignment RHS
    // has run. Compound updates can therefore convert an object key twice.
    return {
        get(){
            const name=__nfContainerKey(key),value=receiver[name];
            // AVM1 exposes an absent native scrollRect as undefined; AS3 and
            // the public JS facade use null. Never rewrite ordinary user fields.
            return !as3&&name==='scrollRect'&&value===null&&typeof __nfDisplay!=='undefined'&&__nfDisplay?.alive(receiver)?undefined:value;
        },
        set(value){
            const name=__nfContainerKey(key);
            if(__nfArrayIsArray(receiver)){
                if(name==='length')__nfArrayBound(value);
                else if(/^(?:0|[1-9]\d*)$/.test(name)&&Number(name)<4294967295&&Number(name)>=__nfArrayLimit)throw new __nfTypeError('Array index exceeds the 1000000-element profile.');
            }
            if((typeof receiver!=='object'&&typeof receiver!=='function')||!Reflect.set(object,name,value,receiver))throw new __nfTypeError('Cannot write this property.');
            return value;
        },
        delete(){const name=__nfContainerKey(key);return program?.hasFixedProperty?.(receiver,name)?false:Reflect.deleteProperty(object,name);},
        has(){const name=__nfContainerKey(key);return program?.hasFixedProperty?.(receiver,name)||Reflect.has(object,name);},
    };
}
function __nfContainerMethod(receiver,key,as3){
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.method(receiver,key,as3);
    if(__nfDictionaryIs(receiver)||(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver))){
        const method=__nfContainerRef(receiver,key,as3).get();
        return (...args)=>__nfApply(method,receiver,args);
    }
    key=__nfContainerKey(key);
    if(__nfArrayIsArray(receiver))return __nfArrayMethod(receiver,key,as3);
    if(typeof receiver==='string')return __nfStringMethod(receiver,key,as3);
    if(__nfDateIs(receiver))return __nfDateMethod(receiver,key,as3);
    if(as3&&key==='toFixed')return __nfFixedMethod(receiver);
    const method=__nfContainerRef(receiver,key,as3).get();
    return (...args)=>__nfApply(method,receiver,args);
}
function __nfEnumerate(receiver,values,as3){
    if(receiver==null)return [][Symbol.iterator]();
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.enumerate(receiver,values,as3);
    if(__nfDictionaryIs(receiver)){if(!as3)throw new __nfTypeError('Dictionary enumeration requires AS3.');return __nfDictionaryEnumerate(receiver,values);}
    if(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver)){
        if(!as3)throw new __nfTypeError('Vector enumeration requires AS3.');
        const keys=__nfVectorKeys(receiver);if(keys.length>__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');
        return (function*(){for(const key of keys){const ref=__nfVectorRef(receiver,key);if(ref.has())yield values?ref.get():key;}})();
    }
    if(typeof receiver!=='object'||receiver[Symbol.toStringTag]==='NeoFlashDisplayObject')throw new __nfTypeError('Enumeration currently supports arrays, plain records and ordinary project objects; hidden native/display storage is unsupported.');
    const program=__nfContainerProgram(),fixed=program?.enumerationKeys?.(receiver)??null;
    const prototype=Object.getPrototypeOf(receiver);
    if(!Array.isArray(receiver)&&fixed===null&&prototype!==null&&prototype!==Object.prototype)throw new __nfTypeError('Enumeration of this native or foreign object is unsupported; copy its public values into a plain record.');
    let keys=fixed;
    if(keys===null){keys=[];for(const key in receiver){if(keys.length>=__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');keys.push(key);}}
    if(keys.length>__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');
    return (function*(){for(const key of keys){const ref=__nfContainerRef(receiver,key,as3);if(ref.has())yield values?ref.get():key;}})();
}
