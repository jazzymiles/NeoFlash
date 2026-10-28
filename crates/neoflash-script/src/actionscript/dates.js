// Date arithmetic delegates to the shared Rust calendar. Browser Date slots
// retain identity/AMF interoperability, timezone, locale and existing parse facilities.
// Deliberate partial boundaries and licensed adaptations: RUNTIME-DATE-PROVENANCE.md.
// Native Date internal slots provide identity across independently compiled units.
// No browser prototype is changed and no global registry is required.
const __nfDateNative = Date;
const __nfDateNumber = Number;
const __nfDateString = String;
const __nfDateApply = Reflect.apply;
const __nfDateParseNative = Date.parse;
const __nfDateMethods = Object.freeze(Object.fromEntries(
    Reflect.ownKeys(Date.prototype)
        .map(name => [name, Object.getOwnPropertyDescriptor(Date.prototype, name).value])
        .filter(([name, value]) => name !== 'constructor' && typeof value === 'function')
));
const __nfDateNativeMethods = new Set(Object.values(__nfDateMethods));
const __nfDateClasses = new Map();
const __nfDateStringLimit = 65536;
const __nfDateGetters = new Set([
    'getDate', 'getDay', 'getFullYear', 'getHours', 'getMilliseconds', 'getMinutes',
    'getMonth', 'getSeconds', 'getTime', 'getTimezoneOffset', 'getUTCDate', 'getUTCDay',
    'getUTCFullYear', 'getUTCHours', 'getUTCMilliseconds', 'getUTCMinutes',
    'getUTCMonth', 'getUTCSeconds', 'valueOf',
]);
const __nfDateFormats = new Set([
    'toString', 'toDateString', 'toTimeString', 'toUTCString',
    'toLocaleString', 'toLocaleDateString', 'toLocaleTimeString',
]);
const __nfDateSetters = Object.freeze({
    setDate: 1, setFullYear: 3, setHours: 4, setMilliseconds: 1, setMinutes: 3,
    setMonth: 2, setSeconds: 2, setTime: 1, setUTCDate: 1, setUTCFullYear: 3,
    setUTCHours: 4, setUTCMilliseconds: 1, setUTCMinutes: 3, setUTCMonth: 2,
    setUTCSeconds: 2,
});
const __nfDateAliases = Object.freeze({
    date: 'Date', day: 'Day', fullYear: 'FullYear', hours: 'Hours',
    milliseconds: 'Milliseconds', minutes: 'Minutes', month: 'Month', seconds: 'Seconds',
    time: 'Time', timezoneOffset: 'TimezoneOffset', dateUTC: 'UTCDate', dayUTC: 'UTCDay',
    fullYearUTC: 'UTCFullYear', hoursUTC: 'UTCHours', millisecondsUTC: 'UTCMilliseconds',
    minutesUTC: 'UTCMinutes', monthUTC: 'UTCMonth', secondsUTC: 'UTCSeconds',
});

