/* Uniform TextField/TextFormat profile over the shared Rust display graph.
 * Primary contracts:
 * https://airsdk.dev/reference/actionscript/3.0/flash/text/TextField.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/text/TextFormat.html
 * TextFormat null slots mean "leave unchanged". Current text and insertion
 * defaults stay separate. HTML, embedded fonts and mixed
 * character-format runs are deliberately outside this profile.
 */
(function(global){
  'use strict';
  const formats=new WeakMap(),legacyFormats=new WeakSet();
  const fields=['font','size','color','bold','italic','underline','url','target','align','leftMargin','rightMargin','indent','leading'];
  const supported=new Set(['font','size','color','bold','italic','align','leading']);
  const legacySupported=new Set([...supported,'underline','leftMargin','rightMargin','indent','blockIndent','letterSpacing','kerning','bullet','tabStops','url','target']);
  function formatValue(name,value,legacy=false){
    if(value===null||value===undefined)return null;
    if(!(legacy?legacySupported:supported).has(name))throw new TypeError(`TextFormat.${name} is not supported by the uniform text profile.`);
    if(name==='font'){if(typeof value!=='string'||!value.length||new TextEncoder().encode(value).length>1024||/[\u0000-\u001f\u007f-\u009f]/.test(value))throw new TypeError('TextFormat.font requires a nonempty font-family string of at most 1024 UTF-8 bytes without controls.');}
    else if(name==='size'){if(typeof value!=='number'||!Number.isFinite(value)||value<=0||value>10000)throw new RangeError('TextFormat.size requires a finite number greater than zero through 10000.');}
    else if(name==='color'){if(!Number.isInteger(value)||value<0||value>0xffffff)throw new RangeError('TextFormat.color requires an RGB integer from zero through 0xffffff.');}
    else if(name==='bold'||name==='italic'||name==='underline'||name==='kerning'||name==='bullet'){if(typeof value!=='boolean')throw new TypeError(`TextFormat.${name} requires a Boolean or null.`);}
    else if(name==='align'){if(!['left','center','right'].includes(value))throw new TypeError('TextFormat.align supports left, center and right; justified layout is pending.');}
    else if(['leftMargin','rightMargin','indent','blockIndent'].includes(name)){if(typeof value!=='number'||!Number.isFinite(value)||Math.abs(value)>10000||(name!=='indent'&&value<0))throw new RangeError(`TextFormat.${name} requires a bounded paragraph length.`);}
    else if(name==='letterSpacing'){if(typeof value!=='number'||!Number.isFinite(value)||Math.abs(value)>1000)throw new RangeError('letterSpacing requires -1000 through1000.');}
    else if(name==='url'||name==='target'){if(typeof value!=='string'||value.length>(name==='url'?4096:256)||/[\u0000-\u001f\u007f]/.test(value))throw new TypeError('TextFormat url/target require bounded strings.');}
    else if(name==='tabStops'){if(!Array.isArray(value)||value.length>64||value.some((v,i)=>!Number.isFinite(v)||v<0||v>100000||(i>0&&v<=value[i-1])))throw new TypeError('tabStops require at most64 increasing nonnegative lengths.');return value.slice();}
    else if(name==='leading'){if(typeof value!=='number'||!Number.isFinite(value)||Math.abs(value)>10000)throw new RangeError('TextFormat.leading requires a finite number from -10000 through 10000.');}
    return value;
  }
  class TextFormat {
    constructor(...args){
      if(args.length>fields.length)throw new TypeError('TextFormat accepts at most thirteen format arguments.');
      const values={};fields.forEach((name,index)=>{values[name]=formatValue(name,args[index]);});
      formats.set(this,values);Object.seal(this);
    }
  }
  for(const name of fields)Object.defineProperty(TextFormat.prototype,name,{enumerable:true,get(){const value=formats.get(this);if(!value)throw new TypeError('Expected TextFormat.');return value[name];},set(value){const current=formats.get(this);if(!current)throw new TypeError('Expected TextFormat.');current[name]=formatValue(name,value,legacyFormats.has(this));}});
  for(const name of ['blockIndent','bullet','kerning','letterSpacing','tabStops','display'])Object.defineProperty(TextFormat.prototype,name,{get(){throw new TypeError(`TextFormat.${name} is not supported by the uniform text profile.`);},set(){throw new TypeError(`TextFormat.${name} is not supported by the uniform text profile.`);}});
  TextFormat.__nfIs=value=>formats.has(value);
  TextFormat.__nfInitialize=()=>{throw new TypeError('Native TextFormat subclassing is not supported.');};
  Object.defineProperty(TextFormat,Symbol.hasInstance,{value:TextFormat.__nfIs});
  class TextFormatAlign {constructor(){throw new TypeError('TextFormatAlign supplies alignment constants.');}}
  Object.assign(TextFormatAlign,{LEFT:'left',CENTER:'center',RIGHT:'right',JUSTIFY:'justify'});Object.freeze(TextFormatAlign);
  class TextFieldType {constructor(){throw new TypeError('TextFieldType supplies INPUT and DYNAMIC constants.');}}
  Object.assign(TextFieldType,{INPUT:'input',DYNAMIC:'dynamic'});Object.freeze(TextFieldType);
  function formatPatch(value){
    const record=formats.get(value);if(!record)throw new TypeError('Expected TextFormat from this runtime.');
    return Object.fromEntries(Object.entries(record).filter(([,item])=>item!==null));
  }
  function formatCopy(value){const result=new TextFormat();for(const name of supported)if(value[name]!==undefined)result[name]=value[name];return result;}
  function cssFont(format){return `${format.italic?'italic ':''}${format.bold?'bold ':''}${format.size}px ${JSON.stringify(format.font)}, sans-serif`;}
  function applyFont(context,format,measurement=false){
    context.font=cssFont(format);
    if('fontKerning' in context)context.fontKerning=format.kerning===null||format.kerning===undefined?'auto':format.kerning?'normal':'none';
    else if(format.kerning!==null&&format.kerning!==undefined)throw new Error('Explicit kerning requires Canvas fontKerning support in this browser.');
    if('letterSpacing' in context)context.letterSpacing=measurement?'0px':(format.letterSpacing||0)+'px';
    else if(!measurement&&format.letterSpacing)throw new Error('letterSpacing requires Canvas letterSpacing support in this browser.');
  }
  function string(value){
    if(typeof value!=='string')throw new TypeError('TextField text requires a string; convert other values explicitly with String().');
    for(let i=0;i<value.length;i++){const unit=value.charCodeAt(i);if(unit>=0xd800&&unit<=0xdbff){const low=value.charCodeAt(++i);if(!(low>=0xdc00&&low<=0xdfff))throw new RangeError('TextField text must preserve complete Unicode characters.');}else if(unit>=0xdc00&&unit<=0xdfff)throw new RangeError('TextField text must preserve complete Unicode characters.');}
    return value.replace(/\r\n?|\n/g,'\n');
  }
  const publicText=value=>value.replace(/\n/g,'\r');
  const UNHANDLED=Symbol('unhandled text member');
  function create({player,measureText,onUpdate=()=>{}}={}){
    if(!player||typeof measureText!=='function')throw new TypeError('TextField requires the shared Player and font measurement service.');
    let as2Execute=null,as2Layout=null,as2Wheel=null;
    class As2TextFormat extends TextFormat {
      constructor(...args){
        super();if(args.length>fields.length)throw new TypeError('AS2 TextFormat accepts at most thirteen arguments.');
        legacyFormats.add(this);const values={};for(let i=0;i<fields.length;i++)values[fields[i]]=formatValue(fields[i],args[i],true);values.blockIndent=null;values.letterSpacing=null;values.kerning=null;values.bullet=null;values.tabStops=null;formats.set(this,values);
      }
      getTextExtent(value,width){
        if(!as2Execute)throw new Error('AS2 text kernel is missing; re-export the project.');
        if(typeof value!=='string'||(width!==undefined&&(!Number.isFinite(width)||width<0)))throw new TypeError('getTextExtent requires text and an optional finite nonnegative wrap width.');
        const packet=as2Execute({op:'extent_field',text:value,format:formatPatch(this),width:width===undefined?null:width});
        return as2Execute({op:'extent_result',layout:layout(packet.field),format:packet.field.format});
      }
    }
    for(const name of ['blockIndent','letterSpacing','kerning','bullet','tabStops'])Object.defineProperty(As2TextFormat.prototype,name,{get(){const value=formats.get(this)[name];return Array.isArray(value)?value.slice():value;},set(value){formats.get(this)[name]=formatValue(name,value,true);}});
    function legacyFormatCopy(value){const result=new As2TextFormat();for(const name of legacySupported)if(value[name]!==undefined)result[name]=value[name];return result;}
    let disposed=false,cacheBytes=0;const cache=new Map(),maxCacheBytes=8*1024*1024;
    const active=()=>{if(disposed)throw new Error('This text service has been disposed.');};
    function info(ref){active();return JSON.parse(player.object_text_info(ref));}
    function update(ref,patch){active();if(!player.object_text_update(ref,JSON.stringify(patch)))throw new Error('TextField update requires a live text object.');as2Layout?.(ref);onUpdate(ref);}
    function layout(field){
      active();const key=JSON.stringify(field),existing=cache.get(key);
      if(existing){cache.delete(key);cache.set(key,existing);return existing.value;}
      const value=JSON.parse(player.text_layout_json(key,(strings,format)=>{
        const style=JSON.parse(format);return JSON.stringify(JSON.parse(strings).map(text=>measureText(text,style)));
      }));
      // Keys and the returned line strings both own text. Bound retained memory,
      // not just entry count, and skip caching exceptionally large layouts.
      const bytes=key.length*4+value.lines.reduce((sum,line)=>sum+line.text.length*2+128,0);
      if(bytes<=maxCacheBytes){while(cache.size>=256||cacheBytes+bytes>maxCacheBytes){const first=cache.keys().next().value;cacheBytes-=cache.get(first).bytes;cache.delete(first);}cache.set(key,{value,bytes});cacheBytes+=bytes;}
      return value;
    }
    function wholeRange(field,begin=-1,end=-1){
      if((begin===-1&&end===-1)||(begin===0&&(end===-1||end===field.text.length)))return;
      throw new RangeError('TextField currently supports whole-field formatting only; mixed character-format ranges are pending.');
    }
    function line(field,index){const result=layout(field);if(!Number.isInteger(index)||index<0||index>=result.lines.length)throw new RangeError('TextField line index is outside the current layout.');return result.lines[index];}
    function get(ref,name){
      const known=['text','textColor','length','numLines','textWidth','textHeight','multiline','wordWrap','defaultTextFormat','type','selectable','displayAsPassword','password','maxChars','restrict','selectionBeginIndex','selectionEndIndex','caretIndex'];
      if(!known.includes(name))return UNHANDLED;
      const field=info(ref);
      const inputNames={type:'field_type',selectable:'selectable',displayAsPassword:'display_as_password',password:'display_as_password',maxChars:'max_chars',restrict:'restrict',selectionBeginIndex:'selection_begin',selectionEndIndex:'selection_end',caretIndex:'caret'};
      if(Object.hasOwn(inputNames,name))return field[inputNames[name]]??({type:'dynamic',selectable:true,displayAsPassword:false,password:false,maxChars:0,restrict:null,selectionBeginIndex:0,selectionEndIndex:0,caretIndex:0}[name]);
      if(name==='text')return publicText(field.text);
      if(name==='textColor')return field.format.color;
      if(name==='length')return field.text.length;
      if(name==='multiline')return field.multiline;
      if(name==='wordWrap')return field.word_wrap;
      if(name==='defaultTextFormat')return formatCopy(field.default_format);
      const result=layout(field);
      return name==='numLines'?result.lines.length:name==='textWidth'?result.text_width:result.text_height;
    }
    function set(ref,name,value){
      if(name==='type'){if(!['input','dynamic'].includes(value))throw new TypeError('TextField.type requires input or dynamic.');update(ref,{field_type:value});return true;}
      if(['selectable','displayAsPassword','password'].includes(name)){if(typeof value!=='boolean')throw new TypeError(`TextField.${name} requires a Boolean.`);update(ref,{[name==='selectable'?'selectable':'display_as_password']:value});return true;}
      if(name==='maxChars'){if(!Number.isInteger(value)||value<0||value>1048576)throw new RangeError('TextField.maxChars requires an integer from 0 through 1048576.');update(ref,{max_chars:value});return true;}
      if(name==='restrict'){if(value!==null&&typeof value!=='string')throw new TypeError('TextField.restrict requires String or null.');update(ref,{restrict:{value}});return true;}
      if(['selectionBeginIndex','selectionEndIndex','caretIndex'].includes(name))throw new TypeError(`TextField.${name} is read-only; use setSelection.`);
      if(name==='text'){update(ref,{text:string(value)});return true;}
      if(name==='textColor'){if(value===null||value===undefined)throw new TypeError('TextField.textColor requires an RGB integer.');update(ref,{format:{color:formatValue('color',value)}});return true;}
      if(name==='multiline'||name==='wordWrap'){if(typeof value!=='boolean')throw new TypeError(`TextField.${name} requires a Boolean.`);update(ref,{[name==='wordWrap'?'word_wrap':name]:value});return true;}
      if(name==='defaultTextFormat'){update(ref,{default_format:formatPatch(value)});return true;}
      if(name==='width'||name==='height'||name==='_width'||name==='_height'){
        if(name[0]==='_')value=Number(value);
        if(typeof value!=='number'||!Number.isFinite(value)||value<0)throw new RangeError('TextField box dimensions must be finite and nonnegative.');
        const axis=name.includes('width')?'width':'height',field=info(ref),patch={[axis]:value};
        if(!field.box){const result=layout(field);patch[axis==='width'?'height':'width']=axis==='width'?result.text_height:result.text_width;}
        update(ref,patch);return true;
      }
      if(['length','numLines','textWidth','textHeight'].includes(name))throw new TypeError(`TextField.${name} is read-only.`);
      return false;
    }
    function methods(ref){return {
      setSelection:(begin,end)=>{if(!Number.isSafeInteger(begin)||!Number.isSafeInteger(end))throw new TypeError('setSelection requires integer UTF-16 indices.');update(ref,{selection:{begin,end}});},
      replaceSelectedText:value=>update(ref,{replace_selected:string(value)}),
      appendText:value=>update(ref,{append:string(value)}),
      replaceText:(begin,end,value)=>{
        const field=info(ref);if(!Number.isInteger(begin)||!Number.isInteger(end)||begin<0||end<begin||end>field.text.length)throw new RangeError('replaceText requires an ordered UTF-16 range within the current text.');
        string(field.text.slice(0,begin));string(field.text.slice(end));update(ref,{replace:{begin,end,text:string(value)}});
      },
      setTextFormat:(...args)=>{
        let format,begin=-1,end=-1;
        if(formats.has(args[0]))[format,begin=-1,end=-1]=args;
        else if(args.length===3)[begin,end,format]=args;
        else throw new TypeError('setTextFormat requires a TextFormat and an optional whole-field range.');
        if(args.length>3)throw new TypeError('setTextFormat accepts at most three arguments.');
        const field=info(ref);wholeRange(field,begin,end);update(ref,{format:formatPatch(format)});
      },
      getTextFormat:(begin=-1,end=-1)=>{const field=info(ref);wholeRange(field,begin,end);return formatCopy(field.format);},
      setNewTextFormat:format=>update(ref,{default_format:formatPatch(format)}),
      getNewTextFormat:()=>formatCopy(info(ref).default_format),
      getLineText:index=>{const field=info(ref),item=line(field,index);return publicText(field.text.slice(item.start,item.end));},
      getLineLength:index=>{const item=line(info(ref),index);return item.end-item.start;},
      getLineOffset:index=>line(info(ref),index).start,
      getLineIndexOfChar:index=>{const field=info(ref);if(!Number.isInteger(index)||index<0||index>=field.text.length)throw new RangeError('TextField character index is outside the current UTF-16 text.');return layout(field).lines.findIndex(item=>index>=item.start&&index<item.end);},
    };}
    function bounds(field,ink=false){
      if(ink&&field.alpha>0&&(field.as2_viewport?.background||field.as2_viewport?.border)){const surface=layout(field).surface;return {left:0,top:0,right:surface.width,bottom:surface.height};}
      if(!ink&&field.box)return {left:0,top:0,right:field.box.width,bottom:field.box.height};
      let box=null;
      for(const item of layout(field).lines){
        if(ink&&(!item.text.trim()||field.alpha===0))continue;
        const next={left:item.x,top:item.y,right:item.x+item.width,bottom:item.y+item.height};
        if(field.box){next.left=Math.max(0,next.left);next.top=Math.max(0,next.top);next.right=Math.min(field.box.width,next.right);next.bottom=Math.min(field.box.height,next.bottom);}
        if(next.right<=next.left||next.bottom<=next.top)continue;
        box=box?{left:Math.min(box.left,next.left),top:Math.min(box.top,next.top),right:Math.max(box.right,next.right),bottom:Math.max(box.bottom,next.bottom)}:next;
      }
      if(field.alpha>0)for(const [x,y,radius]of layout(field).bullets||[]){const marker={left:x-radius,top:y-radius,right:x+radius,bottom:y+radius};if(field.box){marker.left=Math.max(0,marker.left);marker.top=Math.max(0,marker.top);marker.right=Math.min(field.box.width,marker.right);marker.bottom=Math.min(field.box.height,marker.bottom);}if(marker.right>marker.left&&marker.bottom>marker.top)box=box?{left:Math.min(box.left,marker.left),top:Math.min(box.top,marker.top),right:Math.max(box.right,marker.right),bottom:Math.max(box.bottom,marker.bottom)}:marker;}
      return box;
    }
    function draw(context,field){
      const result=layout(field);context.save();
      try{
        const surface=result.surface;
        if(surface?.background!==null&&surface?.background!==undefined){context.fillStyle=`rgba(${surface.background>>>16},${surface.background>>>8&255},${surface.background&255},${field.alpha/255})`;context.fillRect(0,0,surface.width,surface.height);}
        if(surface?.border!==null&&surface?.border!==undefined&&surface.width>0&&surface.height>0){context.strokeStyle=`rgba(${surface.border>>>16},${surface.border>>>8&255},${surface.border&255},${field.alpha/255})`;context.lineWidth=1;context.strokeRect(0.5,0.5,Math.max(0,surface.width-1),Math.max(0,surface.height-1));}
        if(field.box){context.beginPath();context.rect(0,0,field.box.width,field.box.height);context.clip();}
        applyFont(context,field.format);context.textAlign='left';context.textBaseline='top';
        const rgb=field.format.color;context.fillStyle=`rgba(${rgb>>>16},${rgb>>>8&255},${rgb&255},${field.alpha/255})`;
        for(const item of result.runs?.length?result.runs:result.lines)context.fillText(item.text,item.x,item.y);
        for(const [x,y,radius]of result.bullets||[]){context.beginPath();context.arc(x,y,radius,0,Math.PI*2);context.fill();}
        for(const item of result.decorations||[])context.fillRect(item.x,item.y,item.width,item.height);
      }finally{context.restore();}
    }
    function hit(field,x,y){const box=bounds(field);return !!box&&box.right>box.left&&box.bottom>box.top&&field.alpha>0&&x>=box.left&&y>=box.top&&x<=box.right&&y<=box.bottom;}
    function reset(){active();cache.clear();cacheBytes=0;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({measure:(strings,format)=>strings.map(value=>measureText(value,format)),TextFormat,As2TextFormat,legacyFormatCopy,bindAs2:execute=>{as2Execute=execute;},bindAs2Layout:callback=>{as2Layout=callback;},bindAs2Wheel:callback=>{as2Wheel=callback;},wheel:(ref,event)=>as2Wheel?.(ref,event)||false,notifyLayout:ref=>onUpdate(ref),TextFormatAlign,TextFieldType,info,update,layout,get,set,methods,bounds,draw,hit,reset,dispose,UNHANDLED});
  }
  const api={create,TextFormat,TextFormatAlign,TextFieldType,formatPatch,formatCopy,cssFont,applyFont};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashText=api;
})(globalThis);
