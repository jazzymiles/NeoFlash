// Bounded primitive String family, embedded after intrinsics and RegExp helpers.
// Both dialects delegate primitive String algorithms to the shared Rust core.
// RegExp dispatch and existing receiver/value coercion remain here. No browser prototype is modified.
const __nfStringNative = String;
const __nfStringNumber = Number;
const __nfStringApply = Reflect.apply;
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
    toLowerCase: 0, toUpperCase: 0, toLocaleLowerCase: 0, toLocaleUpperCase: 0, localeCompare: 1, toString: 0, valueOf: 0 });

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
    return (as3 ? __nfAs3Strings() : __nfAs2Strings()).invoke('fromCharCode', '', args);
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
function __nfStringMethod(receiver, name, as3) {
    // The shared router chooses this family before any property lookup. Resolve
    // one property now; returned callable runs only after argument evaluation.
    const method = receiver[name];
    if (typeof receiver !== 'string' || method !== __nfStringMethods[name]) {
        return (...args) => __nfStringApply(method, receiver, args);
    }
    return (...args) => {
        __nfStringBound(receiver);
        if (!as3) return __nfAs2Strings().invoke(name, receiver, args);
        if (Object.hasOwn(__nfStringArity, name) && args.length > __nfStringArity[name]) {
            __nfStringUnsupported(name + ' excess-argument behavior is not verified; pass at most ' + __nfStringArity[name] + ' arguments.');
        }
        if (name === 'match' || name === 'search') {
            return name === 'match' ? __nfRegExpMatch(receiver, args[0]) : __nfRegExpSearch(receiver, args[0]);
        }
        if (name === 'replace' && __nfRegExpIs(args[0])) return __nfRegExpReplace(receiver, args[0], args[1]);
        if (name === 'split' && __nfRegExpIs(args[0])) {
            // RegExp execution retains its existing separate backend profile.
            const limit = args.length < 2 || args[1] == null ? 0xffffffff : __nfStringNumber(args[1]) >>> 0;
            if (limit === 0) return [];
            if (receiver.length === 0) return [''];
            return __nfRegExpSplit(receiver, args[0], limit);
        }
        return __nfAs3Strings().invoke(name, receiver, args);
    };
}
