const assert=require('node:assert/strict');const test=require('node:test');
const {itemBounds,targetBounds,SemanticLayer,applyDomAccessibility}=require('../accessibility.js');
const {layout}=require('../preview.js');
const rectangle=(path=['clip','shape'],matrix=[1,0,0,1,0,0])=>({instance_path:path,transform:matrix,alpha:1,kind:{type:'rectangle',width:40,height:20,fill:[255,0,0,255],stroke:[0,0,0,0],stroke_width:0}});
const measure=()=>30;
test('semantic bounds compose nested rotation/nonuniform scale and clip to stage',()=>{
 const item=rectangle(['clip','shape'],[0,2,-3,0,100,10]);
 assert.deepEqual(itemBounds(item,measure),{left:40,top:10,right:100,bottom:90});
 assert.deepEqual(targetBounds([item],['clip'],80,60,measure),{left:40,top:10,right:80,bottom:60});
 assert.equal(targetBounds([item],['other'],80,60,measure),null);
});
test('semantic targets omit offstage, transparent, empty and singular artwork',()=>{
 assert.equal(targetBounds([rectangle(['x'],[1,0,0,1,300,0])],['x'],200,100,measure),null);
 const transparent=rectangle();transparent.kind.fill[3]=0;
 assert.equal(itemBounds(transparent,measure),null);
 const hidden=rectangle();hidden.alpha=0;assert.equal(itemBounds(hidden,measure),null);
 assert.equal(itemBounds(rectangle(['x'],[0,0,0,1,0,0]),measure),null);
 assert.equal(targetBounds([],['x'],200,100,measure),null);
 const bitmap={...rectangle(['clip','pixels'],[0,2,-3,0,100,10]),kind:{type:'bitmap_pixels',width:16,height:16,alpha_bounds:{left:4,top:1,right:12,bottom:11}}};
 assert.deepEqual(itemBounds(bitmap,measure),{left:67,top:18,right:97,bottom:34},'pixel alpha bounds follow the full leaf transform');
 bitmap.kind.alpha_bounds=null;
 assert.equal(targetBounds([bitmap],['clip'],200,100,measure),null,'an entirely transparent bitmap creates no semantic target');
});
test('curved bounds use actual extrema plus affine stroke radius',()=>{
 const item={instance_path:['x'],transform:[2,0,0,3,10,20],alpha:1,kind:{type:'path',commands:[{op:'move_to',x:0,y:0},{op:'quadratic_to',cx:50,cy:100,x:100,y:0}],fill_rule:'nonzero',fill:[0,0,0,0],stroke:[255,255,255,255],stroke_width:4}};
 assert.deepEqual(itemBounds(item,measure),{left:6,top:14,right:214,bottom:176});
});
class Element{
 constructor(doc,tag){this.ownerDocument=doc;this.tagName=tag;this.children=[];this.style={};this.attrs={};this.events={};this.parentNode=null;this.disabled=false;this.textContent='';}
 setAttribute(k,v){this.attrs[k]=String(v);}getAttribute(k){return this.attrs[k]??null;}removeAttribute(k){delete this.attrs[k];}hasAttribute(k){return Object.hasOwn(this.attrs,k);}
 get tabIndex(){return this.hasAttribute('tabindex')?Number(this.attrs.tabindex):(['button','input','textarea','select'].includes(this.tagName.toLowerCase())?0:-1);}set tabIndex(v){this.setAttribute('tabindex',v);}
 appendChild(el){this.insertBefore(el,null);return el;}
 insertBefore(el,before){el.remove();const i=before?this.children.indexOf(before):this.children.length;this.children.splice(i,0,el);el.parentNode=this;}
 remove(){if(this.parentNode){const p=this.parentNode;p.children.splice(p.children.indexOf(this),1);this.parentNode=null;this.blur();}}
 addEventListener(k,fn){this.events[k]=fn;}
 focus(){const previous=this.ownerDocument.activeElement;if(previous&&previous!==this){this.ownerDocument.activeElement=null;previous.events?.blur?.({relatedTarget:this});}this.ownerDocument.activeElement=this;this.events.focus?.({relatedTarget:previous});}
 blur(){if(this.ownerDocument.activeElement===this){this.ownerDocument.activeElement=null;this.events.blur?.({relatedTarget:null});}}
 scrollIntoView(){this.scrollCalls=(this.scrollCalls||0)+1;}
 get firstElementChild(){return this.children[0]||null;}get nextElementSibling(){if(!this.parentNode)return null;return this.parentNode.children[this.parentNode.children.indexOf(this)+1]||null;}
}
function dom(){const doc={activeElement:null,createElement(tag){return new Element(doc,tag)}};return new Element(doc,'div');}
const node=(life='1',role='button',name='First')=>({instance_path:['clip'],lifetime_id:life,accessibility:{name,description:'Description',role,tab_order:0,disabled:false}});
test('moving/renaming a stable button preserves DOM identity and focus',()=>{
 const container=dom(),manager=new SemanticLayer(container,()=>{}),project={width:300,height:200};
 manager.update([node()],[rectangle()],project,1,measure);const button=container.firstElementChild;button.focus();
 manager.update([node('1','button','Renamed')],[rectangle(['clip','shape'],[1,0,0,1,20,30])],project,0.5,measure);
 assert.equal(container.firstElementChild,button);assert.equal(container.ownerDocument.activeElement,button);
 assert.equal(button.getAttribute('aria-label'),'Renamed');assert.equal(button.style.left,'10px');assert.equal(button.style.top,'15px');
});
test('new lifetime and role changes replace DOM targets; images have no activation',()=>{
 const container=dom(),manager=new SemanticLayer(container,()=>{}),project={width:300,height:200};
 manager.update([node()],[rectangle()],project,1,measure);const old=container.firstElementChild;old.focus();
 manager.update([node('2')],[rectangle()],project,1,measure);assert.notEqual(container.firstElementChild,old);assert.equal(container.ownerDocument.activeElement,null);
 manager.update([node('2','image')],[rectangle()],project,1,measure);const image=container.firstElementChild;
 assert.equal(image.tagName,'span');assert.equal(image.getAttribute('role'),'img');assert.equal(image.events.click,undefined);
 manager.update([],[],project,1,measure);assert.equal(container.children.length,0);
 // Two copies of one linked symbol have the same source path and different refs.
 const first={...node('1','button','First copy'),owner_ref:{id:'31',generation:'7'}},second={...node('1','button','Second copy'),owner_ref:{id:'32',generation:'7'}};
 const firstDraw={...rectangle(),owner_ref:{id:'41',generation:'7'},ancestry_refs:[{id:'41',generation:'7'},first.owner_ref]};
 const secondDraw={...rectangle(['clip','shape'],[1,0,0,1,100,0]),owner_ref:{id:'42',generation:'7'},ancestry_refs:[{id:'42',generation:'7'},second.owner_ref]};
 manager.update([first,second],[firstDraw,secondDraw],project,1,measure);
 const [firstButton,secondButton]=container.children;assert.notEqual(firstButton,secondButton);
 assert.equal(firstButton.style.left,'0px');assert.equal(secondButton.style.left,'100px');secondButton.focus();
 manager.update([second],[{...secondDraw,transform:[1,0,0,1,150,0]}],project,1,measure);
 assert.equal(container.firstElementChild,secondButton);assert.equal(secondButton.style.left,'150px');assert.equal(container.ownerDocument.activeElement,secondButton);
 manager.update([{...second,owner_ref:{id:'52',generation:'7'}}],[{...secondDraw,ancestry_refs:[{id:'42',generation:'7'},{id:'52',generation:'7'}]}],project,1,measure);
 assert.notEqual(container.firstElementChild,secondButton,'retired namespace placement cannot reuse the old semantic target');
});
test('native-button adapter forwards current lifetime once and suppresses disabled activation',()=>{
 const container=dom(),received=[],manager=new SemanticLayer(container,n=>received.push(n)),project={width:300,height:200};
 manager.update([node()],[rectangle()],project,1,measure);const button=container.firstElementChild;
 assert.equal(button.events.keydown,undefined);assert.equal(button.events.keyup,undefined);
 button.events.click();assert.equal(received.length,1);assert.equal(received[0].lifetime_id,'1');
 button.focus();
 const disabled=node();disabled.accessibility.disabled=true;manager.update([disabled],[rectangle()],project,1,measure);button.events.click();assert.equal(received.length,1);
 assert.equal(button.tabIndex,-1);assert.equal(button.getAttribute('aria-disabled'),'true');assert.equal(container.ownerDocument.activeElement,null);
 const renamed=node('1','button','Current target');manager.update([renamed],[rectangle()],project,1,measure);
 assert.equal(button.tabIndex,0);assert.equal(button.getAttribute('aria-disabled'),null);button.events.click();assert.equal(received.at(-1),renamed);
 manager.update([node('2')],[rectangle()],project,1,measure);button.events.click();assert.equal(received.length,2,'a retained handler cannot activate a retired lifetime');
 const replacement=container.firstElementChild;replacement.events.click();assert.equal(received.at(-1).lifetime_id,'2');
 const silent=node('2');silent.accessibility.silent=true;replacement.focus();manager.update([silent],[rectangle()],project,1,measure);
 replacement.events.click();assert.equal(received.length,3);assert.equal(container.children.length,0);assert.equal(container.ownerDocument.activeElement,null);
 manager.update([node('3')],[rectangle()],project,1,measure);const cleared=container.firstElementChild;manager.clear();cleared.events.click();assert.equal(received.length,3,'Restart/clear retires retained handlers too');
 const link=node('4','link','Open details');link.accessibility.activateButton=true;
 manager.update([link],[rectangle()],project,1,measure);const overridden=container.firstElementChild;
 assert.equal(overridden.tagName,'button');assert.equal(overridden.getAttribute('role'),'link');overridden.events.click();assert.equal(received.at(-1),link,'an explicit role override preserves original native-button activation');
});
test('Fit and 100 percent preserve authored pixels and expose scrollable narrow-stage area',()=>{
 const fit=layout(960,600,390,650,'fit');assert.ok(fit.stageWidth<=390);assert.ok(fit.stageHeight<=650);assert.equal(fit.spaceWidth,390);
 const actual=layout(960,600,390,650,'1');assert.equal(actual.scale,1);assert.equal(actual.stageWidth,960);assert.ok(actual.spaceWidth>390);assert.equal(actual.left,16);
 const zoom=layout(960,600,390,650,'2');assert.equal(zoom.stageWidth,1920);assert.ok(zoom.spaceHeight>650);
});
test('rectangle miter stroke includes affine-transformed square corner tips',()=>{
 const shape=rectangle(['x'],[1,1,-1,1,30,40]);shape.kind.stroke=[0,0,0,255];shape.kind.stroke_width=10;
 assert.deepEqual(itemBounds(shape,measure),{left:0,top:30,right:80,bottom:110});
});
test('separate offstage descendants do not create a fictitious semantic target',()=>{
 const left=rectangle(['clip','a'],[1,0,0,1,-200,0]),right=rectangle(['clip','b'],[1,0,0,1,300,0]);
 assert.equal(targetBounds([left,right],['clip'],200,100,measure),null);
 const visible=rectangle(['clip','c'],[1,0,0,1,50,10]);
 assert.deepEqual(targetBounds([left,visible],['clip'],200,100,measure),{left:50,top:10,right:90,bottom:30});
});
test('semantic updates measure each leaf once across multiple ancestor targets',()=>{
 const container=dom(),manager=new SemanticLayer(container,()=>{});let measurements=0;
 const leaf={instance_path:['clip','shape'],transform:[1,0,0,1,10,10],alpha:1,kind:{type:'text',text:'One measurement',font_family:'sans-serif',font_size:16,color:[255,255,255,255]}};
 const child={...node('2','image','Leaf description'),instance_path:['clip','shape']};
 manager.update([node(),child],[leaf],{width:300,height:200},1,()=>{measurements++;return 100});
 assert.equal(measurements,1);assert.equal(container.children.length,2);
});
test('zero-area fill-only primitives and collinear paths do not become ghost targets',()=>{
 const flat=rectangle(['x'],[1,1,-1,1,50,0]);flat.kind.width=0;assert.equal(itemBounds(flat,measure),null);
 const diagonal={...rectangle(['x']),kind:{type:'path',commands:[{op:'move_to',x:0,y:0},{op:'line_to',x:50,y:50}],fill_rule:'nonzero',fill:[255,0,0,255],stroke:[0,0,0,0],stroke_width:0}};
 assert.equal(itemBounds(diagonal,measure),null);
 diagonal.kind.stroke=[255,0,0,255];diagonal.kind.stroke_width=2;assert.ok(itemBounds(diagonal,measure));
 const bowtie={...diagonal,kind:{...diagonal.kind,stroke_width:0,commands:[{op:'move_to',x:0,y:0},{op:'line_to',x:50,y:50},{op:'line_to',x:0,y:50},{op:'line_to',x:50,y:0},{op:'close'}]}};
 assert.ok(itemBounds(bowtie,measure));
});
test('zoom/resize and lost pointer capture cancel pan without restoring stale scroll origins',()=>{
 const {Preview}=require('../preview.js');const events={},captured=new Set();
 const viewport={clientWidth:400,clientHeight:300,scrollLeft:0,scrollTop:0,classList:{toggle(){}},addEventListener(name,fn){events[name]=fn},setPointerCapture(id){captured.add(id)},hasPointerCapture(id){return captured.has(id)},releasePointerCapture(id){captured.delete(id)}};
 const preview=new Preview(viewport,{style:{}},{style:{}},{width:0,height:0},{width:1000,height:800},()=>{});
 preview.setMode('1');viewport.scrollLeft=200;
 const down={pointerId:1,pointerType:'mouse',button:1,clientX:100,clientY:100,preventDefault(){}};
 events.pointerdown(down);preview.setMode('2');const after=viewport.scrollLeft;
 events.pointermove({...down,clientX:100,clientY:100});assert.equal(viewport.scrollLeft,after);assert.equal(captured.size,0);
 events.pointerdown(down);events.lostpointercapture(down);events.pointermove({...down,clientX:20});assert.equal(viewport.scrollLeft,after);
});

