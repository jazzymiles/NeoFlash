// Original AS3 RegExp facade. Primary contracts: AIR RegExp/String references;
// constructor/clone behavior cross-checked against Adobe avmplus RegExpClass.cpp
// at 858d034a3bd3a54d9b70909386435cf4aec81d21. No upstream code is copied.
// https://airsdk.dev/reference/actionscript/3.0/RegExp.html
// https://airsdk.dev/reference/actionscript/3.0/String.html
// https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/RegExpClass.cpp
// The engine is native JavaScript, not PCRE: Unicode tables, backtracking and
// zero-width split details follow this backend. g/i/m/s plus ASCII extended x
// are supported. PCRE-only syntax and unknown flags diagnose; AS3 historically
// ignored unknown flags. There is no hard CPU-time bound on a native match.
// Size limits bound retained input/output, not regex execution complexity.
const __nfRegExpNative = RegExp;
const __nfRegExpNativeExec = RegExp.prototype.exec;
const __nfRegExpApply = Reflect.apply;
const __nfRegExpBrand = Symbol.for('neoflash.as3.regexp.v1');
const __nfRegExpDescriptor = Object.getOwnPropertyDescriptor;
const __nfRegExpLimit = 1000000;
let __nfRegExpFacadeClass;
function __nfRegExpUnsupported(detail) { throw new __nfTypeError('Unsupported AS3 RegExp: ' + detail); }
function __nfRegExpIs(value) {
    if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return false;
    const descriptor = __nfRegExpDescriptor(value, __nfRegExpBrand);
    return !!descriptor && Object.hasOwn(descriptor, 'value') && descriptor.value != null && descriptor.value.version === 1;
}
function __nfRegExpState(value) {
    if (!__nfRegExpIs(value)) throw new __nfTypeError('Expected an AS3 RegExp.');
    return __nfRegExpDescriptor(value, __nfRegExpBrand).value;
}
function __nfRegExpText(value, name, limit = __nfRegExpLimit) {
    const text = __nfString(value);
    if (text.length > limit) __nfRegExpUnsupported(name + ' exceeds ' + limit + ' UTF-16 code units.');
    return text;
}
function __nfRegExpPattern(source, extended) {
    let result = '', inClass = false;
    for (let index = 0; index < source.length; index++) {
        const character = source[index];
        if (character === '\\') {
            const next = source[++index];
            if (next === undefined) { result += '\\'; break; }
            if (/[aAeEgGhHkKNoOpPQRXzZ]/.test(next) || ((next === 'x' || next === 'u') && source[index + 1] === '{')) {
                __nfRegExpUnsupported('PCRE-only escape \\' + next + ' is not available in the JavaScript pattern backend.');
            }
            result += '\\' + next; continue;
        }
        if (!inClass && extended && /[\t\n\r\f\v ]/.test(character)) continue;
        if (!inClass && extended && character === '#') { while (index + 1 < source.length && !/[\n\r]/.test(source[index + 1])) index++; continue; }
        if (!inClass && character === '(' && source[index + 1] === '?') {
            const next = source[index + 2];
            const lookbehind = next === '<' && /[=!]/.test(source[index + 3] || '');
            if (![':', '=', '!'].includes(next) && !lookbehind) __nfRegExpUnsupported('named groups, inline flags, conditionals and PCRE group extensions are pending.');
        }
        if (!inClass && ((character === '*' || character === '+' || character === '?' || character === '}') && source[index + 1] === '+')) __nfRegExpUnsupported('possessive quantifiers are not supported by the JavaScript backend.');
        if (character === '[' && !inClass) inClass = true;
        else if (character === ']' && inClass) inClass = false;
        result += character;
    }
    return result;
}
function __nfRegExpEngine(state, global) {
    return new __nfRegExpNative(state.compiled, state.nativeFlags.replace(/g/g, '') + (global ? 'g' : ''));
}
function __nfRegExpExec(value, input) {
    const state = __nfRegExpState(value), text = __nfRegExpText(input == null ? 'null' : input, 'input');
    if (state.global && (state.lastIndex < 0 || state.lastIndex > text.length)) { state.lastIndex = 0; return null; }
    state.engine.lastIndex = state.global ? state.lastIndex : 0;
    const result = __nfRegExpApply(__nfRegExpNativeExec, state.engine, [text]);
    if (state.global) state.lastIndex = state.engine.lastIndex;
    return result;
}
function __nfRegExpCreate(as3, args) {
    if (!as3) __nfRegExpUnsupported('RegExp is not an AS2 built-in.');
    if (args.length > 2) __nfRegExpUnsupported('constructor expects a pattern and optional flags.');
    let source, flags;
    if (__nfRegExpIs(args[0])) {
        if (args[1] !== undefined) throw new __nfTypeError('Cloning RegExp does not accept replacement flags.');
        const previous = __nfRegExpState(args[0]); source = previous.source; flags = previous.flags;
    } else {
        source = args[0] === undefined ? '' : __nfRegExpText(args[0], 'pattern', 65536);
        flags = args[1] === undefined ? '' : __nfRegExpText(args[1], 'flags', 64);
    }
    if (/[^gimsx]/.test(flags)) __nfRegExpUnsupported('flags must contain only g, i, m, s and x; unsupported flags are not silently ignored.');
    flags = [...new Set(flags)].join('');
    const compiled = __nfRegExpPattern(source, flags.includes('x')), nativeFlags = flags.replace(/x/g, '');
    let engine;
    try { engine = new __nfRegExpNative(compiled, nativeFlags); }
    catch (error) { throw new __nfErrorTypes.SyntaxError('Invalid or unsupported AS3 RegExp pattern: ' + error.message); }
    const state = {version:1,source,flags,compiled,nativeFlags,engine,global:flags.includes('g'),lastIndex:0};
    const value = Object.create(__nfRegExpClass(true).prototype);
    Object.defineProperty(value, __nfRegExpBrand, {value:state});
    Object.defineProperties(value, {
        source:{get:()=>state.source},global:{get:()=>state.global},ignoreCase:{get:()=>flags.includes('i')},
        multiline:{get:()=>flags.includes('m')},dotall:{get:()=>flags.includes('s')},extended:{get:()=>flags.includes('x')},
        lastIndex:{get:()=>state.lastIndex,set:next=>{state.lastIndex=__nfNumber(next);}},
        exec:{value:input=>__nfRegExpExec(value,input),writable:true,configurable:true},
        test:{value:input=>__nfRegExpExec(value,input)!==null,writable:true,configurable:true},
        toString:{value:()=>'/'+state.source+'/'+state.flags,writable:true,configurable:true},
    });
    return value;
}
function __nfRegExpClass(as3) {
    if (!as3) __nfRegExpUnsupported('RegExp is not an AS2 built-in.');
    if (!__nfRegExpFacadeClass) {
        const facade = function(...args) {
            if (!new.target && __nfRegExpIs(args[0]) && args[1] === undefined) return args[0];
            return __nfRegExpCreate(true,args);
        };
        Object.defineProperties(facade, {__nfIs:{value:__nfRegExpIs},[Symbol.hasInstance]:{value:__nfRegExpIs},
            __nfInitialize:{value:()=>__nfRegExpUnsupported('native subclass initialization is pending.')}});
        __nfRegExpFacadeClass = facade;
    }
    return __nfRegExpFacadeClass;
}
function __nfRegExpForString(pattern) {
    return __nfRegExpIs(pattern) ? pattern : __nfRegExpCreate(true,[pattern]);
}
function __nfRegExpMatch(text, pattern) {
    if (pattern === undefined) return null;
    const value = __nfRegExpForString(pattern), state = __nfRegExpState(value), engine = __nfRegExpEngine(state,state.global);
    if (!state.global) return __nfRegExpApply(__nfRegExpNativeExec,engine,[text]);
    const result=[];let total=0;
    try {
        for (;;) {
            const match=__nfRegExpApply(__nfRegExpNativeExec,engine,[text]);if(!match)break;
            total+=match[0].length;if(result.length>=__nfRegExpLimit||total>__nfRegExpLimit)__nfRegExpUnsupported('match output exceeds the supported collection/string size.');
            result.push(match[0]);if(!match[0].length)engine.lastIndex=match.index+1;
        }
        return result;
    } finally { state.lastIndex=0; }
}
function __nfRegExpSearch(text, pattern) {
    if (pattern === undefined) return -1;
    const state=__nfRegExpState(__nfRegExpForString(pattern)),match=__nfRegExpApply(__nfRegExpNativeExec,__nfRegExpEngine(state,false),[text]);
    return match?match.index:-1;
}
function __nfRegExpReplacement(template, match, text) {
    // Bound expansion before concatenation: repeated $'/$` may be much larger
    // than the template itself, even for a single match.
    let result='',cursor=0;
    const tokens=/\$(\$|&|`|'|\d{1,2})/g;
    const append=part=>{if(result.length+part.length>__nfRegExpLimit)__nfRegExpUnsupported('replace output exceeds 1000000 UTF-16 code units.');result+=part;};
    for(;;){
        const found=__nfRegExpApply(__nfRegExpNativeExec,tokens,[template]);if(!found)break;
        append(template.slice(cursor,found.index));cursor=found.index+found[0].length;
        const code=found[1];let part=found[0];
        if(code==='$')part='$';else if(code==='&')part=match[0];
        else if(code==='`')part=text.slice(0,match.index);else if(code==="'")part=text.slice(match.index+match[0].length);
        else {
            const index=Number(code),first=Number(code[0]);
            if(index>0&&index<match.length)part=match[index]===undefined?'':match[index];
            else if(code.length===2&&first>0&&first<match.length)part=(match[first]===undefined?'':match[first])+code[1];
        }
        append(part);
    }
    append(template.slice(cursor));return result;
}
function __nfRegExpReplace(text, value, replacement) {
    const state=__nfRegExpState(value),engine=__nfRegExpEngine(state,state.global),callback=typeof replacement==='function';
    const template=callback?null:__nfRegExpText(replacement,'replacement');
    let result='',cursor=0,count=0;
    for (;;) {
        const match=__nfRegExpApply(__nfRegExpNativeExec,engine,[text]);if(!match)break;
        if(++count>__nfRegExpLimit)__nfRegExpUnsupported('replace match count exceeds the supported collection size.');
        const inserted=callback?__nfRegExpText(__nfRegExpApply(replacement,undefined,[...match,match.index,text]),'replacement callback result'):__nfRegExpReplacement(template,match,text);
        if(result.length+match.index-cursor+inserted.length>__nfRegExpLimit)__nfRegExpUnsupported('replace output exceeds 1000000 UTF-16 code units.');
        result+=text.slice(cursor,match.index)+inserted;cursor=match.index+match[0].length;
        if(!state.global)break;if(!match[0].length)engine.lastIndex=match.index+1;
    }
    if(result.length+text.length-cursor>__nfRegExpLimit)__nfRegExpUnsupported('replace output exceeds 1000000 UTF-16 code units.');
    return result+text.slice(cursor);
}
function __nfRegExpSplit(text,value,limit) {
    if(limit===0)return [];if(!text.length)return [''];
    const state=__nfRegExpState(value),engine=__nfRegExpEngine(state,true),result=[];let cursor=0,total=0;
    const push=value=>{total+=value===undefined?0:value.length;if(result.length>=__nfRegExpLimit||total>__nfRegExpLimit)__nfRegExpUnsupported('split output exceeds the supported collection/string size.');result.push(value);return result.length>=limit;};
    for (;;) {
        const match=__nfRegExpApply(__nfRegExpNativeExec,engine,[text]);if(!match)break;
        const end=match.index+match[0].length;
        if(!match[0].length&&(end===cursor||end===text.length)){engine.lastIndex=match.index+1;continue;}
        if(push(text.slice(cursor,match.index)))return result;
        for(let index=1;index<match.length;index++)if(push(match[index]))return result;
        cursor=end;if(!match[0].length)engine.lastIndex=match.index+1;
    }
    push(text.slice(cursor));return result;
}
