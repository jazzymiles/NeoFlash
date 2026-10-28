/* Native browser editing for shared Rust TextFields. Original implementation.
 * Native INPUT/TEXTAREA handles beforeinput, paste, selection and composition;
 * only committed user transactions enter Rust. No keydown-built text editor.
 * The active field replaces its Canvas text and follows affine/scroll/mask
 * geometry through SVG clip paths. While editing it is above the Canvas plane;
 * foreground Canvas occlusion, rich text/IME candidate geometry parity and
 * password+multiline editing remain explicit boundaries of this profile.
 */
(function(global){
  'use strict';
  let serial=0;
  const refKey=ref=>JSON.stringify(ref),svgNS='http://www.w3.org/2000/svg';
  function create(options={}){
    const {player,text,display,events,hub,stage,referenceOf,proxyForRef}=options;
    if(!player?.object_text_input||!text||!display||!events?.TextEvent||!hub||!stage)throw new TypeError('Text input requires the shared text, display and event services.');
    const doc=options.document||options.canvas?.ownerDocument||global.document,surface=options.surface||options.canvas?.parentElement;
    const listeners=new Set(),cleanups=[];let active=null,disposed=false,updating=false,domSuppressed=false,lastSnapshot=null,transition=0;
    const prefix=`neoflash-input-${++serial}-`;
    function open(){if(disposed)throw new Error('Text input service has been disposed.');}
    function report(error){if(options.onError)options.onError(error);else throw error;}
    function snapshot(){return JSON.parse(player.runtime_snapshot_json());}
    function target(value){if(referenceOf(value))return value;const ref=typeof value==='string'?JSON.parse(value):value;return proxyForRef(ref);}
    function entry(value){const receiver=target(value),ref=referenceOf(receiver);if(!ref||!display.alive(receiver)||display.descriptor(receiver).owner!=='text')throw new TypeError('Text editing requires a live TextField.');return {receiver,key:ref.key,field:text.info(ref.key)};}
    function alive(record){return record&&!disposed&&display.alive(record.target);}
    function editable(field){return field.field_type==='input';}
    function selectable(field){return editable(field)||field.selectable!==false;}
    function legacy(receiver,name,args){if(!receiver||!display.alive(receiver))return;const fn=receiver[name];if(typeof fn==='function')Reflect.apply(fn,receiver,args);options.onLegacyTextEvent?.(receiver,name,args);}
    function commit(value,proposal){
      open();const {receiver,key,field}=entry(value),expected=proposal.expected_revision??field.revision;
      if(typeof proposal.text!=='string')throw new TypeError('User text insertion requires a String.');
      let insert=proposal.text;
      if(insert.length){
        const event=new events.TextEvent(events.TextEvent.TEXT_INPUT,true,true,insert);
        if(!hub.dispatch(receiver,event)){sync(key);return text.info(key);}
        if(!display.alive(receiver))return null;
        insert=event.text;if(typeof insert!=='string')throw new TypeError('TextEvent.text must remain a String.');
      }
      const result=JSON.parse(player.object_text_input(key,JSON.stringify({expected_revision:expected,begin:proposal.begin,end:proposal.end,text:insert})));
      sync(key);
      if(result.text!==field.text){
        hub.dispatch(receiver,new events.Event(events.Event.CHANGE,true,false));
        if(display.alive(receiver))legacy(receiver,'onChanged',[receiver]);
      }
      options.afterInput?.();return result;
    }
    function selection(record){
      if(!alive(record)||record.composing||!record.control)return;
      const field=text.info(record.key),begin=record.control.selectionStart??0,end=record.control.selectionEnd??begin;
      const backward=record.control.selectionDirection==='backward';
      if(begin!==field.selection_begin||end!==field.selection_end||(backward?begin:end)!==field.caret)text.update(record.key,{selection:{begin:backward?end:begin,end:backward?begin:end}});
    }
    function difference(before,after){
      let begin=0;while(begin<before.length&&begin<after.length&&before[begin]===after[begin])begin++;
      // Browser replacement boundaries can sit between surrogate code units;
      // expand the common prefix/suffix to complete Unicode characters.
      if(begin>0&&begin<before.length&&/[\uDC00-\uDFFF]/.test(before[begin]))begin--;
      let oldEnd=before.length,newEnd=after.length;
      while(oldEnd>begin&&newEnd>begin&&before[oldEnd-1]===after[newEnd-1]){oldEnd--;newEnd--;}
      if(oldEnd<before.length&&/[\uDC00-\uDFFF]/.test(before[oldEnd])){oldEnd++;newEnd++;}
      return {begin,end:oldEnd,text:after.slice(begin,newEnd)};
    }
    const displayed=field=>field.multiline?field.text:field.text.replace(/\n/g,' ');
    function acceptDom(record){
      if(!alive(record)||!record.control||record.composing)return;
      if(!editable(record.field)){selection(record);return;}
      const proposed=record.control.value.replace(/\r\n?|\n/g,'\n'),base=record.base||record.field;
      record.base=null;
      if(proposed===displayed(base)){selection(record);return;}
      const edit=difference(displayed(base),proposed);
      try{commit(record.target,{...edit,expected_revision:base.revision});}
      catch(error){sync(record.key,true);throw error;}
    }
    function listen(node,name,fn,settings){
      const guarded=event=>{try{fn(event);}catch(error){report(error);}};
      node.addEventListener(name,guarded,settings);return ()=>node.removeEventListener(name,guarded,settings);
    }
    function removeDom(record){
      if(!record)return;for(const cleanup of record.cleanups||[])cleanup();
      domSuppressed=true;try{record.root?.remove();}finally{domSuppressed=false;}
      record.root=record.control=null;
    }
    function path(commands){return commands.map(item=>{switch(item.op){case'move_to':return `M${item.x} ${item.y}`;case'line_to':return `L${item.x} ${item.y}`;case'quadratic_to':return `Q${item.cx} ${item.cy} ${item.x} ${item.y}`;case'cubic_to':return `C${item.cx1} ${item.cy1} ${item.cx2} ${item.cy2} ${item.x} ${item.y}`;case'close':return 'Z';default:throw new Error('Unsupported input clip command.');}}).join(' ');}
    function clips(record,node,scene){
      const entries=new Map((scene.clips||[]).map(item=>[item.id,item]));
      const chain=(node.clip_ids||[]).map(id=>{const item=entries.get(id);if(!item)throw new Error('Text input clipping table is incomplete.');return item;});
      const key=JSON.stringify(chain);if(key===record.clipKey)return;record.clipKey=key;
      if(key.length*2>8*1024*1024)throw new RangeError('Active text input clip geometry exceeds 8 MiB; simplify the mask before editing.');
      if(chain.length>32)throw new RangeError('Active text input supports at most 32 inherited clips.');
      record.defs.replaceChildren();
      for(const wrapper of record.wrappers)wrapper.style.clipPath='none';
      for(let i=0;i<chain.length;i++){
        const clip=doc.createElementNS(svgNS,'clipPath'),id=prefix+record.serial+'-'+i;clip.setAttribute('id',id);clip.setAttribute('clipPathUnits','userSpaceOnUse');
        for(const component of chain[i].components){const item=doc.createElementNS(svgNS,'path');item.setAttribute('d',path(component.commands));item.setAttribute('transform',`matrix(${component.matrix.join(' ')})`);item.setAttribute('clip-rule',component.fill_rule);item.setAttribute('fill-rule',component.fill_rule);clip.appendChild(item);}
        record.defs.appendChild(clip);record.wrappers[i].style.clipPath=`url(#${id})`;
      }
    }
    function makeDom(record){
      if(!doc?.createElement||!surface?.appendChild)return;
      const field=record.field;
      if(field.multiline&&field.display_as_password)throw new TypeError('Native password editing currently requires a single-line TextField.');
      const root=doc.createElement('div'),clipped=doc.createElement('div'),svg=doc.createElementNS(svgNS,'svg'),defs=doc.createElementNS(svgNS,'defs');
      Object.assign(root.style,{position:'absolute',left:'0',top:'0',transformOrigin:'0 0',pointerEvents:'none',overflow:'hidden',zIndex:'1'});
      Object.assign(clipped.style,{position:'absolute',inset:'0',pointerEvents:'none'});svg.setAttribute('width','0');svg.setAttribute('height','0');svg.style.position='absolute';svg.appendChild(defs);root.appendChild(svg);root.appendChild(clipped);
      const control=doc.createElement(field.multiline?'textarea':'input');if(!field.multiline)control.type=field.display_as_password?'password':'text';
      control.autocomplete='off';control.autocapitalize='off';control.spellcheck=false;control.setAttribute('aria-label',display.descriptor(record.target).name||'Text field');control.setAttribute('data-neoflash-text-input','true');
      Object.assign(control.style,{position:'absolute',left:'0',top:'0',margin:'0',padding:'2px',boxSizing:'border-box',border:'0',borderRadius:'0',background:'transparent',outline:'1px solid #2780e3',resize:'none',transformOrigin:'0 0',pointerEvents:'auto',overflow:'auto'});
      const wrappers=[];let parent=clipped;for(let i=0;i<32;i++){const wrapper=doc.createElement('div');Object.assign(wrapper.style,{position:'absolute',inset:'0',pointerEvents:'none'});parent.appendChild(wrapper);wrappers.push(wrapper);parent=wrapper;}parent.appendChild(control);Object.assign(record,{root,control,defs,clipped,wrappers,clipKey:null,serial:++serial,cleanups:[]});
      const on=(name,fn)=>record.cleanups.push(listen(control,name,fn));
      on('beforeinput',()=>{if(!record.composing){selection(record);record.base=text.info(record.key);}});
      on('input',event=>{if(!record.composing&&!event.isComposing)acceptDom(record);});
      on('compositionstart',()=>{selection(record);record.base=text.info(record.key);record.composing=true;});
      on('compositionend',()=>{record.composing=false;acceptDom(record);});
      on('select',()=>selection(record));
      on('blur',()=>{if(domSuppressed||active!==record)return;if(record.composing)record.composing=false;acceptDom(record);if(stage.stage.focus===record.target)stage.changeFocus(null);options.afterInput?.();});
      on('keydown',event=>{if(event.key==='Tab'&&!event.isComposing){if(options.onLegacyTab?.(event))return;event.preventDefault();const scene=snapshot(),items=scene.objects.filter(node=>node.text?.field_type==='input'&&node.on_stage&&node.effective_visible&&node.world[0]*node.world[3]-node.world[1]*node.world[2]!==0);if(items.length){const index=items.findIndex(node=>refKey(node.object_ref)===record.key),next=(index+(event.shiftKey?-1:1)+items.length)%items.length;stage.changeFocus(proxyForRef(items[next].object_ref),'key',event);}}});
      for(const type of ['pointerdown','pointerup','pointermove'])on(type,event=>stage.handlePointer(type,event,record.target));
      on('click',event=>options.onClick?.(record.target,event));
      surface.appendChild(root);style(record,lastSnapshot||snapshot());sync(record.key,true);
      domSuppressed=true;try{control.focus({preventScroll:true});}finally{domSuppressed=false;}
    }
    function applySelection(record,field){
      const control=record.control;if(!control||record.composing)return;
      const begin=field.selection_begin||0,end=field.selection_end||0,direction=field.caret===begin&&end!==begin?'backward':'forward';
      if(control.selectionStart!==begin||control.selectionEnd!==end||control.selectionDirection!==direction)control.setSelectionRange(begin,end,direction);
    }
    function style(record,scene){
      if(!record.control)return;
      const node=scene.objects.find(node=>refKey(node.object_ref)===record.key),draw=scene.draws.find(draw=>refKey(draw.owner_ref)===record.key&&draw.kind.type==='text_field');
      if(!node)return;
      const field=draw?.kind.field||record.field,box=field.box||text.bounds(field),width=options.project?.width||options.canvas?.width||800,height=options.project?.height||options.canvas?.height||600;
      if(!box)throw new TypeError('Native text editing requires a nonempty text box. Set width and height.');
      const w=box.width??box.right-box.left,h=box.height??box.bottom-box.top;
      const rect=options.canvas?.getBoundingClientRect?.(),sx=rect?rect.width/width:1,sy=rect?rect.height/height:1;
      Object.assign(record.root.style,{width:width+'px',height:height+'px',transform:`scale(${sx},${sy})`});
      const c=field.format.color;
      Object.assign(record.control.style,{width:w+'px',height:(field.multiline?h:Math.min(h,field.format.size*1.2+field.format.leading+4))+'px',font:global.NeoFlashText?.cssFont?.(field.format)||`${field.format.italic?'italic ':''}${field.format.bold?'bold ':''}${field.format.size}px ${JSON.stringify(field.format.font)},sans-serif`,lineHeight:(field.format.size*1.2+field.format.leading)+'px',textAlign:field.format.align,color:`rgba(${c>>>16},${c>>>8&255},${c&255},${field.alpha/255*(draw?.alpha??1)})`,caretColor:`rgb(${c>>>16},${c>>>8&255},${c&255})`,transform:`matrix(${node.world.join(',')})`,whiteSpace:field.word_wrap?'pre-wrap':'pre'});
      Object.assign(record.control.style,{paddingLeft:(2+(field.format.leftMargin||0)+(field.format.blockIndent||0))+'px',paddingRight:(2+(field.format.rightMargin||0))+'px',textIndent:(field.format.indent||0)+'px',textDecoration:field.format.underline?'underline':'none'});
      if(field.as2_viewport?.active){
        const view=field.as2_viewport,rgb=value=>`rgba(${value>>>16},${value>>>8&255},${value&255},${field.alpha/255})`;
        record.control.style.background=view.background?rgb(view.backgroundColor):'transparent';record.control.style.border=view.border?'1px solid '+rgb(view.borderColor):'0';
        const viewport=text.layout(field).viewport;record.control.scrollLeft=viewport.offset_x;record.control.scrollTop=viewport.offset_y;
      }
      record.control.readOnly=!editable(field);if(field.multiline)record.control.wrap=field.word_wrap?'soft':'off';else {const type=field.display_as_password?'password':'text';if(record.control.type!==type)record.control.type=type;}
      // Rust filters restrict before enforcing maxChars; native maxlength would
      // truncate a pasted raw string before disallowed characters are removed.
      record.control.removeAttribute('maxlength');
      clips(record,node,scene);applySelection(record,record.field);
    }
    function sync(key,force=false){
      if(!active||active.key!==key||!alive(active))return;
      const record=active,field=text.info(key),changed=field.text!==record.field.text;
      if(record.control&&field.multiline&&field.display_as_password){removeDom(record);active=null;stage.changeFocus(null);throw new TypeError('Native password editing currently requires a single-line TextField. The multiline field remains masked on Canvas.');}
      if(record.composing&&!force){if(!changed){record.field=field;if(record.base)record.base={...record.base,revision:field.revision};return;}record.composing=false;record.base=null;}
      record.field=field;if(changed)record.base=null;
      if(record.control){if(record.control.value!==displayed(field))record.control.value=displayed(field);applySelection(record,field);}
    }
    function focusChanged(next,previous){
      open();const version=++transition,old=active;active=null;
      if(old){if(old.composing)old.composing=false;try{acceptDom(old);}finally{removeDom(old);}}
      if(previous&&display.alive(previous))legacy(previous,'onKillFocus',[next]);if(version!==transition)return true;
      if(next&&display.alive(next)){
        const node=display.descriptor(next);
        if(node.owner==='text'){
          const key=referenceOf(next).key,field=text.info(key);
          if(!selectable(field))return false;
          active={target:next,key,field,base:null,composing:false,control:null,cleanups:[]};makeDom(active);
        }
        legacy(next,'onSetFocus',[previous]);
      }
      if(version!==transition)return true;
      for(const listener of [...listeners]){if(version!==transition)break;if(referenceOf(listener)&&!display.alive(listener)){listeners.delete(listener);continue;}const fn=listener.onSetFocus;if(typeof fn==='function')Reflect.apply(fn,listener,[previous,next]);}
      return Boolean(active?.control);
    }
    function update(scene){
      open();lastSnapshot=scene;if(!active||updating)return;updating=true;
      try{
        const record=active,node=scene.objects.find(node=>refKey(node.object_ref)===record.key);
        if(!alive(record)||!node||!node.on_stage||!node.effective_visible||node.world[0]*node.world[3]-node.world[1]*node.world[2]===0){stage.changeFocus(null);return;}
        const field=text.info(record.key);if(!selectable(field)){stage.changeFocus(null);return;}
        if(record.control&&Boolean(field.multiline)!==(record.control.tagName==='TEXTAREA')){if(record.composing)record.composing=false;acceptDom(record);removeDom(record);record.field=text.info(record.key);makeDom(record);}
        sync(record.key);style(record,scene);
      }finally{updating=false;}
    }
    function canFocus(value){if(value===null||value===stage.stage)return true;const node=display.descriptor(value);return node.owner!=='text'||(node.on_stage&&node.effective_visible&&node.world[0]*node.world[3]-node.world[1]*node.world[2]!==0&&selectable(text.info(referenceOf(value).key)));}
    function focused(){const value=stage.stage.focus;return value&&display.alive(value)&&display.descriptor(value).owner==='text'?value:null;}
    function focusPath(){let value=stage.stage.focus;if(!value)return null;const names=[];while(value&&value!==stage.stage){const node=display.descriptor(value);if(node.name)names.unshift(node.name);value=display.parentSpace(value);}return '_level0'+(names.length?'.'+names.join('.'):'');}
    const Selection=Object.freeze({
      setFocus:value=>{open();if(typeof value==='string')throw new TypeError('Selection.setFocus currently requires a live display object or null; path strings are pending.');if(value!==null&&!display.alive(value))return false;stage.changeFocus(value);return stage.stage.focus===value;},
      getFocus:()=>{open();return focusPath();},
      setSelection:(begin,end)=>{open();const value=focused();if(value)text.methods(referenceOf(value).key).setSelection(begin,end);},
      getBeginIndex:()=>{open();const value=focused();return value?text.info(referenceOf(value).key).selection_begin:-1;},
      getEndIndex:()=>{open();const value=focused();return value?text.info(referenceOf(value).key).selection_end:-1;},
      getCaretIndex:()=>{open();const value=focused();return value?text.info(referenceOf(value).key).caret:-1;},
      addListener:value=>{open();if(!value||!['object','function'].includes(typeof value)||listeners.has(value))return false;if(listeners.size>=1024)throw new RangeError('Selection supports at most 1024 listeners.');listeners.add(value);return true;},
      removeListener:value=>{open();return listeners.delete(value);},
    });
    if(doc?.addEventListener)cleanups.push(listen(doc,'selectionchange',()=>{if(active?.control===doc.activeElement)selection(active);}));
    function reset(){open();transition++;removeDom(active);active=null;lastSnapshot=null;listeners.clear();}
    function dispose(){if(disposed)return;reset();for(const cleanup of cleanups)cleanup();disposed=true;}
    return Object.freeze({commit,focusChanged,update,sync,canFocus,Selection,owns:node=>node===active?.control,hidden:ref=>Boolean(active?.control&&active.key===refKey(ref)),reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashTextInput=api;
})(globalThis);
