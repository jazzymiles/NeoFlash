// Bounded reference profiles, not a complete AVM. Helpers are embedded in each
// compiled AS handler; JavaScript/TypeScript globals and prototypes are untouched.
// AS2 details: Ruffle AVM1 globals.rs at fe6cfc545405b888ebb478ce1c211d63147ca49b.
// AS3 precision conversion: Adobe avmplus Number.as / NumberClass.cpp at
// 858d034a3bd3a54d9b70909386435cf4aec81d21. See intrinsic-fixtures.mjs for provenance.
const __nfNativeParseInt = parseInt;
const __nfNativeFixed = Number.prototype.toFixed;
const __nfNativeExponential = Number.prototype.toExponential;
const __nfNativePrecision = Number.prototype.toPrecision;
const __nfNativeNumberString = Number.prototype.toString;
const __nfNativeNumberValue = Number.prototype.valueOf;
const __nfApply = Reflect.apply;
const __nfNumericBrand = Symbol.for('neoflash.as3.numeric.class.v1');
const __nfNumericNegativeZero = Symbol.for('neoflash.as3.numeric.negative-zero');
const __nfLocalNumericState = {classes:new Map(),methods:new Map()};
function __nfNumericState() {
    return typeof __nfLegacy !== 'undefined' && __nfLegacy?.math?.numericState || __nfLocalNumericState;
}
function __nfNumericClassName(value) {
    if (typeof value !== 'function') return null;
    const descriptor = Object.getOwnPropertyDescriptor(value, __nfNumericBrand);
    return descriptor && ['Number','int','uint','Boolean'].includes(descriptor.value) ? descriptor.value : null;
}
function __nfNumericCreate(name, args) {
    if (args.length > 1) throw new __nfTypeError('AS3 '+name+' conversion/construction currently accepts zero or one argument.');
    return __nfScalarCreate(name, args);
}
function __nfNumericClass(name) {
    const state = __nfNumericState();
    if (!state.classes.has(name)) {
        if (!['Number','int','uint','Boolean'].includes(name)) throw new __nfTypeError('Unsupported numeric class '+name);
        // Construction is lowered through __nfClassConstruct so that aliases
        // return AS3 scalar values, never JavaScript primitive wrapper objects.
        const facade = (...args) => __nfNumericCreate(name,args);
        const constants = name === 'Number' ? {MAX_VALUE:Number.MAX_VALUE,MIN_VALUE:Number.MIN_VALUE,NaN,NEGATIVE_INFINITY:-Infinity,POSITIVE_INFINITY:Infinity}
            : name === 'int' ? {MIN_VALUE:-2147483648,MAX_VALUE:2147483647}
            : name === 'uint' ? {MIN_VALUE:0,MAX_VALUE:4294967295} : {};
        for (const [key,value] of Object.entries(constants)) Object.defineProperty(facade,key,{value});
        Object.defineProperties(facade, {
            [__nfNumericBrand]:{value:name},name:{value:name},length:{value:1},
            prototype:{get(){throw new __nfTypeError('AS3 '+name+'.prototype mutation and prototype method borrowing are outside the numeric class profile.');}},
            __nfIs:{value:value=>name==='Boolean'?typeof value==='boolean':typeof value==='number'&&(name==='Number'||(Number.isInteger(value)&&value>=(name==='int'?-2147483648:0)&&value<=(name==='int'?2147483647:4294967295)))},
        });
        state.classes.set(name,Object.freeze(facade));
    }
    return state.classes.get(name);
}
function __nfClassIs(value) {
    return __nfNumericClassName(value)!==null || (typeof __nfProgram!=='undefined' && __nfProgram?.isClass?.(value)===true);
}
function __nfClassCast(value) {
    if(value==null)return null;
    if(__nfClassIs(value))return value;
    throw new __nfTypeError('AS3 Class requires a supported built-in or registered project class, not an arbitrary Function.');
}
function __nfClassConstruct(value,args) {
    const name=__nfNumericClassName(value);
    if(name)return __nfNumericCreate(name,args);
    if(typeof __nfProgram!=='undefined' && __nfProgram?.isClass?.(value)===true)return __nfProgram.constructClass(value,args);
    throw new __nfTypeError('Constructor alias must reference a supported AS3 class; arbitrary Function construction is unsupported.');
}
function __nfNumericMethod(receiver,name) {
    if(name==='toFixed')return __nfFixedMethod(receiver);
    if(name==='toExponential'||name==='toPrecision')return __nfNumberFormatMethod(receiver,name,true);
    // Keep the existing native toString/valueOf calculation and argument
    // profile. This adapter only retains the numeric receiver for extraction;
    // it does not introduce a new radix algorithm or claim AVM formatting parity.
    const method=name==='valueOf'?__nfNativeNumberValue:__nfNativeNumberString;
    return (...args)=>__nfApply(method,receiver,args);
}
function __nfNumericMethodValue(receiver,value) {
    if(typeof receiver!=='number')return value;
    const names = [[__nfNativeFixed,'toFixed'],[__nfNativeExponential,'toExponential'],[__nfNativePrecision,'toPrecision'],[__nfNativeNumberString,'toString'],[__nfNativeNumberValue,'valueOf']];
    const name=names.find(([method])=>method===value)?.[1];
    if(!name)return value;
    const methods=__nfNumericState().methods,key=Object.is(receiver,-0)?__nfNumericNegativeZero:receiver;
    let cached=methods.get(key);
    if(!cached){
        if(methods.size>=16384)throw new __nfTypeError('Numeric method references exceed 16384 distinct receiver values in this publication.');
        cached=new Map();methods.set(key,cached);
    }
    if(!cached.has(name)){
        const method=__nfNumericMethod(receiver,name);
        Object.defineProperties(method,{length:{value:name==='valueOf'?0:1},name:{value:name}});
        cached.set(name,method);
    }
    return cached.get(name);
}
// Per-publication Rust service; there is deliberately no JavaScript fallback.
function __nfAs2Strings() {
    if (typeof __nfLegacy === 'undefined' || typeof __nfLegacy?.strings?.invoke !== 'function') {
        throw new __nfTypeError('AS2 String Rust service is unavailable. Re-export this project with the current WASM runtime.');
    }
    return __nfLegacy.strings;
}
function __nfEscape2(...args) { return __nfAs2Strings().invoke('escape', '', args); }
function __nfUnescape2(...args) { return __nfAs2Strings().invoke('unescape', '', args); }
function __nfAs3Strings() {
    if (typeof __nfLegacy === 'undefined' || typeof __nfLegacy?.strings3?.invoke !== 'function') {
        throw new __nfTypeError('AS3 String Rust service is unavailable. Re-export this project with the current WASM runtime.');
    }
    return __nfLegacy.strings3;
}
function __nfMath(as3) {
    if (typeof __nfLegacy === 'undefined' || !__nfLegacy?.math) {
        throw new __nfTypeError('Rust Math service is unavailable. Re-export this project.');
    }
    return as3 ? __nfLegacy.math.as3 : __nfLegacy.math.as2;
}
const __nfLegacyStringGlobals = Object.freeze(Object.fromEntries(
    ['chr', 'ord', 'length', 'substring', 'mbchr', 'mbord', 'mblength', 'mbsubstring']
        .map(name => [name, (...args) => __nfAs2Strings().invoke('global.' + name, '', args)])
));
function __nfMethodValue(value) {
    // A computed property name can select the native method without any
    // statically recognizable "toFixed" token. Check the resulting identity,
    // not the key/name: custom functions and fields remain ordinary values.
    if (value === __nfNativeFixed) {
        throw new __nfErrorTypes.Error('Unsupported AS3 Number.toFixed method value: method-closure semantics are not implemented; call value.toFixed(precision) directly.');
    }
    if (value === __nfNativeExponential || value === __nfNativePrecision) {
        const name = value === __nfNativeExponential ? 'toExponential' : 'toPrecision';
        throw new __nfErrorTypes.Error('Unsupported AS3 Number.' + name + ' method value: call the method directly on the number.');
    }
    return value;
}
function __nfPropertyValue(receiver, key) {
    // Preserve native computed-key conversion, getter receiver and exceptions.
    return __nfMethodValue(receiver[key]);
}
function __nfParseInt2(...args) { return __nfAs2Strings().invoke('global.parseInt', '', args); }
function __nfLegacyStringOperator(op, left, right) { return __nfAs2Strings().invoke('operator.' + op, '', [left, right]); }
function __nfParseInt3(value, radix) {
    // AS3 has decimal default radix; no AVM1 automatic octal or signed-hex quirks.
    // The pinned VM's native signature is String="NaN", int=0. Explicit
    // undefined is coerced to null by String's typed parameter, then read as
    // "null" by Toplevel::parseInt. This matters at high radices.
    if (arguments.length > 2) {
        throw new __nfErrorTypes.Error('Unsupported AS3 parseInt call: excess-argument semantics are not verified; pass at most two arguments.');
    }
    const text = arguments.length === 0 ? 'NaN' : value == null ? 'null' : __nfString(value);
    return __nfNativeParseInt(text, __nfNumber(radix) | 0);
}
function __nfFixedMethod(receiver) {
    // Resolve the property before evaluating arguments. Custom user methods
    // keep their own receiver and implementation; no global prototype patch.
    const method = receiver.toFixed;
    if (typeof receiver !== 'number') {
        return (...args) => __nfApply(method, receiver, args);
    }
    return (...args) => {
        if (args.length > 1) {
            throw new __nfErrorTypes.Error('Unsupported AS3 Number.toFixed call: excess-argument semantics are not verified; pass at most one argument.');
        }
        const digits = __nfNumber(args.length ? args[0] : 0) | 0;
        if (digits < 0 || digits > 20) {
            throw __nfMakeError('RangeError', true, ['Number.toFixed precision must be between 0 and 20.', 1002]);
        }
        // Native JS switches to exponential notation at 1e21; AS3's fixed mode
        // emits the full integral value. Such IEEE doubles have no fraction.
        if (Number.isFinite(receiver) && Math.abs(receiver) >= 1e21) {
            return BigInt(receiver).toString() + (digits ? '.' + '0'.repeat(digits) : '');
        }
        return __nfApply(__nfNativeFixed, receiver, [digits]);
    };
}

