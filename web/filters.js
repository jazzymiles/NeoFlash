/* Original display-filter value objects and whole-subtree Canvas raster adapter.
 * Primary contracts: AIR flash.filters.{BlurFilter,GlowFilter,DropShadowFilter,
 * ColorMatrixFilter}; filter arrays/matrix getters are detached copies.
 * Source is transformed into logical Stage pixels, nested subtrees are filtered
 * inside-out, then the owner's mask/scrollRect is applied. Owner clips are
 * deferred during source painting; child-only clipping occurs before the parent
 * filter. Rust owns validation/padding/RGBA kernels; there is no CSS-filter fallback.
 * Browser font rasterization and logical-pixel box blur are a bounded backend
 * profile, not historical Flash pixel conformance. Effects add no hit geometry.
 */
(function(global){
  'use strict';
  const states=new WeakMap(),token=Symbol('filter'),MIB=1024*1024;
  const identity=[1,0,0,0,0,0,1,0,0,0,0,0,1,0,0,0,0,0,1,0];
  const names={blurX:'blur_x',blurY:'blur_y',hideObject:'hide_object',matrixX:'matrix_x',matrixY:'matrix_y',preserveAlpha:'preserve_alpha',highlightColor:'highlight_color',highlightAlpha:'highlight_alpha',shadowColor:'shadow_color',shadowAlpha:'shadow_alpha',type:'placement'};
  function number(value,name,min,max,integer=false){if(typeof value!=='number'||!Number.isFinite(value)||value<min||value>max||(integer&&!Number.isInteger(value)))throw new RangeError(`Filter.${name} requires ${integer?'an integer':'a finite number'} from ${min} through ${max}.`);return value;}
  function validate(name,value,kind){
    if(['matrix','colors','alphas','ratios'].includes(name)){
      const exact=name==='matrix'&&kind==='color_matrix'?20:null,max=name==='matrix'?225:16;
      if(!Array.isArray(value)||value.length>max||(exact!==null&&value.length!==exact)||Array.from({length:value.length},(_,i)=>!Object.hasOwn(value,i)).some(Boolean))throw new TypeError(`Filter.${name} requires a dense Array${exact===null?' of at most '+max:' of exactly '+exact} entries.`);
      const min=name==='matrix'?-65536:0,high=name==='matrix'?65536:name==='colors'?0xffffff:name==='alphas'?1:255;
      return value.map(item=>number(item,name,min,high,name==='colors'||name==='ratios'));
    }
    if(name==='type'){if(!['inner','outer','full'].includes(value))throw new TypeError('Filter.type requires inner, outer or full.');return value;}
    if(['inner','knockout','hideObject','preserveAlpha','clamp'].includes(name)){if(typeof value!=='boolean')throw new TypeError(`Filter.${name} requires a Boolean.`);return value;}
    if(['color','highlightColor','shadowColor'].includes(name))return number(value,name,0,0xffffff,true);
    if(['quality','matrixX','matrixY'].includes(name))return number(value,name,0,15,true);
    if(['alpha','highlightAlpha','shadowAlpha'].includes(name))return number(value,name,0,1);
    if(name==='distance')return number(value,name,-8192,8192);
    if(name==='angle')return number(value,name,-1e9,1e9);
    if(name==='divisor'||name==='bias')return number(value,name,-65536,65536);
    return number(value,name,0,255);
  }
  class BitmapFilter {
    constructor(secret,type){if(secret!==token)throw new TypeError('BitmapFilter is a supplied base; construct a supported concrete filter.');states.set(this,{type});}
    clone(){return fromSpec(spec(this));}
  }
  class BlurFilter extends BitmapFilter {constructor(blurX=4,blurY=4,quality=1){super(token,'blur');this.blurX=blurX;this.blurY=blurY;this.quality=quality;Object.seal(this);}}
  class GlowFilter extends BitmapFilter {constructor(color=0xff0000,alpha=1,blurX=6,blurY=6,strength=2,quality=1,inner=false,knockout=false){super(token,'glow');Object.assign(this,{color,alpha,blurX,blurY,strength,quality,inner,knockout});Object.seal(this);}}
  class DropShadowFilter extends BitmapFilter {constructor(distance=4,angle=45,color=0,alpha=1,blurX=4,blurY=4,strength=1,quality=1,inner=false,knockout=false,hideObject=false){super(token,'drop_shadow');Object.assign(this,{distance,angle,color,alpha,blurX,blurY,strength,quality,inner,knockout,hideObject});Object.seal(this);}}
  class ColorMatrixFilter extends BitmapFilter {constructor(matrix=null){super(token,'color_matrix');this.matrix=matrix===null?identity:matrix;Object.seal(this);}}
  class ConvolutionFilter extends BitmapFilter {constructor(matrixX=0,matrixY=0,matrix=null,divisor=1,bias=0,preserveAlpha=true,clamp=true,color=0,alpha=0){super(token,'convolution');Object.assign(this,{matrixX,matrixY,matrix:matrix===null?[]:matrix,divisor,bias,preserveAlpha,clamp,color,alpha});Object.seal(this);}}
  class BevelFilter extends BitmapFilter {constructor(distance=4,angle=45,highlightColor=0xffffff,highlightAlpha=1,shadowColor=0,shadowAlpha=1,blurX=4,blurY=4,strength=1,quality=1,type='inner',knockout=false){super(token,'bevel');Object.assign(this,{distance,angle,highlightColor,highlightAlpha,shadowColor,shadowAlpha,blurX,blurY,strength,quality,type,knockout});Object.seal(this);}}
  class GradientGlowFilter extends BitmapFilter {constructor(distance=4,angle=45,colors=null,alphas=null,ratios=null,blurX=4,blurY=4,strength=1,quality=1,type='inner',knockout=false){super(token,'gradient_glow');Object.assign(this,{distance,angle,colors:colors===null?[]:colors,alphas:alphas===null?[]:alphas,ratios:ratios===null?[]:ratios,blurX,blurY,strength,quality,type,knockout});Object.seal(this);}}
  class GradientBevelFilter extends BitmapFilter {constructor(distance=4,angle=45,colors=null,alphas=null,ratios=null,blurX=4,blurY=4,strength=1,quality=1,type='inner',knockout=false){super(token,'gradient_bevel');Object.assign(this,{distance,angle,colors:colors===null?[]:colors,alphas:alphas===null?[]:alphas,ratios:ratios===null?[]:ratios,blurX,blurY,strength,quality,type,knockout});Object.seal(this);}}
  const classes={blur:BlurFilter,glow:GlowFilter,drop_shadow:DropShadowFilter,color_matrix:ColorMatrixFilter,convolution:ConvolutionFilter,bevel:BevelFilter,gradient_glow:GradientGlowFilter,gradient_bevel:GradientBevelFilter};
  const gradientProperties=['distance','angle','colors','alphas','ratios','blurX','blurY','strength','quality','type','knockout'];
  for(const[Constructor,properties]of [[BlurFilter,['blurX','blurY','quality']],[GlowFilter,['color','alpha','blurX','blurY','strength','quality','inner','knockout']],[DropShadowFilter,['distance','angle','color','alpha','blurX','blurY','strength','quality','inner','knockout','hideObject']],[ColorMatrixFilter,['matrix']],[ConvolutionFilter,['matrixX','matrixY','matrix','divisor','bias','preserveAlpha','clamp','color','alpha']],[BevelFilter,['distance','angle','highlightColor','highlightAlpha','shadowColor','shadowAlpha','blurX','blurY','strength','quality','type','knockout']],[GradientGlowFilter,gradientProperties],[GradientBevelFilter,gradientProperties]]){
    for(const name of properties)Object.defineProperty(Constructor.prototype,name,{enumerable:true,get(){const record=states.get(this);if(!record)throw new TypeError('Expected a supported BitmapFilter.');const value=record[names[name]||name];return Array.isArray(value)?value.slice():value;},set(value){const record=states.get(this);if(!record)throw new TypeError('Expected a supported BitmapFilter.');record[names[name]||name]=validate(name,value,record.type);}});
  }
  for(const Constructor of [BitmapFilter,...Object.values(classes)])Object.defineProperties(Constructor,{__nfIs:{value:value=>states.has(value)&&(Constructor===BitmapFilter||classes[states.get(value).type]===Constructor)},__nfInitialize:{value:()=>{throw new TypeError('Native filter subclassing is not supported.');}},[Symbol.hasInstance]:{value:value=>states.has(value)&&(Constructor===BitmapFilter||classes[states.get(value).type]===Constructor)}});
  class BitmapFilterQuality {constructor(){throw new TypeError('BitmapFilterQuality supplies LOW, MEDIUM and HIGH constants.');}}
  Object.assign(BitmapFilterQuality,{LOW:1,MEDIUM:2,HIGH:3});Object.freeze(BitmapFilterQuality);
  class BitmapFilterType {constructor(){throw new TypeError('BitmapFilterType supplies INNER, OUTER and FULL constants.');}}
  Object.assign(BitmapFilterType,{INNER:'inner',OUTER:'outer',FULL:'full'});Object.freeze(BitmapFilterType);
  class DisplacementMapFilterMode {constructor(){throw new TypeError('DisplacementMapFilterMode supplies CLAMP, COLOR, IGNORE and WRAP constants.');}}
  Object.assign(DisplacementMapFilterMode,{CLAMP:'clamp',COLOR:'color',IGNORE:'ignore',WRAP:'wrap'});Object.freeze(DisplacementMapFilterMode);
  function spec(value){const record=states.get(value);if(record?.type==='as2_displacement')throw new TypeError('DisplacementMapFilter currently supports BitmapData.applyFilter; DisplayObject.filters map snapshots are not implemented.');if(!record)throw new TypeError('DisplayObject.filters accepts supported BitmapFilter instances.');return Object.fromEntries(Object.entries(record).map(([key,item])=>[key,Array.isArray(item)?item.slice():item]));}
  function fromSpec(value){const Constructor=classes[value?.type];if(!Constructor)throw new TypeError('Unsupported runtime filter specification.');const result=new Constructor();for(const[key,itemValue]of Object.entries(value)){if(key==='type')continue;const name=Object.keys(names).find(name=>names[name]===key)||key;result[name]=itemValue;}return result;}
  function fromSpecs(values){if(!Array.isArray(values))throw new TypeError('Invalid runtime filter array.');return values.map(fromSpec);}
  function toSpecs(values){if(values===null)return [];if(!Array.isArray(values)||values.length>8||Array.from({length:values.length},(_,i)=>!Object.hasOwn(values,i)).some(Boolean))throw new TypeError('DisplayObject.filters requires null or a dense Array of at most eight filters.');return values.map(spec);}
  const union=(a,b)=>!a?b:!b?a:{left:Math.min(a.left,b.left),top:Math.min(a.top,b.top),right:Math.max(a.right,b.right),bottom:Math.max(a.bottom,b.bottom)};
  function rectangle(value){if(!value)return null;if(!['left','top','right','bottom'].every(key=>typeof value[key]==='number'&&Number.isFinite(value[key])))throw new RangeError('Filter geometry bounds must be finite.');return value.right>value.left&&value.bottom>value.top?value:null;}
  class Renderer {
    constructor({player,createCanvas,maxSurfacePixels=4*MIB,maxPoolPixels=8*MIB,maxFramePixels=16*MIB,maxSurfaces=16}={}){
      if(!player?.filter_rgba||!player?.filter_padding)throw new TypeError('Filters require the shared Rust RGBA kernel.');
      for(const value of [maxSurfacePixels,maxPoolPixels,maxFramePixels,maxSurfaces])if(!Number.isSafeInteger(value)||value<1)throw new RangeError('Filter raster budgets must be positive safe integers.');
      Object.assign(this,{player,maxSurfacePixels,maxPoolPixels,maxFramePixels,maxSurfaces});
      this.createCanvas=createCanvas||(()=>typeof OffscreenCanvas==='function'?new OffscreenCanvas(1,1):global.document.createElement('canvas'));
      this.pool=[];this.records=new Map();this.clips=new Map();this.sequence=0;this.version=0;this.framePixels=0;
    }
    discard(surface){for(const record of this.records.values())if(record.surface===surface)record.surface=null;surface.owner=null;}
    acquire(width,height){
      const pixels=width*height;
      if(width>8192||height>8192||pixels>this.maxSurfacePixels)throw new RangeError('Filter raster exceeds 8192 per side or the 4 Mi-pixel surface budget; reduce the filtered subtree or filter padding.');
      let surface=this.pool.find(item=>!item.busy&&!item.owner&&item.width===width&&item.height===height);
      if(!surface){
        for(;;){const total=this.pool.reduce((sum,item)=>sum+item.width*item.height,0);if(total+pixels<=this.maxPoolPixels&&this.pool.length<this.maxSurfaces)break;
          const oldest=this.pool.filter(item=>!item.busy).sort((a,b)=>a.used-b.used)[0];if(!oldest)throw new RangeError('Nested filter surfaces exceed the shared 8 Mi-pixel pool budget.');
          this.discard(oldest);this.pool.splice(this.pool.indexOf(oldest),1);oldest.canvas.width=oldest.canvas.height=1;
        }
        const canvas=this.createCanvas();canvas.width=width;canvas.height=height;const context=canvas.getContext('2d');if(!context)throw new Error('Canvas 2D filter surface is unavailable.');
        surface={canvas,context,width,height,owner:null,busy:false,used:0};this.pool.push(surface);
      }
      surface.busy=true;surface.used=++this.sequence;
      const context=surface.context;context.setTransform(1,0,0,1,0,0);context.globalAlpha=1;context.globalCompositeOperation='source-over';context.clearRect(0,0,width,height);return surface;
    }
    clear(){for(const surface of this.pool)surface.canvas.width=surface.canvas.height=1;this.pool=[];this.records.clear();this.clips.clear();this.framePixels=0;}
    stats(){return {surfaces:this.pool.length,pixels:this.pool.reduce((sum,item)=>sum+item.width*item.height,0),cached:[...this.records.values()].filter(item=>item.surface).length,keyBytes:[...this.records.values(),...this.clips.values()].reduce((sum,item)=>sum+(item.key?.length||0)*2,0),framePixels:this.framePixels};}
    clipVersions(snapshot){
      const live=new Set();for(const clip of snapshot.clips||[]){live.add(clip.id);const key=JSON.stringify(clip),previous=this.clips.get(clip.id);if(!previous||previous.key!==key)this.clips.set(clip.id,{key:key.length<=16384?key:null,version:++this.version});}
      for(const id of this.clips.keys())if(!live.has(id))this.clips.delete(id);
    }
    draw(context,snapshot,{bounds,paint,composite}={}){
      if(typeof bounds!=='function'||typeof paint!=='function'||typeof composite!=='function')throw new TypeError('Filter renderer requires geometry, leaf paint and clipped composite callbacks.');
      this.framePixels=0;this.clipVersions(snapshot);
      const nodes=new Map(),root={id:null,entries:[],clip_ids:[]},live=new Set();
      for(const group of snapshot.filter_groups||[]){if(nodes.has(group.id))throw new Error('Duplicate filter group.');nodes.set(group.id,{...group,entries:[]});live.add(group.id);}
      for(const[id,record]of this.records)if(!live.has(id)){if(record.surface)this.discard(record.surface);this.records.delete(id);}
      const chains=new Map(),closed=new Set();let previous=[];
      const ancestry=id=>{if(id===null||id===undefined)return [];if(chains.has(id))return chains.get(id);const chain=[],seen=new Set();for(let node=nodes.get(id);node;node=node.parent_id===null?null:nodes.get(node.parent_id)){if(seen.has(node.id)||chain.length>=64)throw new Error('Invalid filter group ancestry.');seen.add(node.id);chain.unshift(node);if(node.parent_id!==null&&!nodes.has(node.parent_id))throw new Error('Missing parent filter group.');}if(!chain.length)throw new Error('Missing display filter group.');chains.set(id,chain);return chain;};
      for(const item of snapshot.draws){const chain=ancestry(item.filter_id);let common=0;while(common<previous.length&&common<chain.length&&previous[common].id===chain[common].id)common++;
        for(const node of previous.slice(common))closed.add(node.id);
        for(let at=common;at<chain.length;at++){const node=chain[at];if(closed.has(node.id))throw new Error('Filtered subtree is not contiguous in display order.');(at?chain[at-1]:root).entries.push({group:node});}
        (chain.at(-1)||root).entries.push({item,bounds:chain.length?rectangle(bounds(item)):null});previous=chain;
      }
      const clipKeys=(ids,deferred=[])=>ids.filter(id=>!deferred.includes(id)).map(id=>{const version=this.clips.get(id)?.version;if(version===undefined)throw new Error('Missing clip in filter source.');return[id,version];});
      const prepare=node=>{
        let area=null;for(const entry of node.entries){if(entry.group){prepare(entry.group);area=union(area,entry.group.bounds);}else area=union(area,entry.bounds);}
        if(node===root)return;
        const filters=JSON.stringify(node.filters),pad=JSON.parse(this.player.filter_padding(filters));
        node.bounds=area?{left:Math.floor(area.left-pad.left),top:Math.floor(area.top-pad.top),right:Math.ceil(area.right+pad.right),bottom:Math.ceil(area.bottom+pad.bottom)}:null;
        node.sourceBounds=area;
        const key=JSON.stringify({filters,operation:node.composite_operation,background:node.opaque_background,cache:node.cache_as_bitmap,bounds:node.bounds,entries:node.entries.map(entry=>entry.group?{group:entry.group.id,version:entry.group.record.version,clips:clipKeys(entry.group.clip_ids,node.clip_ids)}:{item:entry.item,clips:clipKeys(entry.item.clip_ids||[],node.clip_ids)})});
        let record=this.records.get(node.id);if(!record||record.key!==key){if(record?.surface)this.discard(record.surface);record={key:key.length<=32768?key:null,version:++this.version,surface:null};this.records.set(node.id,record);}node.record=record;node.filtersJSON=filters;
      };
      prepare(root);
      const raster=node=>{
        if(!node.bounds)return null;const record=node.record;if(record.surface){record.surface.used=++this.sequence;return record.surface;}
        const width=node.bounds.right-node.bounds.left,height=node.bounds.bottom-node.bounds.top;
        if(this.framePixels+width*height>this.maxFramePixels)throw new RangeError('Filter raster work exceeds the per-frame 16 Mi-pixel budget.');
        const surface=this.acquire(width,height);let success=false;
        try{
          const target=surface.context;target.setTransform(1,0,0,1,-node.bounds.left,-node.bounds.top);
          if(node.opaque_background&&node.sourceBounds){const [r,g,b,a]=node.opaque_background,area=node.sourceBounds;target.fillStyle=`rgba(${r},${g},${b},${a/255})`;target.fillRect(area.left,area.top,area.right-area.left,area.bottom-area.top);}
          entries(target,node);
          const source=target.getImageData(0,0,width,height),bytes=this.player.filter_rgba(width,height,source.data,node.filtersJSON);this.framePixels+=width*height;
          if(bytes.length!==width*height*4)throw new Error('Rust filter kernel returned an invalid RGBA length.');
          const result=target.createImageData(width,height);result.data.set(bytes);target.setTransform(1,0,0,1,0,0);target.putImageData(result,0,0);success=true;
          if(record.key!==null&&node.cache_as_bitmap!==false){record.surface=surface;surface.owner=node.id;}return surface;
        }finally{surface.busy=false;if(!success)this.discard(surface);}
      };
      const entries=(target,node)=>{for(const entry of node.entries){if(entry.item)paint(target,entry.item,node.clip_ids);else{const child=entry.group,surface=raster(child);if(surface)composite(target,surface.canvas,child.bounds,child.clip_ids.filter(id=>!node.clip_ids.includes(id)),child.composite_operation||'source-over');}}};
      entries(context,root);
    }
    coverage(bounds,draw){
      bounds=rectangle(bounds);if(!bounds)return null;const left=Math.floor(bounds.left)-1,top=Math.floor(bounds.top)-1,right=Math.ceil(bounds.right)+1,bottom=Math.ceil(bounds.bottom)+1;
      const surface=this.acquire(right-left,bottom-top);
      try{surface.context.setTransform(1,0,0,1,-left,-top);return draw(surface.context);}finally{surface.busy=false;}
    }
  }
  // AS2-only value facade. The shared display-filter serializer rejects it until
  // display snapshot ownership can carry a live map safely; BitmapData applies
  // it through the Rust pixel arena without copying algorithms into the host.
  function legacyDisplacement({getBitmap,geometry}={}){
    const fields=['mapBitmap','mapPoint','componentX','componentY','scaleX','scaleY','mode','color','alpha'];
    class DisplacementMapFilter extends BitmapFilter {
      constructor(mapBitmap=null,mapPoint=null,componentX=0,componentY=0,scaleX=0,scaleY=0,mode='wrap',color=0,alpha=0){
        super(token,'as2_displacement');Object.assign(this,{mapBitmap,mapPoint,componentX,componentY,scaleX,scaleY,mode,color,alpha});Object.seal(this);
      }
      clone(){return new DisplacementMapFilter(...fields.map(name=>this[name]));}
    }
    for(const name of fields)Object.defineProperty(DisplacementMapFilter.prototype,name,{enumerable:true,
      get(){const record=states.get(this);if(record?.type!=='as2_displacement')throw new TypeError('Expected AS2 DisplacementMapFilter.');const value=record[name];return name==='mapPoint'&&value!==null?(geometry?.Point?new geometry.Point(value.x,value.y):{...value}):value;},
      set(value){const record=states.get(this);if(record?.type!=='as2_displacement')throw new TypeError('Expected AS2 DisplacementMapFilter.');
        if(name==='mapBitmap'){if(value!==null)getBitmap?.().refOf(value);}
        else if(name==='mapPoint'){if(value!==null){if(!value||!Number.isInteger(value.x)||!Number.isInteger(value.y)||Math.abs(value.x)>2147483647||Math.abs(value.y)>2147483647)throw new TypeError('DisplacementMapFilter.mapPoint requires an integer Point or null.');value={x:value.x,y:value.y};}}
        else if(name==='componentX'||name==='componentY'){if(![0,1,2,4,8].includes(value))throw new RangeError('Displacement component requires 0, 1, 2, 4 or 8.');}
        else if(name==='scaleX'||name==='scaleY')value=number(value,name,-8192,8192);
        else if(name==='mode'){if(!['wrap','clamp','ignore','color'].includes(value))throw new TypeError('Displacement mode requires wrap, clamp, ignore or color.');}
        else value=validate(name,value,'as2_displacement');
        record[name]=value;
      }});
    Object.defineProperties(DisplacementMapFilter,{__nfIs:{value:value=>states.get(value)?.type==='as2_displacement'},__nfInitialize:{value:()=>{throw new TypeError('Native displacement filter subclassing is unsupported.');}},[Symbol.hasInstance]:{value:value=>states.get(value)?.type==='as2_displacement'}});
    const as2Displacement=value=>{
      const state=states.get(value);if(state?.type!=='as2_displacement')return null;
      return {map_bitmap:state.mapBitmap===null?null:getBitmap().refOf(state.mapBitmap),map_point:state.mapPoint===null?[0,0]:[state.mapPoint.x,state.mapPoint.y],component_x:state.componentX,component_y:state.componentY,scale_x:state.scaleX,scale_y:state.scaleY,mode:state.mode,color:state.color,alpha:state.alpha};
    };
    return {DisplacementMapFilter,as2Displacement};
  }
  function create(options={}){return Object.freeze({...legacyDisplacement(options),BitmapFilter,BlurFilter,GlowFilter,DropShadowFilter,ColorMatrixFilter,ConvolutionFilter,BevelFilter,GradientGlowFilter,GradientBevelFilter,BitmapFilterType,BitmapFilterQuality,DisplacementMapFilterMode,toSpecs,fromSpecs,renderer:new Renderer(options)});}
  const api=Object.freeze({create,Renderer,BitmapFilter,BlurFilter,GlowFilter,DropShadowFilter,ColorMatrixFilter,ConvolutionFilter,BevelFilter,GradientGlowFilter,GradientBevelFilter,BitmapFilterType,BitmapFilterQuality,DisplacementMapFilterMode,toSpecs,fromSpecs});
  if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashFilters=api;
})(globalThis);
