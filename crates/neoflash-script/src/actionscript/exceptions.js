// Compiler intrinsics, captured before entering any user function/catch scope.
const __nfThrownBrand = Symbol.for('neoflash.as2.thrown.v1');
const __nfErrorKind = Symbol.for('neoflash.error.kind.v1');
const __nfDescriptor = Object.getOwnPropertyDescriptor;
const __nfDefineProperty = Object.defineProperty;
const __nfErrorTypes = { Error, TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError,
    ArgumentError: class extends Error { constructor() { super(); this.name = 'ArgumentError'; } },
    DefinitionError: class extends Error { constructor() { super(); this.name = 'DefinitionError'; } },
    SecurityError: class extends Error { constructor() { super(); this.name = 'SecurityError'; } },
    VerifyError: class extends Error { constructor() { super(); this.name = 'VerifyError'; } } };
function __nfIsThrown(value) {
    if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return false;
    try {
        const tag = __nfDescriptor(value, __nfThrownBrand), payload = __nfDescriptor(value, 'value');
        return !!tag && tag.value === true && !!payload && 'value' in payload;
    } catch (_) { return false; }
}
function __nfCatchType(value, type) {
    if (type === '*') return true;
    if (value == null) return false;
    switch (type) {
        case 'Object': return true;
        case 'Number': return typeof value === 'number';
        case 'String': return typeof value === 'string';
        case 'Boolean': return typeof value === 'boolean';
        case 'Function': return typeof value === 'function';
        case 'int': return typeof value === 'number' && (value | 0) === value;
        case 'uint': return typeof value === 'number' && (value >>> 0) === value;
        case 'ArgumentError':
        case 'DefinitionError':
        case 'SecurityError':
        case 'VerifyError': {
            if (!(value instanceof __nfErrorTypes.Error)) return false;
            const kind = __nfDescriptor(value, __nfErrorKind);
            return value instanceof __nfErrorTypes[type] || (!!kind && kind.value === type) || value.name === type;
        }
        default: return value instanceof __nfErrorTypes[type];
    }
}
function __nfMakeError(type, as3, args) {
    const error = new __nfErrorTypes[type]();
    __nfDefineProperty(error, __nfErrorKind, { value: type });
    error.message = as3 ? (args.length ? args[0] : '') : (args[0] === undefined ? 'Error' : args[0]);
    if (as3) __nfDefineProperty(error, 'errorID', { value: args.length > 1 ? (__nfNumber(args[1]) | 0) : 0 });
    error.toString = as3
        ? function() { const value = this.message !== '' ? this.name + ': ' + this.message : this.name; return value == null ? null : __nfString(value); }
        : function() { return this.message; };
    if (as3) {
        error.getStackTrace = function() {
            const head = this.toString();
            if (typeof this.stack === 'string' && this.stack.length > 0) {
                return __nfString(this.stack.startsWith(head) ? this.stack : head + '\n' + this.stack);
            }
            return head;
        };
    }
    return error;
}
