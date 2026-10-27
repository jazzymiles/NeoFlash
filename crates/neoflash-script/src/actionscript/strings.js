// Bounded primitive String family, embedded after intrinsics and RegExp helpers.
// Original implementation. Pinned Adobe/AVM1 references and deliberate boundaries
// are recorded in tests/strings-fixtures.mjs. No browser prototype is modified.
const __nfStringNative = String;
const __nfStringNumber = Number;
const __nfStringApply = Reflect.apply;
const __nfStringFromCode = String.fromCharCode;
const __nfStringMethods = Object.freeze(Object.fromEntries(
    Object.getOwnPropertyNames(String.prototype)
        .filter(name => name !== 'constructor' && typeof String.prototype[name] === 'function')
        .map(name => [name, String.prototype[name]])
));
const __nfStringNativeMethods = new Set(Object.values(__nfStringMethods));
const __nfStringClasses = new Map();
const __nfStringLimit = 1000000;
const __nfStringArity = Object.freeze({ charAt: 1, charCodeAt: 1, indexOf: 2, lastIndexOf: 2,
    slice: 2, substring: 2, substr: 2, split: 2, replace: 2, match: 1, search: 1,
    toLowerCase: 0, toUpperCase: 0, toString: 0, valueOf: 0 });

function __nfStringUnsupported(detail) {
    throw new __nfTypeError('Unsupported ActionScript String operation: ' + detail);
}
function __nfStringBound(value) {
    if (value.length > __nfStringLimit) {
        throw new __nfTypeError('NeoFlash String operations currently support at most 1000000 UTF-16 code units.');
    }
    return value;
}
function __nfStringCreate(as3, args, isNew = false) {
    if (isNew && !as3) __nfStringUnsupported('AS2 boxed String construction is not implemented; use String(value) or a literal.');
    // Both VMs ignore extra constructor arguments. Typed String slots retain
    // their separate nullable coercion; calling String(null) produces "null".
    return __nfStringBound(args.length ? __nfStringNative(args[0]) : '');
}
function __nfStringFromCharCode(as3, args) {
    if (args.length > __nfStringLimit) __nfStringUnsupported('fromCharCode argument count exceeds the supported string limit.');
    let result = '';
    for (const value of args) {
        const code = __nfStringNumber(value) & 65535;
        // AVM1 stops before a zero unit, including conversion of later arguments.
        if (!as3 && code === 0) break;
        result += __nfStringFromCode(code);
    }
    return result;
}
function __nfStringClass(as3) {
    as3 = Boolean(as3);
    if (!__nfStringClasses.has(as3)) {
        const facade = function(value) {
            // JS constructors cannot return primitive values. The compiler lowers
            // AS3 `new String(...)` directly through __nfStringCreate instead.
            if (new.target) __nfStringUnsupported('construct String directly by name; construction through class aliases is not implemented.');
            return __nfStringCreate(as3, arguments);
        };
        Object.defineProperty(facade, 'fromCharCode', { value: (...args) => __nfStringFromCharCode(as3, args) });
        Object.defineProperty(facade, '__nfIs', { value: value => typeof value === 'string' });
        Object.defineProperty(facade, Symbol.hasInstance, { value: value => typeof value === 'string' });
        Object.freeze(facade.prototype);
        __nfStringClasses.set(as3, Object.freeze(facade));
    }
    return __nfStringClasses.get(as3);
}
function __nfStringMethodValue(value) {
    if (__nfStringNativeMethods.has(value)) {
        __nfStringUnsupported('native method extraction is not implemented; call the method directly on the string.');
    }
    return value;
}
function __nfStringInteger(value) {
    const number = __nfStringNumber(value);
    return Number.isNaN(number) ? 0 : Math.trunc(number);
}
function __nfStringWrapping(index, length) {
    return index < 0 ? Math.max(0, length + index) : Math.min(length, index);
}
function __nfStringPlain(value, operation) {
    // AS's RegExp recognition differs from JS Symbol.match/Symbol.split hooks.
    // Keep this profile explicit rather than invoking either by accident.
    if (value !== null && (typeof value === 'object' || typeof value === 'function')) {
        __nfStringUnsupported(operation + ' requires a primitive pattern/delimiter or supported AS3 RegExp; native JavaScript RegExp and custom pattern objects are not supported.');
    }
    return __nfStringNative(value);
}
function __nfStringSplit(receiver, args, as3) {
    // Limit conversion precedes delimiter conversion in both reference profiles.
    const limit = args.length < 2 || args[1] === undefined || (as3 && args[1] === null) ? 0xffffffff
        : as3 ? __nfStringNumber(args[1]) >>> 0 : Math.max(0, __nfStringNumber(args[1]) | 0);
    const delimiter = args[0];
    if (as3 && limit === 0) return [];
    if (as3 && receiver.length === 0) return [''];
    if (!as3 && delimiter === undefined) return [receiver];
    if (as3 && __nfRegExpIs(delimiter)) return __nfRegExpSplit(receiver, delimiter, limit);
    const pattern = __nfStringPlain(delimiter, 'split');
    if (limit === 0) return [];
    const result = [];
    const push = value => {
        if (result.length >= __nfStringLimit) __nfStringUnsupported('split output exceeds 1000000 items.');
        result.push(value);
    };
    if (pattern === '') {
        for (let index = 0; index < receiver.length && result.length < limit; index++) push(receiver[index]);
        return result;
    }
    let start = 0;
    while (start <= receiver.length && result.length < limit) {
        const index = __nfStringApply(__nfStringMethods.indexOf, receiver, [pattern, start]);
        if (index < 0) { push(__nfStringApply(__nfStringMethods.slice, receiver, [start])); break; }
        push(__nfStringApply(__nfStringMethods.slice, receiver, [start, index]));
        start = index + pattern.length;
    }
    return result;
}
function __nfStringReplace(receiver, args) {
    if (__nfRegExpIs(args[0])) return __nfRegExpReplace(receiver, args[0], args[1]);
    // Adobe's string-pattern path converts replacement BEFORE pattern; `$&`
    // and similar text is literal here, unlike JavaScript String.replace.
    const callback = typeof args[1] === 'function' ? args[1] : null;
    let replacement = callback ? null : __nfStringBound(__nfStringNative(args[1]));
    const pattern = __nfStringPlain(args[0], 'replace');
    const index = __nfStringApply(__nfStringMethods.indexOf, receiver, [pattern]);
    if (index < 0) return receiver;
    if (callback) replacement = __nfStringBound(__nfStringNative(__nfStringApply(callback, undefined, [pattern, index, receiver])));
    if (receiver.length - pattern.length + replacement.length > __nfStringLimit) __nfStringUnsupported('replace result exceeds the supported string limit.');
    return __nfStringApply(__nfStringMethods.slice, receiver, [0, index]) + replacement
        + __nfStringApply(__nfStringMethods.slice, receiver, [index + pattern.length]);
}
function __nfStringMethod(receiver, name, as3) {
    // The shared router chooses this family before any property lookup. Resolve
    // one property now; returned callable runs only after argument evaluation.
    const method = receiver[name];
    if (typeof receiver !== 'string' || method !== __nfStringMethods[name]) {
        return (...args) => __nfStringApply(method, receiver, args);
    }
    return (...args) => {
        __nfStringBound(receiver);
        if (as3 && Object.hasOwn(__nfStringArity, name) && args.length > __nfStringArity[name]) {
            __nfStringUnsupported(name + ' excess-argument behavior is not verified; pass at most ' + __nfStringArity[name] + ' arguments.');
        }
        switch (name) {
            case 'toString': case 'valueOf': return receiver;
            case 'toLowerCase': case 'toUpperCase':
                // Modern JavaScript Unicode mappings are a declared partial
                // profile; historical Flash case tables are not reproduced.
                return __nfStringBound(__nfStringApply(method, receiver, []));
            case 'charAt': case 'charCodeAt': {
                const position = as3 ? __nfStringInteger(args[0]) : __nfStringNumber(args[0]) | 0;
                return __nfStringApply(method, receiver, [position]);
            }
            case 'concat': {
                let result = receiver;
                for (const value of args) {
                    const part = __nfStringBound(__nfStringNative(value));
                    if (result.length + part.length > __nfStringLimit) __nfStringUnsupported('concat result exceeds the supported string limit.');
                    result += part;
                }
                return result;
            }
            case 'indexOf': case 'lastIndexOf': {
                if (!as3 && args.length === 0) return undefined;
                const nullable = as3 && args.length > 0 && args[0] == null;
                const pattern = nullable ? null : __nfStringNative(args.length ? args[0] : undefined);
                let position;
                if (name === 'lastIndexOf') {
                    if (args.length < 2 || args[1] === undefined) position = receiver.length;
                    else if (as3) {
                        const number = __nfStringNumber(args[1]);
                        position = Number.isNaN(number) ? receiver.length : Math.trunc(number);
                    } else position = __nfStringNumber(args[1]) | 0;
                } else position = as3 ? __nfStringInteger(args[1]) : __nfStringNumber(args[1]) | 0;
                if (nullable || (name === 'lastIndexOf' && position < 0)) return -1;
                // AVM1 does not clamp a too-large indexOf start to its length.
                if (!as3 && name === 'indexOf' && position > receiver.length) return -1;
                return __nfStringApply(method, receiver, [pattern, position]);
            }
            case 'slice': case 'substring': case 'substr': {
                if (!as3 && args.length === 0) return undefined;
                const start = as3 ? __nfStringInteger(args[0]) : __nfStringNumber(args[0]) | 0;
                const omittedEnd = args.length < 2 || (!as3 && args[1] === undefined);
                const end = omittedEnd ? receiver.length : as3 ? __nfStringInteger(args[1]) : __nfStringNumber(args[1]) | 0;
                if (name !== 'substr') return __nfStringApply(method, receiver, [start, end]);
                const from = __nfStringWrapping(start, receiver.length);
                const combined = as3 ? from + end : (from + end) | 0;
                const to = Math.max(from, __nfStringWrapping(combined, receiver.length));
                return __nfStringApply(__nfStringMethods.slice, receiver, [from, to]);
            }
            case 'split': return __nfStringSplit(receiver, args, as3);
            case 'match': case 'search':
                if (!as3) __nfStringUnsupported(name + ' is not an AS2 native String method.');
                return name === 'match' ? __nfRegExpMatch(receiver, args[0]) : __nfRegExpSearch(receiver, args[0]);
            case 'replace':
                if (!as3) __nfStringUnsupported('replace is not an AS2 native String method.');
                return __nfStringReplace(receiver, args);
            default: __nfStringUnsupported(name + ' is outside the supported String profile (locale and modern JavaScript-only methods remain pending).');
        }
    };
}
