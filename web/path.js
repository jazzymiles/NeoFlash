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
  const srgbToLinear = c => { const s = c / 255; return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4); };
  const linearToSrgb = l => { const v = Math.max(0, Math.min(1, l)); const s = v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055; return Math.max(0, Math.min(255, Math.round(s * 255))); };
  function spreadModeOf(paint) {
    const s = paint?.spread || 'extend';
    if (s === 'extend' || s === 'pad') return 'extend';
    if (s === 'reflect' || s === 'repeat') return s;
    throw new Error(`Unsupported runtime gradient spread: ${s}`);
  }
  function spreadUnit(mode, u) {
    if (mode === 'repeat') return u >= 0 && u <= 1 ? u : ((u % 1) + 1) % 1;
    if (mode === 'reflect') {
      if (u >= 0 && u <= 1) return u;
      const p = ((u % 2) + 2) % 2;
      return p <= 1 ? p : 2 - p;
    }
    return Math.max(0, Math.min(1, u));
  }
  function interpolateColor(c0, c1, f, linearRgb) {
    const a = Math.max(0, Math.min(255, Math.round(c0[3] + (c1[3] - c0[3]) * f)));
    if (linearRgb) {
      return [
        linearToSrgb(srgbToLinear(c0[0]) + (srgbToLinear(c1[0]) - srgbToLinear(c0[0])) * f),
        linearToSrgb(srgbToLinear(c0[1]) + (srgbToLinear(c1[1]) - srgbToLinear(c0[1])) * f),
        linearToSrgb(srgbToLinear(c0[2]) + (srgbToLinear(c1[2]) - srgbToLinear(c0[2])) * f),
        a,
      ];
    }
    return [
      Math.max(0, Math.min(255, Math.round(c0[0] + (c1[0] - c0[0]) * f))),
      Math.max(0, Math.min(255, Math.round(c0[1] + (c1[1] - c0[1]) * f))),
      Math.max(0, Math.min(255, Math.round(c0[2] + (c1[2] - c0[2]) * f))),
      a,
    ];
  }
  function unitStops(paint) {
    const stops = paint.stops, linearRgb = Boolean(paint.linear_rgb), out = [];
    if (stops[0].ratio > 0) out.push({ u: 0, color: stops[0].color });
    for (let i = 0; i < stops.length; i++) {
      const cur = stops[i];
      out.push({ u: cur.ratio / 255, color: cur.color });
      if (linearRgb && i + 1 < stops.length && stops[i + 1].ratio > cur.ratio) {
        const nxt = stops[i + 1], steps = 16;
        for (let k = 1; k < steps; k++) {
          const f = k / steps;
          out.push({ u: (cur.ratio + (nxt.ratio - cur.ratio) * f) / 255, color: interpolateColor(cur.color, nxt.color, f, true) });
        }
      }
    }
    if (stops.at(-1).ratio < 255) out.push({ u: 1, color: stops.at(-1).color });
    return out;
  }
  function validateGradient(paint) {
    if(!paint||!['linear','radial'].includes(paint.kind))throw new Error('Unsupported runtime gradient paint.');
    spreadModeOf(paint);
    if(!Array.isArray(paint.matrix)||paint.matrix.length!==6||!paint.matrix.every(Number.isFinite))throw new Error('Invalid runtime gradient matrix.');
    inverse(paint.matrix);
    if(!Number.isFinite(paint.focal_point_ratio)||Math.abs(paint.focal_point_ratio)>1)throw new Error('Gradient focal point must be from -1 through 1.');
    if(!Array.isArray(paint.stops)||paint.stops.length<1||paint.stops.length>15)throw new Error('Gradient requires 1 through 15 color stops.');
    let ratio=-1;for(const stop of paint.stops){if(!Number.isInteger(stop.ratio)||stop.ratio<0||stop.ratio<ratio||stop.ratio>255||!Array.isArray(stop.color)||stop.color.length!==4||!stop.color.every(c=>Number.isInteger(c)&&c>=0&&c<=255))throw new Error('Invalid runtime gradient color stop.');ratio=stop.ratio;}
    return paint;
  }
  function gradientFor(context,paint,area=null) {
    validateGradient(paint);
    const mode=spreadModeOf(paint),base=unitStops(paint);
    if(mode==='extend'||!area){
      const gradient=paint.kind==='linear'?context.createLinearGradient(-RADIUS,0,RADIUS,0):context.createRadialGradient(paint.focal_point_ratio*RADIUS,0,0,0,0,RADIUS);
      for(const stop of base)gradient.addColorStop(Math.max(0,Math.min(1,stop.u)),rgba(stop.color));
      return gradient;
    }
    if(paint.kind==='linear'){
      const uMin=Math.max(-64,Math.floor((area.left/RADIUS+1)/2)),uMax=Math.min(64,Math.max(uMin+1,Math.ceil((area.right/RADIUS+1)/2)));
      const span=uMax-uMin,eps=1e-5/span;
      const gradient=context.createLinearGradient((2*uMin-1)*RADIUS,0,(2*uMax-1)*RADIUS,0);
      for(let k=uMin;k<uMax;k++){
        const rev=mode==='reflect'&&(((k%2)+2)%2===1);
        const list=rev?[...base].reverse().map(s=>({u:1-s.u,color:s.color})):base;
        for(let si=0;si<list.length;si++){
          let pos=(k-uMin+list[si].u)/span;
          if(mode==='repeat'){
            if(si===0&&k>uMin)pos+=eps;
            else if(si===list.length-1&&k+1<uMax)pos-=eps;
          }
          gradient.addColorStop(Math.max(0,Math.min(1,pos)),rgba(list[si].color));
        }
      }
      return gradient;
    }
    const f=paint.focal_point_ratio,fx=f*RADIUS;
    const maxDist=Math.max(...corners(area).map(([px,py])=>Math.hypot(px-fx,py)));
    const minDenom=Math.max(0.02,1-Math.abs(f))*RADIUS;
    const uMax=Math.min(64,Math.max(1,Math.ceil(maxDist/minDenom))),eps=1e-5/uMax;
    const gradient=context.createRadialGradient(fx,0,0,(1-uMax)*fx,0,uMax*RADIUS);
    for(let k=0;k<uMax;k++){
      const rev=mode==='reflect'&&(k%2===1);
      const list=rev?[...base].reverse().map(s=>({u:1-s.u,color:s.color})):base;
      for(let si=0;si<list.length;si++){
        let pos=(k+list[si].u)/uMax;
        if(mode==='repeat'){
          if(si===0&&k>0)pos+=eps;
          else if(si===list.length-1&&k+1<uMax)pos-=eps;
        }
        gradient.addColorStop(Math.max(0,Math.min(1,pos)),rgba(list[si].color));
      }
    }
    return gradient;
  }
  function meshVisible(mesh) {
    if (!mesh || typeof mesh !== 'object') return false;
    const patchVis = Array.isArray(mesh.patches) && mesh.patches.some(p => Array.isArray(p?.colors) && p.colors.some(c => (c?.[3] || 0) > 0));
    const triVis = Array.isArray(mesh.triangles) && mesh.triangles.some(t => Array.isArray(t?.colors) && t.colors.some(c => (c?.[3] || 0) > 0));
    return patchVis || triVis;
  }
  function evalMeshPatch(patch, u, v, linearRgb) {
    const iu = 1 - u, iv = 1 - v;
    const bu = [iu*iu*iu, 3*u*iu*iu, 3*u*u*iu, u*u*u];
    const bv = [iv*iv*iv, 3*v*iv*iv, 3*v*v*iv, v*v*v];
    let x = 0, y = 0;
    const pts = patch.points;
    for (let r = 0; r < 4; r++) {
      const wr = bv[r];
      for (let c = 0; c < 4; c++) {
        const w = wr * bu[c], pt = pts[r * 4 + c];
        x += w * pt[0]; y += w * pt[1];
      }
    }
    const [c00, c03, c30, c33] = patch.colors;
    const top = interpolateColor(c00, c03, u, linearRgb);
    const bot = interpolateColor(c30, c33, u, linearRgb);
    return { x, y, color: interpolateColor(top, bot, v, linearRgb) };
  }
  function tessellateVectorMesh(mesh) {
    const patches = Array.isArray(mesh?.patches) ? mesh.patches : [];
    const tris = Array.isArray(mesh?.triangles) ? mesh.triangles : [];
    const linearRgb = Boolean(mesh?.linear_rgb);
    const steps = patches.length === 0 ? 1 : patches.length <= 16 ? 12 : patches.length <= 64 ? 8 : 5;
    const out = [];
    for (const patch of patches) {
      if (!Array.isArray(patch?.points) || patch.points.length !== 16 || !Array.isArray(patch?.colors) || patch.colors.length !== 4) continue;
      const grid = [];
      for (let r = 0; r <= steps; r++) {
        const v = r / steps;
        for (let c = 0; c <= steps; c++) grid.push(evalMeshPatch(patch, c / steps, v, linearRgb));
      }
      const stride = steps + 1;
      for (let r = 0; r < steps; r++) {
        for (let c = 0; c < steps; c++) {
          const v00 = grid[r * stride + c], v10 = grid[r * stride + c + 1], v01 = grid[(r + 1) * stride + c], v11 = grid[(r + 1) * stride + c + 1];
          if ((r + c) % 2 === 0) { out.push([v00, v10, v11], [v00, v11, v01]); }
          else { out.push([v00, v10, v01], [v10, v11, v01]); }
        }
      }
    }
    for (const t of tris) {
      if (!Array.isArray(t?.points) || t.points.length !== 3 || !Array.isArray(t?.colors) || t.colors.length !== 3) continue;
      out.push([
        { x: t.points[0][0], y: t.points[0][1], color: t.colors[0] },
        { x: t.points[1][0], y: t.points[1][1], color: t.colors[1] },
        { x: t.points[2][0], y: t.points[2][1], color: t.colors[2] },
      ]);
    }
    return out;
  }
  function paintVisible(kind,channel) {
    if(channel==='fill'&&kind.fill_mesh)return meshVisible(kind.fill_mesh);
    if(channel==='fill'&&kind.fill_bitmap)return kind.fill_bitmap.width>0&&kind.fill_bitmap.height>0;
    if(channel==='stroke'&&kind.stroke_bitmap)return kind.stroke_bitmap.width>0&&kind.stroke_bitmap.height>0;
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
    if(channel==='fill'&&kind.fill_mesh)return meshVisible(kind.fill_mesh)?1:0;
    if(channel==='fill'&&kind.fill_bitmap)return kind.fill_bitmap.width>0&&kind.fill_bitmap.height>0?1:0;
    if(channel==='stroke'&&kind.stroke_bitmap)return kind.stroke_bitmap.width>0&&kind.stroke_bitmap.height>0?1:0;
    const paint=kind[channel+'_gradient'];if(!paint)return(kind[channel]?.[3]||0)/255;validateGradient(paint);
    const rawT=gradientParameter(paint,x,y);if(rawT===null)return 0;
    const t=spreadUnit(spreadModeOf(paint),rawT),ratio=t*255,stops=paint.stops;
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
    const world=multiply(matrix,paint.matrix),inv=inverse(world);
    if(spreadModeOf(paint)!=='extend'){
      if(paint.kind==='radial'&&Math.abs(paint.focal_point_ratio)===1){
        const poly=clipPolygon(corners(bounds),p=>RADIUS-paint.focal_point_ratio*point(inv,...p)[0]);
        return intersect(bounds,box(poly));
      }
      return bounds;
    }
    let result=null;
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
  function matrixOf(context){if(typeof context.getTransform!=='function')return[1,0,0,1,0,0];const m=context.getTransform();return m?[m.a,m.b,m.c,m.d,m.e,m.f]:[1,0,0,1,0,0];}
  function brush(context,paint,width,height) {
    context.transform(...paint.matrix);const inverseBrush=inverse(matrixOf(context)),area=box([[0,0],[width,0],[width,height],[0,height]].map(p=>point(inverseBrush,...p)));
    if(!area)throw new Error('Gradient brush bounds exceed finite rendering limits.');context.fillStyle=gradientFor(context,paint,area);context.fillRect(area.left,area.top,area.right-area.left,area.bottom-area.top);
  }
  function bitmapBrush(context,paint,bitmaps,width,height) {
    if(!bitmaps)throw new Error('Bitmap fill rendering requires the bitmap cache.');
    bitmaps.paint(paint,record=>{
      const m=paint.matrix,det=m[0]*m[3]-m[1]*m[2];
      if(!Number.isFinite(det)||det===0){
        const base=matrixOf(context),baseDet=base[0]*base[3]-base[1]*base[2];if(!Number.isFinite(baseDet)||baseDet===0)return;
        const area=box([[0,0],[width,0],[width,height],[0,height]].map(p=>point(inverse(base),...p)));if(!area)return;
        context.fillStyle=rgba(record.corners[0]);context.fillRect(area.left,area.top,area.right-area.left,area.bottom-area.top);return;
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
      context.drawImage(record.canvas,0,0,w,h);
      if(y0<0)context.drawImage(record.canvas,0,0,w,1,0,y0,w,-y0);
      if(y1>h)context.drawImage(record.canvas,0,h-1,w,1,0,h,w,y1-h);
      if(x0<0)context.drawImage(record.canvas,0,0,1,h,x0,0,-x0,h);
      if(x1>w)context.drawImage(record.canvas,w-1,0,1,h,w,0,x1-w,h);
      if(x0<0&&y0<0){context.fillStyle=rgba(record.corners[0]);context.fillRect(x0,y0,-x0,-y0);}
      if(x1>w&&y0<0){context.fillStyle=rgba(record.corners[1]);context.fillRect(w,y0,x1-w,-y0);}
      if(x0<0&&y1>h){context.fillStyle=rgba(record.corners[2]);context.fillRect(x0,h,-x0,y1-h);}
      if(x1>w&&y1>h){context.fillStyle=rgba(record.corners[3]);context.fillRect(w,h,x1-w,y1-h);}
    });
  }
  function controlBounds(commands,matrix,padding) {
    let bounds=null;for(const c of commands){const points=[];if(c.op!=='close')points.push([c.x,c.y]);if(c.op==='quadratic_to')points.push([c.cx,c.cy]);if(c.op==='cubic_to')points.push([c.cx1,c.cy1],[c.cx2,c.cy2]);if(points.length)bounds=union(bounds,box(points.map(p=>point(matrix,...p))));}
    if(bounds){const dx=padding*Math.hypot(matrix[0],matrix[2])+1,dy=padding*Math.hypot(matrix[1],matrix[3])+1;bounds={left:bounds.left-dx,top:bounds.top-dy,right:bounds.right+dx,bottom:bounds.bottom+dy};}return bounds;
  }
  const lineCapOf = kind => kind.stroke_caps === 'none' ? 'butt' : kind.stroke_caps === 'square' ? 'square' : 'round';
  const lineJoinOf = kind => kind.stroke_joints === 'bevel' ? 'bevel' : kind.stroke_joints === 'miter' ? 'miter' : 'round';
  const miterLimitOf = kind => Number.isFinite(kind.stroke_miter_limit) && kind.stroke_miter_limit >= 1 ? kind.stroke_miter_limit : 3;
  const needsScreenStroke = kind => Boolean(kind.stroke_hairline) || (Boolean(kind.stroke_scale_mode) && kind.stroke_scale_mode !== 'normal') || Boolean(kind.stroke_pixel_hinting);
  function strokeScreenGeometry(kind, matrix, offsetX = 0, offsetY = 0) {
    const sx = Math.hypot(matrix[0], matrix[1]), sy = Math.hypot(matrix[2], matrix[3]);
    const scaleMode = kind.stroke_scale_mode || 'normal', hairline = Boolean(kind.stroke_hairline);
    const hinting = Boolean(kind.stroke_pixel_hinting) && Math.abs(matrix[1]) < 1e-6 && Math.abs(matrix[2]) < 1e-6;
    if (!hairline && scaleMode === 'normal' && !hinting) return null;
    const screenW = hairline ? 1 : scaleMode === 'none' ? kind.stroke_width : scaleMode === 'horizontal' ? kind.stroke_width * sx : scaleMode === 'vertical' ? kind.stroke_width * sy : kind.stroke_width * Math.sqrt((sx * sx + sy * sy) / 2);
    if (!(screenW > 0)) return null;
    const odd = (Math.round(screenW) % 2) === 1;
    const snap = v => !hinting ? v : odd ? Math.floor(v) + 0.5 : Math.round(v);
    const tx = (x, y) => { const wx = matrix[0] * x + matrix[2] * y + matrix[4], wy = matrix[1] * x + matrix[3] * y + matrix[5]; return [snap(wx) - offsetX, snap(wy) - offsetY]; };
    const p = new Path2D();
    for (const c of kind.commands) {
      if (c.op === 'move_to') { const [px, py] = tx(c.x, c.y); p.moveTo(px, py); }
      else if (c.op === 'line_to') { const [px, py] = tx(c.x, c.y); p.lineTo(px, py); }
      else if (c.op === 'quadratic_to') { const [cx, cy] = tx(c.cx, c.cy), [px, py] = tx(c.x, c.y); p.quadraticCurveTo(cx, cy, px, py); }
      else if (c.op === 'cubic_to') { const [c1x, c1y] = tx(c.cx1, c.cy1), [c2x, c2y] = tx(c.cx2, c.cy2), [px, py] = tx(c.x, c.y); p.bezierCurveTo(c1x, c1y, c2x, c2y, px, py); }
      else if (c.op === 'close') p.closePath();
    }
    return { path: p, lineWidth: screenW };
  }
  class GradientRenderer {
    constructor({maxMaskPixels=16*1024*1024,createCanvas}={}) {
      if(!Number.isSafeInteger(maxMaskPixels)||maxMaskPixels<1)throw new Error('Gradient mask pixel budget must be a positive safe integer.');this.maxMaskPixels=maxMaskPixels;this.surface=null;
      this.createCanvas=createCanvas||(()=>typeof OffscreenCanvas==='function'?new OffscreenCanvas(1,1):global.document.createElement('canvas'));
    }
    clear(){if(this.surface){this.surface.width=1;this.surface.height=1;this.surface=null;}if(this.meshSurface){this.meshSurface.width=1;this.meshSurface.height=1;this.meshSurface=null;}}
    paintMeshFill(context,mesh,canvasW,canvasH) {
      const tris=tessellateVectorMesh(mesh);if(!tris.length)return;
      const m=matrixOf(context),det=m[0]*m[3]-m[1]*m[2];if(!Number.isFinite(det)||det===0)return;
      let minX=Infinity,minY=Infinity,maxX=-Infinity,maxY=-Infinity;
      const screenTris=[];
      for(const [v0,v1,v2] of tris){
        const p0=point(m,v0.x,v0.y),p1=point(m,v1.x,v1.y),p2=point(m,v2.x,v2.y);
        if(![...p0,...p1,...p2].every(Number.isFinite))continue;
        minX=Math.min(minX,p0[0],p1[0],p2[0]);minY=Math.min(minY,p0[1],p1[1],p2[1]);
        maxX=Math.max(maxX,p0[0],p1[0],p2[0]);maxY=Math.max(maxY,p0[1],p1[1],p2[1]);
        screenTris.push([p0,v0.color,p1,v1.color,p2,v2.color]);
      }
      if(!screenTris.length||maxX<=0||maxY<=0||minX>=canvasW||minY>=canvasH)return;
      const left=Math.max(0,Math.floor(minX)-1),top=Math.max(0,Math.floor(minY)-1);
      const right=Math.min(canvasW,Math.ceil(maxX)+1),bottom=Math.min(canvasH,Math.ceil(maxY)+1);
      const bw=right-left,bh=bottom-top;if(!(bw>0&&bh>0))return;
      let scratch=null,mctx=null;
      if(bw*bh<=this.maxMaskPixels){
        try{
          scratch=this.meshSurface||(this.meshSurface=this.createCanvas());
          if(scratch.width!==bw||scratch.height!==bh){scratch.width=1;scratch.height=bh;scratch.width=bw;}
          mctx=scratch.getContext('2d');
        }catch(_){mctx=null;}
      }
      if(mctx&&typeof mctx.createImageData==='function'&&typeof mctx.putImageData==='function'&&typeof context.drawImage==='function'){
        const img=mctx.createImageData(bw,bh),data=img.data;
        for(const [p0,c0,p1,c1,p2,c2] of screenTris){
          const ax=p0[0]-left,ay=p0[1]-top,bx=p1[0]-left,by=p1[1]-top,cx=p2[0]-left,cy=p2[1]-top;
          const denom=(by-cy)*(ax-cx)+(cx-bx)*(ay-cy);if(Math.abs(denom)<1e-6)continue;
          const inv=1/denom;
          const x0=Math.max(0,Math.floor(Math.min(ax,bx,cx))),y0=Math.max(0,Math.floor(Math.min(ay,by,cy)));
          const x1=Math.min(bw-1,Math.ceil(Math.max(ax,bx,cx))),y1=Math.min(bh-1,Math.ceil(Math.max(ay,by,cy)));
          for(let py=y0;py<=y1;py++){
            const sy=py+0.5,rowOff=py*bw;
            for(let px=x0;px<=x1;px++){
              const sx=px+0.5;
              const w0=((by-cy)*(sx-cx)+(cx-bx)*(sy-cy))*inv;
              const w1=((cy-ay)*(sx-cx)+(ax-cx)*(sy-cy))*inv;
              const w2=1-w0-w1;
              if(w0<-1e-4||w1<-1e-4||w2<-1e-4)continue;
              const cw0=Math.max(0,Math.min(1,w0)),cw1=Math.max(0,Math.min(1,w1)),cw2=Math.max(0,Math.min(1,1-cw0-cw1));
              const idx=(rowOff+px)*4;
              const sr=Math.round(cw0*c0[0]+cw1*c1[0]+cw2*c2[0]);
              const sg=Math.round(cw0*c0[1]+cw1*c1[1]+cw2*c2[1]);
              const sb=Math.round(cw0*c0[2]+cw1*c1[2]+cw2*c2[2]);
              const sa=Math.round(cw0*c0[3]+cw1*c1[3]+cw2*c2[3]);
              if(sa<=0)continue;
              const da=data[idx+3];
              if(da===0||sa>=255){
                data[idx]=sr;data[idx+1]=sg;data[idx+2]=sb;data[idx+3]=sa;
              }else{
                const saF=sa/255,daF=(da/255)*(1-saF),outAF=saF+daF;
                if(outAF>1e-6){
                  data[idx]=Math.round((sr*saF+data[idx]*daF)/outAF);
                  data[idx+1]=Math.round((sg*saF+data[idx+1]*daF)/outAF);
                  data[idx+2]=Math.round((sb*saF+data[idx+2]*daF)/outAF);
                  data[idx+3]=Math.round(outAF*255);
                }
              }
            }
          }
        }
        mctx.putImageData(img,0,0);
        context.setTransform(1,0,0,1,0,0);
        context.drawImage(scratch,left,top);
        return;
      }
      for(const [v0,v1,v2] of tris){
        const avg=[
          Math.round((v0.color[0]+v1.color[0]+v2.color[0])/3),
          Math.round((v0.color[1]+v1.color[1]+v2.color[1])/3),
          Math.round((v0.color[2]+v1.color[2]+v2.color[2])/3),
          Math.round((v0.color[3]+v1.color[3]+v2.color[3])/3),
        ];
        context.beginPath();context.moveTo(v0.x,v0.y);context.lineTo(v1.x,v1.y);context.lineTo(v2.x,v2.y);context.closePath();
        context.fillStyle=rgba(avg);context.fill();
      }
    }
    draw(context,kind,path,bitmaps=null) {
      if(kind.fill_mesh||kind.fill_gradient||kind.stroke_gradient||kind.fill_bitmap||kind.stroke_bitmap){const m=matrixOf(context),det=m[0]*m[3]-m[1]*m[2];if(!Number.isFinite(det)||det===0)return;}
      const hasStroke = kind.stroke_width > 0 || Boolean(kind.stroke_hairline);
      for(const channel of ['fill','stroke']){
        if(!paintVisible(kind,channel)||(channel==='stroke'&&!hasStroke))continue;
        const paint=kind[channel+'_gradient'],bitmap=channel==='fill'?kind.fill_bitmap:kind.stroke_bitmap;context.save();
        try{
          if(channel==='stroke'){context.lineWidth=kind.stroke_hairline?1:kind.stroke_width;context.lineCap=lineCapOf(kind);context.lineJoin=lineJoinOf(kind);context.miterLimit=miterLimitOf(kind);}
          if(bitmap&&channel==='fill'){context.clip(path,kind.fill_rule||'nonzero');bitmapBrush(context,bitmap,bitmaps,context.canvas.width,context.canvas.height);}
          else if(bitmap)this.stroke(context,kind,path,bitmap,bitmaps);
          else if(channel==='fill'&&kind.fill_mesh){context.clip(path,kind.fill_rule||'nonzero');this.paintMeshFill(context,kind.fill_mesh,context.canvas.width,context.canvas.height);}
          else if(!paint){
            context[channel+'Style']=rgba(kind[channel]);
            if(channel==='fill')context.fill(path,kind.fill_rule||'nonzero');
            else {
              const sg=needsScreenStroke(kind)?strokeScreenGeometry(kind,matrixOf(context),0,0):null;
              if(sg){context.setTransform(1,0,0,1,0,0);context.lineWidth=sg.lineWidth;context.stroke(sg.path);}
              else context.stroke(path);
            }
          }
          else if(channel==='fill'){context.clip(path,kind.fill_rule||'nonzero');brush(context,paint,context.canvas.width,context.canvas.height);}
          else this.stroke(context,kind,path,paint);
        }finally{context.restore();}
      }
    }
    stroke(context,kind,path,paint,bitmaps=null) {
      const matrix=matrixOf(context),sx=Math.hypot(matrix[0],matrix[1]),sy=Math.hypot(matrix[2],matrix[3]),avgScale=Math.max(1e-6,Math.sqrt((sx*sx+sy*sy)/2));
      const padLocal=(kind.stroke_hairline?1/avgScale:kind.stroke_scale_mode==='none'?kind.stroke_width/avgScale:kind.stroke_width)*Math.max(1,miterLimitOf(kind)/2)+1;
      const raw=controlBounds(kind.commands,matrix,padLocal),bounds=intersect(raw,{left:0,top:0,right:context.canvas.width,bottom:context.canvas.height});if(!bounds)return;
      const left=Math.floor(bounds.left),top=Math.floor(bounds.top),width=Math.ceil(bounds.right)-left,height=Math.ceil(bounds.bottom)-top;
      if(width*height>this.maxMaskPixels)throw new RangeError('Gradient stroke mask exceeds 16777216 pixels; reduce publication dimensions or stroke coverage.');
      const canvas=this.surface||(this.surface=this.createCanvas());if(canvas.width!==width||canvas.height!==height){canvas.width=1;canvas.height=height;canvas.width=width;}
      const mask=canvas.getContext('2d');if(!mask)throw new Error('Canvas 2D gradient stroke mask is unavailable.');
      mask.setTransform(1,0,0,1,0,0);mask.globalAlpha=1;mask.globalCompositeOperation='source-over';mask.clearRect(0,0,width,height);
      mask.strokeStyle='#fff';mask.lineCap=lineCapOf(kind);mask.lineJoin=lineJoinOf(kind);mask.miterLimit=miterLimitOf(kind);
      const sg=strokeScreenGeometry(kind,matrix,left,top);
      if(sg){mask.setTransform(1,0,0,1,0,0);mask.lineWidth=sg.lineWidth;mask.stroke(sg.path);}
      else{mask.setTransform(matrix[0],matrix[1],matrix[2],matrix[3],matrix[4]-left,matrix[5]-top);mask.lineWidth=kind.stroke_width;mask.stroke(path);}
      mask.setTransform(matrix[0],matrix[1],matrix[2],matrix[3],matrix[4]-left,matrix[5]-top);
      mask.globalCompositeOperation='source-in';
      if(bitmaps)bitmapBrush(mask,paint,bitmaps,width,height);else brush(mask,paint,width,height);
      mask.globalCompositeOperation='source-over';
      context.setTransform(1,0,0,1,0,0);context.drawImage(canvas,left,top);
    }
  }

  function clipTable(clips=[], filterGroups=[]) {
    const alphaByClipId=new Map();
    if(Array.isArray(filterGroups)){
      for(const group of filterGroups){
        if(Array.isArray(group?.alpha_masks)){
          for(const am of group.alpha_masks){
            if(typeof am?.clip_id==='string'&&Array.isArray(am.components)){
              alphaByClipId.set(am.clip_id,am.components);
            }
          }
        }
      }
    }
    const result=new Map();
    for(const clip of clips){
      if(typeof clip.id!=='string'||!Array.isArray(clip.components)||result.has(clip.id))throw new Error('Invalid runtime clip table.');
      const alphaComponents=Array.isArray(clip.alpha_components)?clip.alpha_components:alphaByClipId.get(clip.id);
      result.set(clip.id,alphaComponents?{...clip,components:alphaComponents}:clip);
    }
    return result;
  }
  function clipGroups(ids,table) {
    return [...new Set(ids||[])].map(id=>{const clip=table.get(id);if(!clip)throw new Error(`Runtime clip '${id}' is missing; re-export this project.`);return clip;});
  }
  function clipsContain(context,ids,table,x,y,cache,bitmaps=null) {
    const hasPath2D=Boolean(context&&typeof context.isPointInPath==='function'&&cache);
    if(hasPath2D)context.save();
    try{
      if(hasPath2D)context.setTransform(1,0,0,1,0,0);
      const containsGroup=(clip,ancestors)=>{
        if(ancestors.has(clip.id)||ancestors.size>=32)throw new Error('Runtime mask dependencies are cyclic or exceed 32 levels.');
        const next=new Set(ancestors);next.add(clip.id);
        const hit=clip.components.some(component=>{
          if(typeof component.alpha==='number'&&component.alpha<=0)return false;
          const m=component.matrix,det=m[0]*m[3]-m[1]*m[2];if(!Number.isFinite(det)||det===0)return false;
          const p=point(inverse(m),x,y);
          if(component.fill_gradient&&paintAlpha(component,'fill',p[0],p[1])<=0)return false;
          const inGeom=hasPath2D
            ?context.isPointInPath(cache.get(component.commands),p[0],p[1],component.fill_rule||'nonzero')
            :containsFallback({...component,fill:[255,255,255,255],stroke_width:0},p[0],p[1]);
          if(!inGeom)return false;
          if(component.fill_bitmap&&bitmaps&&typeof bitmaps.hit==='function'){
            const bm=component.fill_bitmap.matrix||[1,0,0,1,0,0],bdet=bm[0]*bm[3]-bm[1]*bm[2];
            if(!Number.isFinite(bdet)||bdet===0)return false;
            const bp=point(inverse(bm),p[0],p[1]),bw=component.fill_bitmap.width,bh=component.fill_bitmap.height;
            if(!(bw>0&&bh>0))return false;
            const bx=component.fill_bitmap.repeat?(((bp[0]%bw)+bw)%bw):Math.max(0,Math.min(bw-1e-6,bp[0]));
            const by=component.fill_bitmap.repeat?(((bp[1]%bh)+bh)%bh):Math.max(0,Math.min(bh-1e-6,bp[1]));
            if(!bitmaps.hit(component.fill_bitmap,bx,by))return false;
          }
          return clipGroups(component.clip_ids,table).every(child=>containsGroup(child,next));
        });
        return clip.invert?!hit:hit;
      };
      return clipGroups(ids,table).every(clip=>containsGroup(clip,new Set()));
    }finally{if(hasPath2D)context.restore();}
  }
  // Each clip is a UNION of independently filled components. Combining their
  // commands into one Path2D changes overlap/winding semantics. Nested clips are
  // intersections. Source masks are composed recursively before their component
  // joins the union, using a bounded pool of reusable Canvas surfaces.
  class ClipRenderer {
    constructor({maxSurfacePixels=16*1024*1024,maxScratchPixels=32*1024*1024,maxReadPixels=16*1024*1024,maxReadTilePixels=262144,createCanvas}={}) {
      for(const value of [maxSurfacePixels,maxScratchPixels,maxReadPixels,maxReadTilePixels])if(!Number.isSafeInteger(value)||value<1)throw new Error('Clipping budgets must be positive safe integers.');
      Object.assign(this,{maxSurfacePixels,maxScratchPixels,maxReadPixels,maxReadTilePixels});this.surfaces=[];this.readPixels=0;
      this.createCanvas=createCanvas||(()=>typeof OffscreenCanvas==='function'?new OffscreenCanvas(1,1):global.document.createElement('canvas'));
    }
    beginFrame(){this.readPixels=0;}
    clear(){for(const surface of this.surfaces){surface.width=1;surface.height=1;}this.surfaces=[];this.readPixels=0;}
    surface(index,width,height){
      const retained=()=>this.surfaces.reduce((sum,s,i)=>sum+(i===index?0:s.width*s.height),width*height);
      if(retained()>this.maxScratchPixels){
        // Deeper surfaces are inactive when a shallower stack slot is reused.
        for(let i=index+1;i<this.surfaces.length;i++){this.surfaces[i].width=1;this.surfaces[i].height=1;}
        if(retained()>this.maxScratchPixels)throw new RangeError('Nested mask scratch surfaces exceed the pixel budget; reduce publication dimensions or mask nesting.');
      }
      const surface=this.surfaces[index]||(this.surfaces[index]=this.createCanvas());
      if(surface.width!==width||surface.height!==height){surface.width=1;surface.height=height;surface.width=width;}
      const ctx=surface.getContext('2d');if(!ctx)throw new Error('Canvas 2D clipping surface is unavailable.');
      ctx.setTransform(1,0,0,1,0,0);ctx.globalAlpha=1;ctx.globalCompositeOperation='source-over';ctx.clearRect(0,0,width,height);return ctx;
    }
    maskGroup(group,table,cache,shifted,width,height,bitmaps,index,ancestors) {
      if(ancestors.has(group.id)||ancestors.size>=32)throw new Error('Runtime mask dependencies are cyclic or exceed 32 levels.');
      const next=new Set(ancestors);next.add(group.id);
      const mask=this.surface(index,width,height);mask.fillStyle='#fff';
      for(const component of group.components){
        const m=multiply(shifted,component.matrix),d=m[0]*m[3]-m[1]*m[2];if(!Number.isFinite(d)||d===0)continue;
        const dependencies=clipGroups(component.clip_ids,table);
        const needsIsolated=dependencies.length>0||Boolean(component.luminance&&(component.fill_gradient||component.fill_bitmap));
        const source=needsIsolated?this.surface(index+1,width,height):mask;
        source.save();
        try{
          source.setTransform(...m);source.fillStyle='#fff';
          if(typeof component.alpha==='number')source.globalAlpha=Math.max(0,Math.min(1,component.alpha));
          const p=cache.get(component.commands),rule=component.fill_rule||'nonzero';
          if(component.fill_gradient){
            const grad=component.luminance?{...component.fill_gradient,stops:component.fill_gradient.stops.map(s=>({...s,color:[255,255,255,Math.round(s.color[3]*(0.2126*s.color[0]+0.7152*s.color[1]+0.0722*s.color[2])/255)]}))}:component.fill_gradient;
            source.clip(p,rule);brush(source,grad,width,height);
          }
          else if(component.fill_bitmap){source.clip(p,rule);bitmapBrush(source,component.fill_bitmap,bitmaps,width,height);}
          else if(component.resource_bitmap)throw new Error('Runtime mask bitmap was not decoded by the shared core; re-export this project.');
          else source.fill(p,rule);
        }finally{source.restore();}
        if(component.luminance&&component.fill_bitmap&&typeof source.getImageData==='function'&&typeof source.putImageData==='function'){
          const img=source.getImageData(0,0,width,height),d=img.data;
          for(let i=0;i<d.length;i+=4){if(d[i+3])d[i+3]=Math.round(d[i+3]*(0.2126*d[i]+0.7152*d[i+1]+0.0722*d[i+2])/255);}
          source.setTransform(1,0,0,1,0,0);source.putImageData(img,0,0);
        }
        for(const dependency of dependencies){
          const nested=this.maskGroup(dependency,table,cache,shifted,width,height,bitmaps,index+2,next);
          source.setTransform(1,0,0,1,0,0);source.globalAlpha=1;source.globalCompositeOperation='destination-in';source.drawImage(nested.canvas,0,0);
        }
        if(needsIsolated){source.globalCompositeOperation='source-over';mask.drawImage(source.canvas,0,0);}
      }
      if(group.invert){
        mask.setTransform(1,0,0,1,0,0);mask.globalAlpha=1;mask.fillStyle='#fff';mask.globalCompositeOperation='source-out';mask.fillRect(0,0,width,height);mask.globalCompositeOperation='source-over';
      }
      return mask;
    }
    draw(context,item,table,cache,draw,{bounds=null,readBounds=false,bitmaps=null}={}) {
      const groups=clipGroups(item.clip_ids,table);if(!groups.length){draw(context);return undefined;}if(groups.some(g=>!g.components.length&&!g.invert))return null;
      const base=matrixOf(context),det=base[0]*base[3]-base[1]*base[2];if(!Number.isFinite(det)||det===0)return null;
      const viewport={left:0,top:0,right:context.canvas.width,bottom:context.canvas.height};
      let area=bounds?intersect(box(corners(bounds).map(p=>point(base,...p))),viewport):viewport;
      // Control hulls are used only to bound scratch allocation. Actual clipping
      // and semantic coverage below still use filled curves and their holes.
      for(const group of groups){if(group.invert)continue;let extent=null;for(const c of group.components)extent=union(extent,controlBounds(c.commands,multiply(base,c.matrix),0));area=intersect(area,extent);if(!area)return null;}
      const left=Math.max(0,Math.floor(area.left)-1),top=Math.max(0,Math.floor(area.top)-1),right=Math.min(context.canvas.width,Math.ceil(area.right)+1),bottom=Math.min(context.canvas.height,Math.ceil(area.bottom)+1),width=right-left,height=bottom-top;
      if(!(width>0&&height>0))return null;
      if(width*height>this.maxSurfacePixels)throw new RangeError('Clipping surface exceeds 16777216 pixels; reduce publication dimensions or clipped coverage.');
      if(readBounds&&this.readPixels+width*height>this.maxReadPixels)throw new RangeError('Clipped semantic coverage exceeds the per-frame readback budget; reduce semantic clipped coverage.');
      const content=this.surface(0,width,height),shifted=[base[0],base[1],base[2],base[3],base[4]-left,base[5]-top];content.setTransform(...shifted);draw(content);
      for(const group of groups){
        const mask=this.maskGroup(group,table,cache,shifted,width,height,bitmaps,1,new Set());
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

  function flattenSubpaths(commands) {
    const subs = []; let cur = null, start = [0, 0], last = [0, 0];
    const ensure = () => { if (!cur) { cur = { pts: [[last[0], last[1]]], closed: false }; subs.push(cur); } };
    for (const c of commands || []) {
      if (c.op === 'move_to') { start = [c.x, c.y]; last = [c.x, c.y]; cur = { pts: [[c.x, c.y]], closed: false }; subs.push(cur); }
      else if (c.op === 'line_to') { ensure(); last = [c.x, c.y]; cur.pts.push(last); }
      else if (c.op === 'quadratic_to') {
        ensure(); const [x0, y0] = last;
        for (let i = 1; i <= 16; i++) { const t = i / 16, u = 1 - t; cur.pts.push([u*u*x0 + 2*u*t*c.cx + t*t*c.x, u*u*y0 + 2*u*t*c.cy + t*t*c.y]); }
        last = [c.x, c.y];
      } else if (c.op === 'cubic_to') {
        ensure(); const [x0, y0] = last;
        for (let i = 1; i <= 24; i++) { const t = i / 24, u = 1 - t; cur.pts.push([u*u*u*x0 + 3*u*u*t*c.cx1 + 3*u*t*t*c.cx2 + t*t*t*c.x, u*u*u*y0 + 3*u*u*t*c.cy1 + 3*u*t*t*c.cy2 + t*t*t*c.y]); }
        last = [c.x, c.y];
      } else if (c.op === 'close') {
        if (cur) { cur.pts.push([start[0], start[1]]); cur.closed = true; }
        last = [start[0], start[1]]; cur = null;
      }
    }
    return subs;
  }

  function effectiveStrokeWidth(kind, worldMatrix) {
    let effW = kind.stroke_hairline ? 1 : kind.stroke_width;
    if (worldMatrix) {
      const sx = Math.hypot(worldMatrix[0], worldMatrix[1]), sy = Math.hypot(worldMatrix[2], worldMatrix[3]);
      const avg = Math.max(1e-6, Math.sqrt((sx * sx + sy * sy) / 2)), sm = kind.stroke_scale_mode || 'normal';
      if (kind.stroke_hairline) effW = 1 / avg;
      else if (sm === 'none') effW = kind.stroke_width / avg;
      else if (sm === 'horizontal') effW = kind.stroke_width * sx / avg;
      else if (sm === 'vertical') effW = kind.stroke_width * sy / avg;
    }
    return effW;
  }

  function containsFallback(kind, x, y, worldMatrix = null) {
    const subs = flattenSubpaths(kind.commands);
    if (paintAlpha(kind, 'fill', x, y) > 0) {
      let winding = 0, crossings = 0;
      for (const sub of subs) {
        const pts = sub.pts; if (pts.length < 3) continue;
        for (let i = 0; i < pts.length; i++) {
          const [x0, y0] = pts[i], [x1, y1] = pts[(i + 1) % pts.length];
          if ((y0 <= y && y1 > y) || (y1 <= y && y0 > y)) {
            const vt = (y - y0) / (y1 - y0);
            if (x < x0 + vt * (x1 - x0)) { crossings++; winding += y1 > y0 ? 1 : -1; }
          }
        }
      }
      if (kind.fill_rule === 'evenodd' ? (crossings & 1) !== 0 : winding !== 0) return true;
    }
    const hasStroke = kind.stroke_width > 0 || Boolean(kind.stroke_hairline);
    if (!hasStroke || paintAlpha(kind, 'stroke', x, y) <= 0) return false;
    const half = effectiveStrokeWidth(kind, worldMatrix) * 0.5, cap = lineCapOf(kind);
    for (const sub of subs) {
      const pts = sub.pts;
      for (let i = 0; i + 1 < pts.length; i++) {
        const [ax, ay] = pts[i], [bx, by] = pts[i + 1], dx = bx - ax, dy = by - ay, len = Math.hypot(dx, dy);
        if (len <= 1e-9) { if (cap !== 'butt' && Math.hypot(x - ax, y - ay) <= half) return true; continue; }
        const ux = dx / len, uy = dy / len, proj = (x - ax) * ux + (y - ay) * uy, perp = Math.abs((x - ax) * uy - (y - ay) * ux);
        const lo = (!sub.closed && i === 0 && cap === 'square') ? -half : 0;
        const hi = (!sub.closed && i + 2 === pts.length && cap === 'square') ? len + half : len;
        if (proj >= lo && proj <= hi && perp <= half) return true;
        if (cap === 'round' && ((proj < 0 && (!sub.closed && i === 0) && Math.hypot(x - ax, y - ay) <= half) || (proj > len && (!sub.closed && i + 2 === pts.length) && Math.hypot(x - bx, y - by) <= half))) return true;
        if (lineJoinOf(kind) === 'round' && i > 0 && Math.hypot(x - ax, y - ay) <= half) return true;
      }
    }
    return false;
  }

  function contains(context, kind, x, y, path = null, worldMatrix = null) {
    if (!context || typeof context.isPointInPath !== 'function') return containsFallback(kind, x, y, worldMatrix);
    path ??= createPath(kind.commands);
    context.save();
    try {
      // The caller already applied the inverse composed instance transform to the point.
      // DPR and the last rendered object's transform must not affect local geometry tests.
      context.setTransform(1, 0, 0, 1, 0, 0);
      if (paintAlpha(kind,'fill',x,y)>0 && context.isPointInPath(path, x, y, kind.fill_rule || 'nonzero')) return true;
      const hasStroke = kind.stroke_width > 0 || Boolean(kind.stroke_hairline);
      if (!hasStroke || paintAlpha(kind,'stroke',x,y)<=0) return false;
      context.lineWidth = effectiveStrokeWidth(kind, worldMatrix);
      context.lineCap = lineCapOf(kind);
      context.lineJoin = lineJoinOf(kind);
      context.miterLimit = miterLimitOf(kind);
      return context.isPointInStroke(path, x, y);
    } finally {
      context.restore();
    }
  }

  const api = { createPath, PathCache, contains, paintVisible, paintAlpha, visibleBounds, GradientRenderer, clipTable, clipsContain, ClipRenderer };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else global.NeoFlashPaths = api;
})(globalThis);
