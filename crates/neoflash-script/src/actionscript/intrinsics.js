// Bounded reference profiles, not a complete AVM. Helpers are embedded in each
// compiled AS handler; JavaScript/TypeScript globals and prototypes are untouched.
// AS2 details: Ruffle AVM1 globals.rs at fe6cfc545405b888ebb478ce1c211d63147ca49b.
// AS3 precision conversion: Adobe avmplus Number.as / NumberClass.cpp at
// 858d034a3bd3a54d9b70909386435cf4aec81d21. See intrinsic-fixtures.mjs for provenance.
const __nfNativeParseInt = parseInt;
const __nfNativeFixed = Number.prototype.toFixed;
const __nfApply = Reflect.apply;
function __nfMethodValue(value) {
    // A computed property name can select the native method without any
    // statically recognizable "toFixed" token. Check the resulting identity,
    // not the key/name: custom functions and fields remain ordinary values.
    if (value === __nfNativeFixed) {
        throw new __nfErrorTypes.Error('Unsupported AS3 Number.toFixed method value: method-closure semantics are not implemented; call value.toFixed(precision) directly.');
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
