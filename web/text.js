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
  function cssFont(format,embedOnly=false){return `${format.italic?'italic ':''}${format.bold?'bold ':''}${format.size}px ${JSON.stringify(format.font)}${embedOnly?'':', sans-serif'}`;}
  function applyFont(context,format,measurement=false,embedOnly=false){
    context.font=cssFont(format,embedOnly);
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
  function create({player,wasm=null,events=null,project=null,document=global.document,getProgram=null,geometry,measureText,portableMetrics=false,onUpdate=()=>{},onScroll=null}={}){
    if(!player)throw new TypeError('TextField requires the shared Player.');
    const defaultPortableMeasure=(textValue,style)=>{
      const runtimeWasm=wasm||(typeof NeoFlashWasm==='undefined'?global.NeoFlashWasm:NeoFlashWasm);
      const fn=player?.portable_measure_text_json?.bind(player)||runtimeWasm?.portable_measure_text_json;
      if(typeof fn==='function'){
        try{return JSON.parse(fn(JSON.stringify([String(textValue)]),JSON.stringify(style)))[0]??0;}catch(_){}
      }
      const size=Number(style?.size)||16;
      return String(textValue).length*size*0.56;
    };
    if(typeof measureText!=='function')measureText=defaultPortableMeasure;
    else if(portableMetrics){
      measureText=defaultPortableMeasure;
    }
    let as2Execute=null,as2Layout=null,as2Wheel=null;
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
    function update(ref,patch){active();if(!player.object_text_update(ref,JSON.stringify(patch)))throw new Error('TextField update requires a live text object.');if(!fit(ref))as2Layout?.(ref);onUpdate(ref);}
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
      if(name==='textInteractionMode')return TextInteractionMode.NORMAL;
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
      if(name==='textColor'){if(value===null||value===undefined)throw new TypeError('TextField.textColor requires an RGB integer.');update(ref,{format:{color:formatValue('color',value)}});clearHtmlSource(ref);return true;}
      if(name==='multiline'||name==='wordWrap'){if(typeof value!=='boolean')throw new TypeError(`TextField.${name} requires a Boolean.`);update(ref,{[name==='wordWrap'?'word_wrap':name]:value});return true;}
      if(name==='defaultTextFormat'){if(fieldStates.get(ref)?.style)throw illegalTextOp();update(ref,{default_format:formatPatch(value)});return true;}
      if(name==='width'||name==='height'||name==='_width'||name==='_height'){
        if(name[0]==='_')value=Number(value);
        if(typeof value!=='number'||!Number.isFinite(value)||value<0)throw new RangeError('TextField box dimensions must be finite and nonnegative.');
        const axis=name.includes('width')?'width':'height',field=info(ref),patch={[axis]:value};
        if(!field.box){const result=layout(field);patch[axis==='width'?'height':'width']=axis==='width'?result.text_height:result.text_width;}
        update(ref,patch);return true;
      }
      if(['length','numLines','textWidth','textHeight','maxScrollV','maxScrollH','bottomScrollV','textInteractionMode'].includes(name))throw new TypeError(`TextField.${name} is read-only.`);
      return false;
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
      return mime.startsWith('font/')||mime.includes('truetype')||mime.includes('opentype')||mime.includes('woff')||/\.(?:ttf|otf|woff2?)$/.test(name);
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
        const isFont=mime.startsWith('font/')||mime.includes('truetype')||mime.includes('opentype')||mime.includes('woff')||/\.(?:ttf|otf|woff2?)$/.test(name);
        return isFont&&name.includes(wanted);
      }))return true;
      return false;
    }
    function draw(context,field){
      const result=layout(field);context.save();
      try{
        const surface=result.surface;
        if(surface?.background!==null&&surface?.background!==undefined){context.fillStyle=`rgba(${surface.background>>>16},${surface.background>>>8&255},${surface.background&255},${field.alpha/255})`;context.fillRect(0,0,surface.width,surface.height);}
        if(surface?.border!==null&&surface?.border!==undefined&&surface.width>0&&surface.height>0){context.strokeStyle=`rgba(${surface.border>>>16},${surface.border>>>8&255},${surface.border&255},${field.alpha/255})`;context.lineWidth=1;context.strokeRect(0.5,0.5,Math.max(0,surface.width-1),Math.max(0,surface.height-1));}
        if(field.box){context.beginPath();context.rect(0,0,field.box.width,field.box.height);context.clip();}
        const antiAlias=field.anti_alias_type||'normal';
        const gridFit=field.grid_fit_type||'pixel';
        const thickness=Number(field.thickness)||0;
        const sharpness=Number(field.sharpness)||0;
        const embedOnly=Boolean(field.embed_fonts);
        const sharpMod=antiAlias==='advanced'?Math.max(0.35,Math.min(1,1+sharpness/1600)):1;
        const thickAlphaMod=(antiAlias==='advanced'&&thickness<0)?Math.max(0.4,1+thickness/400):1;
        const baseAlpha=Math.max(0,Math.min(1,(field.alpha/255)*sharpMod*thickAlphaMod));
        applyFont(context,field.format,false,embedOnly);context.textAlign='left';context.textBaseline='top';
        const rgb=field.format.color;context.fillStyle=`rgba(${rgb>>>16},${rgb>>>8&255},${rgb&255},${baseAlpha})`;
        for(const item of result.runs?.length?result.runs:result.lines){
          const fmt=item.format||field.format;
          if(embedOnly&&registeredFonts.length>0&&!isEmbeddedFontAvailable(fmt))continue;
          if(item.format){
            applyFont(context,item.format,false,embedOnly);
            const itemRgb=item.format.color;
            context.fillStyle=`rgba(${itemRgb>>>16},${itemRgb>>>8&255},${itemRgb&255},${baseAlpha})`;
          }
          const gx=gridFit==='pixel'?Math.round(item.x):gridFit==='subpixel'?Math.round(item.x*3)/3:item.x;
          const gy=gridFit==='pixel'?Math.round(item.y):item.y;
          context.fillText(item.text,gx,gy);
          if(antiAlias==='advanced'&&thickness>0&&typeof context.strokeText==='function'){
            const cRgb=fmt.color;
            context.strokeStyle=`rgba(${cRgb>>>16},${cRgb>>>8&255},${cRgb&255},${Math.min(1,baseAlpha*(thickness/200)*0.6)})`;
            context.lineWidth=(thickness/200)*0.8;
            context.strokeText(item.text,gx,gy);
          }
        }
        for(const [x,y,radius]of result.bullets||[]){context.beginPath();context.arc(x,y,radius,0,Math.PI*2);context.fill();}
        for(const item of result.decorations||[])context.fillRect(item.x,item.y,item.width,item.height);
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
    let rendererState=defaultRenderer(),rendererAntiAliasType=AntiAliasType.NORMAL;
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
        rendererState=textKernel({op:'renderer_table',state:rendererState,font_name:fontName,font_style:kernelStyle,color_type:colorType,table:entries}).state;
      }
    }
    TextRenderer.setAdvancedAntialiasingTable=TextRenderer.setAdvancedAntiAliasingTable;
    Object.defineProperty(TextRenderer,'antiAliasType',{enumerable:true,
      get(){active();return rendererAntiAliasType;},
      set(value){active();if(!['normal','advanced'].includes(value))throw new TypeError('TextRenderer.antiAliasType requires normal or advanced.');rendererAntiAliasType=value;}});
    for(const name of ['displayMode','maxLevel']){
      Object.defineProperty(TextRenderer,name,{enumerable:true,
        get(){active();return rendererState[name];},
        set(value){active();rendererState=textKernel({op:'renderer_set',state:rendererState,name,value}).state;}});
    }
    Object.defineProperties(TextRenderer,{__nfIs:{value:()=>false},__nfInitialize:{value:()=>{throw new TypeError('TextRenderer cannot be subclassed.');}},[Symbol.hasInstance]:{value:()=>false}});
    Object.freeze(TextRenderer);

    const fontRecords=new WeakMap(),fontToken=Symbol('Font'),registeredFonts=[],registeredFontClasses=new Set();
    class Font {
      constructor(token,record){
        active();
        if(token===fontToken){fontRecords.set(this,record);Object.freeze(this);return;}
        if(arguments.length>0)throw new TypeError('Font constructor accepts no arguments.');
        fontRecords.set(this,{fontName:null,fontStyle:FontStyle.REGULAR,fontType:FontType.EMBEDDED});
      }
      get fontName(){const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');return record.fontName;}
      get fontStyle(){const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');return record.fontStyle;}
      get fontType(){const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');return record.fontType;}
      hasGlyphs(str){
        const record=fontRecords.get(this);if(!record)throw new TypeError('Expected Font.');
        if(str===null||str===undefined)throw new TypeError('Font.hasGlyphs requires a non-null String.');
        const value=String(str);
        for(const ch of value){
          const cp=ch.codePointAt(0);
          if(cp<0x20||(cp>=0x7f&&cp<=0x9f)||(cp>=0xd800&&cp<=0xdfff)||cp===0xfffd||(cp>=0xfdd0&&cp<=0xfdef)||(cp&0xfffe)===0xfffe)return false;
        }
        return true;
      }
      static registerFont(fontClass){
        active();
        if(typeof fontClass!=='function'||fontClass===Font)throw new TypeError('ArgumentError: Font.registerFont requires a Font subclass.');
        const prog=getProgram?.();
        const instance=prog?.isClass?.(fontClass)?prog.constructClass(fontClass,[]):new fontClass();
        if(!Font.__nfIs(instance))throw new TypeError('ArgumentError: Font.registerFont requires a Font subclass.');
        if(!registeredFontClasses.has(fontClass)){registeredFontClasses.add(fontClass);registeredFonts.push(instance);}
      }
      static enumerateFonts(enumerateDeviceFonts=false){
        active();
        const list=registeredFonts.slice();
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
          for(const family of families)list.push(new Font(fontToken,{fontName:family,fontStyle:FontStyle.REGULAR,fontType:FontType.DEVICE}));
        }
        return list;
      }
    }
    Font.__nfIs=value=>fontRecords.has(value);
    Font.__nfAllocate=({className}={})=>{
      const instance=Object.create(Font.prototype);
      const shortName=typeof className==='string'&&className.length?className.split('.').pop():'EmbeddedFont';
      fontRecords.set(instance,{fontName:shortName,fontStyle:FontStyle.REGULAR,fontType:FontType.EMBEDDED});
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
      const field=info(ref);
      if(!field.format.url&&!(field.format_runs||[]).some(r=>r.format?.url))return null;
      return textKernel({op:'hit_link',field,layout:layout(field),x:point.x,y:point.y});
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
    function reset(){active();cache.clear();cacheBytes=0;fieldStates.clear();rendererState=defaultRenderer();rendererAntiAliasType=AntiAliasType.NORMAL;registeredFonts.length=0;registeredFontClasses.clear();}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({measure:(strings,format)=>strings.map(value=>measureText(value,format)),portableMeasure:(strings,format)=>strings.map(value=>defaultPortableMeasure(value,format)),toOutlines,TextFormat,As2TextFormat,legacyFormatCopy,formatPatch,bindAs2:execute=>{as2Execute=execute;},bindAs2Layout:callback=>{as2Layout=callback;},bindAs2Wheel:callback=>{as2Wheel=callback;},wheel:(ref,event)=>as2Wheel?.(ref,event)||false,notifyLayout:ref=>onUpdate(ref),TextFormatAlign,TextFormatDisplay,TextFieldType,TextFieldAutoSize,AntiAliasType,GridFitType,FontStyle,FontType,TextColorType,TextDisplayMode,TextInteractionMode,CSMSettings,TextRenderer,Font,StyleSheet,TextSnapshot,snapshotFor,linkAt,isFontCompatible,TextLineMetrics,info,update,layout,get,set,methods,bounds,draw,hit,reset,dispose,UNHANDLED});
  }
  const api={create,TextFormat,TextFormatAlign,TextFormatDisplay,TextFieldType,TextFieldAutoSize,AntiAliasType,GridFitType,FontStyle,FontType,TextColorType,TextDisplayMode,TextInteractionMode,CSMSettings,TextLineMetrics,formatPatch,formatCopy,cssFont,applyFont};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashText=api;
})(globalThis);
