// Original text-processing scenarios. Expectations are documented_only:
// no original Flash Player execution or bytecode differential is claimed.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const stringsProfile = {
  status: 'documented_only',
  sources: [
    'https://open-flash.github.io/mirrors/as2-language-reference/String.html',
    'https://airsdk.dev/reference/actionscript/3.0/String.html',
    'https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm1/globals/string.rs',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/String.as',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/StringClass.cpp',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/StringObject.cpp',
  ],
  boundary: 'Primitive UTF-16 strings, at most 1000000 code units/results. AS2 follows the named AVM1 implementation for later-SWF defaults and integer positions, not an original-player oracle. No AS2 boxed String, native method extraction, prototype augmentation, RegExp, locale methods or custom pattern objects. Unicode casing and general object conversion retain the JavaScript-backed profile, not historical Flash tables/complete AVM coercion. AS2 extra arguments are evaluated then ignored; AS3 fixed-arity methods diagnose excess arguments because their arity behavior is not verified. This is not a claimed Flash ArgumentError. Constructor arguments after the first are ignored as in the pinned implementation.',
  distinctions: 'AS2 fromCharCode stops at zero; missing search/slice/substr/substring arguments return undefined. AS3 nullable typed search arguments do not match, explicit undefined numeric endpoints convert to zero, and string-pattern replace inserts replacement text literally after converting it before the pattern. AS3 split follows StringClass.cpp: undefined delimiter becomes the text undefined; an empty subject gives one empty result when limit is nonzero. These pinned VM details are more specific than the reference summaries. AS3 new String returns a primitive; constructor aliases under new are not supported.',
};

const commonSource = `var raw = "title:neo|level:3|tag:flash";
var rows = raw.split("|");
var output = "";
for (var i = 0; i < rows.length; i++) {
    var separator = rows[i].indexOf(":");
    var key = rows[i].substring(0, separator).toUpperCase();
    var value = rows[i].slice(separator + 1);
    if (i > 0) output = output.concat(";");
    output = output.concat(key, "=", value);
}
result = output;
var suffix = raw.substr(-5);
var last = raw.lastIndexOf("|");
var heading = raw.charAt(0).toUpperCase().concat(raw.substring(1,5));
var firstCode = heading.charCodeAt(0);
var lower = heading.toLowerCase();
var swapped = "abcdef".substring(4,1);
var wrapped = "abcdef".substr(1,-2);
var negativeSearch = "abc".lastIndexOf("a",-1);
var omitted = raw.slice();
var explicitEnd = "abcd".slice(1,undefined);
var nullSearch = "null".indexOf(null);
var generated = String.fromCharCode(65,0,66);
var pair = String.fromCharCode(55357,56832);
var units = pair.length;
var firstUnit = pair.charCodeAt(0);
var wrappedIndex = "abc".charAt(4294967296);
var defaultParts = "undefinedxundefined".split();
var emptyParts = "".split("");
var aliases = String;
var converted = aliases(17);
var stringify = "x".toString();
var unbox = "x".valueOf();
trace("TEXT", result, suffix, heading, firstCode, lower);`;

export const stringSources = {
  as2: commonSource,
  as3: commonSource + `
var constructed:String = new String("primitive");
var literalReplacement = "red red".replace("red","$&");
var callbackReplacement = "red red".replace("red", function(match:String, index:int, source:String):String {
    return match.toUpperCase() + ":" + index + ":" + source.length;
});
var untouched = "blue".replace("red", function(match:String, index:int, source:String):String { throw new Error("unmatched callback ran"); });
trace("REPLACE", literalReplacement, callbackReplacement, untouched);`,
};

export const stringsFixtures = ['as2','as3'].map(language => ({
  language: language === 'as3' ? 'action_script3' : 'action_script2',
  source: stringSources[language], profile: stringsProfile, context: {},
  verify(c, logs) {
    assert.equal(c.result,'TITLE=neo;LEVEL=3;TAG=flash');
    assert.equal(c.suffix,'flash'); assert.equal(c.last,17); assert.equal(c.heading,'Title');
    assert.equal(c.firstCode,84); assert.equal(c.lower,'title'); assert.equal(c.swapped,'bcd');
    assert.equal(c.wrapped,'bcde'); assert.equal(c.negativeSearch,-1);
    assert.equal(c.units,2); assert.equal(c.firstUnit,55357); assert.equal(c.converted,'17');
    assert.equal(c.stringify,'x'); assert.equal(c.unbox,'x');
    const expected=[['TEXT',c.result,'flash','Title',84,'title']];
    if(language === 'as3') {
      assert.equal(c.omitted,c.raw); assert.equal(c.explicitEnd,''); assert.equal(c.nullSearch,-1);
      assert.equal(c.generated,'A\0B'); assert.equal(c.wrappedIndex,'');
      assert.deepEqual(c.defaultParts,['','x','']); assert.deepEqual(c.emptyParts,['']);
      assert.equal(c.constructed,'primitive'); assert.equal(typeof c.constructed,'string');
      assert.equal(c.literalReplacement,'$& red'); assert.equal(c.callbackReplacement,'RED:0:7 red');
      assert.equal(c.untouched,'blue'); expected.push(['REPLACE','$& red','RED:0:7 red','blue']);
    } else {
      assert.equal(c.omitted,undefined); assert.equal(c.explicitEnd,'bcd'); assert.equal(c.nullSearch,0);
      assert.equal(c.generated,'A'); assert.equal(c.wrappedIndex,'a');
      assert.deepEqual(c.defaultParts,['undefinedxundefined']); assert.deepEqual(c.emptyParts,[]);
    }
    assert.deepEqual(logs,expected);
  },
}));

