// One ordinary publication covers the Graphics family and retained display identity.
// Real Canvas pixels are accepted separately in the browser, not simulated here.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyGradientGraphics(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/gradient-graphics-'));
  let player,host;
  try {
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/dynamic-display.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();let scene=host.proxyFor([]);scene.timer.stop();
    const paints=value=>host.display().descriptor(value).graphics;
    const gradient=value=>paints(value).find(paint=>paint.fill_gradient);
    function initial() {
      assert.equal(scene.card.count,0);assert.equal(scene.card.parent,scene.leftPanel);
      assert.equal(scene.invalidGradientRejected,true);
      assert.equal(scene.leftPanel.numChildren,2);assert.equal(scene.rightPanel.numChildren,1);
      const face=gradient(scene.card),outline=paints(scene.card).find(paint=>paint.stroke_gradient);
      assert.equal(face.type,'gradient_path');assert.equal(face.fill_gradient.kind,'linear');
      assert.deepEqual(face.fill_gradient.stops.map(stop=>stop.color),[[83,193,212,255],[54,83,160,255]]);
      assert.equal(face.fill_gradient.matrix[4],120,'later source Matrix edits do not alter stored paint');
      assert.equal(outline.stroke_width,2);assert.equal(outline.stroke_gradient.stops.length,2);
      assert.equal(gradient(scene.card.getChildByName('badge')).fill_gradient.kind,'radial');
      assert.equal(gradient(scene.card.getChildByName('badge')).fill_gradient.focal_point_ratio,-0.35);
      assert.ok(face.commands.some(command=>command.op==='cubic_to'),'rounded geometry is shared cubic data');
      assert.ok(paints(scene.rightPanel).some(paint=>paint.commands.some(command=>command.op==='cubic_to')),'Vector drawPath and cubicCurveTo reach the runtime graph');
      assert.deepEqual(paints(scene.copiedPaint),paints(scene.card),'Graphics.copyFrom copies fill and stroke runs');
      const legacy=scene.getChildByName('legacy');
      assert.equal(gradient(legacy).fill_gradient.stops[0].color[3],255,'AS2 percentage alpha reaches the same paint');
      const bounds=scene.card.getBounds(scene.card);
      assert.deepEqual([bounds.x,bounds.y,bounds.width,bounds.height],[-1,-1,242,102]);
    }
    initial();const card=scene.card,oldGraphics=card.graphics,copied=JSON.stringify(paints(scene.copiedPaint));
    host.stage().handlePointer('click',{},card);
    assert.equal(card.count,1);assert.deepEqual(gradient(card).fill_gradient.stops[1].color,[141,76,156,255]);
    assert.equal(JSON.stringify(paints(scene.copiedPaint)),copied,'redrawing the original leaves the copied paint unchanged');
    host.stage().handleKey('keydown',{key:'ArrowRight',code:'ArrowRight',keyCode:39});
    assert.equal(card.parent,scene.rightPanel);assert.equal(scene.card,card);
    host.stage().handlePointer('click',{},card);assert.equal(card.count,2);
    host.stage().handleKey('keydown',{key:' ',code:'Space',keyCode:32});assert.equal(card.parent,null);
    host.stage().handleKey('keydown',{key:' ',code:'Space',keyCode:32});assert.equal(card.parent,scene.leftPanel);
    host.stage().handleKey('keydown',{key:'ArrowUp',code:'ArrowUp',keyCode:38});assert.equal(scene.card,card);assert.equal(card.count,2);
    const before=JSON.stringify(paints(card));
    assert.throws(()=>card.graphics.drawPath([1,6],[0,0,1]),/data|coordinate|command|length/i);
    assert.equal(JSON.stringify(paints(card)),before,'a malformed path cannot append partial drawing');
    host.resetProject();scene=host.proxyFor([]);scene.timer.stop();initial();
    assert.notEqual(scene.card,card);assert.throws(()=>oldGraphics.clear(),/stale|previous|removed|replaced|live|another player/i);
    host.stage().handlePointer('click',{},scene.card);assert.equal(scene.card.count,1);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Gradient Graphics: published linear/radial fill and stroke, cubic/round/Vector paths, copied paint, AS2 alpha, retained identity and Restart passed.');
}
