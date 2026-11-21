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
    const flashProxyNs = typeof __nfXML?.namespace === 'function'
        ? __nfXML.namespace('flash_proxy', 'http://www.adobe.com/2006/actionscript/flash/proxy')
        : Object.freeze({ prefix: 'flash_proxy', uri: 'http://www.adobe.com/2006/actionscript/flash/proxy', toString() { return this.uri; } });
    const objectProxyNs = typeof __nfXML?.namespace === 'function'
        ? __nfXML.namespace('object_proxy', 'http://www.adobe.com/2006/flex/mx/internal')
        : Object.freeze({ prefix: 'object_proxy', uri: 'http://www.adobe.com/2006/flex/mx/internal', toString() { return this.uri; } });
    const FlashProxy = typeof __nfFlashProxyBase === 'function' ? __nfFlashProxyBase : (() => {
        function FP() {}
        FP.prototype = Object.create(Object.prototype);
        Object.defineProperty(FP, '__nfInitialize', { value: function() {} });
        Object.defineProperty(FP, '__nfIs', { value: function(v) { return v instanceof FP; } });
        return FP;
    })();
    const MxObjectProxy = typeof __nfMxObjectProxyClass === 'function' ? __nfMxObjectProxyClass : FlashProxy;
    const unwrapProxy = value => typeof __nfUnwrapAs3Proxy === 'function' ? __nfUnwrapAs3Proxy(value) : value;
    const builtins = { Object, Class: __nfClassMetaclass, Function: __nfFunctionClass, Array: __nfArrayClass(true), Date: __nfDateClass(true), RegExp: __nfRegExpClass(true), JSON: __nfJSON, XML:__nfXML?.XML, XMLList:__nfXML?.XMLList, Namespace:__nfXML?.Namespace, QName:__nfXML?.QName, 'flash.utils.Dictionary':__nfDictionaryClass(true), 'flash.utils.Proxy': FlashProxy, 'flash.utils.flash_proxy': flashProxyNs, 'flash.utils.object_proxy': objectProxyNs, 'mx.utils.object_proxy': objectProxyNs, 'mx.utils.ObjectProxy': MxObjectProxy, ...__nfFlashErrors, ...__nfBuiltins, ...(typeof __nfWave7Builtins === 'function' ? __nfWave7Builtins(__nfBuiltins) : {}) };
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
        let current = ownerName(value);
        while (current) {
            if (current === name) return true;
            if (definitions.get(current)?.interfaces.includes(name)) return true;
            current = definitions.get(current)?.base;
        }
        if (definitions.has(name) || interfaces.has(name)) return false;
        const ctor = type(name);
        if (typeof ctor.__nfIs === 'function') return ctor.__nfIs(value);
        if (name === 'Object') return true;
        return typeof ctor === 'function' && unwrapProxy(value) instanceof ctor;
    }
    function cast(name, value) {
        if (value == null) return null;
        if (!is(value, name)) throw new TypeError(`AS3 ${name} value required.`);
        return value;
    }
    function as2Cast(name, value) {
        if (value == null || (typeof value !== 'object' && typeof value !== 'function')) return null;
        if (definitions.has(name) || interfaces.has(name)) {
            if (is(value, name)) return value;
            if (!interfaces.has(name)) {
                const facade = definitions.get(name)?.facade;
                if (typeof facade === 'function' && __nfLegacyInstanceOf(value, facade)) return value;
            }
            return null;
        }
        const ctor = builtins[name] ?? resolveIntrinsic(name) ?? (name === 'TextField.StyleSheet' ? __nfLegacy?.TextField?.StyleSheet : undefined);
        if (ctor == null) return null;
        if (typeof ctor.__nfIs === 'function' && ctor.__nfIs(value)) return value;
        if (name === 'Object') return value;
        if (typeof ctor === 'function' && __nfLegacyInstanceOf(value, ctor)) return value;
        return null;
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
        const raw=unwrapProxy(value);
        const name=identities.get(raw)||identities.get(value)||classFacades.get(value);
        const definition=definitions.get(name);
        return definition?.dialect==='as3'?definition:null;
    }
    function ownerName(value) {
        return identities.get(unwrapProxy(value))||identities.get(value);
    }
    function slotMap(receiver) {const raw=unwrapProxy(receiver);let map=slots.get(raw);if(!map)slots.set(raw,map=new Map());return map;}
    function callerClass(caller) {
        return typeof caller === 'object' && caller !== null ? caller.class : caller;
    }
    function callerNamespaces(caller) {
        return typeof caller === 'object' && caller !== null && Array.isArray(caller.ns) ? caller.ns : null;
    }
    function eligible(member,caller) {
        if(member.access==='public')return true;
        const cls=callerClass(caller);
        const ns=callerNamespaces(caller);
        if(member.access==='namespace'){
            if(!caller)return false;
            if(ns)return ns.includes(member.namespace);
            if(!cls)return false;
            return chain(cls).some(definition=>definition.openNamespaces?.includes(member.namespace));
        }
        const context=definitions.get(cls);if(!context)return false;
        if(member.access==='private')return member.owner===cls;
        if(member.access==='internal')return definitions.get(member.owner).package===context.package;
        return chain(cls).some(definition=>definition.name===member.owner);
    }
    function nativeNamespaceSlot(receiver,name,uri){
        if(!uri||!isProxy(receiver))return null;
        const raw=unwrapProxy(receiver);
        if(is(receiver,'mx.utils.ObjectProxy')){
            const sym=Symbol.for('neoflash.ns.'+uri+'::'+name);
            if(sym in raw){
                return {get:()=>raw[sym],set:v=>{raw[sym]=v;},member:{name,access:'namespace',namespace:uri}};
            }
        }
        if(uri==='http://www.adobe.com/2006/actionscript/flash/proxy'||uri==='http://www.adobe.com/2006/flex/mx/internal'){
            const protoFn=(is(receiver,'mx.utils.ObjectProxy')&&typeof MxObjectProxy.prototype?.[name]==='function')
                ?MxObjectProxy.prototype[name]
                :FlashProxy.prototype[name];
            if(typeof protoFn==='function'&&Object.hasOwn(FlashProxy.prototype,name)){
                return {get:()=>protoFn.bind(receiver),set:()=>{throw new TypeError('Cannot overwrite native Proxy method.');},member:{name,access:'namespace',namespace:uri}};
            }
        }
        return null;
    }
    function resolveOpenNamespaceSlot(receiver,name,caller,cls){
        const openNs=callerNamespaces(caller)||(cls?definitions.get(cls)?.openNamespaces:null);
        if(Array.isArray(openNs)){
            for(const uri of openNs){
                const nsSlot=nativeNamespaceSlot(receiver,name,uri);
                if(nsSlot)return nsSlot;
            }
        }
        return null;
    }
    function resolveSlot(receiver,name,caller) {
        const raw=unwrapProxy(receiver);
        const map=slots.get(raw);
        const cls=callerClass(caller);
        if(!map)return resolveOpenNamespaceSlot(receiver,name,caller,cls);
        // A lexical declaration resolves its own namespace before any unrelated
        // same-spelled trait. Virtual overrides share that namespace slot.
        if(cls)for(const definition of chain(cls).reverse()){
            for(const member of [...definition.fields,...definition.methods,...definition.accessors]){
                if(member.name===name&&eligible(member,caller)&&map.has(member.slot))return map.get(member.slot);
            }
        }
        const matches=[...map.values()].filter(slot=>slot.member.name===name&&eligible(slot.member,caller));
        if(matches.length>1)throw new ReferenceError(`Ambiguous project property ${name}; qualify the declaration or use a public accessor.`);
        if(matches.length===1)return matches[0];
        return resolveOpenNamespaceSlot(receiver,name,caller,cls);
    }
    function publicSlot(receiver,name){return [...(slots.get(unwrapProxy(receiver))?.values()||[])].find(slot=>slot.member.name===name&&slot.member.access==='public')||null;}
    function namespaceSlot(receiver,name,uri){
        if(!uri)return null;
        return [...(slots.get(unwrapProxy(receiver))?.values()||[])].find(slot=>slot.member.name===name&&slot.member.access==='namespace'&&slot.member.namespace===uri)||nativeNamespaceSlot(receiver,name,uri);
    }
    function resolveQualifiedSlot(receiver,name,caller,uri=null){
        if(uri){
            const nsSlot=namespaceSlot(receiver,name,uri);
            if(nsSlot)return nsSlot;
        }
        const effectiveCaller=callerClass(caller)?caller:(caller?{class:ownerName(receiver),ns:callerNamespaces(caller)}:ownerName(receiver));
        return resolveSlot(receiver,name,effectiveCaller)||publicSlot(receiver,name);
    }
    function isProxy(receiver){
        return receiver!=null&&(typeof receiver==='object'||typeof receiver==='function')&&!classFacades.has(receiver)&&(is(receiver,'flash.utils.Proxy')||is(receiver,'mx.utils.ObjectProxy'));
    }
    const proxyBypassProps=new Set(['hasOwnProperty','propertyIsEnumerable','setPropertyIsEnumerable','isPrototypeOf','toString','valueOf','toLocaleString','constructor']);
    function isProxyBypassProp(name){
        return typeof name==='symbol'||name==='__proto__'||name==='prototype'||name==='then'||(typeof name==='string'&&name.startsWith('__nf'))||proxyBypassProps.has(name);
    }
    const proxyTrapNamespaces=['http://www.adobe.com/2006/flex/mx/internal','http://www.adobe.com/2006/actionscript/flash/proxy'];
    function hasNativeObjectProxyTrait(receiver,name){
        return is(receiver,'mx.utils.ObjectProxy')&&typeof MxObjectProxy.__nfHasFixedMethod==='function'&&MxObjectProxy.__nfHasFixedMethod(name);
    }
    function hasTrait(receiver,name,caller){
        if(isProxyBypassProp(name)||hasNativeObjectProxyTrait(receiver,name))return true;
        const effectiveCaller=callerClass(caller)?caller:(caller?{class:ownerName(receiver),ns:callerNamespaces(caller)}:ownerName(receiver));
        if(!classDefinition(receiver))return Boolean(resolveSlot(receiver,name,effectiveCaller));
        return Boolean(resolveSlot(receiver,name,effectiveCaller));
    }
    function invokeProxy(receiver,trap,args){
        const owner=ownerName(receiver);
        if(owner){
            const slot=resolveSlot(receiver,trap,{class:owner,ns:proxyTrapNamespaces})||publicSlot(receiver,trap);
            if(slot){
                const fn=slot.get();
                if(typeof fn==='function')return Reflect.apply(fn,receiver,args);
            }
        }
        if(is(receiver,'mx.utils.ObjectProxy')&&typeof MxObjectProxy.prototype?.[trap]==='function'){
            return Reflect.apply(MxObjectProxy.prototype[trap],receiver,args);
        }
        const protoFn=FlashProxy.prototype[trap];
        if(typeof protoFn==='function')return Reflect.apply(protoFn,receiver,args);
        throw new TypeError(`Proxy trap ${trap} is unavailable.`);
    }
    function dynamicAllowed(value){const definition=classDefinition(value);return !definition||(!classFacades.has(value)&&definition.dynamic);}
    function assertDynamicWrite(value,name){
        if(!dynamicAllowed(value))throw new ReferenceError(`Cannot create property ${name} on sealed class ${classDefinition(value).name}; declare it or mark the class dynamic.`);
    }
    function ref(receiver,name,caller) {
        if(!classDefinition(receiver)){
            const nsSlot=resolveSlot(receiver,name,caller);
            if(nsSlot)return {get:nsSlot.get,set(value){nsSlot.set(value);return value;},delete:()=>false,has:()=>true};
            if(isProxy(receiver)&&!isProxyBypassProp(name)&&!hasNativeObjectProxyTrait(receiver,name)){
                return {
                    get:()=>invokeProxy(receiver,'getProperty',[name]),
                    set(value){invokeProxy(receiver,'setProperty',[name,value]);return value;},
                    delete:()=>Boolean(invokeProxy(receiver,'deleteProperty',[name])),
                    has:()=>Boolean(invokeProxy(receiver,'hasProperty',[name])),
                };
            }
            return null;
        }
        const slot=resolveSlot(receiver,name,caller);
        if(slot)return {get:slot.get,set(value){slot.set(value);return slot.kind==='field'?slot.get():value;},delete:()=>false,has:()=>true};
        if(isProxy(receiver)&&!isProxyBypassProp(name)&&!hasNativeObjectProxyTrait(receiver,name)){
            return {
                get:()=>invokeProxy(receiver,'getProperty',[name]),
                set(value){invokeProxy(receiver,'setProperty',[name,value]);return value;},
                delete:()=>Boolean(invokeProxy(receiver,'deleteProperty',[name])),
                has:()=>Boolean(invokeProxy(receiver,'hasProperty',[name])),
            };
        }
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
        else if(member.access==='namespace'&&member.namespace){
            Object.defineProperty(receiver,Symbol.for('neoflash.ns.'+member.namespace+'::'+member.name),{configurable:true,enumerable:false,get,set});
        }
    }
    function installFields(receiver, definition, isStatic, selfReceiver = receiver) {
        const raw = unwrapProxy(receiver);
        if (definition.dialect === 'as2') {
            for (const field of definition.fields.filter(field => field.static === isStatic)) {
                Object.defineProperty(raw, field.name, { configurable: true, enumerable: false, writable: true,
                    value: field.init ? field.init.call(selfReceiver) : undefined });
            }
            return;
        }
        let states = fieldStates.get(raw);
        if (!states) fieldStates.set(raw, states = new Map());
        for (const field of definition.fields.filter(field => field.static === isStatic)) {
            if (states.has(field.slot)) throw new Error(`Duplicate inherited field ${field.name}.`);
            const state = { value: field.defaultValue, field };
            states.set(field.slot, state);
            installSlot(raw,field,'field',()=>state.value,value=>{
                    if (field.constant) throw new TypeError(`AS3 const ${field.name} is read-only.`);
                    state.value = field.coerce(value);
            });
        }
    }
    function initializeFields(name, receiver, isStatic = false) {
        const definition = definitions.get(name);
        if (definition.dialect === 'as2') return; // Scalar defaults live on the shared prototype.
        const raw = unwrapProxy(receiver);
        for (const field of definition.fields.filter(field => field.static === isStatic)) {
            if (field.init) fieldStates.get(raw).get(field.slot).value = field.coerce(field.init.call(receiver));
        }
    }
    function installMethods(receiver, definition, isStatic, selfReceiver = receiver) {
        const raw = unwrapProxy(receiver);
        for (const method of definition.methods.filter(method => method.static === isStatic)) {
            if(definition.dialect==='as2')Object.defineProperty(raw,method.name,{configurable:true,enumerable:false,writable:true,value:method.fn});
            else {
                const value=__nfBindFunction(method.fn,selfReceiver);
                installSlot(raw,method,'method',()=>value,()=>{throw new TypeError(`AS3 method ${method.name} is read-only.`);});
            }
        }
    }
    function accessorDescriptor(name, pair, receiver) {
        return { configurable: true, enumerable: false,
            get: pair.get ? () => Reflect.apply(pair.get, receiver, []) : () => { throw new TypeError(`AS3 property ${name} is write-only.`); },
            set: pair.set ? value => Reflect.apply(pair.set, receiver, [value]) : () => { throw new TypeError(`AS3 property ${name} is read-only.`); } };
    }
    function installAccessors(receiver, definition, isStatic, selfReceiver = receiver) {
        const raw = unwrapProxy(receiver);
        if(definition.dialect==='as2') {
            // AVM1 source accessors live on the shared prototype, like methods.
            // Resolve this at invocation so distinct instances never share the
            // receiver or storage of the prototype installation itself.
            for(const accessor of definition.accessors.filter(accessor=>accessor.static===isStatic)) {
                let inherited,owner=Object.getPrototypeOf(raw);
                while(owner&&!(inherited=Object.getOwnPropertyDescriptor(owner,accessor.name)))owner=Object.getPrototypeOf(owner);
                const read=accessor.get,write=accessor.set;
                Object.defineProperty(raw,accessor.name,{configurable:true,enumerable:false,
                    get:read?function(){return Reflect.apply(read,this,[]);}:inherited?.get||function(){throw new TypeError(`AS2 property ${accessor.name} is write-only.`);},
                    set:write?function(value){Reflect.apply(write,this,[value]);}:inherited?.set||function(){throw new TypeError(`AS2 property ${accessor.name} is read-only.`);},
                });
            }
            return;
        }
        let slots = accessorStates.get(raw);
        if (!slots) accessorStates.set(raw, slots = new Map());
        for (const accessor of definition.accessors.filter(accessor => accessor.static === isStatic)) {
            const inherited = slots.get(accessor.slot);
            const pair = { get: accessor.get || inherited?.get, set: accessor.set || inherited?.set };
            slots.set(accessor.slot, pair);
            const descriptor=accessorDescriptor(accessor.name,pair,selfReceiver);
            installSlot(raw,accessor,'accessor',descriptor.get,descriptor.set,Boolean(pair.get));
        }
    }
    function invokeConstructor(name, receiver, args) {
        const raw = unwrapProxy(receiver);
        let seen = initialized.get(raw);
        if (!seen) initialized.set(raw, seen = new Set());
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
        const raw = unwrapProxy(receiver);
        const ctor = type(base);
        if (typeof ctor.__nfInitialize === 'function') return ctor.__nfInitialize(raw, args);
        const value = Reflect.construct(ctor, args);
        Object.defineProperties(raw, Object.getOwnPropertyDescriptors(value));
    }
    function superView(name, receiver) {
        const raw = unwrapProxy(receiver);
        let cache = superMethods.get(raw);
        if (!cache) superMethods.set(raw, cache = new Map());
        if (cache.has(name)) return cache.get(name);
        const view = Object.create(null), accessors = new Map();
        let base = definitions.get(name).base;
        while (definitions.has(base)) {
            const definition = definitions.get(base);
            for(const field of definition.fields.filter(field=>!field.static&&eligible(field,name))){
                if(!Object.hasOwn(view,field.name)){const slot=slots.get(raw)?.get(field.slot);if(slot)Object.defineProperty(view,field.name,{get:slot.get,set(value){slot.set(value);return slot.kind==='field'?slot.get():value;}});}
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
            for (let prototype = type(base)?.prototype; prototype; prototype = Object.getPrototypeOf(prototype)) {
                for (const key of Object.getOwnPropertyNames(prototype)) {
                    if (key === 'constructor' || Object.hasOwn(view, key)) continue;
                    const desc = Object.getOwnPropertyDescriptor(prototype, key);
                    if (typeof desc?.value === 'function') {
                        view[key] = __nfBindFunction(desc.value, receiver);
                    } else if (desc?.get || desc?.set) {
                        Object.defineProperty(view, key, {
                            configurable: true,
                            get: desc.get ? () => Reflect.apply(desc.get, receiver, []) : undefined,
                            set: desc.set ? v => Reflect.apply(desc.set, receiver, [v]) : undefined,
                        });
                    }
                }
            }
        }
        const resolvedView = base ? new Proxy(view, {
            get(target, prop, recv) {
                if (typeof prop === 'symbol' || prop === 'constructor' || Object.hasOwn(target, prop)) {
                    return Reflect.get(target, prop, recv);
                }
                const hasProjectSlot = [...(slots.get(raw)?.values() || [])].some(s => s.member.name === prop);
                if (!hasProjectSlot && receiver != null && prop in Object(receiver)) {
                    const val = receiver[prop];
                    return typeof val === 'function' ? __nfBindFunction(val, receiver) : val;
                }
                return Reflect.get(target, prop, recv);
            },
            has(target, prop) {
                if (typeof prop === 'symbol') return Reflect.has(target, prop);
                if (Object.hasOwn(target, prop)) return true;
                const hasProjectSlot = [...(slots.get(raw)?.values() || [])].some(s => s.member.name === prop);
                if (!hasProjectSlot && receiver != null && prop in Object(receiver)) return true;
                return Reflect.has(target, prop);
            },
            set(target, prop, value, recv) {
                if (typeof prop === 'symbol' || Object.hasOwn(target, prop)) {
                    return Reflect.set(target, prop, value, recv);
                }
                const hasProjectSlot = [...(slots.get(raw)?.values() || [])].some(s => s.member.name === prop);
                if (!hasProjectSlot && receiver != null && prop in Object(receiver)) {
                    receiver[prop] = value;
                    return true;
                }
                return Reflect.set(target, prop, value, recv);
            },
        }) : view;
        cache.set(name, resolvedView);
        return resolvedView;
    }
    function attach(name, receiver, args = []) {
        if (!definitions.has(name)) throw new Error(`Linked class ${name} is not a project class.`);
        if (receiver == null || (typeof receiver !== 'object' && typeof receiver !== 'function')) throw new TypeError('A linked class needs an object receiver.');
        const raw = unwrapProxy(receiver);
        if (failed.has(raw)) throw failed.get(raw);
        if (identities.has(raw)) {
            if (identities.get(raw) !== name) throw new Error('Cannot change an existing object class.');
            return receiver;
        }
        identities.set(raw, name);
        const def = definitions.get(name);
        const proxyNativeBase = def.dialect === 'as3' ? chain(name)[0]?.base : null;
        const needsAs3Proxy = (proxyNativeBase === 'flash.utils.Proxy' || proxyNativeBase === 'mx.utils.ObjectProxy') && typeof __nfCreateAs3Proxy === 'function';
        let selfReceiver = receiver;
        if (needsAs3Proxy) {
            selfReceiver = __nfCreateAs3Proxy(raw, {
                hasFixedTrait: prop => Boolean(publicSlot(raw, prop)) || hasNativeObjectProxyTrait(selfReceiver, prop),
                getFixedTrait: prop => {
                    const ps = publicSlot(raw, prop);
                    if (ps) return ps.get();
                    const fn = raw[prop];
                    return typeof fn === 'function' ? fn.bind(selfReceiver) : fn;
                },
                setFixedTrait: (prop, val) => {
                    const ps = publicSlot(raw, prop);
                    if (ps) { ps.set(val); return; }
                    raw[prop] = val;
                },
                invokeTrap: (trap, trapArgs) => invokeProxy(selfReceiver, trap, trapArgs),
            });
            identities.set(selfReceiver, name);
        }
        try {
            if (def.dialect === 'as2') {
                Object.setPrototypeOf(raw, type(name).prototype);
                Object.defineProperty(raw, '__constructor__', { configurable: true, writable: true, enumerable: false, value: type(name) });
            } else {
                for (const definition of chain(name)) installFields(raw, definition, false, selfReceiver);
                for (const definition of chain(name)) installMethods(raw, definition, false, selfReceiver);
                for (const definition of chain(name)) installAccessors(raw, definition, false, selfReceiver);
            }
            invokeConstructor(name, selfReceiver, args);
            if(def.dialect==='as3'&&!def.dynamic&&!needsAs3Proxy&&!__nfBuiltins.__nfIsNativeDisplay?.(raw))Object.preventExtensions(raw);
        } catch (error) {
            failed.set(raw, error);
            throw error;
        }
        return selfReceiver;
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
            const customNs=dialect==='as3'&&typeof value.ns==='string'&&value.ns.length>0?value.ns:null;
            const access=dialect==='as2'?'public':customNs?'namespace':(value.access||'public');
            let namespace=customNs?customNs:access==='private'?name:access==='internal'?packageName:'';
            if(access==='protected'){
                const inherited=chain(base).flatMap(definition=>[...definition.fields,...definition.methods,...definition.accessors]).find(item=>item.name===value.name&&item.static===value.static&&item.access==='protected');
                namespace=inherited?.namespace||name;
            }
            const slot=JSON.stringify([Boolean(value.static),access,namespace,value.name]);
            if(dialect==='as3'&&chain(base).some(definition=>[...definition.methods,...definition.accessors].some(item=>item.slot===slot&&item.final)))throw new TypeError(`Cannot override final member ${value.name}.`);
            return {...value,access,owner:name,namespace,slot};
        }
        fields=fields.map(member);methods=methods.map(member);accessors=accessors.map(member);
        const ordinary=dialect==='as3'&&(!base||base==='Object'||base==='flash.utils.Proxy'||base==='mx.utils.ObjectProxy'||definitions.get(base)?.ordinary===true);
        const openNamespaces=Array.isArray(options.openNamespaces)?options.openNamespaces:[];
        const definition = { name, base, fields, methods, ctor, facade, dialect, accessors, interfaces:implemented,package:packageName,openNamespaces,dynamic:Boolean(options.dynamic),final:Boolean(options.final),reflection:options.reflection,ordinary };
        classFacades.set(facade,name);
        definitions.set(name, definition);
        if(ordinary){
            // Adobe avmplus 858d034a3bd3a54d9b70909386435cf4aec81d21
            // core/MethodEnv.cpp::newclass connects prototype
            // delegates, then defines a nonenumerable constructor. Fixed traits
            // remain per-instance slots; native/display allocation is untouched.
            Object.setPrototypeOf(facade.prototype,definitions.get(base)?.facade.prototype||(base==='flash.utils.Proxy'?FlashProxy.prototype:base==='mx.utils.ObjectProxy'?MxObjectProxy.prototype:Object.prototype));
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
        if(name==='Object')return __nfObjectClass(true);
        if(name==='String')return __nfStringClass(true);
        if(name==='Array')return __nfArrayClass(true);
        if(name==='Date')return __nfDateClass(true);
        if(name==='RegExp')return __nfRegExpClass(true);
        if(name==='Math')return __nfMath(true);
        if(name==='JSON')return __nfJSON;
        if(name==='Class')return __nfClassMetaclass;
        if(name==='Function')return __nfFunctionClass;
        if(name==='flash.utils.Dictionary'&&typeof __nfDictionaryClass==='function')return __nfDictionaryClass(true);
        if(['XML','XMLList','Namespace','QName'].includes(name)&&typeof __nfXML!=='undefined'&&__nfXML?.[name])return __nfXML[name];
        if(name.startsWith('Vector.<')&&name.endsWith('>')&&typeof __nfVectorType==='function'){
            const elem=name.slice(8,-1).replace('::','.');
            return __nfVectorType(['Number','String','Boolean','int','uint','Object','*'].includes(elem)?__nfPrimitiveDescriptor(elem):elementDescriptor(elem));
        }
        if(Object.hasOwn(__nfErrorTypes,name))return __nfErrorClass(name,true);
        if(builtins[name]!==undefined)return builtins[name];
        return type(name);
    }
    function className(value) {
        const numeric=typeof __nfNumericClassName==='function'?__nfNumericClassName(value):null;
        if(numeric)return numeric;
        if(classFacades.has(value))return classFacades.get(value);
        if(value===__nfObjectClass(true)||value===Object)return 'Object';
        if(value===__nfStringClass(true)||value===String)return 'String';
        if(value===__nfArrayClass(true)||value===Array)return 'Array';
        if(value===__nfDateClass(true)||value===Date)return 'Date';
        if(value===__nfRegExpClass(true)||value===RegExp)return 'RegExp';
        if(value===__nfMath(true)||value===Math)return 'Math';
        if(value===__nfJSON||value===JSON)return 'JSON';
        if(value===__nfClassMetaclass)return 'Class';
        if(value===__nfFunctionClass||value===Function)return 'Function';
        if(value===FlashProxy)return 'flash.utils.Proxy';
        if(value===MxObjectProxy)return 'mx.utils.ObjectProxy';
        if(typeof __nfDictionaryClass==='function'&&value===__nfDictionaryClass(true))return 'flash.utils.Dictionary';
        if(typeof __nfXML!=='undefined'&&__nfXML){
            for(const t of ['XML','XMLList','Namespace','QName']){
                if(value===__nfXML[t])return t;
            }
        }
        if(typeof value==='function'){
            const vt=Object.getOwnPropertyDescriptor(value,Symbol.for('neoflash.as3.vector.type.v1'));
            if(vt?.value?.name)return 'Vector.<'+vt.value.name+'>';
            if(typeof value.name==='string'&&Object.hasOwn(__nfErrorTypes,value.name)&&value===__nfErrorClass(value.name,true))return value.name;
        }
        const hostCls=__nfBuiltins.__nfNativeClassName?.(value);
        if(hostCls)return hostCls;
        for(const name of nativeTypeNames)if(builtins[name]!==undefined&&builtins[name]===value)return name;
        return null;
    }
    function isInterface(value) {
        const name = classFacades.get(value) || __nfBuiltins.__nfNativeClassName?.(value);
        return Boolean(name && (interfaces.has(name) || __nfBuiltins.__nfIsNativeInterface?.(name)));
    }
    function isClass(value) {return className(value)!==null&&!isInterface(value);}
    function constructClass(value,args) {
        const name=className(value);
        if(!name)throw new TypeError('Class construction requires a registered class; ordinary functions are unsupported.');
        if(['Number','int','uint','Boolean'].includes(name))return __nfScalarCreate(name,args);
        if(name==='String')return __nfStringCreate(true,args);
        if(name==='Object')return __nfObjectCreate(true,args,true);
        if(name==='Array')return __nfArrayCreate(true,args);
        if(name==='Date')return new (__nfDateClass(true))(...args);
        if(name==='RegExp')return new (__nfRegExpClass(true))(...args);
        if(name==='flash.utils.Dictionary'&&typeof __nfDictionaryClass==='function')return new (__nfDictionaryClass(true))(...args);
        if(name.startsWith('Vector.<')&&typeof value==='function')return new value(...args);
        if(builtins[name]&&!definitions.has(name))return new builtins[name](...args);
        return construct(name,args);
    }
    function reflectionValue(value) {
        const name=className(value);
        if(name)return {kind:'class',name};
        const rawVal=unwrapProxy(value);
        if(identities.has(rawVal)){
            const iname=identities.get(rawVal);
            return definitions.get(iname)?.dialect==='as3'?{kind:'project',name:iname}:{kind:'unsupported'};
        }
        if(MxObjectProxy.__nfIs?.(rawVal))return {kind:'native',name:'mx.utils.ObjectProxy'};
        if(FlashProxy.__nfIs?.(rawVal))return {kind:'native',name:'flash.utils.Proxy'};
        if(prototypeOwners.has(value))return {kind:'object'};
        if(value===null)return {kind:'null'};
        if(value===undefined)return {kind:'undefined'};
        const kind=typeof value;
        if(kind==='boolean'||kind==='string'||kind==='function')return {kind};
        if(kind==='number'){
            const numName=(Number.isInteger(value)&&!Object.is(value,-0)&&value>=-2147483648&&value<=2147483647)?'int':'Number';
            return {kind:'number',name:numName};
        }
        if(__nfArrayIsArray(value))return {kind:'array'};
        if(__nfDateIs(value))return {kind:'date'};
        if(__nfRegExpIs(value))return {kind:'regexp'};
        if(typeof __nfVectorIs==='function'&&__nfVectorIs(value))return {kind:'native',name:'Vector.<'+__nfVectorState(value).element.name+'>'};
        if(typeof __nfDictionaryIs==='function'&&__nfDictionaryIs(value))return {kind:'native',name:'flash.utils.Dictionary'};
        if(typeof __nfXML!=='undefined'){
            if(__nfXML.XML?.__nfIs?.(value))return {kind:'native',name:'XML'};
            if(__nfXML.XMLList?.__nfIs?.(value))return {kind:'native',name:'XMLList'};
            if(__nfXML.Namespace?.__nfIs?.(value))return {kind:'native',name:'Namespace'};
            if(__nfXML.QName?.__nfIs?.(value))return {kind:'native',name:'QName'};
            if(__nfXML.XMLDocument?.__nfIs?.(value))return {kind:'native',name:'flash.xml.XMLDocument'};
            if(__nfXML.XMLNode?.__nfIs?.(value))return {kind:'native',name:'flash.xml.XMLNode'};
        }
        const nativeName=__nfBuiltins.__nfNativeTypeName?.(value);
        if(nativeName)return {kind:'native',name:nativeName};
        if(value instanceof Error&&typeof value.name==='string'&&Object.hasOwn(__nfErrorTypes,value.name))return {kind:'native',name:value.name};
        if(__nfBuiltins.__nfIsNativeDisplay?.(value))return {kind:'native',name:'flash.display.DisplayObject'};
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
    function harvestNativeClassDescriptor(qname) {
        const Ctor=builtins[qname];
        if(typeof Ctor!=='function'||definitions.has(qname)||reflectionInterfaces.has(qname))return null;
        const members=[];
        const scan=(holder,isStatic)=>{
            if(!holder||(typeof holder!=='object'&&typeof holder!=='function'))return;
            let descs;try{descs=Object.getOwnPropertyDescriptors(holder);}catch(_){return;}
            for(const [k,d] of Object.entries(descs)){
                if(!k||k.startsWith('__nf')||k==='constructor'||k==='prototype'||k==='length'||k==='name'||k==='arguments'||k==='caller')continue;
                if(typeof d.get==='function'||typeof d.set==='function'){
                    if(typeof d.get==='function')members.push({name:k,kind:'get',access:'public',static:isStatic,type:'*',parameters:[]});
                    if(typeof d.set==='function')members.push({name:k,kind:'set',access:'public',static:isStatic,type:'*',parameters:[{type:'*',optional:false}]});
                }else if(typeof d.value==='function'){
                    const arity=Math.max(0,Math.min(16,Number(d.value.length)||0));
                    const parameters=Array.from({length:arity},()=>({type:'*',optional:true}));
                    members.push({name:k,kind:'method',access:'public',static:isStatic,type:'*',parameters});
                }else if(isStatic&&d.value!==undefined&&(typeof d.value==='string'||typeof d.value==='number'||typeof d.value==='boolean')){
                    const ty=typeof d.value==='string'?'String':typeof d.value==='boolean'?'Boolean':Number.isInteger(d.value)?'int':'Number';
                    members.push({name:k,kind:d.writable?'variable':'constant',access:'public',static:true,type:ty,parameters:[]});
                }
            }
        };
        scan(Ctor,true);
        if(Ctor.prototype&&Ctor.prototype!==Object.prototype&&Ctor.prototype!==Function.prototype)scan(Ctor.prototype,false);
        return {name:qname,base:null,public:true,dynamic:false,final:false,interface:Boolean(__nfBuiltins.__nfIsNativeInterface?.(qname)),interfaces:[],members,constructor:[]};
    }
    function reflect(operation,args) {
        if(args.length!==1)throw new TypeError(`${operation} expects exactly one argument in the source reflection profile.`);
        if(typeof __nfBuiltins.__nfObjectExecute!=='function')throw new TypeError('Rust Object reflection is unavailable; rebuild and re-export this project.');
        const metadata=[...definitions.values()].filter(definition=>definition.dialect==='as3').map(definition=>definition.reflection).filter(Boolean);
        metadata.push(...reflectionInterfaces.values());
        const request={op:operation,classes:metadata};
        if(operation==='getDefinitionByName') {
            if(typeof args[0]!=='string')throw new TypeError('getDefinitionByName currently requires a primitive String name.');
            const hostDefs=Object.keys(builtins).filter(k=>!k.startsWith('__nf')&&typeof builtins[k]==='function'&&Boolean(__nfBuiltins.__nfNativeClassName?.(builtins[k])));
            request.name=args[0];
            request.definitions=[...new Set([...nativeTypeNames.filter(name=>builtins[name]!==undefined),...hostDefs,'Object','Number','int','uint','Boolean','String','Array','Date','RegExp','Math','JSON','Class','Function','XML','XMLList','Namespace','QName','flash.utils.Dictionary','flash.utils.Proxy','mx.utils.ObjectProxy',...Object.keys(__nfErrorTypes)])];
        } else {
            request.value=reflectionValue(args[0]);
            if(operation==='describeType'&&request.value?.name){
                const desc=harvestNativeClassDescriptor(request.value.name);
                if(desc)request.native_classes=[desc];
            }
        }
        let result;
        try{result=__nfBuiltins.__nfObjectExecute(request);}
        catch(error){throw operation==='getDefinitionByName'?new ReferenceError(String(error?.message||error)):new TypeError(String(error?.message||error));}
        if(operation==='getDefinitionByName')return reflectionClass(result);
        if(operation==='describeType')return new __nfXML.XML(result);
        return result;
    }
    for(const name of ['getQualifiedClassName','getQualifiedSuperclassName','getDefinitionByName','describeType'])builtins['flash.utils.'+name]=(...args)=>reflect(name,args);
    function defineNamespace(name,uri) {
        const prefix=String(name).split('.').pop();
        builtins[name]=typeof __nfXML?.namespace==='function'?__nfXML.namespace(prefix,uri):Object.freeze({prefix,uri,toString(){return this.uri;},valueOf(){return this.uri;}});
    }
    function declareIntrinsic(name) { intrinsics.add(name); }
    function registerInitClip(key, fn) { initClips.push({ key, fn }); }
    function runInitClips(root) {
        for (const entry of initClips) __nfRunInitClip(entry.key, root, entry.fn);
    }
    const getterMarker = Object.freeze({ [Symbol.for('neoflash.trace.getter.v1')]: true });
    function traceObjectInfo(value) {
        if (value == null || (typeof value !== 'object' && typeof value !== 'function')) return null;
        const raw = unwrapProxy(value);
        if (classFacades.has(value) || classFacades.has(raw) || __nfBuiltins.__nfIsNativeDisplay?.(raw)) return null;
        const proxyObj = isProxy(value);
        const name = identities.get(value) || identities.get(raw) || (proxyObj && is(value, 'mx.utils.ObjectProxy') ? 'mx.utils.ObjectProxy' : null);
        if (!name) return null;
        const definition = definitions.get(name);
        if (!definition && !proxyObj) return null;
        const tsSlot = publicSlot(raw, 'toString');
        if (tsSlot && tsSlot.kind === 'method') return { className: name, customToString: true };
        const ownTs = Object.getOwnPropertyDescriptor(raw, 'toString');
        if (ownTs && Object.hasOwn(ownTs, 'value') && typeof ownTs.value === 'function' && ownTs.value !== Object.prototype.toString) {
            return { className: name, customToString: true };
        }
        if (definition?.dialect === 'as2') {
            for (const def of chain(name)) {
                if (def.methods.some(m => !m.static && m.name === 'toString')) return { className: name, customToString: true };
            }
        }
        const entries = [], seenKeys = new Set();
        if (definition?.dialect === 'as2') {
            for (const def of chain(name)) {
                for (const field of def.fields) {
                    if (field.static || seenKeys.has(field.name)) continue;
                    seenKeys.add(field.name);
                    let desc = Object.getOwnPropertyDescriptor(raw, field.name);
                    if (!desc) {
                        for (let p = Object.getPrototypeOf(raw); p && p !== Object.prototype; p = Object.getPrototypeOf(p)) {
                            desc = Object.getOwnPropertyDescriptor(p, field.name);
                            if (desc) break;
                        }
                    }
                    if (desc) entries.push([field.name, Object.hasOwn(desc, 'value') ? desc.value : getterMarker]);
                }
            }
        } else if (definition) {
            const states = fieldStates.get(raw);
            for (const def of chain(name)) {
                for (const field of def.fields) {
                    if (field.static || field.access !== 'public' || seenKeys.has(field.name)) continue;
                    seenKeys.add(field.name);
                    const state = states?.get(field.slot);
                    if (state) entries.push([field.name, state.value]);
                }
            }
            for (const slot of slots.get(raw)?.values() || []) {
                if (slot.member.access === 'public' && slot.kind === 'accessor' && slot.readable && !seenKeys.has(slot.member.name)) {
                    seenKeys.add(slot.member.name);
                    entries.push([slot.member.name, getterMarker]);
                }
            }
        }
        if (proxyObj) {
            try {
                let index = 0, guard = 0;
                while (guard++ < 4096) {
                    index = Number(invokeProxy(value, 'nextNameIndex', [index])) | 0;
                    if (index <= 0) break;
                    const k = String(invokeProxy(value, 'nextName', [index]));
                    if (seenKeys.has(k)) continue;
                    seenKeys.add(k);
                    let v;
                    try { v = invokeProxy(value, 'nextValue', [index]); }
                    catch (_) { try { v = invokeProxy(value, 'getProperty', [k]); } catch (_2) { v = undefined; } }
                    entries.push([k, v]);
                }
            } catch (_) {}
            return { className: name, customToString: false, entries };
        }
        for (const key of Object.keys(value)) {
            if (seenKeys.has(key)) continue;
            seenKeys.add(key);
            const desc = Object.getOwnPropertyDescriptor(value, key);
            if (desc) entries.push([key, Object.hasOwn(desc, 'value') ? desc.value : getterMarker]);
        }
        return { className: name, customToString: false, entries };
    }
    return Object.freeze({ type, is, cast, as2Cast, define, defineInterface, defineNamespace, declareIntrinsic, registerInitClip, runInitClips, attach, construct, superConstruct, superView, initializeFields, initializeStatics, jsonKeys, elementDescriptor, enumerationKeys, hasFixedProperty, isDynamic: dynamicAllowed, ref, assertDynamicWrite, objectDescriptor, isClass, isInterface, constructClass, className, resolveClass: reflectionClass, reflect, defaultObjectMethod, isProxy, hasTrait, invokeProxy, resolveQualifiedSlot, traceObjectInfo,
        instanceClassName:value=>identities.get(value)||null,
        setNativeTypes:names=>{nativeTypeNames=names;},
        dialect:name=>definitions.get(name)?.dialect });
})();
