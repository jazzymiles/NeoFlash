// Original text-processing scenarios. Expectations are documented_only:
// no original Flash Player execution or bytecode differential is claimed.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createStringServices } from './rust-runtime-host.mjs';

export const stringsProfile = {
  status: 'documented_only',
  sources: [
    'https://open-flash.github.io/mirrors/as2-language-reference/String.html',
    'https://airsdk.dev/reference/actionscript/3.0/String.html',
    'https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/avm1/globals/string.rs',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/String.as',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/StringClass.cpp',
    'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/StringObject.cpp',
  ],
  boundary: 'Primitive UTF-16 strings, at most 1000000 code units/results. AS2 follows the named AVM1 implementation for later-SWF defaults and integer positions, not an original-player oracle. AS3 RegExp construction and literals support matching/search/replacement/split through a bounded JavaScript-engine facade; AS2 native RegExp, PCRE-only syntax and custom pattern objects remain unsupported. No AS2 boxed String, native method extraction or prototype augmentation. AS3 localeCompare uses the SWF11+ UTF-16 comparison profile; locale case methods share the historical case maps, not OS locale data. AS2 String algorithms and historical SWF casing tables run in the shared Rust core. AS3 primitive string operations, literal replacement, URI codecs and historical casing now run in Rust; browser Date parsing/timezone, regex operations and general object conversion retain the existing JavaScript-backed profile; complete AVM coercion is not claimed. AS2 extra arguments are evaluated then ignored; AS3 fixed-arity methods diagnose excess arguments because their arity behavior is not verified. This is not a claimed Flash ArgumentError. Constructor arguments after the first are ignored as in the pinned implementation.',
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
var moment = new Date(Date.UTC(2024,1,28,12,30,0));
moment.setUTCDate(moment.getUTCDate()+1);
var leapDay = moment.getUTCDate();
var epoch = moment.getTime();
var dateRoundtrip = Date.parse(moment.toUTCString());
function dateShadow(Date:Function):String { return Date(7); }
var localDate = dateShadow(function(value:Number):String { return "local:"+value; });
trace("TEXT", result, suffix, heading, firstCode, lower);`;

export const stringSources = {
  as2: commonSource + `
var legacyCharacters = chr(65) + mbchr(233);
var legacyCodes = ord("é") + mbord("\\ud800");
var legacyLengths = length("😀") + mblength("neo");
var legacySlices = substring("abcdef", 2, 3) + ":" + mbsubstring("abcdef", 3, -1);
var substringAlias = substring;
var legacyAliasResult = substringAlias("abcd", 2, 2);
function customLength(length:Function):Number { return length("shadow"); }
var shadowedLength = customLength(function(value:String):Number { return 17; });
var escapedRust = escape("A @é+");
var unescapedRust = unescape("A+%40%C3%A9%2B");
var historicalUpper = "straße".toUpperCase();
var historicalLower = "İ".toLowerCase();
var surrogateUnit = "\\ud800".charCodeAt(0);
var escapeAlias = escape;
var escapedAlias = escapeAlias("a+b");
function customEscape(escape:Function):String { return escape("local"); }
var shadowedEscape = customEscape(function(value:String):String { return "user:" + value; });
`,
  as3: commonSource + `
var constructed:String = new String("primitive");
var localeUpper:String="straße".toLocaleUpperCase();var localeLower:String="İ".toLocaleLowerCase();
var localeOrder:int="abc".localeCompare("abd");
var escapedLegacy:String=escape("é €😀");var unescapedLegacy:String=unescape(escapedLegacy);
var legacySurrogate:int=unescape("%uD800").charCodeAt(0);
var xmlNames:Array=[isXMLName("élève_2"),isXMLName("a:b"),isXMLName(null),isXMLName("2name")];
var literalReplacement = "red red".replace("red","$&");
var callbackReplacement = "red red".replace("red", function(match:String, index:int, source:String):String {
    return match.toUpperCase() + ":" + index + ":" + source.length;
});
var untouched = "blue".replace("red", function(match:String, index:int, source:String):String { throw new Error("unmatched callback ran"); });
var absentDate:Date; var absentPattern:RegExp;
var pattern:RegExp = new RegExp("([a-z]+):([0-9]+)","g");
pattern.lastIndex = 4;
var regexMatches = "level:3 score:12".match(pattern);
var resetIndex = pattern.lastIndex;
var transformed = "level:3 score:12".replace(pattern,"$1=$2");
var searched = "level:3 score:12".search(new RegExp("score"));
var captured = "left, right; end".split(new RegExp("[,;] *"));
var callback = "a:2".replace(pattern,function(whole:String,key:String,value:String,index:int,subject:String):String {return key.toUpperCase()+":"+(Number(value)+index+subject.length);});
var alias = RegExp; var samePattern = alias(pattern);
var clone:RegExp = new RegExp(pattern); var cloneIndex = clone.lastIndex;
var typedDate:Date = moment; var typeReads:int = 0;
function chooseDate():Object {typeReads++; return typedDate;}
var dateIs:Boolean = chooseDate() is Date;
var dateAs:Date = chooseDate() as Date;
var wrongAs:RegExp = moment as RegExp;
var regexIs:Boolean = pattern is RegExp;
var dateRejected:Boolean=false; try {typedDate={};} catch(error:TypeError) {dateRejected=true;}
var regexRejected:Boolean=false; try {pattern={};} catch(error:TypeError) {regexRejected=true;}
var report = JSON.parse('{"value":3,"omit":1}',function(key:String,value:*):* {if(key=="omit")return undefined;if(key=="value")return value+1;return value;});
var jsonText = JSON.stringify(report,function(key:String,value:*):* {if(key=="value")return value*2;return value;});
var jsonRoot = JSON.stringify(undefined);
var dateJSON = JSON.parse(JSON.stringify(moment));
function localJSON(JSON:Object):String {return JSON.stringify(5);}
var shadowJSON=localJSON({stringify:function(value:int):String{return "user:"+value;}});
trace("REPLACE", literalReplacement, callbackReplacement, untouched);`,
};

// Parsing literals reuses the same RegExp adapter and string operations.
stringSources.as3 += String.raw`
var literalPattern:RegExp=/([a-z]+):(\d+)/g;
var literalMatches="level:3 score:12".match(literalPattern);
var literalSlash=/[/]/.test("/");var literalEscape=/a\/b/.test("a/b");var literalBraces=/[{}]/.test("}");
var literalEquals=/=/.test("=");var divisionAfterLiteral=24/3/2;
var extendedLiteral=/ a + b /x.test("aaab");var dotallLiteral=/a.b/s.test("a\nb");
function freshLiteral():RegExp {return /a/g;}
var firstLiteral=freshLiteral(),secondLiteral=freshLiteral();firstLiteral.test("a");
var freshLiteralIndex=secondLiteral.lastIndex;
function shadowRegexLiteral(RegExp:Function):Boolean {return /z/.test("z");}
var lexicalRegex=shadowRegexLiteral(function(){return null;});
var uriSource="https://example.test/a b?q=α&x=1";
var uriEncoded=encodeURI(uriSource);var uriDecoded=decodeURI(uriEncoded);
var componentEncoded=encodeURIComponent("a/b α");var componentDecoded=decodeURIComponent(componentEncoded);
var reservedDecoded=decodeURI("%2F%23");var uriDefaults=[encodeURI(),encodeURI(undefined),encodeURIComponent(null)];
var uriAlias:Function=encodeURIComponent;var uriAliased=uriAlias("a b");
function uriShadow(encodeURI:Function):String{return encodeURI("value");}
var uriShadowed=uriShadow(function(value:String):String{return "local:"+value;});
var uriError=false;try{decodeURIComponent("%E0%A4");}catch(e:URIError){uriError=true;}
`;

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
    assert.equal(c.leapDay,29); assert.equal(c.epoch,Date.UTC(2024,1,29,12,30));
    assert.equal(c.dateRoundtrip,c.epoch); assert.equal(c.localDate,'local:7');
    const expected=[['TEXT',c.result,'flash','Title',84,'title']];
    if(language === 'as3') {
      assert.equal(c.omitted,c.raw); assert.equal(c.explicitEnd,''); assert.equal(c.nullSearch,-1);
      assert.equal(c.generated,'A\0B'); assert.equal(c.wrappedIndex,'');
      assert.deepEqual(c.defaultParts,['','x','']); assert.deepEqual(c.emptyParts,['']);
      assert.equal(c.constructed,'primitive'); assert.equal(typeof c.constructed,'string');
      assert.equal(c.localeUpper,'STRAßE');assert.equal(c.localeLower,'i');assert.equal(c.localeOrder,-1);
      assert.equal(c.escapedLegacy,'%E9%20%u20AC%uD83D%uDE00');assert.equal(c.unescapedLegacy,'é €😀');assert.equal(c.legacySurrogate,0xd800);
      assert.deepEqual(c.xmlNames,[true,false,false,false]);
      assert.equal(c.literalReplacement,'$& red'); assert.equal(c.callbackReplacement,'RED:0:7 red');
      assert.equal(c.absentDate,null); assert.equal(c.absentPattern,null);
      assert.deepEqual(c.regexMatches,['level:3','score:12']); assert.equal(c.resetIndex,0);
      assert.equal(c.transformed,'level=3 score=12'); assert.equal(c.searched,8); assert.deepEqual(c.captured,['left','right','end']);
      assert.equal(c.callback,'A:5'); assert.equal(c.samePattern,c.pattern); assert.notEqual(c.clone,c.pattern); assert.equal(c.cloneIndex,0);
      assert.equal(c.dateIs,true); assert.equal(c.regexIs,true); assert.equal(c.dateAs,c.moment); assert.equal(c.wrongAs,null); assert.equal(c.typeReads,2);
      assert.equal(c.dateRejected,true); assert.equal(c.regexRejected,true); assert.equal(c.jsonText,'{"value":8}'); assert.equal(c.jsonRoot,null);
      assert.equal(c.dateJSON,c.moment.toString()); assert.equal(c.shadowJSON,'user:5');
      assert.deepEqual(c.literalMatches,['level:3','score:12']);assert.equal(c.literalSlash,true);assert.equal(c.literalEscape,true);assert.equal(c.literalBraces,true);assert.equal(c.literalEquals,true);assert.equal(c.divisionAfterLiteral,4);
      assert.equal(c.extendedLiteral,true);assert.equal(c.dotallLiteral,true);assert.notEqual(c.firstLiteral,c.secondLiteral);assert.equal(c.freshLiteralIndex,0);assert.equal(c.lexicalRegex,true);
      assert.equal(c.uriEncoded,'https://example.test/a%20b?q=%CE%B1&x=1');assert.equal(c.uriDecoded,c.uriSource);
      assert.equal(c.componentEncoded,'a%2Fb%20%CE%B1');assert.equal(c.componentDecoded,'a/b α');assert.equal(c.reservedDecoded,'%2F%23');
      assert.deepEqual(c.uriDefaults,['undefined','null','null']);assert.equal(c.uriAliased,'a%20b');assert.equal(c.uriShadowed,'local:value');assert.equal(c.uriError,true);
      assert.equal(c.untouched,'blue'); expected.push(['REPLACE','$& red','RED:0:7 red','blue']);
    } else {
      assert.equal(c.omitted,undefined); assert.equal(c.explicitEnd,'bcd'); assert.equal(c.nullSearch,0);
      assert.equal(c.generated,'A'); assert.equal(c.wrappedIndex,'a');
      assert.deepEqual(c.defaultParts,['undefinedxundefined']); assert.deepEqual(c.emptyParts,[]);
      assert.equal(c.legacyCharacters,'Aé'); assert.equal(c.legacyCodes,65766); assert.equal(c.legacyLengths,5);
      assert.equal(c.legacySlices,'bcd:cdef'); assert.equal(c.legacyAliasResult,'bc'); assert.equal(c.shadowedLength,17);
      assert.equal(c.escapedRust,'A%20%40%C3%A9%2B'); assert.equal(c.unescapedRust,'A @é+');
      assert.equal(c.historicalUpper,'STRAßE'); assert.equal(c.historicalLower,'i');
      assert.equal(c.surrogateUnit,0xd800); assert.equal(c.escapedAlias,'a%2Bb'); assert.equal(c.shadowedEscape,'user:local');
    }
    assert.deepEqual(logs,expected);
  },
}));

// One family smoke exercises the helper ABI while the compiler owner integrates
// the same two source scenarios into the shared emitted-code executor.
export function verifyStringsAdapter() {
  const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
  const code=['regexp.js','strings.js'].map(name=>readFileSync(resolve(root,'crates/neoflash-script/src/actionscript',name),'utf8')).join('\n');
  const services=createStringServices();
  const api=new Function('__nfTypeError','__nfString','__nfNumber','__nfErrorTypes','__nfAs2Strings','__nfAs3Strings',code+'\nreturn {method:__nfStringMethod,create:__nfStringCreate,type:__nfStringClass,value:__nfStringMethodValue};')(TypeError,String,Number,{SyntaxError},()=>services.strings,()=>services.strings3);
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
    if(as3) {assert.equal(call('abc','localeCompare','abd'),-1);assert.equal(call('straße','toLocaleUpperCase'),'STRAßE');assert.equal(call('İ','toLocaleLowerCase'),'i');}
    else assert.throws(()=>call('abc','localeCompare','b'),/unsupported|outside the supported/);
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