test('authored reorder restores focused button without undoing manual pan',()=>{
 const container=dom(),manager=new SemanticLayer(container,()=>{}),a=node(),b={...node('2','button','Second'),instance_path:['other']};
 const scene=[rectangle(),rectangle(['other','shape'],[1,0,0,1,100,0])],project={width:300,height:200};
 manager.update([a,b],scene,project,1,measure);const focused=container.children[1];focused.focus();assert.equal(focused.scrollCalls,1);
 manager.update([b,a],scene,project,1,measure);assert.equal(container.ownerDocument.activeElement,focused);assert.equal(focused.scrollCalls,1);
 focused.focus();assert.equal(focused.scrollCalls,2);
 a.accessibility.tab_order=1;b.accessibility.tab_order=2;
 manager.update([b,a],scene,project,1,measure);assert.equal(container.children[0].getAttribute('aria-label'),'First');
 assert.equal(container.children[0].tabIndex,1);assert.equal(focused.tabIndex,2);assert.equal(container.ownerDocument.activeElement,focused);
 a.accessibility.reading_order=2;b.accessibility.reading_order=1;
 manager.update([a,b],scene,project,1,measure);assert.equal(container.children[0],focused,'explicit reading order is separate from keyboard tab order');assert.equal(focused.tabIndex,2);
 b.input={tab_enabled:false};manager.update([a,b],scene,project,1,measure);assert.equal(focused.tabIndex,-1);assert.equal(container.ownerDocument.activeElement,null);
});

