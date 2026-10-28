// Array family adapter, embedded in compiled handlers and the project program.
// Original implementation; semantic references are listed in collections-fixtures.mjs.
// Arrays retain native identity/storage. No JavaScript prototype is modified.
// This profile covers ordinary indexed arrays, documented sort flags and AS3
// callbacks. Historical comparison-call order, prototype extensions, borrowed
// native method values and exotic constructor/index coercions remain unsupported.
const __nfNativeArray = Array;
const __nfArrayIsArray = Array.isArray;
const __nfArrayApply = Reflect.apply;
const __nfArrayNumber = Number;
const __nfArrayString = String;
const __nfArrayTypeError = TypeError;
const __nfArrayRangeError = RangeError;
const __nfArrayMethods = Object.freeze(Object.fromEntries([
    'push', 'pop', 'shift', 'unshift', 'join', 'slice', 'concat', 'reverse', 'splice',
    'sort', 'indexOf', 'lastIndexOf', 'map', 'filter', 'forEach', 'every', 'some', 'toString',
].map(name => [name, Array.prototype[name]])));
const __nfArrayFlags = Object.freeze({ CASEINSENSITIVE: 1, DESCENDING: 2, UNIQUESORT: 4, RETURNINDEXEDARRAY: 8, NUMERIC: 16 });
const __nfArrayLimit = 1000000;
const __nfArrayClasses = new Map();

