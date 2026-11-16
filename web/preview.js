/* Viewing transform only: authored coordinates never change. */
(function(global){
  'use strict';
  function layout(width,height,viewportWidth,viewportHeight,mode,gutter=16){
    const availW=Math.max(1,viewportWidth-2*gutter),availH=Math.max(1,viewportHeight-2*gutter);
    const fit=Math.min(1,availW/width,availH/height);
    if(mode==='stretch'){
      const scale=availW/width,scaleY=availH/height,stageWidth=width*scale,stageHeight=height*scaleY;
      return {scale,scaleY,stageWidth,stageHeight,left:Math.max(gutter,(viewportWidth-stageWidth)/2),top:Math.max(gutter,(viewportHeight-stageHeight)/2),spaceWidth:Math.max(viewportWidth,stageWidth+2*gutter),spaceHeight:Math.max(viewportHeight,stageHeight+2*gutter)};
    }
    const scale=mode==='fit'?fit:mode==='fill'?Math.max(availW/width,availH/height):Number(mode);
    if(!Number.isFinite(scale)||scale<=0)throw new Error('Invalid preview scale');
    const stageWidth=width*scale,stageHeight=height*scale;
    return {scale,stageWidth,stageHeight,left:Math.max(gutter,(viewportWidth-stageWidth)/2),top:Math.max(gutter,(viewportHeight-stageHeight)/2),spaceWidth:Math.max(viewportWidth,stageWidth+2*gutter),spaceHeight:Math.max(viewportHeight,stageHeight+2*gutter)};
  }
  class Preview {
    constructor(viewport,space,surface,canvas,project,onChange){
      Object.assign(this,{viewport,space,surface,canvas,project,onChange});this.mode='fit';this.panMode=false;this.suppressClick=false;this.view=null;this.drag=null;
      viewport.addEventListener('pointerdown',event=>{
        if(!(this.panMode||event.button===1)||event.pointerType==='touch')return;
        this.drag={id:event.pointerId,x:event.clientX,y:event.clientY,left:viewport.scrollLeft,top:viewport.scrollTop};
        viewport.setPointerCapture(event.pointerId);event.preventDefault();
      });
      viewport.addEventListener('pointermove',event=>{if(this.drag&&event.pointerId===this.drag.id){viewport.scrollLeft=this.drag.left+this.drag.x-event.clientX;viewport.scrollTop=this.drag.top+this.drag.y-event.clientY;}});
      const end=event=>{if(this.drag&&event.pointerId===this.drag.id){this.drag=null;this.suppressClick=true;setTimeout(()=>{this.suppressClick=false;},0);if(viewport.hasPointerCapture(event.pointerId))viewport.releasePointerCapture(event.pointerId);}};
      viewport.addEventListener('pointerup',end);viewport.addEventListener('pointercancel',end);viewport.addEventListener('lostpointercapture',end);
    }
    setMode(mode){this.mode=mode;this.resize();this.onChange();}
    setStageLayout(provider){this.stageLayout=provider;this.resize();}
    setPan(enabled){this.panMode=enabled;this.viewport.classList.toggle('pan-mode',enabled);}
    resize(){
      if(this.drag){const id=this.drag.id;this.drag=null;this.suppressClick=true;setTimeout(()=>{this.suppressClick=false;},0);if(this.viewport.hasPointerCapture(id))this.viewport.releasePointerCapture(id);}
      const v=this.viewport,old=this.view;
      const center=old?[(v.scrollLeft+v.clientWidth/2-old.left)/old.scale,(v.scrollTop+v.clientHeight/2-old.top)/(old.scaleY||old.scale)]:[this.project.width/2,this.project.height/2];
      this.view=this.stageLayout?this.stageLayout(Math.max(1,v.clientWidth),Math.max(1,v.clientHeight)):layout(this.project.width,this.project.height,v.clientWidth,v.clientHeight,this.mode);
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
      if(this.stageLayout||this.mode==='fit'){v.scrollLeft=0;v.scrollTop=0;}else if(old){v.scrollLeft=center[0]*next.scale+next.left-v.clientWidth/2;v.scrollTop=center[1]*next.scale+next.top-v.clientHeight/2;}
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
    const MAX_BATCH=64,BATCH_WINDOW_MS=16,MAX_TRACE_PER_SEC=5000;
    const nowMs=typeof hostGlobal.performance?.now==='function'?()=>hostGlobal.performance.now():()=>Date.now();
    const session=`${Date.now().toString(36)}-${Math.random().toString(36).slice(2,10)}`;
    const queue=[];
    let seq=0,inFlight=false,timerId=0,lastFlushMs=-1e9,disabled=false;
    let rateWindowStartMs=0,rateWindowCount=0,suppressedLines=0;
    function clearTimer(){
      if(timerId){
        if(typeof hostGlobal.clearTimeout==='function')hostGlobal.clearTimeout(timerId);
        timerId=0;
      }
    }
    function flushSuppressedNotice(){
      if(suppressedLines>0){
        const count=suppressedLines;
        suppressedLines=0;
        queue.push({seq:++seq,kind:'trace',message:`[trace] ... suppressed ${count} lines ...`,sourcePath:null,line:null,column:null});
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
      if(typeof hostGlobal.fetch!=='function'){disabled=true;queue.length=0;return;}
      inFlight=true;
      lastFlushMs=nowMs();
      const batch=queue.splice(0,MAX_BATCH);
      const payload=JSON.stringify({session,events:batch});
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
        inFlight=false;disabled=true;queue.length=0;return;
      }
      request.then(resp=>{
        if(!resp||!resp.ok){disabled=true;queue.length=0;}
      },()=>{
        disabled=true;queue.length=0;
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
        const payload=JSON.stringify({session,events:batch});
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
      queue.push({
        seq:++seq,
        kind,
        message:String(message??''),
        sourcePath:typeof location?.source==='string'&&location.source?location.source:null,
        line:Number.isInteger(location?.line)&&location.line>0?location.line:null,
        column:Number.isInteger(location?.column)&&location.column>0?location.column:null,
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
      record(detail.kind,detail.message,detail.location);
    });
    hostGlobal.addEventListener?.('pagehide',()=>flushUnload());
    hostGlobal.addEventListener?.('beforeunload',()=>flushUnload());
    const relay=Object.freeze({
      endpoint,
      session,
      record,
      flush:()=>{flushSuppressedNotice();flushAsync();},
      flushUnload,
      get pending(){return queue.length;},
      get seq(){return seq;},
    });
    try{Object.defineProperty(hostGlobal,'__neoflashOutputRelay',{value:relay,configurable:true,enumerable:false});}catch(_){}
    return relay;
  }
  const api={layout,Preview,isLoopbackHttpOrigin,installOutputRelay};if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashPreview=api;
})(globalThis);
