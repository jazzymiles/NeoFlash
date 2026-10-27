/* Pure finite-number geometry profile; original implementation, no VM or DOM.
 * Primary contracts:
 * https://airsdk.dev/reference/actionscript/3.0/flash/geom/Point.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/geom/Rectangle.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/geom/Matrix.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/geom/ColorTransform.html
 * Order/empty-area details were cross-checked against the corresponding .as
 * files under core/src/avm2/globals/flash/geom at Ruffle revision
 * fe6cfc545405b888ebb478ce1c211d63147ca49b. No upstream code is incorporated.
 * Boundary: numeric finite 2D values and ordinary point/rectangle/matrix inputs.
 * Nonfinite values, overflow and singular inversion diagnose, rather than
 * reproducing historical NaN/coercion rules. Angles are radians. Mutators return
 * undefined and validate a complete result before writing it. This does not
 * implement Vector3D, Matrix3D, native class-slot
 * conformance or later output-parameter API extensions. No prototypes are patched.
 */
(function (global) {
  'use strict';
  const pointKeys=['x','y'], rectKeys=['x','y','width','height'], matrixKeys=['a','b','c','d','tx','ty'];
  const colorKeys=['redMultiplier','greenMultiplier','blueMultiplier','alphaMultiplier','redOffset','greenOffset','blueOffset','alphaOffset'];
  function finite(value) {
    if(typeof value!=='number'||!Number.isFinite(value))throw new RangeError('Geometry currently requires finite numeric values; numeric coercion and nonfinite results are not supported.');
    return value;
  }
  function read(object,keys) {
    if(object===null||typeof object!=='object')throw new TypeError('Geometry requires an object with the expected numeric coordinates.');
    return keys.map(key=>finite(object[key]));
  }
  function write(object,keys,values) {
    values.forEach(finite);
    keys.forEach((key,index)=>{object[key]=values[index];});
  }
  function slots(object,keys,values) {
    values.forEach(finite);
    keys.forEach((key,index)=>Object.defineProperty(object,key,{enumerable:true,get:()=>values[index],set:value=>{values[index]=finite(value);}}));
  }
  function rectangleEdges(object) {
    const [x,y,w,h]=read(object,rectKeys);
    return [x,y,finite(x+w),finite(y+h),w,h];
  }
  function pointAt(values,position,delta=false) {
    const [a,b,c,d,tx,ty]=values,[x,y]=read(position,pointKeys);
    return new Point(a*x+c*y+(delta?0:tx),b*x+d*y+(delta?0:ty));
  }
  function compose(before,after) {
    // Column-vector convention: concat(after) maps by before, then by after.
    const rows=([a,b,c,d,tx,ty])=>[[a,c,tx],[b,d,ty],[0,0,1]];
    const left=rows(after),right=rows(before),result=[];
    for(let row=0;row<2;row++)for(let column=0;column<3;column++) {
      let value=0;for(let index=0;index<3;index++)value+=left[row][index]*right[index][column];
      result.push(finite(value));
    }
    return [result[0],result[3],result[1],result[4],result[2],result[5]];
  }

  class Point {
    constructor(x=0,y=0){slots(this,pointKeys,[x,y]);}
    get length(){const [x,y]=read(this,pointKeys);return finite(Math.hypot(x,y));}
    clone(){return new Point(...read(this,pointKeys));}
    copyFrom(point){write(this,pointKeys,read(point,pointKeys));}
    setTo(x,y){write(this,pointKeys,[x,y]);}
    add(point){const [x,y]=read(point,pointKeys);return new Point(this.x+x,this.y+y);}
    subtract(point){const [x,y]=read(point,pointKeys);return new Point(this.x-x,this.y-y);}
    equals(point){const [x,y]=read(point,pointKeys);return this.x===x&&this.y===y;}
    offset(dx,dy){write(this,pointKeys,[this.x+finite(dx),this.y+finite(dy)]);}
    normalize(thickness){finite(thickness);const length=this.length;if(length!==0)write(this,pointKeys,[this.x/length*thickness,this.y/length*thickness]);}
    toString(){return `(x=${this.x}, y=${this.y})`;}
    static distance(first,second){const [x,y]=read(first,pointKeys),[u,v]=read(second,pointKeys);return finite(Math.hypot(x-u,y-v));}
    static interpolate(first,second,factor){const [x,y]=read(first,pointKeys),[u,v]=read(second,pointKeys);finite(factor);return new Point(u+(x-u)*factor,v+(y-v)*factor);}
    static polar(length,angle){finite(length);finite(angle);return new Point(length*Math.cos(angle),length*Math.sin(angle));}
  }

  class Rectangle {
    constructor(x=0,y=0,width=0,height=0){slots(this,rectKeys,[x,y,width,height]);}
    get left(){return this.x;}
    set left(value){finite(value);write(this,rectKeys,[value,this.y,this.right-value,this.height]);}
    get top(){return this.y;}
    set top(value){finite(value);write(this,rectKeys,[this.x,value,this.width,this.bottom-value]);}
    get right(){return finite(this.x+this.width);}
    set right(value){this.width=finite(value)-this.x;}
    get bottom(){return finite(this.y+this.height);}
    set bottom(value){this.height=finite(value)-this.y;}
    get topLeft(){return new Point(this.x,this.y);}
    set topLeft(value){const [x,y]=read(value,pointKeys);write(this,rectKeys,[x,y,this.right-x,this.bottom-y]);}
    get bottomRight(){return new Point(this.right,this.bottom);}
    set bottomRight(value){const [x,y]=read(value,pointKeys);write(this,rectKeys,[this.x,this.y,x-this.x,y-this.y]);}
    get size(){return new Point(this.width,this.height);}
    set size(value){const [width,height]=read(value,pointKeys);write(this,rectKeys,[this.x,this.y,width,height]);}
    // Existing NeoFlash getBounds returns these read-only AS2 aliases.
    get xMin(){return this.left;} get yMin(){return this.top;}
    get xMax(){return this.right;} get yMax(){return this.bottom;}
    clone(){return new Rectangle(...read(this,rectKeys));}
    copyFrom(rectangle){write(this,rectKeys,read(rectangle,rectKeys));}
    setTo(x,y,width,height){write(this,rectKeys,[x,y,width,height]);}
    setEmpty(){write(this,rectKeys,[0,0,0,0]);}
    isEmpty(){return this.width<=0||this.height<=0;}
    equals(rectangle){const values=read(rectangle,rectKeys);return rectKeys.every((key,index)=>this[key]===values[index]);}
    contains(x,y){finite(x);finite(y);return x>=this.x&&y>=this.y&&x<this.right&&y<this.bottom;}
    containsPoint(point){return this.contains(...read(point,pointKeys));}
    containsRect(rectangle){
      const [x,y,right,bottom]=rectangleEdges(rectangle),[left,top,r,b]=rectangleEdges(this);
      return x>=left&&y>=top&&x<r&&y<b&&right>left&&bottom>top&&right<=r&&bottom<=b;
    }
    intersection(rectangle){
      const [x,y,r,b,w,h]=rectangleEdges(this),[u,v,s,t,ow,oh]=rectangleEdges(rectangle);
      if(w<=0||h<=0||ow<=0||oh<=0)return new Rectangle();
      const left=Math.max(x,u),top=Math.max(y,v),right=Math.min(r,s),bottom=Math.min(b,t);
      return right>left&&bottom>top?new Rectangle(left,top,right-left,bottom-top):new Rectangle();
    }
    intersects(rectangle){return !this.intersection(rectangle).isEmpty();}
    union(rectangle){
      const [x,y,r,b,w,h]=rectangleEdges(this),[u,v,s,t,ow,oh]=rectangleEdges(rectangle);
      if(w<=0||h<=0)return new Rectangle(u,v,ow,oh);
      if(ow<=0||oh<=0)return this.clone();
      const left=Math.min(x,u),top=Math.min(y,v);
      return new Rectangle(left,top,Math.max(r,s)-left,Math.max(b,t)-top);
    }
    offset(dx,dy){write(this,rectKeys,[this.x+finite(dx),this.y+finite(dy),this.width,this.height]);}
    offsetPoint(point){this.offset(...read(point,pointKeys));}
    inflate(dx,dy){finite(dx);finite(dy);write(this,rectKeys,[this.x-dx,this.y-dy,this.width+2*dx,this.height+2*dy]);}
    inflatePoint(point){this.inflate(...read(point,pointKeys));}
    toString(){return `(x=${this.x}, y=${this.y}, w=${this.width}, h=${this.height})`;}
  }

  class Matrix {
    constructor(a=1,b=0,c=0,d=1,tx=0,ty=0){slots(this,matrixKeys,[a,b,c,d,tx,ty]);}
    clone(){return new Matrix(...read(this,matrixKeys));}
    copyFrom(matrix){write(this,matrixKeys,read(matrix,matrixKeys));}
    setTo(a,b,c,d,tx,ty){write(this,matrixKeys,[a,b,c,d,tx,ty]);}
    identity(){write(this,matrixKeys,[1,0,0,1,0,0]);}
    concat(matrix){write(this,matrixKeys,compose(read(this,matrixKeys),read(matrix,matrixKeys)));}
    transformPoint(point){return pointAt(read(this,matrixKeys),point);}
    deltaTransformPoint(point){return pointAt(read(this,matrixKeys),point,true);}
    invert(){
      const [a,b,c,d,tx,ty]=read(this,matrixKeys),determinant=finite(a*d-b*c);
      if(determinant===0)throw new RangeError('Singular Matrix inversion is not supported by the finite geometry profile.');
      const linear=[d/determinant,-b/determinant,-c/determinant,a/determinant];
      write(this,matrixKeys,[...linear,-(linear[0]*tx+linear[2]*ty),-(linear[1]*tx+linear[3]*ty)]);
    }
    rotate(angle){finite(angle);const c=Math.cos(angle),s=Math.sin(angle);write(this,matrixKeys,compose(read(this,matrixKeys),[c,s,-s,c,0,0]));}
    scale(sx,sy){finite(sx);finite(sy);write(this,matrixKeys,compose(read(this,matrixKeys),[sx,0,0,sy,0,0]));}
    translate(dx,dy){const values=read(this,matrixKeys);values[4]+=finite(dx);values[5]+=finite(dy);write(this,matrixKeys,values);}
    createBox(sx,sy,rotation=0,tx=0,ty=0){
      [sx,sy,rotation,tx,ty].forEach(finite);
      const c=Math.cos(rotation),s=Math.sin(rotation);
      // Flash createBox is rotation, then world-axis scale, then translation.
      write(this,matrixKeys,[c*sx,s*sy,-s*sx,c*sy,tx,ty]);
    }
    createGradientBox(width,height,rotation=0,tx=0,ty=0){
      [width,height,rotation,tx,ty].forEach(finite);
      this.createBox(width/1638.4,height/1638.4,rotation,finite(tx+width/2),finite(ty+height/2));
    }
    toString(){return `(${matrixKeys.map(key=>`${key}=${this[key]}`).join(', ')})`;}
  }
  class ColorTransform {
    constructor(redMultiplier=1,greenMultiplier=1,blueMultiplier=1,alphaMultiplier=1,redOffset=0,greenOffset=0,blueOffset=0,alphaOffset=0){
      slots(this,colorKeys,[redMultiplier,greenMultiplier,blueMultiplier,alphaMultiplier,redOffset,greenOffset,blueOffset,alphaOffset]);
    }
    get color(){return ((this.redOffset<<16)|(this.greenOffset<<8)|this.blueOffset)>>>0;}
    set color(value){
      if(!Number.isInteger(value)||value<0||value>0xffffffff)throw new RangeError('ColorTransform.color requires an unsigned RGB integer.');
      write(this,colorKeys,[0,0,0,this.alphaMultiplier,(value>>>16)&255,(value>>>8)&255,value&255,this.alphaOffset]);
    }
    concat(second){
      const before=read(this,colorKeys),after=read(second,colorKeys);
      // Apply this transform first, then second (same order as Matrix.concat).
      write(this,colorKeys,[...before.slice(0,4).map((value,index)=>value*after[index]),
        ...before.slice(4).map((value,index)=>value*after[index]+after[index+4])]);
    }
    toString(){return `(${colorKeys.map(key=>`${key}=${this[key]}`).join(', ')})`;}
  }
  for(const Type of [Point,Rectangle,Matrix,ColorTransform])Object.defineProperty(Type,'__nfIs',{value:value=>value instanceof Type});
  ColorTransform.__nfInitialize=()=>{throw new TypeError('Native ColorTransform subclassing is not supported.');};
  for(const name of ['copyColumnFrom','copyColumnTo','copyRowFrom','copyRowTo'])Object.defineProperty(Matrix.prototype,name,{value(){throw new TypeError(`Matrix.${name} requires the unimplemented Vector3D family.`);}});
  const api=Object.freeze({Point,Rectangle,Matrix,ColorTransform,profile:'finite-2d-documented-only'});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;
  else global.NeoFlashGeom=api;
})(globalThis);