function __nfDateUnsupported(detail) {
    throw new __nfTypeError('Unsupported ActionScript Date operation: ' + detail);
}
function __nfDateService() {
    if (typeof __nfLegacy === 'undefined' || !__nfLegacy || !__nfLegacy.date) {
        __nfDateUnsupported('Rust Date bindings are missing; rebuild and re-export this project.');
    }
    return __nfLegacy.date;
}
function __nfDateIs(value) {
    if (value === null || typeof value !== 'object') return false;
    try { __nfDateApply(__nfDateMethods.getTime, value, []); return true; }
    catch (_) { return false; }
}
function __nfDateCast(value) {
    if (value == null) return null;
    if (__nfDateIs(value)) return value;
    throw new __nfTypeError('Cannot coerce this value to Date.');
}
function __nfDateNumberValue(value, operation) {
    const type = typeof value;
    if ((value !== null && (type === 'object' || type === 'function')) || type === 'symbol' || type === 'bigint') {
        __nfDateUnsupported(operation + ' requires primitive numeric components; custom conversion objects are pending.');
    }
    if (type === 'string' && value.length > __nfDateStringLimit) {
        __nfDateUnsupported(operation + ' input exceeds 65536 UTF-16 code units.');
    }
    return __nfDateNumber(value);
}
function __nfDateNumbers(args, count, operation) {
    const result = [];
    for (let index = 0; index < Math.min(args.length, count); index++) {
        result.push(__nfDateNumberValue(args[index], operation));
    }
    return result;
}
function __nfDateParse(value) {
    const type = typeof value;
    if ((value !== null && (type === 'object' || type === 'function')) || type === 'symbol' || type === 'bigint') {
        __nfDateUnsupported('parse requires a primitive string; custom conversion objects are pending.');
    }
    const text = __nfDateString(value);
    if (text.length > __nfDateStringLimit) __nfDateUnsupported('parse input exceeds 65536 UTF-16 code units.');
    return __nfDateParseNative(text);
}
function __nfDateJSON(key) {
    // Adobe Date.as explicitly calls AS3::toString, not the dynamic override.
    return __nfDateApply(__nfDateMethods.toString, this, []);
}
function __nfDateDecorate(value, as3) {
    if (!as3) return value;
    Object.defineProperty(value, 'toJSON', { value: __nfDateJSON, writable: true, configurable: true });
    for (const [name, suffix] of Object.entries(__nfDateAliases)) {
        const getter = __nfDateMethods['get' + suffix], setter = __nfDateMethods['set' + suffix];
        Object.defineProperty(value, name, {
            // AS3 native properties are not enumerable dynamic data members.
            get() { return __nfDateService().get(this, 'get' + suffix); },
            set: setter ? function(next) {
                const number = __nfDateNumberValue(next, name);
                __nfDateService().set(this, 'set' + suffix, [number]);
            } : function() { throw new __nfTypeError('Date.' + name + ' is read-only.'); },
        });
    }
    return value;
}
function __nfDateCreate(as3, args) {
    let parts;
    if (args.length === 1) {
        const first = args[0];
        parts = [typeof first === 'string' ? __nfDateParse(first)
            : __nfDateIs(first) ? __nfDateApply(__nfDateMethods.getTime, first, [])
            : __nfDateNumberValue(first, 'constructor')];
    } else {
        // Adobe's constructor ignores values beyond the seventh component.
        parts = __nfDateNumbers(args, 7, 'constructor');
    }
    return __nfDateDecorate(__nfDateService().create(parts), Boolean(as3));
}
function __nfDateClass(as3) {
    as3 = Boolean(as3);
    if (!__nfDateClasses.has(as3)) {
        const facade = function(...args) {
            if (new.target && new.target !== facade) __nfDateUnsupported('native Date subclass construction is pending.');
            // Adobe DateClass::call returns the current date string and ignores
            // arguments. Typed Date coercion uses __nfDateCast separately.
            return new.target ? __nfDateCreate(as3, args) : __nfDateApply(__nfDateNative, undefined, []);
        };
        Object.defineProperties(facade, {
            length: { value: 7 },
            parse: { value: value => __nfDateParse(value) },
            UTC: { value: (...args) => {
                const parts = __nfDateNumbers(args, 7, 'UTC');
                // UTC requires year and month; modern JS defaults month for a
                // one-argument call, whereas Flash's missing month is NaN.
                while (parts.length < 2) parts.push(NaN);
                return __nfDateService().utc(parts);
            } },
            __nfIs: { value: __nfDateIs },
            [Symbol.hasInstance]: { value: __nfDateIs },
            __nfInitialize: { value: () => __nfDateUnsupported('native Date subclass initialization is pending.') },
        });
        Object.freeze(facade.prototype);
        __nfDateClasses.set(as3, Object.freeze(facade));
    }
    return __nfDateClasses.get(as3);
}
function __nfDateMethodValue(value) {
    if (__nfDateNativeMethods.has(value)) {
        __nfDateUnsupported('native method extraction is not implemented; call the method directly on the Date.');
    }
    return value;
}
function __nfDateMethod(receiver, name, as3) {
    // Resolve exactly once, before argument expressions. A user-defined method
    // retains its normal receiver and is never selected merely by its name.
    const method = receiver[name];
    if (!__nfDateIs(receiver) || method !== __nfDateMethods[name]) {
        return (...args) => __nfDateApply(method, receiver, args);
    }
    return (...args) => {
        if (name === 'toJSON') {
            if (!as3) __nfDateUnsupported('toJSON is an AS3 Date method.');
            return __nfDateApply(__nfDateJSON, receiver, args);
        }
        if (__nfDateGetters.has(name) || (!as3 && name === 'getYear')) {
            if (args.length) __nfDateUnsupported(__nfDateString(name) + ' arguments are outside this profile.');
            return __nfDateService().get(receiver, name);
        }
        if (__nfDateFormats.has(name)) {
            if (args.length) __nfDateUnsupported(__nfDateString(name) + ' arguments, including Intl locale/options, are outside this profile.');
            if (name === 'toUTCString') return __nfDateService().utcString(receiver);
            return __nfDateApply(method, receiver, []);
        }
        const maximum = !as3 && name === 'setYear' ? 1 : __nfDateSetters[name];
        if (maximum !== undefined) {
            if (args.length > maximum) __nfDateUnsupported(name + ' excess-argument behavior is not verified.');
            // Convert the complete supported argument list before changing the
            // receiver. Missing first values deliberately produce Invalid Date.
            return __nfDateService().set(receiver, name, __nfDateNumbers(args, maximum, name));
        }
        __nfDateUnsupported(__nfDateString(name) + ' is outside the supported Date profile.');
    };
}