function __nfArrayBound(length) {
    if (!Number.isInteger(length) || length < 0 || length > __nfArrayLimit) {
        throw new __nfArrayRangeError('NeoFlash Array operations currently support integer lengths from 0 through 1000000. Larger arrays and legacy length coercions are not implemented.');
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
    if (!__nfArrayClasses.has(as3)) {
        const facade = function(...args) { return __nfArrayCreate(as3, args); };
        for (const [name, value] of Object.entries(__nfArrayFlags)) Object.defineProperty(facade, name, { value, enumerable: true });
        Object.defineProperty(facade, '__nfIs', { value: __nfArrayIsArray });
        Object.defineProperty(facade, Symbol.hasInstance, { value: __nfArrayIsArray });
        // Prototype augmentation is outside this adapter. Do not expose a mutable
        // alias to Array.prototype or pretend facade methods were installed there.
        Object.freeze(facade.prototype);
        __nfArrayClasses.set(as3, Object.freeze(facade));
    }
    return __nfArrayClasses.get(as3);
}
function __nfArrayMethodValue(value) {
    if (Object.values(__nfArrayMethods).includes(value)) {
        throw new __nfArrayTypeError('Array native method values are not supported yet. Call the method directly on the array so its receiver and ActionScript profile are preserved.');
    }
    return value;
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
        result = a < b ? -1 : a > b ? 1 : 0;
    } else {
        // Deliberately limit comparison coercion; object hooks have different AVM profiles.
        if ((a !== null && typeof a === 'object') || typeof a === 'function' || (b !== null && typeof b === 'object') || typeof b === 'function') {
            throw new __nfArrayTypeError('Array string sorting currently supports primitive values. Supply a comparator or sortOn field for objects.');
        }
        let left = __nfArrayString(a), right = __nfArrayString(b);
        if (options & 1) { left = left.toLowerCase(); right = right.toLowerCase(); }
        result = left < right ? -1 : left > right ? 1 : 0;
    }
    return options & 2 ? -result : result;
}
function __nfArraySort(receiver, args, as3, onFields) {
    __nfArrayBound(receiver.length);
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
    for (let index = 0; index < receiver.length; index++) values.push({ index, value: receiver[index], present: index in receiver });
    __nfArrayApply(__nfArrayMethods.sort, values, [(a, b) => compare(a.value, b.value)]);
    if (flags & 4) for (let index = 1; index < values.length; index++) {
        if (uniqueCompare(values[index - 1].value, values[index].value) === 0) return 0;
    }
    if (flags & 8) return values.map(item => item.index);
    // Comparison and validation complete before changing the receiver.
    for (let index = 0; index < values.length; index++) {
        if (as3 && !values[index].present) delete receiver[index];
        else receiver[index] = values[index].value;
    }
    return receiver;
}
function __nfArrayIterate(receiver, name, args) {
    const callback = args[0], thisObject = args.length > 1 ? args[1] : null;
    if (typeof callback !== 'function' || args.length > 2) throw new __nfArrayTypeError(`${name} requires a callback and optional thisObject.`);
    const length = receiver.length, result = [];
    __nfArrayBound(length);
    // The initial length is fixed; current values are fetched for each index.
    // AVM2 visits holes as undefined, unlike JavaScript Array.prototype.map.
    for (let index = 0; index < length; index++) {
        const value = receiver[index], answer = __nfArrayApply(callback, thisObject, [value, index, receiver]);
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
function __nfArrayMethod(receiver, name, as3) {
    // Resolve once before caller evaluates arguments. An override, including a
    // getter or explicitly undefined property, must retain ordinary call behavior.
    const method = receiver[name];
    const array = __nfArrayIsArray(receiver);
    const synthesizeSortOn = array && name === 'sortOn' && !(name in receiver);
    const synthesizeMutation = as3 && array && (name === 'insertAt' || name === 'removeAt') && !(name in receiver);
    const native = array && Object.hasOwn(__nfArrayMethods, name) && method === __nfArrayMethods[name];
    return (...args) => {
        if (!native && !synthesizeSortOn && !synthesizeMutation) {
            if (typeof method !== 'function') throw new __nfArrayTypeError(`Array/custom member '${name}' is not callable.`);
            return __nfArrayApply(method, receiver, args);
        }
        __nfArrayBound(receiver.length);
        if (synthesizeMutation) {
            const required = name === 'insertAt' ? 2 : 1;
            if (args.length !== required) throw new __nfArrayTypeError(`Array.${name} requires ${required} arguments.`);
            const service = typeof __nfLegacy !== 'undefined' ? __nfLegacy?.objects3 : null;
            if (typeof service?.arrayPlan !== 'function') throw new __nfArrayTypeError('Rust Array mutation services are unavailable; rebuild and re-export this project.');
            let plan;
            try { plan = service.arrayPlan(name, receiver.length, __nfArrayNumber(args[0]) | 0); }
            catch (error) { throw new __nfArrayRangeError(String(error?.message ?? error)); }
            // Only storage operations happen here. Rust owns index and growth policy.
            // Refuse sparse/accessor/frozen storage before touching any element.
            if (!Object.isExtensible(receiver) || !Object.getOwnPropertyDescriptor(receiver, 'length')?.writable) throw new __nfArrayTypeError('Array insertAt/removeAt requires ordinary mutable dense storage.');
            for (let i = 0; i < receiver.length; i++) {
                const descriptor = Object.getOwnPropertyDescriptor(receiver, String(i));
                if (!descriptor || !Object.hasOwn(descriptor, 'value') || !descriptor.writable || !descriptor.configurable) throw new __nfArrayTypeError('Array insertAt/removeAt does not yet support sparse arrays or exotic element descriptors.');
            }
            const removed = __nfArrayApply(__nfArrayMethods.splice, receiver, plan.insert ? [plan.index, 0, args[1]] : [plan.index, plan.remove]);
            return plan.insert ? undefined : removed[0];
        }
        if (name === 'sort' || name === 'sortOn') return __nfArraySort(receiver, args, as3, name === 'sortOn');
        if (['map', 'filter', 'forEach', 'every', 'some'].includes(name)) {
            if (!as3) throw new __nfArrayTypeError(`${name} is an AS3 Array method. Use an AS2 loop or a custom method.`);
            return __nfArrayIterate(receiver, name, args);
        }
        if (as3 && (name === 'indexOf' || name === 'lastIndexOf')) {
            const length = receiver.length, backwards = name === 'lastIndexOf';
            let index = args.length > 1 ? (__nfArrayNumber(args[1]) | 0) : backwards ? length - 1 : 0;
            if (index < 0) index = length + index;
            index = backwards ? Math.min(index, length - 1) : Math.max(index, 0);
            for (; index >= 0 && index < length; index += backwards ? -1 : 1) if (receiver[index] === args[0]) return index;
            return -1;
        }
        if (name === 'push' || name === 'unshift') __nfArrayBound(receiver.length + args.length);
        if (name === 'splice') __nfArrayBound(receiver.length + Math.max(0, args.length - 2));
        // Ordinary mutation/search/slicing uses native indexed storage. Exotic
        // coercions, borrowed receivers and legacy sparse-array quirks are not claimed.
        return __nfArrayApply(method, receiver, args);
    };
}
