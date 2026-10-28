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
  console.log('Legacy import runtime: original Clock AS1 callbacks/case lookup, six hand rotations, source identity, invisible button Hit state and AS2 release navigation passed.');
}