function __nfBoxCreate(name, args) {
    if (typeof __nfLegacy === 'undefined' || typeof __nfLegacy?.boxing?.create !== 'function') throw new __nfTypeError('AS2 primitive boxes require the current Rust runtime; re-export this project.');
    return __nfLegacy.boxing.create(name, args);
}

// AS3 constructors return scalar values; JavaScript's `new Number/Boolean`
// would instead produce wrapper objects with different truth/type behavior.
function __nfScalarCreate(name, args) {
    const value = args.length ? args[0] : 0;
    switch (name) {
        case 'Number': return __nfNumber(value);
        case 'Boolean': return __nfBoolean(value);
        case 'int': return __nfNumber(value) | 0;
        case 'uint': return __nfNumber(value) >>> 0;
    }
    throw new __nfTypeError('Unsupported scalar constructor ' + name);
}
const __nfObjectClasses = new Map();
function __nfObjectCreate(as3, args, isNew = false) {
    const value = args[0];
    // AVM2 Object construction/calls preserve non-null values, including
    // primitives. AVM1 needs boxed primitive identity; diagnose that boundary.
    if (value == null) return !as3 && !isNew ? Object.create(null) : {};
    if (as3 || typeof value === 'object' || typeof value === 'function') return value;
    throw new __nfTypeError('AS2 Object primitive boxing is not implemented; use an object literal or pass an existing object.');
}
function __nfObjectClass(as3) {
    if (!__nfObjectClasses.has(as3)) {
        const facade = function(...args) {
            return __nfObjectCreate(as3, args, Boolean(new.target));
        };
        Object.defineProperty(facade, 'prototype', { value: Object.prototype, writable: false });
        Object.defineProperty(facade, '__nfIs', { value: value => value != null });
        __nfObjectClasses.set(as3, Object.freeze(facade));
    }
    return __nfObjectClasses.get(as3);
}

