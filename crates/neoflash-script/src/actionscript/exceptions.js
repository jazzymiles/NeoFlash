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
const __nfFlashErrors = (() => {
    function init(error, shortName, args) {
        __nfDefineProperty(error, __nfErrorKind, { value: shortName, configurable: true });
        error.name = shortName;
        error.message = args.length && args[0] !== undefined ? __nfString(args[0]) : '';
        __nfDefineProperty(error, 'errorID', { value: args.length > 1 ? (__nfNumber(args[1]) | 0) : 0, configurable: true });
        error.toString = function() {
            const value = this.message !== '' ? this.name + ': ' + this.message : this.name;
            return value == null ? null : __nfString(value);
        };
        error.getStackTrace = function() {
            const head = this.toString();
            if (typeof this.stack === 'string' && this.stack.length > 0) {
                return __nfString(this.stack.startsWith(head) ? this.stack : head + '\n' + this.stack);
            }
            return head;
        };
    }
    function makeClass(shortName, Base) {
        class FlashError extends Base {
            constructor() {
                super();
                init(this, shortName, arguments);
            }
        }
        function Ctor() {
            return new FlashError(...arguments);
        }
        Ctor.prototype = FlashError.prototype;
        __nfDefineProperty(Ctor.prototype, 'constructor', { value: Ctor, writable: true, configurable: true });
        __nfDefineProperty(Ctor, '__nfIs', {
            value(value) {
                if (!(value instanceof __nfErrorTypes.Error)) return false;
                const kind = __nfDescriptor(value, __nfErrorKind);
                const tag = (kind && kind.value) || value.name;
                if (shortName === 'IOError') return value instanceof FlashError || tag === 'IOError' || tag === 'EOFError';
                return value instanceof FlashError || tag === shortName;
            }
        });
        __nfDefineProperty(Ctor, '__nfInitialize', {
            value(receiver, args) {
                init(receiver, shortName, args);
            }
        });
        return [Ctor, FlashError];
    }
    const [IOError, IOErrorClass] = makeClass('IOError', __nfErrorTypes.Error);
    const [EOFError] = makeClass('EOFError', IOErrorClass);
    const [IllegalOperationError] = makeClass('IllegalOperationError', __nfErrorTypes.Error);
    const [InvalidSWFError] = makeClass('InvalidSWFError', __nfErrorTypes.Error);
    const [MemoryError] = makeClass('MemoryError', __nfErrorTypes.Error);
    const [ScriptTimeoutError] = makeClass('ScriptTimeoutError', __nfErrorTypes.Error);
    const [StackOverflowError] = makeClass('StackOverflowError', __nfErrorTypes.Error);
    return {
        'flash.errors.IOError': IOError,
        'flash.errors.EOFError': EOFError,
        'flash.errors.IllegalOperationError': IllegalOperationError,
        'flash.errors.InvalidSWFError': InvalidSWFError,
        'flash.errors.MemoryError': MemoryError,
        'flash.errors.ScriptTimeoutError': ScriptTimeoutError,
        'flash.errors.StackOverflowError': StackOverflowError,
    };
})();
const __nfErrorClasses = new Map();
function __nfErrorClass(name, as3 = true) {
    const key = `${as3 ? '3' : '2'}:${name}`;
    if (!__nfErrorClasses.has(key)) {
        if (!Object.hasOwn(__nfErrorTypes, name)) throw new __nfTypeError('Unsupported Error class ' + name);
        const facade = function(...args) {
            return __nfMakeError(name, as3, args);
        };
        Object.defineProperties(facade, {
            name: { value: name },
            length: { value: 1 },
            prototype: { value: __nfErrorTypes[name].prototype, writable: false },
            __nfIs: { value: value => __nfCatchType(value, name) },
            [Symbol.hasInstance]: { value: value => __nfCatchType(value, name) },
        });
        __nfErrorClasses.set(key, Object.freeze(facade));
    }
    return __nfErrorClasses.get(key);
}
