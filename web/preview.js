/* Viewing transform only: authored coordinates never change. */
(function(global){
  'use strict';
  function layout(width,height,viewportWidth,viewportHeight,mode,gutter=16){
    const availW=Math.max(1,viewportWidth-2*gutter),availH=Math.max(1,viewportHeight-2*gutter);
    const fit=Math.min(1,availW/width,availH/height);
    if(mode==='stretch'){
      const scale=availW/width,scaleY=availH/height,stageWidth=availW,stageHeight=availH;
      return {scale,scaleY,stageWidth,stageHeight,left:Math.max(gutter,(viewportWidth-stageWidth)/2),top:Math.max(gutter,(viewportHeight-stageHeight)/2),spaceWidth:Math.max(viewportWidth,stageWidth+2*gutter),spaceHeight:Math.max(viewportHeight,stageHeight+2*gutter)};
    }
    const scale=mode==='fit'?fit:mode==='fill'?Math.max(availW/width,availH/height):Number(mode);
    if(!Number.isFinite(scale)||scale<=0)throw new Error('Invalid preview scale');
    const stageWidth=mode==='fit'?Math.min(availW,width*scale):width*scale;
    const stageHeight=mode==='fit'?Math.min(availH,height*scale):height*scale;
    return {scale,stageWidth,stageHeight,left:Math.max(gutter,(viewportWidth-stageWidth)/2),top:Math.max(gutter,(viewportHeight-stageHeight)/2),spaceWidth:Math.max(viewportWidth,stageWidth+2*gutter),spaceHeight:Math.max(viewportHeight,stageHeight+2*gutter)};
  }
  function viewportBox(v){
    let cw=v?.clientWidth||0,ch=v?.clientHeight||0;
    if(v&&typeof v.getBoundingClientRect==='function'&&typeof v.offsetWidth==='number'&&typeof v.offsetHeight==='number'){
      const r=v.getBoundingClientRect();
      if(r&&r.width>0&&Math.abs(r.width-v.offsetWidth)<1)cw=Math.max(0,Math.floor(r.width-Math.max(0,v.offsetWidth-cw)+1e-6));
      if(r&&r.height>0&&Math.abs(r.height-v.offsetHeight)<1)ch=Math.max(0,Math.floor(r.height-Math.max(0,v.offsetHeight-ch)+1e-6));
    }
    return [cw,ch];
  }
  class Preview {
    constructor(viewport,space,surface,canvas,project,onChange){
      Object.assign(this,{viewport,space,surface,canvas,project,onChange});this.mode='fit';this.panMode=false;this.suppressClick=false;this.view=null;this.drag=null;
      viewport.addEventListener('pointerdown',event=>{
        if(!(this.panMode||event.button===1)||event.pointerType==='touch'||viewport.ownerDocument?.pointerLockElement)return;
        this.drag={id:event.pointerId,x:event.clientX,y:event.clientY,left:viewport.scrollLeft,top:viewport.scrollTop};
        try{viewport.setPointerCapture(event.pointerId);}catch{}event.preventDefault();
      });
      viewport.addEventListener('pointermove',event=>{if(this.drag&&event.pointerId===this.drag.id){viewport.scrollLeft=this.drag.left+this.drag.x-event.clientX;viewport.scrollTop=this.drag.top+this.drag.y-event.clientY;}});
      const end=event=>{if(this.drag&&event.pointerId===this.drag.id){this.drag=null;this.suppressClick=true;setTimeout(()=>{this.suppressClick=false;},0);if(viewport.hasPointerCapture(event.pointerId)){try{viewport.releasePointerCapture(event.pointerId);}catch{}}}};
      viewport.addEventListener('pointerup',end);viewport.addEventListener('pointercancel',end);viewport.addEventListener('lostpointercapture',end);
    }
    setMode(mode){this.mode=mode;this.resize();this.onChange();}
    setStageLayout(provider){this.stageLayout=provider;this.resize();}
    setPan(enabled){this.panMode=enabled;this.viewport.classList.toggle('pan-mode',enabled);}
    resize(){
      if(this.drag){const id=this.drag.id;this.drag=null;this.suppressClick=true;setTimeout(()=>{this.suppressClick=false;},0);if(this.viewport.hasPointerCapture(id)){try{this.viewport.releasePointerCapture(id);}catch{}}}
      const v=this.viewport,old=this.view;
      const scrollable=!this.stageLayout&&this.mode!=='fit'&&this.mode!=='stretch';
      const nextOverflow=scrollable?'auto':'hidden';
      if(v.style&&v.style.overflow!==nextOverflow)v.style.overflow=nextOverflow;
      const [vw,vh]=viewportBox(v);
      const center=old?[(v.scrollLeft+vw/2-old.left)/old.scale,(v.scrollTop+vh/2-old.top)/(old.scaleY||old.scale)]:[this.project.width/2,this.project.height/2];
      this.view=this.stageLayout?this.stageLayout(Math.max(1,vw),Math.max(1,vh)):layout(this.project.width,this.project.height,vw,vh,this.mode);
      const next=this.view;
      const spaceW=next.spaceWidth+'px',spaceH=next.spaceHeight+'px';
      if(this.space.style.width!==spaceW)this.space.style.width=spaceW;
      if(this.space.style.height!==spaceH)this.space.style.height=spaceH;
      const surfL=next.left+'px',surfT=next.top+'px',surfW=next.stageWidth+'px',surfH=next.stageHeight+'px';
      if(this.surface.style.left!==surfL)this.surface.style.left=surfL;
      if(this.surface.style.top!==surfT)this.surface.style.top=surfT;
      if(this.surface.style.width!==surfW)this.surface.style.width=surfW;
      if(this.surface.style.height!==surfH)this.surface.style.height=surfH;
      const logicalWidth=next.logicalWidth||this.project.width,logicalHeight=next.logicalHeight||this.project.height;
      const ratio=Math.min((global.devicePixelRatio||1)*Math.max(next.scale,next.scaleY||next.scale),4,8192/logicalWidth,8192/logicalHeight);
      const width=Math.max(1,Math.round(logicalWidth*ratio)),height=Math.max(1,Math.round(logicalHeight*ratio));
      if(this.canvas.width!==width||this.canvas.height!==height){this.canvas.width=width;this.canvas.height=height;}
      if(!scrollable){v.scrollLeft=0;v.scrollTop=0;}else if(old){v.scrollLeft=center[0]*next.scale+next.left-vw/2;v.scrollTop=center[1]*next.scale+next.top-vh/2;}
      return next;
    }
  }
  function isLoopbackHttpOrigin(loc){
    if(!loc)return false;
    const protocol=String(loc.protocol||'').toLowerCase();
    if(protocol!=='http:'&&protocol!=='https:')return false;
    const host=String(loc.hostname||'').toLowerCase();
    return host==='127.0.0.1'||host==='localhost'||host==='::1'||host==='[::1]';
  }
  function extractQueryParam(search,key){
    const raw=String(search||'').replace(/^\?/,'');
    if(!raw)return '';
    for(const part of raw.split('&')){
      if(!part)continue;
      const eq=part.indexOf('=');
      const k=eq<0?part:part.slice(0,eq);
      const v=eq<0?'':part.slice(eq+1);
      try{
        if(decodeURIComponent(k)===key)return decodeURIComponent(v.replace(/\+/g,' '));
      }catch(_){
        if(k===key)return v;
      }
    }
    return '';
  }
  function stripUrlQueryAndHash(url){
    return String(url||'').split('#')[0].split('?')[0];
  }
  function urlFileName(url){
    const clean=stripUrlQueryAndHash(url);
    const slash=clean.lastIndexOf('/');
    return slash>=0?clean.slice(slash+1):clean;
  }
  const INTERNAL_HOST_FILES=new Set([
    'runtime.js','preview.js','neoflash_web.js','neoflash_wasm_fallback.js','display.js','stage.js','events.js',
    'text.js','text-input.js','audio.js','video.js','bitmap.js','path.js','filters.js',
    'geom.js','geom3d.js','net.js','xml.js','loader.js','storage.js','external.js',
    'air.js','legacy-input.js','dom-layout.js','accessibility.js','byte-array.js',
    'fte3d.js','number-format.js','date.js','math.js','easing.js','broadcaster.js',
    'timers.js','amf.js',
  ]);
  function isInternalHostUrl(url){
    const file=urlFileName(url).toLowerCase();
    if(!file)return false;
    if(INTERNAL_HOST_FILES.has(file))return true;
    return /^as[23]-[a-z0-9_-]+\.js$/.test(file);
  }
  function parseStackFrameLine(rawLine){
    const trimmed=String(rawLine||'').trim();
    if(!trimmed)return null;
    const isV8=trimmed.startsWith('at ');
    if(!isV8&&!trimmed.includes('@')){
      const colonSpace=trimmed.indexOf(': ');
      if(colonSpace>0){
        const head=trimmed.slice(0,colonSpace);
        if(/(?:Error|Exception)$/.test(head))return null;
      }
    }
    let functionName=null,locPart='';
    if(isV8){
      const rest=trimmed.slice(3).trim();
      if(rest.endsWith(')')){
        let depth=0,openIdx=-1;
        for(let i=rest.length-1;i>=0;i--){
          if(rest[i]===')')depth++;
          else if(rest[i]==='('&&--depth===0){openIdx=i;break;}
        }
        if(openIdx>=0){
          const fnRaw=rest.slice(0,openIdx).trim().replace(/^async\s+/,'').trim();
          functionName=fnRaw||null;
          locPart=rest.slice(openIdx+1,-1).trim();
        }else{
          locPart=rest;
        }
      }else{
        locPart=rest;
      }
    }else{
      const atIdx=trimmed.indexOf('@');
      if(atIdx>=0){
        const rawFn=trimmed.slice(0,atIdx).trim();
        functionName=rawFn&&rawFn!=='global code'&&rawFn!=='module code'&&rawFn!=='eval code'?rawFn:null;
        locPart=trimmed.slice(atIdx+1).trim();
      }else{
        locPart=trimmed;
      }
    }
    if(!locPart||locPart==='[native code]'||(locPart.startsWith('[')&&locPart.endsWith(']')))return null;
    const match=locPart.match(/^(.*?):(\d+):(\d+)$/);
    if(!match)return null;
    const url=match[1].trim();
    const line=Number(match[2]);
    const column=Number(match[3]);
    if(!url||!Number.isInteger(line)||line<1||!Number.isInteger(column)||column<0)return null;
    const cleanUrl=stripUrlQueryAndHash(url);
    return {
      functionName,
      url,
      cleanUrl,
      fileName:urlFileName(cleanUrl),
      line,
      column:Math.max(1,column),
    };
  }
  function parseStackFrames(stack){
    if(typeof stack!=='string'||!stack)return [];
    const frames=[];
    for(const line of stack.split('\n')){
      const parsed=parseStackFrameLine(line);
      if(parsed)frames.push(parsed);
    }
    return frames;
  }
  const B64_ALPHABET='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const B64_LOOKUP=(()=>{
    const table=new Int8Array(128).fill(-1);
    for(let i=0;i<B64_ALPHABET.length;i++)table[B64_ALPHABET.charCodeAt(i)]=i;
    return table;
  })();
  const decodedMapCache=new WeakMap();
  function decodeVlqValue(str,pos){
    let result=0,shift=0,idx=pos;
    while(idx<str.length){
      const code=str.charCodeAt(idx++);
      const digit=code<128?B64_LOOKUP[code]:-1;
      if(digit<0)return null;
      result|=(digit&31)<<shift;
      shift+=5;
      if((digit&32)===0){
        const neg=(result&1)===1;
        const mag=result>>>1;
        return [neg?-mag:mag,idx];
      }
    }
    return null;
  }
  function utf16ColToCharCol(lineText,utf16Col){
    if(!lineText||utf16Col<=0)return 1;
    let units=0,chars=0;
    for(const ch of lineText){
      if(units>=utf16Col)break;
      units+=ch.length;
      chars++;
    }
    return chars+1;
  }
  function decodeSourceMapLocations(sourceMap){
    if(!sourceMap||typeof sourceMap!=='object'||typeof sourceMap.mappings!=='string')return [];
    if(decodedMapCache.has(sourceMap))return decodedMapCache.get(sourceMap);
    const sources=Array.isArray(sourceMap.sources)?sourceMap.sources:[];
    const contents=Array.isArray(sourceMap.sourcesContent)?sourceMap.sourcesContent:[];
    const names=Array.isArray(sourceMap.names)?sourceMap.names:[];
    const contentLines=contents.map(text=>typeof text==='string'?text.split('\n'):null);
    const locations=[];
    let prevSrc=0,prevOrigLine=0,prevOrigCol=0,prevName=0;
    const genLines=sourceMap.mappings.split(';');
    for(let genLine=0;genLine<genLines.length;genLine++){
      const lineStr=genLines[genLine];
      if(!lineStr)continue;
      let prevGenCol=0;
      for(const seg of lineStr.split(',')){
        if(!seg)continue;
        let step=decodeVlqValue(seg,0);
        if(!step)break;
        prevGenCol+=step[0];
        if(step[1]>=seg.length)continue;
        step=decodeVlqValue(seg,step[1]);if(!step)break;
        prevSrc+=step[0];
        step=decodeVlqValue(seg,step[1]);if(!step)break;
        prevOrigLine+=step[0];
        step=decodeVlqValue(seg,step[1]);if(!step)break;
        prevOrigCol+=step[0];
        let name=null;
        if(step[1]<seg.length){
          const nStep=decodeVlqValue(seg,step[1]);
          if(nStep){prevName+=nStep[0];name=names[Math.max(0,prevName)]||null;}
        }
        const srcIdx=Math.max(0,prevSrc);
        const source=sources[srcIdx];
        if(typeof source!=='string'||!source)continue;
        const origLine0=Math.max(0,prevOrigLine);
        const origCol0=Math.max(0,prevOrigCol);
        const origText=contentLines[srcIdx]?.[origLine0]??'';
        locations.push({
          generated_line:genLine+1,
          generated_column:Math.max(0,prevGenCol)+1,
          source,
          line:origLine0+1,
          column:utf16ColToCharCol(origText,origCol0),
          name,
          sourceLine:origText?origText.trim():null,
        });
      }
    }
    decodedMapCache.set(sourceMap,locations);
    return locations;
  }
  function lookupGeneratedPosition(locations,generatedLine,generatedColumn){
    if(!Array.isArray(locations)||locations.length===0||!(generatedLine>=1))return null;
    let chosen=null,firstOnLine=null;
    for(const loc of locations){
      if(loc.generated_line!==generatedLine)continue;
      if(!firstOnLine||loc.generated_column<firstOnLine.generated_column)firstOnLine=loc;
      if(loc.generated_column<=generatedColumn&&(!chosen||loc.generated_column>=chosen.generated_column)){
        chosen=loc;
      }
    }
    return chosen||firstOnLine;
  }
  function extractSourceLineFromUnit(unit,sourceName,line1){
    if(!(line1>=1)||!unit||typeof unit!=='object')return null;
    if(typeof unit.source==='string'&&(unit.location===sourceName||!sourceName)){
      const raw=unit.source.split('\n')[line1-1];
      if(typeof raw==='string'&&raw.trim())return raw.trim();
    }
    const sm=unit.source_map;
    if(sm&&Array.isArray(sm.sources)&&Array.isArray(sm.sourcesContent)){
      const idx=sm.sources.indexOf(sourceName);
      if(idx>=0&&typeof sm.sourcesContent[idx]==='string'){
        const raw=sm.sourcesContent[idx].split('\n')[line1-1];
        if(typeof raw==='string'&&raw.trim())return raw.trim();
      }
    }
    return null;
  }
  function extractCalledMemberFromSnippet(snippet){
    const text=String(snippet||'');
    const matches=[...text.matchAll(/\.([A-Za-z_$][A-Za-z0-9_$]*)\s*\(/g)];
    return matches.length>0?matches[matches.length-1][1]:null;
  }
  function humanizeRuntimeErrorMessage(rawMessage,sourceLine=null){
    let msg=String(rawMessage??'').trim();
    if(!msg)return 'Runtime error';
    msg=msg.replace(/\s*\((?:In|near)\s+'[^']*__nf[A-Za-z0-9_$]*[^']*'(?:,\s*[^)]*)?\)/g,'');
    msg=msg.replace(/\b__nf[A-Za-z0-9_$]+\b/g,'call');
    const snippet=typeof sourceLine==='string'?sourceLine:'';
    const hasLengthCall=/\.length\s*\(/.test(snippet)||/\.length\s*\(/.test(msg);
    const applyRe=/(?:TypeError:\s*)?(?:Reflect\.apply requires the first argument be a function\.?|Reflect\.apply:\s*target is not callable\.?|Function\.prototype\.apply was called on ([^,]+),\s*which is (?:a|an) ([^\s.]+)\s+and not a function\.?)/i;
    const applyMatch=msg.match(applyRe);
    if(applyMatch){
      let replacement;
      if(hasLengthCall){
        replacement="TypeError: 'length' is a property — use '.length' without '()'";
      }else{
        const member=extractCalledMemberFromSnippet(snippet);
        if(member){
          replacement=`TypeError: '${member}' is not a function (attempted to call a non-function value)`;
        }else{
          replacement='TypeError: Attempted to call a non-function value';
        }
      }
      const idx=applyMatch.index??0;
      const head=msg.slice(0,idx).trim();
      return head?`${head} ${replacement}`:replacement;
    }
    const arrLenMatch=msg.match(/Array\/custom member '([^']+)' is not callable\.?/);
    if(arrLenMatch){
      const member=arrLenMatch[1];
      if(member==='length'){
        return `TypeError: 'length' is a numeric property, not a method — use '.length' without '()' (${arrLenMatch[0]})`;
      }
      return `TypeError: Array member '${member}' is not a function (${arrLenMatch[0]})`;
    }
    if(/\.length is not a function\b/i.test(msg)&&!msg.includes("without '()'")){
      const prefix=/^TypeError:/i.test(msg)?msg:`TypeError: ${msg}`;
      return `${prefix} ('length' is a property — use '.length' without '()')`;
    }
    return msg;
  }
  const safeNativeStackGetter=(()=>{
    try{
      return Object.getOwnPropertyDescriptor(new Error(),'stack')?.get||Object.getOwnPropertyDescriptor(Error.prototype,'stack')?.get||null;
    }catch(_){return null;}
  })();
  function readOwnErrorProp(obj,prop){
    try{
      const desc=Object.getOwnPropertyDescriptor(Object(obj),prop);
      if(!desc)return undefined;
      if(Object.hasOwn(desc,'value'))return desc.value;
      if(prop==='stack'&&desc.get&&desc.get===safeNativeStackGetter)return safeNativeStackGetter.call(obj);
    }catch(_){}
    return undefined;
  }
  function resolveErrorLocation(errorOrEvent,options={}){
    const hostGlobal=options.hostGlobal||global;
    const bundle=options.bundle||hostGlobal?.NeoFlashBundle||null;
    const units=options.sourceUnits||(bundle?[...(bundle.scripts||[]),...(bundle.program?[bundle.program]:[])]:[]);
    const explicitSourceMap=options.sourceMap||bundle?.program?.source_map||null;
    const bodyOffset=options.bodyOffset!==undefined?options.bodyOffset:null;
    const pageCleanUrl=stripUrlQueryAndHash(options.pageUrl||hostGlobal?.location?.href||'');

    const isErrorEvent=Boolean(
      errorOrEvent&&typeof errorOrEvent==='object'&&(
        typeof errorOrEvent.lineno==='number'||typeof errorOrEvent.filename==='string'||'reason' in errorOrEvent
      )&&!(errorOrEvent instanceof Error)
    );
    const rootError=isErrorEvent
      ?('reason' in errorOrEvent?errorOrEvent.reason:(errorOrEvent.error??null))
      :errorOrEvent;

    const candidates=[];
    const visited=new Set();
    for(let cur=rootError,depth=0;cur!==null&&cur!==undefined&&(typeof cur==='object'||typeof cur==='function')&&depth<16&&!visited.has(cur);depth++){
      visited.add(cur);
      const stack=readOwnErrorProp(cur,'stack');
      if(typeof stack==='string'&&stack){
        for(const frame of parseStackFrames(stack))candidates.push(frame);
      }
      const ownLine=readOwnErrorProp(cur,'line')??readOwnErrorProp(cur,'lineNumber');
      const ownCol=readOwnErrorProp(cur,'column')??readOwnErrorProp(cur,'columnNumber');
      const ownUrl=readOwnErrorProp(cur,'sourceURL')??readOwnErrorProp(cur,'fileName');
      if(Number.isInteger(ownLine)&&ownLine>=1&&typeof ownUrl==='string'&&ownUrl){
        const cleanUrl=stripUrlQueryAndHash(ownUrl);
        candidates.push({
          functionName:null,
          url:ownUrl,
          cleanUrl,
          fileName:urlFileName(cleanUrl),
          line:ownLine,
          column:Number.isInteger(ownCol)&&ownCol>=1?ownCol:1,
        });
      }
      cur=readOwnErrorProp(cur,'cause');
    }
    if(isErrorEvent&&typeof errorOrEvent.filename==='string'&&errorOrEvent.filename&&Number.isInteger(errorOrEvent.lineno)&&errorOrEvent.lineno>=1){
      const cleanUrl=stripUrlQueryAndHash(errorOrEvent.filename);
      candidates.push({
        functionName:null,
        url:errorOrEvent.filename,
        cleanUrl,
        fileName:urlFileName(cleanUrl),
        line:errorOrEvent.lineno,
        column:Number.isInteger(errorOrEvent.colno)&&errorOrEvent.colno>=1?errorOrEvent.colno:1,
      });
    }

    for(const frame of candidates){
      if(isInternalHostUrl(frame.cleanUrl))continue;
      for(const unit of units){
        if(!unit||typeof unit!=='object')continue;
        let generatedLine=null;
        if(unit.source_url&&(frame.fileName===unit.source_url||frame.cleanUrl===unit.source_url)){
          if(Number.isInteger(unit.__domBodyStartLine)){
            generatedLine=frame.line-unit.__domBodyStartLine+1;
          }else if(Number.isInteger(bodyOffset)){
            generatedLine=frame.line-bodyOffset-(unit.source_url==='neoflash-project.js'?0:2);
          }else{
            generatedLine=frame.line;
          }
        }else if(
          Number.isInteger(unit.__domBodyStartLine)&&
          Number.isInteger(unit.__domBodyEndLine)&&
          frame.line>=unit.__domBodyStartLine&&
          frame.line<=unit.__domBodyEndLine&&
          (!pageCleanUrl||frame.cleanUrl===pageCleanUrl||frame.fileName==='index.html')
        ){
          generatedLine=frame.line-unit.__domBodyStartLine+1;
        }
        if(generatedLine===null||generatedLine<1)continue;
        const unitLocs=Array.isArray(unit.source_locations)&&unit.source_locations.length>0
          ?unit.source_locations
          :decodeSourceMapLocations(unit.source_map);
        const hit=lookupGeneratedPosition(unitLocs,generatedLine,frame.column);
        if(hit){
          const sourceLine=hit.sourceLine||extractSourceLineFromUnit(unit,hit.source,hit.line);
          return {
            source:hit.source,
            line:hit.line,
            column:hit.column,
            functionName:hit.name||frame.functionName||null,
            sourceLine:sourceLine||null,
            source_url:unit.source_url||frame.fileName,
          };
        }
      }
      if(explicitSourceMap&&(frame.fileName==='bundle.js'||frame.fileName===(explicitSourceMap.file||'')||!units.length)){
        const mapLocs=decodeSourceMapLocations(explicitSourceMap);
        const hit=lookupGeneratedPosition(mapLocs,frame.line,frame.column);
        if(hit){
          return {
            source:hit.source,
            line:hit.line,
            column:hit.column,
            functionName:hit.name||frame.functionName||null,
            sourceLine:hit.sourceLine||null,
            source_url:frame.fileName||'bundle.js',
          };
        }
      }
    }
    return null;
  }
  function extractErrorReason(errorOrEvent,sourceLine=null){
    if(errorOrEvent&&typeof errorOrEvent==='object'&&!(errorOrEvent instanceof Error)){
      if('reason' in errorOrEvent){
        return extractErrorReason(errorOrEvent.reason,sourceLine);
      }
      if(errorOrEvent.error!==undefined&&errorOrEvent.error!==null){
        const inner=extractErrorReason(errorOrEvent.error,sourceLine);
        if(inner)return inner;
      }
      if(typeof errorOrEvent.message==='string'&&errorOrEvent.message)return humanizeRuntimeErrorMessage(errorOrEvent.message,sourceLine);
    }
    if(errorOrEvent!==null&&errorOrEvent!==undefined&&(typeof errorOrEvent==='object'||typeof errorOrEvent==='function')){
      const name=readOwnErrorProp(errorOrEvent,'name')||errorOrEvent.constructor?.name||'';
      const msg=readOwnErrorProp(errorOrEvent,'message');
      if(typeof msg==='string'&&msg){
        const combined=name&&name!=='Error'&&!msg.startsWith(name+':')?`${name}: ${msg}`:msg;
        return humanizeRuntimeErrorMessage(combined,sourceLine);
      }
    }
    try{return humanizeRuntimeErrorMessage(String(errorOrEvent),sourceLine);}catch(_){return 'Uncaught runtime error';}
  }
  function installOutputRelay(hostGlobal=global){
    if(!hostGlobal||typeof hostGlobal!=='object')return null;
    if(hostGlobal.__neoflashOutputRelay)return hostGlobal.__neoflashOutputRelay;
    const doc=hostGlobal.document;
    const stageEl=typeof doc?.getElementById==='function'?doc.getElementById('stage'):null;
    const endpoint=String(
      stageEl?.getAttribute?.('data-neoflash-output-endpoint')||
      stageEl?.dataset?.neoflashOutputEndpoint||
      doc?.body?.getAttribute?.('data-neoflash-output-endpoint')||
      doc?.body?.dataset?.neoflashOutputEndpoint||
      ''
    ).trim();
    if(!endpoint||!isLoopbackHttpOrigin(hostGlobal.location))return null;
    const runIdRaw=String(
      extractQueryParam(hostGlobal.location?.search,'nf_run')||
      stageEl?.getAttribute?.('data-neoflash-run-id')||
      stageEl?.dataset?.neoflashRunId||
      doc?.body?.getAttribute?.('data-neoflash-run-id')||
      doc?.body?.dataset?.neoflashRunId||
      ''
    ).trim();
    const runId=runIdRaw||null;
    const MAX_BATCH=64,BATCH_WINDOW_MS=16,MAX_TRACE_PER_SEC=5000;
    const nowMs=typeof hostGlobal.performance?.now==='function'?()=>hostGlobal.performance.now():()=>Date.now();
    const session=`${Date.now().toString(36)}-${Math.random().toString(36).slice(2,10)}`;
    const reportedErrors=typeof WeakSet==='function'?new WeakSet():null;
    const queue=[];
    let seq=0,inFlight=false,timerId=0,lastFlushMs=-1e9,disabled=false;
    let rateWindowStartMs=0,rateWindowCount=0,suppressedLines=0;
    function clearTimer(){
      if(timerId){
        if(typeof hostGlobal.clearTimeout==='function')hostGlobal.clearTimeout(timerId);
        timerId=0;
      }
    }
    function haltRelay(staleSession=false){
      disabled=true;
      queue.length=0;
      suppressedLines=0;
      clearTimer();
      if(staleSession){
        try{hostGlobal.NeoFlashPlayer?.pause?.();}catch(_){}
      }
    }
    function markReportedError(err){
      if(!reportedErrors||err===null||err===undefined||(typeof err!=='object'&&typeof err!=='function'))return;
      try{
        reportedErrors.add(err);
        const cause=readOwnErrorProp(err,'cause');
        if(cause!==null&&cause!==undefined&&(typeof cause==='object'||typeof cause==='function'))reportedErrors.add(cause);
      }catch(_){}
    }
    function wasErrorReported(err){
      if(!reportedErrors||err===null||err===undefined||(typeof err!=='object'&&typeof err!=='function'))return false;
      try{
        if(reportedErrors.has(err))return true;
        const cause=readOwnErrorProp(err,'cause');
        if(cause!==null&&cause!==undefined&&(typeof cause==='object'||typeof cause==='function')&&reportedErrors.has(cause))return true;
      }catch(_){}
      return false;
    }
    function buildBatchPayload(batch){
      return JSON.stringify(runId?{session,runId,events:batch}:{session,events:batch});
    }
    function flushSuppressedNotice(){
      if(suppressedLines>0){
        const count=suppressedLines;
        suppressedLines=0;
        queue.push({seq:++seq,kind:'trace',message:`[trace] ... suppressed ${count} lines ...`,sourcePath:null,line:null,column:null,sourceLine:null});
      }
    }
    function scheduleNextFlush(){
      if(disabled||inFlight||timerId||queue.length===0)return;
      const elapsed=nowMs()-lastFlushMs;
      const urgent=queue.some(item=>item.kind!=='trace');
      if(urgent||elapsed>=BATCH_WINDOW_MS||queue.length>=MAX_BATCH){
        flushAsync();
      }else if(typeof hostGlobal.setTimeout==='function'){
        const wait=Math.max(1,Math.ceil(BATCH_WINDOW_MS-elapsed));
        timerId=hostGlobal.setTimeout(()=>{timerId=0;flushAsync();},wait);
      }else{
        flushAsync();
      }
    }
    function flushAsync(){
      clearTimer();
      if(disabled||inFlight||queue.length===0)return;
      if(typeof hostGlobal.fetch!=='function'){haltRelay(false);return;}
      inFlight=true;
      lastFlushMs=nowMs();
      const batch=queue.splice(0,MAX_BATCH);
      const payload=buildBatchPayload(batch);
      let request;
      try{
        request=Promise.resolve(hostGlobal.fetch(endpoint,{
          method:'POST',
          headers:{'Content-Type':'application/json'},
          body:payload,
          cache:'no-store',
          keepalive:payload.length<=32768,
        }));
      }catch(_){
        inFlight=false;haltRelay(false);return;
      }
      request.then(resp=>{
        const isStale=resp?.headers?.get?.('x-neoflash-session-stale')==='1';
        if(!resp||!resp.ok||isStale){
          haltRelay(Boolean(isStale));
        }
      },()=>{
        haltRelay(false);
      }).finally(()=>{
        inFlight=false;
        if(!disabled&&queue.length>0)scheduleNextFlush();
      });
    }
    function flushUnload(){
      clearTimer();
      if(disabled)return;
      flushSuppressedNotice();
      while(queue.length>0){
        const batch=queue.splice(0,MAX_BATCH);
        const payload=buildBatchPayload(batch);
        let sent=false;
        try{
          if(typeof hostGlobal.navigator?.sendBeacon==='function'){
            const body=typeof Blob==='function'?new Blob([payload],{type:'application/json'}):payload;
            sent=Boolean(hostGlobal.navigator.sendBeacon(endpoint,body));
          }
        }catch(_){sent=false;}
        if(!sent&&typeof hostGlobal.fetch==='function'){
          try{
            Promise.resolve(hostGlobal.fetch(endpoint,{
              method:'POST',
              headers:{'Content-Type':'application/json'},
              body:payload,
              cache:'no-store',
              keepalive:true,
            })).then(()=>{},()=>{});
          }catch(_){}
        }
      }
    }
    function record(kind,message,location){
      if(disabled)return;
      const now=nowMs();
      if(now-rateWindowStartMs>=1000){
        flushSuppressedNotice();
        rateWindowStartMs=now;
        rateWindowCount=0;
      }
      if(kind==='trace'){
        if(rateWindowCount>=MAX_TRACE_PER_SEC){
          suppressedLines++;
          return;
        }
        rateWindowCount++;
      }else{
        flushSuppressedNotice();
      }
      const cleanMessage=kind==='error'?humanizeRuntimeErrorMessage(message,location?.sourceLine):String(message??'');
      queue.push({
        seq:++seq,
        kind,
        message:cleanMessage,
        sourcePath:typeof location?.source==='string'&&location.source?location.source:null,
        line:Number.isInteger(location?.line)&&location.line>0?location.line:null,
        column:Number.isInteger(location?.column)&&location.column>0?location.column:null,
        sourceLine:typeof location?.sourceLine==='string'&&location.sourceLine?location.sourceLine:null,
      });
      if(kind!=='trace'){
        if(!inFlight)flushAsync();
      }else{
        scheduleNextFlush();
      }
    }
    hostGlobal.addEventListener?.('neoflash-script-message',event=>{
      const detail=event?.detail;
      if(!detail||typeof detail.kind!=='string')return;
      if(detail.error)markReportedError(detail.error);
      record(detail.kind,detail.message,detail.location);
    });
    hostGlobal.addEventListener?.('error',event=>{
      if(disabled||hostGlobal.__neoflashSuppressWindowError)return;
      const errObj=event?.error;
      if(errObj&&wasErrorReported(errObj))return;
      if(errObj)markReportedError(errObj);
      const loc=resolveErrorLocation(event,{hostGlobal});
      const reason=extractErrorReason(event,loc?.sourceLine);
      record('error',reason,loc);
    });
    hostGlobal.addEventListener?.('unhandledrejection',event=>{
      if(disabled||hostGlobal.__neoflashSuppressWindowError)return;
      const reasonObj=event?.reason;
      if(reasonObj&&wasErrorReported(reasonObj))return;
      if(reasonObj)markReportedError(reasonObj);
      const loc=resolveErrorLocation(event,{hostGlobal});
      const reason=extractErrorReason(event,loc?.sourceLine);
      record('error',`Unhandled promise rejection: ${reason}`,loc);
    });
    if(hostGlobal.console&&typeof hostGlobal.console==='object'&&!hostGlobal.console.__neoflashConsoleRelayInstalled){
      const consoleObj=hostGlobal.console;
      const formatConsoleCall=args=>{
        if(typeof hostGlobal.__neoflashConsoleFormat==='function'){
          return hostGlobal.__neoflashConsoleFormat(args);
        }
        if(typeof hostGlobal.NeoFlashFormatTrace?.formatTraceArgs==='function'){
          return {message:hostGlobal.NeoFlashFormatTrace.formatTraceArgs(args),location:null};
        }
        return {
          message:args.map(v=>{
            if(typeof v==='string')return v;
            if(v!==null&&typeof v==='object'){try{return JSON.stringify(v,null,2);}catch(_){}}
            return String(v);
          }).join(' '),
          location:null,
        };
      };
      const isInternalHostLog=args=>args.length>0&&typeof args[0]==='string'&&(args[0].startsWith('[NeoFlash]')||args[0].startsWith('NeoFlash:')||args[0].startsWith('NeoFlash '));
      for(const [method,kind] of [['log','trace'],['info','trace'],['debug','trace'],['dir','trace'],['warn','warning'],['error','error']]){
        const orig=consoleObj[method];
        if(typeof orig!=='function')continue;
        consoleObj[method]=function(...args){
          try{
            if(!hostGlobal.__neoflashSuppressConsoleRelay&&!isInternalHostLog(args)){
              const formatted=formatConsoleCall(args);
              record(kind,formatted.message,formatted.location);
            }
          }catch(_){}
          return orig.apply(this,args);
        };
      }
      try{Object.defineProperty(consoleObj,'__neoflashConsoleRelayInstalled',{value:true,configurable:true,enumerable:false});}catch(_){}
    }
    hostGlobal.addEventListener?.('pagehide',()=>flushUnload());
    hostGlobal.addEventListener?.('beforeunload',()=>flushUnload());
    const relay=Object.freeze({
      endpoint,
      session,
      runId,
      record,
      markReportedError,
      flush:()=>{flushSuppressedNotice();flushAsync();},
      flushUnload,
      get disabled(){return disabled;},
      get pending(){return queue.length;},
      get seq(){return seq;},
    });
    try{Object.defineProperty(hostGlobal,'__neoflashOutputRelay',{value:relay,configurable:true,enumerable:false});}catch(_){}
    return relay;
  }
  const api={layout,viewportBox,Preview,isLoopbackHttpOrigin,parseStackFrameLine,parseStackFrames,decodeSourceMapLocations,resolveErrorLocation,humanizeRuntimeErrorMessage,installOutputRelay};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashPreview=api;
  if(typeof document!=='undefined')installOutputRelay(global);
})(globalThis);
