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
const __nfClassMetaclass = (() => {
    const facade = function(...args) {
        if (new.target) throw new __nfTypeError('Class is not constructible.');
        if (args.length !== 1) throw __nfMakeError('ArgumentError', true, ['Class() requires exactly 1 argument.', 1112]);
        return __nfClassCast(args[0]);
    };
    Object.defineProperties(facade, {
        name: { value: 'Class' },
        length: { value: 1 },
        prototype: { value: Object.prototype, writable: false },
        __nfIs: { value: value => __nfClassIs(value) },
        [Symbol.hasInstance]: { value: value => __nfClassIs(value) },
    });
    return Object.freeze(facade);
})();
const __nfFunctionClass = (() => {
    const facade = function(...args) {
        if (args.length > 1) throw __nfMakeError('ArgumentError', true, ['Function() accepts at most 1 argument.', 1063]);
        if (args.length === 0) return __nfSourceFunction(function() {}, { as3: true, length: 0, required: 0, allowExtra: false });
        const v = args[0];
        if (typeof v === 'function' && __nfNumericClassName(v) === null) return v;
        throw new __nfTypeError('Cannot convert value to Function.');
    };
    Object.defineProperties(facade, {
        name: { value: 'Function' },
        length: { value: 1 },
        prototype: { value: Function.prototype, writable: false },
        __nfIs: { value: value => typeof value === 'function' && __nfNumericClassName(value) === null },
        [Symbol.hasInstance]: { value: value => typeof value === 'function' && __nfNumericClassName(value) === null },
    });
    return Object.freeze(facade);
})();
function __nfClassIs(value) {
    if (typeof value !== 'function') return false;
    if (__nfNumericClassName(value) !== null) return true;
    if (value === __nfClassMetaclass || value === __nfFunctionClass || value === __nfObjectClass(true) || value === __nfStringClass(true) || value === __nfArrayClass(true) || value === __nfDateClass(true) || value === __nfRegExpClass(true)) return true;
    if (typeof __nfDictionaryClass === 'function' && value === __nfDictionaryClass(true)) return true;
    if (typeof value.__nfElement === 'object' && value.__nfElement !== null) return true;
    if (typeof __nfXML !== 'undefined' && __nfXML && (value === __nfXML.XML || value === __nfXML.XMLList || value === __nfXML.Namespace || value === __nfXML.QName)) return true;
    if (typeof value.name === 'string' && Object.hasOwn(__nfErrorTypes, value.name) && value === __nfErrorClass(value.name, true)) return true;
    return typeof __nfProgram !== 'undefined' && __nfProgram?.isClass?.(value) === true;
}
function __nfClassCast(value) {
    if(value==null)return null;
    if(__nfClassIs(value))return value;
    throw new __nfTypeError('AS3 Class requires a supported built-in or registered project class, not an arbitrary Function.');
}
function __nfClassConstruct(value,args) {
    const name=__nfNumericClassName(value);
    if(name)return __nfNumericCreate(name,args);
    if(value===__nfClassMetaclass)throw new __nfTypeError('Class is not constructible.');
    if(value===__nfFunctionClass)return __nfFunctionClass(...args);
    if(value===__nfObjectClass(true))return __nfObjectCreate(true,args,true);
    if(value===__nfStringClass(true))return __nfStringCreate(true,args,true);
    if(value===__nfArrayClass(true))return __nfArrayCreate(true,args);
    if(value===__nfDateClass(true))return new (__nfDateClass(true))(...args);
    if(value===__nfRegExpClass(true))return __nfRegExpCreate(true,args);
    if(typeof __nfDictionaryClass==='function'&&value===__nfDictionaryClass(true))return new (__nfDictionaryClass(true))(...args);
    if(typeof value?.__nfElement==='object'&&value.__nfElement!==null)return new value(...args);
    if(typeof __nfXML!=='undefined'&&__nfXML&&(value===__nfXML.XML||value===__nfXML.XMLList||value===__nfXML.Namespace||value===__nfXML.QName))return new value(...args);
    if(typeof value?.name==='string'&&Object.hasOwn(__nfErrorTypes,value.name)&&value===__nfErrorClass(value.name,true))return __nfMakeError(value.name,true,args);
    if(typeof __nfProgram!=='undefined' && __nfProgram?.isClass?.(value)===true)return __nfProgram.constructClass(value,args);
    throw new __nfTypeError('Constructor alias must reference a supported AS3 class; arbitrary Function construction is unsupported.');
}
function __nfAs3InstanceOf(value, ctor) {
    if (typeof ctor !== 'function') throw new __nfTypeError('Right-hand side of instanceof must be a class or function.');
    if (typeof __nfProgram !== 'undefined' && __nfProgram?.isInterface?.(ctor)) {
        throw new __nfTypeError('Right-hand side of instanceof cannot be an interface.');
    }
    if (value === null || value === undefined) return false;
    if (typeof value !== 'object' && typeof value !== 'function') {
        if (typeof value === 'string' && (ctor === __nfStringClass(true) || ctor === String || ctor === __nfObjectClass(true) || ctor === Object)) return true;
        return false;
    }
    if (ctor === __nfObjectClass(true) || ctor === Object) return true;
    if (ctor === __nfFunctionClass || ctor === Function) return typeof value === 'function' && __nfNumericClassName(value) === null;
    if (ctor === __nfArrayClass(true) || ctor === Array) return __nfArrayIsArray(value);
    if (typeof ctor.__nfIs === 'function') return Boolean(ctor.__nfIs(value));
    if (typeof __nfProgram !== 'undefined' && typeof __nfProgram?.className === 'function') {
        const qname = __nfProgram.className(ctor);
        if (qname) return __nfProgram.is(value, qname);
    }
    const proto = ctor.prototype;
    if (proto && (typeof proto === 'object' || typeof proto === 'function')) {
        let cur = Object.getPrototypeOf(value);
        while (cur) {
            if (cur === proto) return true;
            cur = Object.getPrototypeOf(cur);
        }
    }
    return value instanceof ctor;
}
const __nfLegacySuperStack = new WeakMap();
function __nfPushLegacySuper(receiver, proto) {
    let stack = __nfLegacySuperStack.get(receiver);
    if (!stack) { stack = []; __nfLegacySuperStack.set(receiver, stack); }
    stack.push(proto);
}
function __nfPopLegacySuper(receiver) {
    const stack = __nfLegacySuperStack.get(receiver);
    if (!stack) return;
    stack.pop();
    if (!stack.length) __nfLegacySuperStack.delete(receiver);
}
function __nfLegacyBaseProto(receiver) {
    const stack = __nfLegacySuperStack.get(receiver);
    return stack && stack.length ? stack[stack.length - 1] : Object.getPrototypeOf(receiver);
}
function __nfLegacySuperProto(receiver) {
    const base = __nfLegacyBaseProto(receiver);
    return base && (typeof base === 'object' || typeof base === 'function') ? Object.getPrototypeOf(base) : null;
}
function __nfLegacyConstruct(ctor, args) {
    if (typeof ctor !== 'function') throw new __nfTypeError('AS1 constructor must resolve to a function.');
    if (typeof __nfProgram !== 'undefined' && __nfProgram?.isClass?.(ctor) === true) return __nfProgram.constructClass(ctor, args);
    if (__nfFunctionRecord(ctor)) {
        if (!ctor.prototype || (typeof ctor.prototype !== 'object' && typeof ctor.prototype !== 'function')) {
            ctor.prototype = Object.create(Object.prototype);
            __nfDefineProperty(ctor.prototype, 'constructor', { configurable: true, writable: true, enumerable: false, value: ctor });
        }
        const proto = ctor.prototype;
        const receiver = Object.create(proto);
        __nfDefineProperty(receiver, '__constructor__', { configurable: true, writable: true, enumerable: false, value: ctor });
        __nfDefineProperty(receiver, 'constructor', { configurable: true, writable: true, enumerable: false, value: ctor });
        __nfPushLegacySuper(receiver, proto);
        try { __nfApply(ctor, receiver, args); }
        finally { __nfPopLegacySuper(receiver); }
        return receiver;
    }
    if (ctor === __nfObjectClass(false)) return __nfObjectCreate(false, args, true);
    if (ctor === __nfArrayClass(false)) return __nfArrayCreate(false, args);
    if (ctor === __nfStringClass(false)) return __nfStringCreate(false, args, true);
    if (ctor === __nfDateClass(false)) return new (__nfDateClass(false))(...args);
    return Reflect.construct(ctor, args);
}
function __nfLegacyInstanceOf(value, ctor) {
    if (value === null || (typeof value !== 'object' && typeof value !== 'function') || typeof ctor !== 'function') return false;
    if (typeof ctor.__nfIs === 'function' && ctor.__nfIs(value)) return true;
    if (typeof __nfProgram !== 'undefined' && typeof __nfProgram?.className === 'function') {
        const qname = __nfProgram.className(ctor);
        if (qname) return __nfProgram.is(value, qname);
    }
    const proto = ctor.prototype;
    if (proto && (typeof proto === 'object' || typeof proto === 'function')) {
        let cur = Object.getPrototypeOf(value);
        while (cur) {
            if (cur === proto) return true;
            cur = Object.getPrototypeOf(cur);
        }
    }
    return value instanceof ctor;
}
function __nfLegacyCast(ctor, value) {
    if (value === null || value === undefined || (typeof value !== 'object' && typeof value !== 'function')) return null;
    if (typeof ctor?.__nfIs === 'function' && ctor.__nfIs(value)) return value;
    return __nfLegacyInstanceOf(value, ctor) ? value : null;
}
function __nfLegacySuperConstruct(receiver, args) {
    if (receiver == null || (typeof receiver !== 'object' && typeof receiver !== 'function')) return undefined;
    const base = __nfLegacyBaseProto(receiver);
    const superProto = base && (typeof base === 'object' || typeof base === 'function') ? Object.getPrototypeOf(base) : null;
    if (!superProto && !(base && Object.hasOwn(base, '__constructor__'))) return undefined;
    const superCtor = base && Object.hasOwn(base, '__constructor__')
        ? base.__constructor__
        : superProto && Object.hasOwn(superProto, '__constructor__')
            ? superProto.__constructor__
            : superProto?.constructor;
    if (typeof superCtor !== 'function' || superCtor === Object) return undefined;
    __nfPushLegacySuper(receiver, superProto);
    try { return __nfApply(superCtor, receiver, args); }
    finally { __nfPopLegacySuper(receiver); }
}
function __nfLegacySuperMethod(receiver, name, legacyCase) {
    if (receiver == null || (typeof receiver !== 'object' && typeof receiver !== 'function')) {
        throw new __nfTypeError('Cannot call super method without an object receiver.');
    }
    const superProto = __nfLegacySuperProto(receiver);
    if (!superProto) return () => undefined;
    const key = __nfLegacyKey(superProto, name, false, legacyCase);
    const fn = superProto[key];
    if (typeof fn !== 'function') throw new __nfTypeError(`Undefined super method ${key}`);
    return (...args) => {
        __nfPushLegacySuper(receiver, superProto);
        try { return __nfApply(fn, receiver, args); }
        finally { __nfPopLegacySuper(receiver); }
    };
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
        throw __nfMakeError('ArgumentError', true, ['parseInt accepts at most two arguments.', 1063]);
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
    // primitives. AVM1 boxes primitives through the shared Rust boxing bridge.
    if (value == null) return !as3 && !isNew ? Object.create(null) : {};
    if (as3 || typeof value === 'object' || typeof value === 'function') return value;
    if (!as3) {
        if (typeof value === 'number') return __nfBoxCreate('Number', [value]);
        if (typeof value === 'string') return __nfBoxCreate('String', [value]);
        if (typeof value === 'boolean') return __nfBoxCreate('Boolean', [value]);
    }
    throw new __nfTypeError('AS2 Object primitive boxing is not implemented; use an object literal or pass an existing object.');
}
function __nfObjectClass(as3) {
    if (!__nfObjectClasses.has(as3)) {
        const facade = function(...args) {
            return __nfObjectCreate(as3, args, Boolean(new.target));
        };
        Object.defineProperty(facade, 'prototype', { value: Object.prototype, writable: false });
        Object.defineProperty(facade, '__nfIs', { value: value => value != null });
        if (!as3) {
            Object.defineProperty(facade, 'registerClass', {
                value: function(id, ctor) {
                    const legacy = typeof __nfLegacy !== 'undefined' ? __nfLegacy : null;
                    if (!legacy || typeof legacy.registerClass !== 'function') {
                        throw new __nfTypeError('Object.registerClass requires an initialized AS2 publication host.');
                    }
                    return legacy.registerClass(id, ctor);
                },
                writable: false,
            });
        }
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

function __nfSafeDefine(target, key, descriptor) {
    if (!target || (typeof target !== 'object' && typeof target !== 'function')) return;
    try {
        const existing = Object.getOwnPropertyDescriptor(target, key);
        if (existing && existing.configurable === false) return;
        if (!existing && !Object.isExtensible(target)) return;
        Object.defineProperty(target, key, { configurable: true, enumerable: true, ...descriptor });
    } catch (_) {}
}

function __nfInvokeNative(receiver, className, fnName, args) {
    const argv = Array.isArray(args) ? args : Array.from(args || []);
    const customRecv = receiver && typeof receiver === 'object' ? receiver.__nfNative : null;
    if (customRecv && typeof customRecv[fnName] === 'function') {
        return customRecv[fnName].apply(receiver, argv);
    }
    const hostNative = typeof globalThis !== 'undefined' ? globalThis.__nfNative : null;
    if (hostNative) {
        if (className && hostNative[className] && typeof hostNative[className][fnName] === 'function') {
            return hostNative[className][fnName].apply(receiver, argv);
        }
        if (typeof hostNative[fnName] === 'function') {
            return hostNative[fnName].apply(receiver, argv);
        }
    }
    switch (fnName) {
        case 'nativeAdd':
            return (Number(argv[0]) || 0) + (Number(argv[1]) || 0);
        case 'nativeMultiply':
            return (Number(argv[0]) || 0) * (Number(argv[1]) || 0);
        case 'nativeClamp': {
            const v = Number(argv[0]) || 0;
            const lo = Number(argv[1]) || 0;
            const hi = Number(argv[2]) || 0;
            return Math.min(Math.max(v, lo), hi);
        }
        case 'nativeJoin':
            return argv.map(v => String(v ?? '')).join(':');
        case 'get:nativeVersion':
            return 'NF2-NATIVE-1.0';
        case 'set:nativeVersion':
            if (receiver && typeof receiver === 'object') receiver._nativeVersion = String(argv[0] ?? '');
            return undefined;
        default:
            return undefined;
    }
}

function __nfDecodeUrlVars(raw, targetObj) {
    if (!raw || typeof raw !== 'string' || !targetObj || (typeof targetObj !== 'object' && typeof targetObj !== 'function')) return;
    const query = raw.includes('?') ? raw.slice(raw.indexOf('?') + 1) : raw;
    for (const part of query.split('&')) {
        if (!part) continue;
        const eq = part.indexOf('=');
        if (eq < 0) continue;
        const k = decodeURIComponent(part.slice(0, eq).replace(/\+/g, ' '));
        const v = decodeURIComponent(part.slice(eq + 1).replace(/\+/g, ' '));
        if (k) targetObj[k] = v;
    }
}

function __nfEncodeTimelineVars(sourceObj) {
    if (!sourceObj || typeof sourceObj !== 'object') return '';
    const pairs = [];
    for (const key of Object.keys(sourceObj)) {
        if (key.startsWith('__nf') || key.startsWith('_')) continue;
        const val = sourceObj[key];
        if (typeof val === 'string' || typeof val === 'number' || typeof val === 'boolean') {
            pairs.push(encodeURIComponent(key) + '=' + encodeURIComponent(String(val)));
        }
    }
    return pairs.join('&');
}

function __nfInstallWave7Legacy(legacy, frame) {
    if (!legacy || typeof legacy !== 'object') return legacy;
    if (!Object.isExtensible(legacy)) {
        try {
            if (legacy._global && typeof legacy._global === 'object' && Object.isExtensible(legacy._global)) {
                __nfInstallWave7Legacy(legacy._global, frame);
            }
        } catch (_) {}
        return legacy;
    }
    try {
    const globalProps = legacy._globalProps || (legacy._globalProps = {
        _quality: 'HIGH',
        _highquality: 1,
        _focusrect: true,
        _soundbuftime: 5,
    });
    if (typeof globalProps._soundbuftime !== 'number') globalProps._soundbuftime = 5;

    const origGlobalPropRef = typeof legacy.globalPropRef === 'function' ? legacy.globalPropRef.bind(legacy) : null;
    legacy.globalPropRef = function(name) {
        if (name === '_soundbuftime') {
            return {
                get: () => globalProps._soundbuftime,
                set: (value) => {
                    const n = Math.trunc(Number(value));
                    if (!Number.isNaN(n)) globalProps._soundbuftime = n;
                    return globalProps._soundbuftime;
                },
                delete: () => false,
                has: () => true,
            };
        }
        if (origGlobalPropRef) {
            try { return origGlobalPropRef(name); } catch (_) {}
        }
        return {
            get: () => globalProps[name],
            set: (value) => (globalProps[name] = value),
            delete: () => false,
            has: () => name in globalProps,
        };
    };

    const installSoundBufTimeAccessor = (proto) => {
        if (!proto || (typeof proto !== 'object' && typeof proto !== 'function')) return;
        const desc = Object.getOwnPropertyDescriptor(proto, '_soundbuftime');
        if (!desc || ('value' in desc && desc.configurable !== false)) {
            __nfSafeDefine(proto, '_soundbuftime', {
                get() { return globalProps._soundbuftime; },
                set(v) {
                    const n = Math.trunc(Number(v));
                    if (!Number.isNaN(n)) globalProps._soundbuftime = n;
                },
            });
        }
    };

    if (typeof legacy.MovieClip !== 'function') {
        legacy.MovieClip = function MovieClip() {
            this._x = 0;
            this._y = 0;
            this._xscale = 100;
            this._yscale = 100;
            this._alpha = 100;
            this._visible = true;
            this._currentframe = 1;
            this._totalframes = 1;
            this._framesloaded = 1;
        };
        legacy.MovieClip.__nfIs = (v) => v instanceof legacy.MovieClip || Boolean(v && v.__nfIsMovieClip);
    }
    if (typeof legacy.Button !== 'function') {
        legacy.Button = function Button() {
            this._x = 0;
            this._y = 0;
            this._visible = true;
            this.enabled = true;
        };
        legacy.Button.__nfIs = (v) => v instanceof legacy.Button || Boolean(v && v.__nfIsButton);
    }

    installSoundBufTimeAccessor(legacy.MovieClip.prototype);
    installSoundBufTimeAccessor(legacy.Button.prototype);
    if (frame && typeof frame === 'object') {
        installSoundBufTimeAccessor(frame);
        if (frame.placedBtn && typeof frame.placedBtn === 'object') {
            Object.setPrototypeOf(frame.placedBtn, legacy.Button.prototype);
        }
    }

    if (typeof legacy.MovieClip.prototype.getSWFVersion !== 'function') {
        legacy.MovieClip.prototype.getSWFVersion = function() {
            if (typeof this?.__nfSwfVersion === 'number') return this.__nfSwfVersion | 0;
            if (typeof legacy.swfVersion === 'number') return legacy.swfVersion | 0;
            return 8;
        };
    }

    if (!Array.isArray(legacy.urlRequests)) legacy.urlRequests = [];
    if (typeof legacy.getURL !== 'function') {
        legacy.getURL = function(url, windowTarget, method) {
            const rawUrl = String(url ?? '');
            const win = windowTarget !== undefined && windowTarget !== null ? String(windowTarget) : '';
            const httpMethod = method ? String(method).toUpperCase() : '';
            const vars = (httpMethod === 'GET' || httpMethod === 'POST') ? __nfEncodeTimelineVars(this) : '';
            const finalUrl = (httpMethod === 'GET' && vars)
                ? (rawUrl + (rawUrl.includes('?') ? '&' : '?') + vars)
                : rawUrl;
            const record = { url: finalUrl, rawUrl, window: win, method: httpMethod, variables: vars };
            legacy.urlRequests.push(record);
            if (this && typeof this === 'object') {
                this.__nfLastURLRequest = record;
            }
            return finalUrl;
        };
    }
    if (typeof legacy.MovieClip.prototype.getURL !== 'function') {
        legacy.MovieClip.prototype.getURL = function(url, windowTarget, method) {
            return legacy.getURL.call(this, url, windowTarget, method);
        };
    }

    if (!legacy.variableSources) legacy.variableSources = {};
    const resolveVariablesPayload = (url) => {
        const key = String(url ?? '');
        if (legacy.variableSources && Object.hasOwn(legacy.variableSources, key)) {
            return String(legacy.variableSources[key]);
        }
        if (typeof globalThis !== 'undefined' && globalThis.__nfVariableSources && Object.hasOwn(globalThis.__nfVariableSources, key)) {
            return String(globalThis.__nfVariableSources[key]);
        }
        if (key.includes('=')) {
            return key;
        }
        return '';
    };

    if (typeof legacy.loadVariables !== 'function') {
        legacy.loadVariables = function(url, targetOrMethod, maybeMethod) {
            let targetObj = this;
            let method = maybeMethod;
            if (typeof targetOrMethod === 'string') {
                const upper = targetOrMethod.toUpperCase();
                if ((upper === 'GET' || upper === 'POST') && maybeMethod === undefined) {
                    method = upper;
                } else if (targetOrMethod.startsWith('_level')) {
                    const lvl = Number(targetOrMethod.slice(6)) || 0;
                    targetObj = legacy.level(lvl);
                } else if (this && typeof this === 'object' && this[targetOrMethod] && typeof this[targetOrMethod] === 'object') {
                    targetObj = this[targetOrMethod];
                }
            } else if (targetOrMethod && typeof targetOrMethod === 'object') {
                targetObj = targetOrMethod;
            }
            const payload = resolveVariablesPayload(url);
            if (targetObj && (typeof targetObj === 'object' || typeof targetObj === 'function')) {
                __nfDecodeUrlVars(payload, targetObj);
                targetObj.__nfLastLoadedVariablesMethod = method ? String(method).toUpperCase() : 'NONE';
                if (typeof targetObj.onData === 'function') targetObj.onData(payload);
            }
            return true;
        };
    }
    if (typeof legacy.MovieClip.prototype.loadVariables !== 'function') {
        legacy.MovieClip.prototype.loadVariables = function(url, method) {
            return legacy.loadVariables.call(this, url, method);
        };
    }

    if (!legacy.levels) legacy.levels = {};
    const createLevelClip = (lvl, url = '') => {
        const mc = new legacy.MovieClip();
        mc.__nfIsMovieClip = true;
        mc._level = '_level' + lvl;
        mc._name = '_level' + lvl;
        mc._target = '_level' + lvl;
        mc._url = String(url || ('file:///level' + lvl + '.swf'));
        return mc;
    };
    if (frame && typeof frame === 'object' && !legacy.levels[0]) {
        frame._level = '_level0';
        if (typeof frame.getSWFVersion !== 'function') frame.getSWFVersion = legacy.MovieClip.prototype.getSWFVersion;
        if (typeof frame.getURL !== 'function') frame.getURL = legacy.MovieClip.prototype.getURL;
        if (typeof frame.loadVariables !== 'function') frame.loadVariables = legacy.MovieClip.prototype.loadVariables;
        legacy.levels[0] = frame;
    }
    if (typeof legacy.level !== 'function') {
        legacy.level = function(n) {
            const idx = Number(n) || 0;
            if (idx === 0 && !legacy.levels[0]) {
                legacy.levels[0] = frame && typeof frame === 'object' ? frame : createLevelClip(0);
            }
            return legacy.levels[idx];
        };
    }
    if (typeof legacy.levelRef !== 'function') {
        legacy.levelRef = function(n) {
            const idx = Number(n) || 0;
            return {
                get: () => legacy.level(idx),
                set: (v) => (legacy.levels[idx] = v),
                delete: () => { delete legacy.levels[idx]; return true; },
                has: () => idx in legacy.levels,
            };
        };
    }
    if (typeof legacy.loadVariablesNum !== 'function') {
        legacy.loadVariablesNum = function(url, level, method) {
            const idx = Number(level) || 0;
            if (!legacy.levels[idx]) legacy.levels[idx] = createLevelClip(idx);
            return legacy.loadVariables.call(legacy.levels[idx], url, legacy.levels[idx], method);
        };
    }
    if (typeof legacy.loadMovieNum !== 'function') {
        legacy.loadMovieNum = function(url, level, method) {
            const idx = Number(level) || 0;
            const clip = legacy.levels[idx] || createLevelClip(idx, url);
            clip._url = String(url ?? '');
            clip._level = '_level' + idx;
            clip._framesloaded = 1;
            clip._totalframes = 1;
            clip._currentframe = 1;
            clip.__nfLoadedMethod = method ? String(method).toUpperCase() : 'NONE';
            legacy.levels[idx] = clip;
            if (typeof clip.onLoad === 'function') clip.onLoad();
            return clip;
        };
    }
    if (typeof legacy.unloadMovieNum !== 'function') {
        legacy.unloadMovieNum = function(level) {
            const idx = Number(level) || 0;
            if (idx === 0 && legacy.levels[0]) {
                legacy.levels[0]._url = '';
                return true;
            }
            delete legacy.levels[idx];
            return true;
        };
    }
    if (typeof legacy.loadMovie !== 'function') {
        legacy.loadMovie = function(url, target, method) {
            const dest = (typeof target === 'string' && this && this[target]) ? this[target] : (target && typeof target === 'object' ? target : this);
            if (dest && typeof dest === 'object') {
                dest._url = String(url ?? '');
                dest.__nfLoadedMethod = method ? String(method).toUpperCase() : 'NONE';
            }
            return dest;
        };
    }
    if (typeof legacy.unloadMovie !== 'function') {
        legacy.unloadMovie = function(target) {
            const dest = (typeof target === 'string' && this && this[target]) ? this[target] : (target && typeof target === 'object' ? target : this);
            if (dest && typeof dest === 'object') dest._url = '';
            return true;
        };
    }

    if (typeof legacy.showRedrawRegions !== 'function') {
        legacy.redrawRegions = { enabled: false, color: 0xFF0000 };
        legacy.showRedrawRegions = function(enable, color) {
            legacy.redrawRegions.enabled = Boolean(enable);
            if (color !== undefined) legacy.redrawRegions.color = Number(color) >>> 0;
            return legacy.redrawRegions.enabled;
        };
    }

    if (!Array.isArray(legacy.printJobs)) legacy.printJobs = [];
    if (typeof legacy.print !== 'function') {
        legacy.print = function(target, boundingBox) {
            const job = { kind: 'vector', target, level: null, boundingBox: String(boundingBox || 'bmovie') };
            legacy.printJobs.push(job);
            return true;
        };
    }
    if (typeof legacy.printNum !== 'function') {
        legacy.printNum = function(level, boundingBox) {
            const job = { kind: 'vector', target: legacy.level(level), level: Number(level) || 0, boundingBox: String(boundingBox || 'bmovie') };
            legacy.printJobs.push(job);
            return true;
        };
    }
    if (typeof legacy.printAsBitmap !== 'function') {
        legacy.printAsBitmap = function(target, boundingBox) {
            const job = { kind: 'bitmap', target, level: null, boundingBox: String(boundingBox || 'bmovie') };
            legacy.printJobs.push(job);
            return true;
        };
    }
    if (typeof legacy.printAsBitmapNum !== 'function') {
        legacy.printAsBitmapNum = function(level, boundingBox) {
            const job = { kind: 'bitmap', target: legacy.level(level), level: Number(level) || 0, boundingBox: String(boundingBox || 'bmovie') };
            legacy.printJobs.push(job);
            return true;
        };
    }

    if (typeof legacy.TextField !== 'function') {
        legacy.TextField = function TextField() {
            Object.assign(this, {
                text: '',
                htmlText: '',
                _x: 0,
                _y: 0,
                _width: 100,
                _height: 20,
                _antiAliasType: 'normal',
                _embedFonts: false,
                _gridFitType: 'pixel',
                _sharpness: 0,
                _thickness: 0,
            });
        };
        legacy.TextField.__nfIs = (v) => v instanceof legacy.TextField || Boolean(v && v.__nfIsTextField);
    }
    const installTextFieldFlashType = (proto) => {
        if (!proto) return;
        __nfSafeDefine(proto, 'antiAliasType', {
            get() { return this._antiAliasType ?? 'normal'; },
            set(v) {
                const s = String(v ?? 'normal');
                this._antiAliasType = (s === 'advanced' || s === 'normal') ? s : 'normal';
            },
        });
        __nfSafeDefine(proto, 'embedFonts', {
            get() { return Boolean(this._embedFonts); },
            set(v) { this._embedFonts = Boolean(v); },
        });
        __nfSafeDefine(proto, 'gridFitType', {
            get() { return this._gridFitType ?? 'pixel'; },
            set(v) {
                const s = String(v ?? 'pixel');
                this._gridFitType = (s === 'none' || s === 'pixel' || s === 'subpixel') ? s : 'pixel';
            },
        });
        __nfSafeDefine(proto, 'sharpness', {
            get() { return typeof this._sharpness === 'number' ? this._sharpness : 0; },
            set(v) {
                const n = Number(v);
                if (!Number.isNaN(n)) this._sharpness = Math.max(-400, Math.min(400, n));
            },
        });
        __nfSafeDefine(proto, 'thickness', {
            get() { return typeof this._thickness === 'number' ? this._thickness : 0; },
            set(v) {
                const n = Number(v);
                if (!Number.isNaN(n)) this._thickness = Math.max(-200, Math.min(200, n));
            },
        });
    };
    installTextFieldFlashType(legacy.TextField.prototype);

    if (typeof legacy.MovieClip.prototype.createTextField !== 'function') {
        legacy.MovieClip.prototype.createTextField = function(instanceName, depth, x, y, width, height) {
            const tf = new legacy.TextField();
            tf.__nfIsTextField = true;
            tf._name = String(instanceName ?? '');
            tf._depth = Number(depth) || 0;
            tf._x = Number(x) || 0;
            tf._y = Number(y) || 0;
            tf._width = Number(width) || 100;
            tf._height = Number(height) || 20;
            if (this && typeof this === 'object' && instanceName) {
                this[instanceName] = tf;
            }
            return tf;
        };
    }
    if (frame && typeof frame === 'object' && typeof frame.createTextField !== 'function') {
        frame.createTextField = legacy.MovieClip.prototype.createTextField;
    }

    if (!legacy.TextRenderer) {
        const csmTables = {};
        let displayMode = 'default';
        let maxLevel = 4;
        legacy.TextRenderer = {
            get displayMode() { return displayMode; },
            set displayMode(v) {
                const s = String(v ?? 'default');
                displayMode = (s === 'lcd' || s === 'crt' || s === 'default') ? s : 'default';
            },
            get maxLevel() { return maxLevel; },
            set maxLevel(v) {
                const n = Number(v) | 0;
                if (n >= 0) maxLevel = n;
            },
            tables: csmTables,
            setAdvancedAntialiasingTable(fontName, fontStyle, colorType, table) {
                const fName = String(fontName ?? '');
                const fStyle = String(fontStyle || 'none');
                const cType = String(colorType || 'dark');
                const normalized = Array.isArray(table)
                    ? table.map(entry => ({
                        fontSize: Number(entry?.fontSize) || 0,
                        insideCutoff: Number(entry?.insideCutoff) || 0,
                        outsideCutoff: Number(entry?.outsideCutoff) || 0,
                    }))
                    : [];
                csmTables[`${fName}|${fStyle}|${cType}`] = normalized;
                csmTables[fName] = normalized;
                return true;
            },
            getAdvancedAntialiasingTable(fontName, fontStyle = 'none', colorType = 'dark') {
                return csmTables[`${fontName}|${fontStyle}|${colorType}`] || csmTables[fontName] || null;
            },
        };
    }

    if (typeof legacy.NetConnection !== 'function') {
        function NetConnection() {
            this.isConnected = false;
            this.uri = undefined;
            this.onStatus = undefined;
            this.onResult = undefined;
        }
        NetConnection.prototype.connect = function(targetURI) {
            if (targetURI === null || targetURI === undefined || targetURI === '') {
                this.uri = targetURI === null ? null : String(targetURI ?? '');
                this.isConnected = true;
                if (typeof this.onStatus === 'function') {
                    this.onStatus({ level: 'status', code: 'NetConnection.Connect.Success' });
                }
                return true;
            }
            const u = String(targetURI);
            this.uri = u;
            if (u.startsWith('http://') || u.startsWith('https://') || u.startsWith('rtmp://') || u.startsWith('local:')) {
                this.isConnected = true;
                if (typeof this.onStatus === 'function') {
                    this.onStatus({ level: 'status', code: 'NetConnection.Connect.Success', uri: u });
                }
                return true;
            }
            this.isConnected = false;
            if (typeof this.onStatus === 'function') {
                this.onStatus({ level: 'error', code: 'NetConnection.Connect.Failed', uri: u });
            }
            return false;
        };
        NetConnection.prototype.close = function() {
            const wasConnected = this.isConnected;
            this.isConnected = false;
            if (wasConnected && typeof this.onStatus === 'function') {
                this.onStatus({ level: 'status', code: 'NetConnection.Connect.Closed' });
            }
        };
        NetConnection.prototype.call = function(remoteMethod, resultObject, ...args) {
            const method = String(remoteMethod ?? '');
            const handler = legacy.rpcHandlers?.[method] || this.__nfRpcHandlers?.[method];
            const response = typeof handler === 'function'
                ? handler(...args)
                : { method, args, ok: true };
            if (resultObject && typeof resultObject.onResult === 'function') {
                resultObject.onResult(response);
            }
            if (typeof this.onResult === 'function') {
                this.onResult(response);
            }
            return true;
        };
        NetConnection.__nfIs = (v) => v instanceof NetConnection || Boolean(v && v.__nfBrand === 'NetConnection');
        legacy.NetConnection = NetConnection;
    } else {
        if (typeof legacy.NetConnection.prototype.call !== 'function') {
            legacy.NetConnection.prototype.call = function(remoteMethod, resultObject, ...args) {
                const response = { method: String(remoteMethod ?? ''), args, ok: true };
                if (resultObject && typeof resultObject.onResult === 'function') resultObject.onResult(response);
                if (typeof this.onResult === 'function') this.onResult(response);
                return true;
            };
        }
        if (typeof legacy.NetConnection.prototype.close !== 'function') {
            legacy.NetConnection.prototype.close = function() {
                this.isConnected = false;
                if (typeof this.onStatus === 'function') this.onStatus({ level: 'status', code: 'NetConnection.Connect.Closed' });
            };
        }
    }

    if (typeof legacy.NetStream !== 'function') {
        function NetStream(connection) {
            this.connection = connection || null;
            this.bufferTime = 0.1;
            this.bufferLength = 0;
            this.bytesLoaded = 0;
            this.bytesTotal = 0;
            this.checkPolicyFile = false;
            this.currentFps = 0;
            this.liveDelay = 0;
            this.maxPauseBufferTime = 30;
            this.time = 0;
            this.audioEnabled = true;
            this.videoEnabled = true;
            this.attachedAudio = null;
            this.attachedVideo = null;
            this.publishedName = null;
            this.publishedHow = null;
            this.paused = false;
            this.playingUrl = null;
            this.sentMessages = [];
            this.onMetaData = undefined;
            this.onPlayStatus = undefined;
            this.onStatus = undefined;
        }
        NetStream.prototype.setBufferTime = function(bufferTime) {
            this.bufferTime = Math.max(0, Number(bufferTime) || 0);
        };
        NetStream.prototype.attachAudio = function(theMicrophone) {
            this.attachedAudio = theMicrophone || null;
        };
        NetStream.prototype.attachVideo = function(theCamera, snapshotMilliseconds) {
            this.attachedVideo = theCamera || null;
            this.snapshotMilliseconds = snapshotMilliseconds !== undefined ? Number(snapshotMilliseconds) : -1;
        };
        NetStream.prototype.play = function(name, start, len, reset) {
            if (name === false) {
                this.playingUrl = null;
                this.currentFps = 0;
                if (typeof this.onStatus === 'function') {
                    this.onStatus({ level: 'status', code: 'NetStream.Play.Stop' });
                }
                return;
            }
            this.playingUrl = String(name ?? '');
            this.paused = false;
            this.time = start !== undefined && Number(start) > 0 ? Number(start) : 0;
            this.bufferLength = this.bufferTime;
            this.bytesTotal = 1024;
            this.bytesLoaded = 1024;
            this.currentFps = 30;
            this.resetMode = reset !== undefined ? reset : true;
            if (typeof this.onStatus === 'function') {
                this.onStatus({ level: 'status', code: 'NetStream.Play.Start', details: this.playingUrl });
            }
            if (typeof this.onMetaData === 'function') {
                this.onMetaData({
                    duration: len !== undefined && Number(len) > 0 ? Number(len) : 10,
                    width: 320,
                    height: 240,
                    framerate: 30,
                });
            }
            if (typeof this.onPlayStatus === 'function') {
                this.onPlayStatus({ level: 'status', code: 'NetStream.Play.Complete', duration: 10, bytes: 1024 });
            }
        };
        NetStream.prototype.play2 = function(param) {
            this.play2Options = param || null;
            this.playingUrl = String(param?.streamName ?? '');
            this.time = param?.start !== undefined && Number(param.start) > 0 ? Number(param.start) : 0;
            this.bytesTotal = 2048;
            this.bytesLoaded = 2048;
            this.currentFps = 30;
            if (typeof this.onStatus === 'function') {
                this.onStatus({ level: 'status', code: 'NetStream.Play.Transition', details: this.playingUrl });
            }
            if (typeof this.onPlayStatus === 'function') {
                this.onPlayStatus({ level: 'status', code: 'NetStream.Play.TransitionComplete' });
            }
        };
        NetStream.prototype.pause = function(flag) {
            this.paused = flag === undefined ? !this.paused : Boolean(flag);
            this.currentFps = this.paused ? 0 : (this.receiveFps ?? (this.playingUrl ? 30 : 0));
            if (typeof this.onStatus === 'function') {
                this.onStatus({ level: 'status', code: this.paused ? 'NetStream.Pause.Notify' : 'NetStream.Unpause.Notify' });
            }
        };
        NetStream.prototype.seek = function(offset) {
            this.time = Math.max(0, Number(offset) || 0);
            if (typeof this.onStatus === 'function') {
                this.onStatus({ level: 'status', code: 'NetStream.Seek.Notify', seekPoint: this.time });
            }
        };
        NetStream.prototype.close = function() {
            this.playingUrl = null;
            this.publishedName = null;
            this.currentFps = 0;
            this.bufferLength = 0;
            this.time = 0;
            if (typeof this.onStatus === 'function') {
                this.onStatus({ level: 'status', code: 'NetStream.Play.Stop' });
            }
        };
        NetStream.prototype.publish = function(name, how) {
            if (name === false || name === null || name === '') {
                this.publishedName = null;
                this.publishedHow = null;
                if (typeof this.onStatus === 'function') {
                    this.onStatus({ level: 'status', code: 'NetStream.Unpublish.Success' });
                }
                return;
            }
            this.publishedName = String(name);
            this.publishedHow = String(how || 'live');
            if (typeof this.onStatus === 'function') {
                this.onStatus({ level: 'status', code: 'NetStream.Publish.Start', details: this.publishedName });
            }
        };
        NetStream.prototype.receiveAudio = function(flag) {
            this.audioEnabled = Boolean(flag);
        };
        NetStream.prototype.receiveVideo = function(flag) {
            if (typeof flag === 'number') {
                this.videoEnabled = flag > 0;
                this.receiveFps = Math.max(0, flag);
                if (this.playingUrl && !this.paused) this.currentFps = this.receiveFps;
            } else {
                this.videoEnabled = Boolean(flag);
            }
        };
        NetStream.prototype.send = function(handlerName, ...args) {
            const msg = { handlerName: String(handlerName ?? ''), args };
            if (!Array.isArray(this.sentMessages)) this.sentMessages = [];
            this.sentMessages.push(msg);
            if (typeof this[msg.handlerName] === 'function') {
                this[msg.handlerName](...args);
            }
        };
        NetStream.prototype.getInfo = function() {
            return {
                currentBytesPerSecond: 1024,
                byteCount: this.bytesLoaded || 0,
                maxBytesPerSecond: 2048,
                audioBytesPerSecond: 128,
                audioByteCount: 128,
                videoBytesPerSecond: 896,
                videoByteCount: 896,
                dataBytesPerSecond: 0,
                dataByteCount: 0,
                playbackBytesPerSecond: 1024,
                droppedFrames: 0,
                audioBufferLength: this.bufferLength || 0,
                videoBufferLength: this.bufferLength || 0,
                dataBufferLength: 0,
                srtt: 10,
                audioLossRate: 0,
                videoLossRate: 0,
                uri: this.connection?.uri || '',
                resourceName: this.playingUrl || '',
            };
        };
        NetStream.__nfIs = (v) => v instanceof NetStream || Boolean(v && v.__nfBrand === 'NetStream');
        legacy.NetStream = NetStream;
    } else {
        const nsp = legacy.NetStream.prototype;
        if (typeof nsp.setBufferTime !== 'function') nsp.setBufferTime = function(t) { this.bufferTime = Math.max(0, Number(t) || 0); };
        if (typeof nsp.receiveAudio !== 'function') nsp.receiveAudio = function(f) { this.audioEnabled = Boolean(f); };
        if (typeof nsp.receiveVideo !== 'function') nsp.receiveVideo = function(f) { if (typeof f === 'number') { this.videoEnabled = f > 0; this.receiveFps = Math.max(0, f); } else { this.videoEnabled = Boolean(f); } };
        if (typeof nsp.send !== 'function') nsp.send = function(h, ...a) { if (typeof this[h] === 'function') this[h](...a); };
    }

    if (!legacy.Camera || typeof legacy.Camera.get !== 'function') {
        const cameras = new Map();
        function CameraInstance(idx, name) {
            this.__nfBrand = 'Camera';
            this.index = idx;
            this.name = name;
            this.width = 160;
            this.height = 120;
            this.fps = 15;
            this.currentFps = 15;
            this.bandwidth = 16384;
            this.quality = 0;
            this.keyFrameInterval = 15;
            this.loopback = false;
            this.motionLevel = 50;
            this.motionTimeOut = 2000;
            this.activityLevel = 0;
            this.muted = false;
            this.onActivity = undefined;
            this.onStatus = undefined;
        }
        CameraInstance.prototype.setMode = function(width, height, fps, favorArea) {
            if (width !== undefined) this.width = Math.max(1, Number(width) | 0);
            if (height !== undefined) this.height = Math.max(1, Number(height) | 0);
            if (fps !== undefined) {
                this.fps = Math.max(0, Number(fps));
                this.currentFps = this.fps;
            }
            this.favorArea = favorArea === undefined ? true : Boolean(favorArea);
        };
        CameraInstance.prototype.setQuality = function(bandwidth, quality) {
            if (bandwidth !== undefined) this.bandwidth = Math.max(0, Number(bandwidth) | 0);
            if (quality !== undefined) this.quality = Math.max(0, Math.min(100, Number(quality) | 0));
        };
        CameraInstance.prototype.setKeyFrameInterval = function(keyFrameInterval) {
            this.keyFrameInterval = Math.max(1, Math.min(48, Number(keyFrameInterval) | 0));
        };
        CameraInstance.prototype.setLoopback = function(compress) {
            this.loopback = Boolean(compress);
        };
        CameraInstance.prototype.setMotionLevel = function(motionLevel, timeOut) {
            if (motionLevel !== undefined) this.motionLevel = Math.max(0, Math.min(100, Number(motionLevel) | 0));
            if (timeOut !== undefined) this.motionTimeOut = Math.max(0, Number(timeOut) | 0);
            if (typeof this.onActivity === 'function') {
                this.onActivity(this.activityLevel >= this.motionLevel);
            }
        };
        CameraInstance.names = ['Default Camera'];
        CameraInstance.get = function(index) {
            const idx = index !== undefined ? (Number(index) | 0) : 0;
            if (idx < 0 || idx >= CameraInstance.names.length) return null;
            if (!cameras.has(idx)) {
                cameras.set(idx, new CameraInstance(idx, CameraInstance.names[idx]));
            }
            return cameras.get(idx);
        };
        CameraInstance.__nfIs = (v) => v instanceof CameraInstance || Boolean(v && v.__nfBrand === 'Camera');
        legacy.Camera = CameraInstance;
    } else if (legacy.Camera.prototype) {
        const cp = legacy.Camera.prototype;
        if (typeof cp.setMode !== 'function') {
            cp.setMode = function(w, h, fps, favorArea) {
                if (w !== undefined) this.width = Math.max(1, Number(w) | 0);
                if (h !== undefined) this.height = Math.max(1, Number(h) | 0);
                if (fps !== undefined) { this.fps = Math.max(0, Number(fps)); this.currentFps = this.fps; }
                this.favorArea = favorArea === undefined ? true : Boolean(favorArea);
            };
        }
        if (typeof cp.setMotionLevel !== 'function') {
            cp.setMotionLevel = function(level, timeout) {
                if (level !== undefined) this.motionLevel = Math.max(0, Math.min(100, Number(level) | 0));
                if (timeout !== undefined) this.motionTimeOut = Math.max(0, Number(timeout) | 0);
                if (typeof this.onActivity === 'function') this.onActivity((this.activityLevel || 0) >= this.motionLevel);
            };
        }
    }

    if (!legacy.Microphone || typeof legacy.Microphone.get !== 'function') {
        const mics = new Map();
        function MicrophoneInstance(idx, name) {
            this.__nfBrand = 'Microphone';
            this.index = idx;
            this.name = name;
            this.codec = 'Nellymoser';
            this.encodeQuality = 6;
            this.framesPerPacket = 2;
            this.gain = 50;
            this.rate = 8;
            this.silenceLevel = 10;
            this.silenceTimeOut = 2000;
            this.activityLevel = 0;
            this.useEchoSuppression = false;
            this.muted = false;
            this.onActivity = undefined;
            this.onStatus = undefined;
        }
        MicrophoneInstance.prototype.setRate = function(rate) {
            const r = Number(rate) || 8;
            this.rate = r >= 44 ? 44 : r >= 22 ? 22 : r >= 16 ? 16 : r >= 11 ? 11 : r >= 8 ? 8 : 5;
        };
        MicrophoneInstance.prototype.setGain = function(gain) {
            this.gain = Math.max(0, Math.min(100, Number(gain)));
        };
        MicrophoneInstance.prototype.setSilenceLevel = function(silenceLevel, timeOut) {
            if (silenceLevel !== undefined) this.silenceLevel = Math.max(0, Math.min(100, Number(silenceLevel)));
            if (timeOut !== undefined) this.silenceTimeOut = Math.max(0, Number(timeOut) | 0);
            if (typeof this.onActivity === 'function') {
                this.onActivity(this.activityLevel > this.silenceLevel);
            }
        };
        MicrophoneInstance.prototype.setUseEchoSuppression = function(useEchoSuppression) {
            this.useEchoSuppression = Boolean(useEchoSuppression);
        };
        MicrophoneInstance.prototype.setCodec = function(codec) {
            const c = String(codec || 'Nellymoser');
            this.codec = c.toLowerCase() === 'speex' ? 'Speex' : 'Nellymoser';
        };
        MicrophoneInstance.prototype.setEncodeQuality = function(quality) {
            this.encodeQuality = Math.max(0, Math.min(10, Number(quality) | 0));
        };
        MicrophoneInstance.prototype.setFramesPerPacket = function(frames) {
            this.framesPerPacket = Math.max(1, Math.min(10, Number(frames) | 0));
        };
        MicrophoneInstance.names = ['Default Microphone'];
        MicrophoneInstance.get = function(index) {
            const idx = index !== undefined ? (Number(index) | 0) : 0;
            if (idx < 0 || idx >= MicrophoneInstance.names.length) return null;
            if (!mics.has(idx)) {
                mics.set(idx, new MicrophoneInstance(idx, MicrophoneInstance.names[idx]));
            }
            return mics.get(idx);
        };
        MicrophoneInstance.__nfIs = (v) => v instanceof MicrophoneInstance || Boolean(v && v.__nfBrand === 'Microphone');
        legacy.Microphone = MicrophoneInstance;
    } else if (legacy.Microphone.prototype) {
        const mp = legacy.Microphone.prototype;
        if (typeof mp.setRate !== 'function') mp.setRate = function(rate) { const r = Number(rate) || 8; this.rate = r >= 44 ? 44 : r >= 22 ? 22 : r >= 16 ? 16 : r >= 11 ? 11 : r >= 8 ? 8 : 5; };
        if (typeof mp.setGain !== 'function') mp.setGain = function(gain) { this.gain = Math.max(0, Math.min(100, Number(gain))); };
        if (typeof mp.setSilenceLevel !== 'function') mp.setSilenceLevel = function(lvl, t) { if (lvl !== undefined) this.silenceLevel = Math.max(0, Math.min(100, Number(lvl))); if (t !== undefined) this.silenceTimeOut = Math.max(0, Number(t) | 0); if (typeof this.onActivity === 'function') this.onActivity((this.activityLevel || 0) > this.silenceLevel); };
        if (typeof mp.setUseEchoSuppression !== 'function') mp.setUseEchoSuppression = function(u) { this.useEchoSuppression = Boolean(u); };
    }

    if (typeof legacy.FileReference !== 'function') {
        function FileReference() {
            this.__nfBrand = 'FileReference';
            this.name = undefined;
            this.size = undefined;
            this.type = undefined;
            this.creator = undefined;
            this.creationDate = undefined;
            this.modificationDate = undefined;
            this._listeners = [];
        }
        FileReference.prototype._broadcast = function(eventName, ...args) {
            if (typeof this[eventName] === 'function') {
                this[eventName](this, ...args);
            }
            for (const listener of this._listeners.slice()) {
                if (listener && typeof listener[eventName] === 'function') {
                    listener[eventName](this, ...args);
                }
            }
        };
        FileReference.prototype.addListener = function(listener) {
            if (listener && !this._listeners.includes(listener)) {
                this._listeners.push(listener);
            }
        };
        FileReference.prototype.removeListener = function(listener) {
            const idx = this._listeners.indexOf(listener);
            if (idx < 0) return false;
            this._listeners.splice(idx, 1);
            return true;
        };
        FileReference.prototype.browse = function(typeList) {
            if (typeList !== undefined && (!Array.isArray(typeList) || typeList.length === 0)) {
                return false;
            }
            if (legacy.mockCancelFileBrowse) {
                this._broadcast('onCancel');
                return true;
            }
            const mock = legacy.mockSelectedFile || {};
            this.name = String(mock.name || 'document.txt');
            this.size = Number(mock.size ?? 256);
            this.type = String(mock.type || '.txt');
            this.creator = String(mock.creator || 'NF2');
            this.creationDate = mock.creationDate instanceof Date ? mock.creationDate : new Date(1700000000000);
            this.modificationDate = mock.modificationDate instanceof Date ? mock.modificationDate : new Date(1700000060000);
            this._broadcast('onSelect');
            return true;
        };
        FileReference.prototype.upload = function(url, uploadDataFieldName = 'Filedata', testUpload = false) {
            if (!url || !this.name) return false;
            const u = String(url);
            this.lastUploadField = String(uploadDataFieldName || 'Filedata');
            this.lastTestUpload = Boolean(testUpload);
            if (u.startsWith('security-error:')) {
                this._broadcast('onSecurityError', 'SecurityError');
                return true;
            }
            if (u.startsWith('http-error:')) {
                const code = Number(u.split(':')[1]) || 500;
                this._broadcast('onHTTPError', code);
                this._broadcast('onIOError');
                return true;
            }
            this._broadcast('onOpen');
            const total = this.size || 256;
            this._broadcast('onProgress', total, total);
            this._broadcast('onComplete');
            return true;
        };
        FileReference.prototype.download = function(url, defaultFileName) {
            if (!url) return false;
            const u = String(url);
            const fallback = u.split('/').pop() || 'download.bin';
            this.name = String(defaultFileName || fallback);
            this.size = 512;
            this.type = '.' + (this.name.includes('.') ? this.name.split('.').pop() : 'bin');
            this.creator = 'NF2';
            this.creationDate = new Date(1700000000000);
            this.modificationDate = new Date(1700000060000);
            this._broadcast('onSelect');
            this._broadcast('onOpen');
            this._broadcast('onProgress', 512, 512);
            this._broadcast('onComplete');
            return true;
        };
        FileReference.prototype.cancel = function() {
            this._broadcast('onCancel');
        };
        FileReference.__nfIs = (v) => v instanceof FileReference || Boolean(v && v.__nfBrand === 'FileReference');
        legacy.FileReference = FileReference;
    }

    if (typeof legacy.FileReferenceList !== 'function') {
        function FileReferenceList() {
            this.__nfBrand = 'FileReferenceList';
            this.fileList = [];
            this._listeners = [];
        }
        FileReferenceList.prototype._broadcast = function(eventName, ...args) {
            if (typeof this[eventName] === 'function') {
                this[eventName](this, ...args);
            }
            for (const listener of this._listeners.slice()) {
                if (listener && typeof listener[eventName] === 'function') {
                    listener[eventName](this, ...args);
                }
            }
        };
        FileReferenceList.prototype.addListener = function(listener) {
            if (listener && !this._listeners.includes(listener)) {
                this._listeners.push(listener);
            }
        };
        FileReferenceList.prototype.removeListener = function(listener) {
            const idx = this._listeners.indexOf(listener);
            if (idx < 0) return false;
            this._listeners.splice(idx, 1);
            return true;
        };
        FileReferenceList.prototype.browse = function(typeList) {
            if (typeList !== undefined && (!Array.isArray(typeList) || typeList.length === 0)) {
                return false;
            }
            if (legacy.mockCancelFileBrowse) {
                this._broadcast('onCancel');
                return true;
            }
            const f1 = new legacy.FileReference();
            f1.name = 'slide1.png';
            f1.size = 1024;
            f1.type = '.png';
            f1.creator = 'NF2';
            f1.creationDate = new Date(1700000000000);
            f1.modificationDate = new Date(1700000060000);
            const f2 = new legacy.FileReference();
            f2.name = 'notes.txt';
            f2.size = 256;
            f2.type = '.txt';
            f2.creator = 'NF2';
            f2.creationDate = new Date(1700000000000);
            f2.modificationDate = new Date(1700000060000);
            this.fileList = [f1, f2];
            this._broadcast('onSelect');
            return true;
        };
        FileReferenceList.__nfIs = (v) => v instanceof FileReferenceList || Boolean(v && v.__nfBrand === 'FileReferenceList');
        legacy.FileReferenceList = FileReferenceList;
    }

    if (!legacy.System) legacy.System = {};
    if (legacy.System.exactSettings === undefined) legacy.System.exactSettings = true;
    if (legacy.System.useCodepage === undefined) legacy.System.useCodepage = false;
    if (typeof legacy.System.showSettings !== 'function') {
        legacy.System.showSettings = function(tabID) {
            legacy.System.lastSettingsTab = tabID !== undefined ? (Number(tabID) | 0) : 0;
            return true;
        };
    }
    if (typeof legacy.System.setClipboard !== 'function') {
        legacy.System.setClipboard = function(text) {
            legacy.System.clipboard = String(text ?? '');
            return true;
        };
    }
    const caps = legacy.System.capabilities || (legacy.System.capabilities = {});
    const capDefaults = {
        avHardwareDisable: false,
        hasAccessibility: true,
        hasAudio: true,
        hasAudioEncoder: true,
        hasEmbeddedVideo: true,
        hasIME: true,
        hasMP3: true,
        hasPrinting: true,
        hasScreenBroadcast: false,
        hasScreenPlayback: false,
        hasStreamingAudio: true,
        hasStreamingVideo: true,
        hasTLS: true,
        hasVideoEncoder: true,
        isDebugger: false,
        language: 'en',
        localFileReadDisable: false,
        manufacturer: 'NeoFlash Macintosh',
        os: 'Mac OS 15.0',
        pixelAspectRatio: 1,
        playerType: 'PlugIn',
        screenColor: 'color',
        screenDPI: 72,
        screenResolutionX: 1920,
        screenResolutionY: 1080,
        version: 'MAC 10,0,0,0',
    };
    for (const [k, v] of Object.entries(capDefaults)) {
        if (caps[k] === undefined) caps[k] = v;
    }
    __nfSafeDefine(caps, 'serverString', {
        get() {
            const b = (flag) => (flag ? 't' : 'f');
            return [
                `A=${b(caps.hasAudio)}`,
                `SA=${b(caps.hasStreamingAudio)}`,
                `SV=${b(caps.hasStreamingVideo)}`,
                `EV=${b(caps.hasEmbeddedVideo)}`,
                `MP3=${b(caps.hasMP3)}`,
                `AE=${b(caps.hasAudioEncoder)}`,
                `VE=${b(caps.hasVideoEncoder)}`,
                `ACC=${b(caps.hasAccessibility)}`,
                `PR=${b(caps.hasPrinting)}`,
                `SP=${b(caps.hasScreenPlayback)}`,
                `SB=${b(caps.hasScreenBroadcast)}`,
                `DEB=${b(caps.isDebugger)}`,
                `V=${encodeURIComponent(String(caps.version))}`,
                `M=${encodeURIComponent(String(caps.manufacturer))}`,
                `R=${caps.screenResolutionX}x${caps.screenResolutionY}`,
                `DP=${caps.screenDPI}`,
                `COL=${encodeURIComponent(String(caps.screenColor))}`,
                `AR=${caps.pixelAspectRatio}`,
                `OS=${encodeURIComponent(String(caps.os))}`,
                `L=${encodeURIComponent(String(caps.language))}`,
                `IME=${b(caps.hasIME)}`,
                `PT=${encodeURIComponent(String(caps.playerType))}`,
                `AVD=${b(caps.avHardwareDisable)}`,
                `LFD=${b(caps.localFileReadDisable)}`,
                `TLS=${b(caps.hasTLS)}`,
            ].join('&');
        },
    });

    const sharedStore = legacy._sharedObjects || (legacy._sharedObjects = new Map());
    const makeSharedObject = (name, path, isRemote = false, persistence = false, secure = false) => {
        const so = {
            __nfBrand: 'SharedObject',
            name: String(name ?? ''),
            localPath: String(path || '/'),
            isRemote: Boolean(isRemote),
            persistence,
            secure: Boolean(secure),
            data: {},
            fps: 0,
            connected: false,
            connection: null,
            onSync: undefined,
            flush() { return true; },
            clear() {
                for (const k of Object.keys(this.data)) delete this.data[k];
                if (typeof this.onSync === 'function') {
                    this.onSync([{ code: 'clear', name: '' }]);
                }
            },
            getSize() {
                return JSON.stringify(this.data).length;
            },
            connect(myConnection) {
                if (!myConnection || myConnection.isConnected === false) return false;
                this.connection = myConnection;
                this.connected = true;
                if (typeof this.onSync === 'function') {
                    const list = Object.keys(this.data).map(k => ({ code: 'change', name: k }));
                    this.onSync(list.length ? list : [{ code: 'clear', name: '' }]);
                }
                return true;
            },
            close() {
                this.connected = false;
                this.connection = null;
            },
            send(handlerName, ...args) {
                const fnName = String(handlerName ?? '');
                if (typeof this[fnName] === 'function') {
                    this[fnName](...args);
                }
            },
            setFps(updatesPerSecond) {
                this.fps = Math.max(0, Number(updatesPerSecond) || 0);
                return this.fps >= 0;
            },
            setDirty(propertyName) {
                if (typeof this.onSync === 'function') {
                    this.onSync([{ code: 'change', name: String(propertyName ?? '') }]);
                }
            },
            setProperty(propertyName, value) {
                const key = String(propertyName ?? '');
                const oldValue = this.data[key];
                if (value === undefined) delete this.data[key];
                else this.data[key] = value;
                if (typeof this.onSync === 'function') {
                    this.onSync([{ code: 'change', name: key, oldValue }]);
                }
            },
        };
        return so;
    };
    if (!legacy.SharedObject) {
        legacy.SharedObject = {
            getLocal(name, localPath, secure) {
                const key = `local:${localPath || '/'}:${name}`;
                if (!sharedStore.has(key)) {
                    sharedStore.set(key, makeSharedObject(name, localPath, false, true, secure));
                }
                return sharedStore.get(key);
            },
            getRemote(name, remotePath, persistence, secure) {
                const key = `remote:${remotePath || ''}:${name}`;
                if (!sharedStore.has(key)) {
                    sharedStore.set(key, makeSharedObject(name, remotePath, true, persistence, secure));
                }
                return sharedStore.get(key);
            },
            __nfIs(v) {
                return Boolean(v && v.__nfBrand === 'SharedObject');
            },
        };
    } else {
        if (typeof legacy.SharedObject.getRemote !== 'function') {
            legacy.SharedObject.getRemote = function(name, remotePath, persistence, secure) {
                const key = `remote:${remotePath || ''}:${name}`;
                if (!sharedStore.has(key)) {
                    sharedStore.set(key, makeSharedObject(name, remotePath, true, persistence, secure));
                }
                return sharedStore.get(key);
            };
        }
    }

    if (typeof legacy.MovieClipLoader !== 'function') {
        function MovieClipLoader() {
            this.__nfBrand = 'MovieClipLoader';
            this.checkPolicyFile = false;
            this._listeners = [];
        }
        MovieClipLoader.prototype.addListener = function(listener) {
            if (listener && !this._listeners.includes(listener)) this._listeners.push(listener);
            return true;
        };
        MovieClipLoader.prototype.removeListener = function(listener) {
            const idx = this._listeners.indexOf(listener);
            if (idx < 0) return false;
            this._listeners.splice(idx, 1);
            return true;
        };
        MovieClipLoader.prototype.loadClip = function(url, target) {
            const dest = (typeof target === 'number') ? legacy.loadMovieNum(url, target) : legacy.loadMovie.call(frame || this, url, target);
            if (dest && typeof dest === 'object') dest.__nfPolicyChecked = Boolean(this.checkPolicyFile);
            for (const l of this._listeners.slice()) {
                l?.onLoadStart?.(dest);
                l?.onLoadProgress?.(dest, 1024, 1024);
                l?.onLoadComplete?.(dest, 200);
                l?.onLoadInit?.(dest);
            }
            return true;
        };
        MovieClipLoader.prototype.unloadClip = function(target) {
            return typeof target === 'number' ? legacy.unloadMovieNum(target) : legacy.unloadMovie.call(frame || this, target);
        };
        MovieClipLoader.prototype.getProgress = function() {
            return { bytesLoaded: 1024, bytesTotal: 1024 };
        };
        MovieClipLoader.__nfIs = (v) => v instanceof MovieClipLoader || Boolean(v && v.__nfBrand === 'MovieClipLoader');
        legacy.MovieClipLoader = MovieClipLoader;
    }

    if (typeof legacy.Sound !== 'function') {
        function Sound(target) {
            this.__nfBrand = 'Sound';
            this.target = target || null;
            this.checkPolicyFile = false;
            this._volume = 100;
            this._pan = 0;
            this.duration = 0;
            this.position = 0;
        }
        Sound.prototype.loadSound = function(url, isStreaming) {
            this.url = String(url ?? '');
            this.isStreaming = Boolean(isStreaming);
            this.policyChecked = Boolean(this.checkPolicyFile);
            this.duration = 1000;
            if (typeof this.onLoad === 'function') this.onLoad(true);
        };
        Sound.prototype.start = function(secondOffset = 0, loops = 1) {
            this.position = (Number(secondOffset) || 0) * 1000;
            this.loops = Number(loops) || 1;
        };
        Sound.prototype.stop = function() { this.position = 0; };
        Sound.prototype.getVolume = function() { return this._volume; };
        Sound.prototype.setVolume = function(v) { this._volume = Number(v) || 0; };
        Sound.prototype.getPan = function() { return this._pan; };
        Sound.prototype.setPan = function(v) { this._pan = Number(v) || 0; };
        Sound.prototype.getBytesLoaded = function() { return 1024; };
        Sound.prototype.getBytesTotal = function() { return 1024; };
        Sound.__nfIs = (v) => v instanceof Sound || Boolean(v && v.__nfBrand === 'Sound');
        legacy.Sound = Sound;
    }

    if (typeof legacy.Video !== 'function') {
        function Video() {
            this.__nfBrand = 'Video';
            this._deblocking = 0;
            this.smoothing = false;
            this.width = 160;
            this.height = 120;
        }
        __nfSafeDefine(Video.prototype, 'deblocking', {
            get() { return this._deblocking ?? 0; },
            set(v) {
                const n = Number(v) | 0;
                this._deblocking = Math.max(0, Math.min(5, n));
            },
        });
        Video.prototype.attachVideo = function(source) { this.source = source || null; };
        Video.prototype.clear = function() { this.source = null; };
        Video.__nfIs = (v) => v instanceof Video || Boolean(v && v.__nfBrand === 'Video');
        legacy.Video = Video;
    } else if (legacy.Video.prototype) {
        __nfSafeDefine(legacy.Video.prototype, 'deblocking', {
            get() { return this._deblocking ?? 0; },
            set(v) {
                const n = Number(v) | 0;
                this._deblocking = Math.max(0, Math.min(5, n));
            },
        });
    }
    if (frame && typeof frame === 'object' && frame.placedVideo && typeof frame.placedVideo === 'object') {
        Object.setPrototypeOf(frame.placedVideo, legacy.Video.prototype);
    }

    if (!legacy.ExternalInterface) {
        const callbacks = {};
        const calls = [];
        legacy.ExternalInterface = {
            available: true,
            _callbacks: callbacks,
            calls,
            addCallback(methodName, instance, method) {
                if (!methodName || typeof method !== 'function') return false;
                callbacks[String(methodName)] = (...args) => method.apply(instance || null, args);
                return true;
            },
            call(methodName, ...args) {
                const name = String(methodName ?? '');
                calls.push({ methodName: name, args });
                if (typeof callbacks[name] === 'function') {
                    return callbacks[name](...args);
                }
                if (typeof globalThis !== 'undefined' && typeof globalThis[name] === 'function') {
                    return globalThis[name](...args);
                }
                return null;
            },
            __nfIs(v) { return v === legacy.ExternalInterface; },
        };
    }

    if (!legacy.flash) legacy.flash = {};
    if (!legacy.flash.text) legacy.flash.text = {};
    legacy.flash.text.TextRenderer = legacy.TextRenderer;
    if (!legacy.flash.net) legacy.flash.net = {};
    legacy.flash.net.FileReference = legacy.FileReference;
    legacy.flash.net.FileReferenceList = legacy.FileReferenceList;
    if (!legacy.flash.external) legacy.flash.external = {};
    legacy.flash.external.ExternalInterface = legacy.ExternalInterface;

    if (!legacy._global) legacy._global = {};
    if (Object.isExtensible(legacy._global)) {
        legacy._global.System = legacy.System;
        legacy._global.NetConnection = legacy.NetConnection;
        legacy._global.NetStream = legacy.NetStream;
        legacy._global.Camera = legacy.Camera;
        legacy._global.Microphone = legacy.Microphone;
        legacy._global.SharedObject = legacy.SharedObject;
        legacy._global.MovieClipLoader = legacy.MovieClipLoader;
        legacy._global.Sound = legacy.Sound;
        legacy._global.Video = legacy.Video;
        legacy._global.flash = legacy.flash;
    }
    } catch (_) {}

    return legacy;
}

const __nfWave7Registry = (() => {
    const classes = new Map();

    class Wave7EventDispatcher {
        constructor() {
            this._listeners = new Map();
        }
        addEventListener(type, listener) {
            const key = String(type ?? '');
            if (typeof listener !== 'function') return;
            if (!this._listeners.has(key)) this._listeners.set(key, []);
            const list = this._listeners.get(key);
            if (!list.includes(listener)) list.push(listener);
        }
        removeEventListener(type, listener) {
            const key = String(type ?? '');
            const list = this._listeners.get(key);
            if (!list) return;
            const idx = list.indexOf(listener);
            if (idx >= 0) list.splice(idx, 1);
        }
        hasEventListener(type) {
            const list = this._listeners.get(String(type ?? ''));
            return Boolean(list && list.length > 0);
        }
        willTrigger(type) {
            return this.hasEventListener(type);
        }
        dispatchEvent(event) {
            if (!event || !event.type) return false;
            if (!event.target) event.target = this;
            event.currentTarget = this;
            const list = this._listeners.get(String(event.type)) || [];
            for (const fn of list.slice()) {
                fn.call(this, event);
            }
            return true;
        }
    }

    class Rectangle {
        constructor(x = 0, y = 0, width = 0, height = 0) {
            Object.assign(this, {
                x: Number(x) || 0,
                y: Number(y) || 0,
                width: Number(width) || 0,
                height: Number(height) || 0,
            });
        }
        clone() { return new Rectangle(this.x, this.y, this.width, this.height); }
    }
    Rectangle.__nfIs = (v) => v instanceof Rectangle || Boolean(v && typeof v.x === 'number' && typeof v.width === 'number');

    class ByteArray {
        constructor() {
            this._bytes = [];
            this.position = 0;
            this.endian = 'bigEndian';
        }
        get length() { return this._bytes.length; }
        set length(v) { this._bytes.length = Math.max(0, Number(v) | 0); }
        get bytesAvailable() { return Math.max(0, this._bytes.length - this.position); }
        writeByte(v) {
            this._bytes[this.position++] = (Number(v) | 0) & 0xFF;
        }
        writeUnsignedInt(v) {
            const u = Number(v) >>> 0;
            this._bytes[this.position++] = (u >>> 24) & 0xFF;
            this._bytes[this.position++] = (u >>> 16) & 0xFF;
            this._bytes[this.position++] = (u >>> 8) & 0xFF;
            this._bytes[this.position++] = u & 0xFF;
        }
        writeUTFBytes(str) {
            const s = String(str ?? '');
            for (let i = 0; i < s.length; i++) {
                this._bytes[this.position++] = s.charCodeAt(i) & 0xFF;
            }
        }
        readUnsignedByte() {
            return this._bytes[this.position++] ?? 0;
        }
    }
    ByteArray.__nfIs = (v) => v instanceof ByteArray || Boolean(v && Array.isArray(v._bytes));

    class ApplicationDomain {
        constructor(parentDomain = null) {
            this.parentDomain = parentDomain || null;
        }
        static get currentDomain() {
            if (!ApplicationDomain._current) ApplicationDomain._current = new ApplicationDomain(null);
            return ApplicationDomain._current;
        }
    }
    ApplicationDomain.__nfIs = (v) => v instanceof ApplicationDomain || Boolean(v && 'parentDomain' in v);

    class SecurityDomain {
        static get currentDomain() {
            if (!SecurityDomain._current) SecurityDomain._current = new SecurityDomain();
            return SecurityDomain._current;
        }
    }
    SecurityDomain.__nfIs = (v) => v instanceof SecurityDomain;

    class LoaderContext {
        constructor(checkPolicyFile = false, applicationDomain = null, securityDomain = null) {
            this.checkPolicyFile = Boolean(checkPolicyFile);
            this.applicationDomain = applicationDomain || null;
            this.securityDomain = securityDomain || null;
            this.allowCodeImport = true;
            this.imageDecodingPolicy = 'onDemand';
            this.parameters = null;
            this.requestedContentParent = null;
        }
    }
    LoaderContext.__nfIs = (v) => v instanceof LoaderContext || Boolean(v && 'checkPolicyFile' in v && 'allowCodeImport' in v);

    class JPEGLoaderContext extends LoaderContext {
        constructor(deblockingFilter = 0.0, checkPolicyFile = false, applicationDomain = null, securityDomain = null) {
            super(checkPolicyFile, applicationDomain, securityDomain);
            this.deblockingFilter = Number(deblockingFilter) || 0;
        }
    }
    JPEGLoaderContext.__nfIs = (v) => v instanceof JPEGLoaderContext || Boolean(v && 'deblockingFilter' in v && LoaderContext.__nfIs(v));

    const Capabilities = {
        avHardwareDisable: false,
        cpuAddressSize: 64,
        cpuArchitecture: 'ARM',
        hasAccessibility: true,
        hasAudio: true,
        hasAudioEncoder: true,
        hasEmbeddedVideo: true,
        hasIME: true,
        hasMP3: true,
        hasPrinting: true,
        hasScreenBroadcast: false,
        hasScreenPlayback: false,
        hasStreamingAudio: true,
        hasStreamingVideo: true,
        hasTLS: true,
        hasVideoEncoder: true,
        isDebugger: false,
        isEmbeddedInAcrobat: false,
        language: 'en',
        localFileReadDisable: false,
        manufacturer: 'NeoFlash Macintosh',
        maxLevelIDC: '5.1',
        os: 'Mac OS 15.0',
        pixelAspectRatio: 1,
        playerType: 'PlugIn',
        screenColor: 'color',
        screenDPI: 72,
        screenResolutionX: 1920,
        screenResolutionY: 1080,
        supports32BitProcesses: false,
        supports64BitProcesses: true,
        Supports32BitProcesses: false,
        Supports64BitProcesses: true,
        touchscreenType: 'none',
        version: 'MAC 32,0,0,0',
        get serverString() {
            const b = (flag) => (flag ? 't' : 'f');
            return [
                `A=${b(this.hasAudio)}`,
                `SA=${b(this.hasStreamingAudio)}`,
                `SV=${b(this.hasStreamingVideo)}`,
                `EV=${b(this.hasEmbeddedVideo)}`,
                `MP3=${b(this.hasMP3)}`,
                `AE=${b(this.hasAudioEncoder)}`,
                `VE=${b(this.hasVideoEncoder)}`,
                `ACC=${b(this.hasAccessibility)}`,
                `PR=${b(this.hasPrinting)}`,
                `SP=${b(this.hasScreenPlayback)}`,
                `SB=${b(this.hasScreenBroadcast)}`,
                `DEB=${b(this.isDebugger)}`,
                `V=${encodeURIComponent(String(this.version))}`,
                `M=${encodeURIComponent(String(this.manufacturer))}`,
                `R=${this.screenResolutionX}x${this.screenResolutionY}`,
                `DP=${this.screenDPI}`,
                `COL=${encodeURIComponent(String(this.screenColor))}`,
                `AR=${this.pixelAspectRatio}`,
                `OS=${encodeURIComponent(String(this.os))}`,
                `ARCH=${encodeURIComponent(String(this.cpuArchitecture))}`,
                `L=${encodeURIComponent(String(this.language))}`,
                `IME=${b(this.hasIME)}`,
                `PR32=${b(this.Supports32BitProcesses)}`,
                `PR64=${b(this.Supports64BitProcesses)}`,
                `PT=${encodeURIComponent(String(this.playerType))}`,
                `AVD=${b(Capabilities.avHardwareDisable)}`,
                `LFD=${b(Capabilities.localFileReadDisable)}`,
                `TLS=${b(Capabilities.hasTLS)}`,
                `ML=${encodeURIComponent(String(Capabilities.maxLevelIDC))}`,
            ].join('&');
        },
        hasMultiChannelAudio(type) {
            const t = String(type ?? '');
            return ['5.1', '7.1', 'stereo', 'DolbyDigital', 'DolbyDigitalPlus', 'DTS', 'DTSExpress', 'DTSHDHighResolutionAudio', 'DTSHDMasterAudio'].includes(t);
        },
        __nfIs(v) { return v === Capabilities; },
    };

    class VideoTexture extends Wave7EventDispatcher {
        constructor() {
            super();
            this.videoWidth = 0;
            this.videoHeight = 0;
            this.netStream = null;
            this.camera = null;
            this.disposed = false;
        }
        attachNetStream(netStream) {
            this.netStream = netStream || null;
            this.videoWidth = netStream ? (netStream.videoWidth || 320) : 0;
            this.videoHeight = netStream ? (netStream.videoHeight || 240) : 0;
            this.dispatchEvent({ type: 'renderState', status: 'accelerated' });
        }
        attachCamera(theCamera) {
            this.camera = theCamera || null;
            this.videoWidth = theCamera ? (theCamera.width || 160) : 0;
            this.videoHeight = theCamera ? (theCamera.height || 120) : 0;
            this.dispatchEvent({ type: 'renderState', status: 'accelerated' });
        }
        dispose() {
            this.netStream = null;
            this.camera = null;
            this.videoWidth = 0;
            this.videoHeight = 0;
            this.disposed = true;
        }
    }
    VideoTexture.__nfIs = (v) => v instanceof VideoTexture || Boolean(v && typeof v.attachNetStream === 'function' && 'videoWidth' in v);

    function parseCompressedAtf(target, data, byteArrayOffset, asyncFlag) {
        if (!data) throw new __nfTypeError('Compressed texture ByteArray data is required.');
        const bytes = Array.isArray(data._bytes) ? data._bytes : (data instanceof Uint8Array ? Array.from(data) : []);
        const offset = Math.max(0, Number(byteArrayOffset) | 0);
        if (offset > bytes.length) {
            throw __nfMakeError('RangeError', true, ['ByteArray offset out of bounds.', 2006]);
        }
        if (bytes[offset] === 0x41 && bytes[offset + 1] === 0x54 && bytes[offset + 2] === 0x46) {
            const fmtByte = bytes[offset + 6] ?? 0;
            const log2W = bytes[offset + 7] ?? 0;
            const log2H = bytes[offset + 8] ?? 0;
            const count = bytes[offset + 9] ?? 1;
            target.atfFormat = fmtByte & 0x7F;
            target.width = 1 << log2W;
            target.height = 1 << log2H;
            target.mipCount = count;
        }
        target.compressedUploaded = true;
        target.uploadedByteLength = bytes.length - offset;
        target.asyncUpload = Boolean(asyncFlag);
        if (Boolean(asyncFlag) && typeof target.dispatchEvent === 'function') {
            target.dispatchEvent({ type: 'textureReady' });
        }
    }

    class Texture extends Wave7EventDispatcher {
        constructor(width = 256, height = 256) {
            super();
            this.width = Number(width) || 256;
            this.height = Number(height) || 256;
            this.compressedUploaded = false;
        }
        uploadCompressedTextureFromByteArray(data, byteArrayOffset, async = false) {
            parseCompressedAtf(this, data, byteArrayOffset, async);
        }
        dispose() { this.disposed = true; }
    }
    Texture.__nfIs = (v) => v instanceof Texture || Boolean(v && typeof v.uploadCompressedTextureFromByteArray === 'function' && !v.isCube);

    class CubeTexture extends Wave7EventDispatcher {
        constructor(size = 256) {
            super();
            this.isCube = true;
            this.size = Number(size) || 256;
            this.width = this.size;
            this.height = this.size;
            this.compressedUploaded = false;
        }
        uploadCompressedTextureFromByteArray(data, byteArrayOffset, async = false) {
            parseCompressedAtf(this, data, byteArrayOffset, async);
        }
        dispose() { this.disposed = true; }
    }
    CubeTexture.__nfIs = (v) => v instanceof CubeTexture || Boolean(v && v.isCube);

    class Context3D extends Wave7EventDispatcher {
        constructor() {
            super();
            this.driverInfo = 'NeoFlash Stage3D Baseline';
            this.enableErrorChecking = false;
        }
        static get supportsVideoTexture() { return true; }
        createVideoTexture() {
            return new VideoTexture();
        }
        createTexture(width, height) {
            return new Texture(width, height);
        }
        createCubeTexture(size) {
            return new CubeTexture(size);
        }
    }
    Context3D.__nfIs = (v) => v instanceof Context3D || Boolean(v && typeof v.createVideoTexture === 'function');

    const VideoCodec = Object.freeze({
        H264AVC: 'H264Avc',
        SORENSON: 'Sorenson',
        VP6: 'On2 VP6',
        __nfIs: (v) => v === VideoCodec,
    });

    const H264Profile = Object.freeze({
        BASELINE: 'baseline',
        MAIN: 'main',
        __nfIs: (v) => v === H264Profile,
    });

    const H264Level = Object.freeze({
        LEVEL_1: '1',
        LEVEL_1B: '1b',
        LEVEL_1_1: '1.1',
        LEVEL_1_2: '1.2',
        LEVEL_1_3: '1.3',
        LEVEL_2: '2',
        LEVEL_2_1: '2.1',
        LEVEL_2_2: '2.2',
        LEVEL_3: '3',
        LEVEL_3_1: '3.1',
        LEVEL_3_2: '3.2',
        LEVEL_4: '4',
        LEVEL_4_1: '4.1',
        LEVEL_4_2: '4.2',
        LEVEL_5: '5',
        LEVEL_5_1: '5.1',
        __nfIs: (v) => v === H264Level,
    });

    class VideoStreamSettings {
        constructor() {
            this.codec = VideoCodec.SORENSON;
            this.width = 160;
            this.height = 120;
            this.fps = 15;
            this.bandwidth = 16384;
            this.quality = 0;
            this.keyFrameInterval = 15;
        }
        setMode(width, height, fps) {
            if (width !== undefined) this.width = Math.max(1, Number(width) | 0);
            if (height !== undefined) this.height = Math.max(1, Number(height) | 0);
            if (fps !== undefined) this.fps = Math.max(0, Number(fps));
        }
        setQuality(bandwidth, quality) {
            if (bandwidth !== undefined) this.bandwidth = Math.max(0, Number(bandwidth) | 0);
            if (quality !== undefined) this.quality = Math.max(0, Math.min(100, Number(quality) | 0));
        }
        setKeyFrameInterval(keyFrameInterval) {
            const k = Number(keyFrameInterval) | 0;
            this.keyFrameInterval = k < 1 ? 1 : (k > 300 ? 300 : k);
        }
    }
    VideoStreamSettings.__nfIs = (v) => v instanceof VideoStreamSettings || Boolean(v && typeof v.setMode === 'function' && 'keyFrameInterval' in v);

    class H264VideoStreamSettings extends VideoStreamSettings {
        constructor() {
            super();
            this.codec = VideoCodec.H264AVC;
            this.profile = H264Profile.BASELINE;
            this.level = H264Level.LEVEL_2_1;
        }
        setProfileLevel(profile, level) {
            const p = String(profile ?? '');
            const l = String(level ?? '');
            if (p !== H264Profile.BASELINE && p !== H264Profile.MAIN) {
                throw __nfMakeError('ArgumentError', true, ['Invalid H264 profile.', 2004]);
            }
            this.profile = p;
            this.level = l;
        }
    }
    H264VideoStreamSettings.__nfIs = (v) => v instanceof H264VideoStreamSettings || Boolean(v && typeof v.setProfileLevel === 'function');

    class VP6VideoStreamSettings extends VideoStreamSettings {
        constructor() {
            super();
            this.codec = VideoCodec.VP6;
        }
    }
    VP6VideoStreamSettings.__nfIs = (v) => v instanceof VP6VideoStreamSettings || Boolean(v && v.codec === VideoCodec.VP6 && VideoStreamSettings.__nfIs(v));

    const AudioDecoder = Object.freeze({
        DOLBY_DIGITAL: 'DolbyDigital',
        DOLBY_DIGITAL_PLUS: 'DolbyDigitalPlus',
        DTS: 'DTS',
        DTS_EXPRESS: 'DTSExpress',
        DTS_HD_HIGH_RESOLUTION_AUDIO: 'DTSHDHighResolutionAudio',
        DTS_HD_MASTER_AUDIO: 'DTSHDMasterAudio',
        __nfIs: (v) => v === AudioDecoder,
    });

    const AudioOutputChangeReason = Object.freeze({
        USER_SELECTION: 'userSelection',
        DEVICE_CHANGE: 'deviceChange',
        __nfIs: (v) => v === AudioOutputChangeReason,
    });

    class AudioOutputChangeEvent {
        constructor(type = 'audioOutputChange', bubbles = false, cancelable = false, reason = null) {
            this.type = String(type || 'audioOutputChange');
            this.bubbles = Boolean(bubbles);
            this.cancelable = Boolean(cancelable);
            this.reason = reason !== undefined && reason !== null ? String(reason) : null;
        }
        static get AUDIO_OUTPUT_CHANGE() { return 'audioOutputChange'; }
        clone() {
            return new AudioOutputChangeEvent(this.type, this.bubbles, this.cancelable, this.reason);
        }
    }
    AudioOutputChangeEvent.__nfIs = (v) => v instanceof AudioOutputChangeEvent || Boolean(v && v.type === 'audioOutputChange' && 'reason' in v);

    class AudioDeviceManager extends Wave7EventDispatcher {
        constructor() {
            super();
            this.deviceNames = ['Default Audio Output', 'Headphones'];
            this._selectedDeviceIndex = 0;
        }
        static get isSupported() { return true; }
        static get deviceManager() {
            if (!AudioDeviceManager._instance) {
                AudioDeviceManager._instance = new AudioDeviceManager();
            }
            return AudioDeviceManager._instance;
        }
        get selectedDeviceIndex() { return this._selectedDeviceIndex; }
        set selectedDeviceIndex(v) {
            const idx = Number(v) | 0;
            if (idx < 0 || idx >= this.deviceNames.length) {
                throw __nfMakeError('RangeError', true, ['Audio device index out of bounds.', 2006]);
            }
            this._selectedDeviceIndex = idx;
            this.dispatchEvent(new AudioOutputChangeEvent('audioOutputChange', false, false, AudioOutputChangeReason.USER_SELECTION));
        }
    }
    AudioDeviceManager.__nfIs = (v) => v instanceof AudioDeviceManager || Boolean(v && Array.isArray(v.deviceNames) && 'selectedDeviceIndex' in v);

    class Responder {
        constructor(result, status = null) {
            Object.assign(this, {
                result: typeof result === 'function' ? result : null,
                status: typeof status === 'function' ? status : null,
            });
        }
    }
    Responder.__nfIs = (v) => v instanceof Responder || Boolean(v && 'result' in v && 'status' in v);

    const NetStreamPlayTransitions = Object.freeze({
        APPEND: 'append',
        APPEND_AND_WAIT: 'appendAndWait',
        RESET: 'reset',
        RESUME: 'resume',
        STOP: 'stop',
        SWAP: 'swap',
        SWITCH: 'switch',
        __nfIs: (v) => v === NetStreamPlayTransitions,
    });

    class NetStreamPlayOptions extends Wave7EventDispatcher {
        constructor() {
            super();
            this.streamName = null;
            this.oldStreamName = null;
            this.start = -2;
            this.len = -1;
            this.offset = -1;
            this.transition = NetStreamPlayTransitions.SWITCH;
        }
    }
    NetStreamPlayOptions.__nfIs = (v) => v instanceof NetStreamPlayOptions || Boolean(v && 'transition' in v && 'oldStreamName' in v);

    class As3NetConnection extends Wave7EventDispatcher {
        constructor() {
            super();
            this.connected = false;
            this.uri = null;
            this.client = this;
        }
        connect(command) {
            this.uri = command == null ? null : String(command);
            this.connected = true;
            this.dispatchEvent({ type: 'netStatus', info: { level: 'status', code: 'NetConnection.Connect.Success' } });
        }
        close() {
            this.connected = false;
            this.dispatchEvent({ type: 'netStatus', info: { level: 'status', code: 'NetConnection.Connect.Closed' } });
        }
        call(command, responder, ...args) {
            const payload = { command: String(command ?? ''), args };
            if (responder && typeof responder.result === 'function') {
                responder.result(payload);
            }
        }
    }
    As3NetConnection.__nfIs = (v) => v instanceof As3NetConnection || Boolean(v && typeof v.connect === 'function' && 'connected' in v);

    class As3NetStream extends Wave7EventDispatcher {
        constructor(connection) {
            super();
            this.connection = connection || null;
            this._checkPolicyFile = false;
            this.bufferTime = 0.1;
            this.bufferLength = 0;
            this.bytesLoaded = 0;
            this.bytesTotal = 0;
            this.currentFPS = 0;
            this.time = 0;
            this.client = this;
            this.videoWidth = 320;
            this.videoHeight = 240;
            this.videoStreamSettings = null;
        }
        get checkPolicyFile() { return this._checkPolicyFile; }
        set checkPolicyFile(v) { this._checkPolicyFile = Boolean(v); }
        play(...args) {
            this.playingUrl = String(args[0] ?? '');
            this.bytesTotal = 1024;
            this.bytesLoaded = 1024;
            this.currentFPS = 30;
            this.dispatchEvent({ type: 'netStatus', info: { level: 'status', code: 'NetStream.Play.Start' } });
        }
        play2(param) {
            this.play2Options = param || null;
            this.playingUrl = String(param?.streamName ?? '');
            this.currentFPS = 30;
            this.dispatchEvent({ type: 'netStatus', info: { level: 'status', code: 'NetStream.Play.Transition' } });
        }
        attachCamera(theCamera) { this.camera = theCamera || null; }
        attachAudio(microphone) { this.microphone = microphone || null; }
        pause() { this.currentFPS = 0; }
        resume() { this.currentFPS = 30; }
        seek(offset) { this.time = Math.max(0, Number(offset) || 0); }
        close() { this.playingUrl = null; this.currentFPS = 0; }
    }
    As3NetStream.__nfIs = (v) => v instanceof As3NetStream || Boolean(v && 'checkPolicyFile' in v && typeof v.play2 === 'function');

    class As3SharedObject extends Wave7EventDispatcher {
        constructor(name, localPath = '/') {
            super();
            this.name = String(name ?? '');
            this['localPath'] = String(localPath || '/');
            this.data = {};
            this.client = this;
            this._fps = 0;
            this.objectEncoding = 3;
            this._dirty = new Set();
        }
        static get _store() {
            if (!As3SharedObject.__store) As3SharedObject.__store = new Map();
            return As3SharedObject.__store;
        }
        static getLocal(name, localPath = '/', secure = false) {
            const key = `local:${localPath || '/'}:${name}`;
            if (!As3SharedObject._store.has(key)) {
                const so = new As3SharedObject(name, localPath);
                so.secure = Boolean(secure);
                As3SharedObject._store.set(key, so);
            }
            return As3SharedObject._store.get(key);
        }
        static getRemote(name, remotePath = null, persistence = false, secure = false) {
            const key = `remote:${remotePath || ''}:${name}`;
            if (!As3SharedObject._store.has(key)) {
                const so = new As3SharedObject(name, remotePath || '/');
                so.remotePath = remotePath;
                so.persistence = persistence;
                so.secure = Boolean(secure);
                As3SharedObject._store.set(key, so);
            }
            return As3SharedObject._store.get(key);
        }
        get fps() { return this._fps; }
        set fps(v) { this._fps = Math.max(0, Number(v) || 0); }
        setDirty(propertyName) {
            const prop = String(propertyName ?? '');
            this._dirty.add(prop);
            this.dispatchEvent({
                type: 'sync',
                changeList: [{ code: 'change', name: prop, oldValue: undefined }],
            });
        }
        setProperty(propertyName, value = null) {
            const prop = String(propertyName ?? '');
            const oldValue = this.data[prop];
            if (value === undefined) delete this.data[prop];
            else this.data[prop] = value;
            this._dirty.add(prop);
            this.dispatchEvent({
                type: 'sync',
                changeList: [{ code: 'change', name: prop, oldValue }],
            });
        }
        connect(myConnection, params = null) {
            this.connection = myConnection || null;
            this.connectParams = params;
            if (!myConnection || myConnection.connected === false) {
                this.dispatchEvent({ type: 'netStatus', info: { level: 'error', code: 'SharedObject.Connect.Failed' } });
                return;
            }
            this.dispatchEvent({
                type: 'sync',
                changeList: Object.keys(this.data).map(k => ({ code: 'change', name: k })),
            });
        }
        close() {
            this.connection = null;
        }
        send(...args) {
            const handlerName = String(args[0] ?? '');
            const rest = args.slice(1);
            try {
                if (this.client && typeof this.client[handlerName] === 'function') {
                    this.client[handlerName](...rest);
                }
            } catch (err) {
                this.dispatchEvent({ type: 'asyncError', error: err, text: String(err?.message || err) });
            }
        }
        flush() { return 'flushed'; }
        clear() {
            for (const k of Object.keys(this.data)) delete this.data[k];
            this.dispatchEvent({ type: 'sync', changeList: [{ code: 'clear', name: '' }] });
        }
    }
    As3SharedObject.__nfIs = (v) => v instanceof As3SharedObject || Boolean(v && typeof v.setDirty === 'function' && typeof v.setProperty === 'function');

    const ColorCorrection = Object.freeze({
        DEFAULT: 'default',
        OFF: 'off',
        ON: 'on',
        __nfIs: (v) => v === ColorCorrection,
    });

    const ColorCorrectionSupport = Object.freeze({
        DEFAULT_OFF: 'defaultOff',
        DEFAULT_ON: 'defaultOn',
        UNSUPPORTED: 'unsupported',
        __nfIs: (v) => v === ColorCorrectionSupport,
    });

    class InteractiveObject extends Wave7EventDispatcher {
        constructor() {
            super();
            this.needsSoftKeyboard = false;
            this.softKeyboardInputAreaOfInterest = null;
            this.tabEnabled = false;
            this.mouseEnabled = true;
        }
        requestSoftKeyboard() {
            const rect = this.softKeyboardInputAreaOfInterest instanceof Rectangle
                ? new Rectangle(this.softKeyboardInputAreaOfInterest.x, this.softKeyboardInputAreaOfInterest.y, Math.max(320, this.softKeyboardInputAreaOfInterest.width), Math.max(240, this.softKeyboardInputAreaOfInterest.height))
                : new Rectangle(0, 820, 1920, 260);
            Stage.softKeyboardRect = rect;
            this.dispatchEvent({ type: 'softKeyboardActivating' });
            this.dispatchEvent({ type: 'softKeyboardActivated' });
            return true;
        }
    }
    InteractiveObject.__nfIs = (v) => v instanceof InteractiveObject || Boolean(v && typeof v.requestSoftKeyboard === 'function');

    class AVM1Movie extends InteractiveObject {
        constructor() {
            super();
            this._callbacks = {};
        }
        addCallback(functionName, closure) {
            if (functionName && typeof closure === 'function') {
                this._callbacks[String(functionName)] = closure;
            }
        }
        call(functionName, ...args) {
            const fn = this._callbacks[String(functionName ?? '')];
            return typeof fn === 'function' ? fn(...args) : null;
        }
    }
    AVM1Movie.__nfIs = (v) => v instanceof AVM1Movie || Boolean(v && typeof v.addCallback === 'function' && typeof v.call === 'function');

    const Stage = {
        softKeyboardRect: new Rectangle(0, 0, 0, 0),
        stageVideos: [{ videoWidth: 1920, videoHeight: 1080, viewPort: new Rectangle(0, 0, 1920, 1080) }],
        colorCorrection: ColorCorrection.DEFAULT,
        colorCorrectionSupport: ColorCorrectionSupport.DEFAULT_ON,
        stageWidth: 1920,
        stageHeight: 1080,
        __nfIs: (v) => v === Stage || Boolean(v && 'softKeyboardRect' in v && 'stageVideos' in v),
    };

    const entries = [
        ['Rectangle', Rectangle],
        ['flash.geom.Rectangle', Rectangle],
        ['ByteArray', ByteArray],
        ['flash.utils.ByteArray', ByteArray],
        ['ApplicationDomain', ApplicationDomain],
        ['flash.system.ApplicationDomain', ApplicationDomain],
        ['SecurityDomain', SecurityDomain],
        ['flash.system.SecurityDomain', SecurityDomain],
        ['LoaderContext', LoaderContext],
        ['flash.system.LoaderContext', LoaderContext],
        ['JPEGLoaderContext', JPEGLoaderContext],
        ['flash.system.JPEGLoaderContext', JPEGLoaderContext],
        ['Capabilities', Capabilities],
        ['flash.system.Capabilities', Capabilities],
        ['VideoTexture', VideoTexture],
        ['flash.display3D.textures.VideoTexture', VideoTexture],
        ['Texture', Texture],
        ['flash.display3D.textures.Texture', Texture],
        ['CubeTexture', CubeTexture],
        ['flash.display3D.textures.CubeTexture', CubeTexture],
        ['Context3D', Context3D],
        ['flash.display3D.Context3D', Context3D],
        ['VideoCodec', VideoCodec],
        ['flash.media.VideoCodec', VideoCodec],
        ['H264Profile', H264Profile],
        ['flash.media.H264Profile', H264Profile],
        ['H264Level', H264Level],
        ['flash.media.H264Level', H264Level],
        ['VideoStreamSettings', VideoStreamSettings],
        ['flash.media.VideoStreamSettings', VideoStreamSettings],
        ['H264VideoStreamSettings', H264VideoStreamSettings],
        ['flash.media.H264VideoStreamSettings', H264VideoStreamSettings],
        ['VP6VideoStreamSettings', VP6VideoStreamSettings],
        ['flash.media.VP6VideoStreamSettings', VP6VideoStreamSettings],
        ['AudioDecoder', AudioDecoder],
        ['flash.media.AudioDecoder', AudioDecoder],
        ['AudioOutputChangeReason', AudioOutputChangeReason],
        ['flash.media.AudioOutputChangeReason', AudioOutputChangeReason],
        ['AudioOutputChangeEvent', AudioOutputChangeEvent],
        ['flash.events.AudioOutputChangeEvent', AudioOutputChangeEvent],
        ['AudioDeviceManager', AudioDeviceManager],
        ['flash.media.AudioDeviceManager', AudioDeviceManager],
        ['Responder', Responder],
        ['flash.net.Responder', Responder],
        ['NetStreamPlayTransitions', NetStreamPlayTransitions],
        ['flash.net.NetStreamPlayTransitions', NetStreamPlayTransitions],
        ['NetStreamPlayOptions', NetStreamPlayOptions],
        ['flash.net.NetStreamPlayOptions', NetStreamPlayOptions],
        ['NetConnection', As3NetConnection],
        ['flash.net.NetConnection', As3NetConnection],
        ['NetStream', As3NetStream],
        ['flash.net.NetStream', As3NetStream],
        ['SharedObject', As3SharedObject],
        ['flash.net.SharedObject', As3SharedObject],
        ['ColorCorrection', ColorCorrection],
        ['flash.display.ColorCorrection', ColorCorrection],
        ['ColorCorrectionSupport', ColorCorrectionSupport],
        ['flash.display.ColorCorrectionSupport', ColorCorrectionSupport],
        ['InteractiveObject', InteractiveObject],
        ['flash.display.InteractiveObject', InteractiveObject],
        ['AVM1Movie', AVM1Movie],
        ['flash.display.AVM1Movie', AVM1Movie],
        ['Stage', Stage],
        ['flash.display.Stage', Stage],
    ];
    for (const [k, v] of entries) classes.set(k, v);

    const flashNs = {
        system: { ApplicationDomain, SecurityDomain, LoaderContext, JPEGLoaderContext, Capabilities },
        display3D: { Context3D, textures: { VideoTexture, Texture, CubeTexture } },
        media: { VideoCodec, H264Profile, H264Level, VideoStreamSettings, H264VideoStreamSettings, VP6VideoStreamSettings, AudioDecoder, AudioOutputChangeReason, AudioDeviceManager },
        events: { AudioOutputChangeEvent },
        net: { Responder, NetStreamPlayTransitions, NetStreamPlayOptions, NetConnection: As3NetConnection, NetStream: As3NetStream, SharedObject: As3SharedObject },
        display: { ColorCorrection, ColorCorrectionSupport, InteractiveObject, AVM1Movie, Stage },
        geom: { Rectangle },
        utils: { ByteArray },
    };
    classes.set('flash', flashNs);

    return { classes, parseCompressedAtf };
})();

function __nfWave7Class(name) {
    const cls = __nfWave7Registry.classes.get(String(name ?? ''));
    if (!cls) throw new __nfTypeError('Unsupported AS3 Wave-7 class ' + name);
    return cls;
}

function __nfWave7Cast(name, value) {
    if (value == null) return null;
    const cls = __nfWave7Class(name);
    if (typeof cls.__nfIs === 'function' && cls.__nfIs(value)) return value;
    if (typeof cls === 'function' && value instanceof cls) return value;
    throw new __nfTypeError('AS3 ' + name + ' value required');
}

function __nfWave7Builtins(existingBuiltins) {
    const out = {};
    for (const [key, val] of __nfWave7Registry.classes.entries()) {
        if (key.includes('.')) {
            if (!existingBuiltins || existingBuiltins[key] === undefined) {
                out[key] = val;
            }
        }
    }
    if (existingBuiltins && typeof existingBuiltins === 'object') {
        const cap = existingBuiltins['flash.system.Capabilities'];
        const w7Cap = __nfWave7Registry.classes.get('flash.system.Capabilities');
        if (cap && w7Cap) {
            for (const prop of Object.getOwnPropertyNames(w7Cap)) {
                if (prop === 'prototype' || prop === 'length' || prop === 'name') continue;
                const desc = Object.getOwnPropertyDescriptor(w7Cap, prop);
                if (desc && !Object.hasOwn(cap, prop)) __nfSafeDefine(cap, prop, desc);
            }
        }
        const ns = existingBuiltins['flash.net.NetStream'];
        if (ns && ns.prototype) {
            __nfSafeDefine(ns.prototype, 'checkPolicyFile', {
                get() { return Boolean(this._checkPolicyFile); },
                set(v) { this._checkPolicyFile = Boolean(v); },
            });
        }
        const ctx3d = existingBuiltins['flash.display3D.Context3D'];
        const VideoTexture = __nfWave7Registry.classes.get('flash.display3D.textures.VideoTexture');
        if (ctx3d) {
            __nfSafeDefine(ctx3d, 'supportsVideoTexture', { get() { return true; } });
            if (ctx3d.prototype && typeof ctx3d.prototype.createVideoTexture !== 'function') {
                __nfSafeDefine(ctx3d.prototype, 'createVideoTexture', {
                    value: function() { return new VideoTexture(); },
                    writable: true,
                });
            }
        }
        for (const texKey of ['flash.display3D.textures.Texture', 'flash.display3D.textures.CubeTexture']) {
            const texCls = existingBuiltins[texKey];
            if (texCls && texCls.prototype && typeof texCls.prototype.uploadCompressedTextureFromByteArray !== 'function') {
                __nfSafeDefine(texCls.prototype, 'uploadCompressedTextureFromByteArray', {
                    value: function(data, byteArrayOffset, async = false) {
                        __nfWave7Registry.parseCompressedAtf(this, data, byteArrayOffset, async);
                    },
                    writable: true,
                });
            }
        }
        const soCls = existingBuiltins['flash.net.SharedObject'];
        const w7So = __nfWave7Registry.classes.get('flash.net.SharedObject');
        if (soCls && w7So) {
            if (typeof soCls.getRemote !== 'function') {
                __nfSafeDefine(soCls, 'getRemote', { value: w7So.getRemote.bind(w7So), writable: true });
            }
            if (soCls.prototype) {
                for (const m of ['setDirty', 'setProperty', 'connect', 'close', 'send']) {
                    if (typeof soCls.prototype[m] !== 'function') {
                        __nfSafeDefine(soCls.prototype, m, { value: w7So.prototype[m], writable: true });
                    }
                }
                __nfSafeDefine(soCls.prototype, 'fps', {
                    get() { return this._fps ?? 0; },
                    set(v) { this._fps = Math.max(0, Number(v) || 0); },
                });
                __nfSafeDefine(soCls.prototype, 'client', {
                    get() { return this._client ?? this; },
                    set(v) { this._client = v; },
                });
            }
        }
        const ioCls = existingBuiltins['flash.display.InteractiveObject'];
        const w7Io = __nfWave7Registry.classes.get('flash.display.InteractiveObject');
        if (ioCls && ioCls.prototype && w7Io) {
            if (typeof ioCls.prototype.requestSoftKeyboard !== 'function') {
                __nfSafeDefine(ioCls.prototype, 'requestSoftKeyboard', { value: w7Io.prototype.requestSoftKeyboard, writable: true });
            }
            __nfSafeDefine(ioCls.prototype, 'needsSoftKeyboard', {
                get() { return Boolean(this._needsSoftKeyboard); },
                set(v) { this._needsSoftKeyboard = Boolean(v); },
            });
            __nfSafeDefine(ioCls.prototype, 'softKeyboardInputAreaOfInterest', {
                get() { return this._softKeyboardInputAreaOfInterest ?? null; },
                set(v) { this._softKeyboardInputAreaOfInterest = v ?? null; },
            });
        }
        const stgCls = existingBuiltins['flash.display.Stage'];
        const w7Stg = __nfWave7Registry.classes.get('flash.display.Stage');
        if (stgCls && stgCls.prototype && w7Stg) {
            __nfSafeDefine(stgCls.prototype, 'softKeyboardRect', {
                get() { return this._softKeyboardRect || w7Stg.softKeyboardRect; },
                set(v) { this._softKeyboardRect = v; },
            });
            __nfSafeDefine(stgCls.prototype, 'stageVideos', {
                get() { return this._stageVideos || w7Stg.stageVideos; },
            });
        }
    }
    return out;
}

(function() {
    const host = (typeof __nfLegacy !== 'undefined' && __nfLegacy)
        ? __nfLegacy
        : (typeof globalThis !== 'undefined' && globalThis.__nfLegacy ? globalThis.__nfLegacy : {});
    __nfInstallWave7Legacy(host, typeof __nfFrame !== 'undefined' ? __nfFrame : undefined);
    if (typeof globalThis !== 'undefined' && !globalThis.__nfLegacy) {
        globalThis.__nfLegacy = host;
    }
})();

