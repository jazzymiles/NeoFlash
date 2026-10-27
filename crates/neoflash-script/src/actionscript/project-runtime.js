// Per-publication class registry. No ambient language switch or global patches.
const __nfProgram = (() => {
    const definitions = new Map(), identities = new WeakMap(), fieldStates = new WeakMap();
    const initialized = new WeakMap(), superMethods = new WeakMap(), failed = new WeakMap();
    const builtins = { Object, Array: __nfArrayClass(true), ...__nfBuiltins };
    function type(name) {
        const value = definitions.get(name)?.facade ?? builtins[name];
        if (value === undefined) throw new Error(`Project type ${name} has no runtime implementation.`);
        return value;
    }
    function is(value, name) {
        if (value == null) return false;
        let current = identities.get(value);
        while (current) {
            if (current === name) return true;
            current = definitions.get(current)?.base;
        }
        if (definitions.has(name)) return false;
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
        let base = definitions.get(name).base;
        while (definitions.has(base)) {
            const definition = definitions.get(base);
            for (const method of definition.methods.filter(method => !method.static)) {
                if (!Object.hasOwn(view, method.name)) view[method.name] = method.fn.bind(receiver);
            }
            base = definition.base;
        }
        if (base) {
            const prototype = type(base).prototype;
            if (prototype) for (const key of Object.getOwnPropertyNames(prototype)) {
                if (key !== 'constructor' && !Object.hasOwn(view, key) && typeof prototype[key] === 'function') view[key] = prototype[key].bind(receiver);
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
            }
            invokeConstructor(name, receiver, args);
        } catch (error) {
            failed.set(receiver, error);
            throw error;
        }
        return receiver;
    }
    function construct(name, args = []) {
        if (!definitions.has(name)) {
            const ctor = type(name);
            if (typeof ctor !== 'function') throw new TypeError(`${name} is not constructible.`);
            return Reflect.construct(ctor, args);
        }
        const parents = chain(name), nativeBase = parents[0].base;
        const prototype = nativeBase ? type(nativeBase).prototype : Object.prototype;
        return attach(name, Object.create(prototype || Object.prototype), args);
    }
    function define(name, base, fields, methods, ctor, dialect = 'as3') {
        const facade = function(...args) {
            if (dialect === 'as2' && !new.target) throw new TypeError('AS2 class-as-function casts are not supported; use new or the existing instance.');
            return construct(name, args);
        };
        Object.defineProperty(facade, Symbol.hasInstance, { value: value => is(value, name) });
        const definition = { name, base, fields, methods, ctor, facade, dialect };
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
        }
        for (const definition of definitions.values()) initializeFields(definition.name, definition.facade, true);
    }
    return Object.freeze({ type, is, cast, define, attach, construct, superConstruct, superView, initializeFields, initializeStatics,
        dialect:name=>definitions.get(name)?.dialect });
})();
