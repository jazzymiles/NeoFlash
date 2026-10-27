const assert = require('node:assert/strict');
const test = require('node:test');
const {localPoint, hitShape, oneBasedFrame, sourceKey} = require('../runtime.js');

test('inverse transforms rotated/scaled nested display coordinates', () => {
  const local = localPoint([0, 2, -3, 0, 100, 200], 85, 208);
  assert.deepEqual(local, [4,5]);
  assert.equal(localPoint([0,0,0,0,10,20],12,22), null);
});
test('ellipse picking excludes bounding box corners', () => {
  const ellipse={type:'ellipse',width:100,height:50};
  assert.equal(hitShape(ellipse,0,0),false);
  assert.equal(hitShape(ellipse,50,25),true);
  assert.equal(hitShape(ellipse,100,25),true);
  const transparent={...ellipse,fill:[50,100,150,0],stroke:[0,0,0,0],stroke_width:4};
  assert.equal(hitShape(transparent,50,25),false,'transformed alpha controls hit testing even when draw.alpha is one');
  assert.equal(hitShape({...transparent,fill:[50,100,150,128]},50,25),true,'alpha offsets can restore visible paint');
  assert.equal(hitShape({type:'rectangle',width:100,height:50,fill:[0,0,0,0],stroke:[0,0,0,0],stroke_width:0},50,25),false);
});
test('frame controls are one-based, labels take precedence, invalid targets fail', () => {
  assert.equal(oneBasedFrame(1,24,()=>-1),0);
  assert.equal(oneBasedFrame('intro',24,()=>7),7);
  assert.equal(oneBasedFrame('12',24,()=>-1),11);
  assert.throws(()=>oneBasedFrame(0,24,()=>-1),/between 1 and 24/);
  assert.throws(()=>oneBasedFrame(25,24,()=>-1),/between 1 and 24/);
  assert.throws(()=>oneBasedFrame('missing',24,()=>-1),/does not exist/);
});
test('compiled source lookup distinguishes dialect and exact source',()=>{
  assert.notEqual(sourceKey({language:'java_script',source:'play()'}),sourceKey({language:'action_script2',source:'play()'}));
});
