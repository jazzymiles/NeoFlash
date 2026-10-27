// Original bounded AS3 JSON adapter; no ambient JSON/prototype changes.
// Contracts: https://airsdk.dev/reference/actionscript/3.0/JSON.html
// Nullable stringify result/error IDs: adobe/avmplus core/JSON.as and JSONClass.cpp.
const __nfJSONParseNative = JSON.parse, __nfJSONQuote = JSON.stringify;
const __nfJSONNativeDateJSON = Date.prototype.toJSON;
const __nfJSONKeys = Object.keys, __nfJSONApply = Reflect.apply;
const __nfJSONLimit = 1000000, __nfJSONDepth = 128, __nfJSONVisits = 100000;
function __nfJSONError(type,message,id=0){return __nfMakeError(type,true,[message,id]);}
function __nfJSONBound(text){
    if(text.length>__nfJSONLimit)throw __nfJSONError('RangeError','JSON supports at most 1000000 UTF-16 code units.');
    return text;
}
function __nfJSONParse(text,reviver=null){
    if(arguments.length<1||arguments.length>2)throw __nfJSONError('ArgumentError','JSON.parse expects text and an optional reviver.');
    if(reviver!=null&&typeof reviver!=='function')throw __nfJSONError('TypeError','JSON reviver must be a function or null.',1130);
    if(text==null)throw __nfJSONError('SyntaxError','JSON input cannot be null or undefined.',1132);
    text=__nfJSONBound(String(text));
    // Bound nesting/traversal before native parsing or recursive callbacks.
    let depth=0,quoted=false,escaped=false,items=1;
    for(const c of text){
        if(quoted){if(escaped)escaped=false;else if(c==='\\')escaped=true;else if(c==='"')quoted=false;continue;}
        if(c==='"')quoted=true;
        else if(c==='['||c==='{'){if(++depth>__nfJSONDepth)throw __nfJSONError('RangeError','JSON nesting exceeds 128 containers.');items++;}
        else if(c===']'||c==='}')depth--;
        else if(c===',')items++;
        if(items>__nfJSONVisits)throw __nfJSONError('RangeError','JSON traversal exceeds 100000 values.');
    }
    let value;
    try{value=__nfJSONParseNative(text);}catch(error){if(error instanceof SyntaxError)throw __nfJSONError('SyntaxError','Invalid JSON input.',1132);throw error;}
    if(reviver==null)return value;
    let visits=0;
    function walk(holder,key,level){
        if(++visits>__nfJSONVisits||level>__nfJSONDepth)throw __nfJSONError('RangeError','JSON reviver traversal exceeds its size/depth limit.');
        const current=holder[key];
        if(current!==null&&typeof current==='object'){
            const keys=Array.isArray(current)?Array.from({length:current.length},(_,i)=>String(i)):__nfJSONKeys(current);
            for(const child of keys){const next=walk(current,child,level+1);if(next===undefined)delete current[child];else Object.defineProperty(current,child,{value:next,writable:true,enumerable:true,configurable:true});}
        }
        return __nfJSONApply(reviver,holder,[key,current]);
    }
    return walk({'':value},'',0);
}
function __nfJSONStringify(value,replacer,space){
    if(arguments.length<1||arguments.length>3)throw __nfJSONError('ArgumentError','JSON.stringify expects a value, optional replacer and optional indentation.');
    if(arguments.length<2)replacer=null;if(arguments.length<3)space=null;
    if(replacer!==null&&typeof replacer!=='function'&&!Array.isArray(replacer))throw __nfJSONError('TypeError','JSON replacer must be a function, Array or null.',1131);
    if(space!==null&&typeof space!=='string'&&typeof space!=='number')throw __nfJSONError('TypeError','JSON indentation currently requires String, Number or null; historical conversion of other values is unsupported.');
    const gap=typeof space==='string'?space.slice(0,10):typeof space==='number'?' '.repeat(Number.isNaN(space)?0:Math.max(0,Math.min(10,Math.floor(space)))):'';
    let selected=null;
    if(Array.isArray(replacer)){
        if(replacer.length>__nfJSONVisits)throw __nfJSONError('RangeError','JSON property list exceeds 100000 entries.');
        const names=new Set();for(let i=0;i<replacer.length;i++){const name=replacer[i];if(typeof name==='string'||typeof name==='number')names.add(__nfJSONBound(String(name)));}selected=[...names];
    }
    const active=new Set(),chunks=[];let length=0,visits=0;
    function emit(text){length+=text.length;if(length>__nfJSONLimit)throw __nfJSONError('RangeError','JSON output exceeds 1000000 UTF-16 code units.');chunks.push(text);}
    function quote(text){return __nfJSONQuote(__nfJSONBound(text));}
    function prepared(holder,key){
        if(++visits>__nfJSONVisits)throw __nfJSONError('RangeError','JSON traversal exceeds 100000 values.');
        let item=holder[key];
        if(item!==null&&typeof item==='object'){
            const method=item.toJSON;
            if(typeof method==='function')item=method===__nfJSONNativeDateJSON&&__nfDateIs(item)
                ?__nfJSONApply(__nfDateJSON,item,[key]):__nfJSONApply(method,item,[key]);
        }
        if(typeof replacer==='function')item=__nfJSONApply(replacer,holder,[key,item]);
        if(item===null||typeof item!=='object')return item;
        // Primitive wrappers can arrive from JavaScript handlers. Native brand
        // calls avoid honoring unrelated Symbol.toStringTag/custom valueOf.
        for(const method of [Number.prototype.valueOf,String.prototype.valueOf,Boolean.prototype.valueOf]){
            try{return __nfJSONApply(method,item,[]);}catch(error){if(!(error instanceof TypeError))throw error;}
        }
        return item;
    }
    const omitted=item=>item===undefined||typeof item==='function'||typeof item==='symbol';
    function write(item,level){
        if(item===null){emit('null');return;}
        if(typeof item==='string'){emit(quote(item));return;}
        if(typeof item==='number'){emit(Number.isFinite(item)?String(item):'null');return;}
        if(typeof item==='boolean'){emit(String(item));return;}
        if(typeof item!=='object')throw __nfJSONError('TypeError','Unsupported JSON value type.');
        if(level>=__nfJSONDepth)throw __nfJSONError('RangeError','JSON nesting exceeds 128 containers.');
        if(active.has(item))throw __nfJSONError('TypeError','JSON cannot serialize a cyclic structure.',1129);
        const array=Array.isArray(item);
        let keys;
        if(array){if(item.length>__nfJSONVisits)throw __nfJSONError('RangeError','JSON array exceeds 100000 items.');}
        else{
            const fixed=typeof __nfProgram!=='undefined'&&typeof __nfProgram.jsonKeys==='function'?__nfProgram.jsonKeys(item):null;
            if(fixed===null){const prototype=Object.getPrototypeOf(item);if(prototype!==null&&prototype!==Object.prototype)throw __nfJSONError('TypeError','JSON encoding of this native object is unsupported; supply toJSON or a replacer.');}
            keys=selected||(fixed===null?__nfJSONKeys(item):fixed);
            if(keys.length>__nfJSONVisits)throw __nfJSONError('RangeError','JSON object exceeds 100000 properties.');
        }
        active.add(item);emit(array?'[':'{');let emitted=0;
        const count=array?item.length:keys.length;
        for(let i=0;i<count;i++){
            const key=array?String(i):keys[i],child=prepared(item,key);
            if(!array&&omitted(child))continue;
            if(emitted++)emit(',');if(gap)emit('\n'+gap.repeat(level+1));
            if(!array){emit(quote(key));emit(gap?': ':':');}
            write(omitted(child)?null:child,level+1);
        }
        if(emitted&&gap)emit('\n'+gap.repeat(level));emit(array?']':'}');active.delete(item);
    }
    const root=prepared({'':value},'');if(omitted(root))return null;
    write(root,0);return chunks.join('');
}
const __nfJSON=Object.freeze({parse:__nfJSONParse,stringify:__nfJSONStringify});
