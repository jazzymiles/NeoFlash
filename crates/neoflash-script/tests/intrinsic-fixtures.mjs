// Original executable fixtures. Expected values are documented_only: checked
// against the pinned reference sources, never an original Flash runtime oracle.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

export const intrinsicProfiles = {
  as2: {
    status: 'documented_only',
    catalog_ids: ['as2:global_functions.html#parseInt()'],
    sources: [
      'https://open-flash.github.io/mirrors/as2-language-reference/global_functions.html#parseInt()',
      'https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/avm1/globals.rs#L150',
    ],
  },
  as3: {
    status: 'documented_only',
    catalog_ids: ['AS3:function:package.html#parseInt()', 'AS3:method:Number.html#toFixed()', 'AS3:method:int.html#toFixed()', 'AS3:method:uint.html#toFixed()'],
    sources: [
      'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/actionscript.lang.as#L48',
      'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/Toplevel.cpp#L918',
      'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/Number.as#L146',
      'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/NumberClass.cpp#L33',
      'https://github.com/adobe/avmplus/blob/858d034a3bd3a54d9b70909386435cf4aec81d21/core/MathUtils.cpp#L863',
    ],
  },
};
const fixture = (dialect, source, verify, context = {}) => ({
  language: dialect === 'as2' ? 'action_script2' : 'action_script3',
  profile: intrinsicProfiles[dialect], source, context, verify,
});
export const intrinsicFixtures = [
  fixture('as2', 'values=[parseInt("0777"),parseInt("08"),parseInt("0777tail"),parseInt(" 0777"),parseInt("-0777"),parseInt("0x10"),parseInt("0x10",10),parseInt("-0x10"),parseInt("0x  -10"),parseInt(),parseInt("10",0),parseInt("10",undefined),parseInt("10",null),parseInt("0777",8),parseInt("11",2.9),parseInt("10",1),parseInt("10",37),parseInt("-0")];', c => {
    assert.deepEqual(c.values, [511,8,777,777,-511,16,10,NaN,-16,undefined,NaN,NaN,NaN,511,3,NaN,NaN,-0]);
  }),
  fixture('as3', 'values=[parseInt("0777"),parseInt("08"),parseInt("0777tail"),parseInt("0777",8),parseInt("10",0),parseInt("10",undefined),parseInt("-0x10"),parseInt("0x10",10),parseInt(),parseInt("-0"),parseInt(undefined,36),parseInt(null,36),parseInt("11",4294967298),parseInt("10",Infinity)];', c => {
    assert.deepEqual(c.values, [777,8,777,511,10,10,-16,0,NaN,-0,1112745,1112745,3,10]);
  }),
  ...['as2', 'as3'].map(dialect => fixture(dialect, 'var parse:Function=parseInt; alias=parse("0777"); function shadow(parseInt:Function) { return parseInt("0777"); } local=shadow(function(value) { return "local:"+value; }); var custom={parseInt:function(value) { return "custom:"+value; }}; qualified=custom.parseInt("0777"); function captured(Number, String, Reflect) { return parseInt("0777"); } capturedValue=captured(1,2,3);', c => {
    assert.equal(c.alias, dialect === 'as2' ? 511 : 777);
    assert.equal(c.capturedValue, c.alias);
    assert.equal(c.local, 'local:0777'); assert.equal(c.qualified, 'custom:0777');
  })),
  ...['as2', 'as3'].map(dialect => fixture(dialect, 'var log=""; var input={toString:function() { log+="S"; return "10"; }}; var radix={valueOf:function() { log+="R"; return 8; }}; result=parseInt(input,radix); before=log; log=""; var invalid={valueOf:function() { log+="R"; return 1; }}; rejected=parseInt(input,invalid); after=log;', c => {
    assert.equal(c.result, 8); assert.ok(Number.isNaN(c.rejected));
    assert.equal(c.before, dialect === 'as2' ? 'RS' : 'SR');
    assert.equal(c.after, dialect === 'as2' ? 'R' : 'SR');
  })),
  fixture('as3', 'var n:Number=7.31343; var integer:int=4; var unsigned:uint=4294967295; values=[n.toFixed(3),integer.toFixed(2),unsigned.toFixed(0),n.toFixed(),n.toFixed(undefined),n.toFixed(2.9),n.toFixed(NaN),n.toFixed(Infinity),n.toFixed(4294967296),(-0).toFixed(2),NaN.toFixed(2),Infinity.toFixed(2),(-Infinity).toFixed(2),(1.25).toFixed(1),(1e21).toFixed(2)];', c => {
    assert.deepEqual(c.values, ['7.313','4.00','4294967295','7','7','7.31','7','7','7','0.00','NaN','Infinity','-Infinity','1.3','1000000000000000000000.00']);
  }),
  fixture('as3', 'var values=[]; var numbers=[1,NaN,Infinity,-Infinity]; var digits=[-1,21,4294967295]; for(var i:int=0;i<4;i++){for(var j:int=0;j<3;j++){try{numbers[i].toFixed(digits[j]);values.push("bad");}catch(e:RangeError){values.push(e.errorID);}}} boundary=(1).toFixed(20);', c => {
    assert.deepEqual(c.values, Array(12).fill(1002)); assert.equal(c.boundary, '1.00000000000000000000');
  }),
  fixture('as3', 'var conversions:int=0; var precision={valueOf:function():Number {conversions++;return 2.9;}}; result=(1.25).toFixed(precision); function shadow(Number,Reflect,BigInt) {return (2.5).toFixed(1);} other=shadow(1,2,3);', c => {
    assert.equal(c.conversions, 1); assert.equal(c.result, '1.25'); assert.equal(c.other, '2.5');
  }),
  fixture('as3', 'var tool={toFixed:function(value){return "custom:"+value;}}; custom=tool.toFixed(21); var callback:Function=tool.toFixed; alias=callback(22);', c => {
    assert.equal(c.custom, 'custom:21'); assert.equal(c.alias, 'custom:22');
  }),
  fixture('as3', 'var n:Number=1; var key:String="toFixed"; try {var f:Function=n[key]; result=f(21);} catch(e:Error) {message=e.message;} try {var g:Function=(n[key] ||= 0); g(21);} catch(e:Error) {logical=e.message;}', c => {
    assert.match(c.message, /Unsupported AS3 Number.toFixed method value/);
    assert.match(c.logical, /Unsupported AS3 Number.toFixed method value/);
    assert.equal(c.result, undefined);
  }),
  fixture('as3', 'try {var direct:Function=(1).toFixed;} catch(e:Error) {directMessage=e.message;} try {var literal:Function=(1)["toFixed"];} catch(e:Error) {literalMessage=e.message;} try {var escaped:Function=(1)["to\\u0046ixed"];} catch(e:Error) {escapedMessage=e.message;}', c => {
    for (const key of ['directMessage','literalMessage','escapedMessage']) assert.match(c[key], /Unsupported AS3 Number.toFixed method value/);
  }),
  fixture('as3', 'var data={toFixed:17}; first=data.toFixed; data.toFixed=18; second=data.toFixed; data.toFixed+=1; before=data.toFixed++; after=++data.toFixed;', c => {
    assert.equal(c.first, 17); assert.equal(c.second, 18);
    assert.equal(c.before, 19); assert.equal(c.after, 21); assert.equal(c.data.toFixed, 21);
  }),
  fixture('as3', 'var key:String="toFixed"; var tool={toFixed:function(value){return "custom:"+value;}}; var fn:Function=tool[key]; result=fn(21); var data={toFixed:17,score:1}; field=data[key]; data["score"]+=2; before=data["score"]++; after=++data["score"]; data[key]=18; changed=data[key];', c => {
    assert.equal(c.result, 'custom:21'); assert.equal(c.field, 17); assert.equal(c.changed, 18);
    assert.equal(c.before, 3); assert.equal(c.after, 5); assert.equal(c.data.score, 5);
  }),
  fixture('as3', 'var argument:Function=this.argument; var key=keyObject; result=receiver[key]; receiver[key]=argument(); before=receiver[key]++;', c => {
    assert.equal(c.result, 4); assert.equal(c.before, 7);
    assert.deepEqual(c.log, ['receiver','key','get','receiver','argument','key','set:7','receiver','key','get','key','set:8']);
  }, (() => {
    const c = { log: [] }, receiver = {};
    let value = 4;
    Object.defineProperty(receiver, 'score', { get() { c.log.push('get'); return value; }, set(next) { c.log.push('set:'+next); value=next; } });
    Object.defineProperty(c, 'receiver', { get() { c.log.push('receiver'); return receiver; } });
    c.keyObject = { toString() { c.log.push('key'); return 'score'; } };
    c.argument = () => { c.log.push('argument'); return 7; };
    return c;
  })()),
  fixture('as3', 'var key="method"; try {var fn:Function=receiver[key];} catch(e:Error) {message=e.message;}', c => {
    assert.match(c.message, /Unsupported AS3 Number.toFixed method value/);
    assert.equal(c.reads, 1);
  }, (() => {
    const c = { reads: 0 }, receiver = {};
    Object.defineProperty(receiver, 'method', { get() { assert.equal(this, receiver); c.reads++; return Number.prototype.toFixed; } });
    c.receiver = receiver;
    return c;
  })()),
  fixture('as3', 'var argument:Function=this.argument; read=receiver.toFixed; result=receiver.toFixed(argument());', c => {
    assert.equal(typeof c.read, 'function'); assert.equal(c.result, 'custom:21');
    assert.deepEqual(c.log, ['receiver','method','receiver','method','argument','call']);
  }, (() => {
    const c = { log: [] }, receiver = {};
    Object.defineProperty(c, 'receiver', { get() { c.log.push('receiver'); return receiver; } });
    Object.defineProperty(receiver, 'toFixed', { get() { c.log.push('method'); return function(value) { assert.equal(this, receiver); c.log.push('call'); return 'custom:'+value; }; } });
    c.argument = () => { c.log.push('argument'); return 21; };
    return c;
  })()),
  fixture('as3', 'result=receiver.toString(); var saved:Function=receiver.toString;', c => {
    assert.equal(c.result, 'kept'); assert.equal(c.saved, c.receiver.toString);
  }, { receiver: { value: 'kept', toString() { return this.value; } } }),
  fixture('as3', 'var parse:Function=parseInt; try {parse("1",10,0);} catch(e:Error) {parseMessage=e.message;} try {(1).toFixed(2,3);} catch(e:Error) {fixedMessage=e.message;} var tool={toFixed:function(a,b){return a+b;}}; custom=tool.toFixed(2,3);', c => {
    assert.match(c.parseMessage, /Unsupported AS3 parseInt.*excess-argument/);
    assert.match(c.fixedMessage, /Unsupported AS3 Number.toFixed.*excess-argument/);
    assert.equal(c.custom, 5);
  }),
  fixture('as3', 'function arg():Number {order+="A";return 21;} result=receiver.toFixed(arg());', c => {
    assert.equal(c.order, 'RMA'); assert.equal(c.result, 'kept:21');
  }, (() => {
    const c = { order: '' }, receiver = { label: 'kept:' };
    Object.defineProperty(c, 'receiver', { get() { c.order += 'R'; return receiver; } });
    Object.defineProperty(receiver, 'toFixed', { get() { c.order += 'M'; return function(n) { return this.label + n; }; } });
    return c;
  })()),
  ...['java_script', 'type_script'].map(language => ({ language, source: 'this.decimal=parseInt("0777"); this.precision=(1).toFixed(21);', context: {}, verify(c) {
    assert.equal(c.decimal, 777); assert.equal(c.precision, '1.000000000000000000000');
  } })),
];

