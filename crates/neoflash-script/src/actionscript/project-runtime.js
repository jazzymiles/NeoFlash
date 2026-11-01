// Per-publication class registry. No ambient language switch or global patches.
const __nfProgram = (() => {
    const definitions = new Map(), interfaces = new Map(), identities = new WeakMap(), fieldStates = new WeakMap();
    const accessorStates = new WeakMap(), slots = new WeakMap(), classFacades = new WeakMap();
    const elementDescriptors = new Map();
    const initialized = new WeakMap(), superMethods = new WeakMap(), failed = new WeakMap();
    const reflectionInterfaces = new Map();
    const intrinsics = new Set();
    const initClips = [];
    const prototypeOwners = new WeakMap();
    const objectDefaults = {toString:Object.prototype.toString,toLocaleString:Object.prototype.toLocaleString};
    const classDefaultString = Function.prototype.toString;
    let nativeTypeNames = [];
    const builtins = { Object, Array: __nfArrayClass(true), Date: __nfDateClass(true), RegExp: __nfRegExpClass(true), JSON: __nfJSON, XML:__nfXML?.XML, XMLList:__nfXML?.XMLList, Namespace:__nfXML?.Namespace, QName:__nfXML?.QName, 'flash.utils.Dictionary':__nfDictionaryClass(true), ...__nfBuiltins };
    function resolveIntrinsic(name) {
        let cur = __nfLegacy?._global;
        for (const part of name.split('.')) {
            if (cur == null) { cur = undefined; break; }
            cur = cur[part];
        }
        return cur !== undefined ? cur : __nfLegacy?.[name];
    }
    function type(name) {
        let value = definitions.get(name)?.facade ?? interfaces.get(name) ?? builtins[name];
        if (value === undefined && intrinsics.has(name)) value = resolveIntrinsic(name);
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
    function elementDescriptor(name) {
        if(!elementDescriptors.has(name))elementDescriptors.set(name,Object.freeze({token:type(name),name,defaultValue:null,coerce:value=>cast(name,value)}));
        return elementDescriptors.get(name);
    }
    function classDefinition(value) {
        const name=identities.get(value)||classFacades.get(value);
        const definition=definitions.get(name);
        return definition?.dialect==='as3'?definition:null;
    }
    function slotMap(receiver) {let map=slots.get(receiver);if(!map)slots.set(receiver,map=new Map());return map;}
    function eligible(member,caller) {
        if(member.access==='public')return true;
        const context=definitions.get(caller);if(!context)return false;
        if(member.access==='private')return member.owner===caller;
        if(member.access==='internal')return definitions.get(member.owner).package===context.package;
        return chain(caller).some(definition=>definition.name===member.owner);
    }
    function resolveSlot(receiver,name,caller) {
        const map=slots.get(receiver);if(!map)return null;
        // A lexical declaration resolves its own namespace before any unrelated
        // same-spelled trait. Virtual overrides share that namespace slot.
        if(caller)for(const definition of chain(caller).reverse()){
            for(const member of [...definition.fields,...definition.methods,...definition.accessors]){
                if(member.name===name&&eligible(member,caller)&&map.has(member.slot))return map.get(member.slot);
            }
        }
        const matches=[...map.values()].filter(slot=>slot.member.name===name&&eligible(slot.member,caller));
        if(matches.length>1)throw new ReferenceError(`Ambiguous project property ${name}; qualify the declaration or use a public accessor.`);
        return matches[0]||null;
    }
    function publicSlot(receiver,name){return [...(slots.get(receiver)?.values()||[])].find(slot=>slot.member.name===name&&slot.member.access==='public')||null;}
    function dynamicAllowed(value){const definition=classDefinition(value);return !definition||(!classFacades.has(value)&&definition.dynamic);}
    function assertDynamicWrite(value,name){
        if(!dynamicAllowed(value))throw new ReferenceError(`Cannot create property ${name} on sealed class ${classDefinition(value).name}; declare it or mark the class dynamic.`);
    }
    function ref(receiver,name,caller) {
        if(!classDefinition(receiver))return null;
        const slot=resolveSlot(receiver,name,caller);
        if(slot)return {get:slot.get,set(value){slot.set(value);return value;},delete:()=>false,has:()=>true};
        // Only public/native descriptors are installed on the actual receiver.
        // Nonpublic traits never leak through raw indexing, JSON or host callbacks.
        const object=Object(receiver);
        return {
            get(){if(Reflect.has(object,name)||dynamicAllowed(receiver))return receiver[name];throw new ReferenceError(`Property ${name} is unavailable on sealed class ${classDefinition(receiver).name}.`);},
            set(value){
                if(!dynamicAllowed(receiver)){
                    let descriptor,owner=object;
                    while(owner&&!(descriptor=Object.getOwnPropertyDescriptor(owner,name)))owner=Object.getPrototypeOf(owner);
                    const inheritedSetter=descriptor?.set;
                    const native=typeof __nfBuiltins.__nfHasNativeProperty==='function'&&__nfBuiltins.__nfHasNativeProperty(receiver,name);
                    if(!Object.hasOwn(object,name)&&!inheritedSetter&&!native)assertDynamicWrite(receiver,name);
                }
                if(!Reflect.set(object,name,value,receiver))throw new TypeError(`Cannot write property ${name}.`);return value;
            },
            delete(){return dynamicAllowed(receiver)?Reflect.deleteProperty(object,name):false;},
            has(){return Reflect.has(object,name);},
        };
    }
    function hasFixedProperty(value,key) {return Boolean(publicSlot(value,key));}
    function objectDescriptor(value,key) {
        if(prototypeOwners.has(value)){
            const descriptor=Object.getOwnPropertyDescriptor(value,key);
            return {supported:true,fixed:false,own:Boolean(descriptor),enumerable:Boolean(descriptor?.enumerable),configurable:Boolean(descriptor?.configurable)};
        }
        const definition=classDefinition(value);
        if(!definition)return null;
        const nativeBase=chain(definition.name)[0]?.base;
        if(nativeBase&&nativeBase!=='Object')return {supported:false};
        const fixed=Boolean(publicSlot(value,key)),descriptor=Object.getOwnPropertyDescriptor(value,key);
        // Class objects expose their own public static traits, not incidental
        // JavaScript function name/length/prototype storage or base statics.
        if(classFacades.has(value))return {supported:true,fixed,own:fixed,enumerable:false,configurable:false};
        return {supported:true,fixed,own:fixed||Boolean(descriptor),enumerable:!fixed&&Boolean(descriptor?.enumerable),configurable:!fixed&&Boolean(descriptor?.configurable)};
    }
    function enumerationKeys(value) {
        if(!identities.has(value))return null;
        const keys=[];
        for(const key in value)if(!hasFixedProperty(value,key)){
            if(keys.length>=1000000)throw new TypeError('Project enumeration exceeds 1000000 entries.');
            keys.push(key);
        }
        return keys;
    }
    function jsonKeys(value) {
        const name=identities.get(value);if(!name)return null;
        const keys=new Set();
        if(definitions.get(name).dialect==='as2'){
            for(const definition of chain(name))for(const field of definition.fields)if(!field.static)keys.add(field.name);
        }else for(const slot of slots.get(value)?.values()||[]){
            if(slot.member.access==='public'&&(slot.kind==='field'||slot.kind==='accessor'&&slot.readable))keys.add(slot.member.name);
        }
        for(const key of Object.keys(value))keys.add(key);
        return [...keys];
    }
    function installSlot(receiver,member,kind,get,set,readable=true) {
        slotMap(receiver).set(member.slot,{member,kind,get,set,readable});
        if(member.access==='public')Object.defineProperty(receiver,member.name,{configurable:true,enumerable:kind==='field',get,set});
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
            if (states.has(field.slot)) throw new Error(`Duplicate inherited field ${field.name}.`);
            const state = { value: field.defaultValue, field };
            states.set(field.slot, state);
            installSlot(receiver,field,'field',()=>state.value,value=>{
                    if (field.constant) throw new TypeError(`AS3 const ${field.name} is read-only.`);
                    state.value = field.coerce(value);
            });
        }
    }
    function initializeFields(name, receiver, isStatic = false) {
        const definition = definitions.get(name);
        if (definition.dialect === 'as2') return; // Scalar defaults live on the shared prototype.
        for (const field of definition.fields.filter(field => field.static === isStatic)) {
            if (field.init) fieldStates.get(receiver).get(field.slot).value = field.coerce(field.init.call(receiver));
        }
    }
    function installMethods(receiver, definition, isStatic) {
        for (const method of definition.methods.filter(method => method.static === isStatic)) {
            if(definition.dialect==='as2')Object.defineProperty(receiver,method.name,{configurable:true,enumerable:false,writable:true,value:method.fn});
            else {
                const value=__nfBindFunction(method.fn,receiver);
                installSlot(receiver,method,'method',()=>value,()=>{throw new TypeError(`AS3 method ${method.name} is read-only.`);});
            }
        }
    }
    function accessorDescriptor(name, pair, receiver) {
        return { configurable: true, enumerable: false,
            get: pair.get ? () => Reflect.apply(pair.get, receiver, []) : () => { throw new TypeError(`AS3 property ${name} is write-only.`); },
            set: pair.set ? value => Reflect.apply(pair.set, receiver, [value]) : () => { throw new TypeError(`AS3 property ${name} is read-only.`); } };
    }
    function installAccessors(receiver, definition, isStatic) {
        if(definition.dialect==='as2') {
            // AVM1 source accessors live on the shared prototype, like methods.
            // Resolve this at invocation so distinct instances never share the
            // receiver or storage of the prototype installation itself.
            for(const accessor of definition.accessors.filter(accessor=>accessor.static===isStatic)) {
                let inherited,owner=Object.getPrototypeOf(receiver);
                while(owner&&!(inherited=Object.getOwnPropertyDescriptor(owner,accessor.name)))owner=Object.getPrototypeOf(owner);
                const read=accessor.get,write=accessor.set;
                Object.defineProperty(receiver,accessor.name,{configurable:true,enumerable:false,
                    get:read?function(){return Reflect.apply(read,this,[]);}:inherited?.get||function(){throw new TypeError(`AS2 property ${accessor.name} is write-only.`);},
                    set:write?function(value){Reflect.apply(write,this,[value]);}:inherited?.set||function(){throw new TypeError(`AS2 property ${accessor.name} is read-only.`);},
                });
            }
            return;
        }
        let slots = accessorStates.get(receiver);
        if (!slots) accessorStates.set(receiver, slots = new Map());
        for (const accessor of definition.accessors.filter(accessor => accessor.static === isStatic)) {
            const inherited = slots.get(accessor.slot);
            const pair = { get: accessor.get || inherited?.get, set: accessor.set || inherited?.set };
            slots.set(accessor.slot, pair);
            const descriptor=accessorDescriptor(accessor.name,pair,receiver);
            installSlot(receiver,accessor,'accessor',descriptor.get,descriptor.set,Boolean(pair.get));
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
        const view = Object.create(null), accessors = new Map();
        let base = definitions.get(name).base;
        while (definitions.has(base)) {
            const definition = definitions.get(base);
            for(const field of definition.fields.filter(field=>!field.static&&eligible(field,name))){
                if(!Object.hasOwn(view,field.name)){const slot=slots.get(receiver)?.get(field.slot);if(slot)Object.defineProperty(view,field.name,{get:slot.get,set:slot.set});}
            }
            for (const method of definition.methods.filter(method => !method.static&&eligible(method,name))) {
                if (!Object.hasOwn(view, method.name)) Object.defineProperty(view,method.name,{value:__nfBindFunction(method.fn,receiver)});
            }
            for (const accessor of definition.accessors.filter(accessor => !accessor.static&&eligible(accessor,name))) {
                const pair = accessors.get(accessor.name) || {};
                pair.get ||= accessor.get; pair.set ||= accessor.set;
                accessors.set(accessor.name, pair);
            }
            base = definition.base;
        }
        for (const [key,pair] of accessors) if(!Object.hasOwn(view,key))Object.defineProperty(view,key,accessorDescriptor(key,pair,receiver));
        if (base) {
            const prototype = type(base).prototype;
            if (prototype) for (const key of Object.getOwnPropertyNames(prototype)) {
                const method = Object.getOwnPropertyDescriptor(prototype,key)?.value;
                if (key !== 'constructor' && !Object.hasOwn(view, key) && typeof method === 'function') view[key] = __nfBindFunction(method,receiver);
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
            if (definitions.get(name).dialect === 'as2') {
                Object.setPrototypeOf(receiver, type(name).prototype);
                Object.defineProperty(receiver, '__constructor__', { configurable: true, writable: true, enumerable: false, value: type(name) });
            } else {
                for (const definition of chain(name)) installFields(receiver, definition, false);
                for (const definition of chain(name)) installMethods(receiver, definition, false);
                for (const definition of chain(name)) installAccessors(receiver, definition, false);
            }
            invokeConstructor(name, receiver, args);
            if(definitions.get(name).dialect==='as3'&&!definitions.get(name).dynamic&&!__nfBuiltins.__nfIsNativeDisplay?.(receiver))Object.preventExtensions(receiver);
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
            if (intrinsics.has(name) && __nfFunctionRecord(ctor)) {
                if (!ctor.prototype) ctor.prototype = Object.create(Object.prototype);
                const receiver = Object.create(ctor.prototype);
                Object.defineProperty(receiver, '__constructor__', { configurable: true, writable: true, enumerable: false, value: ctor });
                ctor.apply(receiver, args);
                return receiver;
            }
            return Reflect.construct(ctor, args);
        }
        const parents = chain(name), nativeBase = parents[0].base;
        const nativeType = nativeBase ? type(nativeBase) : null;
        // Display subclasses need their authoritative runtime identity before
        // fields, listeners or constructor code can observe the receiver.
        // Value classes keep their ordinary prototype/initialization path.
        const receiver = typeof nativeType?.__nfAllocate === 'function'
            ? nativeType.__nfAllocate({ className: name, args })
            : Object.create(definitions.get(name).ordinary ? type(name).prototype : nativeType?.prototype || Object.prototype);
        if (receiver === null || (typeof receiver !== 'object' && typeof receiver !== 'function')) {
            throw new TypeError(`Native allocation for ${name} did not return an object.`);
        }
        return attach(name, receiver, args);
    }
    function defineInterface(name,reflection=null) {
        const facade = function(value) {
            if (new.target) throw new TypeError(`Interface ${name} cannot be constructed.`);
            if (arguments.length !== 1) throw new TypeError(`Interface ${name} conversion expects one value.`);
            return cast(name,value);
        };
        Object.defineProperty(facade, Symbol.hasInstance, { value: value => is(value,name) });
        interfaces.set(name,facade);
        classFacades.set(facade,name);
        if(reflection)reflectionInterfaces.set(name,reflection);
    }
    function define(name, base, fields, methods, ctor, dialect = 'as3', accessors = [], implemented = [], options = {}) {
        const facade = function(...args) {
            if (dialect === 'as2' && !new.target) throw new TypeError('AS2 class-as-function casts are not supported; use new or the existing instance.');
            return construct(name, args);
        };
        Object.defineProperty(facade, Symbol.hasInstance, { value: value => is(value, name) });
        const packageName=options.package??name.slice(0,Math.max(0,name.lastIndexOf('.')));
        if(dialect==='as3'&&definitions.get(base)?.final)throw new TypeError(`Cannot extend final class ${base}.`);
        function member(value){
            const access=dialect==='as2'?'public':value.access||'public';
            let namespace=access==='private'?name:access==='internal'?packageName:'';
            if(access==='protected'){
                const inherited=chain(base).flatMap(definition=>[...definition.fields,...definition.methods,...definition.accessors]).find(item=>item.name===value.name&&item.static===value.static&&item.access==='protected');
                namespace=inherited?.namespace||name;
            }
            const slot=JSON.stringify([Boolean(value.static),access,namespace,value.name]);
            if(dialect==='as3'&&chain(base).some(definition=>[...definition.methods,...definition.accessors].some(item=>item.slot===slot&&item.final)))throw new TypeError(`Cannot override final member ${value.name}.`);
            return {...value,access,owner:name,namespace,slot};
        }
        fields=fields.map(member);methods=methods.map(member);accessors=accessors.map(member);
        const ordinary=dialect==='as3'&&(!base||base==='Object'||definitions.get(base)?.ordinary===true);
        const definition = { name, base, fields, methods, ctor, facade, dialect, accessors, interfaces:implemented,package:packageName,dynamic:Boolean(options.dynamic),final:Boolean(options.final),reflection:options.reflection,ordinary };
        classFacades.set(facade,name);
        definitions.set(name, definition);
        if(ordinary){
            // Adobe avmplus 858d034a3bd3a54d9b70909386435cf4aec81d21
            // core/MethodEnv.cpp::newclass connects prototype
            // delegates, then defines a nonenumerable constructor. Fixed traits
            // remain per-instance slots; native/display allocation is untouched.
            Object.setPrototypeOf(facade.prototype,definitions.get(base)?.facade.prototype||Object.prototype);
            Object.defineProperty(facade.prototype,'constructor',{value:facade,writable:true,configurable:true,enumerable:false});
            Object.defineProperty(facade,'prototype',{writable:false});
            prototypeOwners.set(facade.prototype,name);
        }
        if (dialect === 'as2') {
            if (ctor) Object.defineProperty(ctor, '__nfPublicFunction', { value: facade });
            if (base && base !== 'Object') {
                const parent = type(base);
                facade.prototype = Object.create(parent.prototype || Object.prototype);
                // AVM1 ActionExtends exposes the superclass constructor on the
                // subclass prototype, and creates the chain without invoking it.
                Object.defineProperty(facade.prototype, 'constructor', { configurable: true, writable: true, value: parent });
                Object.defineProperty(facade.prototype, '__constructor__', { configurable: true, writable: true, value: parent });
                if (definitions.has(base)) Object.setPrototypeOf(facade, parent);
            } else {
                facade.prototype = Object.create(Object.prototype);
                Object.defineProperty(facade.prototype, 'constructor', { configurable: true, writable: true, value: facade });
            }
            installFields(facade.prototype, definition, false);
            installMethods(facade.prototype, definition, false);
            installAccessors(facade.prototype, definition, false);
        }
    }
    function initializeStatics() {
        for (const definition of definitions.values()) {
            installFields(definition.facade, definition, true);
            installMethods(definition.facade, definition, true);
            installAccessors(definition.facade, definition, true);
        }
        for (const definition of definitions.values()) initializeFields(definition.name, definition.facade, true);
        for(const definition of definitions.values())if(definition.dialect==='as3')Object.preventExtensions(definition.facade);
    }
    // Host objects stay in JavaScript. Only class identities, compiler-emitted
    // metadata and descriptor facts cross into the Rust reflection policy.
    function reflectionClass(name) {
        if(['Number','int','uint','Boolean'].includes(name))return __nfNumericClass(name);
        if(name==='String')return __nfStringClass(true);
        return type(name);
    }
    function className(value) {
        const numeric=typeof __nfNumericClassName==='function'?__nfNumericClassName(value):null;
        if(numeric)return numeric;
        if(classFacades.has(value))return classFacades.get(value);
        if(value===__nfObjectClass(true))return 'Object';
        if(value===__nfStringClass(true))return 'String';
        for(const name of nativeTypeNames)if(builtins[name]!==undefined&&builtins[name]===value)return name;
        return null;
    }
    function isClass(value) {return className(value)!==null;}
    function constructClass(value,args) {
        const name=className(value);
        if(!name)throw new TypeError('Class construction requires a registered class; ordinary functions are unsupported.');
        if(['Number','int','uint','Boolean'].includes(name))return __nfScalarCreate(name,args);
        if(name==='String')return __nfStringCreate(true,args);
        if(name==='Object')return __nfObjectCreate(true,args,true);
        return construct(name,args);
    }
    function reflectionValue(value) {
        const name=className(value);
        if(name)return {kind:'class',name};
        if(identities.has(value)){
            const name=identities.get(value);
            return definitions.get(name)?.dialect==='as3'?{kind:'project',name}:{kind:'unsupported'};
        }
        if(prototypeOwners.has(value))return {kind:'object'};
        if(value===null)return {kind:'null'};
        const kind=typeof value;
        if(['undefined','boolean','number','string','function'].includes(kind))return {kind};
        if(__nfArrayIsArray(value))return {kind:'array'};
        if(__nfDateIs(value))return {kind:'date'};
        if(__nfRegExpIs(value))return {kind:'regexp'};
        if(__nfBuiltins.__nfIsNativeDisplay?.(value))return {kind:'unsupported'};
        const prototype=Object.getPrototypeOf(value);
        return {kind:prototype===Object.prototype||prototype===null?'object':'unsupported'};
    }
    function defaultStringDescriptor(receiver) {
        const definition=classDefinition(receiver);
        if(definition)return definition.ordinary?{name:definition.name,class_object:classFacades.has(receiver)}:null;
        if(prototypeOwners.has(receiver))return {name:'Object',class_object:false};
        if(receiver===null||typeof receiver!=='object'||__nfBuiltins.__nfIsNativeDisplay?.(receiver))return null;
        const prototype=Object.getPrototypeOf(receiver);
        return prototype===Object.prototype||prototype===null?{name:'Object',class_object:false}:null;
    }
    function defaultObjectString() {
        const descriptor=defaultStringDescriptor(this);
        if(!descriptor)throw new TypeError('Default Object string conversion supports ordinary AS3 source objects and plain records; provide an explicit supported receiver.');
        if(typeof __nfBuiltins.__nfObjectExecute!=='function')throw new TypeError('Rust Object formatting is unavailable; rebuild and re-export this project.');
        return __nfBuiltins.__nfObjectExecute({op:'default_string',...descriptor});
    }
    function defaultObjectMethod(receiver,name,resolved) {
        if(!Object.hasOwn(objectDefaults,name)||!defaultStringDescriptor(receiver))return null;
        if(resolved!==objectDefaults[name]&&!(name==='toString'&&classFacades.has(receiver)&&resolved===classDefaultString))return null;
        // Both prototype methods share the native formatter in Adobe Object.as.
        // In particular, the default locale method does not call an authored
        // toString override. Use the invocation receiver, preserving call/apply.
        return defaultObjectString;
    }
    function reflect(operation,args) {
        if(args.length!==1)throw new TypeError(`${operation} expects exactly one argument in the source reflection profile.`);
        if(typeof __nfBuiltins.__nfObjectExecute!=='function')throw new TypeError('Rust Object reflection is unavailable; rebuild and re-export this project.');
        const metadata=[...definitions.values()].filter(definition=>definition.dialect==='as3').map(definition=>definition.reflection).filter(Boolean);
        metadata.push(...reflectionInterfaces.values());
        const request={op:operation,classes:metadata};
        if(operation==='getDefinitionByName') {
            if(typeof args[0]!=='string')throw new TypeError('getDefinitionByName currently requires a primitive String name.');
            request.name=args[0];request.definitions=[...nativeTypeNames.filter(name=>builtins[name]!==undefined),'Number','int','uint','Boolean','String'];
        } else request.value=reflectionValue(args[0]);
        let result;
        try{result=__nfBuiltins.__nfObjectExecute(request);}
        catch(error){throw operation==='getDefinitionByName'?new ReferenceError(String(error?.message||error)):new TypeError(String(error?.message||error));}
        if(operation==='getDefinitionByName')return reflectionClass(result);
        if(operation==='describeType')return new __nfXML.XML(result);
        return result;
    }
    for(const name of ['getQualifiedClassName','getQualifiedSuperclassName','getDefinitionByName','describeType'])builtins['flash.utils.'+name]=(...args)=>reflect(name,args);
    function declareIntrinsic(name) { intrinsics.add(name); }
    function registerInitClip(key, fn) { initClips.push({ key, fn }); }
    function runInitClips(root) {
        for (const entry of initClips) __nfRunInitClip(entry.key, root, entry.fn);
    }
    return Object.freeze({ type, is, cast, define, defineInterface, declareIntrinsic, registerInitClip, runInitClips, attach, construct, superConstruct, superView, initializeFields, initializeStatics, jsonKeys, elementDescriptor, enumerationKeys, hasFixedProperty, ref, assertDynamicWrite, objectDescriptor, isClass, constructClass, className, reflect, defaultObjectMethod,
        setNativeTypes:names=>{nativeTypeNames=names;},
        dialect:name=>definitions.get(name)?.dialect });
})();
