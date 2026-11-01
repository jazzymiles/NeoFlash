// Original source-function adapter; embedded per compiled unit, no prototype patch.
// AIR Function / arguments / ...(rest) contracts and pinned Ruffle fe6cfc545405b888ebb478ce1c211d63147ca49b:
// avm2/globals/function.rs (signature length, apply Array/null), avm2/activation.rs
// (coerced normal arguments, actual count, nonenumerable callee), avm1/globals/function.rs.
// https://airsdk.dev/reference/actionscript/3.0/Function.html
// https://airsdk.dev/reference/actionscript/3.0/arguments.html
// https://open-flash.github.io/mirrors/as2-language-reference/Function.html
// This profile limits a call to 16384 values. AS2 arbitrary array-like apply,
// primitive receiver boxing, caller stacks, native call/apply extraction and
// dynamic Function construction remain explicit boundaries. Null/undefined this
// uses the source frame when supplied by the emitter, never browser globalThis.
const __nfSourceFunctionBrand=Symbol.for('neoflash.source.function.v1');
const __nfNativeFunctionConstructor=Function;
const __nfNativeFunctionCall=Function.prototype.call;
const __nfNativeFunctionApply=Function.prototype.apply;
const __nfNativeFunctionBind=Function.prototype.bind;
const __nfFunctionArrayIsArray=Array.isArray;
const __nfFunctionSlice=Array.prototype.slice;
const __nfFunctionLimit=16384;
const __nfLocalCallStack=[];
function __nfCallerStack(){
    return typeof __nfLegacy!=='undefined'&&__nfFunctionArrayIsArray(__nfLegacy?.__nfCallStack)?__nfLegacy.__nfCallStack:__nfLocalCallStack;
}
function __nfFunctionArity(message){throw __nfMakeError('ArgumentError',true,[message]);}
function __nfFunctionCount(length){
    if(!Number.isSafeInteger(length)||length<0||length>__nfFunctionLimit)throw new __nfTypeError('Function argument count exceeds the 16384-value profile.');
    return length;
}
function __nfFunctionRecord(value){
    if(typeof value!=='function')return null;
    const descriptor=__nfDescriptor(value,__nfSourceFunctionBrand);
    return descriptor&&Object.hasOwn(descriptor,'value')?descriptor.value:null;
}
function __nfSourceFunction(body,configuration){
    if(typeof body!=='function'||!configuration)throw new __nfTypeError('Invalid source function definition.');
    const length=__nfFunctionCount(configuration.length),required=__nfFunctionCount(configuration.required);
    if(required>length)throw new __nfTypeError('Source function required count exceeds its declared parameters.');
    const as3=configuration.as3===true,allowExtra=configuration.allowExtra===true,defaultReceiver=configuration.defaultReceiver;
    const bound=new WeakMap();
    function make(receiver,isBound){
        const callable=function(){
            'use strict';
            if(new.target)throw new __nfTypeError('Constructing a source function is not supported; use a declared project class.');
            __nfFunctionCount(arguments.length);
            const values=__nfApply(__nfFunctionSlice,arguments,[]);
            if(as3&&(values.length<required||(!allowExtra&&values.length>length))){
                __nfFunctionArity(`AS3 function expects ${required===length?String(length):`${required} through ${length}`} arguments${allowExtra?' or more':''}; received ${values.length}.`);
            }
            const actual=isBound?receiver:this==null?defaultReceiver:this;
            if(actual==null)throw new __nfTypeError('This unbound source function has no source-frame receiver. Supply an explicit object to call/apply.');
            if(typeof actual!=='object'&&typeof actual!=='function')throw new __nfTypeError('Source Function.call/apply currently requires an object receiver; primitive receiver boxing is not supported.');
            const stack=__nfCallerStack(),caller=stack.length?stack[stack.length-1]:null;
            if(stack.length>=1024)throw new RangeError('Source function call stack exceeds 1024 frames.');
            const selfFn=!as3&&callable.__nfPublicFunction?callable.__nfPublicFunction:callable;
            stack.push(selfFn);
            try{return __nfApply(body,actual,[{values,callee:selfFn,caller,as3}]);}
            finally{stack.pop();}
        };
        __nfDefineProperty(callable,'length',{value:length,writable:false,configurable:false});
        __nfDefineProperty(callable,__nfSourceFunctionBrand,{value:Object.freeze({
            bound:isBound,
            bind(target){
                if(isBound)return callable;
                if(target===null||(typeof target!=='object'&&typeof target!=='function'))throw new __nfTypeError('A bound source method requires an object receiver.');
                if(!bound.has(target))bound.set(target,make(target,true));
                return bound.get(target);
            },
        })});
        return callable;
    }
    return make(undefined,false);
}
function __nfBindFunction(callable,receiver){
    if(typeof callable!=='function')throw new __nfTypeError('Only a Function can be bound as a method.');
    const record=__nfFunctionRecord(callable);
    return record?record.bind(receiver):__nfApply(__nfNativeFunctionBind,callable,[receiver]);
}
function __nfFunctionArguments(activation,normalizedPrefix=[]){
    if(!activation||!__nfFunctionArrayIsArray(activation.values)||typeof activation.callee!=='function'||!__nfFunctionArrayIsArray(normalizedPrefix))throw new __nfTypeError('Invalid source function activation.');
    const length=__nfFunctionCount(activation.values.length),result=new Array(length);
    for(let i=0;i<length;i++)result[i]=i<normalizedPrefix.length?normalizedPrefix[i]:activation.values[i];
    __nfDefineProperty(result,'callee',{value:activation.callee,writable:true,configurable:true,enumerable:false});
    if(activation.as3){
        __nfDefineProperty(result,'caller',{enumerable:false,configurable:false,get(){throw new __nfTypeError('arguments.caller is not supported. Pass the calling function explicitly.');}});
    }else{
        __nfDefineProperty(result,'caller',{value:activation.caller??null,writable:true,configurable:true,enumerable:false});
    }
    return result;
}
function __nfFunctionMethod(receiver,key,as3,resolvedMethod){
    if(receiver==null)throw new __nfTypeError('Cannot call a method on null or undefined.');
    // Resolve before arguments and never look up the property again. Authored
    // call/apply methods (including getters) keep ordinary receiver semantics.
    const method=arguments.length>=4?resolvedMethod:receiver[key];
    if(method!==__nfNativeFunctionCall&&method!==__nfNativeFunctionApply)return (...args)=>__nfApply(method,receiver,args);
    return (...args)=>{
        if(typeof receiver!=='function')throw new __nfTypeError('Function.call/apply requires a Function receiver.');
        if(receiver===__nfNativeFunctionConstructor)throw new __nfTypeError('Dynamic Function construction is not supported; declare a source function instead.');
        if(method===__nfNativeFunctionCall){__nfFunctionCount(Math.max(0,args.length-1));return __nfApply(receiver,args[0],args.slice(1));}
        if(as3&&args.length>2)__nfFunctionArity('AS3 Function.apply expects at most two arguments.');
        const source=args[1];
        if(source==null)return __nfApply(receiver,args[0],[]);
        if(!__nfFunctionArrayIsArray(source))throw new __nfTypeError('Function.apply requires an Array, source arguments array, null or undefined; arbitrary array-like objects are not supported.');
        const length=__nfFunctionCount(source.length),values=new Array(length);
        // Read holes/inherited indexed properties in order, after all call-site
        // expressions, matching the native apply boundary rather than spreading
        // a user-overridden iterator.
        for(let i=0;i<length;i++)values[i]=source[i];
        return __nfApply(receiver,args[0],values);
    };
}
function __nfFunctionMethodValue(value){
    if(value===__nfNativeFunctionCall||value===__nfNativeFunctionApply)throw new __nfTypeError('Extracted native Function.call/apply aliases are not supported; invoke the method directly on the function.');
    return value;
}
