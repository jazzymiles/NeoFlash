/* Authored semantics. DOM targets describe artwork; Canvas still owns pointer hit testing. */
(function (global) {
  'use strict';
  const point = (m, x, y) => [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
  function union(a, b) {
    if (!a) return b; if (!b) return a;
    return { left: Math.min(a.left, b.left), top: Math.min(a.top, b.top), right: Math.max(a.right, b.right), bottom: Math.max(a.bottom, b.bottom) };
  }
  function fromPoints(points) {
    if (!points.length || points.some(p => !p.every(Number.isFinite))) return null;
    return { left: Math.min(...points.map(p => p[0])), top: Math.min(...points.map(p => p[1])), right: Math.max(...points.map(p => p[0])), bottom: Math.max(...points.map(p => p[1])) };
  }
  function cubicAt(a, b, c, d, t) { const u = 1 - t; return u*u*u*a + 3*u*u*t*b + 3*u*t*t*c + t*t*t*d; }
  function roots(a, b, c) {
    if (Math.abs(a) < 1e-12) return Math.abs(b) < 1e-12 ? [] : [-c / b];
    const discriminant = b*b - 4*a*c;
    if (discriminant < 0) return [];
    const root = Math.sqrt(discriminant);
    return [(-b + root) / (2*a), (-b - root) / (2*a)];
  }
  function pathBounds(commands, matrix) {
    let current = null, start = null, bounds = null;
    const include = p => { bounds = union(bounds, { left:p[0], top:p[1], right:p[0], bottom:p[1] }); };
    for (const op of commands) {
      if (op.op === 'move_to') { current = point(matrix, op.x, op.y); start = current; }
      else if (op.op === 'close') { if (current && start) { include(current); include(start); current = start; } }
      else if (current) {
        const end = point(matrix, op.x, op.y); include(current); include(end);
        if (op.op === 'quadratic_to') {
          const control = point(matrix, op.cx, op.cy);
          for (let axis=0; axis<2; axis++) {
            const denominator = current[axis] - 2*control[axis] + end[axis];
            const t = (current[axis] - control[axis]) / denominator;
            if (t>0 && t<1) include([0,1].map(i => (1-t)*(1-t)*current[i] + 2*(1-t)*t*control[i] + t*t*end[i]));
          }
        } else if (op.op === 'cubic_to') {
          const c1=point(matrix,op.cx1,op.cy1), c2=point(matrix,op.cx2,op.cy2);
          for (let axis=0; axis<2; axis++) {
            for (const t of roots(-current[axis]+3*c1[axis]-3*c2[axis]+end[axis],2*(current[axis]-2*c1[axis]+c2[axis]),c1[axis]-current[axis])) {
              if (t>0 && t<1) include([0,1].map(i=>cubicAt(current[i],c1[i],c2[i],end[i],t)));
            }
          }
        }
        current=end;
      }
    }
    return bounds;
  }
  function areaCapable(commands) {
    let origin=null, direction=null;
    const add=(x,y)=>{
      if (!origin) { origin=[x,y]; return false; }
      const dx=x-origin[0],dy=y-origin[1];
      if (!direction) { if(dx!==0||dy!==0)direction=[dx,dy]; return false; }
      const a=direction[0]*dy,b=direction[1]*dx;
      return Math.abs(a-b)>Number.EPSILON*(Math.abs(a)+Math.abs(b))*8;
    };
    for(const op of commands){
      if(op.op==='move_to'){origin=null;direction=null;add(op.x,op.y);}
      else if(op.op==='close'){origin=null;direction=null;}
      else {
        if(op.op==='quadratic_to'&&add(op.cx,op.cy))return true;
        if(op.op==='cubic_to'&&(add(op.cx1,op.cy1)||add(op.cx2,op.cy2)))return true;
        if(add(op.x,op.y))return true;
      }
    }
    return false;
  }
  function itemBounds(item, measureText) {
    if (!(item.alpha > 0)) return null;
    const m=item.transform, kind=item.kind;
    if (!m.every(Number.isFinite) || Math.abs(m[0]*m[3]-m[1]*m[2])<1e-12) return null;
    let bounds=null;
    const stroke=kind.stroke && kind.stroke[3]>0 && kind.stroke_width>0 ? kind.stroke_width/2 : 0;
    if (kind.type==='rectangle' || kind.type==='ellipse' || kind.type==='path') {
      if (!(kind.fill[3]>0) && !stroke) return null;
      if (!stroke && ((kind.type==='rectangle'||kind.type==='ellipse') && (kind.width<=0||kind.height<=0))) return null;
      if (!stroke && kind.type==='path' && !areaCapable(kind.commands)) return null;
      if (kind.type==='rectangle') bounds=fromPoints([[-stroke,-stroke],[kind.width+stroke,-stroke],[kind.width+stroke,kind.height+stroke],[-stroke,kind.height+stroke]].map(p=>point(m,...p)));
      else if (kind.type==='ellipse') {
        const center=point(m,kind.width/2,kind.height/2);
        const rx=Math.hypot(m[0]*kind.width/2,m[2]*kind.height/2), ry=Math.hypot(m[1]*kind.width/2,m[3]*kind.height/2);
        bounds={left:center[0]-rx,top:center[1]-ry,right:center[0]+rx,bottom:center[1]+ry};
      } else bounds=pathBounds(kind.commands,m);
      if (bounds && stroke && kind.type!=='rectangle') {
        const dx=stroke*Math.hypot(m[0],m[2]),dy=stroke*Math.hypot(m[1],m[3]);
        bounds={left:bounds.left-dx,top:bounds.top-dy,right:bounds.right+dx,bottom:bounds.bottom+dy};
      }
    } else if(kind.type==='bitmap_pixels'||kind.type==='bitmap'){
      if(kind.width<=0||kind.height<=0||kind.alpha_bounds===null)return null;
      const box=kind.alpha_bounds||{left:0,top:0,right:kind.width,bottom:kind.height};
      bounds=fromPoints([[box.left,box.top],[box.right,box.top],[box.right,box.bottom],[box.left,box.bottom]].map(p=>point(m,...p)));
    } else if (kind.type==='text' && kind.color[3]>0 && kind.text.trim()) {
      kind.text.split('\n').forEach((line,index)=>{
        if (!line.trim()) return;
        const width=measureText(line,kind), top=index*kind.font_size*1.2;
        bounds=union(bounds,fromPoints([[0,top],[width,top],[width,top+kind.font_size*1.2],[0,top+kind.font_size*1.2]].map(p=>point(m,...p))));
      });
    }
    return bounds && Object.values(bounds).every(Number.isFinite) ? bounds : null;
  }
  const referenceKey=reference=>'ref:'+JSON.stringify(reference);
  function sceneBoundsByPath(scene, width, height, measureText) {
    const result=new Map();
    for (const item of scene) {
      const child=itemBounds(item,measureText);
      if (!child) continue;
      const clipped={left:Math.max(0,child.left),top:Math.max(0,child.top),right:Math.min(width,child.right),bottom:Math.min(height,child.bottom)};
      if (!(clipped.right>clipped.left && clipped.bottom>clipped.top)) continue;
      // Stable object references distinguish identical relative paths in linked
      // symbol namespaces; ancestry comes from the authoritative runtime graph.
      for(const reference of item.ancestry_refs||(item.owner_ref?[item.owner_ref]:[])){
        const key=referenceKey(reference);result.set(key,union(result.get(key),clipped));
      }
      // Legacy path-only projects/tests retain their original lookup contract.
      for (let depth=0; depth<=(item.instance_path||[]).length; depth++) {
        const key=JSON.stringify((item.instance_path||[]).slice(0,depth));
        result.set(key,union(result.get(key),clipped));
      }
    }
    return result;
  }
  function targetBounds(scene, path, width, height, measureText) {
    return sceneBoundsByPath(scene,width,height,measureText).get(JSON.stringify(path)) || null;
  }

  class SemanticLayer {
    constructor(container, onActivate) { this.container=container; this.onActivate=onActivate; this.records=new Map(); this.nextId=1; this.restoringFocus=false; }
    update(nodes, scene, project, scale, measureText) {
      const doc=this.container.ownerDocument, previouslyFocused=doc.activeElement, wanted=[];
      const boundsByPath=sceneBoundsByPath(scene,project.width,project.height,measureText);
      for (const node of nodes) {
        const bounds=boundsByPath.get(node.owner_ref?referenceKey(node.owner_ref):JSON.stringify(node.instance_path));
        if (!bounds) continue;
        const key=node.owner_ref?referenceKey(node.owner_ref):JSON.stringify(node.instance_path)+'\u0000'+node.lifetime_id, metadata=node.accessibility;
        let record=this.records.get(key);
        if (record && record.role!==metadata.role) { record.element.remove(); this.records.delete(key); record=null; }
        if (!record) {
          const element=doc.createElement(metadata.role==='button'?'button':'span');
          element.className='semantic-target';
          if (metadata.role==='button') element.type='button'; else element.setAttribute('role','img');
          const description=doc.createElement('span'); description.hidden=true; description.id='nf-description-'+this.nextId++;
          element.appendChild(description);
          record={element,description,role:metadata.role,node};
          if (metadata.role==='button') {
            // Native button activation supplies Enter/Space exactly once. No key handler.
            element.addEventListener('click',()=>{ if (!element.disabled) this.onActivate(record.node); });
            element.addEventListener('focus',()=>{ if(!this.restoringFocus)element.scrollIntoView({block:'nearest',inline:'nearest'}); });
          }
          this.records.set(key,record);
        }
        record.node=node;
        const element=record.element;
        if (element.getAttribute('aria-label')!==metadata.name) element.setAttribute('aria-label',metadata.name);
        if (metadata.description) { if(record.description.textContent!==metadata.description)record.description.textContent=metadata.description; if(element.getAttribute('aria-describedby')!==record.description.id)element.setAttribute('aria-describedby',record.description.id); }
        else { if(record.description.textContent)record.description.textContent=''; if(element.getAttribute('aria-describedby'))element.removeAttribute('aria-describedby'); }
        if (metadata.role==='button') element.disabled=Boolean(metadata.disabled);
        Object.assign(element.style,{left:bounds.left*scale+'px',top:bounds.top*scale+'px',width:(bounds.right-bounds.left)*scale+'px',height:(bounds.bottom-bounds.top)*scale+'px'});
        wanted.push(record);
      }
      const retained=new Set(wanted);
      for (const [key,record] of this.records) if (!retained.has(record)) { record.element.remove(); this.records.delete(key); }
      let cursor=this.container.firstElementChild;
      for (const record of wanted) {
        if (record.element===cursor) cursor=cursor.nextElementSibling;
        else this.container.insertBefore(record.element,cursor);
      }
      // Reordering can detach a focused node on some engines. Stable buttons retain focus.
      if (previouslyFocused && doc.activeElement!==previouslyFocused && wanted.some(r=>r.element===previouslyFocused && r.role==='button' && !r.element.disabled)) {
        this.restoringFocus=true;
        try { previouslyFocused.focus({preventScroll:true}); } finally { this.restoringFocus=false; }
      }
      return wanted.length;
    }
  }
  const api={itemBounds,targetBounds,sceneBoundsByPath,pathBounds,areaCapable,SemanticLayer};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAccessibility=api;
})(globalThis);
