const assert=require('node:assert/strict');
const test=require('node:test');
const {PathCache,GradientRenderer}=require('../path.js');
const segment=x=>[{op:'move_to',x,y:0},{op:'line_to',x:x+20,y:30}];
function cache(options={}){let calls=0;return {cache:new PathCache({...options,pathFactory:()=>({allocation:++calls})}),calls:()=>calls}}

test('path cache evicts by byte budget before entry limit',()=>{
  const sample=cache({maxKeyBytes:200,maxEntries:100});
  const first=sample.cache.get(segment(0));
  const second=sample.cache.get(segment(100));
  assert.equal(sample.cache.get(segment(100)),second);
  assert.notEqual(sample.cache.get(segment(0)),first);
  assert.equal(sample.calls(),3);
  assert.ok(sample.cache.keyBytes<=200);
});
test('oversized paths bypass retention without evicting useful cached geometry',()=>{
  const sample=cache({maxKeyBytes:200});
  const small=sample.cache.get(segment(0));
  const large=[{op:'move_to',x:0,y:0},...Array.from({length:20},(_,x)=>({op:'line_to',x,y:x}))];
  assert.notEqual(sample.cache.get(large),sample.cache.get(large));
  assert.equal(sample.cache.get(segment(0)),small);
  assert.equal(sample.calls(),3);
  assert.ok(sample.cache.keyBytes<=200);
});
test('entry limit remains independent of generous byte budget',()=>{
  const sample=cache({maxKeyBytes:10000,maxEntries:1});
  const first=sample.cache.get(segment(0));
  sample.cache.get(segment(100));
  assert.notEqual(sample.cache.get(segment(0)),first);
  assert.equal(sample.calls(),3);
});
test('zero budgets disable retention and invalid budgets are rejected',()=>{
  const sample=cache({maxKeyBytes:0});
  assert.notEqual(sample.cache.get(segment(0)),sample.cache.get(segment(0)));
  assert.equal(sample.cache.keyBytes,0);
  assert.throws(()=>cache({maxKeyBytes:NaN}),/budgets/);
});
test('GradientRenderer renders bitmap fills in repeat and clamped-edge modes',()=>{
  const ops=[];
  const makeContext=(canvas)=>({
    canvas,globalAlpha:1,globalCompositeOperation:'source-over',imageSmoothingEnabled:false,fillStyle:'#000',
    _m:[1,0,0,1,0,0],_stack:[],
    save(){this._stack.push({m:this._m.slice(),alpha:this.globalAlpha,comp:this.globalCompositeOperation,smooth:this.imageSmoothingEnabled});},
    restore(){const s=this._stack.pop();this._m=s.m;this.globalAlpha=s.alpha;this.globalCompositeOperation=s.comp;this.imageSmoothingEnabled=s.smooth;},
    getTransform(){const[a,b,c,d,e,f]=this._m;return{a,b,c,d,e,f};},
    setTransform(a,b,c,d,e,f){this._m=[a,b,c,d,e,f];},
    transform(a,b,c,d,e,f){const[a0,b0,c0,d0,e0,f0]=this._m;this._m=[a0*a+c0*b,b0*a+d0*b,a0*c+c0*d,b0*c+d0*d,a0*e+c0*f+e0,b0*e+d0*f+f0];},
    clearRect(){},fill(){ops.push({op:'fill'});},clip(){ops.push({op:'clip'});},
    createPattern(img,mode){ops.push({op:'pattern',mode,smooth:this.imageSmoothingEnabled});return{pattern:mode,img};},
    fillRect(x,y,w,h){ops.push({op:'fillRect',x,y,w,h,style:this.fillStyle});},
    drawImage(...args){ops.push({op:'drawImage',args,smooth:this.imageSmoothingEnabled});},
  });
  const createCanvas=(w=32,h=32)=>{const c={width:w,height:h};c.getContext=()=>makeContext(c);return c;};
  const renderer=new GradientRenderer({createCanvas});
  const record={canvas:{width:2,height:2},width:2,height:2,bytes:new Uint8Array([255,0,0,255,0,255,0,255,0,0,255,255,255,255,0,255])};
  const bitmaps={paint:(paint,fn)=>fn(record)};
  const commands=[{op:'move_to',x:-4,y:-4},{op:'line_to',x:12,y:-4},{op:'line_to',x:12,y:12},{op:'line_to',x:-4,y:12},{op:'close'}];
  const dest=createCanvas(20,20).getContext('2d');
  renderer.draw(dest,{type:'gradient_path',commands,fill_rule:'nonzero',fill:[0,0,0,0],fill_gradient:null,
    fill_bitmap:{pixel_ref:{id:'1',generation:'1'},revision:1,width:2,height:2,alpha_bounds:{left:0,top:0,right:2,bottom:2},matrix:[1,0,0,1,0,0],repeat:true,smoothing:true},
    stroke:[0,0,0,0],stroke_gradient:null,stroke_width:0},{path:true},bitmaps);
  assert.ok(ops.some(o=>o.op==='pattern'&&o.mode==='repeat'&&o.smooth===true));
  ops.length=0;
  renderer.draw(dest,{type:'gradient_path',commands,fill_rule:'nonzero',fill:[0,0,0,0],fill_gradient:null,
    fill_bitmap:{pixel_ref:{id:'1',generation:'1'},revision:1,width:2,height:2,alpha_bounds:{left:0,top:0,right:2,bottom:2},matrix:[1,0,0,1,2,2],repeat:false,smoothing:false},
    stroke:[0,0,0,0],stroke_gradient:null,stroke_width:0},{path:true},bitmaps);
  assert.equal(ops.some(o=>o.op==='pattern'),false);
  assert.equal(ops.filter(o=>o.op==='drawImage').length,5,'clamped fill draws center + 4 stretched edge strips');
  assert.equal(ops.filter(o=>o.op==='fillRect').length,4,'clamped fill paints 4 corner quadrants');
});
