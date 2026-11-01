/* AS2-only facades. Rust owns CSS parsing/mapping and HTML interpretation;
 * this module transports ordinary style records, native TextFormat values,
 * graph references and URLLoader/browser callbacks. */
(function(global){
  'use strict';
  function create({wasm,player,text,broadcaster=global.NeoFlashBroadcaster,getNet=()=>null,targetForRef=null,afterCallback=()=>{},project=null,document=global.document}){
    if(!wasm||typeof wasm.as2_text_json!=='function')throw new Error('AS2 text Rust kernel is missing; re-export this publication.');
    if(typeof broadcaster?.create!=='function')throw new Error('AS2 text requires the shared broadcaster host.');
    const execute=request=>JSON.parse(wasm.as2_text_json(JSON.stringify(request)));
    text.bindAs2(execute);
    function reflow(ref){const field=text.info(ref);if(field.as2_viewport.auto_size!=='none')return player.as2_text_fit(ref,JSON.stringify(text.layout(field)));return false;}
    text.bindAs2Layout(reflow);
    text.bindAs2Wheel(wheel);
    const broadcasters=broadcaster.create(wasm);
    const defaultRender=()=>({embedFonts:false,antiAliasType:'normal',gridFitType:'pixel',sharpness:0,thickness:0});
    const defaultRenderer=()=>({displayMode:'default',maxLevel:4,tables:{}});
    const sheets=new WeakMap(),fields=new Map(),loads=new Set();let generation=0,disposed=false,rendererState=defaultRenderer();
    function active(){if(disposed)throw new Error('AS2 text service was disposed.');}
    function record(sheet){active();const found=sheets.get(sheet);if(!found)throw new TypeError('Expected an AS2 TextField.StyleSheet from this publication.');return found;}
    function styleRecord(value){
      if(value===null||value===undefined)return null;
      if(typeof value!=='object')throw new TypeError('StyleSheet styles require ordinary data objects.');
      const result=Object.create(null),entries=Object.entries(Object.getOwnPropertyDescriptors(value));
      if(entries.length>128)throw new RangeError('StyleSheet styles support at most 128 properties.');
      for(const [name,item]of entries){if(!item.enumerable)continue;if(!Object.hasOwn(item,'value')||(!['string','number','boolean'].includes(typeof item.value)&&item.value!==null))throw new TypeError('StyleSheet styles require primitive own data properties.');result[name]=item.value;}
      return result;
    }
    function stateFor(ref){active();text.info(ref);let state=fields.get(ref);if(!state){if(fields.size>=4096)throw new RangeError('AS2 text state exceeds 4096 fields.');state={html:false,condense:false,style:null,source:null,revision:null,render:defaultRender()};fields.set(ref,state);}return state;}
    function render(ref,state,source){
      const field=text.info(ref),styles=state.style?record(state.style).state:{styles:{}};
      const parsed=execute({op:'html',source,format:field.default_format,styles,condense_white:state.condense});
      text.update(ref,{text:parsed.text,format:parsed.format});state.source=source;state.revision=text.info(ref).revision;
    }
    function changed(sheet){
      for(const [ref,state]of fields)if(state.style===sheet&&state.source!==null){try{if(!player.object_alive(ref)){fields.delete(ref);continue;}render(ref,state,state.source);}catch(error){throw new Error('StyleSheet change could not reflow its TextField: '+String(error),{cause:error});}}
    }
    function commit(sheet,next){record(sheet).state=next;changed(sheet);}
    class StyleSheet {
      constructor(){active();sheets.set(this,{state:{styles:{}},operation:null});}
      clear(){commit(this,execute({op:'clear'}).state);}
      getStyle(name){if(typeof name!=='string')throw new TypeError('StyleSheet selector requires a string.');const value=execute({op:'get_style',state:record(this).state,selector:name}).result;return value===null?null:value;}
      getStyleNames(){return execute({op:'names',state:record(this).state}).result;}
      setStyle(name,value){if(typeof name!=='string')throw new TypeError('StyleSheet selector requires a string.');commit(this,execute({op:'set_style',state:record(this).state,selector:name,style:styleRecord(value)}).state);}
      parseCSS(source){if(typeof source!=='string')throw new TypeError('StyleSheet.parseCSS requires a string.');const packet=execute({op:'parse_css',state:record(this).state,source});if(packet.result)commit(this,packet.state);return packet.result;}
      transform(value){if(value===undefined||value===null)return null;const patch=execute({op:'transform',style:styleRecord(value)}).result;return text.legacyFormatCopy(patch);}
      load(url){
        if(typeof url!=='string')throw new TypeError('StyleSheet.load requires a URL string.');const net=getNet();if(!net)throw new Error('StyleSheet.load requires the publication URLLoader service.');
        const current=record(this);if(current.operation){current.operation.loader.close();loads.delete(current.operation);}
        const loader=new net.URLLoader(),operation={loader,generation,sheet:this};current.operation=operation;loads.add(operation);
        const done=success=>{if(disposed||generation!==operation.generation||current.operation!==operation)return;try{if(success)success=this.parseCSS(loader.data);const callback=this.onLoad;if(typeof callback==='function')Reflect.apply(callback,this,[success]);afterCallback();}finally{if(current.operation===operation)current.operation=null;loads.delete(operation);}};
        loader.addEventListener('complete',()=>done(true));loader.addEventListener('ioError',()=>done(false));
        try{loader.load(new net.URLRequest(url));}catch(error){current.operation=null;loads.delete(operation);throw error;}return true;
      }
    }
    StyleSheet.__nfIs=value=>sheets.has(value);StyleSheet.__nfInitialize=()=>{throw new TypeError('AS2 StyleSheet subclassing is not supported.');};Object.defineProperty(StyleSheet,Symbol.hasInstance,{value:StyleSheet.__nfIs});
    const renderProps=new Set(['embedFonts','antiAliasType','gridFitType','sharpness','thickness']);
    const names=new Set(['html','htmlText','styleSheet','condenseWhite','getTextFormat','getNewTextFormat','text','background','backgroundColor','border','borderColor','scroll','hscroll','maxscroll','maxhscroll','bottomScroll','onScroller','addListener','removeListener','autoSize','mouseWheelEnabled',...renderProps]);
    function as2Ref(ref,name){
      if(!names.has(name))return null;
      return {has:()=>true,delete:()=>false,get:()=>{
        const state=stateFor(ref),field=text.info(ref);
        if(renderProps.has(name))return state.render[name];
        if(['background','backgroundColor','border','borderColor'].includes(name))return field.as2_viewport[name];
        if(['scroll','hscroll','maxscroll','maxhscroll','bottomScroll'].includes(name)){const viewport=text.layout(field).viewport;return viewport[name==='bottomScroll'?'bottom_scroll':name];}
        if(name==='autoSize')return field.as2_viewport.auto_size;
        if(name==='mouseWheelEnabled')return field.as2_viewport.mouse_wheel_enabled;
        if(name==='addListener')return listener=>{if(!state.broadcaster){state.broadcaster={};broadcasters.AsBroadcaster.initialize(state.broadcaster);}return state.broadcaster.addListener(listener);};
        if(name==='removeListener')return listener=>state.broadcaster?state.broadcaster.removeListener(listener):false;
        if(name==='onScroller')return state.onScroller;
        if(name==='getTextFormat')return (...args)=>{text.methods(ref).getTextFormat(...args);return text.legacyFormatCopy(text.info(ref).format);};
        if(name==='getNewTextFormat')return ()=>text.legacyFormatCopy(text.info(ref).default_format);
        if(name==='text')return text.get(ref,'text');
        if(name==='html')return state.html;if(name==='condenseWhite')return state.condense;if(name==='styleSheet')return state.style;
        if(!state.html&&!state.style)return text.get(ref,'text');
        return state.source!==null&&state.revision===field.revision?state.source:execute({op:'serialize_html',text:field.text,format:field.format}).result;
      },set:value=>{
        const state=stateFor(ref);
        if(renderProps.has(name)){state.render=execute({op:'field_render_set',state:state.render,name,value}).state;return value;}
        if(name==='autoSize'||name==='mouseWheelEnabled'){const field=text.info(ref);if(name==='autoSize'){if(value===true)value='left';if(value===false)value='none';if(!['none','left','center','right'].includes(value))throw new TypeError('autoSize requires none/left/center/right or Boolean.');}else if(typeof value!=='boolean')throw new TypeError('mouseWheelEnabled requires Boolean.');text.update(ref,{as2_viewport:{...field.as2_viewport,active:true,[name==='autoSize'?'auto_size':'mouse_wheel_enabled']:value}});return value;}
        if(name==='onScroller'){if(value!==null&&value!==undefined&&typeof value!=='function')throw new TypeError('onScroller requires a function or null.');state.onScroller=value;return value;}
        if(['background','backgroundColor','border','borderColor','scroll','hscroll'].includes(name)){
          if(['background','border'].includes(name)){if(typeof value!=='boolean')throw new TypeError('TextField.'+name+' requires a Boolean.');}
          else if(!Number.isInteger(value)||value<0)throw new TypeError('TextField.'+name+' requires a nonnegative integer.');
          const field=text.info(ref),previous=text.layout(field).viewport;text.update(ref,{as2_viewport:{...field.as2_viewport,active:true,[name]:value}});
          const next=text.layout(text.info(ref)).viewport;
          if(next.scroll!==previous.scroll||next.hscroll!==previous.hscroll){const receiver=targetForRef?targetForRef(ref):null;if(typeof state.onScroller==='function')Reflect.apply(state.onScroller,receiver,[receiver]);if(state.broadcaster)state.broadcaster.broadcastMessage('onScroller',receiver);}
          return value;
        }
        if(['maxscroll','maxhscroll','bottomScroll'].includes(name))throw new TypeError('TextField.'+name+' is read-only.');
        if(name==='html'||name==='condenseWhite'){if(typeof value!=='boolean')throw new TypeError('TextField.'+name+' requires a Boolean.');const key=name==='html'?'html':'condense';const old=state[key];state[key]=value;try{if(name==='condenseWhite'&&state.source!==null)render(ref,state,state.source);}catch(error){state[key]=old;throw error;}return value;}
        if(name==='styleSheet'){if(value!==null)record(value);const previous=state.style;state.style=value;try{if(state.source!==null)render(ref,state,state.source);}catch(error){state.style=previous;throw error;}return value;}
        if(name==='htmlText'||name==='text'){if(typeof value!=='string')throw new TypeError('TextField text requires a string.');if(state.style||(name==='htmlText'&&state.html))render(ref,state,value);else{text.update(ref,{text:value});state.source=null;state.revision=null;}return value;}
        throw new TypeError('AS2 TextField method '+name+' is read-only.');
      }};
    }
    const snapshots=new WeakMap();
    function snapshotRecord(receiver){active();const state=snapshots.get(receiver);if(!state)throw new TypeError('Expected TextSnapshot from this publication.');return state;}
    function snapshotCall(receiver,request){return JSON.parse(player.as2_snapshot_command(snapshotRecord(receiver),JSON.stringify(request)));}
    function index(value){if(!Number.isSafeInteger(value))throw new TypeError('TextSnapshot indices require safe integers.');return value;}
    const decoded=value=>String.fromCharCode(...value.units);
    class TextSnapshot {
      constructor(){throw new TypeError('TextSnapshot is obtained from MovieClip.getTextSnapshot().');}
      getCount(){return snapshotCall(this,{op:'count'});}
      getText(start,end,includeLineEndings=false){return decoded(snapshotCall(this,{op:'get_text',start:index(start),end:end===undefined?null:index(end),include_line_endings:Boolean(includeLineEndings)}));}
      findText(start,value,caseSensitive){if(typeof value!=='string')throw new TypeError('TextSnapshot.findText requires a String.');const query=[];for(let i=0;i<value.length;i++)query.push(value.charCodeAt(i));return snapshotCall(this,{op:'find',start:index(start),query,case_sensitive:Boolean(caseSensitive)});}
      setSelected(start,end,selected){snapshotCall(this,{op:'set_selected',start:index(start),end:index(end),selected:Boolean(selected)});}
      getSelected(start,end){return snapshotCall(this,{op:'get_selected',start:index(start),end:end===undefined?null:index(end)});}
      getSelectedText(includeLineEndings=false){return decoded(snapshotCall(this,{op:'selected_text',include_line_endings:Boolean(includeLineEndings)}));}
      setSelectColor(color){if(!Number.isInteger(color)||color<0||color>0xffffff)throw new RangeError('TextSnapshot color requires a 24-bit RGB integer.');snapshotCall(this,{op:'select_color',color});}
      getTextRunInfo(start,end){return snapshotCall(this,{op:'run_info',start:index(start),end:index(end)});}
      hitTestTextNearPos(x,y,distance=0){if(![x,y,distance].every(Number.isFinite))throw new TypeError('TextSnapshot hit test requires finite numbers.');return snapshotCall(this,{op:'hit_test',x,y,distance});}
    }
    TextSnapshot.__nfIs=value=>snapshots.has(value);TextSnapshot.__nfInitialize=()=>{throw new TypeError('TextSnapshot cannot be subclassed.');};Object.defineProperty(TextSnapshot,Symbol.hasInstance,{value:TextSnapshot.__nfIs});
    function snapshotFor(ref){active();const handle=player.as2_snapshot_create(ref,(strings,format)=>JSON.stringify(text.measure(JSON.parse(strings),JSON.parse(format))));const result=Object.create(TextSnapshot.prototype);snapshots.set(result,handle);return result;}
    function wheel(ref,event){
      if(disposed||!fields.has(ref)||event.defaultPrevented)return false;
      const field=text.info(ref),result=execute({op:'wheel',field,layout:text.layout(field),delta:event.deltaY??0,mode:event.deltaMode??0});
      if(result.handled){as2Ref(ref,'scroll').set(result.scroll);event.preventDefault?.();return true;}return false;
    }
    function link(ref,point,event){
      if(disposed||event.defaultPrevented)return false;const field=text.info(ref);if(!field.format.url)return false;
      const result=execute({op:'hit_link',field,layout:text.layout(field),x:point.x,y:point.y});if(!result)return false;
      const net=getNet();if(!net)throw new Error('Text hyperlinks require the publication URL navigation service.');
      net.navigateToURL(new net.URLRequest(result.url),result.target||null);return true;
    }
    function getFontList(){
      active();
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
      return execute({op:'font_list',candidates}).result;
    }
    class TextRenderer {
      constructor(){throw new TypeError('TextRenderer supplies static properties and methods and cannot be constructed.');}
      static setAdvancedAntialiasingTable(fontName,fontStyle,colorType,table){
        active();
        if(typeof fontName!=='string'||typeof fontStyle!=='string'||typeof colorType!=='string'||!Array.isArray(table)){
          throw new TypeError('TextRenderer.setAdvancedAntialiasingTable requires (fontName:String, fontStyle:String, colorType:String, table:Array).');
        }
        const entries=table.map(item=>{
          if(!item||typeof item!=='object')throw new TypeError('TextRenderer antialiasing entries require objects with fontSize, insideCutoff and outsideCutoff.');
          return {fontSize:Number(item.fontSize),insideCutoff:Number(item.insideCutoff),outsideCutoff:Number(item.outsideCutoff)};
        });
        rendererState=execute({op:'renderer_table',state:rendererState,font_name:fontName,font_style:fontStyle,color_type:colorType,table:entries}).state;
      }
    }
    for(const name of ['displayMode','maxLevel']){
      Object.defineProperty(TextRenderer,name,{enumerable:true,get(){active();return rendererState[name];},set(value){active();rendererState=execute({op:'renderer_set',state:rendererState,name,value}).state;}});
    }
    Object.defineProperties(TextRenderer,{__nfIs:{value:()=>false},__nfInitialize:{value:()=>{throw new TypeError('TextRenderer cannot be subclassed.');}}});
    Object.freeze(TextRenderer);
    function textEvent(ref,name,args){if(name==='onChanged'&&reflow(ref))text.notifyLayout(ref);if(!['onChanged','onScroller'].includes(name))return;const state=fields.get(ref);if(state?.broadcaster&&player.object_alive(ref))state.broadcaster.broadcastMessage(name,...args);}
    function reset(){generation++;for(const operation of loads)try{operation.loader.close();}catch(_){}loads.clear();fields.clear();rendererState=defaultRenderer();broadcasters.reset();}
    function dispose(){if(disposed)return;reset();broadcasters.dispose();disposed=true;}
    return Object.freeze({StyleSheet,TextSnapshot,TextRenderer,getFontList,snapshotFor,as2Ref,textEvent,wheel,link,execute,reset,dispose});
  }
  const api={create};if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Text=api;
})(globalThis);
