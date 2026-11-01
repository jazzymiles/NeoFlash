/* AS2 Stage browser facilities. Rust owns validated state and viewport layout;
 * Fullscreen API and contextmenu are real browser operations, never fake flags. */
(function(global){'use strict';
function createMenus(options,command,builtinsEnabled){
 const {display,canvas,viewport}=options,doc=options.document||global.document;
 let generation=1,disposed=false,menuRecords=new WeakMap(),itemRecords=new WeakMap(),attached=new WeakMap(),panel=null;
 const builtins=['play','rewind','forward_back','print'];
 function live(){if(disposed)throw new Error('AS2 context menus belong to a disposed publication.');}
 function menuRecord(value){live();const record=menuRecords.get(value);if(!record)throw new TypeError('Expected a live ContextMenu.');return record;}
 function itemRecord(value){live();const record=itemRecords.get(value);if(!record)throw new TypeError('Expected a live ContextMenuItem.');return record;}
 function callback(value){if(value!==undefined&&value!==null&&typeof value!=='function')throw new TypeError('Context-menu callbacks require Function or null.');return value??null;}
 function itemState(value){return itemRecord(value).value;}
 class ContextMenuItem{
  constructor(caption,handler=null,separatorBefore=false,enabled=true,visible=true){
   live();const value=command({op:'menu_item',value:{caption,separatorBefore,enabled,visible}});
   itemRecords.set(this,{value,handler:callback(handler)});
  }
  get onSelect(){return itemRecord(this).handler;} set onSelect(value){itemRecord(this).handler=callback(value);}
  copy(){const r=itemRecord(this),v=r.value;return new ContextMenuItem(v.caption,r.handler,v.separatorBefore,v.enabled,v.visible);}
 }
 for(const key of ['caption','enabled','visible','separatorBefore'])Object.defineProperty(ContextMenuItem.prototype,key,{enumerable:true,get(){return itemState(this)[key];},set(value){const r=itemRecord(this);r.value=command({op:'menu_item',value:{...r.value,[key]:value}});}});
 class ContextMenu{
  constructor(handler=null){
   live();const flags={play:true,rewind:true,forward_back:true,print:true},object={};
   for(const name of builtins)Object.defineProperty(object,name,{enumerable:true,get(){live();return flags[name];},set(value){flags[name]=command({op:'menu_builtin',name,value});}});
   for(const name of ['zoom','quality','loop'])Object.defineProperty(object,name,{get(){throw new Error('ContextMenu builtInItems.'+name+' is unavailable in this browser profile.');},set(){throw new Error('ContextMenu builtInItems.'+name+' is unavailable in this browser profile.');}});
   menuRecords.set(this,{handler:callback(handler),items:[],flags,object});
  }
  get customItems(){return menuRecord(this).items;}
  set customItems(value){if(!Array.isArray(value)||value.length>15)throw new TypeError('ContextMenu.customItems requires an Array of at most 15 ContextMenuItem objects.');for(const item of value)itemRecord(item);menuRecord(this).items=value;}
  get builtInItems(){return menuRecord(this).object;}
  set builtInItems(value){if(!value||typeof value!=='object')throw new TypeError('ContextMenu.builtInItems requires an object.');const next={};for(const name of Object.keys(value))next[name]=command({op:'menu_builtin',name,value:value[name]});Object.assign(menuRecord(this).flags,next);}
  get onSelect(){return menuRecord(this).handler;}set onSelect(value){menuRecord(this).handler=callback(value);}
  hideBuiltInItems(){const r=menuRecord(this);for(const name of builtins)r.object[name]=false;}
  copy(){const r=menuRecord(this),copy=new ContextMenu(r.handler),c=menuRecord(copy);Object.assign(c.flags,r.flags);c.items=r.items.map(item=>item.copy());return copy;}
 }
 for(const [Type,records] of [[ContextMenu,()=>menuRecords],[ContextMenuItem,()=>itemRecords]])Object.defineProperties(Type,{__nfIs:{value:value=>records().has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native context-menu subclassing is unsupported.');}}});
 function menuRef(target,name){if(name!=='menu'||!display?.alive(target))return null;const owner=display.descriptor(target).owner;if(!['movie_clip','button','text'].includes(owner))return null;return {has:()=>true,get(){live();return attached.get(target);},set(value){live();if(value==null)attached.delete(target);else{menuRecord(value);attached.set(target,value);}return value;},delete(){attached.delete(target);return true;}};}
 function close(){panel?.remove();panel=null;}
 function fail(error){close();if(options.onError)options.onError(error);else throw error;}
 function targetMenu(event){let target=options.contextTarget?.(event)||options.getRoot?.();const root=options.getRoot?.();for(let current=target;current&&display?.alive(current);current=display.parentSpace(current)){const menu=attached.get(current);if(menu)return{owner:current,menu};}return root&&display?.alive(root)&&attached.has(root)?{owner:root,menu:attached.get(root)}:null;}
 function runBuiltin(action){const root=options.getRoot?.();if(!root||!display?.alive(root))throw new Error('Context-menu timeline target is unavailable.');switch(action){case'play':root.play();break;case'stop':root.stop();break;case'rewind':root.gotoAndStop(1);break;case'previous':root.prevFrame();break;case'next':root.nextFrame();break;case'print':if(typeof global.print!=='function')throw new Error('Browser printing is unavailable.');global.print();break;default:throw new Error('Unknown context-menu action.');}}
 function show(event){if(disposed)return;try{
  close();const selected=targetMenu(event);if(!selected)return;event.preventDefault();const {owner,menu}=selected,record=menuRecord(menu),born=generation;
  if(record.handler)Reflect.apply(record.handler,menu,[owner,menu]);
  if(disposed||born!==generation||!display.alive(owner))return;
  const items=record.items.slice(),flags=builtinsEnabled()?record.flags:Object.fromEntries(builtins.map(name=>[name,false])),rows=command({op:'menu_plan',items:items.map(itemState),builtInItems:flags});
  if(!rows.length){options.afterEvent?.();return;}
  if(!doc?.createElement||!viewport?.appendChild)throw new Error('Custom context menus require an HTML5 document.');
  const element=doc.createElement('div');element.setAttribute('role','menu');element.setAttribute('aria-label','Content menu');
  Object.assign(element.style,{position:'fixed',zIndex:'2147483647',left:event.clientX+'px',top:event.clientY+'px',minWidth:'180px',maxWidth:'380px',maxHeight:'80vh',overflowY:'auto',padding:'5px',border:'1px solid #79818c',borderRadius:'6px',background:'#ffffff',color:'#17212b',boxShadow:'0 4px 18px #0004',font:'14px system-ui',textAlign:'left'});
  for(const row of rows){
   if(row.separatorBefore){const separator=doc.createElement('div');separator.setAttribute('role','separator');Object.assign(separator.style,{height:'1px',margin:'4px',background:'#c7cdd4'});element.appendChild(separator);}
   const button=doc.createElement('button');button.type='button';button.setAttribute('role','menuitem');button.textContent=row.caption;button.disabled=!row.enabled;
   Object.assign(button.style,{display:'block',width:'100%',border:'0',borderRadius:'3px',padding:'7px 10px',background:'transparent',color:row.enabled?'#17212b':'#9299a1',font:'inherit',textAlign:'left',cursor:row.enabled?'pointer':'default'});
   button.addEventListener('click',event=>{event.preventDefault();event.stopPropagation();close();if(disposed||born!==generation||!display.alive(owner))return;try{if(row.action==='custom'){const item=items[row.index],record=itemRecord(item);if(!record.value.enabled||!record.value.visible)return;if(record.handler)Reflect.apply(record.handler,item,[owner,item]);}else runBuiltin(row.action);if(!disposed&&born===generation)options.afterEvent?.();}catch(error){fail(error);}});
   element.appendChild(button);
  }
  element.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();close();return;}if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){event.preventDefault();const buttons=[...element.querySelectorAll('button:not(:disabled)')];if(!buttons.length)return;const at=buttons.indexOf(doc.activeElement),next=event.key==='Home'?0:event.key==='End'?buttons.length-1:(at+(event.key==='ArrowDown'?1:-1)+buttons.length)%buttons.length;buttons[next].focus();}});
  panel=element;viewport.appendChild(element);const bounds=element.getBoundingClientRect();element.style.left=Math.max(0,Math.min(event.clientX,(global.innerWidth||bounds.right)-bounds.width))+'px';element.style.top=Math.max(0,Math.min(event.clientY,(global.innerHeight||bounds.bottom)-bounds.height))+'px';element.querySelector('button:not(:disabled)')?.focus();options.afterEvent?.();
 }catch(error){fail(error);}}
 const outside=event=>{if(panel&&!panel.contains(event.target))close();};
 canvas?.addEventListener?.('contextmenu',show);doc?.addEventListener?.('pointerdown',outside,true);global.addEventListener?.('blur',close);
 function reset(){generation++;close();menuRecords=new WeakMap();itemRecords=new WeakMap();attached=new WeakMap();}
 function dispose(){if(disposed)return;reset();canvas?.removeEventListener?.('contextmenu',show);doc?.removeEventListener?.('pointerdown',outside,true);global.removeEventListener?.('blur',close);disposed=true;}
 return {ContextMenu,ContextMenuItem,menuRef,reset,dispose};
}

function create(options={}){
 const{wasm,legacy,geometry,preview,viewport,canvas}=options;
 if(!wasm?.ScriptAS2Stage||!legacy?.Stage||!geometry?.Rectangle)throw new Error('AS2 Stage requires the Rust layout, legacy listeners and geometry services.');
 const width=options.width,height=options.height,doc=options.document||global.document;
 let core=new wasm.ScriptAS2Stage(width,height),disposed=false,generation=1,enabled=false,last=null,fullscreen=false,requestGeneration=0;
 const initialOverflow=viewport?.style?.overflow;
 function active(){if(disposed)throw new Error('AS2 Stage belongs to a disposed publication.');}
 function call(value){active();try{return JSON.parse(core.command(JSON.stringify(value)));}catch(error){throw new Error(String(error));}}
 function state(){return call({op:'get'});}
 function report(error){if(options.onError)options.onError(error);else throw error;}
 function ownsFullscreen(){return Boolean(viewport&&doc?.fullscreenElement===viewport);}
 function later(callback){const current=generation;queueMicrotask(()=>{if(!disposed&&generation===current)try{callback();}catch(error){report(error);}});}
 function layout(vw,vh){const result=call({op:'layout',width:vw,height:vh,fullscreen:ownsFullscreen()});const previous=last;last=result;if(previous&&(previous.logicalWidth!==result.logicalWidth||previous.logicalHeight!==result.logicalHeight)&&state().scaleMode==='noScale')later(()=>{legacy.broadcastStage('onResize',[]);options.afterEvent?.();});return result;}
 function activate(){if(!preview||!viewport)throw new Error('AS2 Stage sizing requires an HTML5 browser viewport.');enabled=true;viewport.style.overflow='hidden';preview.setStageLayout(layout);options.afterEvent?.();}
 function change(name,value){if(name==='fullScreenSourceRect'){if(value!==null&&!geometry.Rectangle.__nfIs(value))throw new TypeError('Stage.fullScreenSourceRect requires Rectangle or null.');if(value!==null)value={x:value.x,y:value.y,width:value.width,height:value.height};}const result=call({op:'set',name,value});if(name!=='showMenu')activate();return result;}
 function fullScreenChanged(){if(disposed)return;const next=ownsFullscreen();if(next&&requestGeneration!==generation){Promise.resolve(doc.exitFullscreen()).catch(()=>{});return;}if(next===fullscreen)return;fullscreen=next;if(enabled)preview.resize();legacy.broadcastStage('onFullScreen',[next]);options.afterEvent?.();}
 function display(value){active();if(!['normal','fullScreen'].includes(value))throw new TypeError('Stage.displayState requires normal or fullScreen.');if(value==='fullScreen'){
  if(ownsFullscreen())return;if(!viewport?.requestFullscreen||!doc?.fullscreenEnabled)throw new Error('This browser does not permit the Fullscreen API for this publication.');
  if(global.navigator?.userActivation&&!global.navigator.userActivation.isActive)throw new Error('Stage fullScreen requires a real user click or keyboard action.');activate();
 }else if(!ownsFullscreen())return;
 const born=generation;requestGeneration=born;let promise;try{promise=value==='fullScreen'?viewport.requestFullscreen():doc.exitFullscreen();}catch(error){throw new Error('Fullscreen request failed: '+String(error));}
 Promise.resolve(promise).then(()=>{if((disposed||born!==generation)&&ownsFullscreen())Promise.resolve(doc.exitFullscreen()).catch(()=>{});},error=>{if(!disposed&&born===generation)report(new Error('Fullscreen request was rejected: '+String(error)));});
 }
 function screenSize(name){active();const value=Number((options.screen||global.screen)?.[name]);if(!Number.isFinite(value)||value<=0)throw new Error('Browser monitor dimensions are unavailable.');return value;}
 function menu(event){if(!disposed&&!state().showMenu)event.preventDefault();}
 const Stage={get width(){active();return last?.logicalWidth??legacy.Stage.width;},get height(){active();return last?.logicalHeight??legacy.Stage.height;},addListener:listener=>legacy.Stage.addListener(listener),removeListener:listener=>legacy.Stage.removeListener(listener),get displayState(){active();return ownsFullscreen()?'fullScreen':'normal';},set displayState(value){display(value);},get fullScreenWidth(){return screenSize('width');},get fullScreenHeight(){return screenSize('height');}};
 for(const name of ['align','scaleMode','showMenu','fullScreenSourceRect'])Object.defineProperty(Stage,name,{enumerable:true,get(){const value=state()[name];return name==='fullScreenSourceRect'&&value?new geometry.Rectangle(value.x,value.y,value.width,value.height):value;},set(value){change(name,value);}});
 Object.defineProperty(Stage,'wmodeGPU',{enumerable:true,get(){return Boolean(state().wmodeGPU);},set(){throw new TypeError('Stage.wmodeGPU is read-only.');}});
 Object.freeze(Stage);
 const onFullscreen=()=>{try{fullScreenChanged();}catch(error){report(error);}};
 doc?.addEventListener?.('fullscreenchange',onFullscreen);canvas?.addEventListener?.('contextmenu',menu);
 const menus=createMenus(options,call,()=>state().showMenu);
 function reset(){if(disposed)return;menus.reset();generation++;if(ownsFullscreen())Promise.resolve(doc.exitFullscreen()).catch(()=>{});fullscreen=false;core.free();core=new wasm.ScriptAS2Stage(width,height);enabled=false;last=null;if(preview)preview.setStageLayout(null);if(viewport?.style)viewport.style.overflow=initialOverflow;}
 function dispose(){if(disposed)return;reset();menus.dispose();doc?.removeEventListener?.('fullscreenchange',onFullscreen);canvas?.removeEventListener?.('contextmenu',menu);core.free();disposed=true;}
 return Object.freeze({Stage,layout,ContextMenu:menus.ContextMenu,ContextMenuItem:menus.ContextMenuItem,menuRef:menus.menuRef,reset,dispose});
}
const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Stage=api;
})(globalThis);
