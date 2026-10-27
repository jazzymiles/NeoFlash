// One ordinary publication, measured with deterministic widths rather than a
// browser font-parity claim. Queries use viewport-local coordinates on BOTH
// axes: getCharBoundaries/getCharIndexAtPoint deliberately do not reproduce
// Flash 32's horizontal-scroll inconsistency (pinned Ruffle edit_text.rs).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

export async function verifyTextViewport(root,cargo) {
  const directory=mkdtempSync(resolve(root,'target/text-viewport-'));
  let player,host;
  try {
    const result=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/text-viewport.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});
    assert.equal(result.status,0,result.error?.message||result.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});
    vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);
    const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);
    sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');
    const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,context:{measureText:line=>({width:Array.from(line).length*10})}}));
    host.dispatchScripts();
    let scene=host.proxyFor([]);
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    const snapshot=value=>JSON.parse(player.scene_json()).find(item=>item.kind.type==='text_field'&&JSON.stringify(item.owner_ref)===JSON.stringify(value.toJSON().object_ref)).kind.field;
    const anchors=()=>[scene.leftTag.x,scene.centerTag.x+scene.centerTag.width/2,scene.rightTag.x+scene.rightTag.width];
    const near=(actual,expected)=>assert.ok(Math.abs(actual-expected)<1e-7,`${actual} should equal ${expected}`);
    const reader=scene.reader,legacy=scene.legacyReader;
    assert.equal(host.lastTrace(),'text viewport ready 1 0');
    assert.deepEqual([reader.scrollV,reader.scrollH,legacy.scrollV,legacy.scrollH],[1,0,1,0]);
    assert.ok(reader.maxScrollV>3&&legacy.maxScrollV>3&&legacy.maxScrollH>=50);
    assert.equal(reader.wordWrap,true);assert.equal(legacy.wordWrap,false);
    assert.equal(reader.background,true);assert.equal(reader.border,true);assert.equal(reader.backgroundColor,0x172333);
    assert.equal(scene.legacyTag.autoSize,'left');assert.equal(scene.legacyAutoResult,true,'native AS2 Boolean mapping preserves assignment RHS');
    assert.equal(scene.ordinaryAutoResult,true,'ordinary same-name user fields retain Boolean values');
    assert.deepEqual(anchors(),[80,520,950]);
    assert.ok(['scrollButton','growButton','inspectButton','resetButton'].every(name=>scene[name].caption.selectable===false));
    const originalText=reader.text,legacyText=legacy.text,initialWidth=scene.centerTag.width;
    click('scroll-button');
    assert.deepEqual([reader.scrollV,reader.scrollH,legacy.scrollV,legacy.scrollH],[3,0,3,50]);
    assert.ok(scene.scrollEvents>0&&scene.legacyScrolls>0);
    assert.equal(scene.legacyListenerScrolls,scene.legacyScrolls,'AS2 direct and registered callbacks receive the same changes');
    assert.equal(reader.text,originalText);assert.equal(legacy.text,legacyText);
    assert.ok(reader.bottomScrollV>=reader.scrollV&&reader.bottomScrollV<=reader.numLines);
    click('grow-button');
    assert.equal(scene.grows,1);assert.ok(scene.centerTag.width>initialWidth);
    anchors().forEach((value,index)=>near(value,[80,520,950][index]));
    near(scene.legacyTag.x,548);assert.equal(scene.legacyTag.autoSize,'left');
    click('inspect-button');
    assert.equal(scene.inspectCount,1);assert.equal(scene.inspectLine,reader.scrollV-1);
    assert.ok(scene.inspectIndex>=scene.paragraphStart&&scene.inspectIndex<scene.paragraphStart+scene.paragraphLength);
    assert.equal(host.program().is(scene.character,'flash.geom.Rectangle'),true);
    assert.equal(host.program().is(scene.metrics,'flash.text.TextLineMetrics'),true);
    assert.ok(scene.metrics.width>0&&scene.character.width>0&&scene.character.y>=0&&scene.character.y<reader.height);
    const copied=reader.getCharBoundaries(scene.inspectIndex),metricWidth=scene.metrics.width;
    scene.character.x+=1000;scene.metrics.width=0;
    near(reader.getCharBoundaries(scene.inspectIndex).x,copied.x);
    near(reader.getLineMetrics(scene.inspectLine).width,metricWidth);
    // The same native field is queried from AS3 below; it has already been
    // horizontally scrolled by its AS2 controller. No fabricated AS2 API call.
    const legacyIndex=legacy.getLineOffset(legacy.scrollV-1)+7;
    const visible=legacy.getCharBoundaries(legacyIndex);
    assert.ok(visible&&visible.x>=0&&visible.x<legacy.width);
    assert.equal(legacy.getCharIndexAtPoint(visible.x+visible.width/2,visible.y+visible.height/2),legacyIndex);
    assert.equal(legacy.getLineIndexAtPoint(visible.x+visible.width/2,visible.y+visible.height/2),legacy.scrollV-1);
    const field=snapshot(legacy),layout=host.text().layout(field),paint=[],clips=[];
    const consumer={save(){},restore(){},beginPath(){},rect(...args){clips.push(args);},clip(){},stroke(){paint.push('border');},fillRect(...args){paint.push(['background',...args]);},fillText(text,x,y){paint.push(['text',text,x,y]);}};
    host.text().draw(consumer,field);
    assert.ok(paint.some(item=>Array.isArray(item)&&item[0]==='background'));
    assert.ok(paint.includes('border'));assert.ok(clips.some(item=>JSON.stringify(item)==='[0,0,440,180]'));
    const first=layout.lines[legacy.scrollV-1];
    assert.ok(paint.some(item=>Array.isArray(item)&&item[0]==='text'&&item[1]===first.text&&item[2]===first.x-layout.scroll_x&&item[3]===first.y-layout.scroll_y),'paint consumes scrolled shared line positions');
    assert.equal(reader.text,originalText);assert.equal(reader.scrollV,3,'querying the other field does not move the reader');
    click('reset-button');
    assert.deepEqual([reader.scrollV,reader.scrollH,legacy.scrollV,legacy.scrollH,scene.grows],[1,0,1,0,0]);
    anchors().forEach((value,index)=>near(value,[80,520,950][index]));
    const oldReader=reader;host.resetProject();scene=host.proxyFor([]);
    assert.notEqual(scene.reader,oldReader);assert.equal(scene.reader.text,originalText);
    assert.deepEqual([scene.reader.scrollV,scene.legacyReader.scrollH,scene.grows,scene.inspectCount,scene.scrollEvents,scene.legacyScrolls],[1,0,0,0,0,0]);
    assert.throws(()=>oldReader.scrollV,/removed|replaced|another player|stale|lifetime|previous/i);
  } finally {host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Text viewport: AS3/AS2 scroll, anchored auto-size, background/border, copied line/character/paragraph geometry, shared paint positions and Restart passed (deterministic widths; no Flash glyph parity).');
}
