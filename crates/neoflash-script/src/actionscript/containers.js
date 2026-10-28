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
function __nfLegacyKey(receiver,key,as3){
    const name=__nfContainerKey(key);
    if(as3||typeof __nfLegacyCase==='undefined'||!__nfLegacyCase)return name;
    if(typeof __nfLegacy==='undefined'||typeof __nfLegacy.caseKey!=='function')throw new __nfTypeError('ActionScript 1 requires the SWF6 case-name service; rebuild and re-export this project.');
    return __nfLegacy.caseKey(receiver,name);
}
function __nfContainerProgram(){return typeof __nfProgram!=='undefined'?__nfProgram:null;}
const __nfObjectPropertyMethods=Object.freeze({hasOwnProperty:Object.prototype.hasOwnProperty,propertyIsEnumerable:Object.prototype.propertyIsEnumerable});
const __nfObjectStringMethods=Object.freeze({toString:Object.prototype.toString,toLocaleString:Object.prototype.toLocaleString});
function __nfObjectDefaultMethod(receiver,name,value,program){
    if(name!=='toString'&&name!=='toLocaleString')return null;
    const method=program?.defaultObjectMethod?.(receiver,name,value);
    if(method)return method;
    if(value!==__nfObjectStringMethods[name]||receiver===null||typeof receiver!=='object')return null;
    const prototype=Object.getPrototypeOf(receiver);
    if(prototype!==null&&prototype!==Object.prototype)return null;
    return function(...args){
        if(this===null||typeof this!=='object')return __nfApply(value,this,args);
        const actualPrototype=Object.getPrototypeOf(this);
        if(actualPrototype!==null&&actualPrototype!==Object.prototype)return __nfApply(value,this,args);
        const execute=typeof __nfLegacy!=='undefined'?__nfLegacy?.objects3?.execute:null;
        if(typeof execute!=='function')throw new __nfTypeError('Rust Object string services are unavailable; rebuild and re-export this project.');
        try{return execute({op:'default_string',name:'Object',class_object:false});}
        catch(error){throw new __nfTypeError(String(error?.message??error));}
    };
}
function __nfObjectPropertyDescriptor(receiver,name){
    const project=__nfContainerProgram()?.objectDescriptor?.(receiver,name);
    if(project)return project;
    if(receiver===null||typeof receiver!=='object')return {supported:false};
    const prototype=Object.getPrototypeOf(receiver);
    if(prototype!==null&&prototype!==Object.prototype)return {supported:false};
    const descriptor=Object.getOwnPropertyDescriptor(receiver,name);
    return {supported:true,fixed:false,own:!!descriptor,enumerable:!!descriptor?.enumerable,configurable:!!descriptor?.configurable};
}
function __nfObjectPropertyMethod(receiver,method){
    return (...args)=>{
        if(args.length<1||args.length>(method==='setPropertyIsEnumerable'?2:1))throw new __nfTypeError(`Object.${method} received an unsupported argument count.`);
        const name=__nfContainerKey(args[0]),descriptor=__nfObjectPropertyDescriptor(receiver,name);
        const execute=typeof __nfLegacy!=='undefined'?__nfLegacy?.objects3?.execute:null;
        if(typeof execute!=='function')throw new __nfTypeError('Rust Object property services are unavailable; rebuild and re-export this project.');
        let result;
        try{result=execute({op:'property',method,name,...descriptor,is_enum:args.length<2?true:!!args[1]});}
        catch(error){throw new __nfTypeError(String(error?.message??error));}
        if(result.change_enumerable!==null)Object.defineProperty(receiver,name,{enumerable:result.change_enumerable});
        return method==='setPropertyIsEnumerable'?undefined:result.value;
    };
}
function __nfContainerRef(receiver,key,as3,caller){
    if(receiver==null)throw new __nfTypeError('Cannot access a property on null or undefined.');
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.ref(receiver,key,as3);
    if(__nfDictionaryIs(receiver)){if(!as3)throw new __nfTypeError('Dictionary access requires AS3.');return __nfDictionaryRef(receiver,key);}
    if(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver)){if(!as3)throw new __nfTypeError('Vector access requires AS3.');return __nfVectorRef(receiver,key);}
    const object=Object(receiver),program=__nfContainerProgram();
    const legacy=!as3&&typeof __nfLegacy!=='undefined'?__nfLegacy:null;
    const scopedRef=name=>legacy?.nativeRef?.(receiver,name)??program?.ref?.(receiver,name,caller);
    // Capture the key expression once, but retain its value: ordinary property
    // conversion belongs to each read/write operation, after the assignment RHS
    // has run. Compound updates can therefore convert an object key twice.
    return {
        get(){
            const name=__nfLegacyKey(receiver,key,as3);
            const arrayMember=as3&&__nfArrayIsArray(receiver),arrayPresent=arrayMember&&(name in object);
            if(as3&&name==='setPropertyIsEnumerable'&&!(name in object))return __nfObjectPropertyMethod(receiver,name);
            const scoped=scopedRef(name),read=()=>scoped?scoped.get():receiver[name];
            const value=legacy?.objects?legacy.objects.get(receiver,name,read,()=>scoped?scoped.has():Reflect.has(object,name)):read();
            if(arrayMember){const reference=__nfArrayReference(receiver,name,value,true,arrayPresent);if(reference!==value)return reference;}
            if(as3&&typeof receiver==='number')return __nfNumericMethodValue(receiver,value);
            if(as3&&Object.hasOwn(__nfObjectPropertyMethods,name)&&value===__nfObjectPropertyMethods[name])return __nfObjectPropertyMethod(receiver,name);
            if(as3){const method=__nfObjectDefaultMethod(receiver,name,value,program);if(method)return method;}
            // AVM1 exposes an absent native scrollRect as undefined; AS3 and
            // the public JS facade use null. Never rewrite ordinary user fields.
            return !as3&&name==='scrollRect'&&value===null&&typeof __nfDisplay!=='undefined'&&__nfDisplay?.alive(receiver)?undefined:value;
        },
        set(value){
            const name=__nfLegacyKey(receiver,key,as3);
            const scoped=scopedRef(name);
            if(__nfArrayIsArray(receiver)){
                if(name==='length')__nfArrayBound(value);
                else if(/^(?:0|[1-9]\d*)$/.test(name)&&Number(name)<4294967295&&Number(name)>=__nfArrayLimit)throw new __nfTypeError('Array index exceeds the 1000000-element profile.');
            }
            const write=next=>{if(scoped)return scoped.set(next);if((typeof receiver!=='object'&&typeof receiver!=='function')||!Reflect.set(object,name,next,receiver))throw new __nfTypeError('Cannot write this property.');return next;};
            return legacy?.objects?legacy.objects.set(receiver,name,value,()=>scoped?scoped.get():receiver[name],write):write(value);
        },
        delete(){const name=__nfLegacyKey(receiver,key,as3),scoped=scopedRef(name),removed=scoped?scoped.delete():Reflect.deleteProperty(object,name);if(removed)legacy?.objects?.deleted(receiver,name);return removed;},
        has(){const name=__nfLegacyKey(receiver,key,as3),scoped=scopedRef(name);return scoped?scoped.has():Reflect.has(object,name);},
    };
}
function __nfContainerMethod(receiver,key,as3,caller){
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.method(receiver,key,as3);
    if(__nfDictionaryIs(receiver)||(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver))){
        const method=__nfContainerRef(receiver,key,as3).get();
        return (...args)=>__nfApply(method,receiver,args);
    }
    key=__nfLegacyKey(receiver,key,as3);
    if(__nfArrayIsArray(receiver))return __nfArrayMethod(receiver,key,as3);
    if(typeof receiver==='string')return __nfStringMethod(receiver,key,as3);
    if(__nfDateIs(receiver))return __nfDateMethod(receiver,key,as3);
    if(as3&&key==='toFixed')return __nfFixedMethod(receiver);
    if(key==='toExponential'||key==='toPrecision')return __nfNumberFormatMethod(receiver,key,as3);
    if(as3&&typeof receiver==='number'&&(key==='toString'||key==='valueOf'))return __nfNumericMethod(receiver,key);
    const method=__nfContainerRef(receiver,key,as3,caller).get();
    if(key==='call'||key==='apply')return __nfFunctionMethod(receiver,key,as3,method);
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

// Rust's AS2 compiler lowers with into explicit reference selection. This bridge
// only queries live object references using the ordinary property adapters.
function __nfWithTarget(value){
    if(value===null||(typeof value!=='object'&&typeof value!=='function'))throw new __nfTypeError('AS2 with requires a live object reference in this profile.');
    return value;
}
function __nfWithRef(targets,name,fallback,receiver,caller){
    for(const target of targets){const ref=__nfContainerRef(target,name,false,caller);if(ref.has())return {...ref,receiver:target,name,caller,withTarget:true};}
    return {...fallback(),receiver,name,caller};
}
function __nfWithMethod(ref){
    if(ref.receiver!==undefined&&(ref.name==='gotoAndPlay'||ref.name==='gotoAndStop'))return __nfAs2Method(ref.receiver,ref.name);
    if(ref.withTarget){
        return __nfContainerMethod(ref.receiver,ref.name,false,ref.caller);
    }
    const method=ref.get();return __nfAs2Value(method,ref.receiver);
}
