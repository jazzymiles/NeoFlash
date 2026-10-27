/* AS3 geometry transport only. All geometry state and calculations live in Rust/WASM.
 * Rust algorithm provenance and MIT notices: docs/AS3-REUSE-PROVENANCE.md. */
(function(global){
  'use strict';
  function create(wasm){
    if(typeof wasm?.Geom3dValue!=='function')throw new Error('Rust geometry bindings are missing; rebuild and re-export this project.');
    const native=new WeakMap();
    let numberVector=values=>values;
    function array(value){
      if(value==null)return null;
      if(!Number.isInteger(value.length)||value.length<0||value.length>12288)throw new RangeError('Geometry arrays require 0 through 12288 components.');
      const result=[];for(let i=0;i<value.length;i++)result.push(Number(value[i]));return result;
    }
    function data(value){
      const state=native.get(value);
      if(!state)throw new TypeError('Geometry operation requires a Vector3D or Matrix3D instance.');
      return JSON.parse(state.snapshot());
    }
    function argument(value){return native.has(value)?data(value):value;}
    function invoke(receiver,operation,args=[]){
      const state=native.get(receiver);if(!state)throw new TypeError('Geometry method has an invalid receiver.');
      try{return JSON.parse(state.call(operation,JSON.stringify(args.map(argument))));}
      catch(error){throw new Error(String(error));}
    }
    function result(value){
      if(value?.type==='Vector3D')return new Vector3D(...value.data);
      if(value?.type==='Matrix3D')return new Matrix3D(value.data);
      return value;
    }
    class Vector3D {
      constructor(x=0,y=0,z=0,w=0){native.set(this,new wasm.Geom3dValue('Vector3D',JSON.stringify([Number(x),Number(y),Number(z),Number(w)])));Object.preventExtensions(this);}
      static distance(a,b){return invoke(a,'distance',[b]);}
      static angleBetween(a,b){return invoke(a,'angleBetween',[b]);}
      equals(other,allFour=false){return invoke(this,'equals',[other,Boolean(allFour)]);}
      nearEquals(other,tolerance,allFour=false){return invoke(this,'nearEquals',[other,Number(tolerance),Boolean(allFour)]);}
      setTo(x,y,z){invoke(this,'setTo',[Number(x),Number(y),Number(z)]);}
      scaleBy(value){invoke(this,'scaleBy',[Number(value)]);}
    }
    for(const name of ['x','y','z','w'])Object.defineProperty(Vector3D.prototype,name,{get(){return invoke(this,name);},set(value){invoke(this,'set:'+name,[Number(value)]);}});
    for(const name of ['length','lengthSquared'])Object.defineProperty(Vector3D.prototype,name,{get(){return invoke(this,name);}});
    for(const name of ['add','subtract','crossProduct','dotProduct','incrementBy','decrementBy','copyFrom'])Vector3D.prototype[name]=function(other){return result(invoke(this,name,[other]));};
    for(const name of ['clone','normalize','negate','project','toString'])Vector3D.prototype[name]=function(){return result(invoke(this,name));};
    Object.defineProperties(Vector3D,{X_AXIS:{value:new Vector3D(1,0,0)},Y_AXIS:{value:new Vector3D(0,1,0)},Z_AXIS:{value:new Vector3D(0,0,1)}});
    class Matrix3D {
      constructor(values=null){native.set(this,new wasm.Geom3dValue('Matrix3D',JSON.stringify(array(values))));Object.preventExtensions(this);}
      get rawData(){return numberVector(invoke(this,'rawData'));}
      set rawData(values){invoke(this,'set:rawData',[array(values)]);}
      get position(){return result(invoke(this,'position'));}
      set position(value){invoke(this,'set:position',[value]);}
      get determinant(){return invoke(this,'determinant');}
      appendRotation(degrees,axis,pivot=null){invoke(this,'appendRotation',[Number(degrees),axis,pivot]);}
      prependRotation(degrees,axis,pivot=null){invoke(this,'prependRotation',[Number(degrees),axis,pivot]);}
      copyToMatrix3D(destination){invoke(destination,'copyFrom',[this]);}
      copyRowFrom(index,source){invoke(this,'copyRowFrom',[Number(index),source]);}
      copyColumnFrom(index,source){invoke(this,'copyColumnFrom',[Number(index),source]);}
      copyRowTo(index,destination){const output=invoke(this,'copyRowTo',[Number(index)]);invoke(destination,'set:components',[output.data]);}
      copyColumnTo(index,destination){const output=invoke(this,'copyColumnTo',[Number(index)]);invoke(destination,'set:components',[output.data]);}
      copyRawDataFrom(source,index=0,transpose=false){invoke(this,'copyRawDataFrom',[array(source),Number(index),Boolean(transpose)]);}
      copyRawDataTo(destination,index=0,transpose=false){const output=invoke(this,'copyRawDataTo',[array(destination),Number(index),Boolean(transpose)]);copyArray(destination,output);}
      transformVectors(input,output){copyArray(output,invoke(this,'transformVectors',[array(input),array(output)]));}
      static interpolate(){throw new Error('Matrix3D.interpolate is unsupported in the current Rust profile.');}
    }
    class Utils3D {
      constructor(){throw new TypeError('Utils3D exposes static geometry methods and cannot be constructed.');}
      static projectVector(matrix,vector){return result(invoke(matrix,'projectVector',[vector]));}
      static projectVectors(matrix,vertices,projected,uvts){
        const output=invoke(matrix,'projectVectors',[array(vertices),array(projected),array(uvts)]);
        copyArray(projected,output.projected);copyArray(uvts,output.uvts);
      }
      static pointTowards(){throw new Error('Utils3D.pointTowards is unsupported in the current Rust profile.');}
    }
    function copyArray(destination,values){if(destination.length<values.length)destination.length=values.length;for(let i=0;i<values.length;i++)destination[i]=values[i];}
    for(const name of ['appendTranslation','prependTranslation','appendScale','prependScale'])Matrix3D.prototype[name]=function(x,y,z){invoke(this,name,[Number(x),Number(y),Number(z)]);};
    for(const name of ['append','prepend','copyFrom','transformVector','deltaTransformVector'])Matrix3D.prototype[name]=function(value){return result(invoke(this,name,[value]));};
    for(const name of ['identity','transpose','invert','clone','decompose','recompose','interpolateTo','pointAt'])Matrix3D.prototype[name]=function(){return result(invoke(this,name));};
    for(const Type of [Vector3D,Matrix3D])Object.defineProperty(Type,'__nfIs',{value:value=>value instanceof Type&&native.has(value)});
    return {Vector3D,Matrix3D,Utils3D,setNumberVectorFactory(factory){if(typeof factory!=='function')throw new TypeError('Number Vector factory must be callable.');numberVector=factory;}};
  }
  const api={create};if(typeof module!=='undefined'&&module.exports)module.exports=api;global.NeoFlashGeom3D=api;
})(typeof globalThis!=='undefined'?globalThis:this);
