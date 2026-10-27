/* BitmapData facade and read-only Canvas upload cache over the Rust pixel arena.
 * Primary contracts: https://airsdk.dev/reference/actionscript/3.0/flash/display/BitmapData.html
 * https://airsdk.dev/reference/actionscript/3.0/flash/display/Bitmap.html
 * Pixel mutation, premultiplication, clipping, overlapping copies and disposal
 * belong to Rust. JS retains only revision-stamped render copies, never a second
 * editable pixel model. Integer coordinates and uint colors are required; alpha
 * masks, draw/filter/encode APIs and native BitmapData subclassing are pending.
 * AS2 rectangle aliases rect; invalid/disposed access uses the shared diagnostic
 * profile, not historical AS2 sentinel return values. Pixel snapping is measured
 * in Canvas drawing-buffer pixels, with no Retina/Flash rasterization parity claim.
 */
(function(global){
  'use strict';
  const unsupported=name=>{throw new TypeError(`BitmapData.${name} is not implemented in the shared pixel-buffer profile.`);};
  const key=reference=>JSON.stringify(reference);
  function integer(value,name){if(!Number.isInteger(value)||value< -2147483648||value>2147483647)throw new RangeError(`${name} requires a signed 32-bit integer.`);return value;}
  function color(value){if(!Number.isInteger(value)||value<0||value>0xffffffff)throw new RangeError('BitmapData color requires an unsigned 32-bit ARGB integer.');return value;}
  function rectangle(value){if(!value)throw new TypeError('BitmapData operation requires a Rectangle.');return [integer(value.x,'Rectangle.x'),integer(value.y,'Rectangle.y'),integer(value.width,'Rectangle.width'),integer(value.height,'Rectangle.height')];}
  function point(value){if(!value)throw new TypeError('BitmapData operation requires a Point.');return [integer(value.x,'Point.x'),integer(value.y,'Point.y')];}
  class PixelSnapping {constructor(){throw new TypeError('PixelSnapping supplies AUTO, ALWAYS and NEVER constants.');}}
  Object.assign(PixelSnapping,{AUTO:'auto',ALWAYS:'always',NEVER:'never'});
  Object.freeze(PixelSnapping);
  function snapped(matrix,mode='auto'){
    const [a,b,c,d,e,f]=matrix;
    if(mode==='always')return [a,b,c,d,Math.round(e),Math.round(f)];
    if(mode==='auto'&&b===0&&c===0&&Math.abs(a-1)<=0.001&&Math.abs(d-1)<=0.001)return [1,0,0,1,Math.round(e),Math.round(f)];
    return [...matrix];
  }
  function create({player,geometry,createCanvas,maxCacheBytes=128*1024*1024,maxCacheEntries=256}={}){
    if(!player||!geometry?.Rectangle)throw new TypeError('BitmapData requires the shared Rust Player and Rectangle type.');
    if(!Number.isSafeInteger(maxCacheBytes)||maxCacheBytes<0||!Number.isSafeInteger(maxCacheEntries)||maxCacheEntries<0)throw new RangeError('Bitmap cache budgets must be nonnegative safe integers.');
    const references=new WeakMap(),facades=new Map(),uploads=new Map();
    let generation=1,disposed=false,used=new Set(),cacheBytes=0;
    const identityColor={multipliers:[1,1,1,1],offsets:[0,0,0,0]};
    function active(){if(disposed)throw new Error('This bitmap service has been disposed.');}
    function reference(value){active();const record=references.get(value);if(!record)throw new TypeError('Expected BitmapData from this publication.');if(record.generation!==generation)throw new Error('This BitmapData belongs to a previous publication session.');return record.ref;}
    function descriptor(value){const result=JSON.parse(player.bitmap_info(key(reference(value))));if(result.disposed)throw new Error('This BitmapData has been disposed.');return result;}
    function refOf(value){descriptor(value);return reference(value);}
    function fromRef(ref){
      active();const id=key(ref);if(facades.has(id))return facades.get(id);
      const facade=Object.create(BitmapData.prototype);references.set(facade,{ref,generation});Object.seal(facade);facades.set(id,facade);return facade;
    }
    class BitmapData {
      constructor(width,height,transparent=true,fillColor=0xffffffff){
        active();integer(width,'BitmapData width');integer(height,'BitmapData height');
        if(width<=0||height<=0)throw new RangeError('BitmapData dimensions must be positive.');
        if(typeof transparent!=='boolean')throw new TypeError('BitmapData transparent requires a Boolean.');
        return fromRef(JSON.parse(player.bitmap_create(width,height,transparent,color(fillColor))));
      }
      get width(){return descriptor(this).width;}get height(){return descriptor(this).height;}
      get transparent(){return descriptor(this).transparent;}
      get rect(){const info=descriptor(this);return new geometry.Rectangle(0,0,info.width,info.height);}
      get rectangle(){return this.rect;}
      getPixel(x,y){return player.bitmap_get_pixel(key(refOf(this)),integer(x,'Pixel x'),integer(y,'Pixel y'),false);}
      getPixel32(x,y){return player.bitmap_get_pixel(key(refOf(this)),integer(x,'Pixel x'),integer(y,'Pixel y'),true);}
      setPixel(x,y,value){player.bitmap_set_pixel(key(refOf(this)),integer(x,'Pixel x'),integer(y,'Pixel y'),color(value),false);}
      setPixel32(x,y,value){player.bitmap_set_pixel(key(refOf(this)),integer(x,'Pixel x'),integer(y,'Pixel y'),color(value),true);}
      fillRect(rect,value){player.bitmap_fill_rect(key(refOf(this)),...rectangle(rect),color(value));}
      copyPixels(source,sourceRect,destPoint,alphaBitmapData=null,alphaPoint=null,mergeAlpha=false){
        if(alphaBitmapData!==null||alphaPoint!==null)unsupported('copyPixels alpha-mask arguments');
        if(typeof mergeAlpha!=='boolean')throw new TypeError('copyPixels mergeAlpha requires a Boolean.');
        player.bitmap_copy_pixels(key(refOf(this)),key(refOf(source)),...rectangle(sourceRect),...point(destPoint),mergeAlpha);
      }
      clone(){return fromRef(JSON.parse(player.bitmap_clone(key(refOf(this)))));}
      scroll(x,y){player.bitmap_scroll(key(refOf(this)),integer(x,'Scroll x'),integer(y,'Scroll y'));}
      dispose(){const ref=reference(this),id=key(ref);player.bitmap_dispose(id);for(const [entry,record]of uploads)if(record.pixelKey===id)drop(entry);}
    }
    Object.defineProperties(BitmapData,{__nfIs:{value:value=>references.has(value)},__nfInitialize:{value:()=>unsupported('native subclassing')},loadBitmap:{value:()=>unsupported('loadBitmap library linkage')}});
    Object.defineProperty(BitmapData,Symbol.hasInstance,{value:BitmapData.__nfIs});
    for(const name of ['draw','drawWithQuality','applyFilter','generateFilterRect','colorTransform','compare','copyChannel','copyPixelsToByteArray','encode','floodFill','getColorBoundsRect','getPixels','getVector','hitTest','histogram','lock','unlock','merge','noise','paletteMap','perlinNoise','pixelDissolve','setPixels','setVector','threshold'])Object.defineProperty(BitmapData.prototype,name,{value(){unsupported(name);}});
    function release(record){record.canvas.width=0;record.canvas.height=0;}
    function drop(id){const record=uploads.get(id);if(!record)return;release(record);cacheBytes-=record.cost;uploads.delete(id);}
    function canvas(width,height){
      const result=createCanvas?createCanvas(width,height):typeof global.OffscreenCanvas==='function'?new global.OffscreenCanvas(width,height):global.document?.createElement('canvas');
      if(!result)throw new Error('Bitmap rendering requires a Canvas 2D surface.');result.width=width;result.height=height;return result;
    }
    function upload(kind){
      active();const pixelKey=key(kind.pixel_ref),colorTransform=kind.color_transform||identityColor;
      const colorKey=JSON.stringify({multipliers:colorTransform.multipliers,offsets:colorTransform.offsets});
      const id=JSON.stringify([pixelKey,kind.revision,kind.width,kind.height,colorKey]);used.add(id);
      const old=uploads.get(id);if(old){uploads.delete(id);uploads.set(id,old);return old;}
      // Each derived image retains RGBA bytes for alpha tests plus a Canvas
      // surface (estimated at four bytes/pixel). Browser/GPU bookkeeping is not
      // exactly measurable here. Oversized entries are transient, never cached.
      const cost=kind.width*kind.height*8;
      if(!Number.isSafeInteger(cost)||cost<0)throw new RangeError('Invalid bitmap cache dimensions.');
      let reusable=null;
      for(const [entry,record]of uploads)if(record.pixelKey===pixelKey&&record.revision!==kind.revision){
        if(!reusable&&record.colorKey===colorKey){reusable=record.canvas;cacheBytes-=record.cost;uploads.delete(entry);}
        else drop(entry);
      }
      if(cost<=maxCacheBytes&&maxCacheEntries>0)while(uploads.size&&(cacheBytes+cost>maxCacheBytes||uploads.size>=maxCacheEntries))drop(uploads.keys().next().value);
      const bytes=kind.color_transform?player.bitmap_pixels_transformed(pixelKey,colorKey):player.bitmap_pixels(pixelKey);
      if(bytes.length!==kind.width*kind.height*4)throw new Error('Rust bitmap dimensions do not match its RGBA buffer.');
      const surface=reusable||canvas(kind.width,kind.height),context=surface.getContext('2d');
      if(!context)throw new Error('Bitmap rendering requires Canvas 2D.');
      if(surface.width!==kind.width)surface.width=kind.width;if(surface.height!==kind.height)surface.height=kind.height;
      const data=context.createImageData(kind.width,kind.height);data.data.set(bytes);context.putImageData(data,0,0);
      let alphaBounds=null;
      for(let y=0;y<kind.height;y++)for(let x=0;x<kind.width;x++)if(bytes[(y*kind.width+x)*4+3]){
        if(!alphaBounds)alphaBounds={left:x,top:y,right:x+1,bottom:y+1};
        else {alphaBounds.left=Math.min(alphaBounds.left,x);alphaBounds.top=Math.min(alphaBounds.top,y);alphaBounds.right=Math.max(alphaBounds.right,x+1);alphaBounds.bottom=Math.max(alphaBounds.bottom,y+1);}
      }
      const record={canvas:surface,bytes,width:kind.width,height:kind.height,revision:kind.revision,alphaBounds,pixelKey,colorKey,cost,retained:cost<=maxCacheBytes&&maxCacheEntries>0};
      if(record.retained){uploads.set(id,record);cacheBytes+=cost;}return record;
    }
    function consume(kind,operation){const record=upload(kind);try{return operation(record);}finally{if(!record.retained)release(record);}}
    const cache=Object.freeze({
      transform:snapped,
      beginFrame(){active();used=new Set();},
      draw(context,kind){
        consume(kind,record=>{context.save();
          try {context.imageSmoothingEnabled=Boolean(kind.smoothing);const m=context.getTransform();context.setTransform(...snapped([m.a,m.b,m.c,m.d,m.e,m.f],kind.pixel_snapping));context.drawImage(record.canvas,0,0,kind.width,kind.height);}
          finally {context.restore();}
        });
      },
      hit(kind,x,y){if(x<0||y<0||x>=kind.width||y>=kind.height)return false;return consume(kind,record=>record.bytes[(Math.floor(y)*kind.width+Math.floor(x))*4+3]>0);},
      bounds(kind){return consume(kind,record=>record.alphaBounds);},
      stats(){return {entries:uploads.size,bytes:cacheBytes,maxBytes:maxCacheBytes,maxEntries:maxCacheEntries};},
      endFrame(){for(const id of uploads.keys())if(!used.has(id))drop(id);},
      clear(){for(const id of [...uploads.keys()])drop(id);used.clear();},
    });
    function reset(){active();cache.clear();facades.clear();generation++;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({BitmapData,PixelSnapping,fromRef,refOf,cache,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashBitmap=api;
})(globalThis);
