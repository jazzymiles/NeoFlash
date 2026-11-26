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
  const extraFields=['blockIndent','letterSpacing','kerning','bullet','tabStops','display'];
  const allFields=[...fields,...extraFields];
  const supported=new Set(allFields);
  const legacySupported=supported;
  function formatValue(name,value,legacy=false){
    if(value===null||value===undefined)return null;
    if(!(legacy?legacySupported:supported).has(name))throw new TypeError(`TextFormat.${name} is not supported by the uniform text profile.`);
    if(name==='font'){if(typeof value!=='string'||!value.length||new TextEncoder().encode(value).length>1024||/[\u0000-\u001f\u007f-\u009f]/.test(value))throw new TypeError('TextFormat.font requires a nonempty font-family string of at most 1024 UTF-8 bytes without controls.');}
    else if(name==='size'){if(typeof value!=='number'||!Number.isFinite(value)||value<=0||value>10000)throw new RangeError('TextFormat.size requires a finite number greater than zero through 10000.');}
    else if(name==='color'){if(!Number.isInteger(value)||value<0||value>0xffffff)throw new RangeError('TextFormat.color requires an RGB integer from zero through 0xffffff.');}
    else if(name==='bold'||name==='italic'||name==='underline'||name==='kerning'||name==='bullet'){if(typeof value!=='boolean')throw new TypeError(`TextFormat.${name} requires a Boolean or null.`);}
    else if(name==='align'){if(!['left','center','right','justify','start','end'].includes(value))throw new TypeError('TextFormat.align requires left, center, right, justify, start or end.');}
    else if(name==='display'){if(!['inline','block','none'].includes(value)){const err=new RangeError('TextFormat.display requires inline, block or none.');err.name='ArgumentError';throw err;}}
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
      for(const name of extraFields)values[name]=null;
      formats.set(this,values);Object.seal(this);
    }
  }
  for(const name of allFields)Object.defineProperty(TextFormat.prototype,name,{enumerable:true,get(){const value=formats.get(this);if(!value)throw new TypeError('Expected TextFormat.');const item=value[name];return Array.isArray(item)?item.slice():item;},set(value){const current=formats.get(this);if(!current)throw new TypeError('Expected TextFormat.');current[name]=formatValue(name,value,legacyFormats.has(this));}});
  TextFormat.__nfIs=value=>formats.has(value);
  TextFormat.__nfInitialize=()=>{throw new TypeError('Native TextFormat subclassing is not supported.');};
  Object.defineProperty(TextFormat,Symbol.hasInstance,{value:TextFormat.__nfIs});
  class TextFormatAlign {constructor(){throw new TypeError('TextFormatAlign supplies alignment constants.');}}
  Object.assign(TextFormatAlign,{LEFT:'left',CENTER:'center',RIGHT:'right',JUSTIFY:'justify',START:'start',END:'end'});Object.freeze(TextFormatAlign);
  class TextFieldType {constructor(){throw new TypeError('TextFieldType supplies INPUT and DYNAMIC constants.');}}
  Object.assign(TextFieldType,{INPUT:'input',DYNAMIC:'dynamic'});Object.freeze(TextFieldType);
  class TextFieldAutoSize {constructor(){throw new TypeError('TextFieldAutoSize supplies CENTER, LEFT, NONE and RIGHT constants.');}}
  Object.assign(TextFieldAutoSize,{CENTER:'center',LEFT:'left',NONE:'none',RIGHT:'right'});Object.freeze(TextFieldAutoSize);
  class AntiAliasType {constructor(){throw new TypeError('AntiAliasType supplies ADVANCED and NORMAL constants.');}}
  Object.assign(AntiAliasType,{ADVANCED:'advanced',NORMAL:'normal'});Object.freeze(AntiAliasType);
  class GridFitType {constructor(){throw new TypeError('GridFitType supplies NONE, PIXEL and SUBPIXEL constants.');}}
  Object.assign(GridFitType,{NONE:'none',PIXEL:'pixel',SUBPIXEL:'subpixel'});Object.freeze(GridFitType);
  function defineTextEnum(name,constants){
    const EnumType=function(){throw new TypeError(`${name} supplies constants and cannot be instantiated.`);};
    Object.assign(EnumType,constants);
    Object.defineProperties(EnumType,{
      __nfIs:{value:()=>false},
      __nfInitialize:{value:()=>{throw new TypeError(`${name} cannot be subclassed.`);}},
      [Symbol.hasInstance]:{value:()=>false},
    });
    return Object.freeze(EnumType);
  }
  const FontStyle=defineTextEnum('FontStyle',{BOLD:'bold',BOLD_ITALIC:'boldItalic',ITALIC:'italic',REGULAR:'regular'});
  const FontType=defineTextEnum('FontType',{DEVICE:'device',EMBEDDED:'embedded',EMBEDDED_CFF:'embeddedCFF'});
  const TextColorType=defineTextEnum('TextColorType',{DARK_COLOR:'dark',LIGHT_COLOR:'light'});
  const TextDisplayMode=defineTextEnum('TextDisplayMode',{CRT:'crt',DEFAULT:'default',LCD:'lcd'});
  const TextFormatDisplay=defineTextEnum('TextFormatDisplay',{INLINE:'inline',BLOCK:'block'});
  const TextInteractionMode=defineTextEnum('TextInteractionMode',{NORMAL:'normal',SELECTION:'selection'});
  const csmRecords=new WeakMap(),csmFields=['fontSize','insideCutoff','outsideCutoff'];
  class CSMSettings {
    constructor(fontSize,insideCutoff,outsideCutoff){
      if(arguments.length!==3)throw new TypeError('CSMSettings requires three Number arguments: fontSize, insideCutoff and outsideCutoff.');
      csmRecords.set(this,{fontSize:Number(fontSize),insideCutoff:Number(insideCutoff),outsideCutoff:Number(outsideCutoff)});
      Object.seal(this);
    }
  }
  for(const name of csmFields)Object.defineProperty(CSMSettings.prototype,name,{enumerable:true,
    get(){const record=csmRecords.get(this);if(!record)throw new TypeError('Expected CSMSettings.');return record[name];},
    set(value){const record=csmRecords.get(this);if(!record)throw new TypeError('Expected CSMSettings.');record[name]=Number(value);}});
  CSMSettings.__nfIs=value=>csmRecords.has(value);
  CSMSettings.__nfInitialize=()=>{throw new TypeError('Native CSMSettings subclassing is not supported.');};
  Object.defineProperty(CSMSettings,Symbol.hasInstance,{value:CSMSettings.__nfIs});
  const lineMetrics=new WeakMap(),metricFields=['x','width','height','ascent','descent','leading'];
  class TextLineMetrics {
    constructor(x,width,height,ascent,descent,leading){
      if(arguments.length!==6)throw new TypeError('TextLineMetrics requires six Number arguments: x, width, height, ascent, descent and leading.');
      lineMetrics.set(this,{x:Number(x),width:Number(width),height:Number(height),ascent:Number(ascent),descent:Number(descent),leading:Number(leading)});
      Object.seal(this);
    }
  }
  for(const name of metricFields)Object.defineProperty(TextLineMetrics.prototype,name,{enumerable:true,
    get(){const record=lineMetrics.get(this);if(!record)throw new TypeError('Expected TextLineMetrics.');return record[name];},
    set(value){const record=lineMetrics.get(this);if(!record)throw new TypeError('Expected TextLineMetrics.');record[name]=Number(value);}});
  TextLineMetrics.__nfIs=value=>lineMetrics.has(value);
  TextLineMetrics.__nfInitialize=()=>{throw new TypeError('Native TextLineMetrics subclassing is not supported.');};
  Object.defineProperty(TextLineMetrics,Symbol.hasInstance,{value:TextLineMetrics.__nfIs});
  function formatPatch(value,raw=false){
    const record=formats.get(value);if(!record)throw new TypeError('Expected TextFormat from this runtime.');
    const patch=Object.fromEntries(Object.entries(record).filter(([,item])=>item!==null));
    if(!raw&&patch.align!==undefined){
      patch.alignMode=patch.align;
      if(patch.align==='justify'||patch.align==='start')patch.align='left';
      else if(patch.align==='end')patch.align='right';
    }
    return patch;
  }
  function formatCopy(value){
    const result=new TextFormat();
    for(const name of supported)if(value[name]!==undefined)result[name]=value[name];
    const mode=value.align_mode||value.alignMode;
    if(mode&&['justify','start','end'].includes(mode))result.align=mode;
    return result;
  }
  const GENERIC_FONT_FAMILIES=new Set(['serif','sans-serif','monospace','cursive','fantasy','system-ui','ui-serif','ui-sans-serif','ui-monospace','ui-rounded','emoji','math','fangsong']);
  function cssFontFamily(rawFamily,embedOnly=false){
    const name=String(rawFamily||'').trim();
    if(!name)return 'sans-serif';
    const lower=name.toLowerCase();
    if(GENERIC_FONT_FAMILIES.has(lower))return lower;
    return embedOnly?JSON.stringify(name):`${JSON.stringify(name)}, sans-serif`;
  }
  function cssFont(format,embedOnly=false){return `${format.italic?'italic ':''}${format.bold?'bold ':''}${format.size}px ${cssFontFamily(format.font,embedOnly)}`;}
  function applyFont(context,format,measurement=false,embedOnly=false,field=null,surface=null){
    context.font=cssFont(format,embedOnly);
    if('fontKerning' in context)context.fontKerning=format.kerning===null||format.kerning===undefined?'auto':format.kerning?'normal':'none';
    if('letterSpacing' in context)context.letterSpacing=measurement?'0px':((format.letterSpacing??format.letter_spacing)||0)+'px';
    const lig=surface?.ligatures||field?.ligatures||'common';
    if('fontVariantLigatures' in context){
      context.fontVariantLigatures=lig==='none'?'none':(lig==='uncommon'||lig==='discretionary')?'common-ligatures discretionary-ligatures':lig==='exotic'?'common-ligatures discretionary-ligatures historical-ligatures':'common-ligatures';
    }
    const dw=surface?.digit_width||field?.digit_width||'default';
    const dc=surface?.digit_case||field?.digit_case||'default';
    if('fontVariantNumeric' in context){
      const numParts=[];
      if(dc==='lining')numParts.push('lining-nums');
      else if(dc==='old_style')numParts.push('oldstyle-nums');
      if(dw==='tabular')numParts.push('tabular-nums');
      else if(dw==='proportional')numParts.push('proportional-nums');
      context.fontVariantNumeric=numParts.length?numParts.join(' '):'normal';
    }
    const loc=surface?.locale||field?.locale;
    if('lang' in context)context.lang=loc?String(loc).replaceAll('_','-'):'';
  }
  function string(value){
    if(typeof value!=='string')throw new TypeError('TextField text requires a string; convert other values explicitly with String().');
    for(let i=0;i<value.length;i++){const unit=value.charCodeAt(i);if(unit>=0xd800&&unit<=0xdbff){const low=value.charCodeAt(++i);if(!(low>=0xdc00&&low<=0xdfff))throw new RangeError('TextField text must preserve complete Unicode characters.');}else if(unit>=0xdc00&&unit<=0xdfff)throw new RangeError('TextField text must preserve complete Unicode characters.');}
    return value.replace(/\r\n?|\n/g,'\n');
  }
  const publicText=value=>value.replace(/\n/g,'\r');
  const sharedGraphemeSegmenter=typeof Intl!=='undefined'&&typeof Intl.Segmenter==='function'?new Intl.Segmenter(undefined,{granularity:'grapheme'}):null;
  function graphemeBoundaries(value,runtimeWasm=null){
    const s=String(value??'');
    if(!s.length)return [0];
    if(typeof runtimeWasm?.grapheme_boundaries_utf16==='function'){
      try{return Array.from(runtimeWasm.grapheme_boundaries_utf16(s));}catch(_){}
    }
    if(sharedGraphemeSegmenter){
      const out=[];
      for(const seg of sharedGraphemeSegmenter.segment(s))out.push(seg.index);
      out.push(s.length);
      return out;
    }
    const out=[0];
    for(let i=0;i<s.length;i++){
      const u=s.charCodeAt(i);
      if(u>=0xd800&&u<=0xdbff&&i+1<s.length&&s.charCodeAt(i+1)>=0xdc00&&s.charCodeAt(i+1)<=0xdfff)i++;
      out.push(i+1);
    }
    return out;
  }
  function snapGraphemeFloor(value,index,runtimeWasm=null){
    const s=String(value??'');
    const clamped=Math.max(0,Math.min(s.length,Math.trunc(Number(index)||0)));
    if(clamped<=0||clamped>=s.length)return clamped;
    const bounds=graphemeBoundaries(s,runtimeWasm);
    let best=0;
    for(let i=0;i<bounds.length;i++){if(bounds[i]<=clamped)best=bounds[i];else break;}
    return best;
  }
  function snapGraphemeCeil(value,index,runtimeWasm=null){
    const s=String(value??'');
    const clamped=Math.max(0,Math.min(s.length,Math.trunc(Number(index)||0)));
    if(clamped<=0||clamped>=s.length)return clamped;
    const bounds=graphemeBoundaries(s,runtimeWasm);
    for(let i=0;i<bounds.length;i++){if(bounds[i]>=clamped)return bounds[i];}
    return s.length;
  }
  function prevGrapheme(value,index,runtimeWasm=null){
    const s=String(value??'');
    const clamped=Math.max(0,Math.min(s.length,Math.trunc(Number(index)||0)));
    if(clamped<=0)return 0;
    const bounds=graphemeBoundaries(s,runtimeWasm);
    let prev=0;
    for(let i=0;i<bounds.length;i++){if(bounds[i]<clamped)prev=bounds[i];else break;}
    return prev;
  }
  function nextGrapheme(value,index,runtimeWasm=null){
    const s=String(value??'');
    const clamped=Math.max(0,Math.min(s.length,Math.trunc(Number(index)||0)));
    if(clamped>=s.length)return s.length;
    const bounds=graphemeBoundaries(s,runtimeWasm);
    for(let i=0;i<bounds.length;i++){if(bounds[i]>clamped)return bounds[i];}
    return s.length;
  }
  function graphemeSafeDiff(before,after,runtimeWasm=null){
    const bStr=String(before??''),aStr=String(after??'');
    let begin=0;
    while(begin<bStr.length&&begin<aStr.length&&bStr[begin]===aStr[begin])begin++;
    const bBounds=graphemeBoundaries(bStr,runtimeWasm);
    const aBounds=graphemeBoundaries(aStr,runtimeWasm);
    const bSet=new Set(bBounds),aSet=new Set(aBounds);
    while(begin>0&&(!bSet.has(begin)||!aSet.has(begin)))begin--;
    let oldEnd=bStr.length,newEnd=aStr.length;
    while(oldEnd>begin&&newEnd>begin&&bStr[oldEnd-1]===aStr[newEnd-1]){oldEnd--;newEnd--;}
    while(oldEnd<bStr.length&&newEnd<aStr.length&&(!bSet.has(oldEnd)||!aSet.has(newEnd))){oldEnd++;newEnd++;}
    if(!bSet.has(oldEnd))oldEnd=snapGraphemeCeil(bStr,oldEnd,runtimeWasm);
    if(!aSet.has(newEnd))newEnd=snapGraphemeCeil(aStr,newEnd,runtimeWasm);
    return {begin,end:oldEnd,text:aStr.slice(begin,newEnd)};
  }
  function sliceFieldRuns(field,begin,end){
    const fullLen=String(field?.text??'').length;
    const lo=Math.max(0,Math.min(fullLen,Math.min(begin,end)));
    const hi=Math.max(lo,Math.min(fullLen,Math.max(begin,end)));
    if(lo>=hi)return [];
    const rawRuns=Array.isArray(field?.format_runs)&&field.format_runs.length
      ?field.format_runs
      :[{start:0,end:fullLen,format:field?.format||{}}];
    const out=[];
    for(const r of rawRuns){
      const rs=Number(r.start??r.begin)||0;
      const re=Number(r.end)||0;
      if(re<=lo||rs>=hi)continue;
      const s=Math.max(rs,lo)-lo;
      const e=Math.min(re,hi)-lo;
      if(s<e)out.push({start:s,end:e,format:structuredClone?structuredClone(r.format):JSON.parse(JSON.stringify(r.format||{}))});
    }
    return out;
  }
  function extractHtmlFragment(rawHtml){
    const s=String(rawHtml??'');
    const startMarker='<!--StartFragment-->',endMarker='<!--EndFragment-->';
    const si=s.indexOf(startMarker);
    if(si>=0){
      const contentStart=si+startMarker.length;
      const ei=s.indexOf(endMarker,contentStart);
      if(ei>=contentStart)return s.slice(contentStart,ei);
    }
    return s;
  }
  function decodeBase64Bytes(encoded){
    const clean=String(encoded||'').trim();
    if(!clean)return null;
    try{
      if(typeof Buffer!=='undefined')return new Uint8Array(Buffer.from(clean,'base64'));
      const bin=global.atob(clean),out=new Uint8Array(bin.length);
      for(let i=0;i<bin.length;i++)out[i]=bin.charCodeAt(i);
      return out;
    }catch(_){return null;}
  }
  const RICH_CLIPBOARD_MIME='application/x-neoflash-runs+json';
  const UNHANDLED=Symbol('unhandled text member');
  function create({player,wasm=null,events=null,project=null,document=global.document,getProgram=null,geometry,measureText,createCanvas=null,deviceFontHasGlyphs=null,portableMetrics=false,fontMetrics=null,onUpdate=()=>{},onScroll=null}={}){
    if(!player)throw new TypeError('TextField requires the shared Player.');
    const getRuntimeWasm=()=>wasm||(typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm);
    let sharedRichClipboard=null;
    const defaultPortableMeasure=(textValue,style)=>{
      const runtimeWasm=wasm||(typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm);
      const fn=player?.portable_measure_text_json?.bind(player)||runtimeWasm?.portable_measure_text_json;
      if(typeof fn==='function'){
        try{return JSON.parse(fn(JSON.stringify([String(textValue)]),JSON.stringify(style)))[0]??0;}catch(_){}
      }
      if(fontMetrics&&typeof fontMetrics==='object'){
        const rawFont=String(style?.font||'');
        const wanted=rawFont.trim().toLowerCase();
        const entry=fontMetrics[rawFont]||Object.entries(fontMetrics).find(([k])=>k.toLowerCase()===wanted)?.[1];
        if(entry&&Number(entry.units_per_em)>0){
          const scale=(Number(style?.size)||16)/Number(entry.units_per_em);
          const defAdv=Number(entry.default_advance)||600;
          const advMap=entry.advances||{};
          let sum=0;
          for(const ch of String(textValue)){
            const cp=String(ch.codePointAt(0));
            sum+=(advMap[cp]!==undefined?Number(advMap[cp]):defAdv)*scale;
          }
          return sum;
        }
      }
      const size=Number(style?.size)||16;
      return String(textValue).length*size*0.56;
    };
    if(typeof measureText!=='function')measureText=defaultPortableMeasure;
    else if(portableMetrics){
      measureText=defaultPortableMeasure;
    }
    let as2Execute=null,as2Layout=null,as2Wheel=null,targetForRef=null,as2TextEvent=null;
    function resolveReceiver(ref){
      if(targetForRef){try{const r=targetForRef(ref);if(r)return r;}catch(_){}}
      const rootProxy=document?.defaultView?.NeoFlashPlayer?.root||global.NeoFlashPlayer?.root;
      if(!rootProxy)return null;
      const queue=[rootProxy];
      for(let i=0;i<queue.length&&i<2048;i++){
        const cur=queue[i];
        try{
          if(JSON.stringify(cur?.toJSON?.()?.object_ref)===ref)return cur;
          const n=Number(cur?.numChildren)||0;
          for(let c=0;c<n;c++){const ch=cur.getChildAt(c);if(ch)queue.push(ch);}
        }catch(_){}
      }
      return null;
    }
    const fieldStates=new Map();
    function stateFor(ref){
      let state=fieldStates.get(ref);
      if(!state){state={style:null,source:null,revision:null,useRichTextClipboard:false};fieldStates.set(ref,state);}
      return state;
    }
    function clearHtmlSource(ref){const st=fieldStates.get(ref);if(st){st.source=null;st.revision=null;}}
    function illegalTextOp(message='Error #2009: This method cannot be used on a text field with a style sheet.'){
      const err=new Error(message);err.name='IllegalOperationError';err.errorID=2009;return err;
    }
    class As2TextFormat extends TextFormat {
      constructor(...args){
        super();if(args.length>fields.length)throw new TypeError('AS2 TextFormat accepts at most thirteen arguments.');
        legacyFormats.add(this);const values={};for(let i=0;i<fields.length;i++)values[fields[i]]=formatValue(fields[i],args[i],true);values.blockIndent=null;values.letterSpacing=null;values.kerning=null;values.bullet=null;values.tabStops=null;values.display=null;formats.set(this,values);
      }
      getTextExtent(value,width){
        if(!as2Execute)throw new Error('AS2 text kernel is missing; re-export the project.');
        if(typeof value!=='string'||(width!==undefined&&(!Number.isFinite(width)||width<0)))throw new TypeError('getTextExtent requires text and an optional finite nonnegative wrap width.');
        const packet=as2Execute({op:'extent_field',text:value,format:formatPatch(this),width:width===undefined?null:width});
        return as2Execute({op:'extent_result',layout:layout(packet.field),format:packet.field.format});
      }
    }
    for(const name of ['blockIndent','letterSpacing','kerning','bullet','tabStops','display'])Object.defineProperty(As2TextFormat.prototype,name,{get(){const value=formats.get(this)[name];return Array.isArray(value)?value.slice():value;},set(value){formats.get(this)[name]=formatValue(name,value,true);}});
    function legacyFormatCopy(value){const result=new As2TextFormat();for(const name of legacySupported)if(value[name]!==undefined)result[name]=value[name];return result;}
    let disposed=false,cacheBytes=0;const cache=new Map(),maxCacheBytes=8*1024*1024;
    const active=()=>{if(disposed)throw new Error('This text service has been disposed.');};
    function info(ref){active();return JSON.parse(player.object_text_info(ref));}
    function fit(ref){if(typeof player.as2_text_fit!=='function')return false;const field=info(ref);return field.as2_viewport?.auto_size&&field.as2_viewport.auto_size!=='none'?player.as2_text_fit(ref,JSON.stringify(layout(field))):false;}
    function syncDomInputControl(ref,patch=null){
      if(!document?.querySelectorAll)return;
      let controls;
      try{controls=document.querySelectorAll('.nf-dom-input');}catch(_){return;}
      const wantedId=`input:${ref}`;
      for(const el of controls){
        if(el.getAttribute?.('data-nf-id')!==wantedId)continue;
        let field;
        try{field=info(ref);}catch(_){continue;}
        const readOnly=field.field_type!=='input'||Boolean(el.disabled);
        el.readOnly=readOnly;
        if(patch?.selection&&typeof el.setSelectionRange==='function'){
          const b=field.selection_begin||0,e=field.selection_end||0,dir=field.caret===b&&e!==b?'backward':'forward';
          try{if(el.selectionStart!==b||el.selectionEnd!==e)el.setSelectionRange(b,e,dir);}catch(_){}
        }
      }
    }
    function update(ref,patch){active();if(!player.object_text_update(ref,JSON.stringify(patch)))throw new Error('TextField update requires a live text object.');if(!fit(ref))as2Layout?.(ref);onUpdate(ref);syncDomInputControl(ref,patch);}
    function layout(field){
      active();const key=JSON.stringify(field.revision!==undefined?{...field,revision:0}:field),existing=cache.get(key);
      if(existing){cache.delete(key);cache.set(key,existing);return existing.value;}
      const value=JSON.parse(player.text_layout_json(JSON.stringify(field),(strings,format)=>{
        const style=JSON.parse(format);return JSON.stringify(JSON.parse(strings).map(text=>measureText(text,style)));
      }));
      // Keys and the returned line strings both own text. Bound retained memory,
      // not just entry count, and skip caching exceptionally large layouts.
      const bytes=key.length*4+value.lines.reduce((sum,line)=>sum+line.text.length*2+128,0);
      if(bytes<=maxCacheBytes){while(cache.size>=256||cacheBytes+bytes>maxCacheBytes){const first=cache.keys().next().value;cacheBytes-=cache.get(first).bytes;cache.delete(first);}cache.set(key,{value,bytes});cacheBytes+=bytes;}
      return value;
    }
    function query(field,request){
      active();
      if(typeof player.text_query_json!=='function')throw new Error('TextField query kernel is missing; rebuild and re-export this project.');
      return JSON.parse(player.text_query_json(JSON.stringify(field),JSON.stringify(request),(strings,format)=>{
        const style=JSON.parse(format);return JSON.stringify(JSON.parse(strings).map(text=>measureText(text,style)));
      }));
    }
    function validateRange(field,begin=-1,end=-1){
      if(begin===-1&&end===-1)return {whole:true,begin:0,end:field.text.length};
      const b=begin===-1?0:begin,e=end===-1?field.text.length:end;
      if(!Number.isInteger(b)||!Number.isInteger(e)||b<0||e<b||e>field.text.length)throw new RangeError('TextField format range is out of bounds.');
      return {whole:b===0&&e===field.text.length,begin:b,end:e};
    }
    function line(field,index){const result=layout(field);if(!Number.isInteger(index)||index<0||index>=result.lines.length)throw new RangeError('TextField line index is outside the current layout.');return result.lines[index];}
    function renderHtml(ref,state,source){
      const field=info(ref),styles=state.style?requireStyleSheet(state.style).state:{styles:{}};
      const parsed=textKernel({op:'html',source,format:field.default_format,styles,condense_white:Boolean(field.condense_white),multi_run:true});
      update(ref,{text:parsed.text,format:parsed.format,format_runs:parsed.format_runs||[]});
      state.source=source;state.revision=info(ref).revision;state.images=parsed.images||[];state.imageRefs=null;
    }
    function get(ref,name){
      const known=['text','htmlText','styleSheet','useRichTextClipboard','textColor','length','numLines','textWidth','textHeight','multiline','wordWrap','defaultTextFormat','type','selectable','displayAsPassword','password','maxChars','restrict','selectionBeginIndex','selectionEndIndex','caretIndex','autoSize','background','backgroundColor','border','borderColor','scrollV','scrollH','maxScrollV','maxScrollH','bottomScrollV','mouseWheelEnabled','antiAliasType','gridFitType','sharpness','thickness','embedFonts','condenseWhite','alwaysShowSelection','textInteractionMode'];
      if(!known.includes(name))return UNHANDLED;
      const field=info(ref);
      if(name==='textInteractionMode')return field.interaction_mode||TextInteractionMode.NORMAL;
      if(name==='useRichTextClipboard')return Boolean(fieldStates.get(ref)?.useRichTextClipboard);
      if(name==='styleSheet')return fieldStates.get(ref)?.style??null;
      if(name==='htmlText'){
        const st=fieldStates.get(ref);
        return st?.source!==null&&st?.source!==undefined&&st.revision===field.revision?st.source:textKernel({op:'serialize_html',text:field.text,format:field.format,format_runs:field.format_runs||[]}).result;
      }
      const inputNames={type:'field_type',selectable:'selectable',displayAsPassword:'display_as_password',password:'display_as_password',maxChars:'max_chars',restrict:'restrict',selectionBeginIndex:'selection_begin',selectionEndIndex:'selection_end',caretIndex:'caret',antiAliasType:'anti_alias_type',gridFitType:'grid_fit_type',sharpness:'sharpness',thickness:'thickness',embedFonts:'embed_fonts',condenseWhite:'condense_white',alwaysShowSelection:'always_show_selection'};
      if(Object.hasOwn(inputNames,name))return field[inputNames[name]]??({type:'dynamic',selectable:true,displayAsPassword:false,password:false,maxChars:0,restrict:null,selectionBeginIndex:0,selectionEndIndex:0,caretIndex:0,antiAliasType:'normal',gridFitType:'pixel',sharpness:0,thickness:0,embedFonts:false,condenseWhite:false,alwaysShowSelection:false}[name]);
      if(name==='autoSize')return field.as2_viewport?.auto_size??'none';
      if(name==='background')return Boolean(field.as2_viewport?.background);
      if(name==='backgroundColor')return field.as2_viewport?.backgroundColor??0xffffff;
      if(name==='border')return Boolean(field.as2_viewport?.border);
      if(name==='borderColor')return field.as2_viewport?.borderColor??0;
      if(name==='mouseWheelEnabled')return field.as2_viewport?.mouse_wheel_enabled??true;
      if(['scrollV','scrollH','maxScrollV','maxScrollH','bottomScrollV'].includes(name)){
        const vp=layout(field).viewport;
        return name==='scrollV'?vp.scroll:name==='scrollH'?vp.hscroll:name==='maxScrollV'?vp.maxscroll:name==='maxScrollH'?vp.maxhscroll:vp.bottom_scroll;
      }
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
      if(name==='textInteractionMode')throw new TypeError('TextField.textInteractionMode is read-only.');
      if(name==='useRichTextClipboard'){if(typeof value!=='boolean')throw new TypeError('TextField.useRichTextClipboard requires a Boolean.');info(ref);stateFor(ref).useRichTextClipboard=value;return true;}
      if(name==='styleSheet'){
        info(ref);if(value!==null)requireStyleSheet(value);
        const st=stateFor(ref),prev=st.style;st.style=value;
        try{if(st.source!==null)renderHtml(ref,st,st.source);}catch(err){st.style=prev;throw err;}
        return true;
      }
      if(name==='htmlText'){
        if(typeof value!=='string')throw new TypeError('TextField.htmlText requires a String.');
        const st=stateFor(ref);renderHtml(ref,st,value);return true;
      }
      if(name==='type'){if(!['input','dynamic'].includes(value))throw new TypeError('TextField.type requires input or dynamic.');update(ref,{field_type:value});return true;}
      if(['selectable','displayAsPassword','password','embedFonts','condenseWhite','alwaysShowSelection'].includes(name)){
        if(typeof value!=='boolean')throw new TypeError(`TextField.${name} requires a Boolean.`);
        const patchKey={selectable:'selectable',displayAsPassword:'display_as_password',password:'display_as_password',embedFonts:'embed_fonts',condenseWhite:'condense_white',alwaysShowSelection:'always_show_selection'}[name];
        update(ref,{[patchKey]:value});
        const st=fieldStates.get(ref);
        if(name==='condenseWhite'&&st?.source!==null&&st?.source!==undefined)renderHtml(ref,st,st.source);
        return true;
      }
      if(name==='antiAliasType'){if(!['normal','advanced'].includes(value))throw new TypeError('TextField.antiAliasType requires normal or advanced.');update(ref,{anti_alias_type:value});return true;}
      if(name==='gridFitType'){if(!['none','pixel','subpixel'].includes(value))throw new TypeError('TextField.gridFitType requires none, pixel or subpixel.');update(ref,{grid_fit_type:value});return true;}
      if(name==='sharpness'||name==='thickness'){if(typeof value!=='number'||!Number.isFinite(value))throw new RangeError(`TextField.${name} requires a finite Number.`);update(ref,{[name]:value});return true;}
      if(name==='autoSize'){if(!['none','left','center','right'].includes(value))throw new TypeError('TextField.autoSize requires none, left, center or right.');const field=info(ref);update(ref,{as2_viewport:{...field.as2_viewport,active:true,auto_size:value}});return true;}
      if(['background','border','mouseWheelEnabled'].includes(name)){if(typeof value!=='boolean')throw new TypeError(`TextField.${name} requires a Boolean.`);const field=info(ref),vpKey=name==='mouseWheelEnabled'?'mouse_wheel_enabled':name;update(ref,{as2_viewport:{...field.as2_viewport,active:true,[vpKey]:value}});return true;}
      if(name==='backgroundColor'||name==='borderColor'){if(!Number.isInteger(value)||value<0||value>0xffffff)throw new RangeError(`TextField.${name} requires a 24-bit RGB integer.`);const field=info(ref);update(ref,{as2_viewport:{...field.as2_viewport,active:true,[name]:value}});return true;}
      if(name==='scrollV'||name==='scrollH'){
        if(!Number.isInteger(value))throw new TypeError(`TextField.${name} requires an integer.`);
        const field=info(ref),vp=layout(field).viewport,prevScroll=vp.scroll,prevHScroll=vp.hscroll;
        if(name==='scrollV')update(ref,{as2_viewport:{...field.as2_viewport,active:true,scroll:Math.max(1,Math.min(vp.maxscroll,value))}});
        else update(ref,{as2_viewport:{...field.as2_viewport,active:true,hscroll:Math.max(0,Math.min(vp.maxhscroll,value))}});
        const nextVp=layout(info(ref)).viewport;
        if(nextVp.scroll!==prevScroll||nextVp.hscroll!==prevHScroll)onScroll?.(ref);
        return true;
      }
      if(name==='maxChars'){if(!Number.isInteger(value)||value<0||value>1048576)throw new RangeError('TextField.maxChars requires an integer from 0 through 1048576.');update(ref,{max_chars:value});return true;}
      if(name==='restrict'){if(value!==null&&typeof value!=='string')throw new TypeError('TextField.restrict requires String or null.');update(ref,{restrict:{value}});return true;}
      if(['selectionBeginIndex','selectionEndIndex','caretIndex'].includes(name))throw new TypeError(`TextField.${name} is read-only; use setSelection.`);
      if(name==='text'){if(fieldStates.get(ref)?.style)throw illegalTextOp();update(ref,{text:string(value)});clearHtmlSource(ref);return true;}
      if(name==='textColor'){if(value===null||value===undefined)throw new TypeError('TextField.textColor requires an RGB integer.');const c=formatValue('color',value);update(ref,{format:{color:c},default_format:{color:c}});clearHtmlSource(ref);return true;}
      if(name==='multiline'||name==='wordWrap'){if(typeof value!=='boolean')throw new TypeError(`TextField.${name} requires a Boolean.`);update(ref,{[name==='wordWrap'?'word_wrap':name]:value});return true;}
      if(name==='defaultTextFormat'){if(fieldStates.get(ref)?.style)throw illegalTextOp();update(ref,{default_format:formatPatch(value)});return true;}
      if(name==='width'||name==='height'||name==='_width'||name==='_height'){
        if(name[0]==='_')value=Number(value);
        if(typeof value!=='number'||!Number.isFinite(value)||value<0)throw new RangeError('TextField box dimensions must be finite and nonnegative.');
        const axis=name.includes('width')?'width':'height',field=info(ref),patch={[axis]:value};
        if(!field.box){const result=layout(field);patch[axis==='width'?'height':'width']=axis==='width'?result.text_height:result.text_width;}
        update(ref,patch);return true;
      }
      if(['length','numLines','textWidth','textHeight','maxScrollV','maxScrollH','bottomScrollV'].includes(name))throw new TypeError(`TextField.${name} is read-only.`);
      return false;
    }
    function userInteractionModeChanged(ref,value){
      if(value!==TextInteractionMode.NORMAL&&value!==TextInteractionMode.SELECTION){
        const err=new RangeError('Error #2008: Parameter textInteractionMode must be one of the accepted values.');
        err.name='ArgumentError';err.errorID=2008;throw err;
      }
      const previous=info(ref).interaction_mode||TextInteractionMode.NORMAL;
      if(previous===value)return false;
      update(ref,{interaction_mode:value});
      if(events?.Event){
        const receiver=resolveReceiver(ref);
        receiver?.dispatchEvent?.(new events.Event(events.Event.TEXT_INTERACTION_MODE_CHANGE||'textInteractionModeChange',false,false));
      }
      return true;
    }
    function methods(ref){return {
      setSelection:(begin,end)=>{if(!Number.isSafeInteger(begin)||!Number.isSafeInteger(end))throw new TypeError('setSelection requires integer UTF-16 indices.');update(ref,{selection:{begin,end}});},
      replaceSelectedText:value=>{if(fieldStates.get(ref)?.style)throw illegalTextOp();update(ref,{replace_selected:string(value)});clearHtmlSource(ref);},
      appendText:value=>{if(fieldStates.get(ref)?.style)throw illegalTextOp();update(ref,{append:string(value)});clearHtmlSource(ref);},
      replaceText:(begin,end,value)=>{
        if(fieldStates.get(ref)?.style)throw illegalTextOp();
        const field=info(ref);if(!Number.isInteger(begin)||!Number.isInteger(end)||begin<0||end<begin||end>field.text.length)throw new RangeError('replaceText requires an ordered UTF-16 range within the current text.');
        string(field.text.slice(0,begin));string(field.text.slice(end));update(ref,{replace:{begin,end,text:string(value)}});clearHtmlSource(ref);
      },
      setTextFormat:(...args)=>{
        if(fieldStates.get(ref)?.style)throw illegalTextOp();
        let format,begin=-1,end=-1;
        if(formats.has(args[0]))[format,begin=-1,end=-1]=args;
        else if(args.length===3)[begin,end,format]=args;
        else throw new TypeError('setTextFormat requires a TextFormat and an optional character range.');
        if(args.length>3)throw new TypeError('setTextFormat accepts at most three arguments.');
        const field=info(ref),r=validateRange(field,begin,end),patch=formatPatch(format);
        if(r.whole)update(ref,{format:patch});
        else update(ref,{format_range:{begin:r.begin,end:r.end,format:patch}});
        clearHtmlSource(ref);
      },
      getTextFormat:(begin=-1,end=-1)=>{
        const field=info(ref),r=validateRange(field,begin,end);
        if(r.whole&&(!field.format_runs||field.format_runs.length<=1))return formatCopy(field.format);
        return formatCopy(query(field,{op:'get_format',begin:r.begin,end:r.end}));
      },
      getImageReference:id=>{
        info(ref);
        if(id===null||id===undefined)throw new TypeError('getImageReference requires a non-null id String.');
        const st=stateFor(ref),found=(st.images||[]).find(img=>img.id===String(id));
        if(!found)return null;
        if(!st.imageRefs)st.imageRefs=new Map();
        if(!st.imageRefs.has(found.id)){
          let refObj=null;
          try{
            const prog=getProgram?.();
            const Ctor=prog?.type?.('flash.display.Loader')||prog?.type?.('flash.display.Sprite');
            if(Ctor){refObj=new Ctor();refObj.name=found.id;}
          }catch(_){}
          if(!refObj)refObj=Object.freeze({id:found.id,src:found.src,width:found.width,height:found.height});
          st.imageRefs.set(found.id,refObj);
        }
        return st.imageRefs.get(found.id);
      },
      setNewTextFormat:format=>update(ref,{default_format:formatPatch(format)}),
      getNewTextFormat:()=>formatCopy(info(ref).default_format),
      getLineText:index=>{const field=info(ref),item=line(field,index);return publicText(field.text.slice(item.start,item.end));},
      getLineLength:index=>{const item=line(info(ref),index);return item.end-item.start;},
      getLineOffset:index=>line(info(ref),index).start,
      getLineIndexOfChar:index=>{const field=info(ref);if(!Number.isInteger(index)||index<0||index>=field.text.length)throw new RangeError('TextField character index is outside the current UTF-16 text.');return layout(field).lines.findIndex(item=>index>=item.start&&index<item.end);},
      getLineMetrics:index=>{const field=info(ref),item=line(field,index),ascent=field.format.size,descent=item.height-ascent,leading=field.format.leading;return new TextLineMetrics(item.x,item.width,item.height+leading,ascent,descent,leading);},
      getFirstCharInParagraph:charIndex=>{if(!Number.isSafeInteger(charIndex))throw new TypeError('getFirstCharInParagraph requires an integer character index.');return query(info(ref),{op:'first_char_in_paragraph',char_index:charIndex});},
      getParagraphLength:charIndex=>{if(!Number.isSafeInteger(charIndex))throw new TypeError('getParagraphLength requires an integer character index.');return query(info(ref),{op:'paragraph_length',char_index:charIndex});},
      getLineIndexAtPoint:(x,y)=>{if(typeof x!=='number'||typeof y!=='number'||!Number.isFinite(x)||!Number.isFinite(y))throw new TypeError('getLineIndexAtPoint requires finite x and y Numbers.');return query(info(ref),{op:'line_index_at_point',x,y});},
      getCharIndexAtPoint:(x,y)=>{if(typeof x!=='number'||typeof y!=='number'||!Number.isFinite(x)||!Number.isFinite(y))throw new TypeError('getCharIndexAtPoint requires finite x and y Numbers.');return query(info(ref),{op:'char_index_at_point',x,y});},
      getCharBoundaries:charIndex=>{if(!Number.isSafeInteger(charIndex))throw new TypeError('getCharBoundaries requires an integer character index.');const box=query(info(ref),{op:'char_boundaries',char_index:charIndex});if(box===null)return null;const Rect=geometry?.Rectangle;return Rect?new Rect(box.x,box.y,box.width,box.height):box;},
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
    const projectResources=Array.isArray(project?.resources)?project.resources:[];
    const embeddingMetadataResource=projectResources.find(r=>String(r?.path||'').toLowerCase()==='fonts/embedding.json');
    const projectFontResourceIds=new Set(projectResources.filter(r=>{
      const mime=String(r?.media_type||r?.mime||'').toLowerCase();
      const name=String(r?.path||r?.name||r?.id||'').toLowerCase();
      return mime.startsWith('font/')||mime.includes('truetype')||mime.includes('opentype')||mime.includes('collection')||mime.includes('woff')||/\.(?:ttf|otf|ttc|otc|woff2?)$/.test(name);
    }).map(r=>String(r?.id||'')));
    const embeddedFaces=(()=>{
      if(!embeddingMetadataResource)return [];
      try{
        const encoded=String(embeddingMetadataResource.data_base64||'');
        if(!encoded||encoded.length>1048576)return [];
        let json;
        if(typeof Buffer!=='undefined')json=Buffer.from(encoded,'base64').toString('utf8');
        else{
          const binary=global.atob(encoded),bytes=Uint8Array.from(binary,ch=>ch.charCodeAt(0));
          json=new TextDecoder('utf-8',{fatal:true}).decode(bytes);
        }
        const entries=JSON.parse(json);
        if(!Array.isArray(entries)||entries.length>4096)return [];
        return entries.filter(entry=>entry&&typeof entry==='object'&&typeof entry.id==='string'&&projectFontResourceIds.has(entry.id)&&typeof entry.family==='string'&&typeof entry.style==='string');
      }catch(_){return [];}
    })();
    const requestedFontStyle=format=>format?.bold?(format?.italic?'bolditalic':'bold'):(format?.italic?'italic':'regular');
    const normalizedFontStyle=value=>String(value||'regular').toLowerCase().replace(/[\s_-]+/g,'');
    function isEmbeddedFontAvailable(format){
      const family=format?.font;
      if(!family)return false;
      const wanted=String(family).trim().toLowerCase(),wantedStyle=requestedFontStyle(format);
      if(registeredFonts.some(f=>String(f?.fontName||'').trim().toLowerCase()===wanted&&normalizedFontStyle(f?.fontStyle)===wantedStyle))return true;
      if(embeddingMetadataResource)return embeddedFaces.some(face=>face.family.trim().toLowerCase()===wanted&&normalizedFontStyle(face.style)===wantedStyle);
      if(projectResources.some(r=>{
        const mime=String(r?.media_type||r?.mime||'').toLowerCase();
        const name=String(r?.path||r?.name||r?.id||'').toLowerCase();
        const isFont=mime.startsWith('font/')||mime.includes('truetype')||mime.includes('opentype')||mime.includes('collection')||mime.includes('woff')||/\.(?:ttf|otf|ttc|otc|woff2?)$/.test(name);
        return isFont&&name.includes(wanted);
      }))return true;
      if(document?.fonts&&typeof document.fonts[Symbol.iterator]==='function'){
        const wantItalic=Boolean(format?.italic),wantWeight=format?.bold?700:400;
        for(const face of document.fonts){
          const faceFam=String(face?.family||'').replace(/^['"]|['"]$/g,'').trim().toLowerCase();
          if(faceFam!==wanted)continue;
          const rawWeight=String(face?.weight||'400').toLowerCase();
          const faceWeight=rawWeight==='bold'?700:(Number(face?.weight)||400);
          const rawStyle=String(face?.style||'normal').toLowerCase();
          const faceItalic=rawStyle==='italic'||rawStyle==='oblique';
          if(faceWeight===wantWeight&&faceItalic===wantItalic&&face?.status==='loaded')return true;
        }
      }
      return false;
    }
    function draw(context,field){
      const result=layout(field);context.save();
      try{
        const surface=result.surface;
        if(surface?.background!==null&&surface?.background!==undefined){context.fillStyle=`rgba(${surface.background>>>16},${surface.background>>>8&255},${surface.background&255},${field.alpha/255})`;context.fillRect(0,0,surface.width,surface.height);}
        if(surface?.border!==null&&surface?.border!==undefined&&surface.width>0&&surface.height>0){const bw=Number(surface.border_width)||Number(field.border_width)||1;context.strokeStyle=`rgba(${surface.border>>>16},${surface.border>>>8&255},${surface.border&255},${field.alpha/255})`;context.lineWidth=bw;context.strokeRect(0.5,0.5,Math.max(0,surface.width-1),Math.max(0,surface.height-1));}
        if(field.box){context.beginPath();context.rect(0,0,field.box.width,field.box.height);context.clip();}
        const antiAlias=field.anti_alias_type||(rendererAntiAliasType==='advanced'?'advanced':'normal');
        const gridFit=field.grid_fit_type||'pixel';
        const thickness=Number(field.thickness)||0;
        const sharpness=Number(field.sharpness)||0;
        const embedOnly=Boolean(field.embed_fonts);
        const sharpMod=antiAlias==='advanced'?Math.max(0.35,Math.min(1,1+sharpness/1600)):1;
        const thickAlphaMod=(antiAlias==='advanced'&&thickness<0)?Math.max(0.4,1+thickness/400):1;
        const baseAlpha=Math.max(0,Math.min(1,(field.alpha/255)*sharpMod*thickAlphaMod));
        const rgb=field.format.color;
        for(const item of result.highlights||[]){const hc=item.color??0xffff00;context.fillStyle=`rgba(${hc>>>16},${hc>>>8&255},${hc&255},${baseAlpha})`;context.fillRect(item.x,item.y,item.width,item.height);}
        applyFont(context,field.format,false,embedOnly,field,surface);context.textAlign='left';context.textBaseline='top';
        context.fillStyle=`rgba(${rgb>>>16},${rgb>>>8&255},${rgb&255},${baseAlpha})`;
        const paintGlyphRun=(textStr,drawX,drawY,fmt)=>{
          const cRgb=fmt.color>>>0,cr=(cRgb>>>16)&255,cg=(cRgb>>>8)&255,cb=cRgb&255;
          if(antiAlias!=='advanced'){
            context.fillStyle=`rgba(${cr},${cg},${cb},${baseAlpha})`;
            context.fillText(textStr,drawX,drawY);
            return;
          }
          const csm=resolveRendererCsm(fmt);
          const quantLevels=Math.max(2,Number(csm.aaLevels)||16);
          const rawAlpha=Math.max(0,Math.min(1,baseAlpha*(Number(csm.contrastScale)||1)*(csm.strokeDelta<0?Math.max(0.45,1+csm.strokeDelta*0.25):1)));
          const effAlpha=csm.maxLevel===0?(rawAlpha>=0.5?1:0):(Math.round(rawAlpha*(quantLevels-1))/(quantLevels-1));
          const effX=csm.maxLevel===0?Math.round(drawX):drawX;
          const effY=csm.maxLevel===0?Math.round(drawY):drawY;
          const subWeight=csm.maxLevel>0?Math.min(1,Math.max(0.15,csm.maxLevel/8)):0;
          if(csm.displayMode==='lcd'&&subWeight>0){
            const fringeA=Math.min(0.45,effAlpha*0.22*subWeight);
            context.fillStyle=`rgba(${Math.min(255,cr+64)},${Math.max(0,cg-24)},${Math.max(0,cb-48)},${fringeA})`;
            context.fillText(textStr,effX-0.33,effY);
            context.fillStyle=`rgba(${Math.max(0,cr-48)},${Math.max(0,cg-24)},${Math.min(255,cb+64)},${fringeA})`;
            context.fillText(textStr,effX+0.33,effY);
          }else if(csm.displayMode==='crt'&&subWeight>0){
            const phosphorA=Math.min(0.4,effAlpha*0.18*subWeight);
            const prevBlur=context.shadowBlur,prevShadowColor=context.shadowColor;
            context.shadowBlur=Math.max(0.4,0.8*subWeight);
            context.shadowColor=`rgba(${cr},${cg},${cb},${phosphorA})`;
            context.fillStyle=`rgba(${cr},${cg},${cb},${phosphorA})`;
            context.fillText(textStr,effX-0.38,effY);
            context.fillText(textStr,effX+0.38,effY);
            context.fillStyle=`rgba(${cr},${cg},${cb},${effAlpha})`;
            context.fillText(textStr,effX,effY);
            context.shadowBlur=prevBlur||0;
            context.shadowColor=prevShadowColor||'';
          }else{
            context.fillStyle=`rgba(${cr},${cg},${cb},${effAlpha})`;
            context.fillText(textStr,effX,effY);
          }
          const strokeWeight=csm.maxLevel>0?(thickness>0?(thickness/200)*0.8:0)+(csm.strokeDelta>0?csm.strokeDelta*0.55:0):0;
          if(strokeWeight>1e-3&&typeof context.strokeText==='function'){
            context.strokeStyle=`rgba(${cr},${cg},${cb},${Math.min(1,effAlpha*Math.min(1,strokeWeight*0.85))})`;
            context.lineWidth=strokeWeight;
            context.strokeText(textStr,effX,effY);
          }
        };
        for(const item of result.runs?.length?result.runs:result.lines){
          const fmt=item.format||field.format;
          if(embedOnly&&registeredFonts.length>0&&!isEmbeddedFontAvailable(fmt))continue;
          if(item.format)applyFont(context,item.format,false,embedOnly,field,surface);
          const gx=gridFit==='pixel'?Math.round(item.x):gridFit==='subpixel'?Math.round(item.x*3)/3:item.x;
          const gy=gridFit==='pixel'?Math.round(item.y):item.y;
          const rot=Number(item.rotation)||0;
          if(Math.abs(rot)>1e-4&&typeof context.translate==='function'&&typeof context.rotate==='function'){
            const cx=gx+(fmt.size||12)*0.3,cy=gy+(fmt.size||12)*0.6;
            context.save();
            context.translate(cx,cy);
            context.rotate(rot*Math.PI/180);
            paintGlyphRun(item.text,gx-cx,gy-cy,fmt);
            context.restore();
          }else{
            paintGlyphRun(item.text,gx,gy,fmt);
          }
        }
        for(const [x,y,radius]of result.bullets||[]){context.beginPath();context.arc(x,y,radius,0,Math.PI*2);context.fill();}
        for(const item of result.decorations||[]){const dc=item.color??rgb;context.fillStyle=`rgba(${dc>>>16},${dc>>>8&255},${dc&255},${baseAlpha})`;context.fillRect(item.x,item.y,item.width,item.height);}
      }finally{context.restore();}
    }
    function hit(field,x,y){const box=bounds(field);return !!box&&box.right>box.left&&box.bottom>box.top&&field.alpha>0&&x>=box.left&&y>=box.top&&x<=box.right&&y<=box.bottom;}
    function textKernel(request){
      active();
      if(as2Execute)return as2Execute(request);
      const runtimeWasm=wasm||(typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm);
      if(typeof runtimeWasm?.as2_text_json!=='function')throw new Error('Rust text kernel is unavailable; rebuild and re-export this project.');
      try{return JSON.parse(runtimeWasm.as2_text_json(JSON.stringify(request)));}
      catch(error){throw error instanceof Error?error:new TypeError(String(error));}
    }
    const defaultRenderer=()=>({displayMode:'default',maxLevel:4,tables:{}});
    let rendererState=defaultRenderer(),rendererAntiAliasType=AntiAliasType.NORMAL,rendererRev=1;
    const rendererCsmCache=new Map();
    function setRendererState(next){
      rendererState=next||defaultRenderer();
      rendererRev++;
      rendererCsmCache.clear();
      invalidateCache();
    }
    function resolveRendererCsm(fmt){
      const fontName=String(fmt?.font||'sans-serif');
      const bold=Boolean(fmt?.bold),italic=Boolean(fmt?.italic),rgb=(Number(fmt?.color)||0)>>>0,size=Number(fmt?.size)||12;
      const cacheKey=`${rendererRev}|${fontName}|${bold?1:0}|${italic?1:0}|${rgb}|${size}`;
      if(rendererCsmCache.has(cacheKey))return rendererCsmCache.get(cacheKey);
      let resolved;
      try{
        resolved=textKernel({op:'renderer_resolve',state:rendererState,font_name:fontName,bold,italic,rgb,font_size:size});
      }catch(_){
        resolved={displayMode:rendererState.displayMode||'default',maxLevel:rendererState.maxLevel??4,aaLevels:1<<Math.min(8,Math.max(0,rendererState.maxLevel??4)),hasCsm:false,strokeDelta:0,contrastScale:1};
      }
      if(rendererCsmCache.size>=1024)rendererCsmCache.clear();
      rendererCsmCache.set(cacheKey,resolved);
      return resolved;
    }
    class TextRenderer {
      constructor(){throw new TypeError('TextRenderer supplies static properties and methods and cannot be constructed.');}
      static setAdvancedAntiAliasingTable(fontName,fontStyle,colorType,table){
        active();
        if(typeof fontName!=='string'||typeof fontStyle!=='string'||typeof colorType!=='string'||!Array.isArray(table)){
          throw new TypeError('TextRenderer.setAdvancedAntiAliasingTable requires (fontName:String, fontStyle:String, colorType:String, table:Array).');
        }
        const entries=table.map(item=>{
          if(!item||typeof item!=='object')throw new TypeError('TextRenderer antialiasing entries require objects with fontSize, insideCutoff and outsideCutoff.');
          return {fontSize:Number(item.fontSize),insideCutoff:Number(item.insideCutoff),outsideCutoff:Number(item.outsideCutoff)};
        });
        const kernelStyle=fontStyle==='regular'?'none':fontStyle;
        setRendererState(textKernel({op:'renderer_table',state:rendererState,font_name:fontName,font_style:kernelStyle,color_type:colorType,table:entries}).state);
      }
    }
    TextRenderer.setAdvancedAntialiasingTable=TextRenderer.setAdvancedAntiAliasingTable;
    Object.defineProperty(TextRenderer,'antiAliasType',{enumerable:true,
      get(){active();return rendererAntiAliasType;},
      set(value){active();if(!['normal','advanced'].includes(value))throw new TypeError('TextRenderer.antiAliasType requires normal or advanced.');rendererAntiAliasType=value;invalidateCache();}});
    for(const name of ['displayMode','maxLevel']){
      Object.defineProperty(TextRenderer,name,{enumerable:true,
        get(){active();return rendererState[name];},
        set(value){
          active();
          if(name==='displayMode'&&!['default','lcd','crt'].includes(String(value))){
            const err=new RangeError("TextRenderer.displayMode requires 'default', 'lcd' or 'crt'.");
            err.name='ArgumentError';
            throw err;
          }
          setRendererState(textKernel({op:'renderer_set',state:rendererState,name,value}).state);
        }});
    }
    Object.defineProperties(TextRenderer,{__nfIs:{value:()=>false},__nfInitialize:{value:()=>{throw new TypeError('TextRenderer cannot be subclassed.');}},[Symbol.hasInstance]:{value:()=>false}});
    Object.freeze(TextRenderer);

    const projectSfntResources=(()=>{
      const out=[],rtWasm=getRuntimeWasm();
      if(typeof rtWasm?.sfnt_font_info_json!=='function')return out;
      const metaById=new Map(embeddedFaces.map(f=>[f.id,f]));
      for(const r of projectResources){
        const id=String(r?.id||'');
        if(!projectFontResourceIds.has(id))continue;
        const bytes=decodeBase64Bytes(r?.data_base64);
        if(!bytes||bytes.length<12)continue;
        const meta=metaById.get(id);
        const fallbackFamily=String(meta?.family||r?.name||r?.path||id||'');
        let desc=null;
        try{
          const raw=rtWasm.sfnt_font_info_json(bytes,fallbackFamily);
          if(raw)desc=JSON.parse(raw);
        }catch(_){desc=null;}
        if(!desc||!desc.font_name)continue;
        const mappedStyle=meta?(()=>{
          const ns=normalizedFontStyle(meta.style);
          return ns==='bolditalic'?FontStyle.BOLD_ITALIC:ns==='bold'?FontStyle.BOLD:ns==='italic'?FontStyle.ITALIC:FontStyle.REGULAR;
        })():(desc.font_style||FontStyle.REGULAR);
        out.push({
          id,
          bytes,
          fontName:meta?.family?String(meta.family):String(desc.font_name),
          rawFontName:String(desc.font_name),
          fontStyle:mappedStyle,
          fontType:desc.font_type||FontType.EMBEDDED,
        });
      }
      return out;
    })();

    const fontRecords=new WeakMap(),fontToken=Symbol('Font'),registeredFonts=[],registeredFontClasses=new Set();
    const deviceGlyphCache=new Map();
    let glyphProbeSurface=undefined;
    function getGlyphProbeContext(){
      if(glyphProbeSurface!==undefined)return glyphProbeSurface;
      glyphProbeSurface=null;
      try{
        const c=typeof createCanvas==='function'?createCanvas(40,40):(typeof global.OffscreenCanvas==='function'?new global.OffscreenCanvas(40,40):document?.createElement?.('canvas'));
        if(c){
          c.width=40;c.height=40;
          const ctx=c.getContext?.('2d',{willReadFrequently:true})||c.getContext?.('2d');
          if(ctx&&typeof ctx.clearRect==='function'&&typeof ctx.fillText==='function'&&typeof ctx.getImageData==='function'){
            ctx.clearRect(0,0,40,40);
            ctx.fillStyle='#ffffff';
            ctx.font='24px sans-serif';
            ctx.textBaseline='top';
            ctx.fillText('A',4,4);
            const data=ctx.getImageData(0,0,40,40)?.data;
            let ink=0;
            if(data)for(let i=3;i<data.length;i+=4)if(data[i]>0){ink=1;break;}
            if(ink>0)glyphProbeSurface=ctx;
          }
        }
      }catch(_){glyphProbeSurface=null;}
      return glyphProbeSurface;
    }
    function glyphRasterSignature(ctx,fontSpec,ch){
      ctx.clearRect(0,0,40,40);
      ctx.font=fontSpec;
      ctx.fillStyle='#ffffff';
      ctx.textBaseline='top';
      ctx.fillText(ch,4,4);
      const w=Number(ctx.measureText?.(ch)?.width)||0;
      const data=ctx.getImageData(0,0,40,40).data;
      let h1=2166136261>>>0,ink=0;
      for(let i=3;i<data.length;i+=4){
        const a=data[i];
        if(a>0){ink++;h1=Math.imul(h1^((i<<8)|a),16777619)>>>0;}
      }
      return `${Math.round(w*100)}:${ink}:${h1}`;
    }
    function inspectDeviceFontHasGlyphs(fontName,fontStyle,value){
      const fam=String(fontName||'sans-serif').trim();
      const stylePrefix=fontStyle===FontStyle.BOLD_ITALIC?'italic bold ':fontStyle===FontStyle.BOLD?'bold ':fontStyle===FontStyle.ITALIC?'italic ':'';
      const genericFamilies=new Set(['sans-serif','serif','monospace','cursive','fantasy','system-ui','_sans','_serif','_typewriter']);
      const ctx=getGlyphProbeContext();
      for(const ch of value){
        const cp=ch.codePointAt(0);
        if(cp<0x20||(cp>=0x7f&&cp<=0x9f)||(cp>=0xd800&&cp<=0xdfff)||cp===0xfffd||(cp>=0xfdd0&&cp<=0xfdef)||(cp&0xfffe)===0xfffe)return false;
        if(cp>=0x323b0&&cp<=0xdffff)return false;
        // Private Use Areas have no universal device-font mapping without Canvas tofu verification.
        if(!ctx&&((cp>=0xe000&&cp<=0xf8ff)||cp>=0xf0000))return false;
      }
      if(!ctx){
        if(!genericFamilies.has(fam.toLowerCase())&&document?.fonts?.check){
          try{if(!document.fonts.check(`${stylePrefix}16px ${cssFontFamily(fam)}`))return false;}catch(_){}
        }
        return true;
      }
      const cssFam=genericFamilies.has(fam.toLowerCase())
        ?(fam==='_sans'?'sans-serif':fam==='_serif'?'serif':fam==='_typewriter'?'monospace':fam.toLowerCase())
        :JSON.stringify(fam);
      if(!genericFamilies.has(fam.toLowerCase())){
        const instKey=`installed|${stylePrefix}|${fam.toLowerCase()}`;
        let installed=deviceGlyphCache.get(instKey);
        if(installed===undefined){
          if(document?.fonts?.check){
            try{installed=Boolean(document.fonts.check(`${stylePrefix}24px ${cssFam}`));}catch(_){installed=undefined;}
          }
          if(installed===undefined){
            const probeStr='mmmmmmmmmmlliWW';
            ctx.font=`${stylePrefix}24px monospace`;const monoW=ctx.measureText(probeStr).width;
            ctx.font=`${stylePrefix}24px ${cssFam}, monospace`;const testMonoW=ctx.measureText(probeStr).width;
            ctx.font=`${stylePrefix}24px serif`;const serifW=ctx.measureText(probeStr).width;
            ctx.font=`${stylePrefix}24px ${cssFam}, serif`;const testSerifW=ctx.measureText(probeStr).width;
            installed=Math.abs(testMonoW-monoW)>0.05||Math.abs(testSerifW-serifW)>0.05;
          }
          if(deviceGlyphCache.size>=4096)deviceGlyphCache.clear();
          deviceGlyphCache.set(instKey,installed);
        }
        if(!installed)return false;
      }
      const fontSpec=`${stylePrefix}24px ${cssFam}`;
      const tofuKey=`tofu|${fontSpec}`;
      let tofuSig=deviceGlyphCache.get(tofuKey);
      if(tofuSig===undefined){
        tofuSig=glyphRasterSignature(ctx,fontSpec,'\uffff');
        deviceGlyphCache.set(tofuKey,tofuSig);
      }
      for(const ch of value){
        const cp=ch.codePointAt(0);
        if(cp===0x20||cp===0xa0)continue;
        const gKey=`glyph|${fontSpec}|${cp}`;
        let ok=deviceGlyphCache.get(gKey);
        if(ok===undefined){
          const sig=glyphRasterSignature(ctx,fontSpec,ch);
          const [,inkStr]=sig.split(':');
          ok=Number(inkStr)>0&&sig!==tofuSig;
          if(deviceGlyphCache.size>=4096)deviceGlyphCache.clear();
          deviceGlyphCache.set(gKey,ok);
        }
        if(!ok)return false;
      }
      return true;
    }
    class Font {
      constructor(token,record){
        active();
        if(token===fontToken){fontRecords.set(this,record);Object.freeze(this);return;}
        if(arguments.length>0)throw new TypeError('Font constructor accepts no arguments.');
        fontRecords.set(this,{fontName:null,fontStyle:FontStyle.REGULAR,fontType:FontType.EMBEDDED,fontBytes:null});
      }
      get fontName(){const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');return record.fontName;}
      get fontStyle(){const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');return record.fontStyle;}
      get fontType(){const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');return record.fontType;}
      hasGlyphs(str){
        const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');
        if(str===null||str===undefined)throw new TypeError('Font.hasGlyphs requires a non-null String.');
        const value=String(str);
        for(let i=0;i<value.length;i++){
          const u=value.charCodeAt(i);
          if(u>=0xd800&&u<=0xdbff){
            const low=value.charCodeAt(++i);
            if(!(low>=0xdc00&&low<=0xdfff))return false;
          }else if(u>=0xdc00&&u<=0xdfff){
            return false;
          }
        }
        if(!value.length)return true;
        const rtWasm=getRuntimeWasm();
        if(record.fontBytes instanceof Uint8Array&&typeof rtWasm?.sfnt_font_has_glyphs==='function'){
          const r=rtWasm.sfnt_font_has_glyphs(record.fontBytes,value);
          if(r===1)return true;
          if(r===0)return false;
        }
        if(record.fontType!==FontType.DEVICE&&record.fontName){
          if(typeof player?.font_has_glyphs_check==='function'){
            try{
              const r=player.font_has_glyphs_check(String(record.fontName),String(record.fontStyle||FontStyle.REGULAR),String(record.fontType||''),value);
              if(r===1)return true;
              if(r===0)return false;
            }catch(_){}
          }
          if(typeof rtWasm?.sfnt_font_has_glyphs==='function'&&projectSfntResources.length){
            const wantName=String(record.fontName).trim().toLowerCase();
            const wantStyle=normalizedFontStyle(record.fontStyle);
            const matched=projectSfntResources.find(f=>(f.fontName.trim().toLowerCase()===wantName||f.rawFontName.trim().toLowerCase()===wantName)&&normalizedFontStyle(f.fontStyle)===wantStyle)
              ||projectSfntResources.find(f=>f.fontName.trim().toLowerCase()===wantName||f.rawFontName.trim().toLowerCase()===wantName);
            if(matched){
              const r=rtWasm.sfnt_font_has_glyphs(matched.bytes,value);
              if(r===1)return true;
              if(r===0)return false;
            }
          }
        }
        if(typeof deviceFontHasGlyphs==='function'){
          const custom=deviceFontHasGlyphs(String(record.fontName||''),String(record.fontStyle||FontStyle.REGULAR),value);
          if(typeof custom==='boolean')return custom;
        }
        return inspectDeviceFontHasGlyphs(record.fontName,record.fontStyle,value);
      }
      static registerFont(fontClass){
        active();
        if(typeof fontClass!=='function'||fontClass===Font)throw new TypeError('ArgumentError: Font.registerFont requires a Font subclass.');
        const prog=getProgram?.();
        const instance=prog?.isClass?.(fontClass)?prog.constructClass(fontClass,[]):new fontClass();
        if(!Font.__nfIs(instance))throw new TypeError('ArgumentError: Font.registerFont requires a Font subclass.');
        const rec=fontRecords.get(instance);
        const explicitBytes=fontClass.__nfFontBytes instanceof Uint8Array?fontClass.__nfFontBytes:(fontClass.fontBytes instanceof Uint8Array?fontClass.fontBytes:null);
        if(rec&&explicitBytes){
          rec.fontBytes=explicitBytes;
          const rtWasm=getRuntimeWasm();
          if(typeof rtWasm?.sfnt_font_info_json==='function'){
            try{
              const fallbackFamily=String(rec.fontName||fontClass.fontName||fontClass.name||'');
              const parsed=rtWasm.sfnt_font_info_json(explicitBytes,fallbackFamily);
              if(parsed){
                const d=JSON.parse(parsed);
                if(d?.font_name&&(!rec.fontName||rec.fontName===fontClass.name))rec.fontName=d.font_name;
                if(d?.font_style)rec.fontStyle=d.font_style;
                if(d?.font_type)rec.fontType=d.font_type;
              }
            }catch(_){}
          }
        }
        if(!registeredFontClasses.has(fontClass)){registeredFontClasses.add(fontClass);registeredFonts.push(instance);invalidateCache();}
      }
      static enumerateFonts(enumerateDeviceFonts=false){
        active();
        const list=registeredFonts.slice();
        const seenEmbedded=new Set(list.map(f=>`${String(f.fontName||'').toLowerCase()}\u0000${normalizedFontStyle(f.fontStyle)}\u0000${String(f.fontType||'')}`));
        if(typeof player?.embedded_fonts_json==='function'){
          try{
            const raw=player.embedded_fonts_json();
            const entries=raw?JSON.parse(raw):[];
            if(Array.isArray(entries)){
              for(const e of entries){
                if(!e?.font_name)continue;
                const fn=String(e.font_name),fs=String(e.font_style||FontStyle.REGULAR),ft=String(e.font_type||FontType.EMBEDDED);
                const key=`${fn.toLowerCase()}\u0000${normalizedFontStyle(fs)}\u0000${ft}`;
                if(seenEmbedded.has(key))continue;
                seenEmbedded.add(key);
                list.push(new Font(fontToken,{fontName:fn,fontStyle:fs,fontType:ft,fontBytes:null}));
              }
            }
          }catch(_){}
        }
        for(const sfnt of projectSfntResources){
          const key=`${sfnt.fontName.toLowerCase()}\u0000${normalizedFontStyle(sfnt.fontStyle)}\u0000${sfnt.fontType}`;
          if(seenEmbedded.has(key))continue;
          seenEmbedded.add(key);
          list.push(new Font(fontToken,{fontName:sfnt.fontName,fontStyle:sfnt.fontStyle,fontType:sfnt.fontType,fontBytes:sfnt.bytes}));
        }
        if(Boolean(enumerateDeviceFonts)){
          const candidates=['sans-serif','serif','monospace','Arial','Helvetica','Times New Roman','Courier New','Verdana','Georgia','Trebuchet MS'];
          if(document?.fonts&&typeof document.fonts[Symbol.iterator]==='function'){
            for(const face of document.fonts){if(typeof face?.family==='string')candidates.push(face.family);if(candidates.length>=4096)break;}
          }
          if(Array.isArray(project?.symbols)){
            for(const symbol of project.symbols)for(const layer of symbol?.timeline?.layers||[])for(const keyframe of layer?.keyframes||[])for(const element of keyframe?.elements||[]){
              const family=element?.kind?.font_family||element?.kind?.field?.format?.font;
              if(typeof family==='string'&&candidates.length<4096)candidates.push(family);
            }
          }
          let families;
          try{families=textKernel({op:'font_list',candidates}).result;}
          catch(_){families=[...new Set(candidates)].sort((a,b)=>a.localeCompare(b));}
          for(const family of families)list.push(new Font(fontToken,{fontName:family,fontStyle:FontStyle.REGULAR,fontType:FontType.DEVICE,fontBytes:null}));
        }
        return list;
      }
    }
    Font.__nfIs=value=>fontRecords.has(value);
    Font.__nfAllocate=({className,fontName,fontStyle,fontType,fontBytes}={})=>{
      const instance=Object.create(Font.prototype);
      const shortName=typeof className==='string'&&className.length?className.split('.').pop():'EmbeddedFont';
      fontRecords.set(instance,{
        fontName:typeof fontName==='string'&&fontName.length?fontName:shortName,
        fontStyle:typeof fontStyle==='string'&&fontStyle.length?fontStyle:FontStyle.REGULAR,
        fontType:typeof fontType==='string'&&fontType.length?fontType:FontType.EMBEDDED,
        fontBytes:fontBytes instanceof Uint8Array?fontBytes:null,
      });
      return instance;
    };
    Font.__nfInitialize=(receiver,args=[])=>{
      if(!fontRecords.has(receiver))throw new TypeError('Font base initialization requires a Font instance.');
      if(args.length)throw new TypeError('Font base constructor accepts no arguments.');
    };
    Object.defineProperty(Font,Symbol.hasInstance,{value:Font.__nfIs});

    function styleObjectRecord(value){
      if(value===null||value===undefined)return null;
      if(typeof value!=='object')throw new TypeError('StyleSheet styles require ordinary data objects.');
      const result=Object.create(null),entries=Object.entries(Object.getOwnPropertyDescriptors(value));
      if(entries.length>128)throw new RangeError('StyleSheet styles support at most 128 properties.');
      for(const [name,item]of entries){
        if(!item.enumerable||item.value===undefined)continue;
        if(!Object.hasOwn(item,'value')||(!['string','number','boolean'].includes(typeof item.value)&&item.value!==null)){
          throw new TypeError('StyleSheet styles require primitive own data properties.');
        }
        result[name]=item.value;
      }
      return result;
    }
    const styleSheetRecords=new WeakMap();
    const EventDispatcherBase=events?.EventDispatcher||class{};
    function requireStyleSheet(target){active();const found=styleSheetRecords.get(target);if(!found)throw new TypeError('Expected flash.text.StyleSheet.');return found;}
    function reflowStyleSheet(sheet){
      for(const [ref,st]of fieldStates){
        if(st.style!==sheet||st.source===null)continue;
        if(typeof player.object_alive==='function'&&!player.object_alive(ref)){fieldStates.delete(ref);continue;}
        try{renderHtml(ref,st,st.source);}catch(_){}
      }
    }
    class StyleSheet extends EventDispatcherBase {
      constructor(...args){
        active();
        if(args.length)throw new TypeError('StyleSheet constructor accepts no arguments.');
        super();
        styleSheetRecords.set(this,{state:{styles:{}}});
      }
      get styleNames(){return textKernel({op:'names',state:requireStyleSheet(this).state}).result;}
      clear(){requireStyleSheet(this).state=textKernel({op:'clear'}).state;reflowStyleSheet(this);}
      getStyle(styleName){
        if(typeof styleName!=='string')throw new TypeError('StyleSheet.getStyle requires a String.');
        return textKernel({op:'get_style',state:requireStyleSheet(this).state,selector:styleName}).result;
      }
      setStyle(styleName,styleObject){
        if(typeof styleName!=='string')throw new TypeError('StyleSheet.setStyle requires a String.');
        const current=requireStyleSheet(this);
        current.state=textKernel({op:'set_style',state:current.state,selector:styleName,style:styleObjectRecord(styleObject)}).state;
        reflowStyleSheet(this);
      }
      parseCSS(cssText){
        if(typeof cssText!=='string')throw new TypeError('StyleSheet.parseCSS requires a String.');
        const current=requireStyleSheet(this);
        const packet=textKernel({op:'parse_css',state:current.state,source:cssText});
        if(packet.result){current.state=packet.state;reflowStyleSheet(this);}
      }
      transform(formatObject){
        requireStyleSheet(this);
        if(formatObject===undefined||formatObject===null)return null;
        const patch=textKernel({op:'transform',style:styleObjectRecord(formatObject)}).result;
        return formatCopy(patch);
      }
    }
    StyleSheet.__nfIs=value=>styleSheetRecords.has(value);
    StyleSheet.__nfInitialize=()=>{throw new TypeError('Native StyleSheet subclassing is not supported.');};
    Object.defineProperty(StyleSheet,Symbol.hasInstance,{value:StyleSheet.__nfIs});

    const snapshots=new WeakMap();
    function snapshotRecord(receiver){active();const state=snapshots.get(receiver);if(!state)throw new TypeError('Expected TextSnapshot from this publication.');return state;}
    function snapshotCall(receiver,request){return JSON.parse(player.as2_snapshot_command(snapshotRecord(receiver),JSON.stringify(request)));}
    function snapshotIndex(value){if(!Number.isSafeInteger(value))throw new TypeError('TextSnapshot indices require safe integers.');return value;}
    const decodeUnits=value=>String.fromCharCode(...value.units);
    class TextSnapshot {
      constructor(){
        const err=new RangeError('Error #2012: TextSnapshot$ class cannot be instantiated.');
        err.name='ArgumentError';err.errorID=2012;throw err;
      }
      get charCount(){return snapshotCall(this,{op:'count'});}
      getText(beginIndex,endIndex,includeLineEndings=false){
        return decodeUnits(snapshotCall(this,{op:'get_text',start:snapshotIndex(beginIndex),end:endIndex===undefined?null:snapshotIndex(endIndex),include_line_endings:Boolean(includeLineEndings)}));
      }
      findText(beginIndex,textToFind,caseSensitive){
        if(typeof textToFind!=='string')throw new TypeError('TextSnapshot.findText requires a String.');
        const query=[];for(let i=0;i<textToFind.length;i++)query.push(textToFind.charCodeAt(i));
        return snapshotCall(this,{op:'find',start:snapshotIndex(beginIndex),query,case_sensitive:Boolean(caseSensitive)});
      }
      setSelected(beginIndex,endIndex,select){
        snapshotCall(this,{op:'set_selected',start:snapshotIndex(beginIndex),end:snapshotIndex(endIndex),selected:Boolean(select)});
      }
      getSelected(beginIndex,endIndex){
        return snapshotCall(this,{op:'get_selected',start:snapshotIndex(beginIndex),end:endIndex===undefined?null:snapshotIndex(endIndex)});
      }
      getSelectedText(includeLineEndings=false){
        return decodeUnits(snapshotCall(this,{op:'selected_text',include_line_endings:Boolean(includeLineEndings)}));
      }
      setSelectColor(hexColor=0xffff00){
        if(!Number.isInteger(hexColor)||hexColor<0||hexColor>0xffffff)throw new RangeError('TextSnapshot color requires a 24-bit RGB integer.');
        snapshotCall(this,{op:'select_color',color:hexColor});
      }
      getTextRunInfo(beginIndex,endIndex){
        return snapshotCall(this,{op:'run_info',start:snapshotIndex(beginIndex),end:snapshotIndex(endIndex)});
      }
      hitTestTextNearPos(x,y,maxDistance=0){
        if(![x,y,maxDistance].every(Number.isFinite))throw new TypeError('TextSnapshot hit test requires finite numbers.');
        return snapshotCall(this,{op:'hit_test',x,y,distance:maxDistance});
      }
    }
    TextSnapshot.__nfIs=value=>snapshots.has(value);
    TextSnapshot.__nfInitialize=()=>{throw new TypeError('TextSnapshot cannot be subclassed.');};
    Object.defineProperty(TextSnapshot,Symbol.hasInstance,{value:TextSnapshot.__nfIs});
    function snapshotFor(ref){
      active();
      const handle=player.as2_snapshot_create(ref,(strings,format)=>JSON.stringify(JSON.parse(strings).map(value=>measureText(value,JSON.parse(format)))));
      const result=Object.create(TextSnapshot.prototype);
      snapshots.set(result,handle);
      return Object.seal(result);
    }
    function linkAt(ref,point){
      active();
      const field=info(ref),result=layout(field);
      if(!field.format.url&&!(field.format_runs||[]).some(r=>r.format?.url)&&!(result.runs||[]).some(r=>r.format?.url))return null;
      return textKernel({op:'hit_link',field,layout:result,x:point.x,y:point.y});
    }
    function isFontCompatible(fontName,fontStyle){
      active();
      if(fontName==null||fontStyle==null)throw new TypeError('Error #2007: Parameter fontName and fontStyle must be non-null.');
      const style=String(fontStyle);
      if(!['regular','bold','italic','boldItalic'].includes(style)){
        const err=new RangeError('Error #2008: Parameter fontStyle must be one of the accepted values.');
        err.name='ArgumentError';err.errorID=2008;throw err;
      }
      const wanted=String(fontName);
      return Font.enumerateFonts(true).some(f=>f.fontName===wanted&&(f.fontType===FontType.DEVICE||f.fontStyle===style));
    }

    function toOutlines(ref){
      active();
      const field=info(ref);
      const runtimeWasm=wasm||(typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm);
      const fn=player?.text_to_outlines_json?.bind(player)||runtimeWasm?.text_to_outlines_json;
      if(typeof fn!=='function')throw new Error('Text outline conversion kernel is unavailable.');
      return JSON.parse(fn(JSON.stringify(field)));
    }
    function stripNewlinesFromRuns(normText,rawRuns){
      if(!Array.isArray(rawRuns)||!rawRuns.length||!normText.includes('\n'))return {text:normText,runs:rawRuns};
      const map=new Uint32Array(normText.length+1);
      let out='',pos=0;
      for(let i=0;i<normText.length;i++){
        map[i]=pos;
        if(normText[i]!=='\n'){out+=normText[i];pos++;}
      }
      map[normText.length]=pos;
      const runs=[];
      for(const r of rawRuns){
        const s=map[Math.max(0,Math.min(normText.length,Number(r.start??r.begin)||0))];
        const e=map[Math.max(0,Math.min(normText.length,Number(r.end)||0))];
        if(s<e)runs.push({start:s,end:e,format:structuredClone?structuredClone(r.format):JSON.parse(JSON.stringify(r.format||{}))});
      }
      return {text:out,runs};
    }
    function writeRichClipboard(ref,field,begin,end,clipboardData=null){
      if(field.display_as_password)return null;
      const lo=Math.max(0,Math.min(field.text.length,Math.min(begin,end)));
      const hi=Math.max(lo,Math.min(field.text.length,Math.max(begin,end)));
      if(lo>=hi)return null;
      const plain=field.text.slice(lo,hi);
      const useRich=Boolean(fieldStates.get(ref)?.useRichTextClipboard);
      const runs=useRich?sliceFieldRuns(field,lo,hi):null;
      let html='';
      if(useRich){
        try{html=textKernel({op:'serialize_html',text:plain,format:field.format,format_runs:runs}).result||'';}catch(_){}
      }
      sharedRichClipboard={text:plain,runs,html};
      if(clipboardData?.setData){
        try{
          clipboardData.setData('text/plain',plain);
          if(useRich){
            clipboardData.setData(RICH_CLIPBOARD_MIME,JSON.stringify({text:plain,runs}));
            if(html)clipboardData.setData('text/html',html);
          }
        }catch(_){}
      }
      return sharedRichClipboard;
    }
    function readRichClipboard(ref,field,clipboardData=null,fallbackPlain=null){
      const plainFromCb=clipboardData?.getData?clipboardData.getData('text/plain'):'';
      let plain=typeof fallbackPlain==='string'?fallbackPlain:(plainFromCb||sharedRichClipboard?.text||'');
      plain=String(plain??'').replace(/\r\n?|\n/g,'\n');
      const multiline=field?.multiline!==false;
      const useRich=Boolean(fieldStates.get(ref)?.useRichTextClipboard);
      if(!useRich){
        return {text:multiline?plain:plain.replace(/\n/g,''),runs:null};
      }
      if(clipboardData?.getData){
        try{
          const rawJson=clipboardData.getData(RICH_CLIPBOARD_MIME);
          if(rawJson){
            const parsed=JSON.parse(rawJson);
            if(parsed&&typeof parsed.text==='string'&&Array.isArray(parsed.runs)){
              const normText=parsed.text.replace(/\r\n?|\n/g,'\n');
              if(!plain||plain===normText){
                return multiline?{text:normText,runs:parsed.runs}:stripNewlinesFromRuns(normText,parsed.runs);
              }
            }
          }
        }catch(_){}
        try{
          const rawHtml=clipboardData.getData('text/html');
          if(rawHtml){
            const frag=extractHtmlFragment(rawHtml);
            const parsed=textKernel({op:'html',source:frag,format:field.default_format||field.format,styles:{styles:{}},condense_white:false,multi_run:true});
            if(parsed&&typeof parsed.text==='string'){
              const normHtmlText=parsed.text.replace(/\r\n?|\n/g,'\n');
              if(!plain||plain===normHtmlText){
                const runs=(parsed.format_runs&&parsed.format_runs.length)?parsed.format_runs:(normHtmlText.length?[{start:0,end:normHtmlText.length,format:parsed.format||field.default_format}]:[]);
                return multiline?{text:normHtmlText,runs}:stripNewlinesFromRuns(normHtmlText,runs);
              }
            }
          }
        }catch(_){}
      }
      if(sharedRichClipboard&&sharedRichClipboard.text===plain&&Array.isArray(sharedRichClipboard.runs)&&sharedRichClipboard.runs.length){
        const cloned=sharedRichClipboard.runs.map(r=>({start:r.start,end:r.end,format:{...r.format}}));
        return multiline?{text:plain,runs:cloned}:stripNewlinesFromRuns(plain,cloned);
      }
      return {text:multiline?plain:plain.replace(/\n/g,''),runs:null};
    }

    const domIme=new WeakMap(),domCleanups=[];
    const domDisplayed=field=>field.multiline?String(field.text??''):String(field.text??'').replace(/\n/g,' ');
    const domCanEdit=field=>Boolean(field&&field.field_type==='input');
    function resolveDomInputTarget(ev){
      const el=ev?.target;
      if(!el||!el.classList?.contains?.('nf-dom-input'))return null;
      const attr=String(el.getAttribute?.('data-nf-id')||'');
      if(!attr.startsWith('input:'))return null;
      const ownerKey=attr.slice(6);
      if(!ownerKey||(typeof player.object_alive==='function'&&!player.object_alive(ownerKey)))return null;
      let field;
      try{field=info(ownerKey);}catch(_){return null;}
      return {el,ownerKey,field};
    }
    function syncDomSelectionToPlayer(el,ownerKey){
      if(domIme.has(el)||typeof el.selectionStart!=='number'||typeof el.selectionEnd!=='number')return;
      let field;
      try{field=info(ownerKey);}catch(_){return;}
      const rtWasm=getRuntimeWasm();
      const s=snapGraphemeFloor(field.text,el.selectionStart,rtWasm);
      const e=el.selectionEnd>el.selectionStart?snapGraphemeCeil(field.text,el.selectionEnd,rtWasm):s;
      const backward=el.selectionDirection==='backward'&&s!==e;
      const caret=backward?s:e;
      if(s!==field.selection_begin||e!==field.selection_end||caret!==field.caret){
        player.object_text_update(ownerKey,JSON.stringify({selection:{begin:backward?e:s,end:backward?s:e}}));
      }
    }
    if(typeof document?.addEventListener==='function'){
      const addDocCapture=(type,handler)=>{
        const wrapped=ev=>{if(!disposed)handler(ev);};
        document.addEventListener(type,wrapped,true);
        domCleanups.push(()=>document.removeEventListener(type,wrapped,true));
      };
      for(const guardEvt of ['pointerdown','mousedown','focusin']){
        addDocCapture(guardEvt,ev=>{
          const hit=resolveDomInputTarget(ev);if(!hit)return;
          hit.el.readOnly=!domCanEdit(hit.field)||Boolean(hit.el.disabled);
        });
      }
      const AS2_IME_DOC_HOOKS=Symbol.for('neoflash.as2.imeDocHooks.v1');
      const AS3_IME_REGISTRY=Symbol.for('neoflash.as3.imeRegistry.v1');
      function notifyCapturedImeHooks(phase,ev,receiver=null){
        const docRef=ev?.target?.ownerDocument||document||global.document||null;
        const as2Reg=global[AS2_IME_DOC_HOOKS];
        if(as2Reg){
          const set=docRef&&typeof docRef==='object'?as2Reg.byDoc?.get(docRef):null;
          if(set)for(const fn of set)fn(phase,ev);
          for(const fn of as2Reg.globalHooks||[])fn(phase,ev);
        }
        const as3Reg=global[AS3_IME_REGISTRY];
        const netSysHook=as3Reg?.latest?.netSys;
        if(netSysHook){
          const txt=typeof ev?.data==='string'?ev.data:'';
          netSysHook.notifyDomComposition?.(phase,txt,receiver);
        }
      }
      function isGlobalAs3ImeDisabled(){
        const as3Reg=global[AS3_IME_REGISTRY];
        const netSysHook=as3Reg?.latest?.netSys;
        return Boolean(netSysHook&&!netSysHook.isEnabled());
      }
      addDocCapture('compositionstart',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit)return;
        if(!domCanEdit(hit.field)||hit.el.disabled||isGlobalAs3ImeDisabled()){ev.preventDefault?.();ev.stopImmediatePropagation?.();return;}
        ev.stopImmediatePropagation?.();
        syncDomSelectionToPlayer(hit.el,hit.ownerKey);
        const snap=info(hit.ownerKey);
        domIme.set(hit.el,{ownerKey:hit.ownerKey,baseText:hit.el.value,previewValue:hit.el.value,snapshot:snap,cancelled:false});
        const receiver=resolveReceiver(hit.ownerKey);
        notifyCapturedImeHooks('start',ev,receiver);
        if(events){
          const Evt=events.IMEEvent||events.TextEvent;
          if(receiver&&Evt)receiver.dispatchEvent?.(new Evt('imeStartComposition',false,true,typeof ev.data==='string'?ev.data:''));
        }
      });
      addDocCapture('compositionupdate',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit||!domIme.has(hit.el))return;
        ev.stopImmediatePropagation?.();
        const ime=domIme.get(hit.el);
        const previewVal=hit.el.value,pStart=hit.el.selectionStart,pEnd=hit.el.selectionEnd;
        if(ime)ime.previewValue=previewVal;
        const receiver=resolveReceiver(hit.ownerKey);
        notifyCapturedImeHooks('update',ev,receiver);
        if(events){
          const Evt=events.IMEEvent||events.TextEvent;
          if(receiver&&Evt)receiver.dispatchEvent?.(new Evt('imeComposition',false,true,typeof ev.data==='string'?ev.data:''));
        }
        if(hit.el.value!==previewVal){
          hit.el.value=previewVal;
          if(typeof pStart==='number'&&typeof pEnd==='number'){try{hit.el.setSelectionRange?.(pStart,pEnd,'forward');}catch(_){}}
        }
      });
      function commitDomEdit(el,ownerKey,beforeField,nextDomValue){
        const rtWasm=getRuntimeWasm();
        const before=domDisplayed(beforeField);
        const normalized=String(nextDomValue??'').replace(/\r\n?|\n/g,'\n');
        const edit=graphemeSafeDiff(before,normalized,rtWasm);
        const rawInsert=beforeField.multiline?edit.text:edit.text.replace(/\n/g,'');
        if(!rawInsert.length&&edit.begin===edit.end)return;
        const receiver=resolveReceiver(ownerKey);
        let finalInsert=rawInsert;
        if(rawInsert.length&&receiver&&events?.TextEvent){
          const textEvt=new events.TextEvent(events.TextEvent.TEXT_INPUT||'textInput',true,true,rawInsert);
          if(receiver.dispatchEvent?.(textEvt)===false){
            const cur=info(ownerKey);
            el.value=domDisplayed(cur);
            if(typeof el.setSelectionRange==='function'){
              try{el.setSelectionRange(cur.selection_begin||0,cur.selection_end||0,'forward');}catch(_){}
            }
            onUpdate(ownerKey);
            return;
          }
          finalInsert=String(textEvt.text??'');
        }
        const post=info(ownerKey);
        const postEdit=post.revision===beforeField.revision
          ?{begin:edit.begin,end:edit.end,text:finalInsert}
          :(()=>{
              const d=graphemeSafeDiff(domDisplayed(post),normalized,rtWasm);
              return {begin:d.begin,end:d.end,text:post.multiline?d.text:d.text.replace(/\n/g,'')};
            })();
        const committed=JSON.parse(player.object_text_input(ownerKey,JSON.stringify({
          expected_revision:post.revision,
          begin:postEdit.begin,
          end:postEdit.end,
          text:postEdit.text,
        })));
        const nextDisp=domDisplayed(committed);
        if(el.value!==nextDisp)el.value=nextDisp;
        if(typeof el.setSelectionRange==='function'&&Number.isInteger(committed.caret)){
          try{el.setSelectionRange(committed.caret,committed.caret,'forward');}catch(_){}
        }
        if(committed.text!==beforeField.text){
          if(receiver&&events?.Event)receiver.dispatchEvent?.(new events.Event(events.Event.CHANGE||'change',true,false));
          if(receiver&&typeof receiver.onChanged==='function')receiver.onChanged(receiver);
          as2TextEvent?.(ownerKey,'onChanged',[receiver]);
        }
        onUpdate(ownerKey);
      }
      addDocCapture('input',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit)return;
        if(!domCanEdit(hit.field)||hit.el.disabled){
          hit.el.value=domDisplayed(hit.field);
          ev.preventDefault?.();ev.stopImmediatePropagation?.();return;
        }
        if(domIme.has(hit.el)||ev.isComposing){
          const ime=domIme.get(hit.el);
          if(ime)ime.previewValue=hit.el.value;
          ev.stopImmediatePropagation?.();return;
        }
        // Guard against surrogate-pair or extended-grapheme-cluster splits in dom-layout.js's
        // UTF-16 code-unit diff (e.g. replacing 😀 \uD83D\uDE00 with 😁 \uD83D\uDE01 or editing
        // decomposed combining marks / ZWJ sequences).
        const before=domDisplayed(hit.field),after=String(hit.el.value??'').replace(/\r\n?|\n/g,'\n');
        let rawBegin=0;
        while(rawBegin<before.length&&rawBegin<after.length&&before[rawBegin]===after[rawBegin])rawBegin++;
        let rawEnd=before.length,afterEnd=after.length;
        while(rawEnd>rawBegin&&afterEnd>rawBegin&&before[rawEnd-1]===after[afterEnd-1]){rawEnd--;afterEnd--;}
        const edit=graphemeSafeDiff(before,after,getRuntimeWasm());
        if(edit.begin!==rawBegin||edit.end!==rawEnd){
          ev.stopImmediatePropagation?.();
          commitDomEdit(hit.el,hit.ownerKey,hit.field,after);
        }
      });
      addDocCapture('compositionend',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit)return;
        const ime=domIme.get(hit.el);
        domIme.delete(hit.el);
        if(!ime)return;
        ev.stopImmediatePropagation?.();
        if(ime.cancelled||!domCanEdit(hit.field)||hit.el.disabled){
          hit.el.value=ime.baseText;
          const s=ime.snapshot;
          try{hit.el.setSelectionRange?.(s.selection_begin||0,s.selection_end||0);}catch(_){}
          return;
        }
        let committedDomValue=hit.el.value;
        const evText=typeof ev.data==='string'?ev.data:'';
        if(committedDomValue===ime.baseText&&evText.length>0){
          const sb=ime.snapshot?.selection_begin||0,se=ime.snapshot?.selection_end??sb;
          committedDomValue=ime.baseText.slice(0,sb)+evText+ime.baseText.slice(se);
        }
        const receiver=resolveReceiver(hit.ownerKey);
        notifyCapturedImeHooks('end',ev,receiver);
        if(events){
          const Evt=events.IMEEvent||events.TextEvent;
          if(receiver&&Evt)receiver.dispatchEvent?.(new Evt('imeComposition',false,true,evText));
        }
        if(committedDomValue===ime.baseText)return;
        commitDomEdit(hit.el,hit.ownerKey,ime.snapshot,committedDomValue);
      });
      addDocCapture('keydown',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit)return;
        const {el,ownerKey,field}=hit;
        const rtWasm=getRuntimeWasm();
        if(domIme.has(el)&&ev.key==='Escape'){
          const ime=domIme.get(el);
          ime.cancelled=true;
          domIme.delete(el);
          el.value=ime.baseText;
          const s=ime.snapshot;
          try{el.setSelectionRange?.(s.selection_begin||0,s.selection_end||0);}catch(_){}
          ev.preventDefault?.();ev.stopImmediatePropagation?.();
          return;
        }
        if(!domCanEdit(field)||el.disabled){
          el.readOnly=true;
          if(ev.key==='Backspace'||ev.key==='Delete'||ev.key==='Enter'||(typeof ev.key==='string'&&ev.key.length===1&&!ev.ctrlKey&&!ev.metaKey&&!ev.altKey)){
            ev.preventDefault?.();ev.stopImmediatePropagation?.();
            return;
          }
        }
        if(domIme.has(el)||ev.isComposing||ev.altKey||ev.ctrlKey||ev.metaKey)return;
        if((ev.key==='ArrowLeft'||ev.key==='ArrowRight')&&typeof el.selectionStart==='number'&&typeof el.selectionEnd==='number'){
          const val=el.value;
          const lo=el.selectionStart,hi=el.selectionEnd;
          const backward=el.selectionDirection==='backward'&&lo!==hi;
          const anchor=backward?hi:lo,head=backward?lo:hi;
          if(ev.shiftKey){
            const nextHead=ev.key==='ArrowLeft'?prevGrapheme(val,head,rtWasm):nextGrapheme(val,head,rtWasm);
            const nLo=Math.min(anchor,nextHead),nHi=Math.max(anchor,nextHead),dir=nextHead<anchor?'backward':'forward';
            el.setSelectionRange(nLo,nHi,dir);
            player.object_text_update(ownerKey,JSON.stringify({selection:{begin:anchor,end:nextHead}}));
          }else{
            const nextPos=lo!==hi?(ev.key==='ArrowLeft'?snapGraphemeFloor(val,lo,rtWasm):snapGraphemeCeil(val,hi,rtWasm)):(ev.key==='ArrowLeft'?prevGrapheme(val,lo,rtWasm):nextGrapheme(val,hi,rtWasm));
            el.setSelectionRange(nextPos,nextPos,'forward');
            player.object_text_update(ownerKey,JSON.stringify({selection:{begin:nextPos,end:nextPos}}));
          }
          ev.preventDefault?.();ev.stopImmediatePropagation?.();
          return;
        }
        if((ev.key==='Backspace'||ev.key==='Delete')&&domCanEdit(field)&&!el.disabled&&typeof el.selectionStart==='number'&&typeof el.selectionEnd==='number'){
          const val=el.value,lo=el.selectionStart,hi=el.selectionEnd;
          const delBegin=lo<hi?snapGraphemeFloor(val,lo,rtWasm):(ev.key==='Backspace'?prevGrapheme(val,lo,rtWasm):snapGraphemeFloor(val,lo,rtWasm));
          const delEnd=lo<hi?snapGraphemeCeil(val,hi,rtWasm):(ev.key==='Delete'?nextGrapheme(val,lo,rtWasm):snapGraphemeCeil(val,lo,rtWasm));
          ev.preventDefault?.();ev.stopImmediatePropagation?.();
          if(delBegin<delEnd){
            const nextVal=val.slice(0,delBegin)+val.slice(delEnd);
            el.value=nextVal;
            el.setSelectionRange(delBegin,delBegin,'forward');
            commitDomEdit(el,ownerKey,field,nextVal);
            player.object_text_update(ownerKey,JSON.stringify({selection:{begin:delBegin,end:delBegin}}));
          }
        }
      });
      for(const selEvt of ['select','keyup','mouseup']){
        addDocCapture(selEvt,ev=>{
          const hit=resolveDomInputTarget(ev);if(!hit)return;
          syncDomSelectionToPlayer(hit.el,hit.ownerKey);
        });
      }
      addDocCapture('copy',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit)return;
        const {el,ownerKey,field}=hit;
        if(field.display_as_password)return;
        const lo=el.selectionStart??field.selection_begin??0,hi=el.selectionEnd??field.selection_end??0;
        if(lo<hi&&ev.clipboardData?.setData){
          writeRichClipboard(ownerKey,field,lo,hi,ev.clipboardData);
          ev.preventDefault?.();
        }else if(lo<hi){
          writeRichClipboard(ownerKey,field,lo,hi,null);
        }
      });
      addDocCapture('cut',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit)return;
        const {el,ownerKey,field}=hit;
        if(!domCanEdit(field)||el.disabled){ev.preventDefault?.();ev.stopImmediatePropagation?.();return;}
        const rtWasm=getRuntimeWasm();
        const lo=snapGraphemeFloor(el.value,el.selectionStart??field.selection_begin??0,rtWasm);
        const hi=snapGraphemeCeil(el.value,el.selectionEnd??field.selection_end??0,rtWasm);
        if(lo>=hi){ev.preventDefault?.();ev.stopImmediatePropagation?.();return;}
        writeRichClipboard(ownerKey,field,lo,hi,ev.clipboardData);
        const nextVal=el.value.slice(0,lo)+el.value.slice(hi);
        el.value=nextVal;
        el.setSelectionRange(lo,lo,'forward');
        ev.preventDefault?.();ev.stopImmediatePropagation?.();
        commitDomEdit(el,ownerKey,field,nextVal);
        player.object_text_update(ownerKey,JSON.stringify({selection:{begin:lo,end:lo}}));
      });
      addDocCapture('paste',ev=>{
        const hit=resolveDomInputTarget(ev);if(!hit)return;
        const {el,ownerKey,field}=hit;
        if(!domCanEdit(field)||el.disabled){ev.preventDefault?.();ev.stopImmediatePropagation?.();return;}
        const payload=readRichClipboard(ownerKey,field,ev.clipboardData,null);
        const rawInsert=field.multiline?payload.text:payload.text.replace(/\n/g,'');
        if(!rawInsert.length){ev.preventDefault?.();ev.stopImmediatePropagation?.();return;}
        const rtWasm=getRuntimeWasm();
        const begin=snapGraphemeFloor(field.text,el.selectionStart??field.selection_begin??0,rtWasm);
        const rawEnd=el.selectionEnd??field.selection_end??begin;
        const end=rawEnd>begin?snapGraphemeCeil(field.text,rawEnd,rtWasm):begin;
        if(Array.isArray(payload.runs)&&payload.runs.length&&typeof player.object_text_rich_input==='function'){
          ev.preventDefault?.();ev.stopImmediatePropagation?.();
          const receiver=resolveReceiver(ownerKey);
          let finalInsert=rawInsert;
          if(receiver&&events?.TextEvent){
            const textEvt=new events.TextEvent(events.TextEvent.TEXT_INPUT||'textInput',true,true,rawInsert);
            if(receiver.dispatchEvent?.(textEvt)===false)return;
            finalInsert=String(textEvt.text??'');
          }
          const post=info(ownerKey);
          const committed=JSON.parse(player.object_text_rich_input(ownerKey,JSON.stringify({
            expected_revision:post.revision,
            begin,
            end,
            text:finalInsert,
            runs:finalInsert===rawInsert?payload.runs:[],
          })));
          const nextDisp=domDisplayed(committed);
          if(el.value!==nextDisp)el.value=nextDisp;
          if(typeof el.setSelectionRange==='function'&&Number.isInteger(committed.caret)){
            try{el.setSelectionRange(committed.caret,committed.caret,'forward');}catch(_){}
          }
          if(committed.text!==field.text){
            if(receiver&&events?.Event){
              receiver.dispatchEvent?.(new events.Event(events.Event.CHANGE||'change',true,false));
            }
            if(receiver&&typeof receiver.onChanged==='function')receiver.onChanged(receiver);
            as2TextEvent?.(ownerKey,'onChanged',[receiver]);
          }
          onUpdate(ownerKey);
          return;
        }
        ev.preventDefault?.();ev.stopImmediatePropagation?.();
        const nextVal=el.value.slice(0,begin)+rawInsert+el.value.slice(end);
        el.value=nextVal;
        const caret=begin+rawInsert.length;
        try{el.setSelectionRange(caret,caret,'forward');}catch(_){}
        commitDomEdit(el,ownerKey,field,nextVal);
      });
    }

    function invalidateCache(){cache.clear();cacheBytes=0;}
    function forgetField(ref){fieldStates.delete(ref);}
    function pruneDeadFields(){if(typeof player.object_alive!=='function')return;for(const ref of [...fieldStates.keys()])if(!player.object_alive(ref))fieldStates.delete(ref);}
    function reset(){active();invalidateCache();fieldStates.clear();sharedRichClipboard=null;setRendererState(defaultRenderer());rendererAntiAliasType=AntiAliasType.NORMAL;registeredFonts.length=0;registeredFontClasses.clear();}
    function dispose(){if(disposed)return;reset();for(const fn of domCleanups)fn();domCleanups.length=0;disposed=true;}
    return Object.freeze({
      measure:(strings,format)=>strings.map(value=>measureText(value,format)),
      portableMeasure:(strings,format)=>strings.map(value=>defaultPortableMeasure(value,format)),
      graphemeBoundaries:value=>graphemeBoundaries(value,getRuntimeWasm()),
      snapGraphemeFloor:(value,idx)=>snapGraphemeFloor(value,idx,getRuntimeWasm()),
      snapGraphemeCeil:(value,idx)=>snapGraphemeCeil(value,idx,getRuntimeWasm()),
      prevGrapheme:(value,idx)=>prevGrapheme(value,idx,getRuntimeWasm()),
      nextGrapheme:(value,idx)=>nextGrapheme(value,idx,getRuntimeWasm()),
      graphemeSafeDiff:(before,after)=>graphemeSafeDiff(before,after,getRuntimeWasm()),
      sliceFieldRuns,
      writeRichClipboard,
      readRichClipboard,
      clearRichClipboard:()=>{sharedRichClipboard=null;},
      getRendererState:()=>rendererState,
      setRendererState,
      resolveRendererCsm,
      toOutlines,TextFormat,As2TextFormat,legacyFormatCopy,formatPatch,
      bindAs2:execute=>{as2Execute=execute;},
      bindAs2Layout:callback=>{as2Layout=callback;},
      bindAs2Wheel:callback=>{as2Wheel=callback;},
      bindTargetForRef:fn=>{targetForRef=fn;},
      bindAs2TextEvent:fn=>{as2TextEvent=fn;},
      wheel:(ref,event)=>as2Wheel?.(ref,event)||false,
      notifyLayout:ref=>onUpdate(ref),
      invalidateCache,forgetField,pruneDeadFields,TextFormatAlign,TextFormatDisplay,TextFieldType,TextFieldAutoSize,AntiAliasType,GridFitType,FontStyle,FontType,TextColorType,TextDisplayMode,TextInteractionMode,CSMSettings,TextRenderer,Font,StyleSheet,TextSnapshot,snapshotFor,linkAt,isFontCompatible,TextLineMetrics,info,update,userInteractionModeChanged,layout,get,set,methods,bounds,draw,hit,reset,dispose,UNHANDLED
    });
  }
  const api={create,TextFormat,TextFormatAlign,TextFormatDisplay,TextFieldType,TextFieldAutoSize,AntiAliasType,GridFitType,FontStyle,FontType,TextColorType,TextDisplayMode,TextInteractionMode,CSMSettings,TextLineMetrics,formatPatch,formatCopy,cssFontFamily,cssFont,applyFont,graphemeBoundaries,snapGraphemeFloor,snapGraphemeCeil,prevGrapheme,nextGrapheme,graphemeSafeDiff,sliceFieldRuns};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashText=api;
})(globalThis);
