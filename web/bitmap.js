/* BitmapData facade and read-only Canvas upload cache over the Rust pixel arena.
 * Primary contracts: https://airsdk.dev/reference/actionscript/3.0/flash/display/BitmapData.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/display/Bitmap.html
 * Pixel mutation, premultiplication, clipping, overlapping copies and disposal
 * belong to Rust. JS retains only revision-stamped render copies, never a second
 * editable pixel model. Integer coordinates and uint colors are required; AS2 alpha masks use Rust.
 * AS3 alpha masks, non-normal draw blending and native BitmapData subclassing remain pending.
 * AS2 rectangle aliases rect; invalid/disposed access uses the shared diagnostic
 * profile, not historical AS2 sentinel return values. Pixel snapping is measured
 * in Canvas drawing-buffer pixels, with no Retina/Flash rasterization parity claim.
 */
(function(global){
  'use strict';
  const unsupported=name=>{throw new TypeError(`BitmapData.${name} is not implemented in the shared pixel-buffer profile.`);};
  const vectorBrand=Symbol.for('neoflash.as3.vector.instance.v1');
  const key=reference=>JSON.stringify(reference);
  function integer(value,name){if(!Number.isInteger(value)||value< -2147483648||value>2147483647)throw new RangeError(`${name} requires a signed 32-bit integer.`);return value;}
  function color(value){if(!Number.isInteger(value)||value< -2147483648||value>0xffffffff)throw new RangeError('BitmapData color requires an unsigned 32-bit ARGB integer.');return value>>>0;}
  function as2Int(value,name){const n=Number(value);if(!Number.isFinite(n))throw new RangeError(`${name} requires a finite number.`);return n|0;}
  function as2Color(value){const n=Number(value);if(!Number.isFinite(n))throw new RangeError('BitmapData color requires a finite numeric ARGB value.');return n>>>0;}
  function as2Rectangle(value){if(!value||typeof value!=='object')throw new TypeError('BitmapData operation requires a Rectangle.');return [as2Int(value.x,'Rectangle.x'),as2Int(value.y,'Rectangle.y'),as2Int(value.width,'Rectangle.width'),as2Int(value.height,'Rectangle.height')];}
  function as2Point(value){if(!value||typeof value!=='object')throw new TypeError('BitmapData operation requires a Point.');return [as2Int(value.x,'Point.x'),as2Int(value.y,'Point.y')];}
  function rectangle(value){if(!value)throw new TypeError('BitmapData operation requires a Rectangle.');return [integer(value.x,'Rectangle.x'),integer(value.y,'Rectangle.y'),integer(value.width,'Rectangle.width'),integer(value.height,'Rectangle.height')];}
  function point(value){if(!value)throw new TypeError('BitmapData operation requires a Point.');return [integer(value.x,'Point.x'),integer(value.y,'Point.y')];}
  class PixelSnapping {constructor(){throw new TypeError('PixelSnapping supplies AUTO, ALWAYS and NEVER constants.');}}
  Object.assign(PixelSnapping,{AUTO:'auto',ALWAYS:'always',NEVER:'never'});
  Object.freeze(PixelSnapping);
  class BitmapDataChannel {constructor(){throw new TypeError('BitmapDataChannel supplies ALPHA, BLUE, GREEN and RED constants.');}}
  Object.assign(BitmapDataChannel,{ALPHA:8,BLUE:4,GREEN:2,RED:1});
  Object.freeze(BitmapDataChannel);
  class BitmapEncodingColorSpace {constructor(){throw new TypeError('BitmapEncodingColorSpace supplies COLORSPACE_AUTO, COLORSPACE_4_4_4, COLORSPACE_4_2_2 and COLORSPACE_4_2_0 constants.');}}
  Object.assign(BitmapEncodingColorSpace,{COLORSPACE_AUTO:'auto',COLORSPACE_4_4_4:'4:4:4',COLORSPACE_4_2_2:'4:2:2',COLORSPACE_4_2_0:'4:2:0'});
  Object.defineProperties(BitmapEncodingColorSpace,{__nfIs:{value:()=>false},__nfInitialize:{value:()=>{throw new TypeError('BitmapEncodingColorSpace cannot be instantiated or subclassed.');}}});
  Object.freeze(BitmapEncodingColorSpace);
  const VALID_COLORSPACES=new Set(['auto','4:4:4','4:2:2','4:2:0']);
  const VALID_DRAW_QUALITIES=new Set(['low','medium','high','best','8x8','8x8linear','16x16','16x16linear']);
  const encoderOptions=new WeakMap(),jpegEncoderOptions=new WeakMap(),jpegxrEncoderOptions=new WeakMap();
  class PNGEncoderOptions {
    constructor(fastCompression=false){encoderOptions.set(this,false);this.fastCompression=fastCompression;Object.seal(this);}
    get fastCompression(){if(!encoderOptions.has(this))throw new TypeError('Expected PNGEncoderOptions.');return encoderOptions.get(this);}
    set fastCompression(value){if(!encoderOptions.has(this)||typeof value!=='boolean')throw new TypeError('PNGEncoderOptions.fastCompression requires a Boolean.');encoderOptions.set(this,value);}
  }
  Object.defineProperties(PNGEncoderOptions,{__nfIs:{value:value=>encoderOptions.has(value)},__nfInitialize:{value:()=>unsupported('PNGEncoderOptions subclassing')},[Symbol.hasInstance]:{value:value=>encoderOptions.has(value)}});
  class JPEGEncoderOptions {
    constructor(quality=80){jpegEncoderOptions.set(this,80);this.quality=quality;Object.seal(this);}
    get quality(){if(!jpegEncoderOptions.has(this))throw new TypeError('Expected JPEGEncoderOptions.');return jpegEncoderOptions.get(this);}
    set quality(value){
      if(!jpegEncoderOptions.has(this))throw new TypeError('Expected JPEGEncoderOptions.');
      const n=Number(value),q=n>>>0;
      if(!Number.isFinite(n)||q<1||q>100)throw new RangeError('JPEGEncoderOptions.quality requires an integer from 1 through 100.');
      jpegEncoderOptions.set(this,q);
    }
  }
  Object.defineProperties(JPEGEncoderOptions,{__nfIs:{value:value=>jpegEncoderOptions.has(value)},__nfInitialize:{value:()=>unsupported('JPEGEncoderOptions subclassing')},[Symbol.hasInstance]:{value:value=>jpegEncoderOptions.has(value)}});
  class JPEGXREncoderOptions {
    constructor(quantization=20,colorSpace='auto',trimFlexBits=0){
      jpegxrEncoderOptions.set(this,{quantization:20,colorSpace:'auto',trimFlexBits:0});
      this.quantization=quantization;this.colorSpace=colorSpace;this.trimFlexBits=trimFlexBits;
      Object.seal(this);
    }
    get quantization(){if(!jpegxrEncoderOptions.has(this))throw new TypeError('Expected JPEGXREncoderOptions.');return jpegxrEncoderOptions.get(this).quantization;}
    set quantization(value){
      if(!jpegxrEncoderOptions.has(this))throw new TypeError('Expected JPEGXREncoderOptions.');
      const n=Number(value),q=n>>>0;
      if(!Number.isFinite(n)||n<0||q>255)throw new RangeError('JPEGXREncoderOptions.quantization requires an integer from 0 through 255.');
      jpegxrEncoderOptions.get(this).quantization=q;
    }
    get colorSpace(){if(!jpegxrEncoderOptions.has(this))throw new TypeError('Expected JPEGXREncoderOptions.');return jpegxrEncoderOptions.get(this).colorSpace;}
    set colorSpace(value){
      if(!jpegxrEncoderOptions.has(this))throw new TypeError('Expected JPEGXREncoderOptions.');
      if(value===null||value===undefined){const err=new TypeError('Error #2007: Parameter colorSpace must be non-null.');err.errorID=2007;throw err;}
      const next=String(value);
      if(!VALID_COLORSPACES.has(next)){const err=new RangeError(`Error #2008: Parameter colorSpace must be one of the accepted values ('${next}').`);err.name='ArgumentError';err.errorID=2008;throw err;}
      jpegxrEncoderOptions.get(this).colorSpace=next;
    }
    get trimFlexBits(){if(!jpegxrEncoderOptions.has(this))throw new TypeError('Expected JPEGXREncoderOptions.');return jpegxrEncoderOptions.get(this).trimFlexBits;}
    set trimFlexBits(value){
      if(!jpegxrEncoderOptions.has(this))throw new TypeError('Expected JPEGXREncoderOptions.');
      const n=Number(value),t=n>>>0;
      if(!Number.isFinite(n)||n<0||t>15)throw new RangeError('JPEGXREncoderOptions.trimFlexBits requires an integer from 0 through 15.');
      jpegxrEncoderOptions.get(this).trimFlexBits=t;
    }
  }
  Object.defineProperties(JPEGXREncoderOptions,{__nfIs:{value:value=>jpegxrEncoderOptions.has(value)},__nfInitialize:{value:()=>unsupported('JPEGXREncoderOptions subclassing')},[Symbol.hasInstance]:{value:value=>jpegxrEncoderOptions.has(value)}});
  function snapped(matrix,mode='auto'){
    const [a,b,c,d,e,f]=matrix;
    if(mode==='always')return [a,b,c,d,Math.round(e),Math.round(f)];
    if(mode==='auto'&&b===0&&c===0&&Math.abs(a-1)<=0.001&&Math.abs(d-1)<=0.001)return [1,0,0,1,Math.round(e),Math.round(f)];
    return [...matrix];
  }
  function create({player,geometry,createCanvas,getBinary,getFilters,getFte3D,displayRef,buildDisplayDrawSnapshot,paintScene,onInvalidate,as2,maxCacheBytes=128*1024*1024,maxCacheEntries=256}={}){
    if(!player||!geometry?.Rectangle)throw new TypeError('BitmapData requires the shared Rust Player and Rectangle type.');
    if(!Number.isSafeInteger(maxCacheBytes)||maxCacheBytes<0||!Number.isSafeInteger(maxCacheEntries)||maxCacheEntries<0)throw new RangeError('Bitmap cache budgets must be nonnegative safe integers.');
    const references=new WeakMap(),facades=new Map(),uploads=new Map(),locks=new Map(),partialDisplay=new Map();
    let generation=1,disposed=false,used=new Set(),cacheBytes=0,invalidationFlushes=0;
    let uintVector=values=>Array.from(values,v=>v>>>0),hasUintVector=false;
    let histogramVector=channels=>channels.map(ch=>Array.from(ch,Number));
    function setUintVectorFactory(fn){if(typeof fn==='function'){uintVector=fn;hasUintVector=true;}}
    function setHistogramVectorFactory(fn){if(typeof fn==='function')histogramVector=fn;}
    const identityColor={multipliers:[1,1,1,1],offsets:[0,0,0,0]};
    function native(name,...args){
      try{return player[name](...args);}catch(error){
        if(error instanceof Error||Object.prototype.toString.call(error)==='[object Error]')throw error;
        throw new Error(typeof error==='string'?error:`Rust ${name} failed.`,{cause:error});
      }
    }
    function binary(){const service=getBinary?.();if(!service)throw new Error('BitmapData byte transfer requires the shared ByteArray service.');return service;}
    function filter(value){const service=getFilters?.();if(!service)throw new Error('BitmapData filtering requires the shared BitmapFilter service.');return JSON.stringify(service.toSpecs([value])[0]);}
    function components(value,names){if(value===null||typeof value!=='object')throw new TypeError('Expected a geometry value object.');const result=names.map(name=>value[name]);if(!result.every(value=>typeof value==='number'&&Number.isFinite(value)))throw new RangeError('BitmapData geometry values must be finite.');return result;}
    function colorTransform(value){if(value===null)return identityColor;const values=components(value,['redMultiplier','greenMultiplier','blueMultiplier','alphaMultiplier','redOffset','greenOffset','blueOffset','alphaOffset']);return {multipliers:values.slice(0,4),offsets:values.slice(4)};}
    function active(){if(disposed)throw new Error('This bitmap service has been disposed.');}
    function recordOf(value){active();const record=references.get(value);if(!record)throw new TypeError('Expected BitmapData from this publication.');if(record.generation!==generation)throw new Error('This BitmapData belongs to a previous publication session.');if(record.disposed)throw new Error('This BitmapData has been disposed.');return record;}
    function reference(value){active();const record=references.get(value);if(!record)throw new TypeError('Expected BitmapData from this publication.');if(record.generation!==generation)throw new Error('This BitmapData belongs to a previous publication session.');return record.ref;}
    function descriptor(value){const record=recordOf(value);const result=JSON.parse(native('bitmap_info',record.id));if(result.disposed){record.disposed=true;throw new Error('This BitmapData has been disposed.');}record.width=result.width;record.height=result.height;record.transparent=result.transparent;record.hasMeta=true;return result;}
    function metaOf(value){const record=recordOf(value);if(!record.hasMeta)descriptor(value);return record;}
    function refOf(value){return metaOf(value).ref;}
    function idOf(value){return metaOf(value).id;}
    function fromRef(ref,meta=null){
      active();const id=key(ref);if(facades.has(id))return facades.get(id);
      const facade=Object.create(BitmapData.prototype);
      references.set(facade,{ref,id,generation,disposed:false,width:meta?meta.width:0,height:meta?meta.height:0,transparent:meta?meta.transparent:true,hasMeta:Boolean(meta)});
      if(as2){
        for(const name of ['copyChannel','floodFill','getColorBoundsRect','compare','merge','paletteMap','threshold','hitTest','noise','pixelDissolve','perlinNoise']){
          const method=BitmapData.prototype[name].bind(facade);Object.defineProperty(facade,name,{value:method});
          as2.register(method,facade,name,(...args)=>legacyPixels(facade,name,args));
        }
        {const method=BitmapData.prototype.getPixel.bind(facade);Object.defineProperty(facade,'getPixel',{value:method});
          as2.register(method,facade,'getPixel',(x,y)=>player.bitmap_get_pixel(idOf(facade),as2Int(x,'Pixel x'),as2Int(y,'Pixel y'),false));}
        {const method=BitmapData.prototype.getPixel32.bind(facade);Object.defineProperty(facade,'getPixel32',{value:method});
          as2.register(method,facade,'getPixel32',(x,y)=>player.bitmap_get_pixel(idOf(facade),as2Int(x,'Pixel x'),as2Int(y,'Pixel y'),true));}
        {const method=BitmapData.prototype.setPixel.bind(facade);Object.defineProperty(facade,'setPixel',{value:method});
          as2.register(method,facade,'setPixel',(x,y,v)=>{player.bitmap_set_pixel(idOf(facade),as2Int(x,'Pixel x'),as2Int(y,'Pixel y'),as2Color(v),false);});}
        {const method=BitmapData.prototype.setPixel32.bind(facade);Object.defineProperty(facade,'setPixel32',{value:method});
          as2.register(method,facade,'setPixel32',(x,y,v)=>{player.bitmap_set_pixel(idOf(facade),as2Int(x,'Pixel x'),as2Int(y,'Pixel y'),as2Color(v),true);});}
        {const method=BitmapData.prototype.fillRect.bind(facade);Object.defineProperty(facade,'fillRect',{value:method});
          as2.register(method,facade,'fillRect',(r,v)=>{player.bitmap_fill_rect(idOf(facade),...as2Rectangle(r),as2Color(v));});}
        {const method=BitmapData.prototype.scroll.bind(facade);Object.defineProperty(facade,'scroll',{value:method});
          as2.register(method,facade,'scroll',(x,y)=>{player.bitmap_scroll(idOf(facade),as2Int(x,'Scroll x'),as2Int(y,'Scroll y'));});}
        {const method=BitmapData.prototype.copyPixels.bind(facade);Object.defineProperty(facade,'copyPixels',{value:method});
          as2.register(method,facade,'copyPixels',(source,sourceRect,destPoint,alphaBitmap=null,alphaPoint=null,mergeAlpha=false)=>{
            if(alphaBitmap===null){
              if(typeof mergeAlpha!=='boolean')throw new TypeError('copyPixels mergeAlpha requires a Boolean.');
              if(alphaPoint!==null)as2Point(alphaPoint);
              player.bitmap_copy_pixels(idOf(facade),idOf(source),...as2Rectangle(sourceRect),...as2Point(destPoint),mergeAlpha);
              return;
            }
            if(typeof mergeAlpha!=='boolean')throw new TypeError('copyPixels mergeAlpha requires a Boolean.');
            const [x,y,width,height]=as2Rectangle(sourceRect);
            native('as2_bitmap_command',JSON.stringify({op:'copy_alpha',dst:refOf(facade),src:refOf(source),rect:{x,y,width,height},point:as2Point(destPoint),mask:refOf(alphaBitmap),alpha_point:alphaPoint===null?[0,0]:as2Point(alphaPoint),merge:mergeAlpha}));
          });}
        for(const name of ['applyFilter','generateFilterRect']){
          const method=BitmapData.prototype[name].bind(facade);Object.defineProperty(facade,name,{value:method});
          as2.register(method,facade,name,(...args)=>{
            const settings=getFilters?.().as2Displacement?.(args[name==='applyFilter'?3:1]);
            if(settings){
              const value=name==='applyFilter'?args[1]:args[0],[x,y,width,height]=as2Rectangle(value),rect={x,y,width,height};
              const command=name==='applyFilter'?{op:'displacement',dst:refOf(facade),src:refOf(args[0]),rect,point:as2Point(args[2]),settings}:{op:'filter_bounds',src:refOf(facade),rect};
              const result=JSON.parse(native('as2_bitmap_command',JSON.stringify(command)));
              return name==='applyFilter'?0:new geometry.Rectangle(result.x,result.y,result.width,result.height);
            }
            const result=method(...args);return name==='applyFilter'?0:result;
          });
        }
        for(const name of ['getPixels','setPixels','copyPixelsToByteArray','encode','getVector','setVector','histogram','drawWithQuality']){
          const method=BitmapData.prototype[name].bind(facade);Object.defineProperty(facade,name,{value:method});
          as2.register(method,facade,name,()=>{throw new TypeError(`BitmapData.${name} requires AS3; this API is not part of the AS2 BitmapData profile.`);});
        }
      }
      Object.seal(facade);facades.set(id,facade);return facade;
    }
    // AS2 adapter only: coordinates, references and copied palette values cross
    // one bounded Rust command; no pixel computation is performed in JavaScript.
    function legacyPixels(receiver,name,args){
      const isAs2Call=Boolean(as2);
      const iVal=isAs2Call?as2Int:integer,cVal=isAs2Call?as2Color:color,rVal=isAs2Call?as2Rectangle:rectangle,pVal=isAs2Call?as2Point:point;
      const dst=refOf(receiver),rect=value=>{const [x,y,width,height]=rVal(value);return {x,y,width,height};};
      const byte=(value,label)=>{const v=iVal(value,label);if(v<0||v>255)throw new RangeError(label+' requires 0..255.');return v;};
      const bool=(value,label)=>{if(typeof value!=='boolean')throw new TypeError(label+' requires a Boolean.');return value;};
      const region=()=>({dst,src:refOf(args[0]),rect:rect(args[1]),point:pVal(args[2])});
      let command;
      switch(name){
        case 'copyChannel':command={op:'copy_channel',...region(),source_channel:byte(args[3],'sourceChannel'),dest_channel:byte(args[4],'destChannel')};break;
        case 'floodFill':command={op:'flood_fill',dst,x:iVal(args[0],'Pixel x'),y:iVal(args[1],'Pixel y'),color:cVal(args[2])};break;
        case 'getColorBoundsRect':command={op:'color_bounds',src:dst,mask:cVal(args[0]),color:cVal(args[1]),find_color:bool(args[2]===undefined?true:args[2],'findColor')};break;
        case 'compare':command={op:'compare',dst,src:refOf(args[0])};break;
        case 'merge':command={op:'merge',...region(),multipliers:args.slice(3,7).map(value=>iVal(value,'Channel multiplier'))};break;
        case 'paletteMap':command={op:'palette_map',...region(),tables:[3,4,5,6].map(index=>{
          const table=args[index];if(table==null)return null;
          const tag=typeof table==='object'||typeof table==='function'?Object.getOwnPropertyDescriptor(table,vectorBrand):null;
          const items=Array.isArray(table)?table:tag?.value?.version===1?tag.value.values:null;
          if(!items)throw new TypeError('paletteMap tables require Arrays or null.');
          const mapped=items.slice(0,256).map(cVal);
          while(mapped.length<256)mapped.push(0);
          return mapped;
        })};break;
        case 'threshold':command={op:'threshold',...region(),operation:args[3],threshold:cVal(args[4]),color:cVal(args[5]===undefined?0:args[5]),mask:cVal(args[6]===undefined?0xffffffff:args[6]),copy_source:bool(args[7]===undefined?false:args[7],'copySource')};break;
        case 'noise':command={op:'noise',dst,seed:iVal(args[0],'randomSeed'),low:byte(args[1]===undefined?0:args[1],'low'),high:byte(args[2]===undefined?255:args[2],'high'),channels:byte(args[3]===undefined?7:args[3],'channelOptions'),grayscale:bool(args[4]===undefined?false:args[4],'grayScale')};break;
        case 'perlinNoise':{
          const offsets=args[8]===undefined?[]:args[8];if(!Array.isArray(offsets))throw new TypeError('perlinNoise offsets require an Array of Points.');
          command={op:'perlin_noise',dst,settings:{base:[args[0],args[1]],octaves:byte(args[2],'numOctaves'),seed:iVal(args[3],'randomSeed'),stitch:bool(args[4],'stitch'),fractal:bool(args[5],'fractalNoise'),channels:byte(args[6]===undefined?7:args[6],'channelOptions'),grayscale:bool(args[7]===undefined?false:args[7],'grayScale'),offsets:offsets.map(value=>components(value,['x','y']))}};break;
        }
        case 'pixelDissolve':command={op:'pixel_dissolve',...region(),seed:iVal(args[3]===undefined?0:args[3],'randomSeed'),num_pixels:iVal(args[4]===undefined?0:args[4],'numPixels'),fill_color:cVal(args[5]===undefined?0:args[5])};break;
        case 'hitTest':{
          const second=args[2];let other;
          const secondBitmap=references.has(second)?second:(second&&typeof second==='object'&&references.has(second.bitmapData)?second.bitmapData:null);
          if(secondBitmap)other={kind:'bitmap',pixel_ref:refOf(secondBitmap),point:args[3]==null?[0,0]:pVal(args[3]),threshold:byte(args[4]===undefined?1:args[4],'secondAlphaThreshold')};
          else if(second&&typeof second.width==='number'&&typeof second.height==='number')other={kind:'rectangle',rect:rect(second)};
          else other={kind:'point',point:pVal(second)};
          command={op:'hit_test',src:dst,point:pVal(args[0]),threshold:byte(args[1],'firstAlphaThreshold'),other};break;
        }
        default:unsupported(name);
      }
      const result=JSON.parse(native('as2_bitmap_command',JSON.stringify(command)));
      if(name==='getColorBoundsRect')return new geometry.Rectangle(result.x,result.y,result.width,result.height);
      // Rust JSON maps may sort fields. Rebuild the existing typed PixelRef order
      // before entering the shared identity/cache path.
      if(name==='compare'&&result&&typeof result==='object')return fromRef({id:result.id,generation:result.generation});
      return result===null?undefined:result;
    }
    class BitmapData {
      constructor(width,height,transparent=true,fillColor=0xffffffff){
        active();
        const w=as2?as2Int(width,'BitmapData width'):integer(width,'BitmapData width');
        const h=as2?as2Int(height,'BitmapData height'):integer(height,'BitmapData height');
        if(w<=0||h<=0)throw new RangeError('BitmapData dimensions must be positive.');
        if(typeof transparent!=='boolean')throw new TypeError('BitmapData transparent requires a Boolean.');
        const fc=as2?as2Color(fillColor):color(fillColor);
        return fromRef(JSON.parse(player.bitmap_create(w,h,transparent,fc)),{width:w,height:h,transparent});
      }
      get width(){return metaOf(this).width;}get height(){return metaOf(this).height;}
      get transparent(){return metaOf(this).transparent;}
      get rect(){const info=metaOf(this);return new geometry.Rectangle(0,0,info.width,info.height);}
      get rectangle(){return this.rect;}
      getPixel(x,y){return player.bitmap_get_pixel(idOf(this),integer(x,'Pixel x'),integer(y,'Pixel y'),false);}
      getPixel32(x,y){return player.bitmap_get_pixel(idOf(this),integer(x,'Pixel x'),integer(y,'Pixel y'),true);}
      setPixel(x,y,value){player.bitmap_set_pixel(idOf(this),integer(x,'Pixel x'),integer(y,'Pixel y'),color(value),false);}
      setPixel32(x,y,value){player.bitmap_set_pixel(idOf(this),integer(x,'Pixel x'),integer(y,'Pixel y'),color(value),true);}
      fillRect(rect,value){player.bitmap_fill_rect(idOf(this),...rectangle(rect),color(value));}
      copyPixels(source,sourceRect,destPoint,alphaBitmapData=null,alphaPoint=null,mergeAlpha=false){
        if(typeof mergeAlpha!=='boolean')throw new TypeError('copyPixels mergeAlpha requires a Boolean.');
        if(alphaBitmapData!==null){
          const [x,y,width,height]=rectangle(sourceRect);
          native('as2_bitmap_command',JSON.stringify({op:'copy_alpha',dst:refOf(this),src:refOf(source),rect:{x,y,width,height},point:point(destPoint),mask:refOf(alphaBitmapData),alpha_point:alphaPoint===null?[0,0]:point(alphaPoint),merge:mergeAlpha}));
          return;
        }
        if(alphaPoint!==null)point(alphaPoint);
        player.bitmap_copy_pixels(idOf(this),idOf(source),...rectangle(sourceRect),...point(destPoint),mergeAlpha);
      }
      copyChannel(...args){return legacyPixels(this,'copyChannel',args);}
      floodFill(...args){return legacyPixels(this,'floodFill',args);}
      getColorBoundsRect(...args){return legacyPixels(this,'getColorBoundsRect',args);}
      compare(...args){return legacyPixels(this,'compare',args);}
      merge(...args){return legacyPixels(this,'merge',args);}
      paletteMap(...args){return legacyPixels(this,'paletteMap',args);}
      threshold(...args){return legacyPixels(this,'threshold',args);}
      hitTest(...args){return legacyPixels(this,'hitTest',args);}
      noise(...args){return legacyPixels(this,'noise',args);}
      perlinNoise(...args){return legacyPixels(this,'perlinNoise',args);}
      pixelDissolve(...args){return legacyPixels(this,'pixelDissolve',args);}
      lock(){
        const info=descriptor(this),id=key(reference(this));
        const existing=locks.get(id);
        if(existing){existing.depth++;return;}
        const partial=partialDisplay.get(id);
        const snapBytes=(partial&&partial.revision===info.revision)?new Uint8Array(partial.bytes):new Uint8Array(native('bitmap_pixels',id));
        locks.set(id,{depth:1,lockedRevision:info.revision,bytes:snapBytes,width:info.width,height:info.height});
      }
      unlock(changeRect=null){
        const info=descriptor(this),id=key(reference(this));
        const rect=changeRect===null?null:rectangle(changeRect);
        const st=locks.get(id);
        if(!st)return;
        st.depth--;
        if(st.depth>0)return;
        locks.delete(id);
        if(info.revision===st.lockedRevision&&!partialDisplay.has(id))return;
        for(const [entry,record] of uploads)if(record.pixelKey===id)drop(entry);
        let dirtyRect={x:0,y:0,width:info.width,height:info.height};
        if(rect!==null){
          const x0=Math.max(0,Math.min(info.width,rect[0])),y0=Math.max(0,Math.min(info.height,rect[1]));
          const x1=Math.max(0,Math.min(info.width,rect[0]+Math.max(0,rect[2]))),y1=Math.max(0,Math.min(info.height,rect[1]+Math.max(0,rect[3])));
          dirtyRect={x:x0,y:y0,width:x1-x0,height:y1-y0};
          const merged=new Uint8Array(st.bytes);
          if(x1>x0&&y1>y0){
            const live=native('bitmap_pixels',id);
            for(let py=y0;py<y1;py++){
              const rowStart=(py*info.width+x0)*4,rowEnd=(py*info.width+x1)*4;
              merged.set(live.subarray(rowStart,rowEnd),rowStart);
            }
          }
          partialDisplay.set(id,{revision:info.revision,bytes:merged,token:(partialDisplay.get(id)?.token||0)+1});
        }else{
          partialDisplay.delete(id);
        }
        invalidationFlushes++;
        onInvalidate?.(id,dirtyRect);
      }
      getVector(rect){
        const ref=key(refOf(this)),area=rectangle(rect),bytes=native('bitmap_get_pixels',ref,...area,false);
        const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),count=bytes.byteLength>>>2,values=new Array(count);
        for(let i=0;i<count;i++)values[i]=view.getUint32(i*4,false);
        return uintVector(values);
      }
      setVector(rect,inputVector){
        const info=descriptor(this),ref=key(reference(this)),[rx,ry,rw,rh]=rectangle(rect);
        const tag=inputVector!=null&&(typeof inputVector==='object'||typeof inputVector==='function')?Object.getOwnPropertyDescriptor(inputVector,vectorBrand):null;
        const isUintVec=tag&&Object.hasOwn(tag,'value')&&tag.value?.version===1&&tag.value?.element?.name==='uint';
        if(hasUintVector?!isUintVec:(!isUintVec&&!Array.isArray(inputVector)&&!(inputVector instanceof Uint32Array)))throw new TypeError('BitmapData.setVector requires a Vector.<uint>.');
        const values=isUintVec?tag.value.values:inputVector;
        const x0=Math.max(0,Math.min(info.width,rx)),y0=Math.max(0,Math.min(info.height,ry));
        const x1=Math.max(0,Math.min(info.width,rx+Math.max(0,rw))),y1=Math.max(0,Math.min(info.height,ry+Math.max(0,rh)));
        const count=(x1-x0)*(y1-y0);
        if(values.length<count)throw new RangeError('BitmapData.setVector inputVector length is smaller than the target rectangle.');
        const bytes=new Uint8Array(count*4),view=new DataView(bytes.buffer);
        for(let i=0;i<count;i++)view.setUint32(i*4,values[i]>>>0,false);
        native('bitmap_set_pixels',ref,rx,ry,rw,rh,bytes,false);
      }
      clone(){return fromRef(JSON.parse(player.bitmap_clone(key(refOf(this)))));}
      scroll(x,y){player.bitmap_scroll(key(refOf(this)),integer(x,'Scroll x'),integer(y,'Scroll y'));}
      getPixels(rect){const ref=key(refOf(this)),area=rectangle(rect),result=binary().fromBytes(native('bitmap_get_pixels',ref,...area,false));result.position=result.length;return result;}
      copyPixelsToByteArray(rect,bytes){const ref=key(refOf(this)),area=rectangle(rect);binary().writeFromHost(bytes,little=>native('bitmap_get_pixels',ref,...area,little));}
      setPixels(rect,bytes){
        const ref=key(refOf(this)),area=rectangle(rect);
        const result=binary().readAvailable(bytes,(input,little)=>JSON.parse(native('bitmap_set_pixels',ref,...area,input,little)));
        if(!result.complete){const error=new Error('BitmapData.setPixels reached the end of the ByteArray; available whole pixels were written.');error.name='EOFError';throw error;}
      }
      colorTransform(rect,transform){const ref=key(refOf(this)),area=rectangle(rect),ct=JSON.stringify(colorTransform(transform));native('bitmap_color_transform',ref,...area,ct);}
      applyFilter(source,sourceRect,destPoint,value){
        const settings=getFilters?.().as2Displacement?.(value);
        if(settings){
          const [x,y,width,height]=rectangle(sourceRect);
          native('as2_bitmap_command',JSON.stringify({op:'displacement',dst:refOf(this),src:refOf(source),rect:{x,y,width,height},point:point(destPoint),settings}));
          return;
        }
        const destination=key(refOf(this)),input=key(refOf(source)),area=rectangle(sourceRect),at=point(destPoint),spec=filter(value);native('bitmap_apply_filter',destination,input,...area,...at,spec);
      }
      generateFilterRect(sourceRect,value){
        const settings=getFilters?.().as2Displacement?.(value);
        if(settings){
          const [x,y,width,height]=rectangle(sourceRect),r=JSON.parse(native('as2_bitmap_command',JSON.stringify({op:'filter_bounds',src:refOf(this),rect:{x,y,width,height}})));
          return new geometry.Rectangle(r.x,r.y,r.width,r.height);
        }
        const ref=key(refOf(this)),area=rectangle(sourceRect),spec=filter(value),r=JSON.parse(native('bitmap_generate_filter_rect',ref,...area,spec));return new geometry.Rectangle(r.x,r.y,r.width,r.height);
      }
      encode(rect,compressor,output=null){
        const ref=key(refOf(this));
        if(rect===null||rect===undefined){const err=new TypeError('Error #2007: Parameter rect must be non-null.');err.errorID=2007;throw err;}
        if(compressor===null||compressor===undefined){const err=new TypeError('Error #2007: Parameter compressor must be non-null.');err.errorID=2007;throw err;}
        const area=rectangle(rect),service=binary();
        let produce;
        if(encoderOptions.has(compressor)){
          const fast=compressor.fastCompression;
          produce=()=>native('bitmap_encode_png',ref,...area,fast);
        }else if(jpegEncoderOptions.has(compressor)&&typeof player.bitmap_encode_jpeg==='function'){
          const quality=compressor.quality;
          produce=()=>native('bitmap_encode_jpeg',ref,...area,quality);
        }else if(jpegxrEncoderOptions.has(compressor)&&typeof player.bitmap_encode_jpegxr==='function'){
          const {quantization,colorSpace,trimFlexBits}=jpegxrEncoderOptions.get(compressor);
          produce=()=>native('bitmap_encode_jpegxr',ref,...area,quantization,colorSpace,trimFlexBits);
        }else{
          throw new TypeError('BitmapData.encode requires PNGEncoderOptions, JPEGEncoderOptions or JPEGXREncoderOptions.');
        }
        if(output===null)return service.fromBytes(produce());
        service.writeFromHost(output,produce);return output;
      }
      draw(source,matrix=null,transform=null,blendMode=null,clipRect=null,smoothing=false,quality=null){
        const info=descriptor(this),ref=key(reference(this));
        const m=matrix===null?[1,0,0,1,0,0]:components(matrix,['a','b','c','d','tx','ty']),ct=colorTransform(transform);
        if(m.some((value,index)=>Math.abs(value)>(index<4?1e4:1e12)))throw new RangeError('BitmapData.draw matrix exceeds the shared finite affine bounds.');
        if(ct.multipliers.some(value=>Math.abs(value)>1e4)||ct.offsets.some(value=>Math.abs(value)>255))throw new RangeError('BitmapData.draw ColorTransform exceeds the shared multiplier/offset bounds.');
        const as2BlendNames=['','normal','layer','multiply','screen','lighten','darken','difference','add','subtract','invert','alpha','erase','overlay','hardlight'];
        const normalizedBlend=typeof blendMode==='number'&&Number.isInteger(blendMode)&&blendMode>=0&&blendMode<=14?(as2BlendNames[blendMode]||'normal'):blendMode;
        const validBlendModes=new Set(['normal','layer','multiply','screen','lighten','darken','difference','add','subtract','invert','alpha','erase','overlay','hardlight','shader']);
        if(normalizedBlend!==null&&!validBlendModes.has(normalizedBlend)){const err=new TypeError(`BitmapData.draw blendMode is invalid: '${blendMode}'.`);err.name='ArgumentError';throw err;}
        const drawBlend=normalizedBlend===null?'normal':normalizedBlend;
        let shaderSpec=null;
        if(drawBlend==='shader'){
          shaderSpec=getFte3D?.()?.getBlendShaderSpecForDrawable?.(source)||null;
          if(!shaderSpec){
            const err=new RangeError('BitmapData.draw with BlendMode.SHADER requires source.blendShader to reference a valid Shader.');
            err.name='ArgumentError';throw err;
          }
        }
        const commitDraw=rgba=>{
          if(drawBlend==='shader'&&shaderSpec&&typeof player.bitmap_draw_rgba_shader==='function'){
            native('bitmap_draw_rgba_shader',ref,info.width,info.height,rgba,clipJSON,info.revision,shaderSpec.bytecode_base64,shaderSpec.precision_hint,shaderSpec.params_json);
          }else if(drawBlend!=='normal'&&typeof player.bitmap_draw_rgba_blended==='function')native('bitmap_draw_rgba_blended',ref,info.width,info.height,rgba,clipJSON,info.revision,drawBlend);
          else native('bitmap_draw_rgba',ref,info.width,info.height,rgba,clipJSON,info.revision);
        };
        if(typeof smoothing!=='boolean')throw new TypeError('BitmapData.draw smoothing requires a Boolean.');
        const qNorm=quality===null||quality===undefined?null:String(quality).toLowerCase();
        const highQualitySmooth=Boolean(qNorm&&(qNorm==='best'||qNorm.startsWith('8x8')||qNorm.startsWith('16x16')));
        const effectiveSmoothing=qNorm==='low'?false:(smoothing||highQualitySmooth);
        const rawSub=qNorm===null||qNorm==='low'?1:qNorm==='medium'?2:(qNorm==='high'||qNorm==='best')?4:qNorm.startsWith('8x8')?8:qNorm.startsWith('16x16')?16:4;
        let sub=rawSub;
        while(sub>1&&info.width*sub*info.height*sub>4*1024*1024)sub>>=1;
        const linearGamma=qNorm!==null&&qNorm.endsWith('linear');
        const downsamplePremul=(raw,srcW,dstW,dstH,scale,useLinear)=>{
          if(scale<=1)return raw instanceof Uint8Array?raw:new Uint8Array(raw.buffer,raw.byteOffset,raw.byteLength);
          const rgba=new Uint8Array(dstW*dstH*4),invScale2=1/(scale*scale);
          const toLin=v=>useLinear?Math.pow(v/255,2.2):v;
          const fromLin=v=>useLinear?Math.pow(Math.max(0,v),1/2.2)*255:v;
          const clampByte=v=>v<=0?0:v>=255?255:Math.round(v);
          const stride=srcW*4;
          for(let y=0;y<dstH;y++)for(let x=0;x<dstW;x++){
            let rPremul=0,gPremul=0,bPremul=0,aSum=0;
            for(let sy=0;sy<scale;sy++)for(let sx=0;sx<scale;sx++){
              const si=(y*scale+sy)*stride+(x*scale+sx)*4,a=raw[si+3];
              if(a<=0)continue;
              rPremul+=a*toLin(raw[si]);
              gPremul+=a*toLin(raw[si+1]);
              bPremul+=a*toLin(raw[si+2]);
              aSum+=a;
            }
            if(aSum>0){
              const di=(y*dstW+x)*4;
              rgba[di]=clampByte(fromLin(rPremul/aSum));
              rgba[di+1]=clampByte(fromLin(gPremul/aSum));
              rgba[di+2]=clampByte(fromLin(bPremul/aSum));
              rgba[di+3]=clampByte(aSum*invScale2);
            }
          }
          return rgba;
        };
        const clip=clipRect===null?null:rectangle(clipRect),clipJSON=JSON.stringify(clip&&{x:clip[0],y:clip[1],width:clip[2],height:clip[3]});
        if(info.width*info.height>4*1024*1024)throw new RangeError('BitmapData.draw staging exceeds the 4 Mi-pixel raster budget.');
        let snapshot,usedFallbackSnapshot=false;
        if(references.has(source)){
          const input=descriptor(source);
          snapshot={objects:[],semantics:[],clips:[],filter_groups:[],draws:[{transform:m,alpha:1,clip_ids:[],filter_id:null,kind:{type:'bitmap_pixels',pixel_ref:reference(source),width:input.width,height:input.height,revision:input.revision,color_transform:ct,smoothing:effectiveSmoothing,pixel_snapping:'never'}}]};
        }else{
          const sourceRef=displayRef?.(source);if(!sourceRef)throw new TypeError('BitmapData.draw requires BitmapData or a live DisplayObject from this publication.');
          const srcKey=key(sourceRef);
          if(buildDisplayDrawSnapshot?.needsCustom?.(source)){
            snapshot=buildDisplayDrawSnapshot.build(source,m,ct);
            usedFallbackSnapshot=true;
          }else{
            try{
              snapshot=JSON.parse(native('bitmap_draw_snapshot',srcKey,JSON.stringify(m),JSON.stringify(ct)));
              if(buildDisplayDrawSnapshot?.enrich)snapshot=buildDisplayDrawSnapshot.enrich(snapshot);
            }catch(err){
              if(buildDisplayDrawSnapshot?.build&&/external mask dependencies/i.test(String(err?.message||err))){
                snapshot=buildDisplayDrawSnapshot.build(source,m,ct);
                usedFallbackSnapshot=true;
              }else{
                throw err;
              }
            }
          }
          if(Array.isArray(snapshot?.filter_groups)){
            for(const fg of snapshot.filter_groups){
              if(key(fg.owner_ref)===srcKey&&String(fg.composite_operation||'').startsWith('shader:')){
                fg.composite_operation='source-over';
              }
            }
          }
          if(qNorm==='low'||effectiveSmoothing){
            for(const d of (snapshot?.draws||[])){
              const k=d?.kind;
              if(!k)continue;
              if(k.type==='bitmap_pixels')k.smoothing=qNorm==='low'?false:Boolean(k.smoothing||effectiveSmoothing);
              if(k.fill_bitmap)k.fill_bitmap.smoothing=qNorm==='low'?false:Boolean(k.fill_bitmap.smoothing||effectiveSmoothing);
              if(k.stroke_bitmap)k.stroke_bitmap.smoothing=qNorm==='low'?false:Boolean(k.stroke_bitmap.smoothing||effectiveSmoothing);
            }
          }
        }
        let surface=null;try{surface=canvas(info.width,info.height);}catch(_){surface=null;}
        const target=surface?.getContext?.('2d');
        if(references.has(source)&&(qNorm!==null||!target||typeof target.setTransform!=='function'||typeof target.getImageData!=='function')){
          const input=descriptor(source),src=player.bitmap_pixels(key(reference(source)));
          const [a,b,c,d,tx,ty]=m,det=a*d-b*c,rgba=new Uint8Array(info.width*info.height*4);
          if(Number.isFinite(det)&&Math.abs(det)>=1e-12){
            const clampByte=v=>v<=0?0:v>=255?255:Math.round(v);
            const toLin=v=>linearGamma?Math.pow(v/255,2.2):v;
            const fromLin=v=>linearGamma?Math.pow(Math.max(0,v),1/2.2)*255:v;
            const sampleSrc=(u,v)=>{
              if(!effectiveSmoothing){
                const sx=Math.floor(u),sy=Math.floor(v);
                if(sx<0||sy<0||sx>=input.width||sy>=input.height)return null;
                const si=(sy*input.width+sx)*4;
                return [src[si],src[si+1],src[si+2],src[si+3]];
              }
              const fx=u-0.5,fy=v-0.5,x0=Math.floor(fx),y0=Math.floor(fy),wx=fx-x0,wy=fy-y0;
              if(x0+1<0||y0+1<0||x0>=input.width||y0>=input.height)return null;
              const readP=(px,py)=>{
                const cx=Math.max(0,Math.min(input.width-1,px)),cy=Math.max(0,Math.min(input.height-1,py));
                const si=(cy*input.width+cx)*4;
                return [src[si],src[si+1],src[si+2],src[si+3]];
              };
              const p00=readP(x0,y0),p10=readP(x0+1,y0),p01=readP(x0,y0+1),p11=readP(x0+1,y0+1);
              return [0,1,2,3].map(ch=>(p00[ch]*(1-wx)+p10[ch]*wx)*(1-wy)+(p01[ch]*(1-wx)+p11[ch]*wx)*wy);
            };
            const invSub2=1/(sub*sub);
            for(let y=0;y<info.height;y++)for(let x=0;x<info.width;x++){
              let rAcc=0,gAcc=0,bAcc=0,aAcc=0,hits=0;
              for(let sySub=0;sySub<sub;sySub++)for(let sxSub=0;sxSub<sub;sxSub++){
                const dx=x+(sxSub+0.5)/sub-tx,dy=y+(sySub+0.5)/sub-ty;
                const u=(d*dx-c*dy)/det,v=(-b*dx+a*dy)/det;
                const s=sampleSrc(u,v);
                if(!s)continue;
                hits++;
                rAcc+=toLin(clampByte(s[0]*ct.multipliers[0]+ct.offsets[0]));
                gAcc+=toLin(clampByte(s[1]*ct.multipliers[1]+ct.offsets[1]));
                bAcc+=toLin(clampByte(s[2]*ct.multipliers[2]+ct.offsets[2]));
                aAcc+=clampByte(s[3]*ct.multipliers[3]+ct.offsets[3]);
              }
              if(hits>0){
                const di=(y*info.width+x)*4;
                rgba[di]=clampByte(fromLin(rAcc*invSub2));
                rgba[di+1]=clampByte(fromLin(gAcc*invSub2));
                rgba[di+2]=clampByte(fromLin(bAcc*invSub2));
                rgba[di+3]=clampByte(aAcc*invSub2);
              }
            }
          }
          commitDraw(rgba);
          if(surface)surface.width=surface.height=0;
          return;
        }
        if(!target||typeof target.setTransform!=='function'||typeof target.getImageData!=='function'||(!references.has(source)&&typeof target.clip!=='function')){
          const scale=sub,rw=info.width*scale,rh=info.height*scale;
          const mScaled=scale===1?m:[m[0]*scale,m[1]*scale,m[2]*scale,m[3]*scale,m[4]*scale,m[5]*scale];
          const hasBitmapDraws=(snapshot?.draws||[]).some(d=>d?.kind&&(d.kind.type==='bitmap_pixels'||d.kind.fill_bitmap||d.kind.stroke_bitmap));
          const needSmoothingOverride=(qNorm==='low'||effectiveSmoothing)&&hasBitmapDraws;
          const hasCustomSnapshot=usedFallbackSnapshot||Boolean(
            snapshot?.filter_groups?.some(g=>String(g?.composite_operation||'').startsWith('shader:'))
            ||snapshot?.clips?.some(c=>String(c?.id||'').startsWith('ext-mask-'))
          );
          const scaleSnapshotForSub=snap=>{
            if(scale<=1||!snap)return snap;
            const scaleMat=mat=>Array.isArray(mat)?[mat[0]*scale,mat[1]*scale,mat[2]*scale,mat[3]*scale,mat[4]*scale,mat[5]*scale]:mat;
            const mapComp=comp=>({...comp,matrix:scaleMat(comp.matrix)});
            return {
              ...snap,
              draws:(snap.draws||[]).map(d=>({...d,transform:scaleMat(d.transform)})),
              clips:(snap.clips||[]).map(c=>({
                ...c,
                components:(c.components||[]).map(mapComp),
                ...(c.alpha_components?{alpha_components:c.alpha_components.map(mapComp)}:{}),
              })),
            };
          };
          const usedRust=!references.has(source)&&!needSmoothingOverride&&(hasCustomSnapshot?typeof player.bitmap_render_runtime_snapshot==='function':typeof player.bitmap_render_snapshot==='function');
          const hiRgba=usedRust
            ?(hasCustomSnapshot
              ?new Uint8Array(player.bitmap_render_runtime_snapshot(JSON.stringify(scaleSnapshotForSub(snapshot)),rw,rh))
              :new Uint8Array(player.bitmap_render_snapshot(key(displayRef(source)),JSON.stringify(mScaled),JSON.stringify(ct),rw,rh)))
            :new Uint8Array(rw*rh*4);
          const clampByte=v=>v<=0?0:v>=255?255:Math.round(v);
          const blendPixel=(x,y,r,g,b,a,applyRootCt=true)=>{
            if(x<0||y<0||x>=rw||y>=rh||a<=0)return;
            const cr=applyRootCt?clampByte(r*ct.multipliers[0]+ct.offsets[0]):clampByte(r);
            const cg=applyRootCt?clampByte(g*ct.multipliers[1]+ct.offsets[1]):clampByte(g);
            const cb=applyRootCt?clampByte(b*ct.multipliers[2]+ct.offsets[2]):clampByte(b);
            const ca=applyRootCt?clampByte(a*ct.multipliers[3]+ct.offsets[3]):clampByte(a);
            if(ca<=0)return;
            const di=(y*rw+x)*4,sa=ca/255,da=hiRgba[di+3]/255,oa=sa+da*(1-sa);
            if(oa<=0)return;
            hiRgba[di]=clampByte((cr*sa+hiRgba[di]*da*(1-sa))/oa);
            hiRgba[di+1]=clampByte((cg*sa+hiRgba[di+1]*da*(1-sa))/oa);
            hiRgba[di+2]=clampByte((cb*sa+hiRgba[di+2]*da*(1-sa))/oa);
            hiRgba[di+3]=clampByte(oa*255);
          };
          for(const item of (snapshot?.draws||[])){
            const rawT=item.transform||[1,0,0,1,0,0];
            const [a,b,c,d,tx,ty]=scale===1?rawT:[rawT[0]*scale,rawT[1]*scale,rawT[2]*scale,rawT[3]*scale,rawT[4]*scale,rawT[5]*scale];
            const det=a*d-b*c,itemAlpha=Number.isFinite(item.alpha)?item.alpha:1;
            const k=item.kind||{};
            if(usedRust&&k.type!=='text_field'&&k.type!=='text')continue;
            if(k.type==='bitmap_pixels'&&Number.isFinite(det)&&Math.abs(det)>=1e-12){
              const src=player.bitmap_pixels(key(k.pixel_ref)),sw=k.width|0,sh=k.height|0,smoothBmp=Boolean(k.smoothing);
              const kct=k.color_transform;
              for(let y=0;y<rh;y++)for(let x=0;x<rw;x++){
                const dx=x+0.5-tx,dy=y+0.5-ty;
                const u=(d*dx-c*dy)/det,v=(-b*dx+a*dy)/det;
                if(u<0||v<0||u>=sw||v>=sh)continue;
                let r,g,bl,al;
                if(!smoothBmp){
                  const sx=Math.min(sw-1,Math.max(0,Math.floor(u))),sy=Math.min(sh-1,Math.max(0,Math.floor(v))),si=(sy*sw+sx)*4;
                  r=src[si];g=src[si+1];bl=src[si+2];al=src[si+3];
                }else{
                  const fx=u-0.5,fy=v-0.5,x0=Math.floor(fx),y0=Math.floor(fy),wx=fx-x0,wy=fy-y0;
                  const rd=(px,py)=>{const cx=Math.max(0,Math.min(sw-1,px)),cy=Math.max(0,Math.min(sh-1,py)),si=(cy*sw+cx)*4;return [src[si],src[si+1],src[si+2],src[si+3]];};
                  const p00=rd(x0,y0),p10=rd(x0+1,y0),p01=rd(x0,y0+1),p11=rd(x0+1,y0+1);
                  const w00=(1-wx)*(1-wy),w10=wx*(1-wy),w01=(1-wx)*wy,w11=wx*wy;
                  al=p00[3]*w00+p10[3]*w10+p01[3]*w01+p11[3]*w11;
                  if(al<=0.5)continue;
                  r=(p00[0]*p00[3]*w00+p10[0]*p10[3]*w10+p01[0]*p01[3]*w01+p11[0]*p11[3]*w11)/al;
                  g=(p00[1]*p00[3]*w00+p10[1]*p10[3]*w10+p01[1]*p01[3]*w01+p11[1]*p11[3]*w11)/al;
                  bl=(p00[2]*p00[3]*w00+p10[2]*p10[3]*w10+p01[2]*p01[3]*w01+p11[2]*p11[3]*w11)/al;
                }
                if(kct){
                  r=clampByte(r*kct.multipliers[0]+kct.offsets[0]);
                  g=clampByte(g*kct.multipliers[1]+kct.offsets[1]);
                  bl=clampByte(bl*kct.multipliers[2]+kct.offsets[2]);
                  al=clampByte(al*kct.multipliers[3]+kct.offsets[3]);
                  blendPixel(x,y,r,g,bl,al*itemAlpha,false);
                }else{
                  blendPixel(x,y,r,g,bl,al*itemAlpha,true);
                }
              }
            }else if((k.type==='rectangle'||k.type==='ellipse')&&Number.isFinite(det)&&Math.abs(det)>=1e-12){
              const w=Number(k.width)||0,h=Number(k.height)||0,fill=k.fill||[0,0,0,0];
              if(w>0&&h>0&&fill[3]>0){
                for(let y=0;y<rh;y++)for(let x=0;x<rw;x++){
                  const dx=x+0.5-tx,dy=y+0.5-ty;
                  const lx=(d*dx-c*dy)/det,ly=(-b*dx+a*dy)/det;
                  if(k.type==='rectangle'){
                    if(lx>=0&&ly>=0&&lx<w&&ly<h)blendPixel(x,y,fill[0],fill[1],fill[2],fill[3]*itemAlpha,!references.has(source)?false:true);
                  }else{
                    const nx=(lx-w*0.5)/(w*0.5),ny=(ly-h*0.5)/(h*0.5);
                    if(nx*nx+ny*ny<=1)blendPixel(x,y,fill[0],fill[1],fill[2],fill[3]*itemAlpha,!references.has(source)?false:true);
                  }
                }
              }
            }else if((k.type==='path'||k.type==='gradient_path'||k.type==='painted_path')&&(Array.isArray(k.points)||Array.isArray(k.commands))&&Number.isFinite(det)&&Math.abs(det)>=1e-12){
              let pts=k.points;
              if(!Array.isArray(pts)&&Array.isArray(k.commands)){
                pts=[];
                let startPt=null;
                for(const cmd of k.commands){
                  if(cmd.op==='move_to'){const p={x:Number(cmd.x)||0,y:Number(cmd.y)||0};pts.push(p);startPt=p;}
                  else if(cmd.op==='line_to'||cmd.op==='quadratic_to'||cmd.op==='cubic_to')pts.push({x:Number(cmd.x)||0,y:Number(cmd.y)||0});
                  else if(cmd.op==='close'&&startPt)pts.push(startPt);
                }
              }
              if(Array.isArray(pts)&&pts.length>=2){
                const fill=k.fill||[0,0,0,0],stroke=k.stroke||[0,0,0,0],sw=Number(k.stroke_width)||0,fb=k.fill_bitmap;
                const fbBytes=fb?player.bitmap_pixels(key(fb.pixel_ref)):null,fbInv=fb&&Array.isArray(fb.matrix)?(()=>{const [fa,fb1,fc,fd,ftx,fty]=fb.matrix,fdet=fa*fd-fb1*fc;return Math.abs(fdet)>=1e-12?[fd/fdet,-fb1/fdet,-fc/fdet,fa/fdet,(fc*fty-fd*ftx)/fdet,(fb1*ftx-fa*fty)/fdet]:null;})():[1,0,0,1,0,0];
                let minX=Infinity,minY=Infinity,maxX=-Infinity,maxY=-Infinity;
                for(const p of pts){if(p.x<minX)minX=p.x;if(p.y<minY)minY=p.y;if(p.x>maxX)maxX=p.x;if(p.y>maxY)maxY=p.y;}
                if((fill[3]>0||fbBytes)&&pts.length>=3){
                  for(let y=0;y<rh;y++)for(let x=0;x<rw;x++){
                    const dx=x+0.5-tx,dy=y+0.5-ty;
                    const lx=(d*dx-c*dy)/det,ly=(-b*dx+a*dy)/det;
                    if(lx<minX||lx>maxX||ly<minY||ly>maxY)continue;
                    let inside=false;
                    for(let i=0,j=pts.length-1;i<pts.length;j=i++){
                      const xi=pts[i].x,yi=pts[i].y,xj=pts[j].x,yj=pts[j].y;
                      if(((yi>ly)!==(yj>ly))&&(lx<(xj-xi)*(ly-yi)/((yj-yi)||1e-12)+xi))inside=!inside;
                    }
                    if(inside){
                      if(fbBytes&&fbInv&&fb.width>0&&fb.height>0){
                        let bu=fbInv[0]*lx+fbInv[2]*ly+fbInv[4],bv=fbInv[1]*lx+fbInv[3]*ly+fbInv[5];
                        const fbw=fb.width|0,fbh=fb.height|0,rep=Boolean(fb.repeat);
                        const wrap=(val,lim)=>rep?((val%lim)+lim)%lim:Math.max(0,Math.min(lim-1,val));
                        let r,g,bl,al;
                        if(!fb.smoothing){
                          const sx=wrap(Math.floor(bu),fbw),sy=wrap(Math.floor(bv),fbh),si=(sy*fbw+sx)*4;
                          r=fbBytes[si];g=fbBytes[si+1];bl=fbBytes[si+2];al=fbBytes[si+3];
                        }else{
                          const fx=bu-0.5,fy=bv-0.5,x0=Math.floor(fx),y0=Math.floor(fy),wx=fx-x0,wy=fy-y0;
                          const rd=(px,py)=>{const cx=wrap(px,fbw),cy=wrap(py,fbh),si=(cy*fbw+cx)*4;return [fbBytes[si],fbBytes[si+1],fbBytes[si+2],fbBytes[si+3]];};
                          const p00=rd(x0,y0),p10=rd(x0+1,y0),p01=rd(x0,y0+1),p11=rd(x0+1,y0+1);
                          const w00=(1-wx)*(1-wy),w10=wx*(1-wy),w01=(1-wx)*wy,w11=wx*wy;
                          al=p00[3]*w00+p10[3]*w10+p01[3]*w01+p11[3]*w11;
                          if(al<=0.5)continue;
                          r=(p00[0]*p00[3]*w00+p10[0]*p10[3]*w10+p01[0]*p01[3]*w01+p11[0]*p11[3]*w11)/al;
                          g=(p00[1]*p00[3]*w00+p10[1]*p10[3]*w10+p01[1]*p01[3]*w01+p11[1]*p11[3]*w11)/al;
                          bl=(p00[2]*p00[3]*w00+p10[2]*p10[3]*w10+p01[2]*p01[3]*w01+p11[2]*p11[3]*w11)/al;
                        }
                        const fct=fb.color_transform;
                        if(fct){r=clampByte(r*fct.multipliers[0]+fct.offsets[0]);g=clampByte(g*fct.multipliers[1]+fct.offsets[1]);bl=clampByte(bl*fct.multipliers[2]+fct.offsets[2]);al=clampByte(al*fct.multipliers[3]+fct.offsets[3]);}
                        blendPixel(x,y,r,g,bl,al*itemAlpha,false);
                      }else{
                        blendPixel(x,y,fill[0],fill[1],fill[2],fill[3]*itemAlpha,!references.has(source)?false:true);
                      }
                    }
                  }
                }
                if(stroke[3]>0&&sw>0){
                  const half=Math.max(0.75,sw*0.5*scale);
                  for(let i=0;i<pts.length-1;i++){
                    const x0=pts[i].x,y0=pts[i].y,x1=pts[i+1].x,y1=pts[i+1].y;
                    const steps=Math.max(1,Math.ceil(Math.hypot(x1-x0,y1-y0)*2*scale));
                    for(let s=0;s<=steps;s++){
                      const t=s/steps,lx=x0+(x1-x0)*t,ly=y0+(y1-y0)*t;
                      const sx=Math.round(a*lx+c*ly+tx),sy=Math.round(b*lx+d*ly+ty);
                      const r=Math.max(0,Math.floor(half));
                      for(let oy=-r;oy<=r;oy++)for(let ox=-r;ox<=r;ox++)blendPixel(sx+ox,sy+oy,stroke[0],stroke[1],stroke[2],stroke[3]*itemAlpha,!references.has(source)?false:true);
                    }
                  }
                }
              }
            }else if((k.type==='text_field'||k.type==='text')&&Number.isFinite(det)&&Math.abs(det)>=1e-12){
              const field=k.field||{text:k.text||'',format:{color:k.color||[0,0,0],size:k.font_size||12},alpha:k.color?.[3]??255};
              const fmtColor=field.format?.color;
              const defaultCol=Array.isArray(fmtColor)?fmtColor:[((fmtColor>>>16)&255),((fmtColor>>>8)&255),(fmtColor&255)];
              const runs=Array.isArray(field.runs)&&field.runs.length?field.runs:[{text:field.text||'',size:field.format?.size||12,color:defaultCol}];
              const fieldAlpha=((field.alpha??255)/255)*itemAlpha;
              const yShift=ty<0?-ty/scale:0;
              let cursorX=0;
              for(const run of runs){
                const text=String(run.text||''),sz=Math.max(4,Number(run.size)||12);
                const rawCol=run.color??defaultCol;
                const col=Array.isArray(rawCol)?rawCol:[((rawCol>>>16)&255),((rawCol>>>8)&255),(rawCol&255)];
                const charW=Math.max(3,Math.round(sz*0.6)),charH=Math.max(4,Math.round(sz*0.85));
                for(let ci=0;ci<text.length;ci++){
                  const code=text.charCodeAt(ci);
                  if(code>32){
                    for(let gy=1;gy<charH-1;gy++)for(let gx=1;gx<charW-1;gx++){
                      const bit=((code+gx*3+gy*5)&3)!==0||gx===1||gy===1||gy===Math.floor(charH/2);
                      if(!bit)continue;
                      for(let sy=0;sy<scale;sy++)for(let sxSub=0;sxSub<scale;sxSub++){
                        const lx=cursorX+gx+(sxSub+0.5)/scale-0.5,ly=gy+yShift+(sy+0.5)/scale-0.5;
                        const sx=Math.floor(a*lx+c*ly+tx),syPx=Math.floor(b*lx+d*ly+ty);
                        blendPixel(sx,syPx,col[0],col[1],col[2],Math.round(255*fieldAlpha),true);
                      }
                    }
                  }
                  cursorX+=charW;
                }
              }
            }
          }
          commitDraw(downsamplePremul(hiRgba,rw,info.width,info.height,scale,linearGamma));
          if(surface)surface.width=surface.height=0;
          return;
        }
        if(typeof paintScene!=='function')throw new Error('BitmapData.draw requires the shared scene painter.');
        const scale=sub;
        if(!surface||scale>1){if(surface)surface.width=surface.height=0;surface=canvas(info.width*scale,info.height*scale);}
        try{
          const ctx=surface.getContext('2d');
          if(!ctx)throw new Error('BitmapData.draw requires Canvas 2D.');
          ctx.imageSmoothingEnabled=effectiveSmoothing;
          if('imageSmoothingQuality' in ctx)ctx.imageSmoothingQuality=highQualitySmooth?'high':qNorm==='high'?'medium':'low';
          ctx.setTransform(scale,0,0,scale,0,0);ctx.globalAlpha=1;ctx.globalCompositeOperation='source-over';ctx.clearRect(0,0,info.width,info.height);
          paintScene(ctx,snapshot);
          // Read an immutable full source before changing destination, including
          // when source and destination share the very same Rust pixel buffer.
          const raw=ctx.getImageData(0,0,info.width*scale,info.height*scale).data;
          commitDraw(downsamplePremul(raw,info.width*scale,info.width,info.height,scale,linearGamma));
        }finally{surface.width=surface.height=0;}
      }
      histogram(hRect=null){
        const info=descriptor(this),ref=key(reference(this));
        const rect=hRect===null?{x:0,y:0,width:info.width,height:info.height}:(()=>{const [x,y,width,height]=rectangle(hRect);return {x,y,width,height};})();
        const channels=JSON.parse(native('bitmap_histogram_json',ref,JSON.stringify(rect)));
        return histogramVector(channels);
      }
      drawWithQuality(source,matrix=null,transform=null,blendMode=null,clipRect=null,smoothing=false,quality=null){
        if(quality!==null&&quality!==undefined){
          const q=String(quality).toLowerCase();
          if(!VALID_DRAW_QUALITIES.has(q)){const err=new RangeError(`BitmapData.drawWithQuality quality is invalid: '${quality}'.`);err.name='ArgumentError';throw err;}
        }
        this.draw(source,matrix,transform,blendMode,clipRect,smoothing,quality);
      }
      dispose(){const record=references.get(this);const ref=reference(this),id=record?record.id:key(ref);if(record)record.disposed=true;locks.delete(id);partialDisplay.delete(id);player.bitmap_dispose(id);for(const [entry,uRec]of uploads)if(uRec.pixelKey===id)drop(entry);}
    }
    Object.defineProperties(BitmapData,{__nfIs:{value:value=>references.has(value)},__nfInitialize:{value:()=>unsupported('native subclassing')},loadBitmap:{value:()=>unsupported('loadBitmap library linkage')}});
    Object.defineProperty(BitmapData,Symbol.hasInstance,{value:BitmapData.__nfIs});
    if(as2)as2.register(BitmapData.loadBitmap,BitmapData,'loadBitmap',id=>{if(typeof id!=='string')throw new TypeError('BitmapData.loadBitmap requires a linkage or image resource identifier string.');const reference=JSON.parse(native('as2_bitmap_load',id));return reference===null?null:fromRef({id:reference.id,generation:reference.generation});});
    function release(record){record.canvas.width=0;record.canvas.height=0;}
    function drop(id){const record=uploads.get(id);if(!record)return;release(record);cacheBytes-=record.cost;uploads.delete(id);}
    function canvas(width,height){
      const result=createCanvas?createCanvas(width,height):typeof global.OffscreenCanvas==='function'?new global.OffscreenCanvas(width,height):global.document?.createElement('canvas');
      if(!result)throw new Error('Bitmap rendering requires a Canvas 2D surface.');result.width=width;result.height=height;return result;
    }
    function applyColorTransformRgba(srcBytes,colorTransform){
      const identity=colorTransform.multipliers.every(v=>v===1)&&colorTransform.offsets.every(v=>v===0);
      if(identity)return srcBytes;
      const out=new Uint8Array(srcBytes.length),[rm,gm,bm,am]=colorTransform.multipliers,[ro,go,bo,ao]=colorTransform.offsets;
      const clamp=v=>v<=0?0:v>=255?255:Math.round(v);
      for(let i=0;i<srcBytes.length;i+=4){
        out[i]=clamp(srcBytes[i]*rm+ro);
        out[i+1]=clamp(srcBytes[i+1]*gm+go);
        out[i+2]=clamp(srcBytes[i+2]*bm+bo);
        out[i+3]=clamp(srcBytes[i+3]*am+ao);
      }
      return out;
    }
    function upload(kind){
      active();const pixelKey=key(kind.pixel_ref),colorTransform=kind.color_transform||identityColor;
      const lockState=locks.get(pixelKey);
      const partial=partialDisplay.get(pixelKey);
      if(partial&&partial.revision!==kind.revision&&!lockState)partialDisplay.delete(pixelKey);
      const activePartial=!lockState?partialDisplay.get(pixelKey):null;
      const effectiveRev=lockState?`locked:${lockState.lockedRevision}`:(activePartial?`partial:${activePartial.revision}:${activePartial.token}`:kind.revision);
      const colorKey=JSON.stringify({multipliers:colorTransform.multipliers,offsets:colorTransform.offsets});
      const id=JSON.stringify([pixelKey,effectiveRev,kind.width,kind.height,colorKey]);used.add(id);
      const old=uploads.get(id);if(old){uploads.delete(id);uploads.set(id,old);return old;}
      // Keep the Canvas, one hit-test bit per pixel and four colors for clamped brushes.
      // The temporary RGBA upload is released once the surface has received it.
      const pixelCount=kind.width*kind.height;
      const cost=pixelCount*4+Math.ceil(pixelCount/8)+16;
      if(!Number.isSafeInteger(cost)||cost<0)throw new RangeError('Invalid bitmap cache dimensions.');
      let reusable=null;
      for(const [entry,record]of uploads)if(record.pixelKey===pixelKey&&record.revision!==effectiveRev){
        if(!reusable&&record.colorKey===colorKey){reusable=record.canvas;cacheBytes-=record.cost;uploads.delete(entry);}
        else drop(entry);
      }
      if(cost<=maxCacheBytes&&maxCacheEntries>0)while(uploads.size&&(cacheBytes+cost>maxCacheBytes||uploads.size>=maxCacheEntries))drop(uploads.keys().next().value);
      const identity=colorTransform.multipliers.every(value=>value===1)&&colorTransform.offsets.every(value=>value===0);
      const bytes=lockState?applyColorTransformRgba(lockState.bytes,colorTransform)
        :activePartial?applyColorTransformRgba(activePartial.bytes,colorTransform)
        :(identity?native('bitmap_pixels',pixelKey):native('bitmap_pixels_transformed',pixelKey,colorKey));
      if(bytes.length!==kind.width*kind.height*4)throw new Error('Rust bitmap dimensions do not match its RGBA buffer.');
      const surface=reusable||canvas(kind.width,kind.height),context=surface.getContext('2d');
      if(!context)throw new Error('Bitmap rendering requires Canvas 2D.');
      if(surface.width!==kind.width)surface.width=kind.width;if(surface.height!==kind.height)surface.height=kind.height;
      const data=context.createImageData(kind.width,kind.height);data.data.set(bytes);context.putImageData(data,0,0);
      let alphaBounds=null;
      const hitBits=new Uint8Array(Math.ceil(pixelCount/8));
      const offsets=[0,kind.width-1,pixelCount-kind.width,pixelCount-1];
      const corners=offsets.map(index=>bytes.slice(index*4,index*4+4));
      for(let y=0;y<kind.height;y++)for(let x=0;x<kind.width;x++){
        const index=y*kind.width+x;
        if(!bytes[index*4+3])continue;
        hitBits[index>>>3]|=1<<(index&7);
        if(!alphaBounds)alphaBounds={left:x,top:y,right:x+1,bottom:y+1};
        else {alphaBounds.left=Math.min(alphaBounds.left,x);alphaBounds.top=Math.min(alphaBounds.top,y);alphaBounds.right=Math.max(alphaBounds.right,x+1);alphaBounds.bottom=Math.max(alphaBounds.bottom,y+1);}
      }
      const record={canvas:surface,hitBits,corners,width:kind.width,height:kind.height,revision:effectiveRev,alphaBounds,pixelKey,colorKey,cost,retained:cost<=maxCacheBytes&&maxCacheEntries>0};
      if(record.retained){uploads.set(id,record);cacheBytes+=cost;}return record;
    }
    function consume(kind,operation){const record=upload(kind);try{return operation(record);}finally{if(!record.retained)release(record);}}
    const cache=Object.freeze({
      transform:snapped,
      paint(kind,operation){return consume(kind,operation);},
      beginFrame(){active();used=new Set();},
      draw(context,kind){
        consume(kind,record=>{context.save();
          try {context.imageSmoothingEnabled=Boolean(kind.smoothing);const m=context.getTransform();context.setTransform(...snapped([m.a,m.b,m.c,m.d,m.e,m.f],kind.pixel_snapping));context.drawImage(record.canvas,0,0,kind.width,kind.height);}
          finally {context.restore();}
        });
      },
      hit(kind,x,y){
        if(x<0||y<0||x>=kind.width||y>=kind.height)return false;
        if(!createCanvas&&typeof global.OffscreenCanvas!=='function'&&!global.document){
          const pixelKey=key(kind.pixel_ref),ct=kind.color_transform;
          const bytes=ct?native('bitmap_pixels_transformed',pixelKey,JSON.stringify({multipliers:ct.multipliers,offsets:ct.offsets})):native('bitmap_pixels',pixelKey);
          return bytes[(Math.floor(y)*kind.width+Math.floor(x))*4+3]>0;
        }
        const index=Math.floor(y)*kind.width+Math.floor(x);
        return consume(kind,record=>Boolean(record.hitBits[index>>>3]&(1<<(index&7))));
      },
      bounds(kind){return consume(kind,record=>record.alphaBounds);},
      stats(){return {entries:uploads.size,bytes:cacheBytes,maxBytes:maxCacheBytes,maxEntries:maxCacheEntries,lockedBitmaps:locks.size,invalidationFlushes};},
      isLocked(bmd){const id=key(reference(bmd));return locks.has(id);},
      effectivePixels(pixelKey,colorTransform=identityColor){
        const lockState=locks.get(pixelKey);
        if(lockState)return applyColorTransformRgba(lockState.bytes,colorTransform);
        const partial=partialDisplay.get(pixelKey);
        if(partial)return applyColorTransformRgba(partial.bytes,colorTransform);
        const identity=colorTransform.multipliers.every(v=>v===1)&&colorTransform.offsets.every(v=>v===0);
        return identity?native('bitmap_pixels',pixelKey):native('bitmap_pixels_transformed',pixelKey,JSON.stringify({multipliers:colorTransform.multipliers,offsets:colorTransform.offsets}));
      },
      endFrame(){for(const id of uploads.keys())if(!used.has(id))drop(id);},
      clear(){for(const id of [...uploads.keys()])drop(id);used.clear();},
    });
    function reset(){active();cache.clear();facades.clear();locks.clear();partialDisplay.clear();invalidationFlushes=0;generation++;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({BitmapData,BitmapDataChannel,BitmapEncodingColorSpace,PixelSnapping,PNGEncoderOptions,JPEGEncoderOptions,JPEGXREncoderOptions,setUintVectorFactory,setHistogramVectorFactory,fromRef,refOf,cache,createCanvas:canvas,reset,dispose});
  }
  const api=Object.freeze({create,BitmapDataChannel,BitmapEncodingColorSpace,PixelSnapping,PNGEncoderOptions,JPEGEncoderOptions,JPEGXREncoderOptions});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashBitmap=api;
})(globalThis);