test('semantic role/live transitions preserve current text and clear retired semantics',()=>{
 const container=dom(),manager=new SemanticLayer(container,()=>{}),project={width:300,height:200};
 const status=node('1','status','Ready');status.accessibility.live='polite';
 manager.update([status],[rectangle()],project,1,measure);const element=container.firstElementChild;
 assert.equal(element.getAttribute('role'),'status');assert.equal(element.getAttribute('aria-live'),'polite');assert.equal(element.tabIndex,-1);
 assert.equal(element.children[0].textContent,'Ready');
 status.accessibility.name='Saved';status.accessibility.live='assertive';status.accessibility.description='';
 manager.update([status],[rectangle()],project,1,measure);
 assert.equal(container.firstElementChild,element);assert.equal(element.children[0].textContent,'Saved');assert.equal(element.getAttribute('aria-live'),'assertive');assert.equal(element.getAttribute('aria-describedby'),null);
 status.accessibility.live='off';manager.update([status],[rectangle()],project,1,measure);
 assert.equal(element.getAttribute('aria-live'),'off');assert.equal(element.children[0].textContent,'');
 status.accessibility.role='image';manager.update([status],[rectangle()],project,1,measure);
 assert.notEqual(container.firstElementChild,element);assert.equal(container.firstElementChild.getAttribute('role'),'img');assert.equal(container.firstElementChild.tabIndex,-1);
 const child={...node('2','button','Nested action'),instance_path:['clip','shape']};
 status.accessibility.silent=true;manager.update([status,child],[rectangle()],project,1,measure);assert.equal(container.children.length,0,'a silent authored ancestor suppresses descendants too');
 status.accessibility.silent=false;status.accessibility.forceSimple=true;manager.update([status,child],[rectangle()],project,1,measure);assert.equal(container.children.length,1);
 status.accessibility.forceSimple=false;manager.update([status,child],[rectangle()],project,1,measure);assert.equal(container.children.length,2,'removing ancestor suppression restores eligible descendants');
 const parentRef={id:'1',generation:'1'},childRef={id:'2',generation:'1'},otherRef={id:'3',generation:'1'};
 const parent={...status,owner_ref:parentRef,accessibility:{...status.accessibility,silent:true}};
 const nested={...child,owner_ref:childRef,ancestry_refs:[childRef,parentRef]},other={...child,owner_ref:otherRef,ancestry_refs:[otherRef],accessibility:{...child.accessibility,name:'Other namespace'}};
 manager.update([parent,nested,other],[{...rectangle(),owner_ref:childRef,ancestry_refs:nested.ancestry_refs},{...rectangle(),owner_ref:otherRef,ancestry_refs:other.ancestry_refs}],project,1,measure);
 assert.equal(container.children.length,1);assert.equal(container.firstElementChild.getAttribute('aria-label'),'Other namespace','matching source paths in an unrelated namespace do not inherit suppression');
});

