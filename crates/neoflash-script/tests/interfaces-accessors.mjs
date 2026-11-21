// One model/component publication exercises nominal contracts and actual property
// descriptors together. Runtime/WASM evidence; visual browser QA is separate.
import assert from 'node:assert/strict';
import { runCargo } from '../../../scripts/test-cargo.cjs';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyInterfacesAccessors(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/interfaces-accessors-'));
  let player,host;
  try {
    const exported=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/interfaces-accessors.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();
    const scene=host.proxyFor([]),model=scene.model,other=scene.other,program=host.program();
    assert.equal(host.lastTrace(),'interfaces ready 2 2 7');
    assert.equal(scene.status.text,'bounded:2 | other:7');
    for (const name of ['app.ICounter','app.IReadable','app.IResettable']) {
      assert.equal(program.is(model,name),true,'base class implements the complete inherited interface closure');
      assert.equal(program.cast(name,other),other);
      assert.equal(program.is({value:2,increment(){}},name),false,'interface identity is nominal');
    }
    assert.equal(program.cast('app.ICounter',null),null);
    assert.throws(()=>program.cast('app.ICounter',{}),/value required/);
    const ICounter=program.type('app.ICounter');
    assert.equal(ICounter(model),model);assert.equal(model instanceof ICounter,true);
    assert.throws(()=>new ICounter(),/cannot be constructed/);
    assert.throws(()=>program.construct('app.ICounter'),/cannot be constructed/);
    assert.throws(()=>{scene.model={};},/value required/);
    assert.equal(scene.model,model,'failed interface-slot assignment preserves the old value');
    assert.equal(model.accept(other),other);assert.equal(model.accept(null),null);
    assert.throws(()=>model.accept({}),/value required/);
    model.reads=0;model.writes=0;
    assert.equal(model.post(),2);
    assert.deepEqual([model.stored,model.reads,model.writes],[3,1,1],'postfix evaluates inherited getter and overridden setter once');
    let conversions=0;
    model.value={valueOf(){conversions++;return 8.8;}};
    assert.equal(conversions,1);assert.equal(model.stored,8,'setter argument coerces before clamping and super delegation');
    model.value=50;assert.equal(model.stored,9);
    model.command=4;assert.equal(model.stored,4,'inherited write-only setter dispatches to the actual receiver');
    assert.throws(()=>model.command,/write-only/);
    assert.throws(()=>{model.label='replace';},/read-only/);
    const increment=model.increment;
    assert.equal(increment,model.increment);assert.equal(increment(1),5,'declared methods remain bound');
    assert.equal(other.value,7);
    const Counter=program.type('app.Counter');
    assert.equal(Counter.created,2);
    Counter.created={valueOf(){conversions++;return 6.9;}};
    assert.equal(Counter.created,6);assert.equal(conversions,2,'static accessors coerce through their declared setter');
    model.reset();
    host.stage().handlePointer('click',{},scene);
    assert.equal(host.lastTrace(),'interfaces click 1 1 7');
    assert.equal(scene.status.text,'bounded:1 | other:7');
    host.resetProject();
    const fresh=host.proxyFor([]);
    assert.notEqual(fresh.model,model);
    assert.equal(host.lastTrace(),'interfaces ready 2 2 7');
    assert.equal(host.program().is(model,'app.ICounter'),false,'old publication identities do not leak into a restarted program');
    host.stage().handlePointer('click',{},fresh);
    assert.equal(host.lastTrace(),'interfaces click 1 3 7');
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
  await verifyObjectProxyAndFunction(root,cargo);
  console.log('Interfaces/accessors: published typed components, inherited contracts/properties, object_proxy/Function, coercion, method binding, input and Restart passed.');
}

async function verifyObjectProxyAndFunction(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/object-proxy-fn-'));
  let player,host;
  const bagProxySource=`package app {
import flash.utils.Proxy;
import flash.utils.object_proxy;
use namespace object_proxy;

public dynamic class BagProxy extends Proxy {
  private var keys:Array = [];
  private var vals:Object = {};
  private var attrs:Object = {};
  public var log:Array = [];
  public var lastQName:String = "";

  public function BagProxy() {
    super();
  }

  object_proxy override function isAttribute(name:*):Boolean {
    return super.isAttribute(name);
  }

  object_proxy override function getProperty(name:*):* {
    if (name is QName) {
      lastQName = QName(name).uri + "|" + QName(name).localName;
    }
    var k:String = String(name);
    if (isAttribute(name)) {
      log.push("get@" + k);
      return "attr:" + String(attrs[k]);
    }
    log.push("get:" + k);
    return vals[k];
  }

  object_proxy override function setProperty(name:*, value:*):void {
    if (name is QName) {
      lastQName = QName(name).uri + "|" + QName(name).localName;
    }
    var k:String = String(name);
    if (isAttribute(name)) {
      log.push("set@" + k);
      attrs[k] = value;
      return;
    }
    log.push("set:" + k);
    if (keys.indexOf(k) < 0) keys.push(k);
    vals[k] = value;
  }

  object_proxy override function callProperty(name:*, ...rest):* {
    var k:String = String(name);
    log.push("call:" + k + ":" + rest.length);
    var total:int = 0;
    for (var i:int = 0; i < rest.length; i++) total += int(rest[i]);
    return k + ":" + total;
  }

  object_proxy override function hasProperty(name:*):Boolean {
    var k:String = String(name);
    log.push("has:" + k);
    return keys.indexOf(k) >= 0;
  }

  object_proxy override function deleteProperty(name:*):Boolean {
    var k:String = String(name);
    if (isAttribute(name)) {
      log.push("del@" + k);
      var hadAttr:Boolean = k in attrs;
      delete attrs[k];
      return hadAttr;
    }
    log.push("del:" + k);
    var idx:int = keys.indexOf(k);
    if (idx < 0) return false;
    keys.splice(idx, 1);
    delete vals[k];
    return true;
  }

  object_proxy override function nextNameIndex(index:int):int {
    return index < keys.length ? index + 1 : 0;
  }

  object_proxy override function nextName(index:int):String {
    return String(keys[index - 1]);
  }

  object_proxy override function nextValue(index:int):* {
    return vals[keys[index - 1]];
  }
}
}
`;
  const frameSource=`import app.BagProxy;
import flash.events.Event;
import flash.utils.object_proxy;
import mx.utils.ObjectProxy;

var bag:BagProxy = new BagProxy();
this.bagRef = bag;
bag.alpha = 10;
bag.beta = 25;
this.readSum = int(bag.alpha) + int(bag.beta);
this.callOut = bag.sum(3, 4, 5);
this.hasBefore = ("alpha" in bag) && !("missing" in bag);

bag.isAttribute = "public-prop";
this.publicIsAttr = String(bag.isAttribute);
delete bag.isAttribute;

var qn:QName = new QName("urn:neoflash:test", "qslot");
bag[qn] = 77;
this.qnameRead = int(bag[qn]);
this.qnameCaptured = bag.lastQName;
delete bag[qn];

var names:Array = [];
for (var k:String in bag) names.push(k);
this.iterNames = names.join(",");

var values:Array = [];
for each (var v:* in bag) values.push(v);
this.iterValues = values.join(",");

trace(bag);

bag.@badge = "gold";
this.attrRead = bag.@badge;
this.attrDeleted = delete bag.@badge;

this.alphaDeleted = delete bag.alpha;
this.hasAfter = ("alpha" in bag);
this.proxyLog = bag.log.join("|");

var rawObj:Object = { title: "Neo" };
var wrapped:ObjectProxy = new ObjectProxy(rawObj);
var propEvents:int = 0;
wrapped.addEventListener("propertyChange", function(ev:Event):void { propEvents++; });
wrapped.count = 4;
wrapped.count = 9;
trace(wrapped);
this.objectProxySummary = wrapped.title + ":" + wrapped.count + ":" + propEvents + ":" + ("count" in wrapped) + ":" + (wrapped.object_proxy::object === rawObj);

var fnDyn:Function = function(a:int, b:int):int { return a * b + 2; };
var fnEmpty:Function = new Function();
var fnEvalErrId:int = 0;
try { new Function("a", "b", "return a * b + 2;"); } catch (evalErr:EvalError) { fnEvalErrId = evalErr.errorID; }
var fnPrim:Function = function():Number { return Number(this) + 5; };
var extractedCall:Function = Function.prototype.call;
var extractedApply:Function = Function.prototype.apply;
function checkCaller():* { return arguments.caller; }
function outerCaller():* { return checkCaller(); }
var BoxCtor:Function = function(label:String):void { this.label = label; };
BoxCtor.prototype = { prefix: "box", format: function():String { return this.prefix + ":" + this.label; } };
var dynCtor:* = BoxCtor;
var boxInst:* = new dynCtor("ready");
this.fnSummary = [
  fnDyn(6, 7),
  fnDyn.length,
  fnEmpty() === undefined,
  fnEmpty.length,
  fnEvalErrId,
  fnDyn.call(null, 3, 5),
  fnDyn.apply(null, [4, 6]),
  fnPrim.call(37),
  extractedCall.call(fnDyn, null, 2, 8),
  extractedApply.call(fnDyn, null, [5, 5]),
  outerCaller() === undefined,
  boxInst.format()
].join("|");
`;
  const project={
    format_version:2,name:'ObjectProxy and Function',width:320,height:240,fps:24,background:[255,255,255,255],root:'scene',
    symbols:[{id:'scene',name:'Scene',kind:'movie_clip',timeline:{frame_count:1,layers:[],scripts:[{frame:0,script:{language:'action_script3',source:frameSource}}]}}],
    sources:[{id:'bag-proxy',path:'app/BagProxy.as',language:'action_script3',source:bagProxySource}],
  };
  try {
    const srcFile=resolve(directory,'object-proxy.nflash'),outDir=resolve(directory,'out');
    writeFileSync(srcFile,JSON.stringify(project));
    const exported=runCargo(cargo,['run','--quiet','--locked','-p','neoflash-export','--',srcFile,'--output',outDir],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(outDir,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(outDir,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    let scriptMessages;
    ({host, scriptMessages}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle}));
    host.dispatchScripts();
    const scene=host.proxyFor([]);
    assert.equal(scene.readSum,35);
    assert.equal(scene.callOut,'sum:12');
    assert.equal(scene.hasBefore,true);
    assert.equal(scene.publicIsAttr,'public-prop','public bag.isAttribute dispatches through Proxy getProperty/setProperty');
    assert.equal(scene.qnameRead,77,'Proxy bracket indexing preserves QName');
    assert.equal(scene.qnameCaptured,'urn:neoflash:test|qslot');
    const resolvedBag = await Promise.resolve(scene.bagRef);
    assert.equal(resolvedBag, scene.bagRef, 'Promise.resolve(proxy) bypasses .then without throwing');
    assert.equal(scene.iterNames,'alpha,beta');
    assert.equal(scene.iterValues,'10,25');
    assert.equal(scene.attrRead,'attr:gold');
    assert.equal(scene.attrDeleted,true);
    assert.equal(scene.alphaDeleted,true);
    assert.equal(scene.hasAfter,false);
    assert.match(scene.proxyLog,/set:alpha\|set:beta\|get:alpha\|get:beta\|call:sum:3\|has:alpha\|has:missing/);
    assert.match(scene.proxyLog,/set@badge\|get@badge\|del@badge\|del:alpha\|has:alpha/);
    assert.equal(scene.objectProxySummary,'Neo:9:2:true:true');
    assert.equal(scene.fnSummary,'44|2|true|0|1066|17|26|42|18|27|true|box:ready');
    const traceMsgs = scriptMessages.filter(m => m.kind === 'trace').map(m => m.message);
    assert.ok(traceMsgs.some(msg => msg.includes('BagProxy') && msg.includes('alpha: 10')), `trace(bag) formats Proxy safely, got: ${JSON.stringify(traceMsgs)}`);
    assert.ok(traceMsgs.some(msg => msg.includes('ObjectProxy') && msg.includes('Neo')), `trace(wrapped) formats ObjectProxy safely, got: ${JSON.stringify(traceMsgs)}`);
  } finally {
    host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});
  }
}
