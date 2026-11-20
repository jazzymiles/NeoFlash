// Array family adapter, embedded in compiled handlers and the project program.
// Original implementation; semantic references are listed in collections-fixtures.mjs.
// Arrays retain native identity/storage. No JavaScript prototype is modified.
// This profile covers ordinary indexed arrays, documented sort flags, AS3
// callbacks, AS3 trait method closures, AS3 Array.prototype borrowing, and
// prototype-only toString/toLocaleString extraction up to 1,000,000 elements.
// Historical comparison-call order, mutating Array.prototype at runtime, custom
// namespace lookup, and exotic constructor/length coercions remain unsupported.
const __nfNativeArray = Array;
const __nfArrayIsArray = Array.isArray;
const __nfArrayApply = Reflect.apply;
const __nfArrayNumber = Number;
const __nfArrayString = String;
const __nfArrayTypeError = TypeError;
const __nfArrayRangeError = RangeError;
const __nfArrayObjectToString = Object.prototype.toString;
const __nfArrayValueOf = Array.prototype.valueOf;
const __nfArrayTraitNames = [
    'push', 'pop', 'shift', 'unshift', 'join', 'slice', 'concat', 'reverse', 'splice',
    'sort', 'sortOn', 'indexOf', 'lastIndexOf', 'map', 'filter', 'forEach', 'every', 'some',
    'insertAt', 'removeAt',
];
const __nfArrayPrototypeNames = ['toString', 'toLocaleString'];
const __nfArrayMethods = Object.freeze(Object.fromEntries([
    'push', 'pop', 'shift', 'unshift', 'join', 'slice', 'concat', 'reverse', 'splice',
    'sort', 'indexOf', 'lastIndexOf', 'map', 'filter', 'forEach', 'every', 'some',
    'toString', 'toLocaleString',
].map(name => [name, Array.prototype[name]])));
const __nfArrayFlags = Object.freeze({ CASEINSENSITIVE: 1, DESCENDING: 2, UNIQUESORT: 4, RETURNINDEXEDARRAY: 8, NUMERIC: 16 });
const __nfArrayLimit = 1000000;
const __nfArrayLocalState = { classes: new Map(), methods: new WeakMap() };
// Tracks active outer Array formatting frames to emit "" on cyclic references
// without breaking non-cyclic DAG sharing (try/finally delete).
const __nfArraySeen = new Set();
function __nfArrayState(as3) {
    return as3 && typeof __nfLegacy !== 'undefined' && __nfLegacy?.objects3?.arrays || __nfArrayLocalState;
}

