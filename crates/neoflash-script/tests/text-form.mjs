// One actual-WASM publication connects both dialects' input, selection and saving.
// Native DOM composition/selection is accepted separately in the exported browser.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';
export async function verifyTextForm(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/text-form-')),saved=new Map();let host,player;
  const storage={getItem:key=>saved.get(key)??null,setItem:(key,value)=>saved.set(key,String(value)),removeItem:key=>saved.delete(key)};
  try{
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/text-form.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);
    player=new wasm.WebPlayer(JSON.stringify(bundle.project));({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,storage:{storage,url:'https://example.invalid/forms/profile/index.html'},context:{measureText:line=>({width:Array.from(line).length*10})}}));host.dispatchScripts();
    let scene=host.proxyFor([]);const input=host.textInput(),nick=scene.nickField,note=scene.noteField;
    assert.deepEqual([nick.text,note.text,nick.type,note.multiline],['Pilot','Hello from NeoFlash','input',true]);
    host.stage().stage.focus=nick;nick.setSelection(0,5);
    input.commit(nick,{begin:0,end:5,text:'Анна 42'});assert.equal(nick.text,'Анна ');assert.equal(scene.changes,1);assert.equal(scene.inputs,1);
    input.commit(nick,{begin:5,end:5,text:'abcdefghijklmnopqrstuvwxyz'});assert.equal(nick.length,24);assert.equal(nick.caretIndex,24);
    const changed=scene.changes;nick.text='123'.repeat(10);assert.equal(nick.length,30,'scripted assignment bypasses restrict/maxChars');assert.equal(scene.changes,changed);
    scene.replaceName();assert.equal(nick.text,'Studio3123123123123123123123123');assert.equal(nick.caretIndex,6);assert.equal(scene.changes,changed,'scripted selection replacement does not invent a user CHANGE');
    scene.selectMessage();const selection=host.legacy().Selection;
    assert.equal(host.stage().stage.focus,note);assert.deepEqual([selection.getBeginIndex(),selection.getEndIndex()],[0,5]);assert.match(selection.getFocus(),/noteField$/);
    input.commit(note,{begin:0,end:5,text:'Привет'});assert.equal(note.text,'Привет from NeoFlash');assert.equal(scene.legacyChanges,1);assert.equal(scene.legacyFocus,1);
    const before=note.text,beforeChanges=scene.changes;input.commit(note,{begin:0,end:0,text:'#'});assert.equal(note.text,before);assert.equal(scene.changes,beforeChanges);assert.equal(scene.rejected,1);
    input.commit(note,{begin:0,end:note.length,text:'Line one\nLine two 😀'});assert.equal(note.text,'Line one\rLine two 😀');assert.equal(scene.legacyChanges,2);
    note.setSelection(0,4);note.replaceSelectedText('Page');assert.equal(note.text,'Page one\rLine two 😀');assert.equal(note.caretIndex,4);
    note.displayAsPassword=true;const field=host.text().info(JSON.stringify(note.toJSON().object_ref));assert.ok(host.text().layout(field).lines.every(line=>!/[A-Za-z😀]/u.test(line.text)),'password layout hides the committed characters');note.displayAsPassword=false;
    nick.text='Анна';scene.saveProfile();assert.equal(saved.size,1);const retired=note;
    host.resetProject();scene=host.proxyFor([]);assert.deepEqual([scene.nickField.text,scene.noteField.text],['Анна','Page one\rLine two 😀']);assert.equal(scene.changes,0);assert.equal(scene.legacyChanges,0);assert.throws(()=>retired.text,/removed|replaced|stale|session/i);
    host.stage().stage.focus=scene.noteField;host.stage().stage.focus=null;assert.equal(scene.legacyFocus,1);assert.equal(scene.legacyBlur,1);
    scene.clearProfile();assert.equal(saved.size,0);host.resetProject();scene=host.proxyFor([]);assert.deepEqual([scene.nickField.text,scene.noteField.text],['Pilot','Hello from NeoFlash']);
  }finally{host?.disposeFamily();player?.free();rmSync(directory,{recursive:true,force:true});}
  console.log('Text form: real Rust/WASM edits, user-only filters/limits, Unicode selection, cancelled input, focus callbacks, saved profile and Restart passed.');
}
