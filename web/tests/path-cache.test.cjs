const assert=require('node:assert/strict');
const test=require('node:test');
const {PathCache}=require('../path.js');
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
