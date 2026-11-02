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
import { createStringServices } from './rust-runtime-host.mjs';

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
    language: 'action_script2',
    source: (() => {
      let ladder = 'var idx:Number = 87; matched = -1; if (idx == 0) { matched = 0; }';
      for (let i = 1; i < 95; i++) ladder += ` else if (idx == ${i}) { matched = ${i}; }`;
      ladder += ' else { matched = -2; }';
      return `${ladder} tempVar = "gone"; var topVar:Number = 11; function checkDel(paramVal:Number):Array { var innerLocal:Number = 22; return [delete tempVar, delete tempVar, delete topVar, delete innerLocal, delete paramVal, delete this, delete Math, innerLocal]; } delResults = checkDel(33); mcMatch = MovieClip(this); mcFail = MovieClip({}); sndFail = Sound(123);`;
    })(),
    context: {},
    legacy: {
      MovieClip: { __nfIs(v) { return v && v.__isMc === true; } },
      Sound: { __nfIs() { return false; } },
    },
    verify(context) {
      assert.equal(context.matched, 87);
      assert.deepEqual(Array.from(context.delResults), [true, false, true, false, false, false, false, 22]);
      assert.equal('tempVar' in context, false);
      assert.equal('topVar' in context, false);
      assert.equal(context.mcMatch, null);
      assert.equal(context.mcFail, null);
      assert.equal(context.sndFail, null);
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
  {
    language: 'action_script2',
    source: `
      _soundbuftime = 9;
      this.mcBuf = this._soundbuftime;
      var btn = this.placedBtn;
      this.btnBuf = btn._soundbuftime;
      btn._soundbuftime = 12;
      this.globalBufAfterBtn = _soundbuftime;
      this.swfVer = this.getSWFVersion();
      this.score = 42;
      this.getURL("https://example.test/submit", "_blank", "GET");
      this.loadVariables("user=ada&role=engineer", "POST");
      loadVariables("mode=turbo", this, "GET");
      loadMovieNum("hud.swf", 2, "GET");
      loadVariablesNum("hudReady=yes&lives=3", 2, "POST");
      this.lvl0Identity = (_level0 === this);
      this.lvl2Url = _level2._url;
      this.lvl2Lives = _level2.lives;
      unloadMovieNum(2);
      this.lvl2AfterUnload = _level2;
      this.redrawOn = showRedrawRegions(true, 0x00FF00);
      var tf = this.createTextField("headline", 1, 10, 20, 240, 40);
      tf.antiAliasType = "advanced";
      tf.embedFonts = true;
      tf.gridFitType = "subpixel";
      tf.sharpness = 125;
      tf.thickness = -50;
      this.tfProps = [tf.antiAliasType, tf.embedFonts, tf.gridFitType, tf.sharpness, tf.thickness];
      flash.text.TextRenderer.displayMode = "lcd";
      flash.text.TextRenderer.maxLevel = 7;
      flash.text.TextRenderer.setAdvancedAntialiasingTable("Verdana", "none", "dark", [{ fontSize: 12, insideCutoff: 0.6, outsideCutoff: -0.4 }]);
      this.trMode = flash.text.TextRenderer.displayMode;
      this.trMax = flash.text.TextRenderer.maxLevel;
    `,
    context: { placedBtn: {} },
    verify(context) {
      assert.equal(context.mcBuf, 9);
      assert.equal(context.btnBuf, 9);
      assert.equal(context.globalBufAfterBtn, 12);
      assert.equal(context.swfVer, 8);
      assert.ok(context.__nfLastURLRequest.url.includes('https://example.test/submit?'));
      assert.ok(context.__nfLastURLRequest.url.includes('score=42'));
      assert.equal(context.__nfLastURLRequest.window, '_blank');
      assert.equal(context.user, 'ada');
      assert.equal(context.role, 'engineer');
      assert.equal(context.mode, 'turbo');
      assert.equal(context.lvl0Identity, true);
      assert.equal(context.lvl2Url, 'hud.swf');
      assert.equal(context.lvl2Lives, '3');
      assert.equal(context.lvl2AfterUnload, undefined);
      assert.equal(context.redrawOn, true);
      assert.deepEqual(context.tfProps, ['advanced', true, 'subpixel', 125, -50]);
      assert.equal(context.trMode, 'lcd');
      assert.equal(context.trMax, 7);
    },
  },
  {
    language: 'action_script2',
    source: `
      var statuses = [];
      var nc = new NetConnection();
      nc.onStatus = function(info) { statuses.push(info.code); };
      var rpcResult = null;
      nc.onResult = function(res) { rpcResult = res.method; };
      nc.connect(null);
      this.ncConnected = nc.isConnected;
      nc.call("pingService", { onResult: function(r) { statuses.push("rpc:" + r.method); } }, 101);
      var ns = new NetStream(nc);
      ns.checkPolicyFile = true;
      ns.setBufferTime(2.5);
      var metaDuration = 0;
      var playCode = "";
      ns.onStatus = function(info) { statuses.push(info.code); };
      ns.onMetaData = function(meta) { metaDuration = meta.duration; };
      ns.onPlayStatus = function(info) { playCode = info.code; };
      var cam = Camera.get(0);
      cam.setMode(320, 240, 30, true);
      cam.setQuality(32768, 85);
      cam.setKeyFrameInterval(12);
      cam.setLoopback(true);
      var camActive = null;
      cam.onActivity = function(act) { camActive = act; };
      cam.setMotionLevel(35, 1500);
      var mic = Microphone.get(0);
      mic.setRate(22);
      mic.setGain(75);
      mic.setCodec("Speex");
      mic.setEncodeQuality(8);
      mic.setFramesPerPacket(1);
      mic.setUseEchoSuppression(true);
      var micActive = null;
      mic.onActivity = function(act) { micActive = act; };
      mic.setSilenceLevel(15, 1200);
      ns.attachAudio(mic);
      ns.attachVideo(cam, 0);
      ns.publish("liveFeed", "live");
      ns.send("onCue", "marker1");
      ns.play("clip.flv", 0, 14, true);
      ns.play2({ streamName: "clip_hd.flv", start: 2 });
      ns.receiveAudio(false);
      ns.receiveVideo(24);
      ns.pause(true);
      ns.pause(false);
      ns.seek(4.5);
      var infoObj = ns.getInfo();
      this.nsSummary = [ns.bufferTime, ns.checkPolicyFile, metaDuration, playCode, ns.time, ns.currentFps, infoObj.byteCount, rpcResult];
      this.camSummary = [Camera.names.length, cam.name, cam.width, cam.height, cam.fps, cam.bandwidth, cam.quality, cam.keyFrameInterval, cam.loopback, cam.motionLevel, cam.motionTimeOut, camActive, cam.muted, cam.index];
      this.micSummary = [Microphone.names.length, mic.name, mic.rate, mic.gain, mic.codec, mic.encodeQuality, mic.framesPerPacket, mic.useEchoSuppression, mic.silenceLevel, mic.silenceTimeOut, micActive, mic.muted, mic.index];
      ns.close();
      nc.close();
      this.statuses = statuses;
    `,
    context: {},
    verify(context) {
      assert.equal(context.ncConnected, true);
      assert.deepEqual(context.nsSummary, [2.5, true, 14, 'NetStream.Play.TransitionComplete', 4.5, 24, 2048, 'pingService']);
      assert.deepEqual(context.camSummary, [1, 'Default Camera', 320, 240, 30, 32768, 85, 12, true, 35, 1500, false, false, 0]);
      assert.deepEqual(context.micSummary, [1, 'Default Microphone', 22, 75, 'Speex', 8, 1, true, 15, 1200, false, false, 0]);
      assert.ok(context.statuses.includes('NetConnection.Connect.Success'));
      assert.ok(context.statuses.includes('NetStream.Publish.Start'));
      assert.ok(context.statuses.includes('NetStream.Play.Start'));
      assert.ok(context.statuses.includes('NetStream.Seek.Notify'));
      assert.ok(context.statuses.includes('NetConnection.Connect.Closed'));
    },
  },
  {
    language: 'action_script2',
    source: `
      var events = [];
      var fr = new flash.net.FileReference();
      var listener = {
        onSelect: function(f) { events.push("select:" + f.name); },
        onOpen: function(f) { events.push("open"); },
        onProgress: function(f, loaded, total) { events.push("progress:" + loaded + "/" + total); },
        onComplete: function(f) { events.push("complete"); },
        onCancel: function(f) { events.push("cancel"); }
      };
      fr.addListener(listener);
      fr.browse([{ description: "Text", extension: "*.txt" }]);
      fr.upload("https://example.test/upload", "Filedata", false);
      fr.download("https://example.test/report.pdf", "saved.pdf");
      fr.cancel();
      this.frMeta = [fr.name, fr.size, fr.type, fr.creator, fr.creationDate instanceof Date, fr.modificationDate instanceof Date, fr.removeListener(listener)];
      var frl = new flash.net.FileReferenceList();
      var listCount = 0;
      frl.addListener({ onSelect: function(listObj) { listCount = listObj.fileList.length; } });
      frl.browse();
      this.listCount = listCount;
      System.exactSettings = false;
      System.useCodepage = true;
      System.showSettings(2);
      this.sysProps = [System.exactSettings, System.useCodepage, System.capabilities.hasAudio, System.capabilities.hasStreamingVideo, System.capabilities.screenResolutionX, System.capabilities.screenResolutionY, System.capabilities.serverString.indexOf("R=1920x1080") >= 0];
      var nc = new NetConnection();
      nc.connect(null);
      var rso = SharedObject.getRemote("roomState", nc.uri, false, false);
      var syncEvents = 0;
      rso.onSync = function(list) { syncEvents += list.length; };
      rso.connect(nc);
      rso.setFps(15);
      rso.setProperty("topic", "Wave7");
      rso.setDirty("topic");
      var msgOut = "";
      rso.onChat = function(msg) { msgOut = msg; };
      rso.send("onChat", "hello-rso");
      rso.close();
      this.rsoSummary = [rso.data.topic, syncEvents > 0, msgOut];
      var mcl = new MovieClipLoader();
      mcl.checkPolicyFile = true;
      var snd = new Sound(this);
      snd.checkPolicyFile = true;
      var vid = this.placedVideo;
      vid.deblocking = 4;
      flash.external.ExternalInterface.addCallback("echoMethod", this, function(v) { return "echo:" + v; });
      var extRes = flash.external.ExternalInterface.call("echoMethod", "42");
      print(this, "bmovie");
      printNum(0, "bmax");
      printAsBitmap(this, "bframe");
      printAsBitmapNum(0, "bmovie");
      this.miscSummary = [mcl.checkPolicyFile, snd.checkPolicyFile, vid.deblocking, extRes, events.join(",")];
    `,
    context: { placedVideo: {} },
    verify(context) {
      assert.deepEqual(context.frMeta, ['saved.pdf', 512, '.pdf', 'NF2', true, true, true]);
      assert.equal(context.listCount, 2);
      assert.deepEqual(context.sysProps, [false, true, true, true, 1920, 1080, true]);
      assert.deepEqual(context.rsoSummary, ['Wave7', true, 'hello-rso']);
      assert.deepEqual(context.miscSummary, [
        true,
        true,
        4,
        'echo:42',
        'select:document.txt,open,progress:256/256,complete,select:saved.pdf,open,progress:512/512,complete,cancel',
      ]);
    },
  },
  {
    language: 'action_script3',
    source: `
      native function nativeAdd(a:int, b:int = 7):int;
      native function nativeClamp(v:Number, lo:Number, hi:Number):Number;
      this.nativeSum = nativeAdd(15);
      this.nativeClamped = nativeClamp(140, 0, 100);
      var appDom:ApplicationDomain = ApplicationDomain.currentDomain;
      var secDom:SecurityDomain = SecurityDomain.currentDomain;
      var lc:LoaderContext = new LoaderContext(true, appDom, secDom);
      lc.allowCodeImport = false;
      lc.imageDecodingPolicy = "onLoad";
      lc.parameters = { theme: "dark" };
      var jlc:JPEGLoaderContext = new JPEGLoaderContext(0.75, true, appDom, secDom);
      this.loaderSummary = [lc.checkPolicyFile, lc.allowCodeImport, lc.imageDecodingPolicy, lc.parameters.theme, jlc.deblockingFilter, jlc is LoaderContext];
      this.capSummary = [
        Capabilities.cpuAddressSize,
        Capabilities.cpuArchitecture,
        Capabilities.hasAudio,
        Capabilities.hasTLS,
        Capabilities.maxLevelIDC,
        Capabilities.screenResolutionX,
        Capabilities.screenResolutionY,
        Capabilities.supports64BitProcesses,
        Capabilities.supports32BitProcesses,
        Capabilities.touchscreenType,
        Capabilities.hasMultiChannelAudio(AudioDecoder.DOLBY_DIGITAL),
        Capabilities.hasMultiChannelAudio("unknownCodec"),
        Capabilities.serverString.indexOf("PR64=t") >= 0
      ];
      var ctx3d:Context3D = new Context3D();
      var vtex:VideoTexture = ctx3d.createVideoTexture();
      var nc:NetConnection = new NetConnection();
      nc.connect(null);
      var ns:NetStream = new NetStream(nc);
      ns.checkPolicyFile = true;
      vtex.attachNetStream(ns);
      var vtexDims = [Context3D.supportsVideoTexture, vtex.videoWidth, vtex.videoHeight, ns.checkPolicyFile];
      vtex.dispose();
      var atf:ByteArray = new ByteArray();
      atf.writeUTFBytes("ATF");
      atf.writeByte(0);
      atf.writeByte(0);
      atf.writeByte(0);
      atf.writeByte(2);
      atf.writeByte(8);
      atf.writeByte(8);
      atf.writeByte(4);
      var tex:Texture = new Texture(256, 256);
      tex.uploadCompressedTextureFromByteArray(atf, 0, true);
      var cube:CubeTexture = new CubeTexture(128);
      cube.uploadCompressedTextureFromByteArray(atf, 0, false);
      this.stage3dSummary = [vtexDims[0], vtexDims[1], vtexDims[2], vtexDims[3], vtex.videoWidth, tex.compressedUploaded, tex.width, tex.mipCount, cube.compressedUploaded];
    `,
    context: {},
    verify(context) {
      assert.equal(context.nativeSum, 22);
      assert.equal(context.nativeClamped, 100);
      assert.deepEqual(context.loaderSummary, [true, false, 'onLoad', 'dark', 0.75, true]);
      assert.deepEqual(context.capSummary, [64, 'ARM', true, true, '5.1', 1920, 1080, true, false, 'none', true, false, true]);
      assert.deepEqual(context.stage3dSummary, [true, 320, 240, true, 0, true, 256, 4, true]);
    },
  },
  {
    language: 'action_script3',
    source: `
      var vss:VideoStreamSettings = new VideoStreamSettings();
      vss.setMode(640, 480, 60);
      vss.setQuality(65536, 90);
      vss.setKeyFrameInterval(24);
      var h264:H264VideoStreamSettings = new H264VideoStreamSettings();
      h264.setProfileLevel(H264Profile.MAIN, H264Level.LEVEL_4_1);
      var vp6:VP6VideoStreamSettings = new VP6VideoStreamSettings();
      var adm:AudioDeviceManager = AudioDeviceManager.deviceManager;
      var evtReason:String = "";
      var admEvt:AudioOutputChangeEvent = new AudioOutputChangeEvent(AudioOutputChangeEvent.AUDIO_OUTPUT_CHANGE, false, false, AudioOutputChangeReason.USER_SELECTION);
      evtReason = admEvt.reason;
      adm.selectedDeviceIndex = 1;
      var rpcVal:String = "";
      var resp:Responder = new Responder(function(r):void { rpcVal = r.command; });
      var nc:NetConnection = new NetConnection();
      nc.connect(null);
      nc.call("getScore", resp, 99);
      var nspo:NetStreamPlayOptions = new NetStreamPlayOptions();
      nspo.oldStreamName = "low.mp4";
      nspo.streamName = "high.mp4";
      nspo.start = 0;
      nspo.len = 30;
      nspo.offset = 5;
      nspo.transition = NetStreamPlayTransitions.SWITCH;
      var ns:NetStream = new NetStream(nc);
      ns.play2(nspo);
      var so:SharedObject = SharedObject.getLocal("wave7Prefs");
      so.fps = 30;
      var clientMsg:String = "";
      so.client = { onBroadcast: function(m:String):void { clientMsg = m; } };
      so.setProperty("volume", 80);
      so.setDirty("volume");
      so.send("onBroadcast", "synced");
      var io:InteractiveObject = new InteractiveObject();
      io.needsSoftKeyboard = true;
      io.softKeyboardInputAreaOfInterest = new Rectangle(10, 20, 400, 300);
      var skOk:Boolean = io.requestSoftKeyboard();
      var avm1:AVM1Movie = new AVM1Movie();
      this.mediaNetDisplaySummary = [
        vss.width, vss.height, vss.fps, vss.bandwidth, vss.quality, vss.keyFrameInterval,
        h264.codec, h264.profile, h264.level,
        vp6.codec,
        AudioDeviceManager.isSupported, adm.deviceNames.length, adm.selectedDeviceIndex, evtReason,
        rpcVal, nspo.transition, ns.playingUrl,
        so.data.volume, so.fps, clientMsg,
        ColorCorrection.ON, ColorCorrectionSupport.DEFAULT_ON,
        skOk, Stage.softKeyboardRect.width, Stage.stageVideos.length,
        avm1 is InteractiveObject
      ];
    `,
    context: {},
    verify(context) {
      assert.deepEqual(context.mediaNetDisplaySummary, [
        640, 480, 60, 65536, 90, 24,
        'H264Avc', 'main', '4.1',
        'On2 VP6',
        true, 2, 1, 'userSelection',
        'getScore', 'switch', 'high.mp4',
        80, 30, 'synced',
        'on', 'defaultOn',
        true, 400, 1,
        true,
      ]);
    },
  },
];
const result = spawnSync(cargo, ['run', '--quiet', '-p', 'neoflash-script', '--example', 'compile_batch'], {
  cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
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
  const legacy = {...createStringServices(), ...fixture.legacy};
  for (let i = 0; i < (fixture.repeat || 1); i++) {
    if (fixture.verifyThrown) {
      assert.throws(() => run.call(fixture.context, { log: (...args) => consoleLogs.push(args) }, (...args) => logs.push(args), undefined, as2Bridge.method, as2Bridge.value, display, legacy), error => { fixture.verifyThrown(error); return true; });
    } else {
      run.call(fixture.context, { log: (...args) => consoleLogs.push(args) }, (...args) => logs.push(args), () => { throw new Error('wrong binding'); }, as2Bridge.method, as2Bridge.value, display, legacy);
    }
  }
  fixture.verify(fixture.context, logs);
  if (fixture.language.startsWith('action_script')) assert.deepEqual(consoleLogs, [], 'AS trace must use the host trace binding, never bypass it through console');
  if (fixture.hostIntegration) verifyHostIntegration(root, fixture, outputs[index].Ok);
}
console.log(`Script execution: ${fixtures.length} behavior fixtures and actual-host integration passed.`);