// One family smoke exercises the helper ABI while the compiler owner integrates
// the same two source scenarios into the shared emitted-code executor.
export function verifyStringsAdapter() {
  const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
  const code=readFileSync(resolve(root,'crates/neoflash-script/src/actionscript/strings.js'),'utf8');
  const api=new Function('__nfTypeError',code+'\nreturn {method:__nfStringMethod,create:__nfStringCreate,type:__nfStringClass,value:__nfStringMethodValue};')(TypeError);
  const originalLower=String.prototype.toLowerCase;
  for(const as3 of [false,true]) {
    const call=(text,name,...args)=>api.method(text,name,as3)(...args);
    const S=api.type(as3), raw='title:neo|level:3|tag:flash';
    const rows=call(raw,'split','|'); let output='';
    for(const row of rows) {
      const split=call(row,'indexOf',':');
      output=call(output,'concat',output ? ';' : '',call(call(row,'substring',0,split),'toUpperCase'),'=',call(row,'slice',split+1));
    }
    assert.equal(output,'TITLE=neo;LEVEL=3;TAG=flash');
    assert.equal(call(raw,'substr',-5),'flash'); assert.equal(call(raw,'lastIndexOf','|'),17);
    assert.equal(call('abcdef','substr',1,-2),'bcde'); assert.equal(call('abc','lastIndexOf','a',-1),-1);
    assert.equal(call('abc','charAt',4294967296),as3?'':'a');
    assert.equal(call('abcd','slice',1,undefined),as3?'':'bcd');
    assert.equal(call(raw,'slice'),as3?raw:undefined); assert.equal(call('null','indexOf',null),as3?-1:0);
    assert.deepEqual(call('undefinedxundefined','split'),as3?['','x','']:['undefinedxundefined']);
    assert.deepEqual(call('','split',''),as3?['']:[]);
    assert.equal(S.fromCharCode(65,0,66),as3?'A\0B':'A'); assert.equal(S.fromCharCode(55357,56832).length,2);
    assert.equal(api.type(as3),S); assert.equal(S(), ''); assert.equal(S(null),'null'); assert.equal(S(undefined),'undefined');
    assert.equal(S('x','ignored'),'x'); assert.equal(S.__nfIs('x'),true);
    const effects=[];
    const custom={}; Object.defineProperty(custom,'slice',{get(){effects.push('lookup'); return function(arg){assert.equal(this,custom);effects.push('call:'+arg);return 7;};}});
    const selected=api.method(custom,'slice',as3); effects.push('argument'); assert.equal(selected(3),7);
    assert.deepEqual(effects,['lookup','argument','call:3']);
    const first={valueOf(){effects.push('start');return 1;}},second={valueOf(){effects.push('end');return 3;}};
    assert.equal(call('abcd','slice',first,second),'bc'); assert.deepEqual(effects.slice(-2),['start','end']);
    assert.throws(()=>api.value(String.prototype.slice),/native method extraction/);
    const userMethod=()=>9; assert.equal(api.value(userMethod),userMethod);
    assert.throws(()=>call('abc','localeCompare','b'),/outside the supported/);
    assert.throws(()=>call('abc','split',/b/),/RegExp/);
    if(as3) {
      assert.equal(api.create(true,['primitive'],true),'primitive');
      assert.equal(call('red red','replace','red','$&'),'$& red');
      assert.equal(call('red red','replace','red',(match,index,source)=>match.toUpperCase()+':'+index+':'+source.length),'RED:0:7 red');
      const conversion=[];
      assert.equal(call('red','replace','red',{toString(){conversion.push('replacement');return '$1';}}),'$1');
      assert.deepEqual(conversion,['replacement']);
      assert.equal(call('blue','replace','red',()=>{throw new Error('unmatched callback');}),'blue');
      assert.throws(()=>call('red','replace',/red/,'x'),/RegExp/);
      assert.deepEqual(call('a,b','split',',',null),['a','b']);
      assert.throws(()=>call('abc','charAt',0,1),/excess-argument/);
    } else {
      assert.throws(()=>api.create(false,['box'],true),/boxed String/);
      assert.throws(()=>call('red','replace','red','x'),/not an AS2 native/);
    }
    assert.throws(()=>call('x'.repeat(1000000),'concat','y'),/supported string limit/);
  }
  assert.equal(String.prototype.toLowerCase,originalLower);
  console.log('String adapter: one family smoke with AS2/AS3 text-processing pipelines passed; compiler/export integration is separate.');
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))verifyStringsAdapter();
