/* Browser locale metadata only. All authored numeric formatting/parsing and
 * mutable formatter state are owned by Rust. Fixed Intl probes never receive an
 * authored number or input string. */
(function(global){
 'use strict';
 const writable=['decimalSeparator','digitsType','fractionalDigits','groupingPattern','groupingSeparator','leadingZero','negativeNumberFormat','negativeSymbol','trailingZeros','useGrouping'];
 const readonly=['actualLocaleIDName','requestedLocaleIDName','lastOperationStatus'];
 function metadata(requested){
  if(typeof requested!=='string')throw new TypeError('NumberFormatter requires a locale identifier String.');
  if(requested.length>256)throw new RangeError('NumberFormatter locale identifier exceeds 256 characters.');
  const name=requested==='i-default'?undefined:requested.replace(/_/g,'-');
  let formatter,status='noError';
  try{formatter=new Intl.NumberFormat(name,{useGrouping:true,minimumFractionDigits:2,maximumFractionDigits:2});if(name&&Intl.NumberFormat.supportedLocalesOf([name]).length===0)status='usingFallbackWarning';}
  catch{formatter=new Intl.NumberFormat(undefined,{useGrouping:true,minimumFractionDigits:2,maximumFractionDigits:2});status='usingDefaultWarning';}
  const parts=formatter.formatToParts(-123456789.5),integers=parts.filter(p=>p.type==='integer').map(p=>Array.from(p.value).length);
  const last=integers.at(-1)||3,previous=integers.at(-2)||last;
  const actual=formatter.resolvedOptions().locale;
  const zero=new Intl.NumberFormat(actual,{useGrouping:false,maximumFractionDigits:0}).formatToParts(0).find(p=>p.type==='integer')?.value||'0';
  return {requestedLocaleIDName:requested,actualLocaleIDName:actual,decimalSeparator:parts.find(p=>p.type==='decimal')?.value||'.',groupingSeparator:parts.find(p=>p.type==='group')?.value||',',groupingPattern:last===previous?`${last};*`:`${last};${previous};*`,negativeSymbol:parts.find(p=>p.type==='minusSign')?.value||'-',digitsType:zero.codePointAt(0),lastOperationStatus:status};
 }
 function create(wasm){
  if(typeof wasm?.ScriptNumberFormatter!=='function')throw new Error('Rust NumberFormatter bindings are missing; rebuild and re-export.');
  const states=new WeakMap(),results=new WeakMap();
  const state=value=>{const result=states.get(value);if(!result)throw new TypeError('Expected NumberFormatter.');return result;};
  const call=(value,method,...args)=>{try{return state(value)[method](...args);}catch(error){throw new RangeError(String(error));}};
  class NumberParseResult {
   constructor(value,startIndex=0x7fffffff,endIndex=0x7fffffff){results.set(this,{value:Number(value),startIndex:Number(startIndex)|0,endIndex:Number(endIndex)|0});Object.seal(this);}
  }
  for(const name of ['value','startIndex','endIndex'])Object.defineProperty(NumberParseResult.prototype,name,{enumerable:true,get(){const r=results.get(this);if(!r)throw new TypeError('Expected NumberParseResult.');return r[name];}});
  class NumberFormatter {
   constructor(locale){try{states.set(this,new wasm.ScriptNumberFormatter(JSON.stringify(metadata(locale))));Object.seal(this);}catch(error){throw new RangeError(String(error));}}
   formatNumber(value){return call(this,'format',Number(value),'number');}
   formatInt(value){return call(this,'format',Number(value),'int');}
   formatUint(value){return call(this,'format',Number(value),'uint');}
   parse(text){if(text===null||text===undefined)throw new TypeError('NumberFormatter.parse requires a String.');const r=JSON.parse(call(this,'parse',String(text),false));return new NumberParseResult(r.value===null?NaN:r.value,r.startIndex,r.endIndex);}
   parseNumber(text){if(text===null||text===undefined)throw new TypeError('NumberFormatter.parseNumber requires a String.');const r=JSON.parse(call(this,'parse',String(text),true));return r.value===null?NaN:r.value;}
   static getAvailableLocaleIDNames(){throw new Error('The browser cannot enumerate every supported locale. Construct NumberFormatter with a requested locale and inspect actualLocaleIDName.');}
  }
  for(const name of [...writable,...readonly])Object.defineProperty(NumberFormatter.prototype,name,{enumerable:true,get(){return JSON.parse(call(this,'snapshot'))[name];},...(writable.includes(name)?{set(value){call(this,'set_property',name,JSON.stringify(value));}}:{})});
  for(const [Constructor,registry]of [[NumberFormatter,states],[NumberParseResult,results]])Object.defineProperties(Constructor,{__nfIs:{value:value=>registry.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Number formatting native subclassing is not supported.');}}});
  return Object.freeze({NumberFormatter,NumberParseResult});
 }
 const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashNumberFormat=api;
})(globalThis);
