const assert=require('node:assert/strict');const test=require('node:test');
const {itemBounds,targetBounds,SemanticLayer}=require('../accessibility.js');
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
});
test('curved bounds use actual extrema plus affine stroke radius',()=>{
 const item={instance_path:['x'],transform:[2,0,0,3,10,20],alpha:1,kind:{type:'path',commands:[{op:'move_to',x:0,y:0},{op:'quadratic_to',cx:50,cy:100,x:100,y:0}],fill_rule:'nonzero',fill:[0,0,0,0],stroke:[255,255,255,255],stroke_width:4}};
 assert.deepEqual(itemBounds(item,measure),{left:6,top:14,right:214,bottom:176});
});
class Element{
 constructor(doc,tag){this.ownerDocument=doc;this.tagName=tag;this.children=[];this.style={};this.attrs={};this.events={};this.parentNode=null;this.disabled=false;this.textContent='';}
 setAttribute(k,v){this.attrs[k]=v;}getAttribute(k){return this.attrs[k]??null;}removeAttribute(k){delete this.attrs[k];}
 appendChild(el){this.insertBefore(el,null);return el;}
 insertBefore(el,before){el.remove();const i=before?this.children.indexOf(before):this.children.length;this.children.splice(i,0,el);el.parentNode=this;}
 remove(){if(this.parentNode){const p=this.parentNode;p.children.splice(p.children.indexOf(this),1);this.parentNode=null;if(this.ownerDocument.activeElement===this)this.ownerDocument.activeElement=null;}}
 addEventListener(k,fn){this.events[k]=fn;}focus(){this.ownerDocument.activeElement=this;this.events.focus?.();}scrollIntoView(){this.scrollCalls=(this.scrollCalls||0)+1;}
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
});
test('native-button adapter forwards current lifetime once and suppresses disabled activation',()=>{
 const container=dom(),received=[],manager=new SemanticLayer(container,n=>received.push(n)),project={width:300,height:200};
 manager.update([node()],[rectangle()],project,1,measure);const button=container.firstElementChild;
 assert.equal(button.events.keydown,undefined);assert.equal(button.events.keyup,undefined);
 button.events.click();assert.equal(received.length,1);assert.equal(received[0].lifetime_id,'1');
 const disabled=node();disabled.accessibility.disabled=true;manager.update([disabled],[rectangle()],project,1,measure);button.events.click();assert.equal(received.length,1);
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
});