test('DOM metadata transitions remove stale roles, live state and keyboard eligibility',()=>{
 const container=dom(),element=container.ownerDocument.createElement('span');container.appendChild(element);
 const meta={name:'Updates',description:'Recent changes',role:'tab_panel',live:'polite',shortcut:'Alt+U',tab_order:3};
 applyDomAccessibility(element,meta);assert.equal(element.getAttribute('role'),'tabpanel');assert.equal(element.tabIndex,3);assert.equal(element.getAttribute('aria-live'),'polite');
 element.focus();applyDomAccessibility(element,{...meta,disabled:true});assert.equal(element.tabIndex,-1);assert.equal(element.getAttribute('aria-disabled'),'true');assert.equal(container.ownerDocument.activeElement,null);
 applyDomAccessibility(element,{...meta,silent:true});
 for(const name of ['role','aria-label','aria-description','aria-keyshortcuts','aria-live','aria-disabled'])assert.equal(element.getAttribute(name),null,name);
 assert.equal(element.getAttribute('aria-hidden'),'true');assert.equal(element.tabIndex,-1);
 applyDomAccessibility(element,{name:'Artwork',role:'image',tab_order:0});assert.equal(element.getAttribute('role'),'img');assert.equal(element.tabIndex,-1,'default order does not turn artwork into a keyboard stop');
 applyDomAccessibility(element,null);for(const name of ['role','aria-label','aria-hidden','aria-disabled','aria-live','tabindex'])assert.equal(element.getAttribute(name),null,name);
 const button=container.ownerDocument.createElement('button');container.appendChild(button);
 applyDomAccessibility(button,{role:'check_box',name:'Choice',live:'off'},{isInteractive:true});assert.equal(button.getAttribute('role'),'checkbox');assert.equal(button.getAttribute('aria-live'),'off');assert.equal(button.tabIndex,0);
 button.focus();applyDomAccessibility(button,{silent:true});assert.equal(button.tabIndex,-1,'a silent native control is excluded even without isInteractive');assert.equal(container.ownerDocument.activeElement,null);
 applyDomAccessibility(button,null,{isInteractive:true,explicitTabIndex:0});assert.equal(button.getAttribute('role'),null,'native button semantics return after an override is removed');assert.equal(button.tabIndex,0);
 applyDomAccessibility(button,{tab_order:4},{isInteractive:true,tabEnabled:false});assert.equal(button.tabIndex,-1);
 applyDomAccessibility(button,{tab_order:4},{isInteractive:true,explicitTabIndex:-1});assert.equal(button.tabIndex,4,'the runtime unset sentinel falls back to authored order');
 applyDomAccessibility(button,{live:'invalid',role:'auto'},{isInteractive:true});assert.equal(button.getAttribute('role'),null);assert.equal(button.getAttribute('aria-live'),null);assert.equal(button.tabIndex,0);
});

