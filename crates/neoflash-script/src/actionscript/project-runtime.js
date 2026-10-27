// Per-publication class registry. No ambient language switch or global patches.
const __nfProgram = (() => {
    const definitions = new Map(), interfaces = new Map(), identities = new WeakMap(), fieldStates = new WeakMap();
    const accessorStates = new WeakMap();
    const initialized = new WeakMap(), superMethods = new WeakMap(), failed = new WeakMap();
    const builtins = { Object, Array: __nfArrayClass(true), Date: __nfDateClass(true), RegExp: __nfRegExpClass(true), JSON: __nfJSON, ...__nfBuiltins };
    function type(name) {
        const value = definitions.get(name)?.facade ?? interfaces.get(name) ?? builtins[name];
        if (value === undefined) throw new Error(`Project type ${name} has no runtime implementation.`);
        return value;
    }
    function is(value, name) {
        if (value == null) return false;
        let current = identities.get(value);
        while (current) {
            if (current === name) return true;
            if (definitions.get(current)?.interfaces.includes(name)) return true;
            current = definitions.get(current)?.base;
        }
        if (definitions.has(name) || interfaces.has(name)) return false;
        const ctor = type(name);
        if (typeof ctor.__nfIs === 'function') return ctor.__nfIs(value);
        if (name === 'Object') return true;
        return typeof ctor === 'function' && value instanceof ctor;
    }
    function cast(name, value) {
        if (value == null) return null;
        if (!is(value, name)) throw new TypeError(`AS3 ${name} value required.`);
        return value;
    }
    function chain(name) {
        const result = [];
        for (let current = definitions.get(name); current; current = definitions.get(current.base)) result.unshift(current);
        return result;
    }
    function jsonKeys(value) {
        const name = identities.get(value);
        if (!name) return null;
        const keys = new Set();
        for (const definition of chain(name)) {
            for (const field of definition.fields) if (!field.static) keys.add(field.name);
            // A setter-only override preserves the inherited getter. This
            // metadata lookup must not execute the getter while listing keys.
            for (const accessor of definition.accessors) if (!accessor.static && accessor.get) keys.add(accessor.name);
        }
        for (const key of Object.keys(value)) keys.add(key);
        return [...keys];
    }
    function installFields(receiver, definition, isStatic) {
        if (definition.dialect === 'as2') {
            for (const field of definition.fields.filter(field => field.static === isStatic)) {
                Object.defineProperty(receiver, field.name, { configurable: true, enumerable: false, writable: true,
                    value: field.init ? field.init.call(receiver) : undefined });
            }
            return;
        }
        let states = fieldStates.get(receiver);
        if (!states) fieldStates.set(receiver, states = new Map());
        for (const field of definition.fields.filter(field => field.static === isStatic)) {
            if (states.has(field.name)) throw new Error(`Duplicate inherited field ${field.name}.`);
            const state = { value: field.defaultValue, field };
            states.set(field.name, state);
            Object.defineProperty(receiver, field.name, {
                configurable: true, enumerable: true,
                get() { return state.value; },
                set(value) {
                    if (field.constant) throw new TypeError(`AS3 const ${field.name} is read-only.`);
                    state.value = field.coerce(value);
                },
            });
        }
    }
    function initializeFields(name, receiver, isStatic = false) {
        const definition = definitions.get(name);
        if (definition.dialect === 'as2') return; // Scalar defaults live on the shared prototype.
        for (const field of definition.fields.filter(field => field.static === isStatic)) {
            if (field.init) fieldStates.get(receiver).get(field.name).value = field.coerce(field.init.call(receiver));
        }
    }
    function installMethods(receiver, definition, isStatic) {
        for (const method of definition.methods.filter(method => method.static === isStatic)) {
            Object.defineProperty(receiver, method.name, { configurable: true, enumerable: false,
                writable: definition.dialect === 'as2', value: definition.dialect === 'as2' ? method.fn : method.fn.bind(receiver) });
        }
    }
    function accessorDescriptor(name, pair, receiver) {
        return { configurable: true, enumerable: false,
            get: pair.get ? () => Reflect.apply(pair.get, receiver, []) : () => { throw new TypeError(`AS3 property ${name} is write-only.`); },
            set: pair.set ? value => Reflect.apply(pair.set, receiver, [value]) : () => { throw new TypeError(`AS3 property ${name} is read-only.`); } };
    }
    function installAccessors(receiver, definition, isStatic) {
        let slots = accessorStates.get(receiver);
        if (!slots) accessorStates.set(receiver, slots = new Map());
        for (const accessor of definition.accessors.filter(accessor => accessor.static === isStatic)) {
            const inherited = slots.get(accessor.name);
            const pair = { get: accessor.get || inherited?.get, set: accessor.set || inherited?.set };
            slots.set(accessor.name, pair);
            Object.defineProperty(receiver, accessor.name, accessorDescriptor(accessor.name, pair, receiver));
        }
    }
    function invokeConstructor(name, receiver, args) {
        let seen = initialized.get(receiver);
        if (!seen) initialized.set(receiver, seen = new Set());
        if (seen.has(name)) throw new Error(`Constructor ${name} was invoked more than once on this object.`);
        seen.add(name);
        const definition = definitions.get(name);
        if (definition.ctor) return Reflect.apply(definition.ctor, receiver, args);
        if (args.length && definition.dialect !== 'as2') throw new TypeError(`${name} default constructor expects no arguments.`);
        superConstruct(name, receiver, []);
        initializeFields(name, receiver);
    }
    function superConstruct(name, receiver, args) {
        const base = definitions.get(name).base;
        if (!base) { if (args.length) throw new TypeError('Object base constructor expects no arguments.'); return; }
        if (definitions.has(base)) return invokeConstructor(base, receiver, args);
        const ctor = type(base);
        if (typeof ctor.__nfInitialize === 'function') return ctor.__nfInitialize(receiver, args);
        const value = Reflect.construct(ctor, args);
        Object.defineProperties(receiver, Object.getOwnPropertyDescriptors(value));
    }
    function superView(name, receiver) {
        let cache = superMethods.get(receiver);
        if (!cache) superMethods.set(receiver, cache = new Map());
        if (cache.has(name)) return cache.get(name);
        const view = Object.create(null);
        const accessors = new Map();
        let base = definitions.get(name).base;
        while (definitions.has(base)) {
            const definition = definitions.get(base);
            for (const method of definition.methods.filter(method => !method.static)) {
                if (!Object.hasOwn(view, method.name)) view[method.name] = method.fn.bind(receiver);
            }
            for (const accessor of definition.accessors.filter(accessor => !accessor.static)) {
                const pair = accessors.get(accessor.name) || {};
                pair.get ||= accessor.get; pair.set ||= accessor.set;
                accessors.set(accessor.name, pair);
            }
            base = definition.base;
        }
        for (const [key,pair] of accessors) Object.defineProperty(view,key,accessorDescriptor(key,pair,receiver));
        if (base) {
            const prototype = type(base).prototype;
            if (prototype) for (const key of Object.getOwnPropertyNames(prototype)) {
                const method = Object.getOwnPropertyDescriptor(prototype,key)?.value;
                if (key !== 'constructor' && !Object.hasOwn(view, key) && typeof method === 'function') view[key] = method.bind(receiver);
            }
        }
        cache.set(name, view);
        return view;
    }
    function attach(name, receiver, args = []) {
        if (!definitions.has(name)) throw new Error(`Linked class ${name} is not a project class.`);
        if (receiver == null || (typeof receiver !== 'object' && typeof receiver !== 'function')) throw new TypeError('A linked class needs an object receiver.');
        if (failed.has(receiver)) throw failed.get(receiver);
        if (identities.has(receiver)) {
            if (identities.get(receiver) !== name) throw new Error('Cannot change an existing object class.');
            return receiver;
        }
        identities.set(receiver, name);
        try {
            if (definitions.get(name).dialect === 'as2') Object.setPrototypeOf(receiver, type(name).prototype);
            else {
                for (const definition of chain(name)) installFields(receiver, definition, false);
                for (const definition of chain(name)) installMethods(receiver, definition, false);
                for (const definition of chain(name)) installAccessors(receiver, definition, false);
            }
            invokeConstructor(name, receiver, args);
        } catch (error) {
            failed.set(receiver, error);
            throw error;
        }
        return receiver;
    }
    function construct(name, args = []) {
        if (interfaces.has(name)) throw new TypeError(`Interface ${name} cannot be constructed.`);
        if (!definitions.has(name)) {
            const ctor = type(name);
            if (typeof ctor !== 'function') throw new TypeError(`${name} is not constructible.`);
            return Reflect.construct(ctor, args);
        }
        const parents = chain(name), nativeBase = parents[0].base;
        const nativeType = nativeBase ? type(nativeBase) : null;
        // Display subclasses need their authoritative runtime identity before
        // fields, listeners or constructor code can observe the receiver.
        // Value classes keep their ordinary prototype/initialization path.
        const receiver = typeof nativeType?.__nfAllocate === 'function'
            ? nativeType.__nfAllocate({ className: name, args })
            : Object.create(nativeType?.prototype || Object.prototype);
        if (receiver === null || (typeof receiver !== 'object' && typeof receiver !== 'function')) {
            throw new TypeError(`Native allocation for ${name} did not return an object.`);
        }
        return attach(name, receiver, args);
    }
    function defineInterface(name) {
        const facade = function(value) {
            if (new.target) throw new TypeError(`Interface ${name} cannot be constructed.`);
            if (arguments.length !== 1) throw new TypeError(`Interface ${name} conversion expects one value.`);
            return cast(name,value);
        };
        Object.defineProperty(facade, Symbol.hasInstance, { value: value => is(value,name) });
        interfaces.set(name,facade);
    }
    function define(name, base, fields, methods, ctor, dialect = 'as3', accessors = [], implemented = []) {
        const facade = function(...args) {
            if (dialect === 'as2' && !new.target) throw new TypeError('AS2 class-as-function casts are not supported; use new or the existing instance.');
            return construct(name, args);
        };
        Object.defineProperty(facade, Symbol.hasInstance, { value: value => is(value, name) });
        const definition = { name, base, fields, methods, ctor, facade, dialect, accessors, interfaces:implemented };
        definitions.set(name, definition);
        if (dialect === 'as2') {
            const parent = base ? type(base) : Object;
            facade.prototype = Object.create(parent.prototype || Object.prototype);
            // AVM1 ActionExtends exposes the superclass constructor on the
            // subclass prototype, and creates the chain without invoking it.
            Object.defineProperty(facade.prototype, 'constructor', { configurable: true, writable: true, value: parent });
            Object.defineProperty(facade.prototype, '__constructor__', { configurable: true, writable: true, value: parent });
            installFields(facade.prototype, definition, false);
            installMethods(facade.prototype, definition, false);
            if (definitions.has(base)) Object.setPrototypeOf(facade, parent);
        }
    }
    function initializeStatics() {
        for (const definition of definitions.values()) {
            installFields(definition.facade, definition, true);
            installMethods(definition.facade, definition, true);
            installAccessors(definition.facade, definition, true);
        }
        for (const definition of definitions.values()) initializeFields(definition.name, definition.facade, true);
    }
    return Object.freeze({ type, is, cast, define, defineInterface, attach, construct, superConstruct, superView, initializeFields, initializeStatics, jsonKeys,
        dialect:name=>definitions.get(name)?.dialect });
})();
