/* MovieClip/DisplayObject facade over the shared Rust placement graph. */
(function (global) {
  'use strict';
  const UNHANDLED = Symbol('unhandled display member');
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
  function Point(x=0,y=0) { this.x=Number(x); this.y=Number(y); }
  Point.__nfIs = value => value instanceof Point;
  Point.prototype.clone=function(){return new Point(this.x,this.y);};
  function Rectangle(x=0,y=0,width=0,height=0) { this.x=Number(x);this.y=Number(y);this.width=Number(width);this.height=Number(height); }
  Rectangle.__nfIs=value=>value instanceof Rectangle;
  Rectangle.prototype.clone=function(){return new Rectangle(this.x,this.y,this.width,this.height);};
  Rectangle.prototype.contains=function(x,y){return x>=this.x && y>=this.y && x<this.right && y<this.bottom;};
  for (const [name,get] of Object.entries({left(){return this.x;},top(){return this.y;},right(){return this.x+this.width;},bottom(){return this.y+this.height;},xMin(){return this.x;},yMin(){return this.y;},xMax(){return this.x+this.width;},yMax(){return this.y+this.height;}})) Object.defineProperty(Rectangle.prototype,name,{get});
  function normalizedRotation(value) { const r=value%360;return r>180?r-360:r< -180?r+360:r; }
  const textProperties=new Set(['text','textColor','length','numLines','textWidth','textHeight']);
  const unsupportedText=new Set(['htmlText','defaultTextFormat','styleSheet','type','selectable','multiline','wordWrap','autoSize','embedFonts','displayAsPassword','maxChars','restrict','selectionBeginIndex','selectionEndIndex','caretIndex','scrollH','scrollV','maxScrollH','maxScrollV','bottomScrollV','background','backgroundColor','border','borderColor','antiAliasType','gridFitType','sharpness','thickness','condenseWhite','getTextFormat','setTextFormat','getCharBoundaries','getCharIndexAtPoint','getLineIndexAtPoint','getLineIndexOfChar','getLineMetrics','getLineOffset','getParagraphLength','getFirstCharInParagraph','setSelection','replaceSelectedText']);
  const textValue=value=>value.replace(/\r\n?|\n/g,'\r');

  function create({player,proxyFor,referenceOf,getStage=()=>null,measureText,pathBounds}) {
    const legacy = new Map();
    function nodes() { return JSON.parse(player.display_nodes_json()); }
    function descriptor(target, list=nodes()) {
      const ref=referenceOf(target);
      if (!ref || player.lifetime_id(ref.key)!==ref.lifetime) throw new ReferenceError('Display object was removed, replaced, or belongs to another player.');
      const result=list.find(node=>samePath(node.path,ref.path));
      if (!result) throw new ReferenceError('Display object is no longer active.');
      return result;
    }
    function relative(node, target, list) {
      if (!target) return node.world;
      if (within(node.path,target.path)) {
        let result=identity;
        for(let depth=target.path.length+1;depth<=node.path.length;depth++) {
          const ancestor=list.find(candidate=>samePath(candidate.path,node.path.slice(0,depth)));
          result=multiply(result,matrix(ancestor.transform));
        }
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
        if(!leaf.kind || !within(leaf.path,source.path))continue;
        const kind=leaf.kind,m=relative(leaf,destination,list),stroke=strokes?(kind.stroke_width||0)/2:0;
        const corners=(x,y,w,h)=>{for(const [px,py] of [[x,y],[x+w,y],[x+w,y+h],[x,y+h]]){const p=point(m,px,py);include(p.x,p.y);}};
        if(kind.type==='rectangle')corners(-stroke,-stroke,kind.width+2*stroke,kind.height+2*stroke);
        else if(kind.type==='ellipse') {
          const center=point(m,kind.width/2,kind.height/2);
          const rx=Math.hypot(m[0]*kind.width/2,m[2]*kind.height/2)+stroke*Math.hypot(m[0],m[2]);
          const ry=Math.hypot(m[1]*kind.width/2,m[3]*kind.height/2)+stroke*Math.hypot(m[1],m[3]);
          include(center.x-rx,center.y-ry);include(center.x+rx,center.y+ry);
        } else if(kind.type==='path') {
          const b=pathBounds(kind.commands,m);
          if(b){const dx=stroke*Math.hypot(m[0],m[2]),dy=stroke*Math.hypot(m[1],m[3]);include(b.left-dx,b.top-dy);include(b.right+dx,b.bottom+dy);}
        } else if(kind.type==='text') {
          kind.text.split('\n').forEach((line,i)=>corners(0,i*kind.font_size*1.2,measureText(line,kind),kind.font_size*1.2));
        }
      }
      return box ? new Rectangle(box.left,box.top,box.right-box.left,box.bottom-box.top) : new Rectangle();
    }
    function parentSpace(target) {const node=descriptor(target);return node.path.length?proxyFor(node.path.slice(0,-1)):getStage();}
    function children(target) {const list=nodes(),parent=descriptor(target,list);return list.filter(node=>node.path.length===parent.path.length+1&&within(node.path,parent.path));}
    function requireContainer(target) {const node=descriptor(target);if(!['movie_clip','graphic','button'].includes(node.owner))throw new TypeError('Child traversal requires a display container.');return node;}
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
      if(!player.set_text(referenceOf(target).key,normalized))throw new RangeError('Runtime text must fit within 1048576 UTF-8 bytes on a live TextField.');
    }
    function lineText(target,index) {
      const lines=textValue(textKind(target).text).split('\r');
      if(!Number.isInteger(index)||index<0||index>=lines.length)throw new RangeError('TextField line index is outside the unwrapped text.');
      return lines[index]+(index<lines.length-1?'\r':'');
    }
    function methods(target) {
      const result={
        getBounds: space=>bounds(target,space===undefined?target:space,true),
        getRect: space=>bounds(target,space===undefined?target:space,false),
        localToGlobal: value=>convert(target,value,true),
        globalToLocal: value=>convert(target,value,false),
        getChildAt: index=>{requireContainer(target);const list=children(target);if(!Number.isInteger(index)||index<0||index>=list.length)throw new RangeError('Child index is outside this container.');return proxyFor(list[index].path);},
        getChildByName: name=>{requireContainer(target);const child=children(target).find(node=>node.name===name);return child?proxyFor(child.path):null;},
        getChildIndex: child=>{requireContainer(target);const node=descriptor(child),index=children(target).findIndex(candidate=>samePath(candidate.path,node.path));if(index<0)throw new RangeError('Object is not a direct child of this container.');return index;},
        contains: child=>{requireContainer(target);const parent=descriptor(target),node=descriptor(child);return within(node.path,parent.path);},
      };
      if(descriptor(target).owner==='text')Object.assign(result,{
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
      if(textProperties.has(name)||unsupportedText.has(name)) {
        const node=descriptor(target);
        if(node.owner==='text') {
          if(unsupportedText.has(name))throw new TypeError(`TextField.${name} is not implemented in the plain-text profile. Rich formatting, input, selection and wrapping are pending.`);
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
      if(name==='stage') {descriptor(target);return getStage();}
      return UNHANDLED;
    }
    function set(target,name,value) {
      if(textProperties.has(name)||unsupportedText.has(name)) {
        const node=descriptor(target);
        if(node.owner==='text') {
          if(unsupportedText.has(name))throw new TypeError(`TextField.${name} is not implemented in the plain-text profile. Rich formatting, input, selection and wrapping are pending.`);
          if(name==='text'){writeText(target,value);return true;}
          if(name==='textColor'){
            if(!Number.isInteger(value)||value<0||value>0xffffff)throw new RangeError('textColor currently requires an RGB integer from 0 through 0xffffff.');
            if(!player.set_text_color(referenceOf(target).key,value))throw new Error('Runtime rejected textColor on this TextField.');return true;
          }
          throw new TypeError(`TextField.${name} is read-only.`);
        }
      }
      if(name==='name'||name==='_name') {
        descriptor(target); if(typeof value!=='string')throw new TypeError('Display name must be a string.');
        if(!player.set_name(referenceOf(target).key,value))throw new RangeError('Display name must be at most 4096 UTF-8 bytes.');return true;
      }
      if(name==='width'||name==='height'||name==='_width'||name==='_height') {
        const node=descriptor(target);if(name[0]==='_')value=Number(value);
        if(!Number.isFinite(value)||value<0)throw new RangeError('Display dimensions must be finite nonnegative numbers.');
        if(Math.abs(normalizedRotation(node.transform.rotation)%180)>1e-10)throw new RangeError('Setting dimensions of a rotated object is not supported yet. Set scaleX/scaleY explicitly; transformed bounds remain available.');
        const axis=name.includes('width')?'width':'height',local=bounds(target,target,true)[axis];
        if(local===0){if(value===0)return true;throw new RangeError('Cannot resize empty geometry to a nonzero dimension.');}
        if(!player.set_property(referenceOf(target).key,axis==='width'?'scale_x':'scale_y',value/local))throw new RangeError('Runtime rejected the requested display dimension.');return true;
      }
      if(name==='numChildren'||name==='stage')throw new TypeError(`${name} is read-only.`);
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
    function alive(target){const ref=referenceOf(target);return !!ref&&player.lifetime_id(ref.key)===ref.lifetime;}
    function prune(){for(const[id,record]of legacy)if(!alive(record.target))legacy.delete(id);}
    function enterFrame(){prune();for(const record of [...legacy.values()]){if(!alive(record.target))continue;const handler=callback(record,'onEnterFrame');if(handler)Reflect.apply(handler,record.target,[]);}}
    function press(target){prune();while(target&&referenceOf(target)){const ref=referenceOf(target),record=legacy.get(ref.key+'\u0000'+ref.lifetime),handler=callback(record,'onPress');if(handler){Reflect.apply(handler,target,[]);return;}target=ref.path.length?proxyFor(ref.path.slice(0,-1)):null;}}
    const builtins={'flash.geom.Point':Point,'flash.geom.Rectangle':Rectangle};
    for(const[name,owners]of Object.entries({DisplayObject:null,DisplayObjectContainer:['movie_clip','graphic','button'],MovieClip:['movie_clip'],Sprite:['movie_clip','graphic','button'],TextField:['text']})) {
      function NativeDisplay(){throw new TypeError(`Dynamic new ${name}() placements are not supported yet. Use an authored instance or symbol linkage.`);}
      NativeDisplay.__nfIs=value=>{if(value&&value===getStage())return name==='DisplayObject'||name==='DisplayObjectContainer';try{const node=descriptor(value);return owners===null||owners.includes(node.owner);}catch(_){return false;}};
      NativeDisplay.__nfInitialize=(receiver,args)=>{if(args.length||!NativeDisplay.__nfIs(receiver))throw new TypeError(`${name} base initialization requires an existing compatible authored instance and no arguments.`);};
      Object.defineProperty(NativeDisplay,Symbol.hasInstance,{value:NativeDisplay.__nfIs});
      builtins[(name==='TextField'?'flash.text.':'flash.display.')+name]=NativeDisplay;
    }
    return {methods,get,set,builtins,descriptor,nodes,alive,convert,bounds,setLegacyCallback,refreshLegacyCallbacks,enterFrame,press,prune,clear:()=>legacy.clear(),UNHANDLED};
  }
  const api={create,Point,Rectangle,normalizedRotation,UNHANDLED};
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashDisplay=api;
})(globalThis);
