/* Viewing transform only: authored coordinates never change. */
(function(global){
  'use strict';
  function layout(width,height,viewportWidth,viewportHeight,mode,gutter=16){
    const fit=Math.min(1,Math.max(1,viewportWidth-2*gutter)/width,Math.max(1,viewportHeight-2*gutter)/height);
    const scale=mode==='fit'?fit:Number(mode);
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
      Object.assign(this.space.style,{width:next.spaceWidth+'px',height:next.spaceHeight+'px'});
      Object.assign(this.surface.style,{left:next.left+'px',top:next.top+'px',width:next.stageWidth+'px',height:next.stageHeight+'px'});
      const logicalWidth=next.logicalWidth||this.project.width,logicalHeight=next.logicalHeight||this.project.height;
      const ratio=Math.min((global.devicePixelRatio||1)*Math.max(next.scale,next.scaleY||next.scale),4,8192/logicalWidth,8192/logicalHeight);
      const width=Math.max(1,Math.round(logicalWidth*ratio)),height=Math.max(1,Math.round(logicalHeight*ratio));
      if(this.canvas.width!==width||this.canvas.height!==height){this.canvas.width=width;this.canvas.height=height;}
      if(this.stageLayout||this.mode==='fit'){v.scrollLeft=0;v.scrollTop=0;}else if(old){v.scrollLeft=center[0]*next.scale+next.left-v.clientWidth/2;v.scrollTop=center[1]*next.scale+next.top-v.clientHeight/2;}
      return next;
    }
  }
  const api={layout,Preview};if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashPreview=api;
})(globalThis);
