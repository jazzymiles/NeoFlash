/* MovieClip/DisplayObject facade over the shared Rust placement graph. */
(function (global) {
  'use strict';
  const geometry=typeof module!=='undefined'&&module.exports?require('./geom.js'):global.NeoFlashGeom;
  if(!geometry)throw new Error('The geometry runtime is missing. Re-export this project.');
  const {Point,Rectangle,Matrix,ColorTransform}=geometry;
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
  const graphicsEnums={GradientType,SpreadMethod,InterpolationMethod,GraphicsPathCommand,GraphicsPathWinding};
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
  function matrix(t) { const r=t.rotation*Math.PI/180,c=Math.cos(r),s=Math.sin(r); return [c*t.scale_x,s*t.scale_x,-s*t.scale_y,c*t.scale_y,t.x,t.y]; }
  function inverse(m) {
    const d=m[0]*m[3]-m[1]*m[2];
    if (!Number.isFinite(d) || Math.abs(d)<1e-12) throw new RangeError('Coordinate conversion requires an invertible target transform.');
    return [m[3]/d,-m[1]/d,-m[2]/d,m[0]/d,(m[2]*m[5]-m[3]*m[4])/d,(m[1]*m[4]-m[0]*m[5])/d];
  }
  function normalizedRotation(value) { const r=value%360;return r>180?r-360:r< -180?r+360:r; }
  function nonAxisAligned(node){const m=node.local_matrix||matrix(node.transform);return Math.abs(m[1])>1e-10||Math.abs(m[2])>1e-10;}
  const textProperties=new Set(['text','textColor','length','numLines','textWidth','textHeight']);
  const unsupportedText=new Set(['htmlText','styleSheet','type','selectable','autoSize','embedFonts','displayAsPassword','maxChars','restrict','selectionBeginIndex','selectionEndIndex','caretIndex','scrollH','scrollV','maxScrollH','maxScrollV','bottomScrollV','background','backgroundColor','border','borderColor','antiAliasType','gridFitType','sharpness','thickness','condenseWhite','getCharBoundaries','getCharIndexAtPoint','getLineIndexAtPoint','getLineMetrics','getParagraphLength','getFirstCharInParagraph','setSelection','replaceSelectedText']);
  const textValue=value=>value.replace(/\r\n?|\n/g,'\r');

  function create({player,proxyFor,proxyForRef,referenceOf,getStage=()=>null,measureText,pathBounds,onMutation=()=>{},project,prepareLinked=()=>{},getBitmap=()=>null,getText=()=>null,getLoader=()=>null,getVideo=()=>null,getFilters=()=>null,beforeFilters=()=>{}}) {
    const legacy = new Map();
    const graph=typeof player.runtime_snapshot_json==='function';
    const refKey=ref=>JSON.stringify(ref);
    const nodeKey=node=>graph?refKey(node.object_ref):JSON.stringify(node.path);
    const nodeProxy=node=>graph?proxyForRef(node.object_ref):proxyFor(node.path);
    const invoke=(target,name,...args)=>player[(graph?'object_':'')+name](referenceOf(target).key,...args);
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
        const shapes=leaf.video?[{type:'video_frame',...leaf.video}]:leaf.text?[{type:'text_field',field:leaf.text}]:leaf.owner==='bitmap'?(leaf.bitmap&&!leaf.bitmap.disposed?[{type:'bitmap_pixels',width:leaf.bitmap.width,height:leaf.bitmap.height}]:[]):leaf.kind?[leaf.kind]:[];
        for(const kind of [...shapes,...(leaf.graphics||[])]) {
        const m=relative(leaf,destination,list),stroke=strokes?(kind.stroke_width||0)/2:0;
        const corners=(x,y,w,h)=>{for(const [px,py] of [[x,y],[x+w,y],[x+w,y+h],[x,y+h]]){const p=point(m,px,py);include(p.x,p.y);}};
        if(kind.type==='text_field'){const b=getText().bounds(kind.field);if(b)corners(b.left,b.top,b.right-b.left,b.bottom-b.top);}
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
      get matrix(){const node=descriptor(transformTarget(this));return new Matrix(...(node.local_matrix||matrix(node.transform)));}
      set matrix(value){
        const target=transformTarget(this),components=finiteComponents(value,['a','b','c','d','tx','ty']);
        if(!player.object_set_matrix(referenceOf(target).key,JSON.stringify(components)))throw new RangeError('Runtime rejected the matrix on this display object.');
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
    }
    Object.defineProperties(Transform,{__nfIs:{value:value=>transformTargets.has(value)},__nfInitialize:{value:()=>{throw new TypeError('Native Transform subclassing is not supported.');}}});
    for(const name of ['matrix3D','perspectiveProjection'])Object.defineProperty(Transform.prototype,name,{get(){throw new TypeError(`Transform.${name} requires the unimplemented 3D family.`);},set(){throw new TypeError(`Transform.${name} requires the unimplemented 3D family.`);}});
    for(const name of ['getRelativeMatrix3D'])Object.defineProperty(Transform.prototype,name,{value(){throw new TypeError(`Transform.${name} requires the unimplemented 3D family.`);}});
    function transform(target){descriptor(target);if(!transformCache.has(target))transformCache.set(target,new Transform(target));return transformCache.get(target);}
    function parentSpace(target) {const list=nodes(),node=descriptor(target,list),parent=parentNode(node,list);return parent?nodeProxy(parent):node.on_stage===false?null:getStage();}
    function children(target) {const list=nodes(),parent=descriptor(target,list);return graph?parent.children.map(ref=>list.find(node=>nodeKey(node)===refKey(ref))).filter(Boolean):list.filter(node=>node.path.length===parent.path.length+1&&within(node.path,parent.path));}
    function requireContainer(target) {const node=descriptor(target);if(!['movie_clip','sprite','graphic','button','loader'].includes(node.owner))throw new TypeError('Child traversal requires a display container.');return node;}
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
        beginGradientFill:(...args)=>apply({op:'begin_gradient_fill',gradient:gradient(...args)}),
        lineGradientStyle:(...args)=>apply({op:'line_gradient_style',gradient:gradient(...args)}),
        endFill:()=>apply({op:'end_fill'}),
        lineStyle:(width=NaN,color=0,alpha=1,pixelHinting=false,scaleMode='normal',caps=null,joints=null,miterLimit=3)=>{
          if(pixelHinting!==false||scaleMode!=='normal'||(caps!==null&&caps!=='round')||(joints!==null&&joints!=='round')||miterLimit!==3)throw new TypeError('Graphics.lineStyle currently supports scaling strokes with round caps/joins, no pixel hinting, and default miterLimit.');
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
        setChildIndex:(child,index)=>{if(!Number.isInteger(index)||index<0)throw new RangeError('Child index requires a nonnegative integer.');mutate({op:'set_index',parent:ref(),child:childRef(child),index});},
        swapChildren:(first,second)=>{mutate({op:'swap',parent:ref(),first:childRef(first),second:childRef(second)});},
        swapChildrenAt:(first,second)=>{const api=methods(target);mutate({op:'swap',parent:ref(),first:childRef(api.getChildAt(first)),second:childRef(api.getChildAt(second))});},
      };
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
        beginGradientFill:(type,colors,alphas,ratios,matrix,spread='pad',interpolation='RGB',focal=0)=>drawing.beginGradientFill(type,colors,graphicsNumbers(alphas,'AS2 alphas',15).map(alpha=>alpha/100),ratios,matrix,spread,interpolation==='RGB'?'rgb':interpolation,focal),
        lineGradientStyle:(type,colors,alphas,ratios,matrix,spread='pad',interpolation='RGB',focal=0)=>drawing.lineGradientStyle(type,colors,graphicsNumbers(alphas,'AS2 alphas',15).map(alpha=>alpha/100),ratios,matrix,spread,interpolation==='RGB'?'rgb':interpolation,focal),
        endFill:drawing.endFill,lineStyle:(width,color=0,alpha=100,...options)=>drawing.lineStyle(width,color,alpha/100,...options),
        moveTo:drawing.moveTo,lineTo:drawing.lineTo,curveTo:drawing.curveTo,clear:drawing.clear,
        getNextHighestDepth:()=>children(target).length,
        attachBitmap:(data,depth,pixelSnapping='auto',smoothing=false)=>{
          if(depth!==children(target).length)throw new RangeError('attachBitmap currently supports append-only depth: pass getNextHighestDepth().');
          const image=new builtins['flash.display.Bitmap'](data,pixelSnapping,smoothing);containerMethods(target).addChild(image);
        },
        createTextField:(name,depth,x,y,width,height)=>{
          if(typeof name!=='string'||new TextEncoder().encode(name).length>4096)throw new TypeError('createTextField requires a name of at most 4096 UTF-8 bytes.');
          if(depth!==children(target).length)throw new RangeError('createTextField currently supports append-only depth: pass getNextHighestDepth().');
          if(![x,y,width,height].every(Number.isFinite)||width<0||height<0)throw new RangeError('createTextField requires finite coordinates and nonnegative box dimensions.');
          const text=new builtins['flash.text.TextField']();text.name=name;text.x=x;text.y=y;text.width=width;text.height=height;
          containerMethods(target).addChild(text);return text;
        },
        createEmptyMovieClip:(name,depth)=>{
          if(typeof name!=='string'||new TextEncoder().encode(name).length>4096)throw new TypeError('createEmptyMovieClip requires a name of at most 4096 UTF-8 bytes.');
          if(depth!==children(target).length)throw new RangeError('createEmptyMovieClip currently supports append-only depth: pass getNextHighestDepth(). Flash depth replacement is pending.');
          const clip=proxyForRef(JSON.parse(player.create_display('movie_clip')));clip.name=name;
          containerMethods(target).addChild(clip);return clip;
        },
        removeMovieClip:()=>{const parent=parentSpace(target);if(parent&&parent!==getStage())containerMethods(parent).removeChild(target);else if(descriptor(target).authored_path!==null)throw new TypeError('Authored timelines cannot be removed at runtime.');},
        attachMovie:(identifier,name,depth,initObject)=>{
          requireContainer(target);
          if(typeof identifier!=='string'||!identifier)throw new TypeError('attachMovie requires the exact Export identifier of a MovieClip symbol.');
          if(typeof name!=='string'||new TextEncoder().encode(name).length>4096)throw new TypeError('attachMovie requires a name of at most 4096 UTF-8 bytes.');
          if(depth!==children(target).length)throw new RangeError('attachMovie currently supports append-only depth: pass getNextHighestDepth(). Flash depth replacement is pending.');
          // The bounded initObject profile copies ordinary enumerable own data
          // before the linked constructor, without invoking foreign getters.
          const values=[];
          if(initObject!==null&&typeof initObject==='object'){
            const descriptors=Object.getOwnPropertyDescriptors(initObject);
            for(const key of Object.keys(descriptors))if(descriptors[key].enumerable){
              if(!Object.hasOwn(descriptors[key],'value'))throw new TypeError('attachMovie initObject currently requires enumerable data properties; accessors are unsupported.');
              if(values.length>=1024)throw new RangeError('attachMovie initObject exceeds 1024 properties.');
              values.push([key,descriptors[key].value]);
            }
          }
          const clip=proxyForRef(JSON.parse(player.create_linked(identifier)));clip.name=name;
          for(const [key,value]of values){if(key==='onEnterFrame'||key==='onPress')setLegacyCallback(clip,key,value);else clip[key]=value;}
          const events=mutate({op:'add',parent:referenceOf(target).objectRef,child:referenceOf(clip).objectRef,index:null},false);
          prepareLinked(clip,false);onMutation(events);return clip;
        },
      };
    }
    function methods(target) {
      const result={
        getBounds: space=>bounds(target,space===undefined?target:space,true),
        getRect: space=>bounds(target,space===undefined?target:space,false),
        localToGlobal: value=>convert(target,value,true),
        globalToLocal: value=>convert(target,value,false),
        getChildAt: index=>{requireContainer(target);const list=children(target);if(!Number.isInteger(index)||index<0||index>=list.length)throw new RangeError('Child index is outside this container.');return nodeProxy(list[index]);},
        getChildByName: name=>{requireContainer(target);const child=children(target).find(node=>node.name===name);return child?nodeProxy(child):null;},
        getChildIndex: child=>{requireContainer(target);const node=descriptor(child),index=children(target).findIndex(candidate=>nodeKey(candidate)===nodeKey(node));if(index<0)throw new RangeError('Object is not a direct child of this container.');return index;},
        contains: child=>{requireContainer(target);const parent=descriptor(target),node=descriptor(child);return descendant(node,parent,nodes());},
      };
      if(graph&&['movie_clip','sprite','graphic','button'].includes(descriptor(target).owner))Object.assign(result,containerMethods(target));
      if(graph&&descriptor(target).owner==='movie_clip')Object.assign(result,legacyDrawing(target));
      if(graph&&descriptor(target).owner==='loader')Object.assign(result,getLoader().methods(target));
      if(graph&&descriptor(target).owner==='video'&&getVideo())Object.assign(result,getVideo().methods(target));
      if(descriptor(target).owner==='text'&&getText())Object.assign(result,getText().methods(referenceOf(target).key));
      else if(descriptor(target).owner==='text')Object.assign(result,{
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
      if(descriptor(target).owner==='video'&&getVideo()){const host=getVideo(),value=host.get(target,name);if(value!==host.UNHANDLED)return value;}
      if(name==='filters'){
        descriptor(target);const host=getFilters();if(!host)throw new Error('Display filters require the shared filter runtime. Re-export this project.');
        return host.fromSpecs(JSON.parse(player.object_filters(referenceOf(target).key)));
      }
      if(name==='loaderInfo'){descriptor(target);return getLoader()?.infoForDisplay(target)||null;}
      if(['content','contentLoaderInfo'].includes(name)&&descriptor(target).owner==='loader')return getLoader().get(target,name);
      if(name==='transform')return transform(target);
      if(name==='mask'){
        descriptor(target);const value=JSON.parse(player.object_mask(referenceOf(target).key));
        return value===null?null:proxyForRef(value);
      }
      if(name==='scrollRect'){
        descriptor(target);const value=JSON.parse(player.object_scroll_rect(referenceOf(target).key));
        return value===null?null:new Rectangle(value.x,value.y,value.width,value.height);
      }
      if(getText()&&descriptor(target).owner==='text'){
        const text=getText(),value=text.get(referenceOf(target).key,name);if(value!==text.UNHANDLED)return value;
      }
      if(['bitmapData','smoothing','pixelSnapping'].includes(name)&&descriptor(target).owner==='bitmap'){
        const key=referenceOf(target).key;
        if(name==='bitmapData'){const ref=JSON.parse(player.object_bitmap(key));return ref?getBitmap().fromRef(ref):null;}
        return name==='smoothing'?player.object_smoothing(key):player.object_pixel_snapping(key);
      }
      if(textProperties.has(name)||unsupportedText.has(name)) {
        const node=descriptor(target);
        if(node.owner==='text') {
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
      if(name==='numChildren'){requireContainer(target);return children(target).length;}
      if(name==='width'||name==='_width')return bounds(target,parentSpace(target),true).width;
      if(name==='height'||name==='_height')return bounds(target,parentSpace(target),true).height;
      if(name==='stage') {return descriptor(target).on_stage===false?null:getStage();}
      if(name==='graphics'&&graph&&['movie_clip','sprite','shape'].includes(descriptor(target).owner))return graphics(target);
      return UNHANDLED;
    }
    function set(target,name,value) {
      if(descriptor(target).owner==='video'&&getVideo()&&getVideo().set(target,name,value))return true;
      if(name==='filters'){
        descriptor(target);const host=getFilters();if(!host)throw new Error('Display filters require the shared filter runtime. Re-export this project.');
        const specs=host.toSpecs(value);beforeFilters(target,specs);
        player.object_set_filters(referenceOf(target).key,JSON.stringify(specs));return true;
      }
      if(name==='loaderInfo'||(['content','contentLoaderInfo'].includes(name)&&descriptor(target).owner==='loader'))throw new TypeError(`Display ${name} is read-only.`);
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
      if(getText()&&descriptor(target).owner==='text'){
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
        if(node.owner==='text') {
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
        const node=descriptor(target);if(name[0]==='_')value=Number(value);
        if(!Number.isFinite(value)||value<0)throw new RangeError('Display dimensions must be finite nonnegative numbers.');
        if(nonAxisAligned(node))throw new RangeError('Setting dimensions of a rotated object or skewed object is not supported yet. Set scaleX/scaleY explicitly; transformed bounds remain available.');
        const axis=name.includes('width')?'width':'height',local=bounds(target,target,true)[axis];
        if(local===0){if(value===0)return true;throw new RangeError('Cannot resize empty geometry to a nonzero dimension.');}
        if(!invoke(target,'set_property',axis==='width'?'scale_x':'scale_y',value/local))throw new RangeError('Runtime rejected the requested display dimension.');return true;
      }
      if(name==='numChildren'||name==='stage'||(name==='graphics'&&graph&&['movie_clip','sprite','shape'].includes(descriptor(target).owner)))throw new TypeError(`${name} is read-only.`);
      return false;
    }
    function setLegacyCallback(target,name,value) {
      if(!referenceOf(target)){target[name]=value;return value;}
      const node=descriptor(target);
      if(!['movie_clip','button'].includes(node.owner))throw new TypeError('AS2 callbacks currently require an authored MovieClip or Button instance.');
      if(name!=='onEnterFrame'&&name!=='onPress')throw new TypeError(`AS2 callback ${name} is not supported yet.`);
      if(value!==null&&value!==undefined&&typeof value!=='function')throw new TypeError(`${name} requires a function or null.`);
      target[name]=value;
      const ref=referenceOf(target),id=ref.key+'\u0000'+ref.lifetime;
      let record=legacy.get(id);if(!record){record={target,ref,handlers:new Map(),dynamic:false};legacy.set(id,record);}
      if(value==null)record.handlers.delete(name);else record.handlers.set(name,value);
      if(!record.handlers.size&&!record.dynamic)legacy.delete(id);
      return value;
    }
    function refreshLegacyCallbacks(target) {
      if(!['movie_clip','button'].includes(descriptor(target).owner))return;
      const ref=referenceOf(target),id=ref.key+'\u0000'+ref.lifetime;
      let record=legacy.get(id);
      if(!record){record={target,ref,handlers:new Map(),dynamic:true};legacy.set(id,record);}
      record.dynamic=true;
    }
    function callback(record,name) {
      const handler=record?.dynamic?record.target[name]:record?.handlers.get(name);
      return typeof handler==='function'?handler:null;
    }
    function alive(target){const ref=referenceOf(target);return !!ref&&(graph?player.object_alive(ref.key):player.lifetime_id(ref.key)===ref.lifetime);}
    function prune(){for(const[id,record]of legacy)if(!alive(record.target))legacy.delete(id);}
    function enterFrame(){prune();for(const record of [...legacy.values()]){if(!alive(record.target))continue;const handler=callback(record,'onEnterFrame');if(handler)Reflect.apply(handler,record.target,[]);}}
    function press(target){prune();while(target&&referenceOf(target)){const ref=referenceOf(target),record=legacy.get(ref.key+'\u0000'+ref.lifetime),handler=callback(record,'onPress');if(handler){Reflect.apply(handler,target,[]);return;}target=parentSpace(target);if(target===getStage())target=null;}}
    function pixelSnapping(value){if(!['auto','always','never'].includes(value))throw new TypeError('Bitmap.pixelSnapping requires auto, always or never.');return value;}
    function bitmapArguments(args){
      if(args.length>3)throw new TypeError('Bitmap expects optional bitmapData, pixelSnapping and smoothing.');
      const [data=null,snap='auto',smooth=false]=args;pixelSnapping(snap);if(typeof smooth!=='boolean')throw new TypeError('Bitmap.smoothing requires a Boolean.');
      if(data!==null)getBitmap().refOf(data);return [data,snap,smooth];
    }
    function initializeBitmap(receiver,args){const [data,snap,smooth]=bitmapArguments(args);set(receiver,'bitmapData',data);set(receiver,'pixelSnapping',snap);set(receiver,'smoothing',smooth);}
    const builtins={'flash.display.Graphics':Graphics,'flash.geom.Point':Point,'flash.geom.Rectangle':Rectangle,'flash.geom.Matrix':Matrix,'flash.geom.ColorTransform':ColorTransform,'flash.geom.Transform':Transform};
    for(const[name,type]of Object.entries(graphicsEnums))builtins['flash.display.'+name]=type;
    for(const[name,owners]of Object.entries({DisplayObject:null,DisplayObjectContainer:['movie_clip','sprite','graphic','button','loader'],MovieClip:['movie_clip'],Sprite:['movie_clip','sprite','graphic','button'],Shape:['shape'],Bitmap:['bitmap'],TextField:['text']})) {
      const allocation={Sprite:'sprite',Shape:'shape',MovieClip:'movie_clip',Bitmap:'bitmap',TextField:'text_field'}[name];
      function allocate(){if(!graph||!allocation)throw new TypeError(`Dynamic new ${name}() is not supported. Use an authored instance for this owner.`);return proxyForRef(JSON.parse(player.create_display(allocation)));}
      function NativeDisplay(...args){if(name==='Bitmap'){bitmapArguments(args);const receiver=allocate();initializeBitmap(receiver,args);return receiver;}if(args.length)throw new TypeError(`${name} construction requires no arguments.`);return allocate();}
      if(allocation&&graph)NativeDisplay.__nfAllocate=({className}={})=>{
        const symbol=project?.symbols.find(symbol=>symbol.linkage_id&&symbol.class_name===className);
        if(!symbol)return allocate();
        const receiver=proxyForRef(JSON.parse(player.create_linked(symbol.linkage_id)));
        prepareLinked(receiver,true);return receiver;
      };
      NativeDisplay.__nfIs=value=>{if(value&&value===getStage())return name==='DisplayObject'||name==='DisplayObjectContainer';try{const node=descriptor(value);return owners===null||owners.includes(node.owner);}catch(_){return false;}};
      NativeDisplay.__nfInitialize=(receiver,args)=>{if(name==='Bitmap'){if(!NativeDisplay.__nfIs(receiver))throw new TypeError('Bitmap base initialization requires a Bitmap display object.');initializeBitmap(receiver,args);return;}if(args.length||!NativeDisplay.__nfIs(receiver))throw new TypeError(`${name} base initialization requires an existing compatible display instance and no arguments.`);if(name==='MovieClip')refreshLegacyCallbacks(receiver);};
      Object.defineProperty(NativeDisplay,Symbol.hasInstance,{value:NativeDisplay.__nfIs});
      builtins[(name==='TextField'?'flash.text.':'flash.display.')+name]=NativeDisplay;
    }
    return {methods,get,set,builtins,descriptor,nodes,alive,convert,bounds,parentSpace,children,nodeProxy,setLegacyCallback,refreshLegacyCallbacks,enterFrame,press,prune,clear:()=>legacy.clear(),UNHANDLED};
  }
  const api={create,Point,Rectangle,Matrix,ColorTransform,...graphicsEnums,normalizedRotation,UNHANDLED};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashDisplay=api;
})(globalThis);
