/* Canvas geometry for NeoFlash vector paths. No flattening or bounding-box hit approximation. */
(function (global) {
  'use strict';

  function createPath(commands) {
    const path = new Path2D();
    for (const command of commands) {
      switch (command.op) {
        case 'move_to': path.moveTo(command.x, command.y); break;
        case 'line_to': path.lineTo(command.x, command.y); break;
        case 'quadratic_to': path.quadraticCurveTo(command.cx, command.cy, command.x, command.y); break;
        case 'cubic_to': path.bezierCurveTo(command.cx1, command.cy1, command.cx2, command.cy2, command.x, command.y); break;
        case 'close': path.closePath(); break;
        default: throw new Error(`Unsupported path operation: ${command.op}`);
      }
    }
    return path;
  }

  class PathCache {
    constructor({ maxKeyBytes = 8 * 1024 * 1024, maxEntries = 2048, pathFactory = createPath } = {}) {
      if (!Number.isSafeInteger(maxKeyBytes) || maxKeyBytes < 0 || !Number.isSafeInteger(maxEntries) || maxEntries < 0) {
        throw new Error('Path cache budgets must be non-negative safe integers.');
      }
      this.paths = new Map();
      this.keyBytes = 0;
      this.maxKeyBytes = maxKeyBytes;
      this.maxEntries = maxEntries;
      this.pathFactory = pathFactory;
    }
    get(commands) {
      const key = JSON.stringify(commands);
      const cached = this.paths.get(key);
      if (cached) return cached.path;
      const path = this.pathFactory(commands);
      // Count UTF-16 bytes conservatively. Native Path2D allocation is browser-dependent;
      // bounding geometry keys plus entry count also bounds the retained input complexity.
      const keyBytes = key.length * 2;
      if (keyBytes > this.maxKeyBytes || this.maxEntries <= 0) return path;
      while (this.paths.size >= this.maxEntries || this.keyBytes + keyBytes > this.maxKeyBytes) {
        const oldestKey = this.paths.keys().next().value;
        this.keyBytes -= this.paths.get(oldestKey).keyBytes;
        this.paths.delete(oldestKey);
      }
      this.paths.set(key, { path, keyBytes });
      this.keyBytes += keyBytes;
      return path;
    }
  }

  // Flash gradient coordinates map through the authored paint matrix. Canvas
  // applies its CTM at paint time (WHATWG 2D fill/stroke styles), not at creation.
  const RADIUS = 819.2;
  const rgba = c => `rgba(${c[0]},${c[1]},${c[2]},${c[3]/255})`;
  const multiply = (a,b) => [a[0]*b[0]+a[2]*b[1],a[1]*b[0]+a[3]*b[1],a[0]*b[2]+a[2]*b[3],a[1]*b[2]+a[3]*b[3],a[0]*b[4]+a[2]*b[5]+a[4],a[1]*b[4]+a[3]*b[5]+a[5]];
  const point = (m,x,y) => [m[0]*x+m[2]*y+m[4],m[1]*x+m[3]*y+m[5]];
  function inverse(m) {
    const d=m[0]*m[3]-m[1]*m[2];
    if(!Number.isFinite(d)||d===0)throw new Error('Gradient transform must be finite and invertible.');
    const result=[m[3]/d,-m[1]/d,-m[2]/d,m[0]/d,(m[2]*m[5]-m[3]*m[4])/d,(m[1]*m[4]-m[0]*m[5])/d];
    if(!result.every(Number.isFinite))throw new Error('Gradient inverse exceeds finite rendering bounds.');return result;
  }
  function validateGradient(paint) {
    if(!paint||!['linear','radial'].includes(paint.kind))throw new Error('Unsupported runtime gradient paint.');
    if(!Array.isArray(paint.matrix)||paint.matrix.length!==6||!paint.matrix.every(Number.isFinite))throw new Error('Invalid runtime gradient matrix.');
    inverse(paint.matrix);
    if(!Number.isFinite(paint.focal_point_ratio)||Math.abs(paint.focal_point_ratio)>1)throw new Error('Gradient focal point must be from -1 through 1.');
    if(!Array.isArray(paint.stops)||paint.stops.length<1||paint.stops.length>15)throw new Error('Gradient requires 1 through 15 color stops.');
    let ratio=-1;for(const stop of paint.stops){if(!Number.isInteger(stop.ratio)||stop.ratio<0||stop.ratio<ratio||stop.ratio>255||!Array.isArray(stop.color)||stop.color.length!==4||!stop.color.every(c=>Number.isInteger(c)&&c>=0&&c<=255))throw new Error('Invalid runtime gradient color stop.');ratio=stop.ratio;}
    return paint;
  }
  function gradientFor(context,paint) {
    validateGradient(paint);
    const gradient=paint.kind==='linear'?context.createLinearGradient(-RADIUS,0,RADIUS,0):context.createRadialGradient(paint.focal_point_ratio*RADIUS,0,0,0,0,RADIUS);
    for(const stop of paint.stops)gradient.addColorStop(stop.ratio/255,rgba(stop.color));return gradient;
  }
  function paintVisible(kind,channel) {
    if(channel==='fill'&&kind.fill_bitmap)return kind.fill_bitmap.width>0&&kind.fill_bitmap.height>0;
    const paint=kind[channel+'_gradient'];return paint?validateGradient(paint).stops.some(s=>s.color[3]>0):(kind[channel]?.[3]||0)>0;
  }
  function gradientParameter(paint,x,y) {
    const q=point(inverse(paint.matrix),x,y);
    if(paint.kind==='linear')return(q[0]/RADIUS+1)/2;
    const f=paint.focal_point_ratio,dx=q[0]/RADIUS-f,dy=q[1]/RADIUS,n=dx*dx+dy*dy;
    if(n===0)return 0;
    const denominator=Math.sqrt(f*f*dx*dx+(1-f*f)*n)-f*dx;
    return denominator>0?n/denominator:null; // Endpoint focal cones do not paint their exterior.
  }
  function paintAlpha(kind,channel,x,y) {
    if(channel==='fill'&&kind.fill_bitmap)return kind.fill_bitmap.width>0&&kind.fill_bitmap.height>0?1:0;
    const paint=kind[channel+'_gradient'];if(!paint)return(kind[channel]?.[3]||0)/255;validateGradient(paint);
    const t=gradientParameter(paint,x,y);if(t===null)return 0;const ratio=t*255,stops=paint.stops;
    if(ratio<stops[0].ratio)return stops[0].color[3]/255;
    for(let i=1;i<stops.length;i++)if(ratio<stops[i].ratio){const before=stops[i-1],after=stops[i],f=(ratio-before.ratio)/(after.ratio-before.ratio);return(before.color[3]+(after.color[3]-before.color[3])*f)/255;}
    return stops.at(-1).color[3]/255;
  }
  const corners = b => [[b.left,b.top],[b.right,b.top],[b.right,b.bottom],[b.left,b.bottom]];
  function box(points) {if(!points.length)return null;const b={left:Infinity,top:Infinity,right:-Infinity,bottom:-Infinity};for(const p of points){b.left=Math.min(b.left,p[0]);b.top=Math.min(b.top,p[1]);b.right=Math.max(b.right,p[0]);b.bottom=Math.max(b.bottom,p[1]);}return Object.values(b).every(Number.isFinite)?b:null;}
  const intersect=(a,b)=>{if(!a||!b)return null;const r={left:Math.max(a.left,b.left),top:Math.max(a.top,b.top),right:Math.min(a.right,b.right),bottom:Math.min(a.bottom,b.bottom)};return r.right>r.left&&r.bottom>r.top?r:null;};
  const union=(a,b)=>!a?b:!b?a:{left:Math.min(a.left,b.left),top:Math.min(a.top,b.top),right:Math.max(a.right,b.right),bottom:Math.max(a.bottom,b.bottom)};
  function clipPolygon(poly,evaluate) {
    const result=[];for(let i=0;i<poly.length;i++){const a=poly[i],b=poly[(i+1)%poly.length],fa=evaluate(a),fb=evaluate(b);if(fa>=0)result.push(a);if((fa<0)!==(fb<0)){const t=fa/(fa-fb);result.push([a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t]);}}return result;
  }
  function visibleIntervals(paint) {
    const stops=paint.stops,result=[];if(stops[0].color[3]>0)result.push([-Infinity,stops[0].ratio/255]);
    for(let i=1;i<stops.length;i++)if(stops[i].ratio>stops[i-1].ratio&&(stops[i].color[3]>0||stops[i-1].color[3]>0))result.push([stops[i-1].ratio/255,stops[i].ratio/255]);
    if(stops.at(-1).color[3]>0)result.push([stops.at(-1).ratio/255,Infinity]);return result;
  }
  function circleIntersectsPolygon(poly,cx,r) {
    let sign=0,inside=true,min=Infinity;
    for(let i=0;i<poly.length;i++){const a=poly[i],b=poly[(i+1)%poly.length],dx=b[0]-a[0],dy=b[1]-a[1],cross=dx*(-a[1])-dy*(cx-a[0]);if(cross){const next=Math.sign(cross);if(sign&&next!==sign)inside=false;sign=next;}
      const len=dx*dx+dy*dy,t=len?Math.max(0,Math.min(1,((cx-a[0])*dx-a[1]*dy)/len)):0;min=Math.min(min,(a[0]+t*dx-cx)**2+(a[1]+t*dy)**2);}
    return inside||min<r*r;
  }
  // Conservative geometric bounds intersected with regions of nonzero paint
  // alpha. Exact curve coverage remains Path2D's job; this is not a pixel scan.
  function visibleBounds(kind,channel,bounds,matrix) {
    if(!bounds||!paintVisible(kind,channel))return null;const paint=kind[channel+'_gradient'];if(!paint)return bounds;
    const world=multiply(matrix,paint.matrix),inv=inverse(world);let result=null;
    for(let[lo,hi]of visibleIntervals(paint)){
      let polygon=corners(bounds),candidate=null;
      if(paint.kind==='linear'){
        const coordinate=p=>(point(inv,...p)[0]/RADIUS+1)/2;
        if(Number.isFinite(lo))polygon=clipPolygon(polygon,p=>coordinate(p)-lo);
        if(Number.isFinite(hi))polygon=clipPolygon(polygon,p=>hi-coordinate(p));candidate=intersect(bounds,box(polygon));
      }else{
        lo=Math.max(0,lo);if(hi<=lo)continue;const f=paint.focal_point_ratio;
        if(Math.abs(f)===1)polygon=clipPolygon(polygon,p=>RADIUS-f*point(inv,...p)[0]);
        if(!polygon.length)continue;const canonical=polygon.map(p=>point(inv,...p));
        if(lo>0&&canonical.every(p=>(p[0]-f*RADIUS*(1-lo))**2+p[1]**2<=(RADIUS*lo)**2))continue;
        candidate=box(polygon);
        if(Number.isFinite(hi)){
          const cx=f*RADIUS*(1-hi),radius=RADIUS*hi;if(!circleIntersectsPolygon(canonical,cx,radius))continue;
          const center=point(world,cx,0),rx=radius*Math.hypot(world[0],world[2]),ry=radius*Math.hypot(world[1],world[3]);candidate=intersect(candidate,{left:center[0]-rx,top:center[1]-ry,right:center[0]+rx,bottom:center[1]+ry});
        }
      }
      result=union(result,candidate);
    }
    return result;
  }
  function matrixOf(context){const m=context.getTransform();return[m.a,m.b,m.c,m.d,m.e,m.f];}
  function brush(context,paint,width,height) {
    context.transform(...paint.matrix);const inverseBrush=inverse(matrixOf(context)),area=box([[0,0],[width,0],[width,height],[0,height]].map(p=>point(inverseBrush,...p)));
    if(!area)throw new Error('Gradient brush bounds exceed finite rendering limits.');context.fillStyle=gradientFor(context,paint);context.fillRect(area.left,area.top,area.right-area.left,area.bottom-area.top);
  }
  function bitmapBrush(context,paint,bitmaps,width,height) {
    if(!bitmaps)throw new Error('Bitmap fill rendering requires the bitmap cache.');
    bitmaps.paint(paint,record=>{
      const m=paint.matrix,det=m[0]*m[3]-m[1]*m[2];
      if(!Number.isFinite(det)||det===0){
        const base=matrixOf(context),baseDet=base[0]*base[3]-base[1]*base[2];if(!Number.isFinite(baseDet)||baseDet===0)return;
        const area=box([[0,0],[width,0],[width,height],[0,height]].map(p=>point(inverse(base),...p)));if(!area)return;
        context.fillStyle=rgba(record.bytes.slice(0,4));context.fillRect(area.left,area.top,area.right-area.left,area.bottom-area.top);return;
      }
      context.transform(...m);const total=matrixOf(context),totalDet=total[0]*total[3]-total[1]*total[2];if(!Number.isFinite(totalDet)||totalDet===0)return;
      const area=box([[0,0],[width,0],[width,height],[0,height]].map(p=>point(inverse(total),...p)));if(!area)return;
      context.imageSmoothingEnabled=Boolean(paint.smoothing);
      if(paint.repeat){
        const pattern=context.createPattern(record.canvas,'repeat');
        if(pattern){context.fillStyle=pattern;context.fillRect(area.left,area.top,area.right-area.left,area.bottom-area.top);}
        return;
      }
      const w=record.width,h=record.height,x0=Math.floor(area.left)-1,y0=Math.floor(area.top)-1,x1=Math.ceil(area.right)+1,y1=Math.ceil(area.bottom)+1;
      const corner=(cx,cy)=>rgba(record.bytes.slice((cy*w+cx)*4,(cy*w+cx)*4+4));
      context.drawImage(record.canvas,0,0,w,h);
      if(y0<0)context.drawImage(record.canvas,0,0,w,1,0,y0,w,-y0);
      if(y1>h)context.drawImage(record.canvas,0,h-1,w,1,0,h,w,y1-h);
      if(x0<0)context.drawImage(record.canvas,0,0,1,h,x0,0,-x0,h);
      if(x1>w)context.drawImage(record.canvas,w-1,0,1,h,w,0,x1-w,h);
      if(x0<0&&y0<0){context.fillStyle=corner(0,0);context.fillRect(x0,y0,-x0,-y0);}
      if(x1>w&&y0<0){context.fillStyle=corner(w-1,0);context.fillRect(w,y0,x1-w,-y0);}
      if(x0<0&&y1>h){context.fillStyle=corner(0,h-1);context.fillRect(x0,h,-x0,y1-h);}
      if(x1>w&&y1>h){context.fillStyle=corner(w-1,h-1);context.fillRect(w,h,x1-w,y1-h);}
    });
  }
  function controlBounds(commands,matrix,padding) {
    let bounds=null;for(const c of commands){const points=[];if(c.op!=='close')points.push([c.x,c.y]);if(c.op==='quadratic_to')points.push([c.cx,c.cy]);if(c.op==='cubic_to')points.push([c.cx1,c.cy1],[c.cx2,c.cy2]);if(points.length)bounds=union(bounds,box(points.map(p=>point(matrix,...p))));}
    if(bounds){const dx=padding*Math.hypot(matrix[0],matrix[2])+1,dy=padding*Math.hypot(matrix[1],matrix[3])+1;bounds={left:bounds.left-dx,top:bounds.top-dy,right:bounds.right+dx,bottom:bounds.bottom+dy};}return bounds;
  }
  class GradientRenderer {
    constructor({maxMaskPixels=16*1024*1024,createCanvas}={}) {
      if(!Number.isSafeInteger(maxMaskPixels)||maxMaskPixels<1)throw new Error('Gradient mask pixel budget must be a positive safe integer.');this.maxMaskPixels=maxMaskPixels;this.surface=null;
      this.createCanvas=createCanvas||(()=>typeof OffscreenCanvas==='function'?new OffscreenCanvas(1,1):global.document.createElement('canvas'));
    }
    clear(){if(this.surface){this.surface.width=1;this.surface.height=1;this.surface=null;}}
    draw(context,kind,path,bitmaps=null) {
      if(kind.fill_gradient||kind.stroke_gradient||kind.fill_bitmap){const m=matrixOf(context),det=m[0]*m[3]-m[1]*m[2];if(!Number.isFinite(det)||det===0)return;}
      for(const channel of ['fill','stroke']){
        if(!paintVisible(kind,channel)||channel==='stroke'&&!(kind.stroke_width>0))continue;
        const paint=kind[channel+'_gradient'],bitmap=channel==='fill'?kind.fill_bitmap:null;context.save();
        try{
          if(channel==='stroke'){context.lineWidth=kind.stroke_width;context.lineCap='round';context.lineJoin='round';}
          if(bitmap){context.clip(path,kind.fill_rule||'nonzero');bitmapBrush(context,bitmap,bitmaps,context.canvas.width,context.canvas.height);}
          else if(!paint){context[channel+'Style']=rgba(kind[channel]);if(channel==='fill')context.fill(path,kind.fill_rule||'nonzero');else context.stroke(path);}
          else if(channel==='fill'){context.clip(path,kind.fill_rule||'nonzero');brush(context,paint,context.canvas.width,context.canvas.height);}
          else this.stroke(context,kind,path,paint);
        }finally{context.restore();}
      }
    }
    stroke(context,kind,path,paint) {
      const matrix=matrixOf(context),raw=controlBounds(kind.commands,matrix,kind.stroke_width/2),bounds=intersect(raw,{left:0,top:0,right:context.canvas.width,bottom:context.canvas.height});if(!bounds)return;
      const left=Math.floor(bounds.left),top=Math.floor(bounds.top),width=Math.ceil(bounds.right)-left,height=Math.ceil(bounds.bottom)-top;
      if(width*height>this.maxMaskPixels)throw new RangeError('Gradient stroke mask exceeds 16777216 pixels; reduce publication dimensions or stroke coverage.');
      const canvas=this.surface||(this.surface=this.createCanvas());if(canvas.width!==width||canvas.height!==height){canvas.width=1;canvas.height=height;canvas.width=width;}
      const mask=canvas.getContext('2d');if(!mask)throw new Error('Canvas 2D gradient stroke mask is unavailable.');
      mask.setTransform(1,0,0,1,0,0);mask.globalAlpha=1;mask.globalCompositeOperation='source-over';mask.clearRect(0,0,width,height);
      mask.setTransform(matrix[0],matrix[1],matrix[2],matrix[3],matrix[4]-left,matrix[5]-top);mask.strokeStyle='#fff';mask.lineWidth=kind.stroke_width;mask.lineCap='round';mask.lineJoin='round';mask.stroke(path);
      mask.globalCompositeOperation='source-in';brush(mask,paint,width,height);mask.globalCompositeOperation='source-over';
      context.setTransform(1,0,0,1,0,0);context.drawImage(canvas,left,top);
    }
  }

  function clipTable(clips=[]) {
    const result=new Map();for(const clip of clips){if(typeof clip.id!=='string'||!Array.isArray(clip.components)||result.has(clip.id))throw new Error('Invalid runtime clip table.');result.set(clip.id,clip);}return result;
  }
  function clipGroups(ids,table) {
    return [...new Set(ids||[])].map(id=>{const clip=table.get(id);if(!clip)throw new Error(`Runtime clip '${id}' is missing; re-export this project.`);return clip;});
  }
  function clipsContain(context,ids,table,x,y,cache) {
    context.save();try{
      context.setTransform(1,0,0,1,0,0);
      return clipGroups(ids,table).every(clip=>clip.components.some(component=>{
        const m=component.matrix,det=m[0]*m[3]-m[1]*m[2];if(!Number.isFinite(det)||det===0)return false;
        const p=point(inverse(m),x,y);return context.isPointInPath(cache.get(component.commands),p[0],p[1],component.fill_rule||'nonzero');
      }));
    }finally{context.restore();}
  }
  // Each clip is a UNION of independently filled components. Combining their
  // commands into one Path2D changes overlap/winding semantics. Nested clips are
  // intersections. Two reusable surfaces preserve those rules for all paints.
  class ClipRenderer {
    constructor({maxSurfacePixels=16*1024*1024,maxReadPixels=16*1024*1024,maxReadTilePixels=262144,createCanvas}={}) {
      for(const value of [maxSurfacePixels,maxReadPixels,maxReadTilePixels])if(!Number.isSafeInteger(value)||value<1)throw new Error('Clipping budgets must be positive safe integers.');
      Object.assign(this,{maxSurfacePixels,maxReadPixels,maxReadTilePixels});this.surfaces=[];this.readPixels=0;
      this.createCanvas=createCanvas||(()=>typeof OffscreenCanvas==='function'?new OffscreenCanvas(1,1):global.document.createElement('canvas'));
    }
    beginFrame(){this.readPixels=0;}
    clear(){for(const surface of this.surfaces){surface.width=1;surface.height=1;}this.surfaces=[];this.readPixels=0;}
    surface(index,width,height){
      const surface=this.surfaces[index]||(this.surfaces[index]=this.createCanvas());
      if(surface.width!==width||surface.height!==height){surface.width=1;surface.height=height;surface.width=width;}
      const ctx=surface.getContext('2d');if(!ctx)throw new Error('Canvas 2D clipping surface is unavailable.');
      ctx.setTransform(1,0,0,1,0,0);ctx.globalAlpha=1;ctx.globalCompositeOperation='source-over';ctx.clearRect(0,0,width,height);return ctx;
    }
    draw(context,item,table,cache,draw,{bounds=null,readBounds=false}={}) {
      const groups=clipGroups(item.clip_ids,table);if(!groups.length){draw(context);return undefined;}if(groups.some(g=>!g.components.length))return null;
      const base=matrixOf(context),det=base[0]*base[3]-base[1]*base[2];if(!Number.isFinite(det)||det===0)return null;
      const viewport={left:0,top:0,right:context.canvas.width,bottom:context.canvas.height};
      let area=bounds?intersect(box(corners(bounds).map(p=>point(base,...p))),viewport):viewport;
      // Control hulls are used only to bound scratch allocation. Actual clipping
      // and semantic coverage below still use filled curves and their holes.
      for(const group of groups){let extent=null;for(const c of group.components)extent=union(extent,controlBounds(c.commands,multiply(base,c.matrix),0));area=intersect(area,extent);if(!area)return null;}
      const left=Math.max(0,Math.floor(area.left)-1),top=Math.max(0,Math.floor(area.top)-1),right=Math.min(context.canvas.width,Math.ceil(area.right)+1),bottom=Math.min(context.canvas.height,Math.ceil(area.bottom)+1),width=right-left,height=bottom-top;
      if(!(width>0&&height>0))return null;
      if(width*height>this.maxSurfacePixels)throw new RangeError('Clipping surface exceeds 16777216 pixels; reduce publication dimensions or clipped coverage.');
      if(readBounds&&this.readPixels+width*height>this.maxReadPixels)throw new RangeError('Clipped semantic coverage exceeds the per-frame readback budget; reduce semantic clipped coverage.');
      const content=this.surface(0,width,height),shifted=[base[0],base[1],base[2],base[3],base[4]-left,base[5]-top];content.setTransform(...shifted);draw(content);
      for(const group of groups){
        const mask=this.surface(1,width,height);mask.fillStyle='#fff';
        for(const component of group.components){const m=multiply(shifted,component.matrix),d=m[0]*m[3]-m[1]*m[2];if(!Number.isFinite(d)||d===0)continue;mask.setTransform(...m);mask.fill(cache.get(component.commands),component.fill_rule||'nonzero');}
        content.setTransform(1,0,0,1,0,0);content.globalAlpha=1;content.globalCompositeOperation='destination-in';content.drawImage(mask.canvas,0,0);
      }
      content.globalCompositeOperation='source-over';
      let visible=null;
      if(readBounds){
        this.readPixels+=width*height;const rows=Math.max(1,Math.min(64,Math.floor(this.maxReadTilePixels/width)));let minX=width,minY=height,maxX=-1,maxY=-1;
        // Width is itself bounded by the publication canvas and surface budget.
        // Do not silently exceed the explicitly requested temporary read budget.
        if(width>this.maxReadTilePixels)throw new RangeError('Clipped semantic readback row exceeds the tile budget.');
        for(let y=0;y<height;y+=rows){const h=Math.min(rows,height-y),pixels=content.getImageData(0,y,width,h).data;for(let offset=3;offset<pixels.length;offset+=4)if(pixels[offset]){const p=(offset-3)/4,x=p%width,py=y+Math.floor(p/width);minX=Math.min(minX,x);maxX=Math.max(maxX,x);minY=Math.min(minY,py);maxY=Math.max(maxY,py);}}
        if(maxX>=0)visible=box(corners({left:left+minX,top:top+minY,right:left+maxX+1,bottom:top+maxY+1}).map(p=>point(inverse(base),...p)));
      }
      context.save();try{context.setTransform(1,0,0,1,0,0);context.drawImage(content.canvas,left,top);}finally{context.restore();}
      return readBounds?visible:undefined;
    }
  }

  function contains(context, kind, x, y, path = createPath(kind.commands)) {
    context.save();
    try {
      // The caller already applied the inverse composed instance transform to the point.
      // DPR and the last rendered object's transform must not affect local geometry tests.
      context.setTransform(1, 0, 0, 1, 0, 0);
      if (paintAlpha(kind,'fill',x,y)>0 && context.isPointInPath(path, x, y, kind.fill_rule || 'nonzero')) return true;
      if (kind.stroke_width<=0 || paintAlpha(kind,'stroke',x,y)<=0) return false;
      context.lineWidth = kind.stroke_width;
      context.lineCap = 'round';
      context.lineJoin = 'round';
      return context.isPointInStroke(path, x, y);
    } finally {
      context.restore();
    }
  }

  const api = { createPath, PathCache, contains, paintVisible, paintAlpha, visibleBounds, GradientRenderer, clipTable, clipsContain, ClipRenderer };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else global.NeoFlashPaths = api;
})(globalThis);