function __nfArrayBound(length) {
    if (!Number.isInteger(length) || length < 0 || length > __nfArrayLimit) {
        throw new __nfArrayRangeError('NeoFlash Array operations currently support integer lengths from 0 through 1000000. Larger arrays and legacy length coercions are not implemented.');
    }
}
function __nfArrayEnsureReceiver(receiver, name) {
    if (receiver === null || receiver === undefined) {
        throw new __nfArrayTypeError(`Array.prototype.${name} requires a non-null object receiver.`);
    }
    return typeof receiver === 'object' || typeof receiver === 'function' ? receiver : Object(receiver);
}
function __nfArrayTruncInt(value) {
    const n = __nfArrayNumber(value);
    if (Number.isNaN(n) || n === 0) return 0;
    return Number.isFinite(n) ? Math.trunc(n) : n;
}
function __nfArrayReadLength(target) {
    if (__nfArrayIsArray(target)) {
        __nfArrayBound(target.length);
        return target.length;
    }
    const raw = target.length;
    if (raw === undefined || raw === null) return 0;
    const n = __nfArrayNumber(raw);
    if (Number.isNaN(n) || n === 0) return 0;
    if (!Number.isFinite(n)) throw new __nfArrayRangeError('Borrowed Array length must be a finite non-negative integer within 1000000.');
    const truncated = Math.trunc(n);
    __nfArrayBound(truncated);
    return truncated;
}
function __nfArrayRelativeIndex(raw, length, fallback) {
    if (raw === undefined) return fallback;
    const int = __nfArrayTruncInt(raw);
    if (int === Infinity) return length;
    if (int === -Infinity) return 0;
    return int < 0 ? Math.max(length + int, 0) : Math.min(int, length);
}
function __nfArrayWriteProp(target, key, value) {
    if (!Reflect.set(target, key, value)) {
        throw new __nfArrayTypeError(`Cannot assign '${__nfArrayString(key)}' on borrowed Array target.`);
    }
}
function __nfArrayDeleteProp(target, key) {
    if (!Reflect.deleteProperty(target, key)) {
        throw new __nfArrayTypeError(`Cannot delete '${__nfArrayString(key)}' on borrowed Array target.`);
    }
}
function __nfArrayFormatElement(item, locale, as3 = true, isAs1 = false) {
    if (as3) {
        if (item === null || item === undefined) return '';
    } else {
        if (item === undefined) return isAs1 ? '' : 'undefined';
        if (item === null) return 'null';
    }
    const program = typeof __nfProgram !== 'undefined' ? __nfProgram : null;
    if (__nfArrayIsArray(item)) {
        const key = locale ? 'toLocaleString' : 'toString';
        const resolved = item[key];
        if (resolved !== __nfArrayMethods[key] && typeof resolved === 'function') {
            return __nfArrayString(__nfArrayApply(resolved, item, []));
        }
        return locale ? __nfArrayLocaleString(item) : __nfArrayJoin(item, ',', as3, isAs1);
    }
    if (typeof item === 'object' || typeof item === 'function') {
        const key = locale ? 'toLocaleString' : 'toString';
        const resolved = item[key];
        const defaultMethod = program?.defaultObjectMethod?.(item, key, resolved);
        if (defaultMethod) return __nfArrayString(__nfArrayApply(defaultMethod, item, []));
        if (locale && typeof resolved === 'function') return __nfArrayString(__nfArrayApply(resolved, item, []));
        return __nfArrayString(item);
    }
    if (locale) return __nfArrayString(Object(item).toLocaleString());
    return __nfArrayString(item);
}
function __nfArrayJoin(target, separator, as3 = true, isAs1 = false) {
    const length = __nfArrayReadLength(target);
    if (__nfArraySeen.has(target)) return '';
    __nfArraySeen.add(target);
    try {
        const parts = [];
        for (let index = 0; index < length; index++) {
            if (as3) {
                parts.push(index in target ? __nfArrayFormatElement(target[index], false, true, false) : '');
            } else {
                const elem = index in target ? target[index] : undefined;
                parts.push(__nfArrayFormatElement(elem, false, false, isAs1));
            }
        }
        return __nfArrayApply(__nfArrayMethods.join, parts, [separator]);
    } finally {
        __nfArraySeen.delete(target);
    }
}
function __nfArrayDefaultString(value, as3 = true, isAs1 = false) {
    if (__nfArrayIsArray(value) && value.valueOf === __nfArrayValueOf && value.toString === __nfArrayMethods.toString) {
        return __nfArrayJoin(value, ',', as3, isAs1);
    }
    return value;
}
function __nfArrayLocaleString(target) {
    const length = __nfArrayReadLength(target);
    if (__nfArraySeen.has(target)) return '';
    __nfArraySeen.add(target);
    try {
        const parts = [];
        for (let index = 0; index < length; index++) {
            parts.push(index in target ? __nfArrayFormatElement(target[index], true) : '');
        }
        return __nfArrayApply(__nfArrayMethods.join, parts, [',']);
    } finally {
        __nfArraySeen.delete(target);
    }
}
function __nfArrayCreate(as3, args) {
    // A single numeric argument is a length; a string or multiple arguments are values.
    if (args.length === 1 && typeof args[0] === 'number') {
        __nfArrayBound(args[0]);
        return new __nfNativeArray(args[0]);
    }
    __nfArrayBound(args.length);
    return __nfArrayApply(__nfArrayMethods.slice, args, []);
}
function __nfArrayClass(as3) {
    as3 = Boolean(as3);
    const classes = __nfArrayState(as3).classes;
    if (!classes.has(as3)) {
        const facade = function(...args) { return __nfArrayCreate(as3, args); };
        for (const [name, value] of Object.entries(__nfArrayFlags)) Object.defineProperty(facade, name, { value, enumerable: true });
        Object.defineProperty(facade, '__nfIs', { value: __nfArrayIsArray });
        Object.defineProperty(facade, Symbol.hasInstance, { value: __nfArrayIsArray });
        if (as3) {
            // Expose AS3 Array.prototype generic functions without touching global Array.prototype.
            for (const name of [...__nfArrayTraitNames, ...__nfArrayPrototypeNames]) {
                Object.defineProperty(facade.prototype, name, {
                    value: function() {
                        'use strict';
                        if (new.target) throw new __nfArrayTypeError(`Array.prototype.${name} is not a constructor.`);
                        return __nfArrayExecute(this, name, __nfArrayApply(__nfArrayMethods.slice, arguments, []), true);
                    },
                    enumerable: false,
                });
            }
        }
        Object.freeze(facade.prototype);
        classes.set(as3, Object.freeze(facade));
    }
    return classes.get(as3);
}
function __nfArrayMethodValue(value) {
    if (Object.values(__nfArrayMethods).includes(value)) {
        throw new __nfArrayTypeError('Unbound Array native method values are unsupported. Call the method directly on the array, or read a supported AS3 trait/prototype method in AS3.');
    }
    return value;
}
function __nfArrayReference(receiver, name, value, as3, capturedPresence) {
    if (!as3 || !__nfArrayIsArray(receiver)) return value;
    if (name === 'constructor' && value === __nfNativeArray) return __nfArrayClass(true);
    if ((name === 'toString' || name === 'toLocaleString') && value === __nfArrayMethods[name]) {
        return __nfArrayClass(true).prototype[name];
    }
    const native = name !== 'toString' && name !== 'toLocaleString' && Object.hasOwn(__nfArrayMethods, name) && value === __nfArrayMethods[name];
    const present = capturedPresence === undefined ? name in receiver : capturedPresence;
    const synthetic = (name === 'sortOn' || name === 'insertAt' || name === 'removeAt') && value === undefined && !present;
    if (!native && !synthetic) return value;
    const cache = __nfArrayState(true).methods;
    let methods = cache.get(receiver);
    if (!methods) { methods = new Map(); cache.set(receiver, methods); }
    if (!methods.has(name)) {
        // Capture the resolved target now. Neither an accessor nor a replacement
        // installed after this read may redirect an existing method closure.
        // MethodClosure.cpp retains the receiver even through call/apply.
        methods.set(name, __nfArrayMethod(receiver, name, true, { value, present }));
    }
    return methods.get(name);
}
function __nfArrayOptions(value) {
    if (value === undefined || value === null) return 0;
    if (!Number.isInteger(value) || value < 0 || value > 31) {
        throw new __nfArrayTypeError('Array sort options must combine CASEINSENSITIVE, DESCENDING, UNIQUESORT, RETURNINDEXEDARRAY and NUMERIC.');
    }
    return value;
}
function __nfArrayCompare(a, b, options, as3) {
    let result;
    if ((options & 16) && (as3 || (typeof a === 'number' && typeof b === 'number'))) {
        if (typeof a !== 'number' || typeof b !== 'number') {
            throw new __nfArrayTypeError('AS3 NUMERIC sorting currently requires numeric values. Convert score fields with Number() before sorting.');
        }
        if (Number.isNaN(a) || Number.isNaN(b)) {
            result = Number.isNaN(a) && Number.isNaN(b) ? 0 : Number.isNaN(a) ? 1 : -1;
        } else {
            result = a < b ? -1 : a > b ? 1 : 0;
        }
    } else {
        // Deliberately limit AS2 comparison coercion; object hooks have different AVM1 profiles.
        if (!as3 && ((a !== null && typeof a === 'object') || typeof a === 'function' || (b !== null && typeof b === 'object') || typeof b === 'function')) {
            throw new __nfArrayTypeError('Array string sorting currently supports primitive values. Supply a comparator or sortOn field for objects.');
        }
        let left = __nfArrayString(a), right = __nfArrayString(b);
        if (options & 1) { left = left.toLowerCase(); right = right.toLowerCase(); }
        result = left < right ? -1 : left > right ? 1 : 0;
    }
    return options & 2 ? -result : result;
}
function __nfArraySort(receiver, args, as3, onFields) {
    const length = __nfArrayReadLength(receiver);
    let flags = 0, compare, uniqueCompare;
    if (onFields) {
        if (args.length < 1 || args.length > 2) throw new __nfArrayTypeError('sortOn requires a field name or field-name array and optional sort flags.');
        const fields = typeof args[0] === 'string' ? [args[0]] : __nfArrayIsArray(args[0]) ? args[0].slice() : null;
        if (!fields || fields.some(field => typeof field !== 'string')) throw new __nfArrayTypeError('sortOn field names must be strings.');
        if (!fields.length) return receiver;
        let options;
        if (__nfArrayIsArray(args[1])) {
            // Equal-length option arrays are the common, documented contract.
            if (args[1].length !== fields.length) throw new __nfArrayTypeError('sortOn options and field-name arrays must have equal lengths in this profile.');
            options = args[1].map(__nfArrayOptions);
        } else options = fields.map(() => __nfArrayOptions(args[1]));
        flags = options[0] & 12;
        compare = (left, right) => {
            if (!left || !right || (typeof left !== 'object' && typeof left !== 'function') || (typeof right !== 'object' && typeof right !== 'function')) {
                throw new __nfArrayTypeError('sortOn currently requires object records.');
            }
            for (let index = 0; index < fields.length; index++) {
                const field = fields[index];
                if (!as3) {
                    // AVM1 sortOn reads stored fields, not accessor callbacks.
                    for (const record of [left, right]) {
                        const descriptor = Object.getOwnPropertyDescriptor(record, field);
                        if (descriptor && !Object.hasOwn(descriptor, 'value')) throw new __nfArrayTypeError('AS2 sortOn accessors are not supported; sort plain data fields.');
                    }
                }
                const a = as3 ? left[field] : Object.getOwnPropertyDescriptor(left, field)?.value;
                const b = as3 ? right[field] : Object.getOwnPropertyDescriptor(right, field)?.value;
                const result = __nfArrayCompare(a, b, options[index], as3);
                if (result) return result;
            }
            return 0;
        };
        uniqueCompare = compare;
    } else {
        if (args.length > 2) throw new __nfArrayTypeError('sort accepts a comparator and/or sort flags.');
        const comparator = typeof args[0] === 'function' ? args[0] : null;
        if (comparator) flags = __nfArrayOptions(args[1]);
        else {
            if (args.length > 1) throw new __nfArrayTypeError('Pass a comparator before the second sort argument.');
            flags = __nfArrayOptions(args[0]);
        }
        const ordinary = (a, b) => __nfArrayCompare(a, b, flags, as3);
        compare = comparator ? (a, b) => {
            const value = __nfArrayApply(comparator, undefined, [a, b]);
            if (typeof value !== 'number') throw new __nfArrayTypeError('Array comparator must return a number.');
            const direction = value < 0 ? -1 : value > 0 ? 1 : 0;
            return flags & 2 ? -direction : direction;
        } : ordinary;
        // AVM1 unique sorting compares values using the flags even with a custom comparator.
        uniqueCompare = comparator && !as3 ? ordinary : compare;
    }
    const values = [];
    for (let index = 0; index < length; index++) {
        const present = index in receiver;
        values.push({ index, value: present ? receiver[index] : undefined, present });
    }
    __nfArrayApply(__nfArrayMethods.sort, values, [(a, b) => {
        if (as3) {
            if (!a.present && !b.present) return 0;
            if (!a.present) return 1;
            if (!b.present) return -1;
        }
        return compare(a.value, b.value);
    }]);
    if (flags & 4) for (let index = 1; index < values.length; index++) {
        if (as3 && (!values[index - 1].present || !values[index].present)) continue;
        if (uniqueCompare(values[index - 1].value, values[index].value) === 0) return 0;
    }
    if (flags & 8) return values.map(item => item.index);
    // Comparison and validation complete before changing the receiver.
    for (let index = 0; index < values.length; index++) {
        if (as3 && !values[index].present) __nfArrayDeleteProp(receiver, index);
        else __nfArrayWriteProp(receiver, index, values[index].value);
    }
    return receiver;
}
function __nfArrayIterate(receiver, name, args) {
    const callback = args[0], thisObject = args.length > 1 ? args[1] : null;
    if (typeof callback !== 'function' || args.length > 2) throw new __nfArrayTypeError(`${name} requires a callback and optional thisObject.`);
    const length = __nfArrayReadLength(receiver), result = [];
    // The initial length is fixed; current values are fetched for each index.
    // AVM2 visits holes as undefined, unlike JavaScript Array.prototype.map.
    for (let index = 0; index < length; index++) {
        const value = index in receiver ? receiver[index] : undefined;
        const answer = __nfArrayApply(callback, thisObject, [value, index, receiver]);
        if (name === 'map') result.push(answer);
        else if (name === 'filter' && answer) result.push(value);
        else if (name === 'every' && !answer) return false;
        else if (name === 'some' && answer) return true;
    }
    if (name === 'every') return true;
    if (name === 'some') return false;
    if (name === 'forEach') return undefined;
    return result;
}
function __nfArrayConcatBound(target, length, args) {
    let total = __nfArrayIsArray(target) ? length : 1;
    for (let index = 0; index < args.length; index++) {
        const item = args[index];
        if (__nfArrayIsArray(item)) {
            __nfArrayBound(item.length);
            total += item.length;
        } else {
            total += 1;
        }
        __nfArrayBound(total);
    }
}
function __nfArrayShiftElements(target, start, end, delta) {
    if (delta > 0) {
        for (let index = end - 1; index >= start; index--) {
            if (index in target) __nfArrayWriteProp(target, index + delta, target[index]);
            else __nfArrayDeleteProp(target, index + delta);
        }
    } else if (delta < 0) {
        for (let index = start; index < end; index++) {
            if (index in target) __nfArrayWriteProp(target, index + delta, target[index]);
            else __nfArrayDeleteProp(target, index + delta);
        }
    }
}
function __nfArrayBorrowSlice(target, length, args) {
    const start = __nfArrayRelativeIndex(args[0], length, 0);
    const end = args.length > 1 && args[1] !== undefined ? __nfArrayRelativeIndex(args[1], length, length) : length;
    const count = Math.max(0, end - start), out = new __nfNativeArray(count);
    for (let index = 0; index < count; index++) {
        const source = start + index;
        if (source in target) out[index] = target[source];
    }
    return out;
}
function __nfArrayBorrowSplice(target, length, args) {
    __nfArrayBound(length + Math.max(0, args.length - 2));
    if (!args.length) return [];
    const start = __nfArrayRelativeIndex(args[0], length, 0);
    const actualDelete = args.length === 1 ? length - start : Math.min(Math.max(__nfArrayTruncInt(args[1]), 0), length - start);
    const removed = new __nfNativeArray(actualDelete);
    for (let index = 0; index < actualDelete; index++) {
        const source = start + index;
        if (source in target) removed[index] = target[source];
    }
    const items = __nfArrayApply(__nfArrayMethods.slice, args, [2]);
    const itemCount = items.length, delta = itemCount - actualDelete;
    if (delta > 0 && typeof __nfVectorIs === 'function' && __nfVectorIs(target)) {
        // Vector append trap rejects index > length; expand sequentially before shifting right.
        __nfArrayWriteProp(target, 'length', length + delta);
    }
    if (delta < 0) {
        __nfArrayShiftElements(target, start + actualDelete, length, delta);
        for (let index = length + delta; index < length; index++) __nfArrayDeleteProp(target, index);
    } else if (delta > 0) {
        __nfArrayShiftElements(target, start + actualDelete, length, delta);
    }
    for (let index = 0; index < itemCount; index++) __nfArrayWriteProp(target, start + index, items[index]);
    __nfArrayWriteProp(target, 'length', length + delta);
    return removed;
}
function __nfArrayMutateAt(target, length, name, args) {
    const required = name === 'insertAt' ? 2 : 1;
    if (args.length !== required) throw new __nfArrayTypeError(`Array.${name} requires ${required} arguments.`);
    const service = typeof __nfLegacy !== 'undefined' ? __nfLegacy?.objects3 : null;
    if (typeof service?.arrayPlan !== 'function') throw new __nfArrayTypeError('Rust Array mutation services are unavailable; rebuild and re-export this project.');
    let plan;
    try { plan = service.arrayPlan(name, length, __nfArrayNumber(args[0]) | 0); }
    catch (error) { throw new __nfArrayRangeError(String(error?.message ?? error)); }
    if (__nfArrayIsArray(target)) {
        if (!Object.isExtensible(target) || !Object.getOwnPropertyDescriptor(target, 'length')?.writable) throw new __nfArrayTypeError('Array insertAt/removeAt requires ordinary mutable storage.');
        for (let index = 0; index < length; index++) {
            const descriptor = Object.getOwnPropertyDescriptor(target, String(index));
            if (descriptor && (!Object.hasOwn(descriptor, 'value') || !descriptor.writable || !descriptor.configurable)) throw new __nfArrayTypeError('Array insertAt/removeAt does not support exotic element descriptors.');
        }
    }
    const removed = __nfArrayExecute(target, 'splice', plan.insert ? [plan.index, 0, args[1]] : [plan.index, plan.remove], true);
    return plan.insert ? undefined : removed[0];
}
function __nfArrayIndexSearch(target, length, backwards, args) {
    if (!length) return -1;
    let index;
    if (args.length > 1) {
        const n = __nfArrayNumber(args[1]);
        index = Number.isNaN(n) || n === 0 ? 0 : n === Infinity ? length : n === -Infinity ? -length - 1 : Math.trunc(n);
    } else {
        index = backwards ? length - 1 : 0;
    }
    if (index < 0) index = length + index;
    index = backwards ? Math.min(index, length - 1) : Math.max(index, 0);
    for (; index >= 0 && index < length; index += backwards ? -1 : 1) {
        if ((index in target) && target[index] === args[0]) return index;
    }
    return -1;
}
function __nfArrayExecute(receiver, name, args, as3, legacyCase) {
    const target = __nfArrayEnsureReceiver(receiver, name);
    const array = __nfArrayIsArray(target);
    const length = __nfArrayReadLength(target);
    const isAs1 = !as3 && (Boolean(legacyCase) || (legacyCase === undefined && typeof __nfLegacyCase !== 'undefined' && Boolean(__nfLegacyCase)));
    if (!as3 && name === 'join') {
        const sep = !args.length || args[0] === undefined ? ',' : (args[0] === null ? 'null' : __nfArrayString(args[0]));
        return __nfArrayJoin(target, sep, false, isAs1);
    }
    if (!as3 && name === 'toString') {
        if (array) return __nfArrayJoin(target, ',', false, isAs1);
        return __nfArrayApply(__nfArrayMethods.toString, target, args);
    }
    if (!as3 && name === 'toLocaleString') return __nfArrayApply(__nfArrayMethods[name], target, args);
    if (name === 'insertAt' || name === 'removeAt') {
        if (!as3) throw new __nfArrayTypeError(`${name} is an AS3 Array method.`);
        return __nfArrayMutateAt(target, length, name, args);
    }
    if (name === 'sort' || name === 'sortOn') return __nfArraySort(target, args, as3, name === 'sortOn');
    if (name === 'map' || name === 'filter' || name === 'forEach' || name === 'every' || name === 'some') {
        if (!as3) throw new __nfArrayTypeError(`${name} is an AS3 Array method. Use an AS2 loop or a custom method.`);
        return __nfArrayIterate(target, name, args);
    }
    if (as3 && (name === 'indexOf' || name === 'lastIndexOf')) return __nfArrayIndexSearch(target, length, name === 'lastIndexOf', args);
    if (as3 && name === 'join') return __nfArrayJoin(target, args.length && args[0] !== undefined ? __nfArrayString(args[0]) : ',');
    if (as3 && name === 'toString') {
        if (array) return __nfArrayJoin(target, ',');
        if (typeof target.join === 'function') return __nfArrayApply(target.join, target, []);
        const program = typeof __nfProgram !== 'undefined' ? __nfProgram : null;
        const defaultMethod = program?.defaultObjectMethod?.(target, 'toString', target.toString);
        return defaultMethod ? __nfArrayApply(defaultMethod, target, []) : __nfArrayApply(__nfArrayObjectToString, target, []);
    }
    if (as3 && name === 'toLocaleString') return __nfArrayLocaleString(target);
    if (name === 'concat') {
        __nfArrayConcatBound(target, length, args);
        return __nfArrayApply(__nfArrayMethods.concat, target, args);
    }
    if (array) {
        if (name === 'push' || name === 'unshift') __nfArrayBound(length + args.length);
        if (name === 'splice') __nfArrayBound(length + Math.max(0, args.length - 2));
        return __nfArrayApply(__nfArrayMethods[name], target, args);
    }
    if (name === 'slice') return __nfArrayBorrowSlice(target, length, args);
    if (name === 'splice') return __nfArrayBorrowSplice(target, length, args);
    if (name === 'push') {
        __nfArrayBound(length + args.length);
        let next = length;
        for (let index = 0; index < args.length; index++) __nfArrayWriteProp(target, next++, args[index]);
        __nfArrayWriteProp(target, 'length', next);
        return next;
    }
    if (name === 'pop') {
        if (!length) { __nfArrayWriteProp(target, 'length', 0); return undefined; }
        const last = length - 1, value = last in target ? target[last] : undefined;
        if (last in target) __nfArrayDeleteProp(target, last);
        __nfArrayWriteProp(target, 'length', last);
        return value;
    }
    if (name === 'shift') {
        if (!length) { __nfArrayWriteProp(target, 'length', 0); return undefined; }
        const first = 0 in target ? target[0] : undefined;
        __nfArrayShiftElements(target, 1, length, -1);
        __nfArrayDeleteProp(target, length - 1);
        __nfArrayWriteProp(target, 'length', length - 1);
        return first;
    }
    if (name === 'unshift') {
        const count = args.length;
        __nfArrayBound(length + count);
        if (count > 0 && typeof __nfVectorIs === 'function' && __nfVectorIs(target)) __nfArrayWriteProp(target, 'length', length + count);
        __nfArrayShiftElements(target, 0, length, count);
        for (let index = 0; index < count; index++) __nfArrayWriteProp(target, index, args[index]);
        __nfArrayWriteProp(target, 'length', length + count);
        return length + count;
    }
    if (name === 'reverse') {
        const middle = Math.floor(length / 2);
        for (let lower = 0; lower < middle; lower++) {
            const upper = length - 1 - lower, hasLower = lower in target, hasUpper = upper in target;
            const lowerValue = hasLower ? target[lower] : undefined, upperValue = hasUpper ? target[upper] : undefined;
            if (hasLower && hasUpper) { __nfArrayWriteProp(target, lower, upperValue); __nfArrayWriteProp(target, upper, lowerValue); }
            else if (!hasLower && hasUpper) { __nfArrayWriteProp(target, lower, upperValue); __nfArrayDeleteProp(target, upper); }
            else if (hasLower && !hasUpper) { __nfArrayDeleteProp(target, lower); __nfArrayWriteProp(target, upper, lowerValue); }
        }
        return target;
    }
    return __nfArrayApply(__nfArrayMethods[name], target, args);
}
function __nfArrayMethod(receiver, name, as3, resolved, legacyCase) {
    // Resolve once before caller evaluates arguments. An override, including a
    // getter or explicitly undefined property, must retain ordinary call behavior.
    const method = resolved ? resolved.value : receiver[name];
    const array = __nfArrayIsArray(receiver);
    const absent = () => resolved ? !resolved.present : !(name in receiver);
    const synthesizeSortOn = array && name === 'sortOn' && absent();
    const synthesizeMutation = as3 && array && (name === 'insertAt' || name === 'removeAt') && absent();
    const native = array && Object.hasOwn(__nfArrayMethods, name) && method === __nfArrayMethods[name];
    return (...args) => {
        if (!native && !synthesizeSortOn && !synthesizeMutation) {
            if (typeof method !== 'function') throw new __nfArrayTypeError(`Array/custom member '${name}' is not callable.`);
            return __nfArrayApply(method, receiver, args);
        }
        return __nfArrayExecute(receiver, name, args, as3, legacyCase);
    };
}
