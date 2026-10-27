// One ordinary publication: actual WASM/class host, authored JS bootstrap,
// synchronous calls in both directions, and actual HTTP requests. The small DOM
// boundary observes authored controls; the browser gate proves real navigation.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { createServer } from 'node:http';
import vm from 'node:vm';
import { createRuntimeHost } from './host-integration.mjs';

function authoredDOM(){
  class Element extends EventTarget{
    constructor(tag){super();this.tagName=tag.toUpperCase();this.children=[];this.parentNode=null;this.style={};this.attributes={};this.textContent='';this.id='';}
    append(...children){for(const child of children){child.remove();child.parentNode=this;this.children.push(child);}}
    remove(){if(this.parentNode){const list=this.parentNode.children;list.splice(list.indexOf(this),1);this.parentNode=null;}}
    setAttribute(key,value){this.attributes[key]=String(value);}
    getBoundingClientRect(){return{left:0,top:0,width:1040,height:700};}
    focus(){}
  }
  const body=new Element('body'),canvas=new Element('canvas'),footer=new Element('footer');canvas.id='stage';body.append(canvas,footer);
  const all=()=>{const nodes=[];function visit(node){nodes.push(node);node.children.forEach(visit);}visit(body);return nodes;};
  const document={createElement:tag=>new Element(tag),getElementById:id=>all().find(node=>node.id===id)||null,querySelector:tag=>all().find(node=>node.tagName===tag.toUpperCase())||null,body};
  return{canvas,document,global:{document},all};
}
async function until(predicate,label){const deadline=Date.now()+5000;while(!predicate()&&Date.now()<deadline)await new Promise(resolve=>setTimeout(resolve,5));assert.ok(predicate(),label);}
export async function verifyWebIntegration(root,cargo){
  const directory=mkdtempSync(resolve(root,'target/web-integration-'));let player,host,server;const requests=[],navigations=[],pending=[],errors=[];const dom=authoredDOM();
  try{
    const exported=spawnSync(cargo,['run','--quiet','--locked','-p','neoflash-export','--','examples/web-integration.nflash','--output',directory],{cwd:root,encoding:'utf8',maxBuffer:8*1024*1024});assert.equal(exported.status,0,exported.error?.message||exported.stderr);
    server=createServer((request,response)=>{let body='';request.setEncoding('utf8');request.on('data',part=>body+=part);request.on('end',()=>{requests.push({method:request.method,url:request.url,body});response.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});response.end('<!doctype html><title>Receipt</title><p>Accepted</p>');});});
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const baseURL=`http://127.0.0.1:${server.address().port}/index.html`;
    function navigate(prepared){navigations.push(prepared);const body=prepared.method==='POST'?new URLSearchParams(prepared.fields).toString():undefined;const task=fetch(prepared.url,{method:prepared.method,body,headers:body===undefined?undefined:{'Content-Type':'application/x-www-form-urlencoded'}}).then(response=>response.text());pending.push(task);}
    const sandbox=vm.createContext({TextEncoder,TextDecoder,WebAssembly,console,URL,Request,Response});vm.runInContext(readFileSync(resolve(directory,'bundle.js'),'utf8'),sandbox);const bundle=vm.runInContext('NeoFlashBundle',sandbox);
    vm.runInContext(readFileSync(resolve(directory,'neoflash_web.js'),'utf8'),sandbox);sandbox.wasmBytes=Buffer.from(bundle.wasm,'base64');const wasm=await vm.runInContext('NeoFlashWasm({module_or_path:wasmBytes}).then(()=>NeoFlashWasm)',sandbox);player=new wasm.WebPlayer(JSON.stringify(bundle.project));
    ({host}=createRuntimeHost(root,player,Array.from(bundle.scripts,item=>({script:{language:item.language,source:item.source},code:item.code,location:item.location})),{bundle,canvas:dom.canvas,external:{hostObject:dom.canvas,hostGlobal:dom.global,navigate,onDiagnostic:error=>errors.push(error)},net:{baseURL,navigate,onSendError:error=>errors.push(error)},context:{measureText:line=>({width:line.length*8})}}));
    host.dispatchScripts();let scene=host.proxyFor([]);
    assert.equal(requests.length,0,'startup works without any network request');assert.equal(scene.legacyReady,true);
    assert.equal(host.external().ExternalInterface.objectID,'stage');assert.equal(scene.ask3.caption.selectable,false);assert.equal(scene.legacyButton.caption.selectable,false);
    const html=id=>{const element=dom.document.getElementById(id);assert.ok(element,`authored bootstrap created ${id}`);element.dispatchEvent(new Event('click'));};
    const click=id=>{player.click(JSON.stringify([id]));host.dispatchScripts([id]);};
    html('html-to-as3');html('html-to-as2');assert.deepEqual([scene.total,scene.calls,scene.legacyTotal,scene.legacyCalls],[3,1,2,1]);assert.equal(scene.legacyPanel.value,2,'AS2 callback uses its supplied instance receiver');assert.match(dom.document.getElementById('web-host-output').textContent,/Returned AS2: score 2, calls 1/);
    const returned=dom.canvas.addPoints(4);assert.deepEqual(returned,{dialect:'AS3',total:7,calls:2,objectID:'stage'});returned.total=999;assert.equal(scene.total,7,'callback returns a copied plain value');
    click('ask3');click('ask2');assert.equal(scene.quoteScore,70);assert.match(scene.hostReply,/HTML reply #1/);assert.match(scene.legacyReply,/HTML reply #2 \/ score 20/);assert.equal(dom.global.NeoFlashExampleHost.calls,2,'dot-path call preserves the host object receiver');
    click('get-button');click('post-button');click('send-button');click('legacy-button');await Promise.all(pending);await until(()=>requests.length===4,'all navigation/form and fire-and-forget requests reach the actual local HTTP server');
    assert.equal(navigations.length,3);assert.ok(navigations.every(request=>request.target==='neoflashReceipt'));
    const received=requests.map(request=>{const url=new URL(request.url,baseURL);assert.equal(url.pathname,'/__neoflash_web_receipt__');const fields=request.method==='POST'?new URLSearchParams(request.body):url.searchParams;return{method:request.method,source:fields.get('source'),score:fields.get('score')};});
    assert.ok(received.some(item=>item.method==='GET'&&item.source==='AS3-navigation'&&item.score==='7'));
    assert.ok(received.some(item=>item.method==='POST'&&item.source==='AS2-LoadVars'&&item.score==='2'));
    assert.ok(received.some(item=>item.method==='POST'&&item.source==='AS3-sendToURL'&&item.score==='7'));
    assert.ok(received.some(item=>item.method==='GET'&&item.source==='AS2-getURL'));
    const retained3=dom.canvas.addPoints,retained2=dom.canvas.addLegacy,previousHost=dom.global.NeoFlashExampleHost;
    host.resetProject();scene=host.proxyFor([]);assert.deepEqual([scene.total,scene.calls,scene.legacyTotal,scene.legacyCalls],[0,0,0,0]);assert.equal(scene.legacyReady,true);
    assert.notEqual(dom.canvas.addPoints,retained3);assert.notEqual(dom.canvas.addLegacy,retained2);assert.notEqual(dom.global.NeoFlashExampleHost,previousHost);assert.equal(dom.all().filter(node=>node.id==='web-host-panel').length,1);
    assert.throws(()=>retained3(9),/session|stale|retired|replaced|generation|inactive/i);assert.throws(()=>retained2(9),/session|stale|retired|replaced|generation|inactive/i);
    html('html-to-as3');html('html-to-as2');assert.deepEqual([scene.total,scene.calls,scene.legacyTotal,scene.legacyCalls],[3,1,2,1]);assert.deepEqual(errors,[]);
    const last=dom.canvas.addPoints;host.disposeFamily();assert.equal(Object.hasOwn(dom.canvas,'addPoints'),false);assert.equal(Object.hasOwn(dom.canvas,'addLegacy'),false);assert.throws(()=>last(1),/session|stale|retired|replaced|disposed|generation|inactive/i);
  }finally{host?.disposeFamily();player?.free();await Promise.allSettled(pending);if(server)await new Promise(resolve=>server.close(resolve));rmSync(directory,{recursive:true,force:true});}
  console.log('Web integration: ordinary export + actual WASM, authored HTML bootstrap, AS3/AS2 synchronous callbacks, copied returns, local HTTP GET/POST/sendToURL and Restart passed.');
}