// The ordinary authoring document used for browser acceptance. Read its actual
// scripts rather than maintaining a similar test-only copy of the source.
const published = JSON.parse(readFileSync(new URL('../../../examples/as-intrinsics.nflash', import.meta.url), 'utf8'));
export const publishedIntrinsicTraces = {
  as2: [
    ['AS2 octal', 'number', 511],
    ['AS2 edge', 8, 16, true, 'undefined', -Infinity],
    ['AS2 binding', 511, 'local:0777', 'custom:0777'],
  ],
  as3: [
    ['AS3 decimal', 'number', 777],
    ['AS3 edge', 8, -16, 10, -Infinity],
    ['AS3 precision20', 'string', '1.00000000000000000000'],
    ['AS3 precision21', 'RangeError', 1002],
    ['AS3 NaN range', 'RangeError', 1002],
    ['AS3 convert', '7.3', '1', '0.00', 'Infinity'],
    ['AS3 binding', 777, 'local:0777', 'custom:21', 'custom:22'],
    ['AS3 custom field', true],
    ['AS3 dynamic method', true],
  ],
};
for (const dialect of ['as2', 'as3']) {
  const script = published.symbols.find(symbol => symbol.id === `${dialect}-symbol`).timeline.scripts[0].script;
  intrinsicFixtures.push({ ...script, profile: intrinsicProfiles[dialect], context: { stop() {} },
    verify(_context, logs) { assert.deepEqual(logs, publishedIntrinsicTraces[dialect]); },
  });
}
