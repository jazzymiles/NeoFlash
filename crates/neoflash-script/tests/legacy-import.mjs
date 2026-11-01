// Legacy source through an ordinary export, actual WASM graph and host callbacks.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdtempSync,rmSync} from 'node:fs';
import {resolve} from 'node:path';
import vm from 'node:vm';
import {createRuntimeHost} from './host-integration.mjs';

const clockSource = `_root.onEnterFrame = function() {
\t// create an instance of the Date object
\tmyDate = new Date();
\t//
\t//rotate clock Hands and shadows
\thourHand._rotation = myDate.getHours()*30+(myDate.getMinutes()/2);
\thourHandShadow._rotation = myDate.getHours()*30+(myDate.getMinutes()/2);
\tminuteHand._rotation = myDate.getMinutes()*6+(myDate.getSeconds()/10);
\tminuteHandShadow._rotation = myDate.getMinutes()*6+(myDate.getSeconds()/10);
\tsecondHand._rotation = myDate.getSeconds()*6;
\tsecondHandShadow._rotation = myDate.getSeconds()*6;
};
`;
const transform={x:0,y:0,scale_x:1,scale_y:1,rotation:0,alpha:1};
const key=(frame,elements)=>({frame,elements,label:'',tween:null});
const element=(id,kind)=>({id,name:id,kind,transform:{...transform},scripts:[]});
const rect=()=>({type:'rectangle',width:40,height:50,fill:[255,0,0,255],stroke:[0,0,0,0],stroke_width:0});
const symbol=(id,kind,frames,keys,scripts=[])=>({id,name:id,kind,timeline:{frame_count:frames,layers:[{id:id+'-layer',name:id,visible:true,locked:false,keyframes:keys}],scripts}});
const instance=(id,symbol_id)=>element(id,{type:'instance',symbol_id,playback:'independent'});
const normalized=value=>value%360>180?value%360-360:value%360;
export async function verifyLegacyImport(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/legacy-import-runtime-'));let player,host;
  try{
    const hands=['hourhand','hourhandshadow','minutehand','minutehandshadow','secondhand','secondhandshadow'].map(name=>instance(name,'hand'));
    const button=instance('banner','button');button.transform.x=100;button.scripts=[{event:'load',script:{language:'action_script2',source:'on(release) { getURL(_root.link1); }'}}];
    const project={format_version:5,name:'Legacy runtime family',root:'scene',width:300,height:600,fps:35,background:[255,255,255,255],symbols:[
      symbol('scene','movie_clip',1,[key(0,[...hands,button])],[{frame:0,script:{language:'action_script1',source:clockSource}}]),
      symbol('hand','movie_clip',1,[key(0,[element('hand-shape',rect())])]),
      symbol('button','button',4,[key(0,[]),key(1,[]),key(2,[]),key(3,[element('hit',rect())])])
    ]};
    const path=resolve(directory,'legacy.nflash');writeFileSync(path,JSON.stringify(project));
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--',path,'--output',resolve(directory,'html')],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'html/bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    assert.equal(bundle.scripts.find(s=>s.language==='action_script1').source,clockSource,'publication keeps original source and language');
    vm.runInContext(readFileSync(resolve(directory,'html/neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    const navigations=[];
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,net:{baseURL:'https://example.test/index.html',navigate:request=>navigations.push(request)},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();const scene=host.proxyFor([]);scene.link1='https://example.test/banner';
    host.broadcastFrame();
    const time=scene.myDate,expected=[time.getHours()*30+time.getMinutes()/2,time.getMinutes()*6+time.getSeconds()/10,time.getSeconds()*6];
    for(const [index,name] of ['hourhand','minutehand','secondhand'].entries()){
      assert.ok(Math.abs(scene[name]._rotation-normalized(expected[index]))<1e-7,`original Clock source rotates ${name}`);
      assert.equal(scene[name]._rotation,scene[name+'shadow']._rotation);
    }
    assert.equal(scene.hourHand,undefined,'ordinary JS names remain case-sensitive');
    const names=['Ärger','hourhand'];assert.equal(wasm.legacy_case_name_json('HOURHAND',JSON.stringify(names)),'hourhand');assert.equal(wasm.legacy_case_name_json('ärger',JSON.stringify(names)),'Ärger');
    assert.equal(host.display().pick(110,10),scene.banner,'blank Up state still picks Hit geometry');
    assert.equal(host.display().pick(160,70),null);
    assert.ok(JSON.parse(player.runtime_snapshot_json()).draws.every(draw=>!draw.instance_path.includes('banner')));
    host.display().pointer('mouseDown',scene.banner,{button:0});host.display().pointer('mouseUp',scene.banner,{button:0});
    assert.equal(navigations.length,1,'AS2 on(release) registers once on Load');assert.equal(navigations[0].url,'https://example.test/banner');
    const before=scene.hourhand;host.resetProject();host.broadcastFrame();assert.notEqual(host.proxyFor([]).hourhand,before);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  await verifyAs1FullRuntime(root,cargo);
  console.log('Legacy import runtime: original Clock AS1 callbacks/case lookup, six hand rotations, source identity, invisible button Hit state, AS1 project/events/Flash 4 paths/Sound and AS2 release navigation passed.');
}

async function verifyAs1FullRuntime(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/as1-full-runtime-'));let player,host;
  try{
    const includeText=`function helperBoost(v) { var BOOSTED = v + 5; return boosted; }\n`;
    const initClipSource=`#initclip
#include "shared/as1_inc.as"
MovieClip.prototype.onLoad = function() {
  _root.protoOnLoadHits = (_root.protoOnLoadHits || 0) + 1;
};
function BaseGadget(seed) {
  this.total = HELPERBOOST(seed || 10);
}
BaseGadget.prototype.step = function(delta) {
  this.TOTAL = this.total + DELTA;
  return this.total;
};
function SubGadget(seed) {
  super(seed);
  this.kind = "sub";
}
SubGadget.prototype.__proto__ = BaseGadget.prototype;
SubGadget.prototype.step = function(delta) {
  return super.STEP(delta) + 1;
};
_global.BaseGadget = BaseGadget;
_global.SubGadget = SubGadget;
Object.registerClass("GadgetLink", SubGadget);
#endinitclip
this.orderLog = (this.orderLog || "") + "GF1:";
stop();
`;
    const actorClipScript=`onClipEvent(load) {
  _root.orderLog = (_root.orderLog || "") + "CL1:";
  this.loadHits = (this.loadHits || 0) + 1;
}
onClipEvent(LOAD) {
  _root.orderLog = _root.orderLog + "CL2:";
  this.loadHits = this.LOADHITS + 1;
}
onClipEvent(keyDown) {
  this.kdCount = (this.kdCount || 0) + 1;
  this.lastDownCode = Key.getCode();
  this.leftHeld = Key.isDown(Key.LEFT);
}
onClipEvent(keyUp) {
  this.kuCount = (this.kuCount || 0) + 1;
  this.lastUpCode = Key.getCode();
}
onClipEvent(data) {
  this.dataLog = (this.dataLog || "") + "D1:" + this.BONUS + ";";
}
onClipEvent(DATA) {
  this.dataLog = this.DATALOG + "D2:" + this.TAG + ";";
}
on(keyPress "<Space>") {
  this.spaceHits = (this.spaceHits || 0) + 1;
}
`;
    const navBtnScript=`on(press) {
  this.btnLog = (this.btnLog || "") + "P:";
}
on(release, keyPress "<Enter>") {
  this.btnLog = (this.btnLog || "") + "R1:";
  GOTOANDSTOP(2);
}
on(release) {
  this.btnLog = (this.btnLog || "") + "R2:";
}
on(keyPress "<Left>") {
  this.leftKeyHits = (this.leftKeyHits || 0) + 1;
}
on(KEYPRESS "a") {
  this.charAHits = (this.charAHits || 0) + 1;
}
`;
    const mainFrame0=`#include "shared/as1_inc.as"
stop();
var addItem = "flash";
var equal = "four";
var note = 7;
var order = 9;
var andGate = true;
this.f4Str = addItem add "-" add equal;
this.f4Bool = (this.F4STR eq "flash-four") and (this.f4Str ne "other")
  and ("a" lt "b") and ("b" gt "a") and ("a" le "a") and ("b" ge "b")
  and (not false or false) and (note <> order) and andGate;

var g = new SubGadget(20);
this.oopCheck = (g instanceof SubGadget) and (g instanceof BaseGadget);
this.oopStep = g.STEP(4);
this.gadgetStep = this.GADGET.STEP(3);
this.strictResult = StrictHelper.check();

set("/:rootScore", 50);
this.readRootScore = eval("/:ROOTSCORE");
set("/actor/inner:coins", 12);
this.readInnerCoins = eval("/ACTOR/INNER:COINS");

setProperty("/ACTOR", _X, 45);
setProperty("/actor", _ALPHA, 75);
this.actorX = getProperty("/actor", _x);
this.actorAlpha = getProperty("/ACTOR", _alpha);
this.actorName = getProperty("/ACTOR", _NAME);
this.actorTarget = getProperty("/ACTOR", _TARGET);
this.curFrameProp = getProperty("", _CURRENTFRAME);

tellTarget("/ACTOR/INNER") {
  set(":localVal", 33);
  set("../:siblingScore", eval(":LOCALVAL") + 7);
  set("../peer:flag", "ok");
  GOTOANDSTOP(2);
}

this.ACTOR.CREATEEMPTYMOVIECLIP("dynSub", 14);
tellTarget("/ACTOR") {
  setProperty("INNER", _X, 64);
  duplicateMovieClip("DYNSUB", "innerCopy", 15);
  setProperty("INNERCOPY", _Y, 28);
  startDrag("INNERCOPY");
  stopDrag();
}
this.dupY = getProperty("/actor/innerCopy", _Y);
tellTarget("/ACTOR") {
  removeMovieClip("INNERCOPY");
}
this.afterRemove = eval("/actor/innerCopy");

var s0 = new Sound();
var s1 = new Sound(this.ACTOR);
s1.setVolume(65);
this.soundVols = s0.getVolume() + ":" + s1.GETVOLUME();

this.passive.onKeyDown = function() {
  _root.unregisteredKeyHits = (_root.unregisteredKeyHits || 0) + 1;
};
this.triggerDataLoad = function() {
  loadVariables("https://example.test/vars.txt", "/ACTOR");
};
this.disableNavBtn = function() {
  this.NAVBTN.ENABLED = false;
};
`;

    const actorInst=instance('actor','actor_sym');
    actorInst.scripts=[{event:'load',script:{language:'action_script1',source:actorClipScript}}];
    const gadgetInst=instance('gadget','gadget_sym');
    const passiveInst=instance('passive','leaf_sym');
    const strictInst=instance('strictClip','strict_sym');
    const btnInst=instance('navBtn','button_sym');
    btnInst.transform.x=120;
    btnInst.scripts=[{event:'load',script:{language:'action_script1',source:navBtnScript}}];

    const gadgetSym={...symbol('gadget_sym','movie_clip',1,[key(0,[element('g-shape',rect())])],[{frame:0,script:{language:'action_script1',source:initClipSource}}]),linkage_id:'GadgetLink'};
    const actorSym=symbol('actor_sym','movie_clip',1,[key(0,[instance('inner','two_frame_sym'),instance('peer','leaf_sym')])],[{frame:0,script:{language:'action_script1',source:'_root.orderLog = (_root.orderLog || "") + "AF1:";'}}]);
    const twoFrameSym=symbol('two_frame_sym','movie_clip',2,[key(0,[element('tf-1',rect())]),key(1,[element('tf-2',rect())])],[{frame:0,script:{language:'action_script1',source:'stop();'}}]);
    const leafSym=symbol('leaf_sym','movie_clip',1,[key(0,[element('leaf-r',rect())])]);
    const strictSym=symbol('strict_sym','movie_clip',1,[key(0,[element('st-r',rect())])],[{frame:0,script:{language:'action_script2',source:'this.exactCase = 91; this.wrongCase = this.EXACTCASE;'}}]);
    const buttonSym=symbol('button_sym','button',4,[key(0,[element('b-up',rect())]),key(1,[]),key(2,[]),key(3,[element('b-hit',rect())])]);
    const mainSym=symbol('scene','movie_clip',2,[key(0,[actorInst,gadgetInst,passiveInst,strictInst,btnInst]),key(1,[actorInst,gadgetInst,passiveInst,strictInst,btnInst])],[
      {frame:0,script:{language:'action_script1',source:mainFrame0}},
      {frame:1,script:{language:'action_script1',source:'this.reachedFrame2 = helperBoost(10); stop();'}}
    ]);

    const project={
      format_version:5,name:'AS1 full runtime',root:'scene',width:300,height:300,fps:24,background:[255,255,255,255],
      resources:[{id:'inc-1',path:'shared/as1_inc.as',media_type:'text/plain',data_base64:Buffer.from(includeText,'utf8').toString('base64')}],
      sources:[{id:'src-as2',path:'StrictHelper.as',language:'action_script2',source:'class StrictHelper { public static function check():Number { var item:Number = 19; return item; } }'}],
      symbols:[mainSym,gadgetSym,actorSym,twoFrameSym,leafSym,strictSym,buttonSym]
    };
    const path=resolve(directory,'as1-full.nflash');writeFileSync(path,JSON.stringify(project));
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--',path,'--output',resolve(directory,'html')],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'html/bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'html/neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{
      bundle,
      net:{
        baseURL:'https://example.test/index.html',
        fetch:async()=>new Response('bonus=77&tag=loaded',{status:200,headers:{'Content-Type':'application/x-www-form-urlencoded'}}),
      },
      context:{measureText:line=>({width:line.length*8})}
    }));

    host.dispatchScripts();
    const scene=host.proxyFor([]);

    // 1. AS1 #include, #initclip, prototype OOP + super, Object.registerClass & AS2 case isolation.
    assert.equal(scene.f4Str,'flash-four');
    assert.equal(scene.f4Bool,true);
    assert.equal(scene.oopCheck,true);
    assert.equal(scene.oopStep,30,'SubGadget(20)->total=25, step(4)->super.step(4)+1=30');
    assert.equal(scene.gadget.kind,'sub','Object.registerClass ran SubGadget constructor on linked clip');
    assert.equal(scene.gadgetStep,19,'linked clip default total=15, step(3)->19');
    assert.equal(scene.strictResult,19);
    assert.equal(scene.strictClip.exactCase,91);
    assert.equal(scene.strictClip.wrongCase,undefined,'AS2 script in mixed project stays strictly case-sensitive');

    // 2. onClipEvent(load) order, chaining, one-shot semantics, and separation from MovieClip.prototype.onLoad.
    assert.equal(scene.orderLog,'CL1:CL2:AF1:','chained onClipEvent(load) fires in order before actor frame script');
    assert.equal(scene.actor.loadHits,2);
    assert.equal(scene.protoOnLoadHits,undefined,'onClipEvent(load) must not trigger MovieClip.prototype.onLoad');
    host.broadcastFrame();
    assert.equal(scene.actor.loadHits,2,'onClipEvent(load) fires only once on mount');

    // 3. Flash 4 slash/colon paths, unquoted properties, tellTarget actions, and new Sound(target).
    assert.equal(scene.rootScore,50);
    assert.equal(scene.readRootScore,50);
    assert.equal(scene.actor.inner.coins,12);
    assert.equal(scene.readInnerCoins,12);
    assert.equal(scene.actorX,45);
    assert.equal(scene.actorAlpha,75);
    assert.equal(scene.actorName,'actor');
    assert.equal(scene.actorTarget,'/actor');
    assert.equal(scene.curFrameProp,1);
    assert.equal(scene.actor.inner.localVal,33);
    assert.equal(scene.actor.siblingScore,40);
    assert.equal(scene.actor.peer.flag,'ok');
    assert.equal(scene.actor.inner._currentframe,2);
    assert.equal(scene.actor.inner._x,64);
    assert.equal(scene.dupY,28);
    assert.equal(scene.afterRemove,undefined);
    assert.equal(scene.soundVols,'100:65');

    // 4. Automatic stage onClipEvent(keyDown/keyUp) & Button/MovieClip on(keyPress) + chained on(press/release).
    host.stage().handleKey('keydown',{key:'ArrowLeft',code:'ArrowLeft',keyCode:37});
    host.stage().handleKey('keyup',{key:'ArrowLeft',code:'ArrowLeft',keyCode:37});
    assert.equal(scene.actor.kdCount,1);
    assert.equal(scene.actor.lastDownCode,37);
    assert.equal(scene.actor.leftHeld,true);
    assert.equal(scene.actor.kuCount,1);
    assert.equal(scene.actor.lastUpCode,37);
    assert.equal(scene.leftKeyHits,1,'Button on(keyPress "<Left>") writes to parent timeline');
    assert.equal(scene.unregisteredKeyHits,undefined,'dynamic mc.onKeyDown without Key.addListener must not auto-fire');

    host.stage().handleKey('keydown',{key:'a',code:'KeyA',keyCode:65});
    host.stage().handleKey('keyup',{key:'a',code:'KeyA',keyCode:65});
    assert.equal(scene.charAHits,1,'Button on(KEYPRESS "a") matches lowercase "a"');
    host.stage().handleKey('keydown',{key:'A',code:'KeyA',keyCode:65,shiftKey:true});
    host.stage().handleKey('keyup',{key:'A',code:'KeyA',keyCode:65,shiftKey:true});
    assert.equal(scene.charAHits,1,'Button on(KEYPRESS "a") is case-sensitive per single-char ASCII');

    host.stage().handleKey('keydown',{key:' ',code:'Space',keyCode:32});
    host.stage().handleKey('keyup',{key:' ',code:'Space',keyCode:32});
    assert.equal(scene.actor.spaceHits,1,'MovieClip on(keyPress "<Space>") dispatches on stage keydown');

    // Chained Button on(press) + multiple on(release) + on(release, keyPress "<Enter>") controlling parent timeline.
    host.display().pointer('mouseDown',scene.navBtn,{button:0});
    host.display().pointer('mouseUp',scene.navBtn,{button:0});
    host.dispatchScripts();
    assert.equal(scene.btnLog,'P:R1:R2:','chained on(press) and multiple on(release) run in declaration order');
    assert.equal(scene._currentframe,2,'Button authored on(release) gotoAndStop(2) advances parent timeline');
    assert.equal(scene.reachedFrame2,15);

    scene.gotoAndStop(1);
    host.dispatchScripts();
    assert.equal(scene._currentframe,1);
    host.stage().handleKey('keydown',{key:'Enter',code:'Enter',keyCode:13});
    host.stage().handleKey('keyup',{key:'Enter',code:'Enter',keyCode:13});
    assert.equal(scene._currentframe,2,'Button on(release, keyPress "<Enter>") triggers on Enter key');

    scene.disableNavBtn();
    host.stage().handleKey('keydown',{key:'ArrowLeft',code:'ArrowLeft',keyCode:37});
    host.stage().handleKey('keyup',{key:'ArrowLeft',code:'ArrowLeft',keyCode:37});
    assert.equal(scene.leftKeyHits,1,'disabled Button suppresses on(keyPress)');

    // 5. onClipEvent(data) fires in declaration order when compiled AS1 loadVariables delivers variables to the clip.
    scene.triggerDataLoad();
    await new Promise(r=>setTimeout(r,25));
    assert.equal(scene.actor.bonus,'77');
    assert.equal(scene.actor.tag,'loaded');
    assert.equal(scene.actor.dataLog,'D1:77;D2:loaded;');
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
}
