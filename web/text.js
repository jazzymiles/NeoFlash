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
  const formats=new WeakMap();
  const fields=['font','size','color','bold','italic','underline','url','target','align','leftMargin','rightMargin','indent','leading'];
  const supported=new Set(['font','size','color','bold','italic','align','leading']);
  function formatValue(name,value){
    if(value===null||value===undefined)return null;
    if(!supported.has(name))throw new TypeError(`TextFormat.${name} is not supported by the uniform text profile.`);
    if(name==='font'){if(typeof value!=='string'||!value.length||new TextEncoder().encode(value).length>1024||/[\u0000-\u001f\u007f-\u009f]/.test(value))throw new TypeError('TextFormat.font requires a nonempty font-family string of at most 1024 UTF-8 bytes without controls.');}
    else if(name==='size'){if(typeof value!=='number'||!Number.isFinite(value)||value<=0||value>10000)throw new RangeError('TextFormat.size requires a finite number greater than zero through 10000.');}
    else if(name==='color'){if(!Number.isInteger(value)||value<0||value>0xffffff)throw new RangeError('TextFormat.color requires an RGB integer from zero through 0xffffff.');}
    else if(name==='bold'||name==='italic'){if(typeof value!=='boolean')throw new TypeError(`TextFormat.${name} requires a Boolean or null.`);}
    else if(name==='align'){if(!['left','center','right'].includes(value))throw new TypeError('TextFormat.align supports left, center and right; justified layout is pending.');}
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
  for(const name of fields)Object.defineProperty(TextFormat.prototype,name,{enumerable:true,get(){const value=formats.get(this);if(!value)throw new TypeError('Expected TextFormat.');return value[name];},set(value){const current=formats.get(this);if(!current)throw new TypeError('Expected TextFormat.');current[name]=formatValue(name,value);}});
  for(const name of ['blockIndent','bullet','kerning','letterSpacing','tabStops','display'])Object.defineProperty(TextFormat.prototype,name,{get(){throw new TypeError(`TextFormat.${name} is not supported by the uniform text profile.`);},set(){throw new TypeError(`TextFormat.${name} is not supported by the uniform text profile.`);}});
  TextFormat.__nfIs=value=>formats.has(value);
  TextFormat.__nfInitialize=()=>{throw new TypeError('Native TextFormat subclassing is not supported.');};
  Object.defineProperty(TextFormat,Symbol.hasInstance,{value:TextFormat.__nfIs});
  class TextFormatAlign {constructor(){throw new TypeError('TextFormatAlign supplies alignment constants.');}}
  Object.assign(TextFormatAlign,{LEFT:'left',CENTER:'center',RIGHT:'right',JUSTIFY:'justify'});Object.freeze(TextFormatAlign);
  class TextFieldType {constructor(){throw new TypeError('TextFieldType supplies INPUT and DYNAMIC constants.');}}
  Object.assign(TextFieldType,{INPUT:'input',DYNAMIC:'dynamic'});Object.freeze(TextFieldType);
  class TextFieldAutoSize {constructor(){throw new TypeError('TextFieldAutoSize supplies NONE, LEFT, CENTER and RIGHT constants.');}}
  Object.assign(TextFieldAutoSize,{NONE:'none',LEFT:'left',CENTER:'center',RIGHT:'right'});Object.freeze(TextFieldAutoSize);
  const metricsBrand=new WeakSet();
  class TextLineMetrics {
    constructor(x,width,height,ascent,descent,leading){
      const values={x,width,height,ascent,descent,leading};if(!Object.values(values).every(Number.isFinite))throw new TypeError('TextLineMetrics requires six finite numeric measurements.');
      Object.assign(this,values);metricsBrand.add(this);Object.seal(this);
    }
  }
  TextLineMetrics.__nfIs=value=>metricsBrand.has(value);
  TextLineMetrics.__nfInitialize=()=>{throw new TypeError('Native TextLineMetrics subclassing is unsupported.');};
  Object.defineProperty(TextLineMetrics,Symbol.hasInstance,{value:TextLineMetrics.__nfIs});
  function formatPatch(value){
    const record=formats.get(value);if(!record)throw new TypeError('Expected TextFormat from this runtime.');
    return Object.fromEntries(Object.entries(record).filter(([,item])=>item!==null));
  }
  function formatCopy(value){const result=new TextFormat();for(const name of supported)if(value[name]!==undefined)result[name]=value[name];return result;}
  function cssFont(format){return `${format.italic?'italic ':''}${format.bold?'bold ':''}${format.size}px ${JSON.stringify(format.font)}, sans-serif`;}
  function string(value){
    if(typeof value!=='string')throw new TypeError('TextField text requires a string; convert other values explicitly with String().');
    for(let i=0;i<value.length;i++){const unit=value.charCodeAt(i);if(unit>=0xd800&&unit<=0xdbff){const low=value.charCodeAt(++i);if(!(low>=0xdc00&&low<=0xdfff))throw new RangeError('TextField text must preserve complete Unicode characters.');}else if(unit>=0xdc00&&unit<=0xdfff)throw new RangeError('TextField text must preserve complete Unicode characters.');}
    return value.replace(/\r\n?|\n/g,'\n');
  }
  const publicText=value=>value.replace(/\n/g,'\r');
  const UNHANDLED=Symbol('unhandled text member');
  function create({player,measureText,onUpdate=()=>{},onScroll=()=>{},receiverFor=()=>null,isAlive=()=>true,geometry=global.NeoFlashGeom}={}){
    if(!player||typeof measureText!=='function')throw new TypeError('TextField requires the shared Player and font measurement service.');
    let disposed=false,cacheBytes=0,generation=1,preparing=false;const cache=new Map(),settled=new Map(),listeners=new Map(),maxCacheBytes=8*1024*1024;
    const active=()=>{if(disposed)throw new Error('This text service has been disposed.');};
    function native(fn){try{return fn();}catch(error){if(error instanceof Error)throw error;throw new Error(String(error),{cause:error});}}
    const measure=(strings,format)=>{const style=JSON.parse(format);return JSON.stringify(JSON.parse(strings).map(text=>measureText(text,style)));};
    function remember(field,value){
      const key=JSON.stringify(field),bytes=key.length*4+value.lines.reduce((sum,line)=>sum+line.text.length*2+160,0)+(value.glyphs||[]).reduce((sum,glyph)=>sum+glyph.text.length*2+48,0)+(value.paragraphs?.length||0)*24;
      if(cache.has(key)){cacheBytes-=cache.get(key).bytes;cache.delete(key);}
      if(bytes<=maxCacheBytes){while(cache.size>=256||cacheBytes+bytes>maxCacheBytes){const first=cache.keys().next().value;cacheBytes-=cache.get(first).bytes;cache.delete(first);}cache.set(key,{value,bytes});cacheBytes+=bytes;}
      return value;
    }
    function notifyLegacy(ref,name,args){
      active();const version=generation,receiver=receiverFor(ref);if(!receiver||!isAlive(receiver))return;
      const targets=[receiver,...(listeners.get(ref)||[])];
      for(const target of targets){
        if(version!==generation||disposed||!isAlive(receiver))return;
        if(!isAlive(target)){listeners.get(ref)?.delete(target);continue;}
        const fn=target[name];if(version!==generation||disposed)return;
        if(typeof fn==='function')Reflect.apply(fn,target,args);
      }
    }
    function accept(ref,result,notify=true){
      settled.set(ref,result.field.revision);remember(result.field,result.layout);
      if(notify)onUpdate(ref);
      if(result.scroll_changed){const version=generation;onScroll(ref);if(version===generation&&!disposed){const receiver=receiverFor(ref);notifyLegacy(ref,'onScroller',[receiver]);}}
      return result.field;
    }
    function reflow(ref,field){
      if(typeof player.object_text_reflow!=='function')return field;
      return accept(ref,JSON.parse(native(()=>player.object_text_reflow(ref,measure))),false);
    }
    function info(ref){active();const field=JSON.parse(native(()=>player.object_text_info(ref)));return settled.get(ref)===field.revision?field:reflow(ref,field);}
    function update(ref,patch){
      active();
      if(typeof player.object_text_update_layout==='function')return accept(ref,JSON.parse(native(()=>player.object_text_update_layout(ref,JSON.stringify(patch),measure))));
      if(!native(()=>player.object_text_update(ref,JSON.stringify(patch))))throw new Error('TextField update requires a live text object.');onUpdate(ref);return info(ref);
    }
    function input(ref,edit){
      active();
      if(typeof player.object_text_input_layout==='function')return accept(ref,JSON.parse(native(()=>player.object_text_input_layout(ref,JSON.stringify(edit),measure))));
      const field=JSON.parse(native(()=>player.object_text_input(ref,JSON.stringify(edit))));onUpdate(ref);return field;
    }
    function prepareSnapshot(scene){
      active();if(preparing||typeof player.object_text_reflow!=='function')return scene;
      preparing=true;let changed=false;
      try{const live=new Set();for(const node of scene.objects||[]){if(!node.text)continue;const ref=JSON.stringify(node.object_ref);live.add(ref);if(settled.get(ref)===node.text.revision)continue;const field=reflow(ref,node.text);changed ||=field.revision!==node.text.revision;}for(const ref of settled.keys())if(!live.has(ref)){settled.delete(ref);listeners.delete(ref);}return changed?JSON.parse(player.runtime_snapshot_json()):scene;}finally{preparing=false;}
    }
    function layout(field){
      active();const key=JSON.stringify(field),existing=cache.get(key);
      if(existing){cache.delete(key);cache.set(key,existing);return existing.value;}
      const value=JSON.parse(native(()=>player.text_layout_json(key,measure)));
      // Keys and the returned line strings both own text. Bound retained memory,
      // not just entry count, and skip caching exceptionally large layouts.
      return remember(field,value);
    }
    function wholeRange(field,begin=-1,end=-1){
      if((begin===-1&&end===-1)||(begin===0&&(end===-1||end===field.text.length)))return;
      throw new RangeError('TextField currently supports whole-field formatting only; mixed character-format ranges are pending.');
    }
    function line(field,index){const result=layout(field);if(!Number.isInteger(index)||index<0||index>=result.lines.length)throw new RangeError('TextField line index is outside the current layout.');return result.lines[index];}
    function get(ref,name){
      const known=['text','textColor','length','numLines','textWidth','textHeight','multiline','wordWrap','defaultTextFormat','type','selectable','displayAsPassword','password','maxChars','restrict','selectionBeginIndex','selectionEndIndex','caretIndex','autoSize','background','backgroundColor','border','borderColor','scrollH','scrollV','maxScrollH','maxScrollV','bottomScrollV','hscroll','scroll','maxhscroll','maxscroll','bottomScroll'];
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
      const decoration={autoSize:'auto_size',background:'background',backgroundColor:'background_color',border:'border',borderColor:'border_color'};
      if(Object.hasOwn(decoration,name))return field[decoration[name]]??({autoSize:'none',background:false,backgroundColor:0xffffff,border:false,borderColor:0}[name]);
      const result=layout(field);
      const scrolling={scrollH:'scroll_h',hscroll:'scroll_h',scrollV:'scroll_v',scroll:'scroll_v',maxScrollH:'max_scroll_h',maxhscroll:'max_scroll_h',maxScrollV:'max_scroll_v',maxscroll:'max_scroll_v',bottomScrollV:'bottom_scroll_v',bottomScroll:'bottom_scroll_v'};
      if(Object.hasOwn(scrolling,name))return result[scrolling[name]]??field[scrolling[name]]??(name.toLowerCase().includes('h')?0:1);
      return name==='numLines'?result.lines.length:name==='textWidth'?result.text_width:result.text_height;
    }
    function set(ref,name,value){
      if(name==='autoSize'){if(!['none','left','center','right'].includes(value))throw new TypeError('TextField.autoSize requires none, left, center or right.');update(ref,{auto_size:value});return true;}
      if(name==='background'||name==='border'){if(typeof value!=='boolean')throw new TypeError(`TextField.${name} requires Boolean.`);update(ref,{[name]:value});return true;}
      if(name==='backgroundColor'||name==='borderColor'){if(value===null||value===undefined)throw new TypeError(`TextField.${name} requires an RGB integer.`);update(ref,{[name==='backgroundColor'?'background_color':'border_color']:formatValue('color',value)});return true;}
      if(['scrollH','scrollV','scroll','hscroll'].includes(name)){if(!Number.isSafeInteger(value))throw new TypeError('TextField scrolling requires integer pixels or a one-based line index.');update(ref,{[name==='scrollH'||name==='hscroll'?'scroll_h':'scroll_v']:value});return true;}
      if(['maxScrollH','maxScrollV','bottomScrollV','maxhscroll','maxscroll','bottomScroll'].includes(name))throw new TypeError(`TextField.${name} is read-only.`);
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
    function query(ref,value){
      const field=info(ref);
      if(typeof player.text_query_json!=='function')throw new Error('TextField geometry queries require the current shared text runtime. Re-export the publication.');
      return JSON.parse(native(()=>player.text_query_json(JSON.stringify(field),JSON.stringify(layout(field)),JSON.stringify(value))));
    }
    function wheel(ref,event){
      if(event.defaultPrevented||event.ctrlKey||event.metaKey)return false;
      const field=info(ref),before=layout(field),factor=event.deltaMode===1?before.line_height:event.deltaMode===2?(before.viewport?.height||before.line_height):1;
      const dx=(Number(event.deltaX)||0)+(event.shiftKey?(Number(event.deltaY)||0):0),dy=event.shiftKey?0:Number(event.deltaY)||0;
      if(!Number.isFinite(dx)||!Number.isFinite(dy))return false;
      const horizontal=Math.round(Math.max(-1e6,Math.min(1e6,dx*factor))),vertical=Math.sign(dy)*Math.ceil(Math.min(1e6,Math.abs(dy*factor))/before.line_height);
      if(!horizontal&&!vertical)return false;
      const after=layout(update(ref,{scroll_h:(before.scroll_h||0)+horizontal,scroll_v:(before.scroll_v||1)+vertical}));
      const changed=after.scroll_h!==before.scroll_h||after.scroll_v!==before.scroll_v;
      if(changed)event.preventDefault?.();return changed;
    }
    function methods(ref){return {
      addListener:value=>{active();info(ref);if(value===null||!['object','function'].includes(typeof value))throw new TypeError('TextField.addListener requires an object.');let set=listeners.get(ref);if(!set){set=new Set();listeners.set(ref,set);}if(set.has(value))return false;if(set.size>=1024)throw new RangeError('TextField supports at most 1024 listeners.');set.add(value);return true;},
      removeListener:value=>{active();info(ref);return listeners.get(ref)?.delete(value)||false;},
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
      getLineMetrics:index=>{const field=info(ref),item=line(field,index);return new TextLineMetrics(item.x,item.width,item.height,item.ascent??field.format.size,item.descent??field.format.size*.2,item.leading??field.format.leading);},
      getCharBoundaries:index=>{const item=query(ref,{type:'char_bounds',index});if(!item)return null;if(!geometry?.Rectangle)throw new Error('TextField character bounds require Rectangle.');return new geometry.Rectangle(item.x,item.y,item.width,item.height);},
      getCharIndexAtPoint:(x,y)=>query(ref,{type:'char_at_point',x,y}),
      getLineIndexAtPoint:(x,y)=>query(ref,{type:'line_at_point',x,y}),
      getFirstCharInParagraph:index=>query(ref,{type:'first_char_in_paragraph',index}),
      getParagraphLength:index=>query(ref,{type:'paragraph_length',index}),
    };}
    function bounds(field,ink=false){
      const result=layout(field),viewport=result.viewport||field.box;
      if(viewport&&(!ink||(field.background&&(field.background_alpha??255)>0)||(field.border&&(field.border_alpha??255)>0)))return {left:0,top:0,right:viewport.width,bottom:viewport.height};
      let box=null;
      for(const item of result.lines){
        if(ink&&(!item.text.trim()||field.alpha===0))continue;
        const x=item.x-(result.scroll_x||0),y=item.y-(result.scroll_y||0),next={left:x,top:y,right:x+item.width,bottom:y+item.height};
        if(viewport){next.left=Math.max(0,next.left);next.top=Math.max(0,next.top);next.right=Math.min(viewport.width,next.right);next.bottom=Math.min(viewport.height,next.bottom);}
        if(next.right<=next.left||next.bottom<=next.top)continue;
        box=box?{left:Math.min(box.left,next.left),top:Math.min(box.top,next.top),right:Math.max(box.right,next.right),bottom:Math.max(box.bottom,next.bottom)}:next;
      }
      return box;
    }
    function draw(context,field){
      const result=layout(field);context.save();
      try{
        const viewport=result.viewport||field.box;
        if(viewport){
          const {width,height}=viewport;
          if(field.background){const c=field.background_color??0xffffff;context.fillStyle=`rgba(${c>>>16},${c>>>8&255},${c&255},${(field.background_alpha??255)/255})`;context.fillRect(0,0,width,height);}
          if(field.border&&width>0&&height>0){const c=field.border_color??0;context.strokeStyle=`rgba(${c>>>16},${c>>>8&255},${c&255},${(field.border_alpha??255)/255})`;context.lineWidth=1;context.beginPath();context.rect(.5,.5,Math.max(0,width-1),Math.max(0,height-1));context.stroke();}
          context.beginPath();context.rect(0,0,width,height);context.clip();
        }
        context.font=cssFont(field.format);context.textAlign='left';context.textBaseline='top';
        const rgb=field.format.color;context.fillStyle=`rgba(${rgb>>>16},${rgb>>>8&255},${rgb&255},${field.alpha/255})`;
        for(const item of result.lines){const x=item.x-(result.scroll_x||0),y=item.y-(result.scroll_y||0);if(!viewport||(y+item.height>0&&y<viewport.height&&x+item.width>0&&x<viewport.width))context.fillText(item.text,x,y);}
      }finally{context.restore();}
    }
    function hit(field,x,y){const box=bounds(field);return !!box&&box.right>box.left&&box.bottom>box.top&&(field.alpha>0||(field.background&&(field.background_alpha??255)>0)||(field.border&&(field.border_alpha??255)>0))&&x>=box.left&&y>=box.top&&x<=box.right&&y<=box.bottom;}
    function reset(){active();generation++;cache.clear();settled.clear();listeners.clear();cacheBytes=0;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({TextFormat,TextFormatAlign,TextFieldType,TextFieldAutoSize,TextLineMetrics,info,update,input,layout,prepareSnapshot,notifyLegacy,wheel,get,set,methods,bounds,draw,hit,reset,dispose,UNHANDLED});
  }
  const api={create,TextFormat,TextFormatAlign,TextFieldType,TextFieldAutoSize,TextLineMetrics,formatPatch,formatCopy,cssFont};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashText=api;
})(globalThis);
