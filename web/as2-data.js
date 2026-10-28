/* AS2 nonvisual data references and event bridge. Rust owns all row IDs,
 * collection order, sort comparisons, view membership, cursors and local deltas.
 * Public contracts and bounded profile: docs/AS2-DATA-BATCH.md. */
(function(global){
  'use strict';
  function create(wasm,options={}){
    if(typeof wasm?.ScriptAS2Data!=='function'||typeof options.EventDispatcher?.initialize!=='function')throw new Error('AS2 data requires Rust ScriptAS2Data and the AS2 EventDispatcher service.');
    const models=new WeakMap(),iterators=new WeakMap(),packets=new WeakSet(),deltas=new WeakSet(),deltaItems=new WeakSet(),records=new Set();
    let generation=1,disposed=false;
    const unsupported=name=>{throw new Error(`${name} is outside the nonvisual AS2 data profile; use explicit item fields and local delta inspection.`);};
    function active(record){if(disposed||record.generation!==generation)throw new Error('This AS2 data object belongs to a disposed or restarted publication.');}
    function state(value){const record=models.get(value);if(!record)throw new TypeError('Expected a data model from this publication.');active(record);return record;}
    function call(record,op,arguments_={}){active(record);try{return JSON.parse(record.core.execute(JSON.stringify({op,...arguments_})));}catch(error){throw new Error(`AS2 data ${op}: ${String(error)}`);}}
    function index(value){if(typeof value!=='number'||!Number.isSafeInteger(value))throw new TypeError('Data indices require finite integers.');return value;}
    function field(value){if(typeof value!=='string'||!value.length||value.length>256||['__proto__','prototype','constructor'].includes(value))throw new TypeError('Data field names require 1–256 characters and cannot change object prototypes.');return value;}
    function object(value){if(value===null||typeof value!=='object')throw new TypeError('DataSet rows require ordinary objects.');return value;}
    function reference(record,value){
      const table=value!==null&&(typeof value==='object'||typeof value==='function')?record.objects:record.primitives;
      let id=table.get(value);if(id===undefined){id=call(record,'reference');table.set(value,id);record.references.set(id,value);}return id;
    }
    function resolve(record,id){return id===0?null:record.references.get(id);}
    function snapshot(record,view){return call(record,'snapshot',view===undefined?{}:{view});}
    function register(target,kind){
      if(disposed)throw new Error('The AS2 data service is disposed.');if(records.size>=256)throw new RangeError('A publication supports at most 256 data models before Restart.');
      const record={core:new wasm.ScriptAS2Data(),generation,target,kind,objects:new WeakMap(),primitives:new Map(),references:new Map(),filters:new Map([[0,null]]),packet:null,provider:null,array:null,mutating:false};
      models.set(target,record);records.add(record);if(kind!=='collection')options.EventDispatcher.initialize(target);return record;
    }
    function mutation(record,callback){active(record);if(record.mutating)throw new Error('Reentrant data mutation from a data event or filter is unsupported; edit after the callback returns.');call(record,'check_write');record.mutating=true;try{return callback();}finally{record.mutating=false;}}
    function emit(record,type,details={}){
      if(snapshot(record).disabled)return;
      const event={type,target:record.target,...details};record.target.dispatchEvent(event);
      if(record.provider&&type==='modelChanged'){active(record);record.provider.dispatchEvent({...event,target:record.provider});}
      return event;
    }
    function modelChanged(record,eventName,details={}){emit(record,'modelChanged',{eventName,...details});}
    function removeBatch(record,ids){
      const all=call(record,'all'),references=new Map(all.ids.map((id,i)=>[id,all.handles[i]])),accepted=[];
      for(const id of ids){const item=resolve(record,references.get(id));const event=record.kind==='dataset'?emit(record,'removeItem',{item,result:true}):null;if(event?.result!==false)accepted.push(id);}
      const removed=call(record,'remove_many',{ids:accepted});refresh(record);modelChanged(record,'removeItems',{firstItem:0,lastItem:ids.length-1,removedIDs:removed});
    }
    function primitive(value){if(value===undefined)return null;if(value===null||typeof value==='string'||typeof value==='boolean'||typeof value==='number'&&Number.isFinite(value))return value;throw new TypeError('Data sort/search keys require finite numbers, strings, Boolean or null; custom coercion objects are unsupported.');}
    function keys(record,value,names){const output={};for(const name of names){if(name==='$value')output[name]=primitive(value);else{if(value===null||typeof value!=='object'||!(name in value))throw new Error(`Data sort field '${name}' is absent on an item.`);output[name]=primitive(value[name]);}}return output;}
    function refresh(record,extra=[]){
      const all=call(record,'all'),names=[...new Set([...call(record,'fields'),...extra])];
      call(record,'keys',{rows:all.ids.map((id,i)=>({id,keys:keys(record,resolve(record,all.handles[i]),names)}))});
      for(const [view,filter]of record.filters){
        const current=snapshot(record,view);if(current.filtered){
          const accepted=[];for(let i=0;i<all.ids.length;i++){const value=resolve(record,all.handles[i]);if(!filter||Reflect.apply(filter,record.target,[value]))accepted.push(all.ids[i]);active(record);}
          call(record,'filter',{view,ids:accepted});
        }
      }
      const current=call(record,'refresh');
      if(record.array){record.array.splice(0,record.array.length,...current.handles.map(handle=>resolve(record,handle)));}
      return current;
    }
    function ensureArray(record){if(!record.array)return;const current=snapshot(record);const handles=record.array.map(value=>reference(record,value));if(handles.length!==current.handles.length||handles.some((id,i)=>id!==current.handles[i]))throw new Error('Use the DataProvider methods after initialize(array); direct array structure changes are unsupported.');}
    function currentView(record){return snapshot(record).active;}
    class IteratorImpl{
      constructor(){unsupported('Direct IteratorImpl construction; call getIterator()');}
      hasNext(){const value=iterators.get(this);if(!value||value.kind!=='snapshot')throw new TypeError('Expected a collection iterator.');return call(value.record,'has_next',{id:value.id});}
      next(){const value=iterators.get(this);if(!value||value.kind!=='snapshot')throw new TypeError('Expected a collection iterator.');return resolve(value.record,call(value.record,'next',{id:value.id}));}
    }
    function makeIterator(record,handles){const result=Object.create(IteratorImpl.prototype);iterators.set(result,{record,kind:'snapshot',id:call(record,'iterator',{handles})});return result;}
    class CollectionImpl{
      constructor(items=[]){if(!Array.isArray(items))throw new TypeError('CollectionImpl expects an optional array.');const record=register(this,'collection');call(record,'load',{handles:items.filter(value=>value!=null).map(value=>reference(record,value))});}
      addItem(item){if(item==null)return false;const r=state(this),s=snapshot(r);const result=call(r,'add',{handle:reference(r,item),index:s.length,unique:false});call(r,'refresh');return result.added;}
      clear(){const r=state(this);call(r,'remove_many',{ids:call(r,'all').ids});}
      contains(item){const r=state(this);return call(r,'contains',{handle:reference(r,item)});}
      getItemAt(position){const r=state(this),s=snapshot(r);return resolve(r,s.handles[index(position)]||0);}
      getIterator(){const r=state(this);return makeIterator(r,snapshot(r).handles);}
      getLength(){return snapshot(state(this)).length;}
      isEmpty(){return this.getLength()===0;}
      removeItem(item){const r=state(this),found=call(r,'find_handle',{handle:reference(r,item),all:true});return !!found&&call(r,'remove',{id:found.id});}
    }
    class Provider{
      get length(){const r=state(this);ensureArray(r);return snapshot(r).length;}
      getItemAt(position){const r=state(this);ensureArray(r);const s=snapshot(r);return r.references.get(s.handles[index(position)]);}
      getItemID(position){const r=state(this);ensureArray(r);return snapshot(r).ids[index(position)];}
      addItem(item){return this.addItemAt(this.length,item);}
      addItemAt(position,item){const r=state(this);ensureArray(r);position=index(position);return mutation(r,()=>{
        if(r.kind==='dataset')object(item);const before=snapshot(r);if(position<0||position>before.length)return false;
        call(r,'validate_keys',{keys:keys(r,item,call(r,'fields'))});
        if(r.kind==='dataset'){const event=emit(r,'addItem',{item,result:true});if(event?.result===false)return false;}
        const result=call(r,'add',{index:position,handle:reference(r,item),unique:r.kind==='dataset'});if(!result.added)return false;
        refresh(r);if(r.kind==='dataset')call(r,'locate',{id:result.id});modelChanged(r,'addItems',{firstItem:position,lastItem:position});return r.kind==='dataset'?true:undefined;
      });}
      removeItemAt(position){const r=state(this);ensureArray(r);position=index(position);return mutation(r,()=>{
        const before=snapshot(r),id=before.ids[position];if(id===undefined)return r.kind==='dataset'?false:undefined;
        const item=resolve(r,before.handles[position]);if(r.kind==='dataset'){const event=emit(r,'removeItem',{item,result:true});if(event?.result===false)return false;}
        const removed=call(r,'remove',{id});refresh(r);modelChanged(r,'removeItems',{firstItem:position,lastItem:position,removedIDs:[id]});return r.kind==='dataset'?removed:undefined;
      });}
      removeAll(){const r=state(this);ensureArray(r);return mutation(r,()=>removeBatch(r,call(r,'all').ids));}
      replaceItemAt(position,item){const r=state(this);ensureArray(r);position=index(position);return mutation(r,()=>{
        const current=snapshot(r),id=current.ids[position];if(id===undefined)return;if(r.kind==='dataset')object(item);
        const handle=reference(r,item);if(r.kind==='dataset'&&current.handles[position]!==handle&&call(r,'contains',{handle}))throw new Error('The replacement item already exists in this DataSet.');
        call(r,'validate_keys',{id,keys:keys(r,item,call(r,'fields'))});
        call(r,'remove',{id});call(r,'add',{index:position,handle:reference(r,item),unique:r.kind==='dataset'});refresh(r);modelChanged(r,'updateItems',{firstItem:position,lastItem:position});
      });}
      editField(position,name,newData){const r=state(this);ensureArray(r);position=index(position);name=field(name);return mutation(r,()=>{
        const s=snapshot(r),id=s.ids[position];if(id===undefined)return;const item=object(resolve(r,s.handles[position])),old=reference(r,item[name]),next=reference(r,newData),sortKeys=keys(r,item,call(r,'fields'));
        if(Object.hasOwn(sortKeys,name))sortKeys[name]=primitive(newData);call(r,'validate_keys',{id,field:name,keys:sortKeys});
        if(!Reflect.set(item,name,newData))throw new Error(`Cannot write data field '${name}'.`);
        call(r,'edit',{id,field:name,old,new:next});refresh(r);modelChanged(r,'updateField',{firstItem:position,lastItem:position,fieldName:name});
      });}
      getEditingData(position,name){const item=this.getItemAt(position);return item==null?undefined:item[field(name)];}
      sortItems(options=0){if(typeof options==='function')unsupported('DataProvider.sortItems custom comparator');return sortProvider(state(this),['$value'],options);}
      sortItemsBy(name,options=0){return sortProvider(state(this),[field(name)],options);}
    }
    function sortProvider(record,fields,flags){ensureArray(record);if(flags==='ASC')flags=0;else if(flags==='DESC')flags=2;if(typeof flags!=='number'||!Number.isInteger(flags)||flags&~23)throw new TypeError('DataProvider sort supports case/descending/unique/numeric flags; indexed-result mode is unsupported.');return mutation(record,()=>{
      refresh(record,fields);if(call(record,'sort_has',{name:'$provider'}))call(record,'sort_remove',{name:'$provider'});
      call(record,'sort_add',{name:'$provider',sort:{fields,flags,natural:false}});refresh(record);modelChanged(record,'sort');
    });}
    function makeProvider(record){const result=Object.create(Provider.prototype);models.set(result,record);options.EventDispatcher.initialize(result);record.provider=result;return result;}
    class DataSet extends Provider{
      constructor(){super();register(this,'dataset');}
      addItem(item){if(arguments.length===0)item=this.createItem();return this.addItemAt(this.length,item);}
      createItem(item={}){const r=state(this);object(item);emit(r,'newItem',{item});return item;}
      get dataProvider(){const r=state(this);return r.provider||makeProvider(r);}
      set dataProvider(value){this.items=value;}
      get items(){const r=state(this);return call(r,'all').handles.map(handle=>resolve(r,handle));}
      set items(values){if(!Array.isArray(values))throw new TypeError('DataSet.items requires an array of records.');const r=state(this);mutation(r,()=>{call(r,'load',{handles:values.map(value=>reference(r,object(value)))});r.packet=null;refresh(r);emit(r,'afterLoaded');modelChanged(r,'updateAll');});}
      get currentItem(){const r=state(this);return resolve(r,snapshot(r).current);}
      get selectedIndex(){return snapshot(state(this)).position;}
      set selectedIndex(value){move(state(this),undefined,'index',index(value));}
      get logChanges(){return snapshot(state(this)).logChanges;}
      set logChanges(value){if(typeof value!=='boolean')throw new TypeError('logChanges requires Boolean.');call(state(this),'flags',{logChanges:value});}
      get readOnly(){return snapshot(state(this)).readOnly;}
      set readOnly(value){if(typeof value!=='boolean')throw new TypeError('readOnly requires Boolean.');call(state(this),'flags',{readOnly:value});}
      get filtered(){return snapshot(state(this)).filtered;}
      set filtered(value){setFiltered(state(this),undefined,value);}
      get filterFunc(){const r=state(this);return r.filters.get(currentView(r));}
      set filterFunc(value){if(value!==null&&typeof value!=='function')throw new TypeError('filterFunc requires a function or null.');const r=state(this);r.filters.set(currentView(r),value);}
      get deltaPacket(){return state(this).packet;}
      set deltaPacket(value){unsupported('Assigning a remote/reconciliation delta packet');}
      getLength(){return snapshot(state(this)).total;}
      isEmpty(){return this.getLength()===0;}
      contains(item){return call(state(this),'contains',{handle:reference(state(this),item)});}
      clear(){const r=state(this);mutation(r,()=>removeBatch(r,snapshot(r).ids));}
      removeItem(item){const r=state(this),s=snapshot(r);if(arguments.length===0)return this.removeItemAt(s.position);const found=call(r,'find_handle',{handle:reference(r,item),all:true});if(!found)return false;return mutation(r,()=>{const event=emit(r,'removeItem',{item,result:true});if(event?.result===false)return false;const removed=call(r,'remove',{id:found.id});refresh(r);modelChanged(r,'removeItems',{removedIDs:[found.id]});return removed;});}
      changesPending(){return snapshot(state(this)).pending;}
      disableEvents(){call(state(this),'disable');}
      enableEvents(){const r=state(this),before=snapshot(r);const after=call(r,'enable');if(before.disabled&&after.disabled===0)modelChanged(r,'updateAll');}
      first(){move(state(this),undefined,'first');}last(){move(state(this),undefined,'last');}
      next(){this.skip(1);}previous(){this.skip(-1);}skip(offset){move(state(this),undefined,'skip',index(offset));}
      hasNext(){return snapshot(state(this)).hasNext;}
      hasPrevious(){return snapshot(state(this)).hasPrevious;}
      getItemId(position){const r=state(this),s=snapshot(r),id=s.ids[arguments.length?index(position):s.position];return id===undefined?null:String(id);}
      locateById(id){return locate(state(this),undefined,id);}
      addSort(name,fields,flags=0){addSort(state(this),undefined,name,fields,flags);}
      hasSort(name){return call(state(this),'sort_has',{name});}
      useSort(name,order){if(order!==undefined)unsupported('useSort order override; create a sort with the requested order');const r=state(this);refresh(r);call(r,'sort_use',{name});modelChanged(r,'sort');}
      removeSort(name){const r=state(this);call(r,'sort_remove',{name});modelChanged(r,'sort');}
      find(values){return find(state(this),undefined,values,false);}findFirst(values){return this.find(values);}findLast(values){return find(state(this),undefined,values,true);}
      setRange(start,end){const r=state(this);call(r,'range',{start,end});modelChanged(r,'filterModel');}
      removeRange(){const r=state(this);call(r,'range_clear');modelChanged(r,'filterModel');}
      getIterator(){const r=state(this);return makeView(r,currentView(r));}
      setIterator(value){const r=state(this),iterator=iterators.get(value);if(iterator?.kind!=='view'||iterator.record!==r)throw new TypeError('setIterator requires an iterator cloned from this DataSet.');call(r,'set_view',{view:iterator.id});modelChanged(r,'updateAll');}
      applyUpdates(){const r=state(this),result=call(r,'apply');if(result===null)return;r.packet=makePacket(r,result);emit(r,'deltaPacketChanged');}
      get schema(){unsupported('DataSet.schema');}set schema(value){unsupported('DataSet.schema');}
      get properties(){unsupported('DataSet.properties schema metadata');}
      get itemClassName(){unsupported('DataSet.itemClassName');}set itemClassName(value){unsupported('DataSet.itemClassName');}
      loadFromSharedObj(){unsupported('DataSet.loadFromSharedObj');}saveToSharedObj(){unsupported('DataSet.saveToSharedObj');}
    }
    function move(record,view,mode,value){const result=call(record,'move',{...(view===undefined?{}:{view}),mode,...(value===undefined?{}:{value})});if(result.scrolled&& (view===undefined||view===result.active))emit(record,'iteratorScrolled',{scrolled:result.scrolled});}
    function locate(record,view,value){const id=typeof value==='string'&&/^\d+$/.test(value)?Number(value):value;index(id);const before=snapshot(record,view),found=call(record,'locate',{...(view===undefined?{}:{view}),id});const after=snapshot(record,view);if(found&&before.position!==after.position&&(view===undefined||view===after.active))emit(record,'iteratorScrolled',{scrolled:after.position-before.position});return found;}
    function find(record,view,values,last){if(!Array.isArray(values))throw new TypeError('DataSet search requires an array of primitive sort-key values.');refresh(record);const before=snapshot(record,view),found=call(record,'find',{...(view===undefined?{}:{view}),values:values.map(primitive),last});const after=snapshot(record,view);if(found&&before.position!==after.position&&(view===undefined||view===after.active))emit(record,'iteratorScrolled',{scrolled:after.position-before.position});return found;}
    function addSort(record,view,name,fields,flags){if(typeof name!=='string'||!Array.isArray(fields))throw new TypeError('addSort requires a name and array of field names.');fields=fields.map(field);refresh(record,fields);call(record,'sort_add',{...(view===undefined?{}:{view}),name,sort:{fields,flags,natural:true}});modelChanged(record,'sort');}
    function setFiltered(record,view,value){if(typeof value!=='boolean')throw new TypeError('filtered requires Boolean.');view=view===undefined?currentView(record):view;const all=call(record,'all'),filter=record.filters.get(view),ids=[];if(value)for(let i=0;i<all.ids.length;i++){if(!filter||Reflect.apply(filter,record.target,[resolve(record,all.handles[i])]))ids.push(all.ids[i]);active(record);}call(record,'filter',{view,ids:value?ids:null});modelChanged(record,'filterModel');}
    class DataSetIterator{
      constructor(){unsupported('Direct DataSetIterator construction; use DataSet.getIterator()');}
      get currentItem(){const i=viewState(this);return resolve(i.record,snapshot(i.record,i.id).current);}
      get length(){const i=viewState(this);return snapshot(i.record,i.id).length;}
      get selectedIndex(){const i=viewState(this);return snapshot(i.record,i.id).position;}
      set selectedIndex(value){const i=viewState(this);move(i.record,i.id,'index',index(value));}
      first(){const i=viewState(this);move(i.record,i.id,'first');}last(){const i=viewState(this);move(i.record,i.id,'last');}
      next(){this.skip(1);}previous(){this.skip(-1);}skip(value){const i=viewState(this);move(i.record,i.id,'skip',index(value));}
      hasNext(){const i=viewState(this);return snapshot(i.record,i.id).hasNext;}hasPrevious(){const i=viewState(this);return snapshot(i.record,i.id).hasPrevious;}
      find(values){const i=viewState(this);return find(i.record,i.id,values,false);}findFirst(values){return this.find(values);}findLast(values){const i=viewState(this);return find(i.record,i.id,values,true);}
      locateById(id){const i=viewState(this);return locate(i.record,i.id,id);}
      getItemId(position){const i=viewState(this),s=snapshot(i.record,i.id),id=s.ids[arguments.length?index(position):s.position];return id===undefined?null:String(id);}
      get filtered(){const i=viewState(this);return snapshot(i.record,i.id).filtered;}set filtered(value){const i=viewState(this);setFiltered(i.record,i.id,value);}
      get filterFunc(){const i=viewState(this);return i.record.filters.get(i.id);}set filterFunc(value){if(value!==null&&typeof value!=='function')throw new TypeError('filterFunc requires function or null.');const i=viewState(this);i.record.filters.set(i.id,value);}
    }
    function viewState(value){const i=iterators.get(value);if(i?.kind!=='view')throw new TypeError('Expected a DataSet iterator.');active(i.record);return i;}
    function makeView(record,view){const result=Object.create(DataSetIterator.prototype),id=call(record,'clone_view',{view});record.filters.set(id,record.filters.get(view));iterators.set(result,{record,kind:'view',id});return result;}
    function makePacket(record,result){
      const packet={},timestamp=Date.now(),entries=[];packets.add(packet);
      for(const change of result.changes){
        const delta={},fields=[];deltas.add(delta);
        for(const changeField of Object.values(change.fields)){
          const item={};deltaItems.add(item);const read=callback=>({get(){active(record);return callback();}});Object.defineProperties(item,{kind:read(()=>0),name:read(()=>changeField.name),oldValue:read(()=>resolve(record,changeField.old)),newValue:read(()=>resolve(record,changeField.new)),delta:read(()=>delta),message:read(()=>''),curValue:read(()=>undefined),argList:read(()=>undefined)});fields.push(Object.freeze(item));
        }
        Object.assign(delta,{
          getChangeList(){active(record);return fields.slice();},getDeltaPacket(){active(record);return packet;},getId(){active(record);return String(change.id);},
          getItemByName(name){active(record);return fields.find(item=>item.name===name)||null;},getMessage(){active(record);return '';},
          getOperation(){active(record);return change.operation;},getSource(){active(record);return resolve(record,change.handle);},
          addDeltaItem(){unsupported('Editing a generated local delta');}
        });entries.push(Object.freeze(delta));
      }
      Object.assign(packet,{
        getIterator(){active(record);return makeIterator(record,entries.map(value=>reference(record,value)));},getSource(){active(record);return record.target;},
        getTimestamp(){active(record);return new Date(timestamp);},getTransactionId(){active(record);return String(result.transaction);},
        getConfigInfo(){active(record);return Object.freeze({profile:'NeoFlash local AS2 delta',remoteResolver:false});},logChanges(){active(record);return false;}
      });return Object.freeze(packet);
    }
    function interfaceType(name,predicate){function Interface(){unsupported(`${name} is an interface; use its concrete implementation`);}Interface.__nfIs=predicate;Interface.__nfInitialize=()=>unsupported(name+' native subclassing');return Interface;}
    const Collection=interfaceType('Collection',value=>models.get(value)?.kind==='collection'),Iterator=interfaceType('Iterator',value=>iterators.get(value)?.kind==='snapshot');
    const DataProvider=interfaceType('DataProvider',value=>models.has(value)&&models.get(value).kind!=='collection');
    DataProvider.initialize=function(array){if(!Array.isArray(array))throw new TypeError('DataProvider.initialize requires an array.');if(models.has(array))return array;const record=register(array,'provider');record.array=array;call(record,'load',{handles:array.map(value=>reference(record,value))});for(const key of Object.getOwnPropertyNames(Provider.prototype)){if(key==='constructor'||key==='length')continue;Object.defineProperty(array,key,Object.getOwnPropertyDescriptor(Provider.prototype,key));}return array;};
    const DeltaPacket=interfaceType('DeltaPacket',value=>packets.has(value)),Delta=interfaceType('Delta',value=>deltas.has(value)),DeltaItem=interfaceType('DeltaItem',value=>deltaItems.has(value));
    for(const [Ctor,predicate]of [[CollectionImpl,value=>models.get(value)?.kind==='collection'],[IteratorImpl,value=>iterators.get(value)?.kind==='snapshot'],[DataSet,value=>models.get(value)?.kind==='dataset'],[DataSetIterator,value=>iterators.get(value)?.kind==='view']]){Ctor.__nfIs=predicate;Ctor.__nfInitialize=()=>unsupported(Ctor.name+' native subclassing');}
    Object.assign(DataSetIterator,{Ascending:0,Descending:2,Unique:4,CaseInsensitive:1});
    const builtins=Object.freeze({
      'mx.utils.Collection':Collection,'mx.utils.CollectionImpl':CollectionImpl,'mx.utils.Iterator':Iterator,'mx.utils.IteratorImpl':IteratorImpl,
      'mx.controls.listclasses.DataProvider':DataProvider,'mx.data.components.DataSet':DataSet,'mx.data.components.datasetclasses.DataSetIterator':DataSetIterator,
      'mx.data.to.ValueListIterator':DataSetIterator,'mx.data.components.datasetclasses.DeltaPacket':DeltaPacket,'mx.data.components.datasetclasses.Delta':Delta,
      'mx.data.components.datasetclasses.DeltaItem':DeltaItem,'mx.data.components.datasetclasses.DeltaPacketConsts':Object.freeze({Added:0,Removed:1,Modified:2})
    });
    function reset(){if(disposed)return;for(const record of records){record.core.free();record.references.clear();record.primitives.clear();record.filters.clear();}records.clear();generation++;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({builtins,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Data=api;
})(globalThis);
