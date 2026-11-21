// Shared reference/enumeration bridge. Expressions are evaluated once by the
// emitter; this helper preserves key identity before any ordinary String coercion.
// Ordinary enumeration is an explicit bounded JS-backed own/inherited-enumerable
// profile, not historical AVM iteration order. Fixed project traits are filtered
// by the program registry. Native objects with hidden storage diagnose.
const __nfContainerLimit=1000000;
// Runtime-owned native facades can opt into direct method lookup. A shared
// WeakSet keeps the brand invisible to ActionScript and avoids probing user
// Proxy objects with a private Symbol property.
const __nfDirectNativeMethods=(()=>{
    const key=Symbol.for('neoflash.actionscript.direct-native-methods.v1');
    const existing=globalThis[key];
    if(existing instanceof WeakSet)return existing;
    const registry=new WeakSet();
    Object.defineProperty(globalThis,key,{value:registry,configurable:true});
    return registry;
})();
function __nfContainerKey(key){
    if(typeof key==='symbol'||typeof key==='bigint')throw new __nfTypeError('ActionScript property keys do not support Symbol or BigInt in this profile.');
    const name=__nfString(key);if(name.length>__nfContainerLimit)throw new __nfTypeError('Property key exceeds 1000000 UTF-16 units.');return name;
}
function __nfLegacyKey(receiver,key,as3,legacyCase){
    const name=__nfContainerKey(key);
    const isLegacyCase=legacyCase===true||(legacyCase===undefined&&typeof __nfLegacyCase!=='undefined'&&Boolean(__nfLegacyCase));
    if(as3||!isLegacyCase)return name;
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
    const array=Array.isArray(receiver)&&prototype===Array.prototype;
    if(!array&&prototype!==null&&prototype!==Object.prototype)return {supported:false};
    const descriptor=Object.getOwnPropertyDescriptor(receiver,name);
    return {supported:true,fixed:array&&name==='length',own:!!descriptor,enumerable:!!descriptor?.enumerable,configurable:!!descriptor?.configurable};
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
function __nfFindNsSymbol(object,name){
    if(object===null||(typeof object!=='object'&&typeof object!=='function'))return null;
    const suffix='::'+name;
    try{
        for(const sym of Object.getOwnPropertySymbols(object)){
            const k=Symbol.keyFor(sym);
            if(k&&k.startsWith('neoflash.ns.')&&k.endsWith(suffix))return sym;
        }
    }catch(_){return null;}
    return null;
}
function __nfNamespaceUri(ns){
    if(ns==null)return '';
    if(typeof ns==='string')return ns;
    if(typeof ns==='object'&&typeof ns.uri==='string')return ns.uri;
    return __nfString(ns);
}
function __nfQualifiedRef(receiver,namespace,key,as3,caller){
    if(receiver==null)throw new __nfTypeError('Cannot access a qualified property on null or undefined.');
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver)){
        return __nfXML.ref(receiver,__nfXML.selector(key,namespace,false,false),as3);
    }
    const uri=__nfNamespaceUri(namespace);
    const name=__nfContainerKey(key);
    if(uri==='http://adobe.com/AS3/2006/builtin'||uri==='AS3'){
        return {
            get(){return __nfFamilyPropertyValue(receiver,name,true,caller);},
            set(v){return __nfContainerRef(receiver,name,true,caller).set(v);},
            delete(){return __nfContainerRef(receiver,name,true,caller).delete();},
            has(){return __nfContainerRef(receiver,name,true,caller).has();},
        };
    }
    const sym=Symbol.for('neoflash.ns.'+uri+'::'+name);
    const obj=Object(receiver);
    const program=__nfContainerProgram();
    const isProxy=__nfIsProxyReceiver(receiver,program);
    const makeProxyQName=()=>{
        if(typeof __nfXML!=='undefined'&&typeof __nfXML?.QName==='function')return new __nfXML.QName(uri,name);
        return Object.freeze({uri,localName:name,toString(){return uri?uri+'::'+name:name;},valueOf(){return name;}});
    };
    return {
        get(){
            if(typeof receiver==='object'||typeof receiver==='function'){
                const slot=program?.resolveQualifiedSlot?.(receiver,name,caller,uri);
                if(slot)return slot.get();
                if(sym in obj)return obj[sym];
                if(as3&&isProxy)return __nfInvokeProxyTrap(receiver,program,'getProperty',[makeProxyQName()]);
            }
            return __nfContainerRef(receiver,name,true,caller).get();
        },
        set(value){
            if(typeof receiver==='object'||typeof receiver==='function'){
                const slot=program?.resolveQualifiedSlot?.(receiver,name,caller,uri);
                if(slot){slot.set(value);return value;}
                if(sym in obj){obj[sym]=value;return value;}
                if(as3&&isProxy){__nfInvokeProxyTrap(receiver,program,'setProperty',[makeProxyQName(),value]);return value;}
                obj[sym]=value;
                return value;
            }
            return __nfContainerRef(receiver,name,true,caller).set(value);
        },
        delete(){
            if((typeof receiver==='object'||typeof receiver==='function')&&(sym in obj))return delete obj[sym];
            if(as3&&isProxy)return Boolean(__nfInvokeProxyTrap(receiver,program,'deleteProperty',[makeProxyQName()]));
            return __nfContainerRef(receiver,name,true,caller).delete();
        },
        has(){
            if((typeof receiver==='object'||typeof receiver==='function')&&(sym in obj))return true;
            if(program?.resolveQualifiedSlot?.(receiver,name,caller,uri))return true;
            if(as3&&isProxy)return Boolean(__nfInvokeProxyTrap(receiver,program,'hasProperty',[makeProxyQName()]));
            return __nfContainerRef(receiver,name,true,caller).has();
        },
    };
}
function __nfIsQNameKey(key){
    return key!==null&&typeof key==='object'&&((typeof __nfXML!=='undefined'&&__nfXML?.QName?.__nfIs?.(key))||(typeof key.localName==='string'&&typeof key.uri==='string'));
}
function __nfIsProxyReceiver(receiver,program){
    return Boolean(program?.isProxy?.(receiver)||(typeof __nfIsAs3Proxy==='function'&&__nfIsAs3Proxy(receiver)));
}
function __nfInvokeProxyTrap(receiver,program,trap,args){
    return program?.isProxy?.(receiver)?program.invokeProxy(receiver,trap,args):__nfInvokeStandaloneAs3Proxy(receiver,trap,args);
}
function __nfXmlOrProxySelectRef(receiver,makeSelector,namespace,local,attribute,descendant,as3){
    if(receiver!=null&&as3&&(typeof __nfXML==='undefined'||!__nfXML?.is?.(receiver))){
        const program=__nfContainerProgram();
        const isProxy=program?.isProxy?.(receiver)||(typeof __nfIsAs3Proxy==='function'&&__nfIsAs3Proxy(receiver));
        if(isProxy){
            const invoke=(trap,args)=>program?.isProxy?.(receiver)?program.invokeProxy(receiver,trap,args):__nfInvokeStandaloneAs3Proxy(receiver,trap,args);
            const uri=namespace===undefined?'':__nfNamespaceUri(namespace);
            const localName=__nfContainerKey(local);
            let keyObj;
            if(typeof __nfXML!=='undefined'&&typeof __nfXML?.QName==='function'){
                keyObj=new __nfXML.QName(uri,localName);
                if(attribute&&typeof __nfMarkProxyAttribute==='function')__nfMarkProxyAttribute(keyObj);
            }else{
                keyObj=Object.freeze({uri,localName,__nfAttribute:Boolean(attribute),isAttribute:Boolean(attribute),toString(){return localName;},valueOf(){return localName;}});
            }
            if(descendant){
                return {
                    get(){return invoke('getDescendants',[keyObj]);},
                    set(){throw new __nfTypeError('Cannot assign to descendant selector on Proxy.');},
                    delete(){return false;},
                    has(){return false;},
                };
            }
            return {
                get(){return invoke('getProperty',[keyObj]);},
                set(value){invoke('setProperty',[keyObj,value]);return value;},
                delete(){return Boolean(invoke('deleteProperty',[keyObj]));},
                has(){return Boolean(invoke('hasProperty',[keyObj]));},
            };
        }
    }
    return __nfXML.ref(receiver,makeSelector(),as3);
}
function __nfQualifiedMethod(receiver,namespace,key,as3,caller){
    if(receiver==null)throw new __nfTypeError('Cannot call a qualified method on null or undefined.');
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.method(receiver,key,as3);
    const uri=__nfNamespaceUri(namespace);
    const name=__nfContainerKey(key);
    if(uri==='http://adobe.com/AS3/2006/builtin'||uri==='AS3'){
        if(name==='call'||name==='apply')return __nfFunctionMethod(receiver,name,true);
        if(name==='hasOwnProperty'||name==='propertyIsEnumerable'||name==='setPropertyIsEnumerable')return __nfObjectPropertyMethod(receiver,name);
        if(name==='isPrototypeOf')return (...args)=>args.length>0&&args[0]!=null&&Object.prototype.isPrototypeOf.call(Object(receiver),Object(args[0]));
        return __nfContainerMethod(receiver,name,true,caller);
    }
    const program=__nfContainerProgram();
    const slot=program?.resolveQualifiedSlot?.(receiver,name,caller,uri);
    if(slot){
        const fn=slot.get();
        return (...args)=>__nfApply(fn,receiver,args);
    }
    const sym=Symbol.for('neoflash.ns.'+uri+'::'+name);
    const obj=Object(receiver);
    if((typeof receiver==='object'||typeof receiver==='function')&&typeof obj[sym]==='function'){
        return (...args)=>__nfApply(obj[sym],receiver,args);
    }
    const isProxy=__nfIsProxyReceiver(receiver,program);
    if((uri==='http://www.adobe.com/2006/actionscript/flash/proxy'||uri==='http://www.adobe.com/2006/flex/mx/internal')&&isProxy){
        return (...args)=>__nfInvokeProxyTrap(receiver,program,name,args);
    }
    if(as3&&isProxy){
        const qn=(typeof __nfXML!=='undefined'&&typeof __nfXML?.QName==='function')?new __nfXML.QName(uri,name):Object.freeze({uri,localName:name,toString(){return uri?uri+'::'+name:name;}});
        return (...args)=>__nfInvokeProxyTrap(receiver,program,'callProperty',[qn,...args]);
    }
    return __nfContainerMethod(receiver,name,true,caller);
}
function __nfContainerRef(receiver,key,as3,caller,legacyCase){
    if(receiver==null){
        if(as3)throw new __nfTypeError('Cannot access a property on null or undefined.');
        return {get(){return undefined;},set(value){return value;},delete(){return false;},has(){return false;}};
    }
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.ref(receiver,key,as3);
    if(__nfDictionaryIs(receiver)){if(!as3)throw new __nfTypeError('Dictionary access requires AS3.');return __nfDictionaryRef(receiver,key);}
    if(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver)){if(!as3)throw new __nfTypeError('Vector access requires AS3.');return __nfVectorRef(receiver,key);}
    const object=Object(receiver),program=__nfContainerProgram();
    if(as3&&__nfIsQNameKey(key)&&__nfIsProxyReceiver(receiver,program)){
        const qSlot=program?.resolveQualifiedSlot?.(receiver,key.localName,caller,key.uri||null);
        if(qSlot)return {get:()=>qSlot.get(),set(value){qSlot.set(value);return value;},delete:()=>false,has:()=>true};
        return {
            get:()=>__nfInvokeProxyTrap(receiver,program,'getProperty',[key]),
            set(value){__nfInvokeProxyTrap(receiver,program,'setProperty',[key,value]);return value;},
            delete:()=>Boolean(__nfInvokeProxyTrap(receiver,program,'deleteProperty',[key])),
            has:()=>Boolean(__nfInvokeProxyTrap(receiver,program,'hasProperty',[key])),
        };
    }
    const legacy=!as3&&typeof __nfLegacy!=='undefined'?__nfLegacy:null;
    const scopedRef=name=>legacy?.nativeRef?.(receiver,name)??program?.ref?.(receiver,name,caller);
    // Capture the key expression once, but retain its value: ordinary property
    // conversion belongs to each read/write operation, after the assignment RHS
    // has run. Compound updates can therefore convert an object key twice.
    return {
        get(){
            const name=__nfLegacyKey(receiver,key,as3,legacyCase);
            const arrayMember=as3&&__nfArrayIsArray(receiver),arrayPresent=arrayMember&&(name in object);
            if(as3&&name==='setPropertyIsEnumerable'&&!(name in object))return __nfObjectPropertyMethod(receiver,name);
            const scoped=scopedRef(name),legacyProto=!as3&&!scoped&&name==='__proto__'&&(typeof receiver==='object'||typeof receiver==='function')&&receiver!==null&&!Object.hasOwn(receiver,'__proto__');
            if(as3&&!scoped&&!(name in object)){const nsSym=__nfFindNsSymbol(object,name);if(nsSym)return object[nsSym];}
            const read=()=>scoped?scoped.get():legacyProto?(Object.getPrototypeOf(receiver)??undefined):receiver[name];
            const value=legacy?.objects?legacy.objects.get(receiver,name,read,()=>scoped?scoped.has():legacyProto?true:Reflect.has(object,name)):read();
            if(arrayMember){const reference=__nfArrayReference(receiver,name,value,true,arrayPresent);if(reference!==value)return reference;}
            if(as3&&typeof receiver==='number')return __nfNumericMethodValue(receiver,value);
            if(as3&&Object.hasOwn(__nfObjectPropertyMethods,name)&&value===__nfObjectPropertyMethods[name])return __nfObjectPropertyMethod(receiver,name);
            if(as3){const method=__nfObjectDefaultMethod(receiver,name,value,program);if(method)return method;}
            if(name==='constructor'&&typeof __nfNativeFunctionConstructor==='function'&&value===__nfNativeFunctionConstructor){
                return as3?__nfFunctionClass:(typeof legacy?.Function==='function'?legacy.Function:__nfFunctionClass);
            }
            if(!as3&&name==='constructor'){
                if(value===Boolean)return __nfBoolean2;
                if(value===String)return __nfStringClass(false);
                if(value===Number)return legacyCase?__nfNumber1:__nfNumber2;
                if(value===Object){
                    if(legacy?.boxing?.is?.(receiver)){
                        const kind=legacy.boxing.kind(receiver);
                        if(kind==='Boolean')return __nfBoolean2;
                        if(kind==='String')return __nfStringClass(false);
                        if(kind==='Number')return legacyCase?__nfNumber1:__nfNumber2;
                    }
                    return __nfObjectClass(false);
                }
            }
            // AVM1 exposes an absent native scrollRect as undefined; AS3 and
            // the public JS facade use null. Never rewrite ordinary user fields.
            return !as3&&name==='scrollRect'&&value===null&&typeof __nfDisplay!=='undefined'&&__nfDisplay?.alive(receiver)?undefined:value;
        },
        set(value){
            const name=__nfLegacyKey(receiver,key,as3,legacyCase);
            const scoped=scopedRef(name),legacyProto=!as3&&!scoped&&name==='__proto__'&&(typeof receiver==='object'||typeof receiver==='function')&&receiver!==null&&!Object.hasOwn(receiver,'__proto__');
            if(as3&&!scoped&&!(name in object)){const nsSym=__nfFindNsSymbol(object,name);if(nsSym){object[nsSym]=value;return value;}}
            if(__nfArrayIsArray(receiver)){
                if(name==='length')__nfArrayBound(value);
                else if(/^(?:0|[1-9]\d*)$/.test(name)&&Number(name)<4294967295&&Number(name)>=__nfArrayLimit)throw new __nfTypeError('Array index exceeds the 1000000-element profile.');
            }
            const write=next=>{
                if(scoped)return scoped.set(next);
                if(legacyProto){
                    if(next!==null&&typeof next!=='object'&&typeof next!=='function')throw new __nfTypeError('AS2 __proto__ requires an object, function or null.');
                    Object.setPrototypeOf(receiver,next);
                    return next;
                }
                if((typeof receiver!=='object'&&typeof receiver!=='function')||!Reflect.set(object,name,next,receiver))throw new __nfTypeError('Cannot write this property.');
                return next;
            };
            return legacy?.objects?legacy.objects.set(receiver,name,value,()=>scoped?scoped.get():legacyProto?(Object.getPrototypeOf(receiver)??undefined):receiver[name],write):write(value);
        },
        delete(){const name=__nfLegacyKey(receiver,key,as3,legacyCase),scoped=scopedRef(name);if(!as3&&!scoped){const hasOwn=typeof legacy?.hasOwnDeleteTarget==='function'?legacy.hasOwnDeleteTarget(receiver,name):Object.hasOwn(object,name);if(!hasOwn)return false;}if(as3&&!scoped&&!(name in object)){const nsSym=__nfFindNsSymbol(object,name);if(nsSym)return delete object[nsSym];}const removed=scoped?scoped.delete():Reflect.deleteProperty(object,name);if(removed)legacy?.objects?.deleted(receiver,name);return removed;},
        has(){const name=__nfLegacyKey(receiver,key,as3,legacyCase),scoped=scopedRef(name);if(as3&&!scoped&&!(name in object)&&__nfFindNsSymbol(object,name))return true;return scoped?scoped.has():Reflect.has(object,name);},
    };
}
function __nfContainerMethod(receiver,key,as3,caller,legacyCase){
    if(!as3&&receiver==null)return ()=>undefined;
    // Graphics and other explicitly branded host facades have already
    // validated their receiver and own method surface. Avoid probing every
    // XML/Dictionary/Vector/Array/String/Date family for each native call, but
    // retain the same synchronous lookup, receiver binding and error path.
    if(receiver!=null&&(typeof receiver==='object'||typeof receiver==='function')&&__nfDirectNativeMethods.has(receiver)){
        key=__nfLegacyKey(receiver,key,as3,legacyCase);
        const method=receiver[key];
        return (...args)=>__nfApply(method,receiver,args);
    }
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.method(receiver,key,as3);
    if(__nfDictionaryIs(receiver)||(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver))){
        const method=__nfContainerRef(receiver,key,as3).get();
        return (...args)=>__nfApply(method,receiver,args);
    }
    const program=__nfContainerProgram();
    if(as3&&__nfIsQNameKey(key)&&__nfIsProxyReceiver(receiver,program)){
        const qSlot=program?.resolveQualifiedSlot?.(receiver,key.localName,caller,key.uri||null);
        if(qSlot){
            const fn=qSlot.get();
            return (...args)=>__nfApply(fn,receiver,args);
        }
        return (...args)=>__nfInvokeProxyTrap(receiver,program,'callProperty',[key,...args]);
    }
    key=__nfLegacyKey(receiver,key,as3,legacyCase);
    if(__nfArrayIsArray(receiver)&&!(as3&&(key==='hasOwnProperty'||key==='propertyIsEnumerable'||key==='setPropertyIsEnumerable')))return __nfArrayMethod(receiver,key,as3,null,legacyCase);
    if(typeof receiver==='string')return __nfStringMethod(receiver,key,as3);
    if(__nfDateIs(receiver))return __nfDateMethod(receiver,key,as3);
    if(as3&&key==='toFixed')return __nfFixedMethod(receiver);
    if(key==='toExponential'||key==='toPrecision')return __nfNumberFormatMethod(receiver,key,as3);
    if(as3&&typeof receiver==='number'&&(key==='toString'||key==='valueOf'))return __nfNumericMethod(receiver,key);
    if(!as3&&typeof receiver==='number'&&key==='toString')return (...args)=>{if(!args.length||args[0]===undefined)return String(receiver);const n=Number(args[0]),base=Number.isFinite(n)?Math.trunc(n):10;return base>=2&&base<=36&&base!==10&&Number.isFinite(receiver)&&receiver!==0?__nfApply(__nfNativeNumberString,receiver,[base]):String(receiver);};
    if(as3&&program?.isProxy?.(receiver)&&!program?.hasTrait?.(receiver,key,caller)){
        return (...args)=>program.invokeProxy(receiver,'callProperty',[key,...args]);
    }
    if(as3&&typeof __nfIsAs3Proxy==='function'&&__nfIsAs3Proxy(receiver)&&!program?.isProxy?.(receiver)&&!__nfHasStandaloneAs3ProxyTrait(receiver,key)){
        return (...args)=>__nfInvokeStandaloneAs3Proxy(receiver,'callProperty',[key,...args]);
    }
    const method=__nfContainerRef(receiver,key,as3,caller,legacyCase).get();
    if(key==='call'||key==='apply')return __nfFunctionMethod(receiver,key,as3,method);
    return (...args)=>__nfApply(method,receiver,args);
}
function __nfEnumerate(receiver,values,as3,legacyCase){
    if(receiver==null)return [][Symbol.iterator]();
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(receiver))return __nfXML.enumerate(receiver,values,as3);
    if(__nfDictionaryIs(receiver)){if(!as3)throw new __nfTypeError('Dictionary enumeration requires AS3.');return __nfDictionaryEnumerate(receiver,values);}
    if(typeof __nfVectorIs==='function'&&__nfVectorIs(receiver)){
        if(!as3)throw new __nfTypeError('Vector enumeration requires AS3.');
        const keys=__nfVectorKeys(receiver);if(keys.length>__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');
        return (function*(){for(const key of keys){const ref=__nfVectorRef(receiver,key);if(ref.has())yield values?ref.get():key;}})();
    }
    const program=__nfContainerProgram();
    if(!as3){
        if(typeof receiver!=='object'&&typeof receiver!=='function')return [][Symbol.iterator]();
        if(receiver[Symbol.toStringTag]==='NeoFlashDisplayObject'){
            const legacy=typeof __nfLegacy!=='undefined'?__nfLegacy:null;
            const keys=typeof legacy?.enumerateKeys==='function'?legacy.enumerateKeys(receiver):[];
            if(keys.length>__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');
            return (function*(){for(const key of keys){const ref=__nfContainerRef(receiver,key,false,undefined,legacyCase);if(ref.has())yield values?ref.get():key;}})();
        }
        const fixed=program?.enumerationKeys?.(receiver)??null;
        let keys=fixed;
        if(keys===null){keys=[];for(const key in receiver){if(keys.length>=__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');keys.push(key);}}
        if(keys.length>__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');
        return (function*(){for(const key of keys){const ref=__nfContainerRef(receiver,key,false,undefined,legacyCase);if(ref.has())yield values?ref.get():key;}})();
    }
    if(as3&&(program?.isProxy?.(receiver)||(typeof __nfIsAs3Proxy==='function'&&__nfIsAs3Proxy(receiver)))){
        const invoke=(trap,args)=>program?.isProxy?.(receiver)?program.invokeProxy(receiver,trap,args):__nfInvokeStandaloneAs3Proxy(receiver,trap,args);
        return (function*(){
            let index=0,steps=0;
            while(true){
                if(++steps>__nfContainerLimit)throw new __nfTypeError('Proxy enumeration exceeds 1000000 entries.');
                index=__nfNumber(invoke('nextNameIndex',[index]))|0;
                if(index<=0)break;
                yield values?invoke('nextValue',[index]):__nfString(invoke('nextName',[index]));
            }
        })();
    }
    if(typeof receiver!=='object'||receiver[Symbol.toStringTag]==='NeoFlashDisplayObject')throw new __nfTypeError('Enumeration currently supports arrays, plain records and ordinary project objects; hidden native/display storage is unsupported.');
    const fixed=program?.enumerationKeys?.(receiver)??null;
    const prototype=Object.getPrototypeOf(receiver);
    if(!Array.isArray(receiver)&&fixed===null&&prototype!==null&&prototype!==Object.prototype)throw new __nfTypeError('Enumeration of this native or foreign object is unsupported; copy its public values into a plain record.');
    let keys=fixed;
    if(keys===null){keys=[];for(const key in receiver){if(keys.length>=__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');keys.push(key);}}
    if(keys.length>__nfContainerLimit)throw new __nfTypeError('Enumeration exceeds 1000000 entries.');
    return (function*(){for(const key of keys){const ref=__nfContainerRef(receiver,key,as3,undefined,legacyCase);if(ref.has())yield values?ref.get():key;}})();
}

// Rust's AS2/AS3 compiler lowers with into explicit reference selection. This bridge
// queries live object and XML references using the ordinary property adapters.
function __nfWithTarget(value,as3){
    if(value===null||value===undefined)throw new __nfTypeError('Cannot use null or undefined with the with statement.');
    if(!as3&&typeof value!=='object'&&typeof value!=='function')throw new __nfTypeError('AS2 with requires a live object reference in this profile.');
    return Object(value);
}
function __nfWithXmlAttr(targets,fallbackThis,selector,namespace,local,attribute,descendant,caller){
    for(const target of targets){
        if(typeof __nfXML!=='undefined'&&__nfXML?.is(target))return __nfXML.ref(target,selector,true);
    }
    if(typeof __nfXML!=='undefined'&&__nfXML?.is(fallbackThis))return __nfXML.ref(fallbackThis,selector,true);
    if(!attribute&&!descendant&&namespace!==undefined)return __nfQualifiedRef(fallbackThis,namespace,local,true,caller);
    throw new __nfTypeError('E4X '+(descendant?'descendant ..':'attribute @')+' selector requires an enclosing XML with/filter target or XML receiver.');
}
function __nfWithRef(targets,name,fallback,receiver,as3,caller,legacyCase){
    let firstXmlRef=null;
    for(const target of targets){
        if(as3&&typeof __nfXML!=='undefined'&&__nfXML?.is(target)){
            if(name in target&&typeof target[name]==='function'){
                const fn=target[name].bind(target);
                return {get:()=>fn,set(){throw new __nfTypeError('Cannot overwrite XML method '+name);},delete:()=>false,has:()=>true,receiver:target,name,as3:true,caller,legacyCase,withTarget:true};
            }
            const xmlRef=__nfXML.ref(target,__nfXML.selector(name,undefined,false,false),true);
            const list=xmlRef.get();
            if(list&&typeof list.length==='function'&&list.length()>0){
                return {...xmlRef,receiver:target,name,as3:true,caller,legacyCase,withTarget:true};
            }
            if(!firstXmlRef)firstXmlRef={...xmlRef,receiver:target,name,as3:true,caller,legacyCase,withTarget:true};
            continue;
        }
        const ref=__nfContainerRef(target,name,Boolean(as3),caller,legacyCase);
        if(ref.has())return {...ref,receiver:target,name,as3:Boolean(as3),caller,legacyCase,withTarget:true};
    }
    const fb=fallback();
    if(firstXmlRef&&(typeof fb.has!=='function'||!fb.has()))return firstXmlRef;
    return {...fb,receiver,name,as3:Boolean(as3),caller,legacyCase};
}
function __nfWithMethod(ref){
    if(ref.as3){
        if(ref.withTarget)return __nfContainerMethod(ref.receiver,ref.name,true,ref.caller);
        const method=ref.get();
        return (...args)=>__nfApply(method,ref.receiver,args);
    }
    const methodKey=ref.legacyCase&&ref.receiver!=null?__nfLegacyKey(ref.receiver,ref.name,false,true):ref.name;
    if(ref.receiver!==undefined&&(methodKey==='gotoAndPlay'||methodKey==='gotoAndStop'||methodKey==='nextScene'||methodKey==='prevScene'))return __nfAs2Method(ref.receiver,methodKey,Boolean(ref.legacyCase));
    if(ref.withTarget){
        return __nfContainerMethod(ref.receiver,ref.name,false,ref.caller,ref.legacyCase);
    }
    const method=ref.get();return __nfAs2Value(method,ref.receiver);
}
function __nfTellTarget(frame,value,baseFrame,legacyCase){
    if(value===''||value===null||value===undefined)return baseFrame||frame;
    if(typeof value==='string'){
        if(typeof __nfLegacy?.resolveTarget!=='function')throw new __nfTypeError('AS2 tellTarget string paths require the publication display host.');
        const resolved=__nfLegacy.resolveTarget(value,frame,Boolean(legacyCase));
        if(!resolved)throw new __nfTypeError(`AS2 tellTarget target '${value}' was not found.`);
        return resolved;
    }
    if(typeof value==='object'||typeof value==='function')return value;
    throw new __nfTypeError('AS2 tellTarget requires a MovieClip path string or live object reference.');
}
function __nfRunInitClip(key,root,fn){
    if(typeof __nfLegacy?.runInitClip==='function')return __nfLegacy.runInitClip(key,root,fn);
    if(!__nfRunInitClip.seen)__nfRunInitClip.seen=new Set();
    if(__nfRunInitClip.seen.has(key))return;
    __nfRunInitClip.seen.add(key);
    fn(root);
}
function __nfVarScrollRef(value,owner,varName,prop,caller,legacyCase){
    if(value!==null&&(typeof value==='object'||typeof value==='function'))return __nfContainerRef(value,prop,false,caller,legacyCase);
    const bound=__nfLegacy?.objects?.variableScrollRef?.(owner,varName,prop);
    if(bound)return bound;
    return __nfContainerRef(value,prop,false,caller,legacyCase);
}