function __nfNumberFormatMethod(receiver, name, as3) {
    const method = receiver[name];
    if (typeof receiver !== 'number') return (...args) => __nfApply(method, receiver, args);
    if (!as3) throw new __nfTypeError(name + ' belongs to the AS3 Number family; AS2 supports numeric toString/valueOf.');
    return (...args) => {
        if (args.length > 1) throw new __nfTypeError('Unsupported AS3 Number.' + name + ' call: pass at most one precision argument.');
        // The pinned AVM2 Number.as defaults p to zero. Only an explicitly
        // supplied null/undefined takes toPrecision's ordinary-string branch.
        if (name === 'toPrecision' && args.length && args[0] == null) return __nfString(receiver);
        const digits = __nfNumber(args.length ? args[0] : 0) | 0;
        const minimum = name === 'toPrecision' ? 1 : 0;
        const maximum = name === 'toPrecision' ? 21 : 20;
        if (digits < minimum || digits > maximum) {
            throw __nfMakeError('RangeError', true, ['Number.' + name + ' precision must be between ' + minimum + ' and ' + maximum + '.', 1002]);
        }
        return __nfApply(name === 'toPrecision' ? __nfNativePrecision : __nfNativeExponential, receiver, [digits]);
    };
}

// Argument conversion and transport live in the per-publication String service.
// The URI/UTF-16 codecs and XML-name validation execute in Rust.
const __nfUriFunctions = Object.freeze(Object.fromEntries(
    ['encodeURI','encodeURIComponent','decodeURI','decodeURIComponent','escape','unescape','isXMLName']
        .map(name => [name, (...args) => __nfAs3Strings().invoke(name, '', args)])
));

// AVM1 range conversion and random fraction quantization live in Rust.
function __nfRandom2(...args) { __nfMath(false); return __nfLegacy.math.randomInt(...args); }

// AVM1 Boolean() differs from Boolean(undefined): only omission is undefined.
function __nfBoolean2(value) {
    if (new.target) throw new __nfTypeError('AS2 boxed Boolean construction is not implemented; use Boolean(value) for a primitive.');
    return arguments.length ? __nfBoolean(value) : undefined;
}
