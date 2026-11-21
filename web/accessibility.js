/* Authored semantics. DOM targets describe artwork; Canvas still owns pointer hit testing. */
(function (global) {
  'use strict';
  const paints=typeof module!=='undefined'&&module.exports?require('./path.js'):global.NeoFlashPaths;
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
    // Filled-mask holes cannot be represented by intersecting bounding boxes.
    // The renderer supplies the actual clipped alpha coverage when needed by a
    // semantic ancestor; null means no remaining visible pixels.
    if(Object.hasOwn(item,'clipped_bounds')){const b=item.clipped_bounds;return b&&Object.values(b).every(Number.isFinite)&&b.right>b.left&&b.bottom>b.top?b:null;}
    const m=item.transform, kind=item.kind;
    if (!m.every(Number.isFinite) || Math.abs(m[0]*m[3]-m[1]*m[2])<1e-12) return null;
    let bounds=null;
    const sx=Math.hypot(m[0],m[1]),sy=Math.hypot(m[2],m[3]),avgScale=Math.max(1e-6,Math.sqrt((sx*sx+sy*sy)/2));
    const miterFactor=kind.stroke_joints==='miter'&&Number.isFinite(kind.stroke_miter_limit)?Math.max(1,kind.stroke_miter_limit/2):1;
    const rawStroke=kind.stroke_hairline?1/avgScale:kind.stroke_scale_mode==='none'?(kind.stroke_width||0)/avgScale:kind.stroke_scale_mode==='horizontal'?(kind.stroke_width||0)*sx/avgScale:kind.stroke_scale_mode==='vertical'?(kind.stroke_width||0)*sy/avgScale:(kind.stroke_width||0);
    const stroke=paints.paintVisible(kind,'stroke') && rawStroke>0 ? (rawStroke/2)*miterFactor : 0;
    if (kind.type==='rectangle' || kind.type==='ellipse' || kind.type==='path' || kind.type==='gradient_path') {
      if (!paints.paintVisible(kind,'fill') && !stroke) return null;
      if (!stroke && ((kind.type==='rectangle'||kind.type==='ellipse') && (kind.width<=0||kind.height<=0))) return null;
      if (!stroke && (kind.type==='path'||kind.type==='gradient_path') && !areaCapable(kind.commands)) return null;
      if (kind.type==='rectangle') bounds=fromPoints([[-stroke,-stroke],[kind.width+stroke,-stroke],[kind.width+stroke,kind.height+stroke],[-stroke,kind.height+stroke]].map(p=>point(m,...p)));
      else if (kind.type==='ellipse') {
        const center=point(m,kind.width/2,kind.height/2);
        const rx=Math.hypot(m[0]*kind.width/2,m[2]*kind.height/2), ry=Math.hypot(m[1]*kind.width/2,m[3]*kind.height/2);
        bounds={left:center[0]-rx,top:center[1]-ry,right:center[0]+rx,bottom:center[1]+ry};
      } else bounds=pathBounds(kind.commands,m);
      if(kind.type==='gradient_path') {
        const fillBounds=areaCapable(kind.commands)?paints.visibleBounds(kind,'fill',bounds,m):null;
        let strokeBounds=null;if(bounds&&stroke){const dx=stroke*Math.hypot(m[0],m[2]),dy=stroke*Math.hypot(m[1],m[3]);strokeBounds=paints.visibleBounds(kind,'stroke',{left:bounds.left-dx,top:bounds.top-dy,right:bounds.right+dx,bottom:bounds.bottom+dy},m);}
        bounds=union(fillBounds,strokeBounds);
      } else if (bounds && stroke && kind.type!=='rectangle') {
        const dx=stroke*Math.hypot(m[0],m[2]),dy=stroke*Math.hypot(m[1],m[3]);
        bounds={left:bounds.left-dx,top:bounds.top-dy,right:bounds.right+dx,bottom:bounds.bottom+dy};
      }
    } else if(kind.type==='bitmap_pixels'||kind.type==='bitmap'||kind.type==='video_frame'){
      if(kind.width<=0||kind.height<=0||kind.alpha_bounds===null)return null;
      const box=kind.alpha_bounds||{left:0,top:0,right:kind.width,bottom:kind.height};
      bounds=fromPoints([[box.left,box.top],[box.right,box.top],[box.right,box.bottom],[box.left,box.bottom]].map(p=>point(m,...p)));
    } else if(kind.type==='text_field'){
      const box=kind.text_bounds;
      if(box&&kind.field.alpha>0)bounds=fromPoints([[box.left,box.top],[box.right,box.top],[box.right,box.bottom],[box.left,box.bottom]].map(p=>point(m,...p)));
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
  function sceneBoundsByPath(scene, width, height, measureText, origin = null) {
    const ox = Number(origin?.x) || 0, oy = Number(origin?.y) || 0;
    const minX = -ox, minY = -oy, maxX = width - ox, maxY = height - oy;
    const result=new Map();
    for (const item of scene) {
      const child=itemBounds(item,measureText);
      if (!child) continue;
      const clipped={left:Math.max(minX,child.left),top:Math.max(minY,child.top),right:Math.min(maxX,child.right),bottom:Math.min(maxY,child.bottom)};
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

  const roleAliases={image:'img',check_box:'checkbox',radio_button:'radio',combo_box:'combobox',list_item:'listitem',menu_item:'menuitem',progress_bar:'progressbar',tab_panel:'tabpanel'};
  function msaaRoleToAria(role) {
    switch (Number(role) >>> 0) {
      case 0x09: case 0x10: return 'region';
      case 0x0a: case 0x14: return 'group';
      case 0x0b: return 'menu';
      case 0x0c: return 'menuitem';
      case 0x0d: return 'tooltip';
      case 0x0e: return 'application';
      case 0x0f: return 'document';
      case 0x12: return 'dialog';
      case 0x15: return 'separator';
      case 0x16: return 'toolbar';
      case 0x17: return 'status';
      case 0x18: return 'table';
      case 0x19: return 'columnheader';
      case 0x1a: return 'rowheader';
      case 0x1c: return 'row';
      case 0x1d: return 'cell';
      case 0x1e: return 'link';
      case 0x21: return 'listbox';
      case 0x22: return 'option';
      case 0x23: return 'tree';
      case 0x24: return 'treeitem';
      case 0x25: return 'tab';
      case 0x26: return 'tabpanel';
      case 0x28: return 'img';
      case 0x29: return 'note';
      case 0x2a: return 'textbox';
      case 0x2b: return 'button';
      case 0x2c: return 'checkbox';
      case 0x2d: return 'radio';
      case 0x2e: return 'combobox';
      case 0x30: return 'progressbar';
      case 0x33: return 'slider';
      case 0x34: return 'spinbutton';
      case 0x3c: return 'tablist';
      default: return 'group';
    }
  }
  function decodeMsaaState(state, roleName = '') {
    const bits = Number(state) >>> 0;
    const r = String(roleName || '').toLowerCase();
    const disabled = (bits & 0x00000001) !== 0;
    const selectedFlag = (bits & 0x00000002) !== 0;
    const focused = (bits & 0x00000004) !== 0;
    const pressed = (bits & 0x00000008) !== 0;
    const checkedFlag = (bits & 0x00000010) !== 0;
    const mixedFlag = (bits & 0x00000020) !== 0;
    const readonly = (bits & 0x00000040) !== 0;
    const expandedFlag = (bits & 0x00000200) !== 0;
    const collapsedFlag = (bits & 0x00000400) !== 0;
    const invisible = (bits & 0x00008000) !== 0;
    const focusable = (bits & 0x00100000) !== 0;
    const selectable = (bits & 0x00200000) !== 0 || ['option', 'tab', 'treeitem', 'row', 'cell'].includes(r);
    return {
      disabled,
      selected: selectedFlag ? true : (selectable ? false : null),
      focused,
      pressed,
      checked: mixedFlag ? 'mixed' : (checkedFlag ? 'true' : (['checkbox', 'radio', 'menuitemcheckbox', 'menuitemradio'].includes(r) ? 'false' : null)),
      readonly,
      expanded: expandedFlag ? true : (collapsedFlag ? false : null),
      invisible,
      focusable,
      selectable,
    };
  }
  function ariaRole(role) {
    const value=String(role || '').trim().toLowerCase();
    return !value || value==='auto' ? null : (Object.hasOwn(roleAliases,value) ? roleAliases[value] : value);
  }
  function attribute(element,name,value) {
    if (value===null) { if(element.getAttribute(name)!==null)element.removeAttribute(name); }
    else if(element.getAttribute(name)!==String(value))element.setAttribute(name,String(value));
  }
  function nonnegativeOrder(value) {
    if(value===undefined || value===null || value==='')return null;
    const order=Number(value);
    return Number.isInteger(order) && order>=0 ? order : null;
  }
  function metadataOrder(metadata) { return nonnegativeOrder(metadata?.tab_order ?? metadata?.tabIndex); }
  const semanticKey=node=>{
    const childSuffix=node.childID!==undefined&&node.childID!==null&&Number(node.childID)>0?'#child:'+Number(node.childID):'';
    return (node.owner_ref?referenceKey(node.owner_ref):JSON.stringify(node.instance_path)+'\u0000'+node.lifetime_id)+childSuffix;
  };

  class SemanticLayer {
    constructor(container, onActivate, options = {}) {
      this.container=container;this.onActivate=onActivate;this.onFocus=options.onFocus;this.onBlur=options.onBlur;
      this.records=new Map();this.elements=new WeakMap();this.nextId=1;this.restoringFocus=false;this.syncingFocus=false;
    }
    eligible(record) { return Boolean(record?.active && record.element.parentNode===this.container && !record.element.disabled && record.element.tabIndex>=0); }
    focusTarget(node) {
      const record=node && this.records.get(semanticKey(node));
      if(!this.eligible(record))return false;
      if(this.container.ownerDocument.activeElement!==record.element){
        this.syncingFocus=true;
        try {record.element.focus({preventScroll:true});} finally {this.syncingFocus=false;}
      }
      return this.container.ownerDocument.activeElement===record.element;
    }
    blurTarget(node) {
      const record=node && this.records.get(semanticKey(node));
      if(record && this.container.ownerDocument.activeElement===record.element){
        this.syncingFocus=true;
        try {record.element.blur?.();} finally {this.syncingFocus=false;}
      }
    }
    retire(record) {
      const focused=this.container.ownerDocument.activeElement===record.element;
      record.active=false;
      if(focused)record.element.blur?.();
      record.element.remove();
      if(focused && !this.syncingFocus)this.onBlur?.(record.node,{relatedTarget:null});
    }
    update(nodes, scene, project, scale, measureText, origin = null) {
      const scaleX=typeof scale==='object'?scale.x:scale,scaleY=typeof scale==='object'?scale.y:scale;
      const originX=Number(origin?.x)||0,originY=Number(origin?.y)||0;
      const doc=this.container.ownerDocument, previouslyFocused=doc.activeElement, wanted=[];
      const boundsByPath=sceneBoundsByPath(scene,project.width,project.height,measureText,origin);
      const collapsedPaths=[], collapsedRefs=new Set();
      for (const node of nodes) {
        if (node && Number(node.childID) > 0) continue;
        const meta=node&&node.accessibility;
        if (!meta) continue;
        if (meta.silent || meta.forceSimple || meta.force_simple) {
          if (Array.isArray(node.instance_path)) collapsedPaths.push(node.instance_path);
          if (node.owner_ref) collapsedRefs.add(referenceKey(node.owner_ref));
        }
      }
      const isSuppressedByAncestor=node=>{
        if (node && Number(node.childID) > 0) return false;
        if (!node.owner_ref && Array.isArray(node.instance_path)) {
          for (const prefix of collapsedPaths) {
            if (node.instance_path.length>prefix.length && prefix.every((seg,idx)=>node.instance_path[idx]===seg)) return true;
          }
        }
        if (collapsedRefs.size && Array.isArray(node.ancestry_refs)) {
          const ownKey=node.owner_ref?referenceKey(node.owner_ref):null;
          for (const ref of node.ancestry_refs) {
            const rk=referenceKey(ref);
            if (rk!==ownKey && collapsedRefs.has(rk)) return true;
          }
        }
        return false;
      };
      const expandedNodes = [];
      for (const node of nodes) {
        expandedNodes.push(node);
        const meta = node && node.accessibility;
        if (meta && !meta.silent && !(meta.forceSimple || meta.force_simple) && Array.isArray(meta.accChildren)) {
          const hostBounds = boundsByPath.get(node.owner_ref ? referenceKey(node.owner_ref) : JSON.stringify(node.instance_path)) || node.explicitBounds || null;
          for (const childSpec of meta.accChildren) {
            if (!childSpec || childSpec.invisible) continue;
            let childBounds = null;
            if (childSpec.rect && Number.isFinite(childSpec.rect.width) && Number.isFinite(childSpec.rect.height) && childSpec.rect.width > 0 && childSpec.rect.height > 0) {
              const baseX = childSpec.rectSpace === 'stage' ? 0 : (hostBounds ? hostBounds.left : 0);
              const baseY = childSpec.rectSpace === 'stage' ? 0 : (hostBounds ? hostBounds.top : 0);
              childBounds = {
                left: baseX + Number(childSpec.rect.x || 0),
                top: baseY + Number(childSpec.rect.y || 0),
                right: baseX + Number(childSpec.rect.x || 0) + Number(childSpec.rect.width),
                bottom: baseY + Number(childSpec.rect.y || 0) + Number(childSpec.rect.height),
              };
            } else if (hostBounds) {
              childBounds = hostBounds;
            }
            expandedNodes.push({
              owner_ref: node.owner_ref,
              instance_path: node.instance_path,
              lifetime_id: node.lifetime_id,
              childID: childSpec.childID,
              explicitBounds: childBounds,
              accessibility: childSpec.accessibility,
            });
          }
        }
      }
      const hostRecordsByBaseKey = new Map();
      for (const node of expandedNodes) {
        const metadata=node&&node.accessibility;
        if (!metadata || metadata.silent || isSuppressedByAncestor(node)) continue;
        const bounds=node.explicitBounds || boundsByPath.get(node.owner_ref?referenceKey(node.owner_ref):JSON.stringify(node.instance_path));
        if (!bounds) continue;
        const key=semanticKey(node);
        let record=this.records.get(key);
        const role=ariaRole(metadata.role);
        const hasAccHooks=Boolean(metadata.accImplHooks);
        const isChildNode=Number(node.childID)>0;
        const activateButton=metadata.activateButton===undefined ? role==='button' : metadata.activateButton===true;
        const useButtonTag=activateButton && !isChildNode;
        if (record && (record.role!==role || record.useButtonTag!==useButtonTag || record.hasAccHooks!==hasAccHooks)) { this.retire(record); this.records.delete(key); record=null; }
        if (!record) {
          const element=doc.createElement(useButtonTag?'button':'span');
          element.className=isChildNode?'semantic-target semantic-child-target':'semantic-target';
          element.id='nf-semantic-'+this.nextId++;
          if (useButtonTag) element.type='button';
          const description=doc.createElement('span'); description.hidden=true; description.id='nf-description-'+this.nextId++;
          element.appendChild(description);
          record={element,description,liveContent:null,role,activateButton,useButtonTag,hasAccHooks,node,active:true};
          const invokeNodeAction=(ev)=>{
            if(!record.active || this.records.get(key)!==record || element.parentNode!==this.container || element.disabled || element.getAttribute('aria-disabled')==='true')return;
            const curMeta=record.node?.accessibility;
            const hooks=curMeta?.accImplHooks;
            if(hooks){
              if(curMeta.selectable){
                const selOp=ev?.shiftKey ? 0x04 : ((ev?.ctrlKey || ev?.metaKey) ? (curMeta.selected === true ? 0x10 : 0x08) : 0x02);
                hooks.onSelect?.(selOp, ev);
              }
              hooks.onDefaultAction?.(ev);
            }
            if(!Number(record.node?.childID) && record.activateButton){
              this.onActivate?.(record.node);
            }
          };
          if (useButtonTag || hasAccHooks) {
            element.addEventListener('click', ev => invokeNodeAction(ev));
          }
          if (!useButtonTag && (hasAccHooks || activateButton)) {
            element.addEventListener('keydown', ev => {
              if (ev?.key === 'Enter' || ev?.key === ' ' || ev?.key === 'Spacebar') {
                ev.preventDefault?.();
                invokeNodeAction(ev);
              }
            });
          }
          element.addEventListener('focus',event=>{
            if(this.syncingFocus || !this.eligible(record))return;
            if(!this.restoringFocus)element.scrollIntoView?.({block:'nearest',inline:'nearest'});
            const hooks=record.node?.accessibility?.accImplHooks;
            hooks?.onSelect?.(0x01, event);
            if(!Number(record.node?.childID))this.onFocus?.(record.node,event);
          });
          element.addEventListener('blur',event=>{
            if(this.syncingFocus || !record.active)return;
            if(this.eligible(this.elements.get(event?.relatedTarget)))return;
            if(!Number(record.node?.childID))this.onBlur?.(record.node,event);
          });
          this.elements.set(element,record);
          this.records.set(key,record);
        }
        record.node=node;
        const element=record.element;
        const isInteractive=useButtonTag || activateButton || hasAccHooks && Boolean(metadata.focusable || metadata.selectable || metadata.defaultAction);
        applyDomAccessibility(element,{...metadata,childID:node.childID},{isInteractive,tabEnabled:node.input?.tab_enabled ?? metadata.tabEnabled,explicitTabIndex:node.input?.tab_index});
        if (!isChildNode) {
          const baseKey = node.owner_ref ? referenceKey(node.owner_ref) : JSON.stringify(node.instance_path) + '\u0000' + node.lifetime_id;
          hostRecordsByBaseKey.set(baseKey, record);
          attribute(element, 'aria-activedescendant', null);
        } else {
          const baseKey = node.owner_ref ? referenceKey(node.owner_ref) : JSON.stringify(node.instance_path) + '\u0000' + node.lifetime_id;
          const hostRec = hostRecordsByBaseKey.get(baseKey);
          if (hostRec && Number(hostRec.node?.accessibility?.activeDescendantChildId) === Number(node.childID)) {
            attribute(hostRec.element, 'aria-activedescendant', element.id);
          }
        }
        // A live region needs changed text content, not only a changed accessible name.
        // The existing semantic-target style keeps this text visually transparent.
        const live=element.getAttribute('aria-live');
        const liveText=live==='polite' || live==='assertive' || (live===null && ['status','alert','log'].includes(role)) ? String(metadata.name || '') : '';
        if(liveText && !record.liveContent){record.liveContent=doc.createElement('span');element.insertBefore(record.liveContent,record.description);}
        if(record.liveContent && record.liveContent.textContent!==liveText)record.liveContent.textContent=liveText;
        if (metadata.description) { if(record.description.textContent!==metadata.description)record.description.textContent=metadata.description; if(element.getAttribute('aria-describedby')!==record.description.id)element.setAttribute('aria-describedby',record.description.id); }
        else { if(record.description.textContent)record.description.textContent=''; if(element.getAttribute('aria-describedby'))element.removeAttribute('aria-describedby'); }
        Object.assign(element.style,{left:(bounds.left+originX)*scaleX+'px',top:(bounds.top+originY)*scaleY+'px',width:(bounds.right-bounds.left)*scaleX+'px',height:(bounds.bottom-bounds.top)*scaleY+'px'});
        wanted.push(record);
      }
      // Preserve runtime reading order when no explicit authored order is present.
      wanted.sort((a,b)=>{
        const am=a.node.accessibility,bm=b.node.accessibility;
        const reading=m=>nonnegativeOrder(m.reading_order ?? m.readingOrder) || Infinity;
        const tab=m=>metadataOrder(m) || Infinity;
        return reading(am)-reading(bm) || tab(am)-tab(bm) || 0;
      });
      const retained=new Set(wanted);
      for (const [key,record] of this.records) if (!retained.has(record)) { this.retire(record); this.records.delete(key); }
      // Reordering may detach and blur a node. Restore native focus without a second Stage transition.
      this.syncingFocus=true;
      try {
        let cursor=this.container.firstElementChild;
        for (const record of wanted) {
          if (record.element===cursor) cursor=cursor.nextElementSibling;
          else this.container.insertBefore(record.element,cursor);
        }
        if(previouslyFocused && doc.activeElement!==previouslyFocused && wanted.some(r=>r.element===previouslyFocused && this.eligible(r))){
          this.restoringFocus=true;
          try {previouslyFocused.focus({preventScroll:true});} finally {this.restoringFocus=false;}
        }
      } finally {this.syncingFocus=false;}
      return wanted.length;
    }
    clear() {
      for (const record of this.records.values()) this.retire(record);
      this.records.clear();
    }
  }
  function applyDomAccessibility(element, metadata = null, options = {}) {
    if (!element) return;
    const { autoLabel = '', isInteractive = false, explicitTabIndex = undefined, disabled = false } = options;
    const tabEnabled=options.tabEnabled ?? metadata?.tabEnabled ?? metadata?.tab_enabled;
    const silent = Boolean(metadata?.silent);
    const suppressAuto = Boolean(metadata?.noAutoLabeling || metadata?.no_auto_labeling);
    const label = String(metadata?.name || (!suppressAuto ? autoLabel : '') || '').trim();
    attribute(element,'aria-hidden',silent ? 'true' : null);
    attribute(element,'aria-label',!silent && label ? label : null);
    const description = String(metadata?.description || '').trim();
    attribute(element,'aria-description',!silent && description ? description : null);
    const valueText = metadata?.valueText ?? metadata?.ariaValueText;
    attribute(element,'aria-valuetext',!silent && valueText !== undefined && valueText !== null && String(valueText).length > 0 ? String(valueText) : null);
    attribute(element,'aria-selected',!silent && typeof metadata?.selected === 'boolean' ? String(metadata.selected) : null);
    const checkedVal = metadata?.checked;
    attribute(element,'aria-checked',!silent && checkedVal !== undefined && checkedVal !== null && checkedVal !== '' ? String(checkedVal) : null);
    attribute(element,'aria-pressed',!silent && typeof metadata?.pressed === 'boolean' && metadata.pressed ? 'true' : null);
    attribute(element,'aria-expanded',!silent && typeof metadata?.expanded === 'boolean' ? String(metadata.expanded) : null);
    attribute(element,'aria-readonly',!silent && metadata?.readonly === true ? 'true' : null);
    attribute(element,'data-default-action',!silent && metadata?.defaultAction ? String(metadata.defaultAction) : null);
    attribute(element,'data-child-id',metadata?.childID !== undefined && metadata?.childID !== null && Number(metadata.childID) > 0 ? String(metadata.childID) : null);
    const shortcut = String(metadata?.shortcut || '').trim();
    attribute(element,'aria-keyshortcuts',!silent && shortcut ? shortcut : null);
    const live = String(metadata?.live || '').trim().toLowerCase();
    // Keep an explicit "off": status/alert roles otherwise retain their implicit live behavior.
    attribute(element,'aria-live',!silent && ['off','polite','assertive'].includes(live) ? live : null);
    attribute(element,'role',silent ? null : ariaRole(metadata?.role));
    attribute(element,'data-force-simple',metadata?.forceSimple || metadata?.force_simple ? 'true' : null);
    attribute(element,'data-no-auto-labeling',suppressAuto ? 'true' : null);
    const tag = (element.tagName || '').toUpperCase();
    const isNativeControl = tag === 'BUTTON' || tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    const effectiveDisabled = Boolean(disabled || metadata?.disabled);
    if (isNativeControl) element.disabled = effectiveDisabled;
    attribute(element,'aria-disabled',!silent && effectiveDisabled ? 'true' : null);
    const explicitOrder=nonnegativeOrder(explicitTabIndex),parsedOrder=explicitOrder ?? metadataOrder(metadata);
    const positiveOrder = parsedOrder!==null && parsedOrder>0 ? parsedOrder : null;
    if (silent || effectiveDisabled || tabEnabled === false) {
      // Native controls remain focusable without an explicit tabindex, including silent inputs.
      if (isInteractive || isNativeControl || element.getAttribute('tabindex')!==null || positiveOrder!==null) element.tabIndex = -1;
      if(element.ownerDocument?.activeElement===element)element.blur?.();
    } else if (positiveOrder !== null) {
      if (element.tabIndex !== positiveOrder) element.tabIndex = positiveOrder;
    } else if (isInteractive || isNativeControl || tabEnabled === true || explicitOrder === 0) {
      if (element.tabIndex !== 0) element.tabIndex = 0;
    } else if (element.getAttribute('tabindex')!==null) {
      element.removeAttribute('tabindex');
      if(element.ownerDocument?.activeElement===element)element.blur?.();
    }
  }
  const api={itemBounds,targetBounds,sceneBoundsByPath,pathBounds,areaCapable,msaaRoleToAria,decodeMsaaState,SemanticLayer,applyDomAccessibility};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAccessibility=api;
})(globalThis);
