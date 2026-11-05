// A single published composition spans clipping, linked identity and both dialects.
// Pointer geometry and clipped semantic silhouettes are checked in the browser.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyClippedScene(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/clipped-scene-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/clipped-scene.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();let scene=host.proxyFor([]);
    const ref=value=>value.toJSON().object_ref,key=value=>JSON.stringify(value);
    const rect=value=>[value.x,value.y,value.width,value.height];
    function check(phase) {
      assert.equal(scene.phase,phase);assert.equal(scene.legacyPhase,phase);
      assert.equal(scene.cards.length,5);assert.equal(host.program().type('app.ScrollCard').created,5);
      assert.equal(scene.rectangleCopied,true);assert.equal(scene.badMaskRejected,true);assert.equal(scene.badScrollRejected,true);
      assert.equal(scene.legacyAbsent,true);assert.equal(scene.legacyMaskAccepted,true);
      assert.deepEqual(rect(scene.viewport.scrollRect),[phase*155,0,450,230]);
      assert.deepEqual(rect(scene.viewport.getBounds(scene)),[60,220,450,230],'scrolling keeps the viewport bounds fixed');
      assert.equal(scene.viewport.transform.matrix.tx,60,'scrolling does not rewrite authored/base transform');
      const Point=host.program().type('flash.geom.Point');
      assert.equal(scene.cards[0].localToGlobal(new Point()).x,80-phase*155);
      assert.equal(scene.panel.mask,phase===2?null:scene.aperture);
      assert.equal(scene.rightViewport.scrollRect===null,phase===2);
      const snapshot=JSON.parse(player.runtime_snapshot_json()),objects=new Map(snapshot.objects.map(node=>[key(node.object_ref),node]));
      function descendant(reference,owner) {for(let node=objects.get(key(reference));node;node=objects.get(key(node.parent_ref)))if(key(node.object_ref)===key(owner))return true;return false;}
      const panelDraws=snapshot.draws.filter(draw=>descendant(draw.owner_ref,ref(scene.panel)));
      assert.ok(panelDraws.length>0);
      assert.ok(panelDraws.every(draw=>(draw.clip_ids||[]).length===(phase===2?0:2)),'mask and ancestor scroll rectangle form one intersected clip chain');
      if(phase!==2){
        assert.ok(snapshot.clips.some(clip=>clip.components.length===2),'one mask combines both filled vector components');
        assert.equal(snapshot.draws.some(draw=>descendant(draw.owner_ref,ref(scene.aperture))),false,'consumed mask geometry never paints independently');
      }
      const first=scene.cards[0];assert.equal(first.index,1);assert.equal(first.parent,scene.viewport);
    }
    check(0);const first=scene.cards[0],mask=scene.aperture,oldViewport=scene.viewport;
    host.stage().handlePointer('click',{},first);assert.equal(scene.selected,1);assert.equal(first.hits,1);
    const advance=()=>{player.click(JSON.stringify(['advance-button']));host.dispatchScripts(['advance-button']);};
    advance();check(1);assert.equal(scene.cards[0],first);assert.equal(first.hits,1);
    advance();check(2);assert.equal(scene.legacyPanel.scrollRect,null,'the public host/AS3 surface uses null; the AS2 source separately observed undefined');
    advance();check(0);assert.equal(scene.aperture,mask);assert.equal(scene.cards[0],first);
    host.resetProject();scene=host.proxyFor([]);check(0);
    assert.notEqual(scene.cards[0],first);assert.notEqual(scene.aperture,mask);
    assert.throws(()=>oldViewport.scrollRect,/stale|previous|removed|replaced|live|another player/i);
    host.stage().handlePointer('click',{},scene.cards[0]);assert.equal(scene.cards[0].hits,1);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  await verifySoftMaskSources(root,cargo);
  console.log('Clipped scene: published compound mask unions, nested scroll clips, copied Rectangle state, AS2 absence/setMask, linked identity and Restart passed.');
}