test('semantic native and programmatic focus share Stage events without reorder or retirement duplicates',()=>{
 const Events=require('../events.js'),{create}=require('../stage.js');
 const container=dom(),project={width:300,height:200},a={id:'a',node:node('a')},b={id:'b',node:{...node('b'),instance_path:['other']}};
 const targets=new Map([['a',a],['b',b]]),seen=[];let manager,stageHost;
 const hub=Events.create({alive:()=>true,parentOf:target=>target===stageHost?.stage?null:stageHost?.stage});
 for(const target of [a,b]){
   Object.assign(target,hub.methods(target));
   target.addEventListener('focusIn',event=>seen.push(['in',target.id,event.relatedObject?.id||null]));
   target.addEventListener('focusOut',event=>seen.push(['out',target.id,event.relatedObject?.id||null]));
 }
 stageHost=create({events:hub,width:300,height:200,frameRate:24,alive:()=>true,onFocusChanged:(next,previous)=>{
   manager?.blurTarget(previous?.node);return manager?.focusTarget(next?.node);
 }});
 manager=new SemanticLayer(container,()=>{}, {
   onFocus:(n,event)=>{if(!stageHost.changeFocus(targets.get(n.lifetime_id),'key',event))manager.focusTarget(stageHost.stage.focus?.node);},
   onBlur:n=>{if(stageHost.stage.focus===targets.get(n.lifetime_id))stageHost.stage.focus=null;},
 });
 const scene=[rectangle(),rectangle(['other','shape'],[1,0,0,1,100,0])];
 manager.update([a.node,b.node],scene,project,1,measure);const [first,second]=container.children;
 first.focus();second.focus();assert.equal(stageHost.stage.focus,b);
 assert.deepEqual(seen,[['in','a',null],['out','a','b'],['in','b','a']]);
 stageHost.stage.focus=a;assert.equal(container.ownerDocument.activeElement,first);assert.deepEqual(seen.slice(-2),[['out','b','a'],['in','a','b']]);
 let cancel=true;a.addEventListener('keyFocusChange',event=>{if(cancel)event.preventDefault();});
 const before=seen.length;second.focus();assert.equal(stageHost.stage.focus,a);assert.equal(container.ownerDocument.activeElement,first);assert.equal(seen.length,before,'canceled keyboard focus returns to the previous native target');cancel=false;
 manager.update([b.node,a.node],scene,project,1,measure);assert.equal(container.ownerDocument.activeElement,first);assert.equal(seen.length,before,'stable reordering emits no synthetic focus pair');
 a.node.accessibility.disabled=true;manager.update([b.node,a.node],scene,project,1,measure);assert.equal(stageHost.stage.focus,null);assert.deepEqual(seen.at(-1),['out','a',null]);
 second.focus();const focused=seen.length;manager.clear();assert.equal(stageHost.stage.focus,null);assert.equal(seen.length,focused+1);assert.deepEqual(seen.at(-1),['out','b',null]);
 second.events.focus({});second.events.blur({});assert.equal(stageHost.stage.focus,null);assert.equal(seen.length,focused+1,'retained handlers cannot refocus a cleared lifetime');
 stageHost.dispose();
});
