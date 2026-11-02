/* MovieClip/DisplayObject facade over the shared Rust placement graph. */
(function (global) {
  'use strict';
  const UNHANDLED = Symbol('unhandled display member');
  const graphicsInstances=new WeakSet();
  class Graphics {constructor(){throw new TypeError('Graphics is created by Sprite, Shape or MovieClip; use the object.graphics property.');}}
  Graphics.__nfIs=value=>graphicsInstances.has(value);
  Graphics.__nfInitialize=()=>{throw new TypeError('Graphics cannot be used as a native base class.');};
  Object.defineProperty(Graphics,Symbol.hasInstance,{value:Graphics.__nfIs});
  // Constants are shared by every publication, but cannot be constructed or
  // extended. Unsupported modes remain named so selecting one gives a useful
  // operation diagnostic instead of an undefined value.
  function graphicsConstants(name,values) {
    const type=class {constructor(){throw new TypeError(`${name} supplies Graphics constants and cannot be constructed.`);}};
    Object.defineProperty(type,'name',{value:name});
    type.__nfInitialize=()=>{throw new TypeError(`${name} cannot be a native base class.`);};
    return Object.freeze(Object.assign(type,values));
  }
  const GradientType=graphicsConstants('GradientType',{LINEAR:'linear',RADIAL:'radial'});
  const SpreadMethod=graphicsConstants('SpreadMethod',{PAD:'pad',REFLECT:'reflect',REPEAT:'repeat'});
  const InterpolationMethod=graphicsConstants('InterpolationMethod',{RGB:'rgb',LINEAR_RGB:'linearRGB'});
  const GraphicsPathCommand=graphicsConstants('GraphicsPathCommand',{NO_OP:0,MOVE_TO:1,LINE_TO:2,CURVE_TO:3,WIDE_MOVE_TO:4,WIDE_LINE_TO:5,CUBIC_CURVE_TO:6});
  const GraphicsPathWinding=graphicsConstants('GraphicsPathWinding',{EVEN_ODD:'evenOdd',NON_ZERO:'nonZero'});
  const BlendMode=graphicsConstants('BlendMode',{ADD:'add',ALPHA:'alpha',DARKEN:'darken',DIFFERENCE:'difference',ERASE:'erase',HARDLIGHT:'hardlight',INVERT:'invert',LAYER:'layer',LIGHTEN:'lighten',MULTIPLY:'multiply',NORMAL:'normal',OVERLAY:'overlay',SCREEN:'screen',SHADER:'shader',SUBTRACT:'subtract'});
  const CapsStyle=graphicsConstants('CapsStyle',{NONE:'none',ROUND:'round',SQUARE:'square'});
  const JointStyle=graphicsConstants('JointStyle',{BEVEL:'bevel',MITER:'miter',ROUND:'round'});
  const LineScaleMode=graphicsConstants('LineScaleMode',{HORIZONTAL:'horizontal',NONE:'none',NORMAL:'normal',VERTICAL:'vertical'});
  const TriangleCulling=graphicsConstants('TriangleCulling',{NONE:'none',POSITIVE:'positive',NEGATIVE:'negative'});
  const graphicsEnums={GradientType,SpreadMethod,InterpolationMethod,GraphicsPathCommand,GraphicsPathWinding,BlendMode,CapsStyle,JointStyle,LineScaleMode,TriangleCulling};
  const vectorBrand=Symbol.for('neoflash.as3.vector.instance.v1');
  function graphicsNumbers(value,name,limit,allowVector=false) {
    const tag=value!=null&&(typeof value==='object'||typeof value==='function')?Object.getOwnPropertyDescriptor(value,vectorBrand):null;
    const vector=allowVector&&tag&&Object.hasOwn(tag,'value')&&tag.value?.version===1;
    if(!Array.isArray(value)&&!vector)throw new TypeError(`Graphics ${name} requires a dense Array${allowVector?' or typed Vector':''}.`);
    const length=value.length;
    if(!Number.isInteger(length)||length<0||length>limit)throw new RangeError(`Graphics ${name} exceeds ${limit} entries.`);
    const result=[];
    for(let i=0;i<length;i++) {
      const item=Object.getOwnPropertyDescriptor(value,String(i));
      if(!item||!Object.hasOwn(item,'value')||typeof item.value!=='number'||!Number.isFinite(item.value))throw new TypeError(`Graphics ${name}[${i}] requires a finite numeric data value; holes and accessors are unsupported.`);
      result.push(item.value);
    }
    return result;
  }
  const identity = [1, 0, 0, 1, 0, 0];
  const samePath = (a, b) => a.length === b.length && a.every((id, i) => id === b[i]);
  const within = (path, parent) => path.length >= parent.length && parent.every((id, i) => path[i] === id);
  const point = (m, x, y) => ({ x: m[0] * x + m[2] * y + m[4], y: m[1] * x + m[3] * y + m[5] });
  const multiply = (a, b) => [a[0]*b[0]+a[2]*b[1], a[1]*b[0]+a[3]*b[1], a[0]*b[2]+a[2]*b[3], a[1]*b[2]+a[3]*b[3], a[0]*b[4]+a[2]*b[5]+a[4], a[1]*b[4]+a[3]*b[5]+a[5]];
  function matrix(t) { const r=t.rotation*Math.PI/180,y=r+(t.skew||0)*Math.PI/180; return [Math.cos(r)*t.scale_x,Math.sin(r)*t.scale_x,-Math.sin(y)*t.scale_y,Math.cos(y)*t.scale_y,t.x,t.y]; }
  function inverse(m) {
    const d=m[0]*m[3]-m[1]*m[2];
    if (!Number.isFinite(d) || Math.abs(d)<1e-12) throw new RangeError('Coordinate conversion requires an invertible target transform.');
    return [m[3]/d,-m[1]/d,-m[2]/d,m[0]/d,(m[2]*m[5]-m[3]*m[4])/d,(m[1]*m[4]-m[0]*m[5])/d];
  }
  function normalizedRotation(value) { const r=value%360;return r>180?r-360:r< -180?r+360:r; }
  function nonAxisAligned(node){const m=node.local_matrix||matrix(node.transform);return Math.abs(m[1])>1e-10||Math.abs(m[2])>1e-10;}
  const textProperties=new Set(['text','textColor','length','numLines','textWidth','textHeight']);
  const unsupportedText=new Set(['htmlText','styleSheet']);
  const textValue=value=>value.replace(/\r\n?|\n/g,'\r');

  function create({player,geometry,getGeom3D=()=>null,proxyFor,proxyForRef,referenceOf,getStage=()=>null,measureText,pathBounds,onMutation=()=>{},onTabChange=()=>{},onTextLink=null,project,prepareLinked=()=>{},getBitmap=()=>null,getText=()=>null,getLoader=()=>null,getVideo=()=>null,getFilters=()=>null,getNet=()=>null,getAudio=()=>null,getAS2Text=()=>null,getFte3D=()=>null,shapeContains=null,clipsContain=null,as2=null,beforeFilters=()=>{},beforeRaster=()=>{}}) {
    if(!geometry)throw new Error('The Rust geometry family is missing. Re-export this project.');
    const {Point,Rectangle,Matrix,ColorTransform}=geometry;
    const legacy = new Map();
    const pendingClipLoads = [];
    const clipEventForCallback = {onEnterFrame:'enterFrame',onUnload:'unload',onMouseDown:'mouseDown',onMouseMove:'mouseMove',onMouseUp:'mouseUp'};
    const buttonEventForCallback = {onPress:'press',onRelease:'release',onReleaseOutside:'releaseOutside',onRollOver:'rollOver',onRollOut:'rollOut',onDragOver:'dragOver',onDragOut:'dragOut'};
    function matchFlashKeyPress(keySpec, event = {}) {
      if (typeof keySpec !== 'string' || !keySpec) return false;
      const key = typeof event.key === 'string' ? event.key : null;
      const code = typeof event.code === 'string' ? event.code : '';
      const kc = typeof event.keyCode === 'number' ? event.keyCode : -1;
      switch (keySpec) {
        case '<Left>': return key === 'ArrowLeft' || key === 'Left' || code === 'ArrowLeft' || kc === 37;
        case '<Right>': return key === 'ArrowRight' || key === 'Right' || code === 'ArrowRight' || kc === 39;
        case '<Up>': return key === 'ArrowUp' || key === 'Up' || code === 'ArrowUp' || kc === 38;
        case '<Down>': return key === 'ArrowDown' || key === 'Down' || code === 'ArrowDown' || kc === 40;
        case '<Enter>': return key === 'Enter' || code === 'Enter' || code === 'NumpadEnter' || kc === 13;
        case '<Space>': return key === ' ' || key === 'Spacebar' || code === 'Space' || kc === 32;
        case '<Tab>': return key === 'Tab' || code === 'Tab' || kc === 9;
        case '<Escape>': return key === 'Escape' || key === 'Esc' || code === 'Escape' || kc === 27;
        case '<Backspace>': return key === 'Backspace' || code === 'Backspace' || kc === 8;
        case '<Delete>': return key === 'Delete' || key === 'Del' || code === 'Delete' || kc === 46;
        case '<Home>': return key === 'Home' || code === 'Home' || kc === 36;
        case '<End>': return key === 'End' || code === 'End' || kc === 35;
        case '<PageUp>': return key === 'PageUp' || code === 'PageUp' || kc === 33;
        case '<PageDown>': return key === 'PageDown' || code === 'PageDown' || kc === 34;
        case '<Insert>': return key === 'Insert' || code === 'Insert' || kc === 45;
        default: break;
      }
      if (key !== null && [...key].length === 1) return key === keySpec;
      if (key === null && kc > 0 && [...keySpec].length === 1) {
        if (keySpec >= 'a' && keySpec <= 'z') return !event.shiftKey && kc === keySpec.toUpperCase().charCodeAt(0);
        if (keySpec >= 'A' && keySpec <= 'Z') return Boolean(event.shiftKey) && kc === keySpec.charCodeAt(0);
        return kc === keySpec.charCodeAt(0);
      }
      return false;
    }
    const staticTextLifetimes = new Map();
    let accProps = new WeakMap(), spriteSoundTransforms = new WeakMap();
    let touchDragId = null;
    let graphicsVectors = {
      intVector: values => Array.from(values, v => v | 0),
      numberVector: values => Array.from(values, Number),
      dataVector: values => Array.from(values),
    };
    function setGraphicsVectorFactories(factories = {}) {
      if (typeof factories.intVector === 'function') graphicsVectors.intVector = factories.intVector;
      if (typeof factories.numberVector === 'function') graphicsVectors.numberVector = factories.numberVector;
      if (typeof factories.dataVector === 'function') graphicsVectors.dataVector = factories.dataVector;
    }
    const graph=typeof player.runtime_snapshot_json==='function';
    const refKey=ref=>JSON.stringify(ref);
    const nodeKey=node=>graph?refKey(node.object_ref):JSON.stringify(node.path);
    const nodeProxy=node=>graph?proxyForRef(node.object_ref):proxyFor(node.path);
    const invoke=(target,name,...args)=>player[(graph?'object_':'')+name](referenceOf(target).key,...args);
    function isStaticTextNode(node) {
      if (!node || node.owner !== 'text' || node.kind?.type !== 'text') return false;
      const id = node.lifetime_id || nodeKey(node);
      if (!staticTextLifetimes.has(id)) staticTextLifetimes.set(id, !node.name);
      return staticTextLifetimes.get(id);
    }
    function nodes() { return graph?JSON.parse(player.runtime_snapshot_json()).objects:JSON.parse(player.display_nodes_json()); }
    function descriptor(target, list=nodes()) {
      const ref=referenceOf(target);
      if (!ref || !alive(target)) throw new ReferenceError('Display object was removed, replaced, or belongs to another player.');
      const result=list.find(node=>nodeKey(node)===ref.key);
      if (!result) throw new ReferenceError('Display object is no longer active.');
      return result;
    }
    function parentNode(node,list) {
      return graph?(node.parent_ref?list.find(n=>nodeKey(n)===refKey(node.parent_ref)):null):list.find(n=>samePath(n.path,node.path.slice(0,-1))&&node.path.length>0);
    }
    function descendant(node,parent,list) {
      if(!graph)return within(node.path,parent.path);
      for(let current=node;current;current=parentNode(current,list))if(nodeKey(current)===nodeKey(parent))return true;
      return false;
    }
    function relative(node, target, list) {
      if (!target) return node.world;
      if (descendant(node,target,list)) {
        const chain=[];
        for(let current=node;nodeKey(current)!==nodeKey(target);current=parentNode(current,list))chain.push(current);
        let result=identity;for(const child of chain.reverse())result=multiply(result,child.content_local_matrix||child.local_matrix||matrix(child.transform));
        return result;
      }
      return multiply(inverse(target.world),node.world);
    }
    function bounds(target, space, strokes=true) {
      const list=nodes(), source=descriptor(target,list);
      const destination=space===null || space===getStage() ? null : descriptor(space,list);
      let box=null;
      const include=(x,y)=>{ if (!Number.isFinite(x)||!Number.isFinite(y)) throw new RangeError('Display bounds exceed the finite coordinate range.'); box=box ? {left:Math.min(box.left,x),top:Math.min(box.top,y),right:Math.max(box.right,x),bottom:Math.max(box.bottom,y)} : {left:x,top:y,right:x,bottom:y}; };
      for(const leaf of list) {
        if(!descendant(leaf,source,list))continue;
        // A scroll viewport replaces that subtree's reported bounds, even when
        // its content is smaller. Keep mask geometry out of this bounds rule:
        // masks affect painting/picking, not the unmasked script geometry.
        let cropped=false;
        for(let ancestor=parentNode(leaf,list);ancestor&&descendant(ancestor,source,list);ancestor=parentNode(ancestor,list))if(ancestor.scroll_rect){cropped=true;break;}
        if(cropped)continue;
        if(leaf.scroll_rect){
          const rect=leaf.scroll_rect;
          const m=destination&&nodeKey(leaf)===nodeKey(destination)?identity:multiply(relative(leaf,destination,list),[1,0,0,1,rect.x,rect.y]);
          for(const [x,y]of [[0,0],[rect.width,0],[rect.width,rect.height],[0,rect.height]]){const p=point(m,x,y);include(p.x,p.y);}
          continue;
        }
        const shapes=leaf.video?[{type:'video_frame',...leaf.video}]:leaf.text?[{type:'text_field',field:leaf.text}]:leaf.owner==='bitmap'?(leaf.bitmap&&!leaf.bitmap.disposed?[{type:'bitmap_pixels',width:leaf.bitmap.width,height:leaf.bitmap.height}]:[]):leaf.fte_draws?.length?leaf.fte_draws.map(r=>({type:'fte_run',...r})):leaf.kind?[leaf.kind]:[];
        if(leaf.owner==='text_line'&&!shapes.length){const tb=getFte3D()?.textLineBounds?.(nodeProxy(leaf));if(tb)shapes.push({type:'fte_run',x:tb.x,y:tb.y,width:tb.width,font_size:tb.height/1.2});}
        for(const kind of [...shapes,...(leaf.graphics||[])]) {
        const m=relative(leaf,destination,list),stroke=strokes?(kind.stroke_width||0)/2:0;
        const corners=(x,y,w,h)=>{for(const [px,py] of [[x,y],[x+w,y],[x+w,y+h],[x,y+h]]){const p=point(m,px,py);include(p.x,p.y);}};
        if(kind.type==='text_field'){const b=getText().bounds(kind.field);if(b)corners(b.left,b.top,b.right-b.left,b.bottom-b.top);}
        else if(kind.type==='fte_run')corners(kind.x,kind.y,(kind.width||kind.text.length*kind.font_size*0.6),kind.font_size*1.2);
        else if(kind.type==='bitmap_pixels'||kind.type==='video_frame')corners(0,0,kind.width,kind.height);
        else if(kind.type==='rectangle')corners(-stroke,-stroke,kind.width+2*stroke,kind.height+2*stroke);
        else if(kind.type==='ellipse') {
          const center=point(m,kind.width/2,kind.height/2);
          const rx=Math.hypot(m[0]*kind.width/2,m[2]*kind.height/2)+stroke*Math.hypot(m[0],m[2]);
          const ry=Math.hypot(m[1]*kind.width/2,m[3]*kind.height/2)+stroke*Math.hypot(m[1],m[3]);
          include(center.x-rx,center.y-ry);include(center.x+rx,center.y+ry);
        } else if(kind.type==='path'||kind.type==='gradient_path') {
          const b=pathBounds(kind.commands,m);
          if(b){const dx=stroke*Math.hypot(m[0],m[2]),dy=stroke*Math.hypot(m[1],m[3]);include(b.left-dx,b.top-dy);include(b.right+dx,b.bottom+dy);}
        } else if(kind.type==='text') {
          kind.text.split('\n').forEach((line,i)=>corners(0,i*kind.font_size*1.2,measureText(line,kind),kind.font_size*1.2));
        }
      }
      }
      return box ? new Rectangle(box.left,box.top,box.right-box.left,box.bottom-box.top) : new Rectangle();
    }
    // Transform is bound to a Rust object identity. Returned value objects are
    // copies; concatenated/pixel coordinates use the logical stage, excluding
    // the editor/export preview's CSS zoom and browser-window mapping.
    const transformTargets=new WeakMap(),transformCache=new WeakMap();
    let transformMatrix3D=new WeakMap(),transformPerspective=new WeakMap();
    const colorNames=['redMultiplier','greenMultiplier','blueMultiplier','alphaMultiplier','redOffset','greenOffset','blueOffset','alphaOffset'];
    const identityColor={multipliers:[1,1,1,1],offsets:[0,0,0,0]};
    const colorCopy=value=>new ColorTransform(...value.multipliers,...value.offsets);
    function transformTarget(value){const target=transformTargets.get(value);if(!target)throw new TypeError('Expected a bound Transform.');descriptor(target);return target;}
    function finiteComponents(value,names){
      if(value===null||typeof value!=='object')throw new TypeError('Transform requires a geometry value object.');
      const result=names.map(name=>value[name]);
      if(!result.every(value=>typeof value==='number'&&Number.isFinite(value)))throw new RangeError('Transform components must be finite numbers.');
      return result;
    }
    class Transform {
      constructor(target){descriptor(target);transformTargets.set(this,target);Object.seal(this);}
      get matrix(){const target=transformTarget(this);if(transformMatrix3D.has(target))return null;const node=descriptor(target);return new Matrix(...(node.local_matrix||matrix(node.transform)));}
      set matrix(value){
        const target=transformTarget(this);
        if(value===null){transformMatrix3D.delete(target);getFte3D()?.setMatrix3DForDisplay(target,null);return;}
        const components=finiteComponents(value,['a','b','c','d','tx','ty']);
        transformMatrix3D.delete(target);
        getFte3D()?.setMatrix3DForDisplay(target,null);
        if(!player.object_set_matrix(referenceOf(target).key,JSON.stringify(components)))throw new RangeError('Runtime rejected the matrix on this display object.');
      }
      get matrix3D(){
        const target=transformTarget(this),host=getFte3D();
        if(host)return host.getMatrix3DForDisplay(target);
        const m=transformMatrix3D.get(target);
        return m?m.clone():null;
      }
      set matrix3D(value){
        const target=transformTarget(this),host=getFte3D();
        if(host){host.setMatrix3DForDisplay(target,value);return;}
        if(value===null||value===undefined){transformMatrix3D.delete(target);return;}
        const geom3d=getGeom3D();
        if(!geom3d?.Matrix3D?.__nfIs?.(value))throw new TypeError('Transform.matrix3D requires a Matrix3D or null.');
        const copy=value.clone(),raw=copy.rawData;
        transformMatrix3D.set(target,copy);
        const a=Number(raw[0]),b=Number(raw[1]),c=Number(raw[4]),d=Number(raw[5]),tx=Number(raw[12]),ty=Number(raw[13]);
        if([a,b,c,d,tx,ty].every(Number.isFinite)&&Math.abs(a*d-b*c)>1e-8){
          try{player.object_set_matrix(referenceOf(target).key,JSON.stringify([a,b,c,d,tx,ty]));}catch(_){}
        }
      }
      get perspectiveProjection(){
        const target=transformTarget(this),host=getFte3D();
        if(host)return host.getPerspectiveProjectionForDisplay(target);
        return transformPerspective.get(target)||null;
      }
      set perspectiveProjection(value){
        const target=transformTarget(this),host=getFte3D();
        if(host){host.setPerspectiveProjectionForDisplay(target,value);return;}
        if(value===null||value===undefined){transformPerspective.delete(target);return;}
        const geom3d=getGeom3D();
        if(!geom3d?.PerspectiveProjection?.__nfIs?.(value))throw new TypeError('Transform.perspectiveProjection requires a PerspectiveProjection or null.');
        transformPerspective.set(target,value);
      }
      get colorTransform(){return colorCopy(descriptor(transformTarget(this)).color_transform||identityColor);}
      set colorTransform(value){
        const target=transformTarget(this),components=finiteComponents(value,colorNames);
        if(!player.object_set_color_transform(referenceOf(target).key,JSON.stringify({multipliers:components.slice(0,4),offsets:components.slice(4)})))throw new RangeError('Runtime rejected the color transform on this display object.');
      }
      get concatenatedMatrix(){return new Matrix(...descriptor(transformTarget(this)).world);}
      get concatenatedColorTransform(){return colorCopy(descriptor(transformTarget(this)).concatenated_color_transform||identityColor);}
      get pixelBounds(){
        const box=bounds(transformTarget(this),null,true);
        if(box.width<=0||box.height<=0)return new Rectangle();
        const x=Math.floor(box.x),y=Math.floor(box.y);return new Rectangle(x,y,Math.ceil(box.right)-x,Math.ceil(box.bottom)-y);
      }
      getRelativeMatrix(relativeTo){const list=nodes(),node=descriptor(transformTarget(this),list),target=relativeTo===getStage()?null:descriptor(relativeTo,list);return new Matrix(...relative(node,target,list));}
      getRelativeMatrix3D(relativeTo){
        if(relativeTo===null||relativeTo===undefined)throw new TypeError('Transform.getRelativeMatrix3D requires a non-null relativeTo DisplayObject.');
        const host=getFte3D();
        if(host){const target=transformTarget(this);if(relativeTo!==getStage())descriptor(relativeTo);return host.getRelativeMatrix3D(target,relativeTo);}
        const list=nodes(),sourceTarget=transformTarget(this),node=descriptor(sourceTarget,list);
        const target=relativeTo===getStage()?null:descriptor(relativeTo,list);
        const geom3d=getGeom3D();
        if(!geom3d?.Matrix3D)throw new Error('Matrix3D service is unavailable.');
        const m=relative(node,target,list);
        const rel=new geom3d.Matrix3D([m[0],m[1],0,0,m[2],m[3],0,0,0,0,1,0,m[4],m[5],0,1]);
        const local3d=transformMatrix3D.get(sourceTarget);
        if(local3d&&nodeKey(node)===(target?nodeKey(target):''))return local3d.clone();
        return rel;
      }
    }
    Object.defineProperties(Transform,{__nfIs:{value:value=>transformTargets.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native Transform subclassing is not supported.');}}});
    function transform(target){descriptor(target);if(!transformCache.has(target))transformCache.set(target,new Transform(target));return transformCache.get(target);}
    function parentSpace(target) {const list=nodes(),node=descriptor(target,list),parent=parentNode(node,list);return parent?nodeProxy(parent):node.on_stage===false?null:getStage();}
    function children(target) {const list=nodes(),parent=descriptor(target,list);return graph?parent.children.map(ref=>list.find(node=>nodeKey(node)===refKey(ref))).filter(Boolean):list.filter(node=>node.path.length===parent.path.length+1&&within(node.path,parent.path));}
    const containerOwners=['movie_clip','sprite','graphic','loader','text_line'];
    const spriteOwners=['movie_clip','sprite','graphic'];
    const interactiveOwners=['movie_clip','sprite','graphic','button','text','loader','text_line'];
    function requireContainer(target) {const node=descriptor(target);if(!containerOwners.includes(node.owner))throw new TypeError('Child traversal requires a display container.');return node;}
    function convert(target,value,toGlobal) {
      if(!value || !Number.isFinite(value.x)||!Number.isFinite(value.y))throw new TypeError('Coordinate conversion requires finite point.x and point.y.');
      const m=descriptor(target).world,p=point(toGlobal?m:inverse(m),value.x,value.y);return new Point(p.x,p.y);
    }
    function textKind(target) {
      const node=descriptor(target);
      if(node.owner!=='text')throw new TypeError('This operation requires an authored TextField.');
      return node.kind;
    }
    function writeText(target,value) {
      textKind(target);
      if(typeof value!=='string')throw new TypeError('Plain TextField text currently requires a string. Convert primitive values explicitly with String().');
      // Rust strings cannot retain lone UTF-16 surrogates. Reject before the
      // WASM string encoder can silently replace one with U+FFFD.
      for(let i=0;i<value.length;i++) {
        const unit=value.charCodeAt(i);
        if(unit>=0xd800&&unit<=0xdbff){const low=value.charCodeAt(++i);if(!(low>=0xdc00&&low<=0xdfff))throw new RangeError('TextField edits must preserve complete Unicode characters; split-surrogate text is not supported.');}
        else if(unit>=0xdc00&&unit<=0xdfff)throw new RangeError('TextField edits must preserve complete Unicode characters; split-surrogate text is not supported.');
      }
      const normalized=value.replace(/\r\n?|\n/g,'\n');
      if(!invoke(target,'set_text',normalized))throw new RangeError('Runtime text must fit within 1048576 UTF-8 bytes on a live TextField.');
    }
    function lineText(target,index) {
      const lines=textValue(textKind(target).text).split('\r');
      if(!Number.isInteger(index)||index<0||index>=lines.length)throw new RangeError('TextField line index is outside the unwrapped text.');
      return lines[index]+(index<lines.length-1?'\r':'');
    }
    const graphicsCache=new WeakMap(),graphicsTargets=new WeakMap();
    function graphics(target) {
      descriptor(target);
      if(graphicsCache.has(target))return graphicsCache.get(target);
      const number=(value,name)=>{if(typeof value!=='number'||!Number.isFinite(value))throw new TypeError(`Graphics ${name} must be a finite number.`);return value;};
      const rgb=value=>{if(!Number.isInteger(value)||value<0||value>0xffffff)throw new RangeError('Graphics color requires an RGB integer from 0 through 0xffffff.');return value;};
      const opacity=value=>{number(value,'alpha');if(value<0||value>1)throw new RangeError('Graphics alpha requires 0 through 1.');return value;};
      const apply=op=>{descriptor(target);player.graphics_apply(referenceOf(target).key,JSON.stringify([op]));};
      const rectangle=(op,x,y,width,height)=>apply({op,x:number(x,'x'),y:number(y,'y'),width:number(width,'width'),height:number(height,'height')});
      // AIR Graphics/GraphicsPathCommand and the Adobe AS2 MovieClip reference
      // define stop ranges, the 1638.4 gradient square and WIDE_* dummy pairs.
      // Only native Matrix objects, pad spread and rgb interpolation are handled.
      const gradient=(kind,colors,alphas,ratios,matrix=null,spreadMethod='pad',interpolationMethod='rgb',focalPointRatio=0)=>{
        if(kind!=='linear'&&kind!=='radial')throw new TypeError('Graphics gradient type requires linear or radial.');
        if(spreadMethod!=='pad')throw new TypeError('Graphics gradient spread currently supports pad; repeat and reflect are pending.');
        if(interpolationMethod!=='rgb')throw new TypeError('Graphics gradient interpolation currently supports rgb; linearRGB is pending.');
        const stops=graphicsNumbers(colors,'colors',15).map(rgb),alpha=graphicsNumbers(alphas,'alphas',15).map(value=>Math.max(0,Math.min(1,value))),positions=graphicsNumbers(ratios,'ratios',15);
        if(stops.length===0||stops.length!==alpha.length||stops.length!==positions.length)throw new RangeError('Graphics gradients require matching colors, alphas and ratios with 1 through 15 stops.');
        for(let i=0;i<positions.length;i++)if(!Number.isInteger(positions[i])||positions[i]<0||positions[i]>255||(i>0&&positions[i]<positions[i-1]))throw new RangeError('Graphics gradient ratios require nondecreasing integers from 0 through 255.');
        if(matrix!==null&&!Matrix.__nfIs(matrix))throw new TypeError('Graphics gradient matrix requires flash.geom.Matrix or null; legacy box and 3×3 matrix objects are unsupported.');
        const components=matrix===null?[...identity]:['a','b','c','d','tx','ty'].map(key=>number(matrix[key],`matrix.${key}`));
        return {kind,colors:stops,alphas:alpha,ratios:positions,matrix:components,spread_method:spreadMethod,interpolation_method:interpolationMethod,focal_point_ratio:Math.max(-1,Math.min(1,number(focalPointRatio,'focalPointRatio')))};
      };
      const drawPath=(commands,data,winding='evenOdd')=>{
        if(winding!=='evenOdd'&&winding!=='nonZero')throw new TypeError('Graphics.drawPath winding requires evenOdd or nonZero.');
        const codes=graphicsNumbers(commands,'commands',100000,true),values=graphicsNumbers(data,'data',600000,true),path=[];
        let cursor=0;
        const next=()=>{if(cursor>=values.length)throw new RangeError('Graphics.drawPath data ends before all command coordinates.');return values[cursor++];};
        for(const code of codes) {
          switch(code) {
            case 0:break;
            case 1:case 2:path.push({op:code===1?'move_to':'line_to',x:next(),y:next()});break;
            case 3:path.push({op:'quadratic_to',cx:next(),cy:next(),x:next(),y:next()});break;
            case 4:case 5:next();next();path.push({op:code===4?'move_to':'line_to',x:next(),y:next()});break;
            case 6:path.push({op:'cubic_to',cx1:next(),cy1:next(),cx2:next(),cy2:next(),x:next(),y:next()});break;
            default:throw new TypeError(`Graphics.drawPath command ${code} is unsupported; use GraphicsPathCommand constants 0 through 6.`);
          }
        }
        if(cursor!==values.length)throw new RangeError('Graphics.drawPath has unused coordinate data; provide exactly the command data.');
        apply({op:'draw_path',commands:path,fill_rule:winding==='evenOdd'?'evenodd':'nonzero'});
      };
      const methods={
        clear:()=>apply({op:'clear'}),
        beginFill:(color,alpha=1)=>apply({op:'begin_fill',color:rgb(color),alpha:opacity(alpha)}),
        beginBitmapFill:(bitmap,matrix=null,repeat=true,smoothing=false)=>{
          const bitmapHost=getBitmap();if(!bitmapHost)throw new Error('Bitmap runtime is missing. Re-export this project.');
          const pixelRef=bitmapHost.refOf(bitmap);
          if(matrix!==null&&!Matrix.__nfIs(matrix))throw new TypeError('Graphics.beginBitmapFill matrix requires flash.geom.Matrix or null.');
          if(typeof repeat!=='boolean'||typeof smoothing!=='boolean')throw new TypeError('Graphics.beginBitmapFill repeat and smoothing require Booleans.');
          const components=matrix===null?[...identity]:['a','b','c','d','tx','ty'].map(key=>number(matrix[key],`matrix.${key}`));
          apply({op:'begin_bitmap_fill',bitmap:{pixel_ref:pixelRef,matrix:components,repeat,smoothing}});
        },
        beginGradientFill:(...args)=>apply({op:'begin_gradient_fill',gradient:gradient(...args)}),
        beginShaderFill:(shader,matrix=null)=>{
          const host=getFte3D();if(!host)throw new Error('Shader runtime is missing.');
          const bmp=host.rasterizeShaderToBitmapData(shader);
          methods.beginBitmapFill(bmp,matrix,true,false);
        },
        lineGradientStyle:(...args)=>apply({op:'line_gradient_style',gradient:gradient(...args)}),
        lineShaderStyle:(shader,matrix=null)=>{
          const host=getFte3D();if(!host)throw new Error('Shader runtime is missing.');
          const bmp=host.rasterizeShaderToBitmapData(shader);
          methods.lineBitmapStyle(bmp,matrix,true,false);
        },
        lineBitmapStyle:(bitmap,matrix=null,repeat=true,smooth=false)=>{
          const bitmapHost=getBitmap();if(!bitmapHost)throw new Error('Bitmap runtime is missing. Re-export this project.');
          const pixelRef=bitmapHost.refOf(bitmap);
          if(matrix!==null&&!Matrix.__nfIs(matrix))throw new TypeError('Graphics.lineBitmapStyle matrix requires flash.geom.Matrix or null.');
          if(typeof repeat!=='boolean'||typeof smooth!=='boolean')throw new TypeError('Graphics.lineBitmapStyle repeat and smooth require Booleans.');
          const components=matrix===null?[...identity]:['a','b','c','d','tx','ty'].map(key=>number(matrix[key],`matrix.${key}`));
          apply({op:'line_bitmap_style',bitmap:{pixel_ref:pixelRef,matrix:components,repeat,smoothing:smooth}});
        },
        endFill:()=>apply({op:'end_fill'}),
        lineStyle:(width=NaN,color=0,alpha=1,pixelHinting=false,scaleMode='normal',caps=null,joints=null,miterLimit=3)=>{
          if(typeof pixelHinting!=='boolean')throw new TypeError('Graphics.lineStyle pixelHinting requires a Boolean.');
          if(scaleMode!==null&&!['normal','none','vertical','horizontal'].includes(scaleMode)){const err=new RangeError(`Graphics.lineStyle scaleMode is invalid: '${scaleMode}'.`);err.name='ArgumentError';throw err;}
          if(caps!==null&&!['none','round','square'].includes(caps)){const err=new RangeError(`Graphics.lineStyle caps is invalid: '${caps}'.`);err.name='ArgumentError';throw err;}
          if(joints!==null&&!['round','bevel','miter'].includes(joints)){const err=new RangeError(`Graphics.lineStyle joints is invalid: '${joints}'.`);err.name='ArgumentError';throw err;}
          number(miterLimit,'miterLimit');if(miterLimit<1||miterLimit>255)throw new RangeError('Graphics.lineStyle miterLimit must be from 1 through 255.');
          if(width!==undefined&&!Number.isNaN(width)){number(width,'thickness');if(width<=0||width>255)throw new RangeError('Graphics thickness currently requires greater than 0 through 255; hairlines are pending.');}
          apply({op:'line_style',width:width===undefined||Number.isNaN(width)?null:width,color:rgb(color),alpha:opacity(alpha)});
        },
        moveTo:(x,y)=>apply({op:'move_to',x:number(x,'x'),y:number(y,'y')}),
        lineTo:(x,y)=>apply({op:'line_to',x:number(x,'x'),y:number(y,'y')}),
        curveTo:(cx,cy,x,y)=>apply({op:'curve_to',cx:number(cx,'controlX'),cy:number(cy,'controlY'),x:number(x,'x'),y:number(y,'y')}),
        cubicCurveTo:(cx1,cy1,cx2,cy2,x,y)=>apply({op:'cubic_curve_to',cx1:number(cx1,'controlX1'),cy1:number(cy1,'controlY1'),cx2:number(cx2,'controlX2'),cy2:number(cy2,'controlY2'),x:number(x,'x'),y:number(y,'y')}),
        drawRect:(x,y,width,height)=>rectangle('draw_rect',x,y,width,height),
        drawRoundRect:(x,y,width,height,ellipseWidth,ellipseHeight=NaN)=>apply({op:'draw_round_rect',x:number(x,'x'),y:number(y,'y'),width:number(width,'width'),height:number(height,'height'),ellipse_width:number(ellipseWidth,'ellipseWidth'),ellipse_height:number(Number.isNaN(ellipseHeight)?ellipseWidth:ellipseHeight,'ellipseHeight')}),
        // NeoFlash convenience extension, not a native Flash Graphics method:
        // four corner radii in the Flex/OpenFL ordering (bottom-left before right).
        drawRoundRectComplex:(x,y,width,height,topLeftRadius,topRightRadius,bottomLeftRadius,bottomRightRadius)=>apply({op:'draw_round_rect_complex',x:number(x,'x'),y:number(y,'y'),width:number(width,'width'),height:number(height,'height'),top_left_radius:number(topLeftRadius,'topLeftRadius'),top_right_radius:number(topRightRadius,'topRightRadius'),bottom_left_radius:number(bottomLeftRadius,'bottomLeftRadius'),bottom_right_radius:number(bottomRightRadius,'bottomRightRadius')}),
        drawPath,
        drawTriangles:(vertices,indices=null,uvtData=null,culling='none')=>{
          if(!['none','positive','negative'].includes(culling)){const err=new RangeError(`Graphics.drawTriangles culling is invalid: '${culling}'.`);err.name='ArgumentError';throw err;}
          const verts=graphicsNumbers(vertices,'vertices',600000,true);
          if(verts.length%2!==0){const err=new RangeError('Graphics.drawTriangles vertices length must be even.');err.name='ArgumentError';throw err;}
          const vertCount=verts.length>>>1;
          let inds;
          if(indices===null||indices===undefined){
            if(vertCount%3!==0){const err=new RangeError('Graphics.drawTriangles unindexed vertices must be a multiple of 3 points.');err.name='ArgumentError';throw err;}
            inds=Array.from({length:vertCount},(_,i)=>i);
          }else{
            inds=graphicsNumbers(indices,'indices',300000,true);
            if(inds.length%3!==0){const err=new RangeError('Graphics.drawTriangles indices length must be a multiple of 3.');err.name='ArgumentError';throw err;}
            for(let i=0;i<inds.length;i++)if(!Number.isInteger(inds[i])||inds[i]<0||inds[i]>=vertCount)throw new RangeError(`Graphics.drawTriangles index ${inds[i]} is out of bounds.`);
          }
          if(uvtData!==null&&uvtData!==undefined){
            const uvts=graphicsNumbers(uvtData,'uvtData',900000,true);
            if(uvts.length!==vertCount*2&&uvts.length!==vertCount*3){const err=new RangeError('Graphics.drawTriangles uvtData must supply 2 or 3 numbers per vertex.');err.name='ArgumentError';throw err;}
          }
          const triCommands=[];
          for(let i=0;i<inds.length;i+=3){
            const i0=inds[i]*2,i1=inds[i+1]*2,i2=inds[i+2]*2;
            const x0=verts[i0],y0=verts[i0+1],x1=verts[i1],y1=verts[i1+1],x2=verts[i2],y2=verts[i2+1];
            const cross=(x1-x0)*(y2-y0)-(y1-y0)*(x2-x0);
            if(culling==='positive'&&cross<=0)continue;
            if(culling==='negative'&&cross>=0)continue;
            triCommands.push({op:'move_to',x:x0,y:y0},{op:'line_to',x:x1,y:y1},{op:'line_to',x:x2,y:y2},{op:'line_to',x:x0,y:y0});
          }
          if(triCommands.length)apply({op:'draw_path',commands:triCommands,fill_rule:'nonzero'});
        },
        drawGraphicsData:graphicsData=>{
          descriptor(target);
          if(!graphicsData||typeof graphicsData[Symbol.iterator]!=='function')throw new TypeError('Graphics.drawGraphicsData requires a Vector.<IGraphicsData>.');
          for(const item of graphicsData){
            if(!IGraphicsData.__nfIs(item))throw new TypeError('Graphics.drawGraphicsData elements must implement IGraphicsData.');
            if(GraphicsSolidFill.__nfIs(item))methods.beginFill(item.color,item.alpha);
            else if(GraphicsGradientFill.__nfIs(item))methods.beginGradientFill(item.type,item.colors,item.alphas,item.ratios,item.matrix,item.spreadMethod,item.interpolationMethod,item.focalPointRatio);
            else if(GraphicsBitmapFill.__nfIs(item))methods.beginBitmapFill(item.bitmapData,item.matrix,item.repeat,item.smooth);
            else if(getFte3D()?.GraphicsShaderFill?.__nfIs(item))methods.beginShaderFill(item.shader,item.matrix);
            else if(GraphicsEndFill.__nfIs(item))methods.endFill();
            else if(GraphicsStroke.__nfIs(item)){
              if(item.fill===null||item.fill===undefined||Number.isNaN(item.thickness)){
                methods.lineStyle(NaN);
              }else if(GraphicsSolidFill.__nfIs(item.fill)){
                methods.lineStyle(item.thickness,item.fill.color,item.fill.alpha,item.pixelHinting,item.scaleMode,item.caps,item.joints,item.miterLimit);
              }else if(GraphicsGradientFill.__nfIs(item.fill)){
                methods.lineStyle(item.thickness,0,1,item.pixelHinting,item.scaleMode,item.caps,item.joints,item.miterLimit);
                methods.lineGradientStyle(item.fill.type,item.fill.colors,item.fill.alphas,item.fill.ratios,item.fill.matrix,item.fill.spreadMethod,item.fill.interpolationMethod,item.fill.focalPointRatio);
              }else if(GraphicsBitmapFill.__nfIs(item.fill)){
                methods.lineStyle(item.thickness,0,1,item.pixelHinting,item.scaleMode,item.caps,item.joints,item.miterLimit);
                methods.lineBitmapStyle(item.fill.bitmapData,item.fill.matrix,item.fill.repeat,item.fill.smooth);
              }else if(getFte3D()?.GraphicsShaderFill?.__nfIs(item.fill)){
                methods.lineStyle(item.thickness,0,1,item.pixelHinting,item.scaleMode,item.caps,item.joints,item.miterLimit);
                methods.lineShaderStyle(item.fill.shader,item.fill.matrix);
              }else{
                throw new TypeError('GraphicsStroke.fill must implement IGraphicsFill or be null.');
              }
            }else if(GraphicsPath.__nfIs(item)){
              if(item.commands&&item.data)methods.drawPath(item.commands,item.data,item.winding);
            }else if(GraphicsTrianglePath.__nfIs(item)){
              if(item.vertices)methods.drawTriangles(item.vertices,item.indices,item.uvtData,item.culling);
            }
          }
        },
        readGraphicsData:(recurse=true)=>{
          const list=nodes(),source=descriptor(target,list),items=[];
          const toFill=(rgbaArr,grad,bmp)=>{
            if(bmp&&getBitmap())return new GraphicsBitmapFill(getBitmap().fromRef(bmp.pixel_ref),new Matrix(...bmp.matrix),Boolean(bmp.repeat),Boolean(bmp.smoothing));
            if(grad){
              const colors=grad.stops.map(s=>((s.color[0]<<16)|(s.color[1]<<8)|s.color[2])>>>0);
              const alphas=grad.stops.map(s=>s.color[3]/255);
              const ratios=grad.stops.map(s=>s.ratio);
              return new GraphicsGradientFill(grad.kind,colors,alphas,ratios,new Matrix(...grad.matrix),'pad','rgb',grad.focal_point_ratio||0);
            }
            if(rgbaArr&&rgbaArr[3]>0)return new GraphicsSolidFill(((rgbaArr[0]<<16)|(rgbaArr[1]<<8)|rgbaArr[2])>>>0,rgbaArr[3]/255);
            return null;
          };
          for(const leaf of list){
            if(recurse?!descendant(leaf,source,list):nodeKey(leaf)!==nodeKey(source))continue;
            const m=nodeKey(leaf)===nodeKey(source)?identity:relative(leaf,source,list);
            const tx=(x,y)=>{const p=point(m,x,y);return [p.x,p.y];};
            const shapes=[...(leaf.kind?[leaf.kind]:[]),...(leaf.graphics||[])];
            for(const kind of shapes){
              if(!['rectangle','ellipse','path','gradient_path'].includes(kind.type))continue;
              const cmds=[],coords=[];
              if(kind.type==='rectangle'){
                const [x0,y0]=tx(0,0),[x1,y1]=tx(kind.width,0),[x2,y2]=tx(kind.width,kind.height),[x3,y3]=tx(0,kind.height);
                cmds.push(1,2,2,2,2);coords.push(x0,y0,x1,y1,x2,y2,x3,y3,x0,y0);
              }else if(kind.type==='ellipse'){
                const rx=kind.width/2,ry=kind.height/2,cx=rx,cy=ry,k=0.5522847498307936;
                const p0=tx(cx+rx,cy),c1=tx(cx+rx,cy+k*ry),c2=tx(cx+k*rx,cy+ry),p1=tx(cx,cy+ry);
                const c3=tx(cx-k*rx,cy+ry),c4=tx(cx-rx,cy+k*ry),p2=tx(cx-rx,cy);
                const c5=tx(cx-rx,cy-k*ry),c6=tx(cx-k*rx,cy-ry),p3=tx(cx,cy-ry);
                const c7=tx(cx+k*rx,cy-ry),c8=tx(cx+rx,cy-k*ry);
                cmds.push(1,6,6,6,6);
                coords.push(...p0,...c1,...c2,...p1,...c3,...c4,...p2,...c5,...c6,...p3,...c7,...c8,...p0);
              }else{
                let start=[0,0];
                for(const c of kind.commands||[]){
                  if(c.op==='move_to'){const p=tx(c.x,c.y);start=p;cmds.push(1);coords.push(p[0],p[1]);}
                  else if(c.op==='line_to'){const p=tx(c.x,c.y);cmds.push(2);coords.push(p[0],p[1]);}
                  else if(c.op==='quadratic_to'){const cp=tx(c.cx,c.cy),p=tx(c.x,c.y);cmds.push(3);coords.push(cp[0],cp[1],p[0],p[1]);}
                  else if(c.op==='cubic_to'){const c1=tx(c.cx1,c.cy1),c2=tx(c.cx2,c.cy2),p=tx(c.x,c.y);cmds.push(6);coords.push(c1[0],c1[1],c2[0],c2[1],p[0],p[1]);}
                  else if(c.op==='close'){cmds.push(2);coords.push(start[0],start[1]);}
                }
              }
              if(!cmds.length)continue;
              const hasStroke=kind.stroke_width>0;
              const strokeFill=hasStroke?toFill(kind.stroke,kind.stroke_gradient,kind.stroke_bitmap):null;
              const fillObj=toFill(kind.fill,kind.fill_gradient,kind.fill_bitmap);
              if(strokeFill)items.push(new GraphicsStroke(kind.stroke_width,false,'normal','round','round',3,strokeFill));
              if(fillObj)items.push(fillObj);
              const winding=kind.fill_rule==='evenodd'?'evenOdd':'nonZero';
              items.push(new GraphicsPath(graphicsVectors.intVector(cmds),graphicsVectors.numberVector(coords),winding));
              if(fillObj)items.push(new GraphicsEndFill());
              if(strokeFill)items.push(new GraphicsStroke(NaN));
            }
          }
          return graphicsVectors.dataVector(items);
        },
        copyFrom:source=>{const owner=graphicsTargets.get(source);if(!owner)throw new TypeError('Graphics.copyFrom requires a Graphics facade from the same player.');descriptor(target);descriptor(owner);player.graphics_copy_from(referenceOf(target).key,referenceOf(owner).key);},
        drawEllipse:(x,y,width,height)=>rectangle('draw_ellipse',x,y,width,height),
        drawCircle:(x,y,radius)=>{number(radius,'radius');if(radius<0)throw new RangeError('Graphics radius must be nonnegative.');rectangle('draw_ellipse',number(x,'x')-radius,number(y,'y')-radius,radius*2,radius*2);},
      };
      const facade=new Proxy(Object.freeze(methods),{get:(target,name)=>{if(typeof name==='symbol'||name==='then')return undefined;if(Object.hasOwn(target,name))return target[name];throw new TypeError(`Graphics.${name} is not implemented in the path/gradient profile; bitmap/shader fills, triangles and graphics-data objects remain unsupported.`);},set:()=>{throw new TypeError('Graphics members are read-only.');}});
      graphicsInstances.add(facade);graphicsTargets.set(facade,target);graphicsCache.set(target,facade);return facade;
    }
    function mutate(operation,deliver=true) {
      const packet=JSON.parse(player.display_mutate(JSON.stringify(operation)));
      if(deliver)onMutation(packet.events); // captured routes survive the completed reparent/removal
      return packet.events;
    }
    function containerMethods(target) {
      const ref=()=>{requireContainer(target);return referenceOf(target).objectRef;};
      const childRef=child=>{descriptor(child);return referenceOf(child).objectRef;};
      return {
        addChild:child=>{mutate({op:'add',parent:ref(),child:childRef(child),index:null});return child;},
        addChildAt:(child,index)=>{if(!Number.isInteger(index)||index<0)throw new RangeError('Child index requires a nonnegative integer.');mutate({op:'add',parent:ref(),child:childRef(child),index});return child;},
        removeChild:child=>{mutate({op:'remove',parent:ref(),child:childRef(child)});return child;},
        removeChildAt:index=>{const child=methods(target).getChildAt(index);mutate({op:'remove',parent:ref(),child:childRef(child)});return child;},
        removeChildren:(...args)=>{
          if(args.length>2)throw new TypeError('removeChildren accepts beginIndex and endIndex only.');
          const begin=args.length?args[0]:0,end=args.length>1?args[1]:2147483647;
          if(![begin,end].every(value=>typeof value==='number'&&Number.isInteger(value)&&value>=-2147483648&&value<=2147483647))throw new RangeError('removeChildren indices require primitive signed 32-bit integers.');
          requireContainer(target);
          if(typeof player.object_remove_children!=='function')throw new Error('Rust removeChildren services are unavailable; rebuild and re-export this project.');
          let packet;
          try{packet=JSON.parse(player.object_remove_children(referenceOf(target).key,begin,end));}
          catch(error){throw error instanceof Error?error:new Error(String(error),{cause:error});}
          // Rust preflights the complete range and captures lifecycle routes
          // before detaching. User callbacks run only after the atomic change.
          onMutation(packet.events);
        },
        stopAllMovieClips:(...args)=>{
          if(args.length)throw new TypeError('stopAllMovieClips accepts no arguments.');
          requireContainer(target);
          if(typeof player.object_stop_all_movie_clips!=='function')throw new Error('Rust stopAllMovieClips services are unavailable; rebuild and re-export this project.');
          try{player.object_stop_all_movie_clips(referenceOf(target).key);}
          catch(error){throw error instanceof Error?error:new Error(String(error),{cause:error});}
        },
        setChildIndex:(child,index)=>{if(!Number.isInteger(index)||index<0)throw new RangeError('Child index requires a nonnegative integer.');mutate({op:'set_index',parent:ref(),child:childRef(child),index});},
        swapChildren:(first,second)=>{mutate({op:'swap',parent:ref(),first:childRef(first),second:childRef(second)});},
        swapChildrenAt:(first,second)=>{const api=methods(target);mutate({op:'swap',parent:ref(),first:childRef(api.getChildAt(first)),second:childRef(api.getChildAt(second))});},
      };
    }
    const stageRefs=()=>graph&&typeof player.stage_children_json==='function'?JSON.parse(player.stage_children_json()):[];
    function stageMutate(operation){
      if(!graph||typeof player.stage_mutate!=='function')throw new Error('Stage display-list mutation requires the shared runtime graph.');
      let packet;
      try{packet=JSON.parse(player.stage_mutate(JSON.stringify(operation)));}
      catch(error){
        const err=error instanceof Error?error:new RangeError(String(error));
        if(/not a direct child|cannot be added/i.test(err.message))err.name='ArgumentError';
        throw err;
      }
      onMutation(packet.events);
      return packet.events;
    }
    const stageContainer={
      numChildren:()=>stageRefs().length,
      addChild(child){
        if(child===getStage()){const err=new RangeError('Stage cannot be added as a child of itself.');err.name='ArgumentError';throw err;}
        descriptor(child);stageMutate({op:'add',child:referenceOf(child).objectRef,index:null});return child;
      },
      addChildAt(child,index){
        if(child===getStage()){const err=new RangeError('Stage cannot be added as a child of itself.');err.name='ArgumentError';throw err;}
        if(!Number.isInteger(index)||index<0)throw new RangeError('Child index requires a nonnegative integer.');
        descriptor(child);stageMutate({op:'add',child:referenceOf(child).objectRef,index});return child;
      },
      removeChild(child){
        descriptor(child);stageMutate({op:'remove',child:referenceOf(child).objectRef});return child;
      },
      removeChildAt(index){
        const child=stageContainer.getChildAt(index);stageMutate({op:'remove',child:referenceOf(child).objectRef});return child;
      },
      getChildAt(index){
        const refs=stageRefs();
        if(!Number.isInteger(index)||index<0||index>=refs.length)throw new RangeError('Child index is outside Stage.');
        return proxyForRef(refs[index]);
      },
      getChildByName(name){
        const wanted=String(name),list=nodes();
        for(const ref of stageRefs()){
          const k=refKey(ref),node=list.find(n=>nodeKey(n)===k);
          if(node&&node.name===wanted)return proxyForRef(ref);
        }
        return null;
      },
      getChildIndex(child){
        descriptor(child);
        const k=referenceOf(child).key,idx=stageRefs().findIndex(ref=>refKey(ref)===k);
        if(idx<0){const err=new RangeError('Object is not a direct child of Stage.');err.name='ArgumentError';throw err;}
        return idx;
      },
      setChildIndex(child,index){
        if(!Number.isInteger(index)||index<0)throw new RangeError('Child index requires a nonnegative integer.');
        descriptor(child);stageMutate({op:'set_index',child:referenceOf(child).objectRef,index});
      },
      swapChildren(first,second){
        descriptor(first);descriptor(second);
        stageMutate({op:'swap',first:referenceOf(first).objectRef,second:referenceOf(second).objectRef});
      },
      swapChildrenAt(first,second){
        const a=stageContainer.getChildAt(first),b=stageContainer.getChildAt(second);
        stageMutate({op:'swap',first:referenceOf(a).objectRef,second:referenceOf(b).objectRef});
      },
      contains(child){
        if(child===getStage())return true;
        return descriptor(child).on_stage!==false;
      },
      contentSize(){
        let box=null;
        for(const ref of stageRefs()){
          const r=bounds(proxyForRef(ref),null,true);
          if(r.width<=0&&r.height<=0)continue;
          const next={left:r.x,top:r.y,right:r.x+r.width,bottom:r.y+r.height};
          box=box?{left:Math.min(box.left,next.left),top:Math.min(box.top,next.top),right:Math.max(box.right,next.right),bottom:Math.max(box.bottom,next.bottom)}:next;
        }
        return box?{width:box.right-box.left,height:box.bottom-box.top}:{width:0,height:0};
      },
    };
    function legacyDepth(value) {
      if(!Number.isInteger(value)||value<0||value>1048575)throw new RangeError('AS2 runtime depth requires an integer from 0 through 1048575.');
      return value;
    }
    function placeAtDepth(parent,child,depth,deliver=true) {
      requireContainer(parent);descriptor(child);
      const packet=JSON.parse(player.as2_place_depth(referenceOf(parent).key,referenceOf(child).key,legacyDepth(depth)));
      if(deliver)onMutation(packet.events);return packet.events;
    }
    function initValues(value) {
      const result=[];
      if(value!==null&&typeof value==='object')for(const [key,item]of Object.entries(Object.getOwnPropertyDescriptors(value))){
        if(!item.enumerable)continue;
        if(!Object.hasOwn(item,'value'))throw new TypeError('AS2 initObject requires ordinary enumerable data properties; accessors are unsupported.');
        if(result.length>=1024)throw new RangeError('AS2 initObject exceeds 1024 properties.');
        result.push([key,item.value]);
      }
      return result;
    }
    function initializeLegacy(clip,values) {
      for(const [key,value]of values){
        if(callbackNames.has(key))setLegacyCallback(clip,key,value);
        else {const ordinary=referenceOf(clip)?.as2TimelineRef?.(key);if(ordinary)ordinary.set(value);else clip[key]=value;}
      }
    }
    function targetPath(target) {descriptor(target);return player.as2_target_path(referenceOf(target).key,false);}
    function resolveTarget(value,relative=proxyFor([]),legacyCase=false) {
      if(typeof value!=='string'){descriptor(value);return value;}
      descriptor(relative);
      const ref=JSON.parse(
        legacyCase&&typeof player.as2_resolve_target_case==='function'
          ? player.as2_resolve_target_case(referenceOf(relative).key,value,true)
          : player.as2_resolve_target(referenceOf(relative).key,value)
      );
      return ref?proxyForRef(ref):undefined;
    }
    function legacyDrawing(target) {
      const drawing=graphics(target);
      return {
        setMask:(...args)=>{
          descriptor(target);
          if(!args.length)return undefined;
          const mask=args[0];
          if(typeof mask==='string')throw new TypeError('AS2 setMask string target paths are not supported; pass a live display object or null.');
          // Pinned Ruffle AVM1 set_mask returns Boolean success, despite the
          // Adobe reference's Void annotation. Missing arguments are a no-op.
          if(mask!=null&&!referenceOf(mask))return false;
          set(target,'mask',mask==null?null:mask);return true;
        },
        beginFill:(color,alpha=100)=>drawing.beginFill(color,alpha/100),
        beginBitmapFill:(bitmap,matrix=null,repeat=true,smoothing=false)=>drawing.beginBitmapFill(bitmap,matrix??null,Boolean(repeat),Boolean(smoothing)),
        beginGradientFill:(type,colors,alphas,ratios,matrix,spread='pad',interpolation='RGB',focal=0)=>drawing.beginGradientFill(type,colors,graphicsNumbers(alphas,'AS2 alphas',15).map(alpha=>alpha/100),ratios,matrix,spread,interpolation==='RGB'?'rgb':interpolation,focal),
        lineGradientStyle:(type,colors,alphas,ratios,matrix,spread='pad',interpolation='RGB',focal=0)=>drawing.lineGradientStyle(type,colors,graphicsNumbers(alphas,'AS2 alphas',15).map(alpha=>alpha/100),ratios,matrix,spread,interpolation==='RGB'?'rgb':interpolation,focal),
        endFill:drawing.endFill,lineStyle:(width,color=0,alpha=100,...options)=>drawing.lineStyle(width,color,alpha/100,...options),
        moveTo:drawing.moveTo,lineTo:drawing.lineTo,curveTo:drawing.curveTo,clear:drawing.clear,
        getTextSnapshot:()=>{const host=getAS2Text();if(!host)throw new Error('AS2 text snapshot service is missing; re-export the project.');descriptor(target);return host.snapshotFor(referenceOf(target).key);},
        getNextHighestDepth:()=>{requireContainer(target);return player.as2_next_depth(referenceOf(target).key);},
        getInstanceAtDepth:depth=>{
          requireContainer(target);if(!Number.isInteger(depth)||depth< -16384||depth>1048575)throw new RangeError('AS2 lookup depth requires an integer from -16384 through 1048575.');
          const ref=JSON.parse(player.as2_child_at_depth(referenceOf(target).key,depth));return ref?proxyForRef(ref):undefined;
        },
        swapDepths:value=>{
          let depth=value;
          if(typeof value!=='number'){
            const other=resolveTarget(value,parentSpace(target));
            if(!other)throw new ReferenceError('swapDepths target path was not found.');
            if(parentSpace(other)!==parentSpace(target))throw new TypeError('swapDepths requires siblings in the same container.');
            depth=player.as2_get_depth(referenceOf(other).key);
          }
          player.as2_swap_depth(referenceOf(target).key,legacyDepth(depth));
        },
        duplicateMovieClip:(name,depth,initObject)=>{
          descriptor(target);if(typeof name!=='string')throw new TypeError('duplicateMovieClip requires a string instance name.');
          const values=initValues(initObject),packet=JSON.parse(player.as2_duplicate(referenceOf(target).key,name,legacyDepth(depth)));
          const clip=proxyForRef(packet.object_ref);initializeLegacy(clip,values);onMutation(packet.events);return clip;
        },
        attachBitmap:(data,depth,pixelSnapping='auto',smoothing=false)=>{
          legacyDepth(depth);
          const image=new builtins['flash.display.Bitmap'](data,pixelSnapping,smoothing);placeAtDepth(target,image,depth);
        },
        createTextField:(name,depth,x,y,width,height)=>{
          if(typeof name!=='string'||new TextEncoder().encode(name).length>4096)throw new TypeError('createTextField requires a name of at most 4096 UTF-8 bytes.');
          legacyDepth(depth);
          if(![x,y,width,height].every(Number.isFinite)||width<0||height<0)throw new RangeError('createTextField requires finite coordinates and nonnegative box dimensions.');
          const text=new builtins['flash.text.TextField']();text.name=name;text.x=x;text.y=y;text.width=width;text.height=height;
          placeAtDepth(target,text,depth);return text;
        },
        createEmptyMovieClip:(name,depth)=>{
          if(name===undefined||name===null)name=String(name);
          if(typeof name!=='string'||new TextEncoder().encode(name).length>4096)throw new TypeError('createEmptyMovieClip requires a name of at most 4096 UTF-8 bytes.');
          legacyDepth(depth);
          const clip=proxyForRef(JSON.parse(player.create_display('movie_clip')));clip.name=name;
          placeAtDepth(target,clip,depth);return clip;
        },
        removeMovieClip:()=>{
          const node=descriptor(target);
          if(node.authored_path!=null||node.source_path?.length)throw new TypeError('AS2 removeMovieClip cannot remove authored timeline placements.');
          const parent=parentSpace(target);if(parent&&parent!==getStage())containerMethods(parent).removeChild(target);
        },
        attachMovie:(identifier,name,depth,initObject)=>{
          requireContainer(target);
          if(typeof identifier!=='string'||!identifier)throw new TypeError('attachMovie requires the exact Export identifier of a MovieClip symbol.');
          if(typeof name!=='string'||new TextEncoder().encode(name).length>4096)throw new TypeError('attachMovie requires a name of at most 4096 UTF-8 bytes.');
          legacyDepth(depth);
          const values=initValues(initObject);
          const clip=proxyForRef(JSON.parse(player.create_linked(identifier)));clip.name=name;
          initializeLegacy(clip,values);
          const events=placeAtDepth(target,clip,depth,false);
          prepareLinked(clip,false,identifier);onMutation(events);return clip;
        },
      };
    }
    // Input targets use the authoritative graph and raw geometry, independently
    // of rendered ColorTransform alpha. Filters never add hit geometry.
    const inputNames={mouseEnabled:'mouse_enabled',mouseChildren:'mouse_children',buttonMode:'button_mode',useHandCursor:'use_hand_cursor',hitArea:'hit_area',doubleClickEnabled:'double_click_enabled',tabEnabled:'tab_enabled',tabIndex:'tab_index',tabChildren:'tab_children',focusRect:'focus_rect'};
    const dragOwners=new Set(['sprite','movie_clip','graphic']);
    const existingCallbackNames=new Set(['onEnterFrame','onPress','onRelease','onReleaseOutside','onRollOver','onRollOut']);
    const buttonCallbacks=new Set(['onPress','onRelease','onReleaseOutside','onRollOver','onRollOut','onDragOver','onDragOut']);
    const callbackNames=new Set([...existingCallbackNames,'onMouseDown','onMouseMove','onMouseUp','onKeyDown','onKeyUp','onSetFocus','onKillFocus','onDragOver','onDragOut','onUnload']);
    const as2InputNames={enabled:'enabled',focusEnabled:'focus_enabled',tabEnabled:'tab_enabled',tabIndex:'tab_index',tabChildren:'tab_children',trackAsMenu:'track_as_menu',_focusrect:'focus_rect',forceSmoothing:'force_smoothing',_lockroot:'lock_root'};
    let as2TabActive=false,pressedInside=false;
    function as2Settings(request={op:'get'}){return JSON.parse(player.as2_settings_command(JSON.stringify(request)));}
    function as2GlobalPropRef(name,check=null){
      if(!['_quality','_highquality','_focusrect','_soundbuftime'].includes(name))return null;
      return {
        has:()=>true,
        delete:()=>false,
        get:()=>{
          check?.();
          const current=as2Settings();
          return name==='_quality'?current.quality:name==='_highquality'?current.high_quality:name==='_soundbuftime'?current.sound_buf_time:current.focus_rect;
        },
        set:value=>{
          check?.();
          if(name==='_quality')as2Settings({op:'set_quality',value:String(value)});
          else if(name==='_highquality')as2Settings({op:'set_high_quality',value:Number(value)});
          else if(name==='_soundbuftime')as2Settings({op:'set_sound_buf_time',value:Number(value)});
          else as2Settings({op:'set_focus_rect',value:Boolean(value)});
          return value;
        },
      };
    }
    function toggleHighQuality(){return as2Settings({op:'toggle_high_quality'}).high_quality;}
    function showRedrawRegions(enable,color){
      const request={op:'show_redraw_regions',enable:Boolean(enable)};
      if(color!==undefined){
        const numeric=Number(color);
        if(!Number.isInteger(numeric)||numeric<0||numeric>0xffffff)throw new RangeError('showRedrawRegions color requires a 24-bit RGB integer.');
        request.color=numeric;
      }
      as2Settings(request);
    }
    function as2Input(target,request={op:'get'}){descriptor(target);return JSON.parse(player.as2_input_command(referenceOf(target).key,JSON.stringify(request)));}
    function buttonLike(target){const ref=referenceOf(target),record=ref&&legacy.get(ref.key+'\u0000'+ref.lifetime);return [...buttonCallbacks].some(name=>hasCallback(record,name))||Boolean(record?.authoredButton?.some(entry=>entry.event==='keyPress'));}
    function as2CanFocus(target){if(target===null)return true;const ok=as2Input(target,{op:'focusable',has_button_handler:buttonLike(target)});if(ok)as2TabActive=true;return ok;}
    function as2Ref(target,name){
      const ordinary=referenceOf(target)?.as2TimelineRef?.(name);if(ordinary)return ordinary;
      if(name==='_accProps'&&['movie_clip','button','text'].includes(descriptor(target).owner))return {has:()=>accProps.has(target),delete:()=>accProps.delete(target),get:()=>accProps.get(target),set:value=>{if(value===undefined)accProps.delete(target);else accProps.set(target,value);return value;}};
      if((name==='width'||name==='height')&&descriptor(target).owner==='video')return {has:()=>true,delete:()=>false,get:()=>getVideo().get(target,name==='width'?'videoWidth':'videoHeight'),set:()=>{throw new TypeError('AS2 Video.width/height report decoded dimensions; use _width/_height to resize.');}};
      if(name==='getSWFVersion'&&descriptor(target).owner==='movie_clip')return {has:()=>true,delete:()=>false,get:()=>()=>{descriptor(target);return 8;},set:()=>{throw new TypeError('MovieClip.getSWFVersion is read-only.');}};
      if(['_quality','_highquality','_soundbuftime'].includes(name)&&['movie_clip','button','text'].includes(descriptor(target).owner))return as2GlobalPropRef(name,()=>descriptor(target));
      if(['loadMovie','unloadMovie','getBytesLoaded','getBytesTotal'].includes(name)&&descriptor(target).owner==='movie_clip'){
        const method=()=>{const loader=getLoader();if(!loader)throw new Error('MovieClip image methods require the publication Loader service.');return loader.as2MovieMethods(target)[name];};
        return {has:()=>true,delete:()=>false,get:method,set:()=>{throw new TypeError('MovieClip image methods are read-only.');}};
      }
      if(['blendMode','cacheAsBitmap','opaqueBackground','scale9Grid'].includes(name)){
        const owner=descriptor(target).owner;if(!['movie_clip','button'].includes(owner)||(name==='opaqueBackground'&&owner!=='movie_clip'))return null;
        const call=request=>JSON.parse(player.as2_render_command(referenceOf(target).key,JSON.stringify(request)));
        if(name==='scale9Grid'){
          return {
            has:()=>true,
            delete:()=>{call({op:'set',name,value:null});return true;},
            get:()=>{const grid=call({op:'get'}).scale9Grid;return grid?new Rectangle(grid[0],grid[1],grid[2],grid[3]):undefined;},
            set:value=>{
              if(value!==null&&value!==undefined&&!Rectangle.__nfIs(value))throw new TypeError('scale9Grid requires a flash.geom.Rectangle, null or undefined.');
              call({op:'set',name,value:value==null?null:{x:value.x,y:value.y,width:value.width,height:value.height}});
              return value;
            },
          };
        }
        return {has:()=>true,delete:()=>false,get:()=>call({op:'get'})[name],set:value=>{const next=call({op:'prepare',name,value});if(next.isolated)beforeRaster(target);call({op:'set',name,value});return value;}};
      }
      if(!Object.hasOwn(as2InputNames,name)&&!callbackNames.has(name))return null;
      const owner=descriptor(target).owner;if(!['movie_clip','button','text'].includes(owner))return null;
      if((name==='forceSmoothing'||name==='_lockroot')&&owner!=='movie_clip')return null;
      if(callbackNames.has(name)){
        if(owner==='text')return null;
        return {has:()=>true,delete:()=>{setLegacyCallback(target,name,null);return true;},get:()=>target[name],set:value=>{setLegacyCallback(target,name,value);return value;}};
      }
      if((owner==='text'&&['enabled','focusEnabled','tabChildren','trackAsMenu','_focusrect'].includes(name))||(name==='focusEnabled'&&owner!=='movie_clip'))return null;
      return {has:()=>true,delete:()=>{as2Input(target,{op:'set',name,value:null});return true;},get:()=>{const value=as2Input(target)[as2InputNames[name]];return value===null?undefined:value;},set:value=>{const payload=name==='_lockroot'?Boolean(value):(value===undefined?null:value);as2Input(target,{op:'set',name,value:payload});if(name==='enabled'&&owner==='button'&&!as2Input(target).enabled){if(buttonOver===target){buttonState(target,0);buttonOver=null;}if(buttonPressed===target){buttonState(target,0);buttonPressed=null;}buttonState(target,0);}if(name!=='_lockroot')as2TabActive=true;return value;}};
    }
    function as2Tab(event={}){
      if(!as2TabActive||event.defaultPrevented)return false;
      const button_targets=[];for(const record of legacy.values())if(alive(record.target)&&buttonLike(record.target))button_targets.push(record.ref.objectRef);
      const refs=as2Input(proxyFor([]),{op:'tab_order',button_targets});if(!refs.length)return false;
      const stage=getStage(),current=stage.focus,position=current?refs.findIndex(ref=>refKey(ref)===referenceOf(current)?.key):-1;
      const next=position<0?(event.shiftKey?refs.length-1:0):(position+(event.shiftKey?-1:1)+refs.length)%refs.length;
      event.preventDefault?.();stage.focus=proxyForRef(refs[next]);return true;
    }

    let dragging=null,pressed=null,hovered=null;
    const drops=new WeakMap();
    let contextMenus=new WeakMap(),accessImpls=new WeakMap(),buttonRecords=new WeakMap(),softKeyboardNeeds=new WeakMap(),softKeyboardRects=new WeakMap();
    function buttonRecord(target){let rec=buttonRecords.get(target);if(!rec){rec={upState:null,overState:null,downState:null,hitTestState:null,volume:1,pan:0};buttonRecords.set(target,rec);}return rec;}
    function ownsInputProperty(owner,name){
      if(name==='mouseChildren'||name==='tabChildren')return containerOwners.includes(owner);
      if(name==='buttonMode'||name==='hitArea')return spriteOwners.includes(owner);
      if(name==='useHandCursor')return spriteOwners.includes(owner)||owner==='button';
      return interactiveOwners.includes(owner);
    }
    function inputOf(node){
      const defaults={mouse_enabled:true,mouse_children:true,button_mode:node.owner==='button',movieclip_enabled:true,use_hand_cursor:true,hit_area:null,double_click_enabled:false,tab_enabled:false,tab_index:-1,tab_children:true,focus_rect:null,button_enabled:true,track_as_menu:false};
      if(Object.hasOwn(node,'input'))return node.input?{...defaults,...node.input}:null;
      return interactiveOwners.includes(node.owner)?defaults:null;
    }
    function nativeInput(target){const node=descriptor(target);if(!inputOf(node))throw new TypeError('This display object is not interactive.');return inputOf(node);}
    function inputWrite(target,name,value){
      const node=descriptor(target);
      if(name!=='enabled'&&name!=='button_enabled'&&name!=='track_as_menu'&&!ownsInputProperty(node.owner,name))throw new TypeError(`${name} is not supported on this display object.`);
      const input=nativeInput(target);
      const key=name==='enabled'?'movieclip_enabled':name==='button_enabled'?'button_enabled':name==='track_as_menu'?'track_as_menu':inputNames[name];
      let normalized=value;
      if(name==='hitArea'){if(value!==null)descriptor(value);normalized=value===null?null:referenceOf(value).objectRef;}
      else if(name==='tabIndex'){if(typeof value!=='number'||!Number.isFinite(value))throw new TypeError('tabIndex requires a finite integer.');normalized=value|0;}
      else if(name==='focusRect'){if(value!==null&&typeof value!=='boolean')throw new TypeError('focusRect requires Boolean or null.');normalized=value;}
      else if(typeof value!=='boolean')throw new TypeError(`${name} requires a Boolean.`);
      const previous=input[key];
      try{player.object_input_update(referenceOf(target).key,JSON.stringify({...input,[key]:normalized}));}catch(error){if(error instanceof Error)throw error;throw new Error(String(error),{cause:error});}
      if(previous!==normalized){
        if(name==='tabEnabled')onTabChange(target,'tabEnabledChange');
        else if(name==='tabIndex')onTabChange(target,'tabIndexChange');
        else if(name==='tabChildren')onTabChange(target,'tabChildrenChange');
      }
    }
    function nodeShapes(node){if(node.text)return[{type:'text_field',field:node.text}];if(node.video)return[{type:'video_frame',...node.video}];if(node.bitmap&&!node.bitmap.disposed)return[{type:'bitmap_pixels',...node.bitmap,smoothing:node.smoothing,pixel_snapping:'never'}];if(node.fte_draws?.length)return node.fte_draws.map(r=>({type:'fte_run',...r}));if(node.owner==='text_line'){const tb=getFte3D()?.textLineBounds?.(nodeProxy(node));if(tb)return[{type:'fte_run',x:tb.x,y:tb.y,width:tb.width,font_size:tb.height/1.2}];}return node.kind?[node.kind]:[];}
    function leafHit(node,x,y){
      let q;try{q=point(inverse(node.world),x,y);}catch(_){return false;}
      for(const kind of [...nodeShapes(node),...(node.graphics||[])]){
        if(kind.type==='rectangle'){const stroke=kind.stroke_width>0&&kind.stroke?.[3]>0?kind.stroke_width/2:0;const inside=q.x>=0&&q.y>=0&&q.x<=kind.width&&q.y<=kind.height,outer=q.x>=-stroke&&q.y>=-stroke&&q.x<=kind.width+stroke&&q.y<=kind.height+stroke,inner=q.x>stroke&&q.y>stroke&&q.x<kind.width-stroke&&q.y<kind.height-stroke;if((kind.fill?.[3]>0&&inside)||(stroke&&outer&&!inner))return true;}
        else if(kind.type==='fte_run'){const w=kind.width||kind.text.length*kind.font_size*0.6,h=kind.font_size*1.2;if(q.x>=kind.x&&q.x<=kind.x+w&&q.y>=kind.y&&q.y<=kind.y+h)return true;}
        else if(kind.type==='ellipse'){const stroke=kind.stroke_width>0&&kind.stroke?.[3]>0?kind.stroke_width/2:0;if(kind.width>0&&kind.height>0){const ellipse=(rx,ry)=>rx>0&&ry>0&&Math.pow((q.x-kind.width/2)/rx,2)+Math.pow((q.y-kind.height/2)/ry,2)<=1;if((kind.fill?.[3]>0&&ellipse(kind.width/2,kind.height/2))||(stroke&&ellipse(kind.width/2+stroke,kind.height/2+stroke)&&!ellipse(kind.width/2-stroke,kind.height/2-stroke)))return true;}}
        else if(kind.type==='text_field'){const b=kind.field.box?{left:0,top:0,right:kind.field.box.width,bottom:kind.field.box.height}:getText().bounds({...kind.field,alpha:255},true);if(b&&q.x>=b.left&&q.y>=b.top&&q.x<=b.right&&q.y<=b.bottom)return true;}
        else if(kind.type==='text'){const lines=kind.text.split('\n');if(q.x>=0&&q.y>=0&&q.y<=lines.length*kind.font_size*1.2&&q.x<=Math.max(0,...lines.map(line=>measureText(line,kind))))return true;}
        else if(kind.type==='bitmap_pixels'){if(getBitmap()?.cache.hit(kind,q.x,q.y))return true;}
        else if(kind.type==='video_frame'){if(getVideo()?.get(nodeProxy(node),'videoWidth')>0&&q.x>=0&&q.y>=0&&q.x<kind.width&&q.y<kind.height)return true;}
        else if(kind.type==='path'||kind.type==='gradient_path'){if(!shapeContains)throw new Error('Shape hit testing requires the Canvas Path2D service.');if(shapeContains(kind,q.x,q.y))return true;}
      }
      return false;
    }
    function buttonHit(node,x,y) {
      return (node.button_hits||[]).some(item=>leafHit({...node,world:multiply(node.world,item.transform),kind:item.kind,graphics:[],text:null,video:null,bitmap:null},x,y));
    }
    function shapeHit(target,x,y,snapshot=JSON.parse(player.runtime_snapshot_json()),respectVisibility=false){
      const list=snapshot.objects,source=descriptor(target,list);
      for(const node of list)if(descendant(node,source,list)&&(!respectVisibility||node.effective_visible)&&(!node.clip_ids?.length||clipsContain?.(snapshot,node.clip_ids,x,y))&&leafHit(node,x,y))return true;
      return false;
    }
    function resolveInput(target,list=nodes(),x=null,y=null){
      if(!target||target===getStage())return null;
      let chosen=null;
      for(let node=descriptor(target,list);node;node=parentNode(node,list)){
        if(node.on_stage===false||node.effective_visible===false)return null;
        const input=inputOf(node);if(!input)continue;
        if(node.owner==='button'&&node.button_hits?.length&&x!==null&&!buttonHit(node,x,y))return null;
        if(node.owner==='button'&&input.mouse_enabled)chosen=node;
        if(!chosen&&input.mouse_enabled)chosen=node;
        if(input.mouse_children===false&&nodeKey(node)!==referenceOf(target).key){if(!input.mouse_enabled)return null;chosen=node;}
      }
      if(!chosen)return null;
      if(inputOf(chosen).hit_area&&x!==null&&!shapeHit(proxyForRef(inputOf(chosen).hit_area),x,y))return null;
      return nodeProxy(chosen);
    }
    function inputRoute(owner,list=nodes(),x=null,y=null){
      const target=resolveInput(owner,list,x,y);if(!target)return null;
      let nearest=descriptor(owner,list);while(nearest&&!inputOf(nearest))nearest=parentNode(nearest,list);
      // Authored Click scripts on a noninteractive leaf keep their own source
      // identity; retargeting an interactive branch must not bypass its flags.
      const authored=nearest&&nodeKey(nearest)===referenceOf(target).key?owner:target;
      return {owner:authored,target};
    }
    function pickDetails(x,y,exclude=null){
      if(!Number.isFinite(x)||!Number.isFinite(y))return null;
      const snapshot=JSON.parse(player.runtime_snapshot_json()),list=snapshot.objects,painted=new Set(snapshot.draws.map(item=>refKey(item.owner_ref))),excluded=exclude&&alive(exclude)?descriptor(exclude,list):null;
      // Runtime objects are emitted in display preorder; reversing it tests the
      // foremost child before its container and earlier siblings.
      for(let i=list.length-1;i>=0;i--){const node=list[i];if(node.on_stage===false||node.effective_visible===false||(excluded&&descendant(node,excluded,list)))continue;
        if(node.clip_ids?.length&&!clipsContain?.(snapshot,node.clip_ids,x,y))continue;
        const area=node.input?.hit_area;
        const hit=area?shapeHit(proxyForRef(area),x,y,snapshot):node.owner==='button'&&node.button_hits?.length?buttonHit(node,x,y):painted.has(nodeKey(node))&&leafHit(node,x,y);
        if(!hit)continue;const result=inputRoute(nodeProxy(node),list,x,y);if(result)return result;
      }
      return null;
    }
    function pick(x,y,exclude=null){return pickDetails(x,y,exclude)?.target||null;}
    function hitTestPoint(target,x,y,shape=false){if(!Number.isFinite(x)||!Number.isFinite(y)||typeof shape!=='boolean')throw new TypeError('hitTestPoint requires finite Stage coordinates and a Boolean shapeFlag.');if(shape)return shapeHit(target,x,y);const b=bounds(target,null,true);return b.width>0&&b.height>0&&x>=b.x&&y>=b.y&&x<=b.x+b.width&&y<=b.y+b.height;}
    function hitTestObject(target,other){descriptor(other);const a=bounds(target,null,true),b=bounds(other,null,true);return a.width>0&&a.height>0&&b.width>0&&b.height>0&&a.x<b.x+b.width&&b.x<a.x+a.width&&a.y<b.y+b.height&&b.y<a.y+a.height;}
    function mousePoint(target){const stage=getStage();return convert(target,{x:stage?.mouseX||0,y:stage?.mouseY||0},false);}
    function dragPoint(target){const parent=parentSpace(target),stage=getStage();return !parent||parent===stage?{x:stage?.mouseX||0,y:stage?.mouseY||0}:convert(parent,{x:stage?.mouseX||0,y:stage?.mouseY||0},false);}
    function startDrag(target,lockCenter=false,rect=null){const node=descriptor(target);if(!dragOwners.has(node.owner))throw new TypeError('startDrag requires Sprite or MovieClip.');if(!node.on_stage)throw new Error('startDrag requires an attached display object.');if(typeof lockCenter!=='boolean')throw new TypeError('startDrag lockCenter requires a Boolean.');let box=null;if(rect!==null){if(!Rectangle.__nfIs(rect))throw new TypeError('startDrag bounds require Rectangle or null.');const values=[rect.x,rect.y,rect.width,rect.height];if(!values.every(Number.isFinite)||values.some(v=>Math.abs(v)>1e12)||rect.width<0||rect.height<0)throw new RangeError('Drag bounds require finite coordinates within ±1e12 and nonnegative size.');box={x:rect.x,y:rect.y,width:rect.width,height:rect.height};}const p=dragPoint(target);dragging={target,parent:refKey(node.parent_ref),offsetX:lockCenter?0:node.local_matrix[4]-p.x,offsetY:lockCenter?0:node.local_matrix[5]-p.y,box};updateDrag();}
    function updateDrag(){if(!dragging)return;const d=dragging;if(!alive(d.target)){dragging=null;return;}const node=descriptor(d.target);if(node.on_stage===false||node.effective_visible===false||refKey(node.parent_ref)!==d.parent){dragging=null;return;}const p=dragPoint(d.target);let x=p.x+d.offsetX,y=p.y+d.offsetY;if(d.box){x=Math.max(d.box.x,Math.min(d.box.x+d.box.width,x));y=Math.max(d.box.y,Math.min(d.box.y+d.box.height,y));}if(!Number.isFinite(x)||!Number.isFinite(y)||Math.abs(x)>1e12||Math.abs(y)>1e12)throw new RangeError('Drag coordinates exceed the finite display range.');if(!invoke(d.target,'set_property','x',x)||!invoke(d.target,'set_property','y',y))throw new Error('The runtime rejected movement of this drag target.');drops.set(d.target,pick(getStage().mouseX,getStage().mouseY,d.target));}
    function stopDrag(target){if(dragging){updateDrag();dragging=null;}}
    function legacyPath(target){if(!target||!alive(target))return '';const names=[];for(let current=target;current&&current!==getStage();current=parentSpace(current)){const node=descriptor(current);if(!node.parent_ref)break;if(!node.name||/[/.]/.test(node.name))throw new TypeError('AS2 _droptarget paths require named placements without slash or dot; use dropTarget for stable object identity.');names.push(node.name);}return '/'+names.reverse().join('/');}
    function callbackOwner(target,names){while(target&&target!==getStage()&&alive(target)){const ref=referenceOf(target),record=legacy.get(ref.key+'\u0000'+ref.lifetime);if(as2Input(target).enabled&&names.some(name=>hasCallback(record,name)))return target;target=parentSpace(target);}return null;}
    function callLegacy(target,name,args=[]){
      if(!target||!alive(target))return;
      if(buttonCallbacks.has(name)&&!as2Input(target).enabled)return;
      const ref=referenceOf(target),record=legacy.get(ref.key+'\u0000'+ref.lifetime);
      for(const fn of authoredHandlersFor(record,name)){
        if(alive(target))Reflect.apply(fn,authoredReceiver(target),args);
      }
      const fn=callback(record,name);
      if(fn&&alive(target))Reflect.apply(fn,target,args);
    }
    let buttonOver=null,buttonPressed=null;
    function authoredButton(target){for(let current=target;current&&current!==getStage()&&alive(current);current=parentSpace(current))if(descriptor(current).owner==='button'&&as2Input(current).enabled&&nativeInput(current).button_enabled!==false)return current;return null;}
    function buttonState(target,state){if(target&&alive(target))player.object_button_state?.(referenceOf(target).key,state);}
    function movieClipPointer(type,target){
      if(typeof player.object_movieclip_pointer!=='function')throw new Error('Rust MovieClip button-state services are unavailable; rebuild and re-export this project.');
      const key=target&&target!==getStage()&&alive(target)?referenceOf(target).key:'null';
      try{return player.object_movieclip_pointer(type,key);}
      catch(error){throw error instanceof Error?error:new Error(String(error),{cause:error});}
    }
    function pointer(type,target,event={}){
      prune();
      // Rust owns MovieClip eligibility, pointer ownership and label selection.
      // This runs alongside the existing authored Button and AS2 callback paths;
      // enabled=false never suppresses ordinary mouse or keyboard dispatch.
      if(type==='mouseMove'||((type==='mouseDown'||type==='mouseUp')&&(event.button===undefined||event.button===0)))movieClipPointer(type,target);
      if(['mouseMove','mouseDown','mouseUp'].includes(type)){
        const next=authoredButton(target);
        if(buttonOver!==next){buttonState(buttonOver,0);buttonOver=next;}
        if(type==='mouseMove'&&buttonPressed&&next&&buttonPressed!==next&&as2Input(buttonPressed,{op:'menu_transfer',target:referenceOf(next).objectRef})){buttonState(buttonPressed,0);buttonPressed=next;}
        if(type==='mouseDown'&&(event.button===undefined||event.button===0))buttonPressed=next;
        if(type==='mouseUp'&&(event.button===undefined||event.button===0)){buttonState(buttonPressed,buttonPressed===next?1:0);buttonPressed=null;}
        buttonState(next,buttonPressed===next?2:buttonPressed?0:1);
      }
      if(type==='click'&&target&&target!==getStage()&&descriptor(target).owner==='text'){
        if(!isStaticTextNode(descriptor(target))){
          const pt=mousePoint(target),hit=getText()?.linkAt?.(referenceOf(target).key,pt);
          if(hit&&/^event:/i.test(hit.url)){onTextLink?.(target,hit.url.slice(6));return;}
        }
        getAS2Text()?.link(referenceOf(target).key,mousePoint(target),event);return;
      }
      if(type==='mouseWheel'&&target&&target!==getStage()&&descriptor(target).owner==='text'){getAS2Text()?.wheel(referenceOf(target).key,event);return;}
      if(type==='keyDown' ||type==='keyUp'){
        if(target&&target!==getStage())callLegacy(target,type==='keyDown'?'onKeyDown':'onKeyUp');
        for(const record of [...legacy.values()]){
          if(!alive(record.target))continue;
          const node=descriptor(record.target);
          if(node.on_stage===false)continue;
          const clipHandlers=record.authoredClip?.get(type);
          if(clipHandlers){
            for(const fn of [...clipHandlers]){
              if(alive(record.target))Reflect.apply(fn,authoredReceiver(record.target),[]);
            }
          }
          if(type==='keyDown'&&record.authoredButton?.length&&node.effective_visible!==false&&as2Input(record.target).enabled&&(node.owner==='button'?nativeInput(record.target).button_enabled!==false:nativeInput(record.target).movieclip_enabled!==false)){
            for(const entry of [...record.authoredButton]){
              if(entry.event==='keyPress'&&matchFlashKeyPress(entry.key,event)&&alive(record.target)){
                Reflect.apply(entry.fn,authoredReceiver(record.target),[]);
              }
            }
          }
        }
        if(type==='keyDown'&&(event.key==='Tab'||event.keyCode===9))as2Tab(event);return;
      }
      const broadcast={mouseDown:'onMouseDown',mouseMove:'onMouseMove',mouseUp:'onMouseUp'}[type];
      if(broadcast)for(const record of [...legacy.values()])if(alive(record.target)&&descriptor(record.target).on_stage)callLegacy(record.target,broadcast);
      if(type==='mouseMove'&&pressed&&alive(pressed)){
        const next=callbackOwner(target,[...buttonCallbacks]);
        if(next&&next!==pressed&&as2Input(pressed,{op:'menu_transfer',target:referenceOf(next).objectRef})){
          const old=pressed,wasInside=pressedInside;pressed=next;pressedInside=true;if(wasInside)callLegacy(old,'onDragOut');if(pressed===next&&alive(next))callLegacy(next,'onDragOver');
        }
        if(!pressed||!alive(pressed)){pressed=null;pressedInside=false;return;}
        const inside=Boolean(target&&target!==getStage()&&alive(target)&&descendant(descriptor(target),descriptor(pressed),nodes()));
        if(inside!==pressedInside){pressedInside=inside;callLegacy(pressed,inside?'onDragOver':'onDragOut');}
      }
      if(type==='mouseMove'||type==='mouseDown'){const next=callbackOwner(target,[...buttonCallbacks]);if(next!==hovered){const old=hovered;hovered=next;callLegacy(old,'onRollOut');if(hovered===next)callLegacy(next,'onRollOver');}}
      if(type==='mouseDown'&&(event.button===undefined||event.button===0)){pressed=callbackOwner(target,[...buttonCallbacks]);pressedInside=Boolean(pressed);callLegacy(pressed,'onPress');}
      if(type==='mouseUp'&&(event.button===undefined||event.button===0)){const old=pressed;pressed=null;pressedInside=false;if(old&&alive(old)){const inside=target&&target!==getStage()&&alive(target)&&descendant(descriptor(target),descriptor(old),nodes());callLegacy(old,inside?'onRelease':'onReleaseOutside');}}
    }
    function cancelPointer(){const changed=movieClipPointer('cancel',null);buttonState(buttonOver,0);buttonState(buttonPressed,0);buttonOver=null;buttonPressed=null;dragging=null;pressed=null;hovered=null;pressedInside=false;return changed;}
    function as2FocusBounds(){
      const stage=getStage(),target=stage?.focus;
      if(!target||target===stage||!alive(target))return null;
      const node=descriptor(target);
      if(as2TabActive&&['movie_clip','button'].includes(node.owner)){
        const pref=as2Input(target).focus_rect,globalPref=typeof player.as2_settings_command==='function'&&as2Settings().focus_rect;
        if((pref??globalPref)!==true)return null;
        const rect=bounds(target,stage,true);
        return {x:rect.x,y:rect.y,width:rect.width,height:rect.height};
      }
      if(!as2TabActive&&interactiveOwners.includes(node.owner)&&!isStaticTextNode(node)){
        const objPref=inputOf(node)?.focus_rect;
        const stagePref=stage?.stageFocusRect??true;
        if((objPref??stagePref)!==true)return null;
        const rect=bounds(target,stage,true);
        return {x:rect.x,y:rect.y,width:rect.width,height:rect.height};
      }
      return null;
    }
    function handCursor(target){for(let current=target;current&&current!==getStage()&&alive(current);current=parentSpace(current)){const node=descriptor(current),ref=referenceOf(current),record=legacy.get(ref.key+'\u0000'+ref.lifetime);if(node.input?.use_hand_cursor&&as2Input(current).enabled&&(node.owner!=='movie_clip'||node.input.movieclip_enabled!==false)&&(node.owner!=='button'||node.input.button_enabled!==false)&&(node.input.button_mode||[...buttonCallbacks].some(name=>hasCallback(record,name))))return true;}return false;}
    function methods(target) {
      const result={
        getDepth:()=>{descriptor(target);return player.as2_get_depth(referenceOf(target).key);},
        hitTestPoint:(x,y,shape=false)=>hitTestPoint(target,x,y,shape),hitTestObject:other=>hitTestObject(target,other),
        hitTest:(...args)=>args.length===1?hitTestObject(target,args[0]):hitTestPoint(target,args[0],args[1],args[2]??false),
        getBounds: space=>bounds(target,space===undefined?target:space,true),
        getRect: space=>bounds(target,space===undefined?target:space,false),
        localToGlobal: value=>convert(target,value,true),
        globalToLocal: value=>convert(target,value,false),
        local3DToGlobal: point3d=>{descriptor(target);const host=getFte3D();if(!host)throw new Error('3D display runtime is missing.');return host.local3DToGlobal(target,point3d);},
        globalToLocal3D: pt=>{descriptor(target);const host=getFte3D();if(!host)throw new Error('3D display runtime is missing.');return host.globalToLocal3D(target,pt);},
      };
      if(interactiveOwners.includes(descriptor(target).owner)&&!isStaticTextNode(descriptor(target))){
        result.requestSoftKeyboard=()=>{descriptor(target);return getFte3D()?Boolean(getFte3D().requestSoftKeyboard(target)):true;};
      }
      if(containerOwners.includes(descriptor(target).owner))Object.assign(result,{
        getChildAt: index=>{requireContainer(target);const list=children(target);if(!Number.isInteger(index)||index<0||index>=list.length)throw new RangeError('Child index is outside this container.');return nodeProxy(list[index]);},
        getChildByName: name=>{requireContainer(target);const child=children(target).find(node=>node.name===name);return child?nodeProxy(child):null;},
        getChildIndex: child=>{requireContainer(target);const node=descriptor(child),index=children(target).findIndex(candidate=>nodeKey(candidate)===nodeKey(node));if(index<0)throw new RangeError('Object is not a direct child of this container.');return index;},
        contains: child=>{requireContainer(target);const parent=descriptor(target),node=descriptor(child);return descendant(node,parent,nodes());},
        getObjectsUnderPoint: pt=>{
          requireContainer(target);
          if(!Point.__nfIs(pt))throw new TypeError('DisplayObjectContainer.getObjectsUnderPoint requires a flash.geom.Point.');
          const px=Number(pt.x),py=Number(pt.y);
          if(!Number.isFinite(px)||!Number.isFinite(py))return [];
          const snapshot=graph?JSON.parse(player.runtime_snapshot_json()):{objects:nodes(),draws:[],clips:[]};
          const list=snapshot.objects,source=descriptor(target,list),matches=[];
          for(const node of list){
            if(!descendant(node,source,list)||node.effective_visible===false)continue;
            if(node.clip_ids?.length&&clipsContain&&!clipsContain(snapshot,node.clip_ids,px,py))continue;
            if(leafHit(node,px,py))matches.push(nodeProxy(node));
          }
          return matches;
        },
        areInaccessibleObjectsUnderPoint: pt=>{
          requireContainer(target);
          if(!Point.__nfIs(pt))throw new TypeError('DisplayObjectContainer.areInaccessibleObjectsUnderPoint requires a flash.geom.Point.');
          return false;
        },
      });
      if(as2) {
        for(const name of ['getBounds','getRect'])as2.register(result[name],target,name,space=>{
          const destination=space===undefined?target:resolveTarget(space,target);
          if(!destination)throw new ReferenceError('AS2 bounds target path was not found.');
          const box=bounds(target,destination,name==='getBounds');return {xMin:box.x,yMin:box.y,xMax:box.x+box.width,yMax:box.y+box.height};
        });
        for(const name of ['localToGlobal','globalToLocal'])as2.register(result[name],target,name,value=>{
          if(value===null||typeof value!=='object')throw new TypeError('AS2 coordinate conversion requires a point object.');
          const position=convert(target,value,name==='localToGlobal');value.x=position.x;value.y=position.y;
        });
      }
      if(dragOwners.has(descriptor(target).owner)){
        result.startDrag=(lockCenter=false,bounds=null)=>startDrag(target,lockCenter,bounds);
        result.stopDrag=()=>stopDrag(target);
        result.startTouchDrag=(touchPointID,lockCenter=false,bounds=null)=>{
          if(!Number.isInteger(Number(touchPointID)))throw new TypeError('Sprite.startTouchDrag requires an integer touchPointID.');
          touchDragId=Number(touchPointID)|0;
          startDrag(target,lockCenter,bounds);
        };
        result.stopTouchDrag=touchPointID=>{
          if(!Number.isInteger(Number(touchPointID)))throw new TypeError('Sprite.stopTouchDrag requires an integer touchPointID.');
          if(touchDragId===null||touchDragId===(Number(touchPointID)|0)){touchDragId=null;stopDrag(target);}
        };
        if(as2)as2.register(result.startDrag,target,'startDrag',(lockCenter=false,...limits)=>{if(limits.length!==0&&limits.length!==4)throw new TypeError('AS2 startDrag requires lockCenter and optional left,top,right,bottom.');let bounds=null;if(limits.length){const[left,top,right,bottom]=limits;if(!limits.every(Number.isFinite)||right<left||bottom<top)throw new RangeError('AS2 drag bounds require ordered finite coordinates.');bounds=new Rectangle(left,top,right-left,bottom-top);}startDrag(target,lockCenter,bounds);});
      }
      if(graph&&containerOwners.includes(descriptor(target).owner))Object.assign(result,containerMethods(target));
      if(graph&&descriptor(target).owner==='movie_clip')Object.assign(result,legacyDrawing(target));
      if(descriptor(target).owner==='movie_clip'){
        result.getURL=(url,window,method)=>{descriptor(target);const net=getNet();if(!net)throw new Error('Browser navigation requires the publication network service.');return net.getURL(target,url,window,method);};
        for(const name of ['loadMovie','unloadMovie','getBytesLoaded','getBytesTotal'])result[name]=(...args)=>{const loader=getLoader();if(!loader)throw new Error('MovieClip image methods require the publication Loader service.');return loader.as2MovieMethods(target)[name](...args);};
      }
      if(graph&&descriptor(target).owner==='loader')Object.assign(result,getLoader().methods(target));
      if(graph&&descriptor(target).owner==='text_line'&&getFte3D())Object.assign(result,getFte3D().textLineMethods(target));
      if(graph&&descriptor(target).owner==='morph_shape'&&getFte3D())Object.assign(result,getFte3D().morphShapeMethods(target));
      if(graph&&descriptor(target).owner==='video'&&getVideo())Object.assign(result,getVideo().methods(target));
      if(descriptor(target).owner==='text'&&!isStaticTextNode(descriptor(target))&&getText()){
        Object.assign(result,getText().methods(referenceOf(target).key));
        result.replaceSel=result.replaceSelectedText;
        result.removeTextField=()=>{
          const node=descriptor(target);
          if(node.authored_path!=null||node.source_path?.length)throw new TypeError('AS2 removeTextField cannot remove authored text fields.');
          const parent=parentSpace(target);
          if(parent&&parent!==getStage())containerMethods(parent).removeChild(target);
          else throw new TypeError('removeTextField requires an attached runtime-created TextField; authored fields cannot be removed.');
        };
      }
      else if(descriptor(target).owner==='text'&&!isStaticTextNode(descriptor(target)))Object.assign(result,{
        appendText:value=>{if(typeof value!=='string')throw new TypeError('appendText currently requires a string.');writeText(target,textValue(textKind(target).text)+value);},
        replaceText:(begin,end,value)=>{
          const text=textValue(textKind(target).text);
          if(!Number.isInteger(begin)||!Number.isInteger(end)||begin<0||end<begin||end>text.length)throw new RangeError('replaceText requires an ordered UTF-16 range within the current text.');
          if(typeof value!=='string')throw new TypeError('replaceText currently requires string replacement text.');
          writeText(target,text.slice(0,begin)+value+text.slice(end));
        },
        getLineText:index=>lineText(target,index),
        getLineLength:index=>lineText(target,index).length,
      });
      return result;
    }
    function get(target,name) {
      if(name==='_target'){descriptor(target);return player.as2_target_path(referenceOf(target).key,true);}
      if(name==='_framesloaded')return descriptor(target).total_frames;
      if(name==='_url'){descriptor(target);const loadedUrl=getLoader()?.urlForDisplay?.(target);if(loadedUrl)return loadedUrl;if(!global.location)throw new TypeError('AS2 _url requires a browser publication URL.');return global.location.href;}
      if(name==='_focusrect'&&['movie_clip','button'].includes(descriptor(target).owner)){const value=as2Input(target).focus_rect;return value===null?undefined:value;}

      if(Object.hasOwn(inputNames,name)){const node=descriptor(target);if(isStaticTextNode(node)||!ownsInputProperty(node.owner,name))return UNHANDLED;const input=nativeInput(target);return name==='hitArea'?(input.hit_area?proxyForRef(input.hit_area):null):input[inputNames[name]];}
      if(name==='accessibilityProperties'){descriptor(target);return accProps.get(target)??null;}
      if(name==='contextMenu'||name==='accessibilityImplementation'){const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return UNHANDLED;return (name==='contextMenu'?contextMenus:accessImpls).get(target)??null;}
      if(name==='needsSoftKeyboard'){const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return UNHANDLED;return getFte3D()?getFte3D().displayGet(target,name):Boolean(softKeyboardNeeds.get(target));}
      if(name==='softKeyboardInputAreaOfInterest'){const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return UNHANDLED;if(getFte3D())return getFte3D().displayGet(target,name);const r=softKeyboardRects.get(target);return r?new Rectangle(r.x,r.y,r.width,r.height):null;}
      if(['mouseX','mouseY','_xmouse','_ymouse'].includes(name)){const p=mousePoint(target);return name==='mouseX'||name==='_xmouse'?p.x:p.y;}
      if(name==='enabled'&&descriptor(target).owner==='movie_clip')return nativeInput(target).movieclip_enabled!==false;
      if(name==='trackAsMenu'&&descriptor(target).owner==='movie_clip')return Boolean(nativeInput(target).track_as_menu);
      if(name==='soundTransform'&&spriteOwners.includes(descriptor(target).owner)){
        const ST=getAudio()?.SoundTransform;if(!ST)throw new Error('Sprite.soundTransform requires the audio host.');
        const rec=spriteSoundTransforms.get(target)||{volume:1,pan:0};
        return new ST(rec.volume,rec.pan);
      }
      if(name==='textSnapshot'&&containerOwners.includes(descriptor(target).owner)){
        const text=getText();if(!text?.snapshotFor)throw new Error('DisplayObjectContainer.textSnapshot requires the shared text service.');
        return text.snapshotFor(referenceOf(target).key);
      }
      if(descriptor(target).owner==='button'){
        if(name==='enabled')return nativeInput(target).button_enabled!==false;
        if(name==='trackAsMenu')return Boolean(nativeInput(target).track_as_menu);
        if(['upState','overState','downState','hitTestState'].includes(name))return buttonRecord(target)[name];
        if(name==='soundTransform'){const ST=getAudio()?.SoundTransform;if(!ST)throw new Error('SimpleButton.soundTransform requires the audio host.');const rec=buttonRecord(target);return new ST(rec.volume,rec.pan);}
      }
      if(name==='dropTarget'||name==='_droptarget'){descriptor(target);const dropped=drops.get(target);return name==='_droptarget'?legacyPath(dropped):dropped&&alive(dropped)?dropped:null;}
      if(descriptor(target).owner==='text_line'&&getFte3D()){
        const tlVal=getFte3D().textLineGet(target,name);
        if(tlVal!==undefined)return tlVal;
      }
      if(descriptor(target).owner==='morph_shape'&&getFte3D()){
        const msVal=getFte3D().morphShapeGet(target,name);
        if(msVal!==undefined)return msVal;
      }
      if(['z','scaleZ','rotationX','rotationY','rotationZ','blendShader'].includes(name)){
        descriptor(target);return getFte3D()?.displayGet(target,name);
      }
      if(['needsSoftKeyboard','softKeyboardInputAreaOfInterest'].includes(name)){
        const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return UNHANDLED;
        return getFte3D()?.displayGet(target,name);
      }
      if(descriptor(target).owner==='video'&&getVideo()){const host=getVideo(),value=host.get(target,name);if(value!==host.UNHANDLED)return value;}
      if(name==='filters'){
        descriptor(target);const host=getFilters();if(!host)throw new Error('Display filters require the shared filter runtime. Re-export this project.');
        return host.fromSpecs(JSON.parse(player.object_filters(referenceOf(target).key)));
      }
      if(['blendMode','cacheAsBitmap','opaqueBackground'].includes(name)){
        descriptor(target);
        if(name==='blendMode'&&getFte3D()?.isBlendModeShaderActive(target))return 'shader';
        if(typeof player.as2_render_command!=='function')return name==='blendMode'?'normal':name==='cacheAsBitmap'?false:null;
        return JSON.parse(player.as2_render_command(referenceOf(target).key,JSON.stringify({op:'get',as3:true})))[name];
      }
      if(name==='scale9Grid'){
        descriptor(target);
        if(typeof player.as2_render_command!=='function')return null;
        const grid=JSON.parse(player.as2_render_command(referenceOf(target).key,JSON.stringify({op:'get',as3:true}))).scale9Grid;
        return grid?new Rectangle(grid[0],grid[1],grid[2],grid[3]):null;
      }
      if(name==='loaderInfo'){descriptor(target);return getLoader()?.infoForDisplay(target)||null;}
      if(['content','contentLoaderInfo','uncaughtErrorEvents'].includes(name)&&descriptor(target).owner==='loader')return getLoader().get(target,name);
      if(name==='transform')return transform(target);
      if(name==='mask'){
        descriptor(target);const value=JSON.parse(player.object_mask(referenceOf(target).key));
        return value===null?null:proxyForRef(value);
      }
      if(name==='scrollRect'){
        descriptor(target);const value=JSON.parse(player.object_scroll_rect(referenceOf(target).key));
        return value===null?null:new Rectangle(value.x,value.y,value.width,value.height);
      }
      if(descriptor(target).owner==='text'&&isStaticTextNode(descriptor(target))){
        if(name==='text')return textValue(descriptor(target).kind.text);
      }else if(getText()&&descriptor(target).owner==='text'){
        const text=getText(),value=text.get(referenceOf(target).key,name);if(value!==text.UNHANDLED)return value;
      }
      if(['bitmapData','smoothing','pixelSnapping'].includes(name)&&descriptor(target).owner==='bitmap'){
        const key=referenceOf(target).key;
        if(name==='bitmapData'){const ref=JSON.parse(player.object_bitmap(key));return ref?getBitmap().fromRef(ref):null;}
        return name==='smoothing'?player.object_smoothing(key):player.object_pixel_snapping(key);
      }
      if(textProperties.has(name)||unsupportedText.has(name)) {
        const node=descriptor(target);
        if(node.owner==='text'&&!isStaticTextNode(node)) {
          if(unsupportedText.has(name))throw new TypeError(`TextField.${name} is not implemented in the plain-text profile. This TextField API is outside the current uniform text profile.`);
          const kind=node.kind,text=textValue(kind.text),lines=text.split('\r');
          if(name==='text')return text;
          if(name==='textColor')return kind.color[0]*65536+kind.color[1]*256+kind.color[2];
          if(name==='length')return text.length;
          if(name==='numLines')return lines.length;
          // These are the current Canvas renderer's unwrapped logical metrics,
          // not Flash's font-engine measurements or a TextFormat layout box.
          if(name==='textHeight')return lines.length*kind.font_size*1.2;
          let width=0;for(const line of lines)width=Math.max(width,measureText(line,kind));return width;
        }
      }
      if(name==='name'||name==='_name')return descriptor(target).name;
      if(name==='numChildren'){if(!containerOwners.includes(descriptor(target).owner))return UNHANDLED;return children(target).length;}
      if(name==='width'||name==='_width')return bounds(target,parentSpace(target),true).width;
      if(name==='height'||name==='_height')return bounds(target,parentSpace(target),true).height;
      if(name==='stage') {return descriptor(target).on_stage===false?null:getStage();}
      if(name==='graphics'&&graph&&['movie_clip','sprite','shape'].includes(descriptor(target).owner))return graphics(target);
      return UNHANDLED;
    }
    function set(target,name,value) {
      if(name==='_focusrect'&&['movie_clip','button'].includes(descriptor(target).owner)){as2Input(target,{op:'set',name:'_focusrect',value:value===undefined?null:value});as2TabActive=true;return true;}
      if(name==='enabled'&&descriptor(target).owner==='movie_clip'){inputWrite(target,name,value);return true;}
      if(name==='trackAsMenu'&&descriptor(target).owner==='movie_clip'){inputWrite(target,'track_as_menu',value);return true;}
      if(name==='soundTransform'&&spriteOwners.includes(descriptor(target).owner)){
        const ST=getAudio()?.SoundTransform;if(!ST||!ST.__nfIs(value))throw new TypeError('Sprite.soundTransform requires a SoundTransform.');
        spriteSoundTransforms.set(target,{volume:value.volume,pan:value.pan});return true;
      }
      if(name==='textSnapshot'&&containerOwners.includes(descriptor(target).owner))throw new TypeError('DisplayObjectContainer.textSnapshot is read-only.');
      if(descriptor(target).owner==='button'){
        if(name==='enabled'){inputWrite(target,'button_enabled',value);return true;}
        if(name==='trackAsMenu'){inputWrite(target,'track_as_menu',value);return true;}
        if(['upState','overState','downState','hitTestState'].includes(name)){validateButtonState(value);buttonRecord(target)[name]=value;return true;}
        if(name==='soundTransform'){const ST=getAudio()?.SoundTransform;if(!ST||!ST.__nfIs(value))throw new TypeError('SimpleButton.soundTransform requires a SoundTransform.');const rec=buttonRecord(target);rec.volume=value.volume;rec.pan=value.pan;return true;}
      }
      if(name==='accessibilityProperties'){descriptor(target);if(value!==null&&value!==undefined&&typeof value!=='object')throw new TypeError('DisplayObject.accessibilityProperties requires an AccessibilityProperties or null.');if(value===null||value===undefined)accProps.delete(target);else accProps.set(target,value);return true;}
      if(name==='contextMenu'||name==='accessibilityImplementation'){const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return false;if(value!==null&&value!==undefined&&typeof value!=='object')throw new TypeError(`${name} requires an Object or null.`);(name==='contextMenu'?contextMenus:accessImpls).set(target,value??null);return true;}
      if(name==='needsSoftKeyboard'){const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return false;if(typeof value!=='boolean')throw new TypeError('InteractiveObject.needsSoftKeyboard requires a Boolean.');if(getFte3D())return Boolean(getFte3D().displaySet(target,name,value));softKeyboardNeeds.set(target,value);return true;}
      if(name==='softKeyboardInputAreaOfInterest'){const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return false;if(value!==null&&!Rectangle.__nfIs(value))throw new TypeError('InteractiveObject.softKeyboardInputAreaOfInterest requires a Rectangle or null.');if(getFte3D())return Boolean(getFte3D().displaySet(target,name,value));softKeyboardRects.set(target,value===null?null:{x:value.x,y:value.y,width:value.width,height:value.height});return true;}
      if(Object.hasOwn(inputNames,name)){const node=descriptor(target);if(isStaticTextNode(node)||!ownsInputProperty(node.owner,name))return false;inputWrite(target,name,value);return true;}
      if(['mouseX','mouseY','_xmouse','_ymouse','dropTarget','_droptarget','_target','_framesloaded','_url'].includes(name))throw new TypeError(`${name} is read-only.`);
      if(descriptor(target).owner==='text_line'&&getFte3D()?.textLineSet(target,name,value))return true;
      if(descriptor(target).owner==='morph_shape'&&getFte3D()?.morphShapeSet(target,name,value))return true;
      if(['z','scaleZ','rotationX','rotationY','rotationZ','blendShader'].includes(name)){
        descriptor(target);return Boolean(getFte3D()?.displaySet(target,name,value));
      }
      if(['needsSoftKeyboard','softKeyboardInputAreaOfInterest'].includes(name)){
        const node=descriptor(target);if(isStaticTextNode(node)||!interactiveOwners.includes(node.owner))return false;
        return Boolean(getFte3D()?.displaySet(target,name,value));
      }
      if(descriptor(target).owner==='video'&&getVideo()&&getVideo().set(target,name,value))return true;
      if(name==='filters'){
        descriptor(target);const host=getFilters();if(!host)throw new Error('Display filters require the shared filter runtime. Re-export this project.');
        const specs=host.toSpecs(value);beforeFilters(target,specs);
        try{player.object_set_filters(referenceOf(target).key,JSON.stringify(specs));}
        catch(error){throw error instanceof Error?error:new RangeError(String(error));}
        return true;
      }
      if(['blendMode','cacheAsBitmap','opaqueBackground'].includes(name)){
        descriptor(target);
        if(name==='blendMode'){
          if(value==='shader'){
            if(!getFte3D()?.hasBlendShader(target)){
              const err=new RangeError('DisplayObject.blendMode = BlendMode.SHADER requires blendShader to be assigned first.');
              err.name='ArgumentError';throw err;
            }
            getFte3D().setBlendModeShaderActive(target,true);
            return true;
          }
          getFte3D()?.setBlendModeShaderActive(target,false);
        }
        if(typeof player.as2_render_command!=='function')throw new Error('Display compositing requires the shared Rust render state.');
        const key=referenceOf(target).key;
        let next;
        try{next=JSON.parse(player.as2_render_command(key,JSON.stringify({op:'prepare',as3:true,name,value})));}
        catch(error){throw error instanceof Error?error:new TypeError(String(error));}
        if(next.isolated)beforeRaster(target);
        player.as2_render_command(key,JSON.stringify({op:'set',as3:true,name,value}));
        return true;
      }
      if(name==='scale9Grid'){
        descriptor(target);
        if(value!==null&&!Rectangle.__nfIs(value))throw new TypeError('DisplayObject.scale9Grid requires a native Rectangle or null.');
        if(typeof player.as2_render_command!=='function')throw new Error('Display scale9Grid requires the shared Rust render state.');
        const payload=value===null?null:{x:value.x,y:value.y,width:value.width,height:value.height};
        try{player.as2_render_command(referenceOf(target).key,JSON.stringify({op:'set',as3:true,name:'scale9Grid',value:payload}));}
        catch(error){const err=new RangeError(String(error));err.name='ArgumentError';throw err;}
        return true;
      }
      if(name==='loaderInfo'||(['content','contentLoaderInfo','uncaughtErrorEvents'].includes(name)&&descriptor(target).owner==='loader'))throw new TypeError(`Display ${name} is read-only.`);
      if(name==='transform')throw new TypeError('Replacing DisplayObject.transform is not supported; set transform.matrix or transform.colorTransform.');
      if(name==='mask'){
        descriptor(target);if(value!==null)descriptor(value);
        player.object_set_mask(referenceOf(target).key,JSON.stringify(value===null?null:referenceOf(value).objectRef));return true;
      }
      if(name==='scrollRect'){
        descriptor(target);
        if(value!==null&&!Rectangle.__nfIs(value))throw new TypeError('DisplayObject.scrollRect requires a native Rectangle or null.');
        let rect=null;
        if(value!==null){
          const values=['x','y','width','height'].map(name=>value[name]);
          if(!values.every(value=>typeof value==='number'&&Number.isInteger(value)&&Math.abs(value)<=1e9)||values[2]<0||values[3]<0)throw new RangeError('scrollRect requires integer coordinates within ±1000000000 and nonnegative integer dimensions. Fractional normalization and render-delayed updates are outside this profile.');
          rect={x:values[0],y:values[1],width:values[2],height:values[3]};
        }
        // Shared Rust owns copied state. Every getter also returns a new value;
        // mutating either the assigned Rectangle or a prior read has no effect.
        player.object_set_scroll_rect(referenceOf(target).key,JSON.stringify(rect));return true;
      }
      if(descriptor(target).owner==='text'&&isStaticTextNode(descriptor(target))){
        if(name==='text')throw new TypeError('StaticText.text is read-only.');
      }else if(getText()&&descriptor(target).owner==='text'){
        if(['width','height','_width','_height'].includes(name)&&nonAxisAligned(descriptor(target)))throw new RangeError('Setting dimensions of a rotated TextField or skewed TextField is not supported yet. Set its logical box before applying the matrix.');
        if(getText().set(referenceOf(target).key,name,value))return true;
      }
      if(['bitmapData','smoothing','pixelSnapping'].includes(name)&&descriptor(target).owner==='bitmap'){
        const key=referenceOf(target).key;let accepted;
        if(name==='bitmapData')accepted=player.object_set_bitmap(key,JSON.stringify(value===null?null:getBitmap().refOf(value)));
        else if(name==='smoothing'){if(typeof value!=='boolean')throw new TypeError('Bitmap.smoothing requires a Boolean.');accepted=player.object_set_smoothing(key,value);}
        else {pixelSnapping(value);accepted=player.object_set_pixel_snapping(key,value);}
        if(!accepted)throw new Error(`Cannot set Bitmap.${name} on this display object.`);return true;
      }
      if(textProperties.has(name)||unsupportedText.has(name)) {
        const node=descriptor(target);
        if(node.owner==='text'&&!isStaticTextNode(node)) {
          if(unsupportedText.has(name))throw new TypeError(`TextField.${name} is not implemented in the plain-text profile. This TextField API is outside the current uniform text profile.`);
          if(name==='text'){writeText(target,value);return true;}
          if(name==='textColor'){
            if(!Number.isInteger(value)||value<0||value>0xffffff)throw new RangeError('textColor currently requires an RGB integer from 0 through 0xffffff.');
            if(!invoke(target,'set_text_color',value))throw new Error('Runtime rejected textColor on this TextField.');return true;
          }
          throw new TypeError(`TextField.${name} is read-only.`);
        }
      }
      if(name==='name'||name==='_name') {
        descriptor(target); if(typeof value!=='string')throw new TypeError('Display name must be a string.');
        if(!invoke(target,'set_name',value))throw new RangeError('Display name must be at most 4096 UTF-8 bytes.');return true;
      }
      if(name==='width'||name==='height'||name==='_width'||name==='_height') {
        const node=descriptor(target);if(name[0]==='_'){value=Number(value);if(!Number.isFinite(value)||value<0)return true;}
        if(!Number.isFinite(value)||value<0)throw new RangeError('Display dimensions must be finite nonnegative numbers.');
        if(nonAxisAligned(node))throw new RangeError('Setting dimensions of a rotated object or skewed object is not supported yet. Set scaleX/scaleY explicitly; transformed bounds remain available.');
        const axis=name.includes('width')?'width':'height',local=bounds(target,target,true)[axis];
        if(local===0){if(value===0||name[0]==='_')return true;throw new RangeError('Cannot resize empty geometry to a nonzero dimension.');}
        if(!invoke(target,'set_property',axis==='width'?'scale_x':'scale_y',value/local))throw new RangeError('Runtime rejected the requested display dimension.');return true;
      }
      if(name==='numChildren'||name==='stage'||(name==='graphics'&&graph&&['movie_clip','sprite','shape'].includes(descriptor(target).owner)))throw new TypeError(`${name} is read-only.`);
      return false;
    }
    function ensureLegacyRecord(target) {
      const ref=referenceOf(target),id=ref.key+'\u0000'+ref.lifetime;
      let record=legacy.get(id);
      if(!record){record={target,ref,handlers:new Map(),dynamic:false,authoredClip:new Map(),authoredButton:[]};legacy.set(id,record);}
      return {id,record};
    }
    function authoredReceiver(target) {
      if(!target||!alive(target))return target;
      if(descriptor(target).owner==='button'){
        const parent=parentSpace(target);
        if(parent&&parent!==getStage()&&alive(parent))return parent;
      }
      return target;
    }
    function authoredHandlersFor(record,name) {
      if(!record)return [];
      const out=[];
      const clipName=clipEventForCallback[name];
      if(clipName&&record.authoredClip?.has(clipName))out.push(...record.authoredClip.get(clipName));
      const btnName=buttonEventForCallback[name];
      if(btnName&&record.authoredButton?.length){
        for(const entry of record.authoredButton)if(entry.event===btnName)out.push(entry.fn);
      }
      return out;
    }
    function hasCallback(record,name) {
      return Boolean(authoredHandlersFor(record,name).length||callback(record,name));
    }
    function addLegacyEvent(target,kind,event,keySpec,fn) {
      if(!referenceOf(target))throw new TypeError('Legacy event handlers require a live display object.');
      const node=descriptor(target);
      if(kind==='clip'){
        if(node.owner!=='movie_clip')throw new TypeError('onClipEvent requires a MovieClip instance.');
      }else if(kind==='button'){
        if(!['movie_clip','button'].includes(node.owner))throw new TypeError('on(...) requires a Button or MovieClip instance.');
      }else throw new TypeError(`Unsupported legacy event kind '${kind}'.`);
      if(typeof fn!=='function')throw new TypeError('Legacy event handler requires a function.');
      const {record}=ensureLegacyRecord(target);
      if(kind==='clip'){
        let list=record.authoredClip.get(event);
        if(!list){list=[];record.authoredClip.set(event,list);}
        list.push(fn);
        if(event==='load')pendingClipLoads.push({target,fn});
      }else{
        record.authoredButton.push({event,key:keySpec??null,fn});
      }
    }
    function flushClipLoads() {
      while(pendingClipLoads.length){
        const batch=pendingClipLoads.splice(0,pendingClipLoads.length);
        for(const item of batch){
          if(alive(item.target))Reflect.apply(item.fn,authoredReceiver(item.target),[]);
        }
      }
    }
    function notifyData(target) {
      if(!target||!alive(target))return;
      const ref=referenceOf(target),record=ref&&legacy.get(ref.key+'\u0000'+ref.lifetime);
      const handlers=record?.authoredClip?.get('data');
      if(handlers){
        for(const fn of [...handlers]){
          if(alive(target))Reflect.apply(fn,authoredReceiver(target),[]);
        }
      }
      if(alive(target)){
        const handler=target.onData;
        if(typeof handler==='function')Reflect.apply(handler,target,[]);
      }
    }
    function setLegacyCallback(target,name,value) {
      if(!referenceOf(target)){target[name]=value;return value;}
      const node=descriptor(target);
      if(node.owner==='text'&&['onSetFocus','onKillFocus','onChanged'].includes(name)){target[name]=value;return value;}
      if(!['movie_clip','button'].includes(node.owner))throw new TypeError('AS2 callbacks require a MovieClip or Button instance.');
      if(!callbackNames.has(name))throw new TypeError(`AS2 callback ${name} is not supported yet.`);
      if(value!==null&&value!==undefined&&typeof value!=='function')throw new TypeError(`${name} requires a function or null.`);
      target[name]=value;
      const {id,record}=ensureLegacyRecord(target);
      if(value==null)record.handlers.delete(name);else record.handlers.set(name,value);
      if(!record.handlers.size&&!record.dynamic&&!record.authoredClip.size&&!record.authoredButton.length)legacy.delete(id);
      return value;
    }
    function refreshLegacyCallbacks(target,as2Class=false) {
      if(!['movie_clip','button'].includes(descriptor(target).owner))return;
      const {record}=ensureLegacyRecord(target);
      record.dynamic=true;record.as2Class=record.as2Class||as2Class;
    }
    function callback(record,name) {
      if(!existingCallbackNames.has(name)&&!record?.as2Class&&!record?.handlers.has(name))return null;
      const handler=record?.dynamic?record.target[name]:record?.handlers.get(name);
      return typeof handler==='function'?handler:null;
    }
    function alive(target){const ref=referenceOf(target);return !!ref&&(graph?player.object_alive(ref.key):player.lifetime_id(ref.key)===ref.lifetime);}
    function prune(){for(const[id,record]of legacy)if(!alive(record.target))legacy.delete(id);if(dragging&&(!alive(dragging.target)||descriptor(dragging.target).on_stage===false))dragging=null;if(pressed&&(!alive(pressed)||descriptor(pressed).on_stage===false))pressed=null;if(hovered&&(!alive(hovered)||descriptor(hovered).on_stage===false))hovered=null;}
    function enterFrame(){
      prune();
      for(const record of [...legacy.values()]){
        if(!alive(record.target))continue;
        for(const fn of authoredHandlersFor(record,'onEnterFrame')){
          if(alive(record.target))Reflect.apply(fn,authoredReceiver(record.target),[]);
        }
        const handler=callback(record,'onEnterFrame');
        if(handler&&alive(record.target))Reflect.apply(handler,record.target,[]);
      }
    }
    function press(target){
      prune();
      while(target&&referenceOf(target)){
        const ref=referenceOf(target),record=legacy.get(ref.key+'\u0000'+ref.lifetime);
        if(hasCallback(record,'onPress')){callLegacy(target,'onPress');return;}
        target=parentSpace(target);
        if(target===getStage())target=null;
      }
    }
    function pixelSnapping(value){if(!['auto','always','never'].includes(value))throw new TypeError('Bitmap.pixelSnapping requires auto, always or never.');return value;}
    function bitmapArguments(args){
      if(args.length>3)throw new TypeError('Bitmap expects optional bitmapData, pixelSnapping and smoothing.');
      const [data=null,snap='auto',smooth=false]=args;pixelSnapping(snap);if(typeof smooth!=='boolean')throw new TypeError('Bitmap.smoothing requires a Boolean.');
      if(data!==null)getBitmap().refOf(data);return [data,snap,smooth];
    }
    function initializeBitmap(receiver,args){const [data,snap,smooth]=bitmapArguments(args);set(receiver,'bitmapData',data);set(receiver,'pixelSnapping',snap);set(receiver,'smoothing',smooth);}
    function validateButtonState(value){
      if(value===null)return null;
      if(value===getStage()||!builtins['flash.display.DisplayObject'].__nfIs(value))throw new TypeError('SimpleButton state requires a DisplayObject or null.');
      return value;
    }
    function simpleButtonArguments(args){
      if(args.length>4)throw new TypeError('SimpleButton expects at most four optional DisplayObject state arguments.');
      const [up=null,over=null,down=null,hit=null]=args;
      return [validateButtonState(up),validateButtonState(over),validateButtonState(down),validateButtonState(hit)];
    }
    function initializeSimpleButton(receiver,args){
      const [upState,overState,downState,hitTestState]=simpleButtonArguments(args);
      const rec=buttonRecord(receiver);
      rec.upState=upState;rec.overState=overState;rec.downState=downState;rec.hitTestState=hitTestState;
      refreshLegacyCallbacks(receiver);
    }
    const solidFillSet=new WeakSet(),gradientFillMap=new WeakMap(),bitmapFillSet=new WeakSet(),endFillSet=new WeakSet(),strokeMap=new WeakMap(),pathMap=new WeakMap(),triPathMap=new WeakMap();
    function finalizeGraphicsType(Type,check){
      Type.__nfIs=check;
      Type.__nfInitialize=()=>{throw new TypeError(`${Type.name} is final and cannot be subclassed.`);};
      Object.defineProperty(Type,Symbol.hasInstance,{value:check});
      return Type;
    }
    class GraphicsSolidFill {
      constructor(color=0,alpha=1.0){
        if(new.target!==GraphicsSolidFill)throw new TypeError('GraphicsSolidFill is final and cannot be subclassed.');
        this.color=Number(color)>>>0;this.alpha=Number(alpha);
        solidFillSet.add(this);return Object.seal(this);
      }
    }
    finalizeGraphicsType(GraphicsSolidFill,v=>solidFillSet.has(v));
    class GraphicsGradientFill {
      constructor(type='linear',colors=null,alphas=null,ratios=null,matrix=null,spreadMethod='pad',interpolationMethod='rgb',focalPointRatio=0.0){
        if(new.target!==GraphicsGradientFill)throw new TypeError('GraphicsGradientFill is final and cannot be subclassed.');
        gradientFillMap.set(this,{type:'linear',spreadMethod:'pad',interpolationMethod:'rgb'});
        this.type=type;this.colors=colors;this.alphas=alphas;this.ratios=ratios;this.matrix=matrix;
        this.spreadMethod=spreadMethod;this.interpolationMethod=interpolationMethod;this.focalPointRatio=Number(focalPointRatio);
        return Object.seal(this);
      }
      get type(){return gradientFillMap.get(this).type;}
      set type(value){const next=String(value);if(next!=='linear'&&next!=='radial'){const err=new RangeError(`GraphicsGradientFill.type is invalid: '${next}'.`);err.name='ArgumentError';throw err;}gradientFillMap.get(this).type=next;}
      get spreadMethod(){return gradientFillMap.get(this).spreadMethod;}
      set spreadMethod(value){const next=String(value);if(!['pad','reflect','repeat'].includes(next)){const err=new RangeError(`GraphicsGradientFill.spreadMethod is invalid: '${next}'.`);err.name='ArgumentError';throw err;}gradientFillMap.get(this).spreadMethod=next;}
      get interpolationMethod(){return gradientFillMap.get(this).interpolationMethod;}
      set interpolationMethod(value){const next=String(value);if(next!=='rgb'&&next!=='linearRGB'){const err=new RangeError(`GraphicsGradientFill.interpolationMethod is invalid: '${next}'.`);err.name='ArgumentError';throw err;}gradientFillMap.get(this).interpolationMethod=next;}
    }
    finalizeGraphicsType(GraphicsGradientFill,v=>gradientFillMap.has(v));
    class GraphicsBitmapFill {
      constructor(bitmapData=null,matrix=null,repeat=true,smooth=false){
        if(new.target!==GraphicsBitmapFill)throw new TypeError('GraphicsBitmapFill is final and cannot be subclassed.');
        if(bitmapData!==null&&!getBitmap()?.BitmapData.__nfIs(bitmapData))throw new TypeError('GraphicsBitmapFill.bitmapData requires BitmapData or null.');
        if(matrix!==null&&!Matrix.__nfIs(matrix))throw new TypeError('GraphicsBitmapFill.matrix requires Matrix or null.');
        this.bitmapData=bitmapData;this.matrix=matrix;this.repeat=Boolean(repeat);this.smooth=Boolean(smooth);
        bitmapFillSet.add(this);return Object.seal(this);
      }
    }
    finalizeGraphicsType(GraphicsBitmapFill,v=>bitmapFillSet.has(v));
    class GraphicsEndFill {
      constructor(){
        if(new.target!==GraphicsEndFill)throw new TypeError('GraphicsEndFill is final and cannot be subclassed.');
        endFillSet.add(this);return Object.seal(this);
      }
    }
    finalizeGraphicsType(GraphicsEndFill,v=>endFillSet.has(v));
    class GraphicsStroke {
      constructor(thickness=NaN,pixelHinting=false,scaleMode='normal',caps='none',joints='round',miterLimit=3.0,fill=null){
        if(new.target!==GraphicsStroke)throw new TypeError('GraphicsStroke is final and cannot be subclassed.');
        strokeMap.set(this,{scaleMode:'normal',caps:'none',joints:'round'});
        this.thickness=Number(thickness);this.pixelHinting=Boolean(pixelHinting);
        this.scaleMode=scaleMode;this.caps=caps;this.joints=joints;
        this.miterLimit=Number(miterLimit);
        if(fill!==null&&!IGraphicsFill.__nfIs(fill))throw new TypeError('GraphicsStroke.fill requires an IGraphicsFill or null.');
        this.fill=fill;
        return Object.seal(this);
      }
      get scaleMode(){return strokeMap.get(this).scaleMode;}
      set scaleMode(value){const next=String(value);if(!['normal','none','vertical','horizontal'].includes(next)){const err=new RangeError(`GraphicsStroke.scaleMode is invalid: '${next}'.`);err.name='ArgumentError';throw err;}strokeMap.get(this).scaleMode=next;}
      get caps(){return strokeMap.get(this).caps;}
      set caps(value){const next=String(value);if(!['none','round','square'].includes(next)){const err=new RangeError(`GraphicsStroke.caps is invalid: '${next}'.`);err.name='ArgumentError';throw err;}strokeMap.get(this).caps=next;}
      get joints(){return strokeMap.get(this).joints;}
      set joints(value){const next=String(value);if(!['round','bevel','miter'].includes(next)){const err=new RangeError(`GraphicsStroke.joints is invalid: '${next}'.`);err.name='ArgumentError';throw err;}strokeMap.get(this).joints=next;}
    }
    finalizeGraphicsType(GraphicsStroke,v=>strokeMap.has(v));
    class GraphicsPath {
      constructor(commands=null,data=null,winding='evenOdd'){
        if(new.target!==GraphicsPath)throw new TypeError('GraphicsPath is final and cannot be subclassed.');
        pathMap.set(this,{winding:'evenOdd'});
        this.commands=commands;this.data=data;this.winding=winding;
        return Object.seal(this);
      }
      get winding(){return pathMap.get(this).winding;}
      set winding(value){const next=String(value);if(next!=='evenOdd'&&next!=='nonZero'){const err=new RangeError(`GraphicsPath.winding must be 'evenOdd' or 'nonZero'; got '${next}'.`);err.name='ArgumentError';throw err;}pathMap.get(this).winding=next;}
      moveTo(x,y){this.#ensure();this.commands.push(1);this.data.push(Number(x),Number(y));}
      lineTo(x,y){this.#ensure();this.commands.push(2);this.data.push(Number(x),Number(y));}
      curveTo(controlX,controlY,anchorX,anchorY){this.#ensure();this.commands.push(3);this.data.push(Number(controlX),Number(controlY),Number(anchorX),Number(anchorY));}
      wideMoveTo(x,y){this.#ensure();this.commands.push(4);this.data.push(0,0,Number(x),Number(y));}
      wideLineTo(x,y){this.#ensure();this.commands.push(5);this.data.push(0,0,Number(x),Number(y));}
      cubicCurveTo(cx1,cy1,cx2,cy2,ax,ay){this.#ensure();this.commands.push(6);this.data.push(Number(cx1),Number(cy1),Number(cx2),Number(cy2),Number(ax),Number(ay));}
      #ensure(){
        if(this.commands===null||this.commands===undefined)this.commands=graphicsVectors.intVector([]);
        if(this.data===null||this.data===undefined)this.data=graphicsVectors.numberVector([]);
      }
    }
    finalizeGraphicsType(GraphicsPath,v=>pathMap.has(v));
    class GraphicsTrianglePath {
      constructor(vertices=null,indices=null,uvtData=null,culling='none'){
        if(new.target!==GraphicsTrianglePath)throw new TypeError('GraphicsTrianglePath is final and cannot be subclassed.');
        triPathMap.set(this,{culling:'none'});
        this.vertices=vertices;this.indices=indices;this.uvtData=uvtData;this.culling=culling;
        return Object.seal(this);
      }
      get culling(){return triPathMap.get(this).culling;}
      set culling(value){const next=String(value);if(!['none','positive','negative'].includes(next)){const err=new RangeError(`GraphicsTrianglePath.culling is invalid: '${next}'.`);err.name='ArgumentError';throw err;}triPathMap.get(this).culling=next;}
    }
    finalizeGraphicsType(GraphicsTrianglePath,v=>triPathMap.has(v));
    function makeGraphicsInterface(name,check){
      const Iface=function(){throw new TypeError(`${name} is an interface and cannot be instantiated.`);};
      Object.defineProperty(Iface,'name',{value:name});
      Iface.__nfIs=check;
      Object.defineProperty(Iface,Symbol.hasInstance,{value:check});
      return Iface;
    }
    const IGraphicsFill=makeGraphicsInterface('IGraphicsFill',v=>GraphicsSolidFill.__nfIs(v)||GraphicsGradientFill.__nfIs(v)||GraphicsBitmapFill.__nfIs(v)||GraphicsEndFill.__nfIs(v)||Boolean(getFte3D()?.GraphicsShaderFill?.__nfIs(v)));
    const IGraphicsPath=makeGraphicsInterface('IGraphicsPath',v=>GraphicsPath.__nfIs(v)||GraphicsTrianglePath.__nfIs(v));
    const IGraphicsStroke=makeGraphicsInterface('IGraphicsStroke',v=>GraphicsStroke.__nfIs(v));
    const IGraphicsData=makeGraphicsInterface('IGraphicsData',v=>IGraphicsFill.__nfIs(v)||IGraphicsPath.__nfIs(v)||IGraphicsStroke.__nfIs(v));
    const IBitmapDrawable=makeGraphicsInterface('IBitmapDrawable',v=>Boolean(builtins['flash.display.DisplayObject']?.__nfIs(v)||getBitmap()?.BitmapData?.__nfIs(v)));
    const builtins={
      'flash.display.Graphics':Graphics,
      'flash.display.IBitmapDrawable':IBitmapDrawable,
      'flash.display.IGraphicsData':IGraphicsData,
      'flash.display.IGraphicsFill':IGraphicsFill,
      'flash.display.IGraphicsPath':IGraphicsPath,
      'flash.display.IGraphicsStroke':IGraphicsStroke,
      'flash.display.GraphicsSolidFill':GraphicsSolidFill,
      'flash.display.GraphicsGradientFill':GraphicsGradientFill,
      'flash.display.GraphicsBitmapFill':GraphicsBitmapFill,
      'flash.display.GraphicsEndFill':GraphicsEndFill,
      'flash.display.GraphicsStroke':GraphicsStroke,
      'flash.display.GraphicsPath':GraphicsPath,
      'flash.display.GraphicsTrianglePath':GraphicsTrianglePath,
      'flash.geom.Point':Point,'flash.geom.Rectangle':Rectangle,'flash.geom.Matrix':Matrix,'flash.geom.ColorTransform':ColorTransform,'flash.geom.Transform':Transform
    };
    for(const[name,type]of Object.entries(graphicsEnums))builtins['flash.display.'+name]=type;
    for(const[name,owners]of Object.entries({DisplayObject:null,InteractiveObject:interactiveOwners,DisplayObjectContainer:containerOwners,MovieClip:['movie_clip'],SimpleButton:['button'],Sprite:spriteOwners,Shape:['shape'],Bitmap:['bitmap'],TextField:['text'],StaticText:['text']})) {
      const allocation={Sprite:'sprite',Shape:'shape',MovieClip:'movie_clip',Bitmap:'bitmap',TextField:'text_field',SimpleButton:'button'}[name];
      function allocate(){
        if(name==='StaticText'){const err=new RangeError('Error #2012: StaticText$ class cannot be instantiated.');err.name='ArgumentError';err.errorID=2012;throw err;}
        if(name==='DisplayObject'||name==='InteractiveObject'||name==='DisplayObjectContainer')throw new TypeError(`ArgumentError: Error #2012: ${name}$ class cannot be instantiated.`);
        if(!graph||!allocation)throw new TypeError(`Dynamic new ${name}() is not supported. Use an authored instance for this owner.`);
        return proxyForRef(JSON.parse(player.create_display(allocation)));
      }
      function NativeDisplay(...args){if(name==='Bitmap'){bitmapArguments(args);const receiver=allocate();initializeBitmap(receiver,args);return receiver;}if(name==='SimpleButton'){simpleButtonArguments(args);const receiver=allocate();initializeSimpleButton(receiver,args);return receiver;}if(name==='StaticText')return allocate();if(args.length)throw new TypeError(`${name} construction requires no arguments.`);return allocate();}
      if((allocation||name==='SimpleButton')&&graph)NativeDisplay.__nfAllocate=({className}={})=>{
        const symbol=project?.symbols.find(symbol=>symbol.linkage_id&&symbol.class_name===className);
        if(!symbol)return allocate();
        const receiver=proxyForRef(JSON.parse(player.create_linked(symbol.linkage_id)));
        prepareLinked(receiver,true);return receiver;
      };
      NativeDisplay.__nfIs=value=>{
        if(value&&value===getStage())return ['DisplayObject','InteractiveObject','DisplayObjectContainer'].includes(name);
        try{
          const node=descriptor(value);
          if(name==='StaticText')return isStaticTextNode(node);
          if((name==='InteractiveObject'||name==='TextField')&&isStaticTextNode(node))return false;
          return owners===null||owners.includes(node.owner);
        }catch(_){return false;}
      };
      NativeDisplay.__nfInitialize=(receiver,args)=>{if(name==='Bitmap'){if(!NativeDisplay.__nfIs(receiver))throw new TypeError('Bitmap base initialization requires a Bitmap display object.');initializeBitmap(receiver,args);return;}if(name==='SimpleButton'){if(!NativeDisplay.__nfIs(receiver))throw new TypeError('SimpleButton base initialization requires a SimpleButton display object.');initializeSimpleButton(receiver,args);return;}if(args.length||!NativeDisplay.__nfIs(receiver))throw new TypeError(`${name} base initialization requires an existing compatible display instance and no arguments.`);if(name==='MovieClip')refreshLegacyCallbacks(receiver);};
      Object.defineProperty(NativeDisplay,Symbol.hasInstance,{value:NativeDisplay.__nfIs});
      builtins[(name==='TextField'||name==='StaticText'?'flash.text.':'flash.display.')+name]=NativeDisplay;
    }
    builtins['flash.text.TextField'].isFontCompatible=(fontName,fontStyle)=>{
      const text=getText();
      if(!text?.isFontCompatible)throw new Error('TextField.isFontCompatible requires the shared text service.');
      return text.isFontCompatible(fontName,fontStyle);
    };
    return {methods,get,set,builtins,stageContainer,setGraphicsVectorFactories,as2Ref,readAccProps:target=>accProps.get(target),as2GlobalPropRef,toggleHighQuality,showRedrawRegions,as2CanFocus,as2Tab,as2FocusBounds,lifecycle:(type,target)=>{if(type==='removed_from_stage')callLegacy(target,'onUnload');},targetPath,resolveTarget,descriptor,nodes,alive,convert,bounds,parentSpace,children,nodeProxy,addLegacyEvent,flushClipLoads,notifyData,setLegacyCallback,refreshLegacyCallbacks,enterFrame,press,pointer,cancelPointer,updateDrag,hasDrag:()=>Boolean(dragging),pick,pickDetails,inputRoute,resolveInput,handCursor,prune,clear:()=>{legacy.clear();pendingClipLoads.length=0;staticTextLifetimes.clear();accProps=new WeakMap();spriteSoundTransforms=new WeakMap();touchDragId=null;contextMenus=new WeakMap();accessImpls=new WeakMap();buttonRecords=new WeakMap();softKeyboardNeeds=new WeakMap();softKeyboardRects=new WeakMap();transformMatrix3D=new WeakMap();transformPerspective=new WeakMap();as2TabActive=false;cancelPointer();},UNHANDLED};
  }
  const api={create,...graphicsEnums,normalizedRotation,UNHANDLED};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashDisplay=api;
})(globalThis);