// Authored paint enters the same masks through both language facades. Keep this
// in the shared clipping workflow; Canvas pixel acceptance lives in path-browser.
export async function verifySoftMaskSources(root,cargo,runtime=null) {
  const directory=mkdtempSync(resolve(root,'target/clipped-soft-sources-'));
  const transform=(x=0)=>({x,y:0,scale_x:1,scale_y:1,rotation:0,alpha:1});
  const rectangle=[{op:'move_to',x:0,y:0},{op:'line_to',x:100,y:0},{op:'line_to',x:100,y:100},{op:'line_to',x:0,y:100},{op:'close'}];
  const element=(id,kind)=>({id,name:id,transform:transform(),kind,scripts:[]});
  const target=id=>element(id,{type:'instance',symbol_id:'target-art',playback:'independent'});
  const targetShape=element('target-fill',{type:'rectangle',width:100,height:100,fill:[240,80,40,255],stroke:[0,0,0,0],stroke_width:0});
  const gradient=element('maskGradient',{type:'gradient_path',commands:rectangle,fill_rule:'nonzero',fill:[0,0,0,255],stroke:[0,0,0,0],stroke_width:0,
    fill_gradient:{kind:'linear',matrix:[100/1638.4,0,0,1,50,0],focal_point_ratio:0,stops:[{ratio:0,color:[0,0,0,0]},{ratio:255,color:[0,0,0,255]}]}});
  const bitmap=element('maskBitmap',{type:'bitmap',resource_id:'alpha-image',width:4,height:1,smoothing:false});
  const layer=(id,elements)=>({id,name:id,visible:true,locked:false,keyframes:[{frame:0,label:'',tween:null,elements}]});
  try {
    for(const language of ['action_script2','action_script3']) {
      const isAS2=language==='action_script2';
      const assignment=(target,mask)=>isAS2?`this.${target}.setMask(this.${mask});`:`this.${target}.mask=this.${mask};`;
      const source=`stop();this.targetGradient.cacheAsBitmap=true;this.maskGradient.cacheAsBitmap=true;this.targetBitmap.cacheAsBitmap=true;this.maskBitmap.cacheAsBitmap=true;${assignment('targetGradient','maskGradient')}${assignment('targetBitmap','maskBitmap')}this.masksReady=true;`;
      const animated=layer('gradient',[gradient]);animated.keyframes[0].tween='linear';animated.keyframes.push({frame:2,label:'',tween:null,elements:[{...gradient,transform:transform(20)}]});
      const project={format_version:8,name:'Cached authored mask sources',width:160,height:120,fps:24,background:[0,0,0,0],root:'scene',resources:[{
        id:'alpha-image',path:'alpha.png',media_type:'image/png',data_base64:'iVBORw0KGgoAAAANSUhEUgAAAAQAAAABCAYAAAD5PA/NAAAAEUlEQVR4nGNggIBQIF4FxP8BB18B/xuQSbAAAAAASUVORK5CYII='
      }],symbols:[{id:'scene',name:'Main',kind:'movie_clip',timeline:{frame_count:3,layers:[layer('content',[target('targetGradient'),target('targetBitmap'),bitmap]),animated],scripts:[{frame:0,script:{language,source}}]}},{id:'target-art',name:'Target artwork',kind:'movie_clip',timeline:{frame_count:1,layers:[layer('target-art-layer',[targetShape])],scripts:[]}}]};
      const file=resolve(directory,`${language}.nflash`),output=resolve(directory,language);writeFileSync(file,JSON.stringify(project));
      const args=['run','--quiet','--locked','-p','neoflash-export','--',file,'--output',output];if(runtime)args.push('--runtime',runtime);
      const exported=spawnSync(cargo,args,{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});assert.equal(exported.status,0,exported.error?.message||exported.stderr);
      const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
      vm.runInContext(readFileSync(resolve(output,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
      vm.runInContext(readFileSync(resolve(output,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
      const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
      const player=new wasm.WebPlayer(JSON.stringify(bundle.project));let host;
      try {
        ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle}));
        host.dispatchScripts();assert.equal(host.proxyFor([]).masksReady,true,language);
        const check=x=>{
          const snapshot=JSON.parse(player.runtime_snapshot_json());
          const components=snapshot.clips.flatMap(clip=>clip.alpha_components??[]);
          const ramp=components.find(component=>component.fill_gradient),png=components.find(component=>component.fill_bitmap);
          assert.ok(ramp,`${language}: authored gradient survives cached mask`);
          assert.deepEqual(ramp.fill_gradient.stops.map(stop=>stop.color[3]),[0,255]);assert.equal(ramp.matrix[4],x);
          assert.ok(png,`${language}: decoded authored PNG survives cached mask`);
          const bytes=Array.from(player.bitmap_pixels(JSON.stringify(png.fill_bitmap.pixel_ref)));
          assert.deepEqual(bytes.filter((_,i)=>i%4===3),[0,85,170,255]);
          assert.equal(snapshot.draws.some(draw=>['maskGradient','maskBitmap'].includes(draw.element_id)),false);
        };
        check(0);player.control('[]','goto_and_stop',1);host.dispatchScripts();check(10);
        player.control('[]','goto_and_stop',2);host.dispatchScripts();check(20);
        host.resetProject();assert.equal(host.proxyFor([]).masksReady,true);check(0);
      }finally{host?.disposeFamily();player.free();}
    }
  }finally{rmSync(directory,{recursive:true,force:true});}
  console.log('Clipped sources: AS2/AS3 authored gradient and decoded PNG alpha, tweened positions and Restart passed.');
}
