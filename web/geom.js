/* Per-publication geometry facades. All value state and geometry algorithms are
 * in the shared Rust core; this module only validates/marshals host values.
 * Pinned adaptation details: docs/RUNTIME-GEOMETRY-PROVENANCE.md. */
(function(global){
  'use strict';
  const pointKeys=['x','y'],rectKeys=['x','y','width','height'],matrixKeys=['a','b','c','d','tx','ty'];
  const colorKeys=['redMultiplier','greenMultiplier','blueMultiplier','alphaMultiplier','redOffset','greenOffset','blueOffset','alphaOffset'];
  function finite(value){
    if(typeof value!=='number'||!Number.isFinite(value))throw new RangeError('Geometry currently requires finite numeric values; numeric coercion and nonfinite results are not supported.');
    return value;
  }
  function create(wasm,{getVector3D=()=>null}={}){
    if(typeof wasm?.Geom2dValue!=='function')throw new Error('Rust 2D geometry bindings are missing. Rebuild and re-export this project.');
    const native=new WeakMap();
    function read(value,keys){
      if(value===null||typeof value!=='object')throw new TypeError('Geometry requires an object with the expected numeric coordinates.');
      return keys.map(key=>finite(value[key]));
    }
    function invoke(receiver,operation,args=[]){
      const state=native.get(receiver);if(!state)throw new TypeError('Geometry method has an invalid receiver.');
      try{return JSON.parse(state.call(operation,JSON.stringify(args)));}
      catch(error){throw new RangeError(String(error));}
    }
    function calculate(kind,data,operation,args,output){
      try{const value=JSON.parse(wasm.Geom2dValue.calculate(kind,JSON.stringify(data),operation,JSON.stringify(args)));return output===undefined?wrap(value):pointOutput(value,output);}
      catch(error){throw new RangeError(String(error));}
    }
    function initialize(receiver,kind,keys,args,defaults){
      if(native.has(receiver))throw new TypeError('Native geometry initialization may occur only once.');
      const values=defaults.map((fallback,i)=>args[i]===undefined?fallback:finite(args[i]));
      let state;try{state=new wasm.Geom2dValue(kind,JSON.stringify(values));}catch(error){throw new RangeError(String(error));}
      native.set(receiver,state);
      for(const key of keys)Object.defineProperty(receiver,key,{enumerable:true,get:()=>invoke(receiver,key),set:value=>{invoke(receiver,'set:'+key,[finite(value)]);}});
    }
    function wrap(value){
      if(value?.type==='Point')return new Point(...value.data);
      if(value?.type==='Rectangle')return new Rectangle(...value.data);
      if(value?.type==='Matrix')return new Matrix(...value.data);
      if(value?.type==='ColorTransform')return new ColorTransform(...value.data);
      return value;
    }
    function pointOutput(value,output){
      if(output==null)return wrap(value);
      if(!Point.__nfIs(output))throw new TypeError('Geometry output requires a Point instance or null.');
      invoke(output,'setTo',value.data);return output;
    }
    class Point {
      constructor(x=0,y=0){initialize(this,'Point',pointKeys,[x,y],[0,0]);}
      get length(){return invoke(this,'length');}
      clone(){return wrap(invoke(this,'clone'));}
      copyFrom(point){invoke(this,'copyFrom',[read(point,pointKeys)]);}
      setTo(x,y){invoke(this,'setTo',[finite(x),finite(y)]);}
      add(point){return wrap(invoke(this,'add',[read(point,pointKeys)]));}
      subtract(point){return wrap(invoke(this,'subtract',[read(point,pointKeys)]));}
      addToOutput(point,output=null){return pointOutput(invoke(this,'add',[read(point,pointKeys)]),output);}
      subtractToOutput(point,output=null){return pointOutput(invoke(this,'subtract',[read(point,pointKeys)]),output);}
      equals(point){return invoke(this,'equals',[read(point,pointKeys)]);}
      offset(dx,dy){invoke(this,'offset',[finite(dx),finite(dy)]);}
      normalize(thickness){invoke(this,'normalize',[finite(thickness)]);}
      toString(){return invoke(this,'toString');}
      static distance(first,second){return calculate('Point',read(first,pointKeys),'distance',[read(second,pointKeys)]);}
      static interpolate(first,second,factor){return calculate('Point',read(first,pointKeys),'interpolate',[read(second,pointKeys),finite(factor)]);}
      static polar(length,angle){return calculate('Point',[0,0],'polar',[finite(length),finite(angle)]);}
      static interpolateToOutput(first,second,factor,output=null){return calculate('Point',read(first,pointKeys),'interpolate',[read(second,pointKeys),finite(factor)],output);}
      static polarToOutput(length,angle,output=null){return calculate('Point',[0,0],'polar',[finite(length),finite(angle)],output);}
    }
    class Rectangle {
      constructor(x=0,y=0,width=0,height=0){initialize(this,'Rectangle',rectKeys,[x,y,width,height],[0,0,0,0]);}
      clone(){return wrap(invoke(this,'clone'));}
      copyFrom(rectangle){invoke(this,'copyFrom',[read(rectangle,rectKeys)]);}
      setTo(x,y,width,height){invoke(this,'setTo',[finite(x),finite(y),finite(width),finite(height)]);}
      setEmpty(){invoke(this,'setEmpty');}
      isEmpty(){return invoke(this,'isEmpty');}
      equals(rectangle){return invoke(this,'equals',[read(rectangle,rectKeys)]);}
      contains(x,y){return invoke(this,'contains',[finite(x),finite(y)]);}
      containsPoint(point){return invoke(this,'containsPoint',[read(point,pointKeys)]);}
      containsRect(rectangle){return invoke(this,'containsRect',[read(rectangle,rectKeys)]);}
      intersection(rectangle){return wrap(invoke(this,'intersection',[read(rectangle,rectKeys)]));}
      intersects(rectangle){return invoke(this,'intersects',[read(rectangle,rectKeys)]);}
      union(rectangle){return wrap(invoke(this,'union',[read(rectangle,rectKeys)]));}
      offset(dx,dy){invoke(this,'offset',[finite(dx),finite(dy)]);}
      offsetPoint(point){invoke(this,'offsetPoint',[read(point,pointKeys)]);}
      inflate(dx,dy){invoke(this,'inflate',[finite(dx),finite(dy)]);}
      inflatePoint(point){invoke(this,'inflatePoint',[read(point,pointKeys)]);}
      toString(){return invoke(this,'toString');}
    }
    for(const key of ['left','top','right','bottom'])Object.defineProperty(Rectangle.prototype,key,{get(){return invoke(this,key);},set(value){invoke(this,'set:'+key,[finite(value)]);}});
    for(const key of ['topLeft','bottomRight','size'])Object.defineProperty(Rectangle.prototype,key,{get(){return wrap(invoke(this,key));},set(value){invoke(this,'set:'+key,[read(value,pointKeys)]);}});
    for(const key of ['xMin','yMin','xMax','yMax'])Object.defineProperty(Rectangle.prototype,key,{get(){return invoke(this,key);}});
    function vector(value){
      const Type=getVector3D();
      if(typeof Type?.__nfIs!=='function'||!Type.__nfIs(value))throw new TypeError('Matrix row/column operations require the current publication Vector3D instance.');
      return read(value,['x','y','z','w']);
    }
    class Matrix {
      constructor(a=1,b=0,c=0,d=1,tx=0,ty=0){initialize(this,'Matrix',matrixKeys,[a,b,c,d,tx,ty],[1,0,0,1,0,0]);}
      clone(){return wrap(invoke(this,'clone'));}
      copyFrom(matrix){invoke(this,'copyFrom',[read(matrix,matrixKeys)]);}
      setTo(a,b,c,d,tx,ty){invoke(this,'setTo',[a,b,c,d,tx,ty].map(finite));}
      identity(){invoke(this,'identity');}
      concat(matrix){invoke(this,'concat',[read(matrix,matrixKeys)]);}
      transformPoint(point){return wrap(invoke(this,'transformPoint',[read(point,pointKeys)]));}
      deltaTransformPoint(point){return wrap(invoke(this,'deltaTransformPoint',[read(point,pointKeys)]));}
      transformPointToOutput(point,output=null){return pointOutput(invoke(this,'transformPoint',[read(point,pointKeys)]),output);}
      deltaTransformPointToOutput(point,output=null){return pointOutput(invoke(this,'deltaTransformPoint',[read(point,pointKeys)]),output);}
      invert(){invoke(this,'invert');}
      rotate(angle){invoke(this,'rotate',[finite(angle)]);}
      scale(sx,sy){invoke(this,'scale',[finite(sx),finite(sy)]);}
      translate(dx,dy){invoke(this,'translate',[finite(dx),finite(dy)]);}
      createBox(sx,sy,rotation=0,tx=0,ty=0){invoke(this,'createBox',[sx,sy,rotation,tx,ty].map(finite));}
      createGradientBox(width,height,rotation=0,tx=0,ty=0){invoke(this,'createGradientBox',[width,height,rotation,tx,ty].map(finite));}
      copyRowFrom(index,source){invoke(this,'copyRowFrom',[finite(index),vector(source)]);}
      copyColumnFrom(index,source){invoke(this,'copyColumnFrom',[finite(index),vector(source)]);}
      copyRowTo(index,destination){const value=invoke(this,'copyRowTo',[finite(index),vector(destination)]);for(let i=0;i<3;i++)destination[['x','y','z'][i]]=value.data[i];}
      copyColumnTo(index,destination){const value=invoke(this,'copyColumnTo',[finite(index),vector(destination)]);for(let i=0;i<3;i++)destination[['x','y','z'][i]]=value.data[i];}
      toString(){return invoke(this,'toString');}
    }
    function transformerMatrix(value){if(!Matrix.__nfIs(value))throw new TypeError('MatrixTransformer requires a Matrix from the same publication.');return value;}
    class MatrixTransformer {
      constructor(){throw new TypeError('MatrixTransformer provides static methods and cannot be constructed.');}
      static matchInternalPointWithExternal(matrix,internal,external){invoke(transformerMatrix(matrix),'transformer:matchInternalPointWithExternal',[read(internal,pointKeys),read(external,pointKeys)]);}
    }
    for(const name of ['getScaleX','getScaleY','getSkewX','getSkewXRadians','getSkewY','getSkewYRadians','getRotation','getRotationRadians'])MatrixTransformer[name]=matrix=>invoke(transformerMatrix(matrix),'transformer:'+name);
    for(const name of ['setScaleX','setScaleY','setSkewX','setSkewXRadians','setSkewY','setSkewYRadians','setRotation','setRotationRadians'])MatrixTransformer[name]=(matrix,value)=>{invoke(transformerMatrix(matrix),'transformer:'+name,[finite(value)]);};
    for(const name of ['rotateAroundInternalPoint','rotateAroundExternalPoint'])MatrixTransformer[name]=(matrix,x,y,degrees)=>{invoke(transformerMatrix(matrix),'transformer:'+name,[finite(x),finite(y),finite(degrees)]);};
    MatrixTransformer.__nfInitialize=()=>{throw new TypeError('MatrixTransformer is a static utility and cannot initialize subclasses.');};
    class ColorTransform {
      constructor(redMultiplier=1,greenMultiplier=1,blueMultiplier=1,alphaMultiplier=1,redOffset=0,greenOffset=0,blueOffset=0,alphaOffset=0){initialize(this,'ColorTransform',colorKeys,[redMultiplier,greenMultiplier,blueMultiplier,alphaMultiplier,redOffset,greenOffset,blueOffset,alphaOffset],[1,1,1,1,0,0,0,0]);}
      get color(){return invoke(this,'color');}
      set color(value){invoke(this,'set:color',[finite(value)]);}
      concat(second){invoke(this,'concat',[read(second,colorKeys)]);}
      toString(){return invoke(this,'toString');}
    }
    class MotionColor extends ColorTransform {
      get brightness(){return invoke(this,'motion:brightness');}
      set brightness(value){invoke(this,'motion:setBrightness',[finite(value)]);}
      get tintColor(){return invoke(this,'motion:tintColor');}
      set tintColor(value){invoke(this,'motion:setTintColor',[finite(value)]);}
      get tintMultiplier(){return invoke(this,'motion:tintMultiplier');}
      set tintMultiplier(value){invoke(this,'motion:setTintMultiplier',[finite(value)]);}
      setTint(color,amount){invoke(this,'motion:setTint',[finite(color),finite(amount)]);}
      static interpolateColor(from,to,progress){return calculate('ColorTransform',[1,1,1,1,0,0,0,0],'motion:interpolateColor',[finite(from),finite(to),finite(progress)]);}
      static interpolateTransform(from,to,progress){return calculate('ColorTransform',read(from,colorKeys),'motion:interpolateTransform',[read(to,colorKeys),finite(progress)]);}
      static fromXML(){throw new TypeError('fl.motion.Color.fromXML is outside the current runtime profile.');}
    }
    Object.defineProperty(MotionColor,'__nfIs',{value:value=>value instanceof MotionColor&&native.has(value)});
    MotionColor.__nfInitialize=()=>{throw new TypeError('Native fl.motion.Color subclass initialization is unsupported.');};
    const colorTargets=new WeakMap();
    function boundColor(receiver){
      if(!colorTargets.has(receiver))throw new TypeError('Color method has an invalid receiver.');
      const target=colorTargets.get(receiver);
      if(target===null||typeof target!=='object'||!target.transform)throw new TypeError('Color requires a live display object target; unresolved target paths are unsupported.');
      const transform=target.transform,color=transform.colorTransform;
      if(!ColorTransform.__nfIs(color))throw new TypeError('Color requires a display transform from the same publication.');
      return {transform,color};
    }
    class Color {
      constructor(target){colorTargets.set(this,target);Object.defineProperty(this,'target',{value:target});boundColor(this);}
      getRGB(){return invoke(boundColor(this).color,'legacy:getRGB');}
      setRGB(value){const {transform,color}=boundColor(this);invoke(color,'legacy:setRGB',[finite(value)]);transform.colorTransform=color;}
      getTransform(){return invoke(boundColor(this).color,'legacy:getTransform');}
      setTransform(value){
        if(value===null||typeof value!=='object')throw new TypeError('Color.setTransform requires an object.');
        const values={};for(const key of ['ra','ga','ba','aa','rb','gb','bb','ab'])if(Object.hasOwn(value,key))values[key]=finite(value[key]);
        const {transform,color}=boundColor(this);invoke(color,'legacy:setTransform',[values]);transform.colorTransform=color;
      }
    }
    Object.defineProperty(Color,'__nfIs',{value:value=>value instanceof Color&&colorTargets.has(value)});
    Color.__nfInitialize=()=>{throw new TypeError('Native Color subclassing is not supported.');};
    for(const [Type,kind,keys,defaults] of [[Point,'Point',pointKeys,[0,0]],[Rectangle,'Rectangle',rectKeys,[0,0,0,0]],[Matrix,'Matrix',matrixKeys,[1,0,0,1,0,0]]]){
      Object.defineProperty(Type,'__nfInitialize',{value:(receiver,args)=>initialize(receiver,kind,keys,args,defaults)});
    }
    ColorTransform.__nfInitialize=()=>{throw new TypeError('Native ColorTransform subclassing is not supported.');};
    for(const Type of [Point,Rectangle,Matrix,ColorTransform])Object.defineProperty(Type,'__nfIs',{value:value=>value instanceof Type&&native.has(value)});
    function as2Ref(value,name){
      if(name==='rgb'&&ColorTransform.__nfIs(value))return {has:()=>true,delete:()=>false,get:()=>invoke(value,'color'),set:next=>{invoke(value,'set:color',[finite(next)]);return next;}};
      if(name==='containsRectangle'&&Rectangle.__nfIs(value))return {has:()=>true,delete:()=>false,get:()=>rectangle=>invoke(value,'containsRect',[read(rectangle,rectKeys)]),set:()=>{throw new TypeError('Rectangle.containsRectangle is read-only.');}};
      return null;
    }
    return Object.freeze({Point,Rectangle,Matrix,ColorTransform,Color,MotionColor,MatrixTransformer,as2Ref,profile:'rust-finite-2d'});
  }
  const api=Object.freeze({create});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;
  else global.NeoFlashGeom=api;
})(globalThis);
