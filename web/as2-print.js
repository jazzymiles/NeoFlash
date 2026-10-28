/* AS2 PrintJob: Rust owns virtual page policy; browser facilities display actual
 * captured PNG pages and the native print preview. No printer facts are invented. */
(function(global){
'use strict';
function create(wasm,options={}){
 if(typeof wasm?.ScriptAS2Print!=='function'||typeof options.capture!=='function')throw new Error('PrintJob requires Rust platform policy and the shared scene capture.');
 const records=new WeakMap(),jobs=new Set();let generation=1,disposed=false,active=null;
 const gesture=options.isUserGesture||(()=>global.navigator?.userActivation?.isActive===true);
 function report(error){if(options.onError)options.onError(error);else global.console?.error(error);}
 function state(value){const r=records.get(value);if(disposed||!r||r.generation!==generation)throw new Error('PrintJob belongs to a previous publication session.');return r;}
 function invoke(r,q){try{return JSON.parse(r.core.execute(JSON.stringify(q))).value;}catch(e){throw e instanceof Error?e:new TypeError(String(e));}}
 function retire(r){for(const url of r.urls)global.URL.revokeObjectURL(url);r.urls=[];r.window?.close();r.window=null;r.core.free();}
 function decorate(r){
  const doc=r.window.document;doc.title='NeoFlash print preview';doc.body.textContent='';
  const style=doc.createElement('style');style.textContent='@page{size:595pt 842pt;margin:36pt}body{margin:0;background:#e5e7eb;font:14px system-ui}.toolbar{padding:16px;position:sticky;top:0;background:white}.toolbar button{margin-right:12px;padding:8px 14px}.page{box-sizing:border-box;width:523pt;height:770pt;overflow:hidden;margin:20px auto;background:white;break-after:page}.page:last-child{break-after:auto}.page img{display:block;max-width:none;max-height:none}@media print{body{background:white}.toolbar{display:none}.page{margin:0}}';doc.head.append(style);
  const toolbar=doc.createElement('div');toolbar.className='toolbar';
  const print=doc.createElement('button');print.textContent='Print';print.disabled=true;print.addEventListener('click',()=>{if(!r.window?.closed)r.window.print();});r.printButton=print;
  const close=doc.createElement('button');close.textContent='Close preview';close.addEventListener('click',()=>r.window?.close());
  const label=doc.createElement('span');label.textContent='A4 portrait · 595 × 842 pt · printer settings are controlled by your browser';toolbar.append(print,close,label);doc.body.append(toolbar);
 }
 class PrintJob{
  constructor(){if(disposed)throw new Error('Print service disposed.');if(active&&!active.sent&&active.window&&!active.window.closed)throw new Error('Finish the active PrintJob before creating another.');if(jobs.size>=16)throw new Error('PrintJob limit is 16 jobs before Restart.');const r={receiver:this,generation,core:new wasm.ScriptAS2Print(),window:null,urls:[],images:[],sent:false};records.set(this,r);jobs.add(r);}
  start(){
   const r=state(this);if(!gesture())throw new Error('PrintJob.start requires an explicit user click to open its preview.');if(r.window)throw new Error('PrintJob has already started.');if(active&&!active.sent&&active.window&&!active.window.closed)throw new Error('Another PrintJob is active.');
   const win=(options.openWindow||(()=>global.open('about:blank','_blank','popup,width=900,height=800')))();
   if(!win)return invoke(r,{op:'start',opened:false});
   r.window=win;try{win.opener=null;decorate(r);if(!invoke(r,{op:'start',opened:true})){win.close();r.window=null;return false;}active=r;return true;}catch(error){win.close();r.window=null;throw error;}
  }
  addPage(target,printArea=null,settings=null,frameNum=undefined){
   const r=state(this);if(!r.window||r.window.closed||r.sent)return invoke(r,{op:'fail'});
   if(settings?.printAsBitmap!==true)throw new TypeError('This PrintJob profile requires {printAsBitmap:true}; vector printing is not implemented.');
   if(frameNum!==undefined&&frameNum!==null&&frameNum!==0)throw new TypeError('PrintJob prints the current runtime frame; explicit timeline frame selection is not implemented.');
   const receiver=options.resolveTarget?options.resolveTarget(target):target;
   const area=printArea??options.bounds?.(receiver);if(!area)throw new TypeError('PrintJob requires a current MovieClip and a bounded print area.');
   const plan=invoke(r,{op:'plan',area});if(!plan)return false;
   try{
    const bytes=options.capture(receiver,plan);if(!(bytes instanceof Uint8Array)||!bytes.length)throw new TypeError('Print capture must return actual PNG bytes.');
    const url=global.URL.createObjectURL(new Blob([bytes],{type:'image/png'}));r.urls.push(url);const doc=r.window.document,page=doc.createElement('section'),img=doc.createElement('img');page.className='page';img.alt='Printed page '+plan.page;img.style.width=plan.width+'pt';img.style.height=plan.height+'pt';img.src=url;page.append(img);doc.body.append(page);
    if(!invoke(r,{op:'commit',bytes:bytes.length})){page.remove();global.URL.revokeObjectURL(url);r.urls.pop();return false;}r.images.push(img);return true;
   }catch(error){invoke(r,{op:'fail'});report(error);return false;}
  }
  send(){
   const r=state(this);if(!r.window||r.window.closed)throw new Error('Print preview was closed before send().');invoke(r,{op:'send'});r.sent=true;if(active===r)active=null;const epoch=generation;
   Promise.all(r.images.map(img=>typeof img.decode==='function'?img.decode():img.complete?Promise.resolve():new Promise((resolve,reject)=>{img.addEventListener('load',resolve,{once:true});img.addEventListener('error',()=>reject(new Error('Print page image failed to load.')),{once:true});}))).then(()=>{if(disposed||epoch!==generation||r.window?.closed)return;r.printButton.disabled=false;r.window.focus();r.window.print();}).catch(error=>{if(!disposed&&epoch===generation)report(error);});
  }
 }
 for(const name of ['orientation','pageWidth','pageHeight','paperWidth','paperHeight'])Object.defineProperty(PrintJob.prototype,name,{get(){return invoke(state(this),{op:'properties'})[name];},set(){throw new TypeError('PrintJob.'+name+' is read-only.');}});
 Object.defineProperties(PrintJob,{__nfIs:{value:value=>{const r=records.get(value);return Boolean(r&&!disposed&&r.generation===generation);}},__nfInitialize:{value:()=>{throw new TypeError('PrintJob native subclassing is not implemented.');}}});
 function reset(){for(const r of jobs)retire(r);jobs.clear();active=null;generation++;}
 return{PrintJob,reset,dispose(){if(disposed)return;reset();disposed=true;}};
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Print=api;
})(globalThis);
