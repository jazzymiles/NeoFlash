// Execute emitted code in a real JavaScript engine; no golden-string assertions.
// Run from the repository with: node crates/neoflash-script/tests/execute.mjs
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { verifyHostIntegration } from './host-integration.mjs';
import { controlFlowFixtures } from './control-flow-fixtures.mjs';
import { intrinsicFixtures } from './intrinsic-fixtures.mjs';
import { collectionsFixtures } from './collections-fixtures.mjs';
import { stringsFixtures } from './strings-fixtures.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const { createAs2Methods } = createRequire(import.meta.url)(resolve(root, 'web/runtime.js'));
const as2Bridge = createAs2Methods();
const localCargo = resolve(homedir(), '.cargo/bin/cargo');
const cargo = process.env.CARGO || (existsSync(localCargo) ? localCargo : 'cargo');
const fixtures = [
  ...controlFlowFixtures,
  ...intrinsicFixtures,
  ...collectionsFixtures,
  ...stringsFixtures,
  {
    language: 'action_script3',
    source: 'var count:int; function increment():int { count += 1; return count; } trace("Blue", increment());',
    context: {}, repeat: 2, hostIntegration: true,
    verify(c, logs) { assert.equal(c.count, 2); assert.deepEqual(logs, [['Blue', 1], ['Blue', 2]]); },
  },
  {
    language: 'action_script3',
    source: 'function f():int { var a:int=1; var b:int=2; a\n++\nb; return a*10+b; } result=f(); function comment():int { var a:int=1; var b:int=2; a /* next\n line */ ++b; return a*10+b; } other=comment();',
    context: {},
    verify(c) { assert.equal(c.result, 13); assert.equal(c.other, 13); },
  },
  {
    language: 'action_script3',
    source: 'function choose(flag:Boolean):int { var x:int=0; var answer:int=flag ? x=1 : x=2; return answer*10+x; } yes=choose(true); no=choose(false);',
    context: {},
    verify(c) { assert.equal(c.yes, 11); assert.equal(c.no, 22); },
  },
  {
    language: 'action_script3',
    source: 'var reads:int = 0; var value:int = 1; this.value = {valueOf:function():Number { reads++; return reads; }}; var before:Number = value++; var downReads:int = 0; var down:int = 1; this.down = {valueOf:function():Number { downReads++; return 5; }}; var beforeDown:Number = down--;',
    context: {},
    verify(c) { assert.equal(c.reads, 1); assert.equal(c.value, 2); assert.equal(c.before, 1); assert.equal(c.downReads, 1); assert.equal(c.down, 4); assert.equal(c.beforeDown, 5); },
  },
  {
    language: 'action_script3',
    source: String.raw`var quoted = {"run":function():int { return 7; }}; first = quoted.run(); var escaped = {'ru\u006e':function():int { return 8; }}; second = escaped.run(); var hex = {"r\x75n":function():int { return 9; }}; third = hex.run(); var unicode = {"\u03c0":function():int { return 10; }}; fourth = unicode.π(); var quote = {"say\"hi":function():int { return 11; }};`,
    context: {},
    verify(c) { assert.equal(c.first, 7); assert.equal(c.second, 8); assert.equal(c.third, 9); assert.equal(c.fourth, 10); assert.equal(c.quote['say"hi'](), 11); },
  },
  {
    language: 'action_script2',
    source: 'function make() { return function(step) { _x += step; return _x; }; } var callback:Function = make();',
    context: { _x: 1 },
    verify(c) { const other = { _x: 100 }; assert.equal(c.callback.call(other, 3), 4); assert.equal(c._x, 4); assert.equal(other._x, 100); },
  },
  {
    language: 'action_script3',
    source: 'function shadow(Number:int, play:int):int { Number++; play += 2; return Number + play; } result = shadow(4, 6); function builtinShadow():int { var Math = { custom: function():int { return 11; } }; return Math.custom(); } custom = builtinShadow();',
    context: {},
    verify(c) { assert.equal(c.result, 13); assert.equal(c.custom, 11); },
  },
  {
    language: 'action_script3',
    source: 'function operate(Number:Function, trace:Function):int { var n:int = 4294967295; n++; trace(n); return Number(n); } answer = operate(function(x:int):int { return x + 7; }, function(x:int):void { captured = x; }); function nested():int { function trace(x:int):int { return x * 3; } return trace(4); } other = nested();',
    context: {},
    verify(c) { assert.equal(c.answer, 7); assert.equal(c.captured, 0); assert.equal(c.other, 12); },
  },
  {
    language: 'action_script2',
    source: 'function local(Number, trace) { return trace(Number(2)); } result = local(function(x) { return x + 1; }, function(x) { return x * 4; }); scrollRect=null; absentBare=scrollRect; absentMember=this.scrollRect; var key="scrollRect"; absentComputed=this[key]; var record={scrollRect:null}; ordinary=record.scrollRect; function localRect(scrollRect){return scrollRect;} shadowedRect=localRect(null); var imageLoader:MovieClipLoader=new MovieClipLoader(); loaderBytes=imageLoader.getProgress(this).bytesLoaded; function localLoader(MovieClipLoader:Function){return MovieClipLoader("local");} loaderShadow=localLoader(function(value){return value;}); var save:SharedObject=SharedObject.getLocal("fixture"); save.data.score=4; saveStatus=save.flush(); savedSize=save.getSize(); function localStore(SharedObject:Object){return SharedObject.getLocal("local");} storeShadow=localStore({getLocal:function(name){return name;}}); var focusListener={count:0,onSetFocus:function(oldFocus,newFocus){this.count++;}};Selection.addListener(focusListener);focusAccepted=Selection.setFocus(this);Selection.setSelection(1,3);focusPath=Selection.getFocus();focusStart=Selection.getBeginIndex();focusEnd=Selection.getEndIndex();focusCaret=Selection.getCaretIndex();Selection.removeListener(focusListener);focusCalls=focusListener.count;function localSelection(Selection:Object):String{return Selection.getFocus();}focusShadow=localSelection({getFocus:function(){return "local-focus";}});',
    context: {}, nativeDisplay: true,
    legacy: {Selection:{listener:null,target:null,begin:0,end:0,addListener(listener){this.listener=listener;},removeListener(listener){assert.equal(listener,this.listener);this.listener=null;return true;},setFocus(target){this.target=target;this.listener.onSetFocus(null,target);return true;},getFocus(){return this.target?"_level0":"";},setSelection(begin,end){this.begin=begin;this.end=end;},getBeginIndex(){return this.begin;},getEndIndex(){return this.end;},getCaretIndex(){return this.end;}},MovieClipLoader:class {getProgress(target){assert.ok(target.imageLoader===this);return {bytesLoaded:17,bytesTotal:17};}},SharedObject:{getLocal(name){assert.equal(name,"fixture");return {data:{},flush(){return this.data.score===4;},getSize(){return 13;}};}}},
    verify(c) { assert.equal(c.result, 12); assert.equal(c.absentBare,undefined); assert.equal(c.absentMember,undefined); assert.equal(c.absentComputed,undefined); assert.equal(c.scrollRect,null); assert.equal(c.ordinary,null); assert.equal(c.shadowedRect,null); assert.equal(c.loaderBytes,17); assert.equal(c.loaderShadow,'local'); assert.equal(c.saveStatus,true); assert.equal(c.savedSize,13); assert.equal(c.storeShadow,'local'); assert.equal(c.focusAccepted,true);assert.equal(c.focusPath,'_level0');assert.equal(c.focusStart,1);assert.equal(c.focusEnd,3);assert.equal(c.focusCaret,3);assert.equal(c.focusCalls,1);assert.equal(c.focusShadow,'local-focus'); },
  },
  {
    language: 'action_script3',
    source: 'function log():void { var console = 2; trace("safe trace"); } log();',
    context: {},
    verify(c, logs) { assert.deepEqual(logs, [['safe trace']]); },
  },
  {
    language: 'action_script3',
    source: 'answer = sum(2, 3); function sum(left:int, right:int):int { return left + right; }',
    context: {},
    verify(c) { assert.equal(c.answer, 5); assert.equal(typeof c.sum, 'function'); assert.ok(!Object.hasOwn(c, 'left')); },
  },
  {
    language: 'action_script2',
    source: 'answer = advance(4); function advance(step:Number):Number { var temporary = step + 1; _x += temporary; stop(); return _x; }',
    context: { _x: 10, stop() { this.stopped = true; } },
    verify(c) { assert.equal(c.answer, 15); assert.equal(c.stopped, true); assert.ok(!Object.hasOwn(c, 'temporary')); },
  },
  {
    language: 'action_script3',
    source: 'function makeCounter(seed:int):Function { var count:int = seed; return function(step:int):int { count += step; return count; }; } var counter:Function = makeCounter(10); first = counter(2); second = counter(3);',
    context: {},
    verify(c) { assert.equal(c.first, 12); assert.equal(c.second, 15); assert.equal(c.counter(1), 16); assert.ok(!Object.hasOwn(c, 'count')); },
  },
  {
    language: 'action_script3',
    source: 'var factorial:Function = function recur(n:int):int { if (n <= 1) return 1; return n * recur(n - 1); }; answer = factorial(6); leaked = typeof recur;',
    context: {},
    verify(c) { assert.equal(c.answer, 720); assert.equal(c.leaked, 'undefined'); },
  },
  {
    language: 'action_script3',
    source: 'function parity(n:int):Boolean { return even(n); function even(x:int):Boolean { if (x == 0) return true; return odd(x - 1); } function odd(x:int):Boolean { if (x == 0) return false; return even(x - 1); } } answer = parity(8); other = parity(7);',
    context: {},
    verify(c) { assert.equal(c.answer, true); assert.equal(c.other, false); assert.ok(!Object.hasOwn(c, 'even')); },
  },
  {
    language: 'action_script3',
    source: 'var x:int = 100; function shadow(x:int):int { var before:int = local; local = 7; { var local:int; } return before + local + x; } result = shadow(2);',
    context: {},
    verify(c) { assert.equal(c.x, 100); assert.equal(c.result, 9); assert.ok(!Object.hasOwn(c, 'local')); },
  },
  {
    language: 'action_script3',
    source: 'var n:int = 10; function local() { var n; n = "12"; return n + 1; } result = local();',
    context: {},
    verify(c) { assert.equal(c.n, 10); assert.equal(c.result, '121'); },
  },
  {
    language: 'action_script3',
    source: 'function make(start:uint):Function { var n:uint = start; return function():uint { return ++n; }; } var increment:Function = make(4294967295); first = increment(); second = increment();',
    context: {},
    verify(c) { assert.equal(c.first, 0); assert.equal(c.second, 1); },
  },
  {
    language: 'action_script3',
    source: 'function first(x:int):int { if (x < 0) return -1; return x + 1; } negative = first(-9); positive = first(4); function wrap(x:Number):int { return x; } wrapped = wrap(4294967295); function text(x:String):String { return x; } label = text(12); empty = text(null);',
    context: {},
    verify(c) { assert.equal(c.negative, -1); assert.equal(c.positive, 5); assert.equal(c.wrapped, -1); assert.equal(c.label, '12'); assert.equal(c.empty, null); },
  },
  {
    language: 'action_script2',
    source: 'function echo(value:Number):Number { return value; } result = echo("uncoerced"); missing = echo(); function newline() { return\n 23; } early = newline(); function inspect(value:Number):Array {var saved=arguments[0];value=88;return [saved,arguments.length,arguments.callee===inspect,arguments[1]];} raw=inspect("unchanged","extra");var dynamic=function(step){return this.value+step;};called=dynamic.call({value:10},2);applied=dynamic.apply({value:20},[3]);function forward(){return dynamic.apply({value:30},arguments);}forwarded=forward(4);',
    context: {},
    verify(c) { assert.equal(c.result, 'uncoerced'); assert.equal(c.missing, undefined); assert.equal(c.early, undefined); assert.deepEqual(c.raw,['unchanged',2,true,'extra']);assert.equal(c.called,12);assert.equal(c.applied,23);assert.equal(c.forwarded,34); },
  },
  {
    language: 'action_script3',
    source: 'var tools = { twice: function(value:int):int { return value * 2; } }; first = tools.twice(3); this.custom = function(value:int):int { return value + 1; }; second = this.custom(9);',
    context: {},
    verify(c) { assert.equal(c.first, 6); assert.equal(c.second, 10); },
  },
  {
    language: 'action_script3',
    source: 'function method():int { return this.x; } var detached:Function = method; result = detached(); function invoke(callback:Function):int { return callback(); } passed = invoke(function():int { return 7; }); iife = (function(x:int):int { return x * 2; })(4);',
    context: { x: 3 },
    verify(c) { assert.equal(c.result, 3); assert.equal(c.method.call({x: 9}), 3); assert.equal(c.passed, 7); assert.equal(c.iife, 8); },
  },
  {
    language: 'action_script3',
    source: `function expects(value:int):int { return value; }
      function defaults(value:Number=7,label:String="ready"):Array {return [value,label];}
      omitted=defaults();explicit=defaults(undefined,undefined);
      function gather(head:int=3,...items:Array):Array {return [head,items.length,items.join(":")];}
      noRest=gather();rest=gather("4.8","a","b");
      function inspect(value:int):Array {var count:int=arguments.length;var supplied:int=arguments[0];value=99;arguments[0]=66;return [count,supplied,value,arguments[0],arguments[1],arguments.callee===inspect];}
      original=inspect("4.8","raw");
      var dynamic:Function=function(step:int=1):int {return this.value+step;};
      called=dynamic.call({value:8});applied=dynamic["apply"]({value:10},[]);
      var custom={call:function(value:int):int{return value+10;}};customCall=custom.call(5);
      var aliasFailed:Boolean=false;try{var alias:Function=dynamic.apply;}catch(e:TypeError){aliasFailed=true;}
      var missingArgument:Boolean=false;try{expects();}catch(e:ArgumentError){missingArgument=true;}
      var arrayLikeFailed:Boolean=false;try{dynamic.apply(this,{length:0});}catch(e:TypeError){arrayLikeFailed=true;}
      var nativeConstructorFailed:Boolean=false;try{Function.call(null,"return 1");}catch(e:TypeError){nativeConstructorFailed=true;}`,
    context: {},
    verify(c) { assert.throws(() => c.expects(), /expects 1 arguments/); assert.throws(() => c.expects(1,2), /expects 1 arguments/); assert.equal(c.expects('12.8'), 12);
      assert.deepEqual(c.omitted,[7,'ready']);assert.ok(Number.isNaN(c.explicit[0]));assert.equal(c.explicit[1],null);
      assert.deepEqual(c.noRest,[3,0,'']);assert.deepEqual(c.rest,[4,2,'a:b']);assert.deepEqual(c.original,[2,4,99,66,'raw',true]);
      assert.equal(c.called,9);assert.equal(c.applied,11);assert.equal(c.customCall,15);assert.ok(c.aliasFailed&&c.missingArgument&&c.arrayLikeFailed&&c.nativeConstructorFailed);
    },
  },
  {
    language: 'action_script2',
    source: 'var step:Number = 4; _x += step; if (_x >= 8) stop(); trace("stop() _x");',
    context: { _x: 4, stopped: false, stop() { this.stopped = true; } },
    verify(context, logs) {
      assert.equal(context._x, 8); assert.equal(context.step, 4);
      assert.equal(context.stopped, true); assert.deepEqual(logs, [['stop() _x']]);
    },
  },
  {
    language: 'action_script3',
    source: 'var n:int = 2147483647; var before:int = n++; var after:int = ++n;',
    context: {},
    verify(context) {
      assert.equal(context.before, 2147483647);
      assert.equal(context.after, -2147483647); assert.equal(context.n, -2147483647);
    },
  },
  {
    language: 'action_script3',
    source: 'x = 2 + 3 * 4; y = (2 + 3) * 4; sum = 0; for (var i:int = 0; i < 5; i++) { if (i == 2) continue; sum += i; }',
    context: {},
    verify(context) { assert.equal(context.x, 14); assert.equal(context.y, 20); assert.equal(context.sum, 8); },
  },
  {
    language: 'action_script3',
    source: 'var active:Boolean = false; active &&= true; var label:String = null; label = 12; child.gotoAndStop(3);',
    context: (() => {
      let active; const context = { writes: 0, child: { frame: 0, gotoAndStop(frame) { this.frame = frame; } } };
      Object.defineProperty(context, 'active', { get() { return active; }, set(value) { active = value; this.writes++; } });
      return context;
    })(),
    verify(context) {
      assert.equal(context.active, false); assert.equal(context.writes, 1);
      assert.equal(context.label, '12'); assert.equal(context.child.frame, 3);
    },
  },
  {
    language: 'action_script3',
    source: 'var counter:int; counter++;',
    context: {}, repeat: 2,
    verify(context) { assert.equal(context.counter, 2); },
  },
  {
    language: 'type_script',
    source: 'interface Point { x: number; } enum Direction { Left = 2, Right } const p: Point = {x: 4}; this.x = p.x; this.direction = Direction.Right;',
    context: {},
    verify(context) { assert.equal(context.x, 4); assert.equal(context.direction, 3); },
  },
  {
    language: 'java_script',
    source: 'const stop = () => 5; const [a, b] = [stop(), 2]; this.x = a + b; return this.x;',
    context: {},
    verify(context) { assert.equal(context.x, 7); },
  },
];
const result = spawnSync(cargo, ['run', '--quiet', '-p', 'neoflash-script', '--example', 'compile_batch'], {
  cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024,
  input: JSON.stringify(fixtures.map(({ language, source }) => ({ language, source }))),
});
assert.equal(result.status, 0, result.stderr || result.error?.message);
const outputs = JSON.parse(result.stdout);
for (const [index, fixture] of fixtures.entries()) {
  assert.ok(outputs[index].Ok, JSON.stringify(outputs[index]));
  const logs = [];
  const consoleLogs = [];
  const run = new Function('console', 'trace', 'stop', '__nfAs2Method', '__nfAs2Value', '__nfDisplay', '__nfLegacy', '"use strict"; return (function(){\n' + outputs[index].Ok + '\n}).call(this)');
  const display=fixture.nativeDisplay?{alive:receiver=>receiver===fixture.context}:undefined;
  for (let i = 0; i < (fixture.repeat || 1); i++) {
    if (fixture.verifyThrown) {
      assert.throws(() => run.call(fixture.context, { log: (...args) => consoleLogs.push(args) }, (...args) => logs.push(args), undefined, as2Bridge.method, as2Bridge.value, display, fixture.legacy), error => { fixture.verifyThrown(error); return true; });
    } else {
      run.call(fixture.context, { log: (...args) => consoleLogs.push(args) }, (...args) => logs.push(args), () => { throw new Error('wrong binding'); }, as2Bridge.method, as2Bridge.value, display, fixture.legacy);
    }
  }
  fixture.verify(fixture.context, logs);
  if (fixture.language.startsWith('action_script')) assert.deepEqual(consoleLogs, [], 'AS trace must use the host trace binding, never bypass it through console');
  if (fixture.hostIntegration) verifyHostIntegration(root, fixture, outputs[index].Ok);
}
console.log(`Script execution: ${fixtures.length} behavior fixtures and actual-host integration passed.`);
