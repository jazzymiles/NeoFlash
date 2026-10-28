// Bounded reference profiles, not a complete AVM. Helpers are embedded in each
// compiled AS handler; JavaScript/TypeScript globals and prototypes are untouched.
// AS2 details: Ruffle AVM1 globals.rs at fe6cfc545405b888ebb478ce1c211d63147ca49b.
// AS3 precision conversion: Adobe avmplus Number.as / NumberClass.cpp at
// 858d034a3bd3a54d9b70909386435cf4aec81d21. See intrinsic-fixtures.mjs for provenance.
const __nfNativeParseInt = parseInt;
const __nfNativeFixed = Number.prototype.toFixed;
const __nfNativeExponential = Number.prototype.toExponential;
const __nfNativePrecision = Number.prototype.toPrecision;
const __nfApply = Reflect.apply;
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
function __nfParseInt2(value, radix) {
    if (arguments.length === 0) return undefined;
    const explicit = arguments.length > 1;
    let base = explicit ? (__nfNumber(radix) | 0) : 10;
    // AVM1 converts/checks radix before converting the input string, and an
    // explicitly supplied zero/undefined radix is not automatic detection.
    if (explicit && (base < 2 || base > 36)) return NaN;
    let text = __nfString(value);
    const signedHex = /^[+-]0[xX]/.test(text);
    let ignoreSign = false;
    if (signedHex) {
        if (!explicit || base <= 33) return NaN;
        ignoreSign = true;
    } else if (/^0[xX]/.test(text)) {
        text = text.slice(2);
        if (!explicit) base = 16;
    } else if (!explicit && /^[+-]?0[0-7]*$/.test(text)) {
        base = 8;
    }
    text = text.replace(/^[\t\n\r ]+/, '');
    let negative = false;
    let index = 0;
    if (text[0] === '+' || text[0] === '-') {
        negative = !ignoreSign && text[0] === '-';
        index = 1;
    }
    let result = 0;
    const start = index;
    for (; index < text.length; index++) {
        const code = text.charCodeAt(index);
        const digit = code >= 48 && code <= 57 ? code - 48
            : code >= 65 && code <= 90 ? code - 55
            : code >= 97 && code <= 122 ? code - 87 : 36;
        if (digit >= base) break;
        result = result * base + digit;
    }
    if (index === start) return NaN;
    return negative ? -result : result;
}
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
