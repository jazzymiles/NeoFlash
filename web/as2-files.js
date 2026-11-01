/* AS2-only FileReference browser facilities. Rust validates filters/filenames,
 * assembles multipart bytes and owns listener ordering through AsBroadcaster.
 * Transport reuses URLLoader/ByteArray; no independent fetch implementation.
 */
(function(global){
  'use strict';
  const unsupported=name=>{throw new TypeError(name+' is unavailable in the AS2 browser file profile.');};
  function create(options={}){
    const {wasm,net,binary,AsBroadcaster}=options;
    if(!wasm?.as2_file_command||!wasm?.as2_file_multipart||!net?.URLLoader||!binary?.fromBytes||!AsBroadcaster?.initialize)throw new Error('AS2 file services require the current Rust, broadcaster and network runtime.');
    const records=new Set(),files=new WeakMap(),lists=new WeakMap(),document=options.document||global.document;
    let generation=1,disposed=false,picker=null;
    const gesture=()=>options.isUserGesture?options.isUserGesture():global.navigator?.userActivation?.isActive===true;
    function state(value,table){const r=table.get(value);if(disposed||!r||r.generation!==generation)throw new Error('This file object belongs to a disposed or previous publication.');return r;}
    function core(command){return JSON.parse(wasm.as2_file_command(JSON.stringify(command)));}
    function valid(r,token){return !disposed&&r.generation===generation&&(!token||r.operation===token);}
    function report(error){if(options.onError)options.onError(error);else console.error('NeoFlash FileReference:',error);}
    function emit(r,name,...args){if(!valid(r))return false;const epoch=generation;r.bus.broadcastMessage(name,r.receiver,...args);if(!disposed&&generation===epoch)options.afterEvent?.();return !disposed&&generation===epoch;}
    function make(receiver,table){if(disposed)throw new Error('File service is disposed.');if(records.size>=256)throw new RangeError('At most 256 file/list objects are supported per publication.');const bus={};AsBroadcaster.initialize(bus);const r={receiver,bus,generation,file:null,metadata:null,selection:[],postData:'',operation:null};records.add(r);table.set(receiver,r);return r;}
    function cleanupPicker(token){if(!token?.input)return;token.input.remove();token.input=null;if(picker===token)picker=null;}
    function cancel(r){const token=r.operation;if(!token)return;r.operation=null;cleanupPicker(token);token.loader?.close();token.body?.clear();token.body=null;token.writable?.abort?.().catch(()=>{});}
    function metadata(file){
      if(!Number.isSafeInteger(file.size)||file.size<0||file.size>8*1024*1024)throw new RangeError('Each selected file must be at most 8 MiB.');
      const name=core({op:'name',name:file.name});
      return {...name,size:file.size,modified:Number.isFinite(file.lastModified)?file.lastModified:null};
    }
    function filters(types){
      if(types===undefined||types===null)return '';
      if(!Array.isArray(types)||types.length>32)throw new TypeError('File browse filters require an array of at most 32 groups.');
      return core({op:'filters',types:types.map(item=>({extension:item?.extension,macType:item?.macType}))});
    }
    function browse(r,types,multiple){
      if(!gesture())throw new Error('File browsing requires an actual user click.');
      if(r.operation||picker)throw new Error('Finish or cancel the current file operation before browsing.');
      if(!document?.createElement||!document.body)throw new Error('Browser file selection is unavailable.');
      const accept=filters(types),input=document.createElement('input');input.type='file';input.multiple=multiple;input.accept=accept;input.style.display='none';input.setAttribute('aria-label',multiple?'Choose files for AS2 publication':'Choose file for AS2 publication');
      const token={input};picker=token;r.operation=token;
      input.addEventListener('cancel',()=>{if(!valid(r,token))return;r.operation=null;cleanupPicker(token);try{emit(r,'onCancel');}catch(error){report(error);}});
      input.addEventListener('change',()=>{
        if(!valid(r,token))return;
        const selected=Array.from(input.files||[]);r.operation=null;cleanupPicker(token);
        try{
          if(!selected.length){emit(r,'onCancel');return;}
          if(selected.length>(multiple?64:1)||records.size+(multiple?selected.length:0)>256)throw new RangeError('Select at most 64 files within the publication object budget.');
          const details=selected.map(metadata);if(details.reduce((total,item)=>total+item.size,0)>64*1024*1024)throw new RangeError('A file selection cannot exceed 64 MiB.');
          if(multiple)r.selection=selected.map((file,i)=>{const receiver=new FileReference(),item=state(receiver,files);item.file=file;item.metadata=details[i];return receiver;});
          else{r.file=selected[0];r.metadata=details[0];}
          emit(r,'onSelect');
        }catch(error){emit(r,'onIOError');report(error);}
      });
      document.body.appendChild(input);try{input.click();}catch(error){r.operation=null;cleanupPicker(token);throw error;}return true;
    }
    function url(value){if(typeof value!=='string'||!value||value.length>8192)throw new TypeError('File transfer requires a URL string of 1..8192 characters.');const parsed=new URL(value,global.location?.href);if(!['http:','https:'].includes(parsed.protocol)||parsed.username||parsed.password)throw new Error('File transfer requires browser HTTP(S), without embedded credentials.');return parsed.href;}
    function failed(r,token,error){
      if(!valid(r,token))return;r.operation=null;token.loader?.close();token.body?.clear();token.body=null;
      try{if(!token.httpError)emit(r,error?.name==='SecurityError'?'onSecurityError':'onIOError',...(error?.name==='SecurityError'?[String(error.message)]:[]));}catch(callbackError){report(callbackError);}
      console.warn('NeoFlash FileReference:',String(error?.message||error));
    }
    function listenLoader(r,token,download){
      const loader=token.loader;
      loader.addEventListener('httpStatus',event=>{if(!valid(r,token))return;if(event.status>=400){token.httpError=true;emit(r,'onHTTPError',event.status);}});
      loader.addEventListener('ioError',event=>failed(r,token,new Error(event.text)));
      if(download)loader.addEventListener('progress',event=>{if(valid(r,token))emit(r,'onProgress',event.bytesLoaded,event.bytesTotal);});
    }
    async function upload(r,target,field,testUpload){
      const token=r.operation,selected=r.file,postData=r.postData;
      try{
        if(!emit(r,'onOpen')||!valid(r,token))return;
        emit(r,'onProgress',0,selected.size);if(!valid(r,token))return;
        const bytes=new Uint8Array(await selected.arrayBuffer());if(!valid(r,token))return;
        const boundary='NeoFlash-'+global.crypto.randomUUID();
        const body=wasm.as2_file_multipart(JSON.stringify({boundary,filename:selected.name,field,content_type:selected.type||'application/octet-stream',post_data:postData}),bytes);
        token.body=binary.fromBytes(body);token.loader=new net.URLLoader();listenLoader(r,token,false);
        token.loader.addEventListener('complete',()=>{
          if(!valid(r,token))return;
          try{const response=token.loader.data;token.body.clear();token.body=null;
            if(!emit(r,'onProgress',selected.size,selected.size)||!valid(r,token))return;
            r.operation=null;const epoch=generation;
            if(emit(r,'onComplete')&&!disposed&&generation===epoch&&!r.operation)emit(r,'onUploadCompleteData',response);
          }catch(error){report(error);}
        });
        const request=new net.URLRequest(target);request.method='POST';request.contentType='multipart/form-data; boundary='+boundary;request.data=token.body;token.loader.load(request);
      }catch(error){failed(r,token,error);}
    }
    async function download(r,target,suggested){
      const token=r.operation;
      try{
        // Invoke before awaiting: this must stay in the initiating user gesture.
        const handle=await global.showSaveFilePicker({suggestedName:suggested});if(!valid(r,token))return;
        const name=core({op:'name',name:handle.name});r.metadata={...name,size:0,modified:null};
        if(!emit(r,'onSelect')||!valid(r,token))return;
        if(!emit(r,'onOpen')||!valid(r,token))return;
        token.loader=new net.URLLoader();token.loader.dataFormat='binary';listenLoader(r,token,true);
        token.loader.addEventListener('complete',()=>{void (async()=>{
          if(!valid(r,token))return;
          try{
            const bytes=binary.toBytes(token.loader.data);token.loader.data.clear();
            if(bytes.length>8*1024*1024)throw new RangeError('Downloaded files must be at most 8 MiB.');
            token.writable=await handle.createWritable();if(!valid(r,token)){await token.writable.abort();return;}
            await token.writable.write(bytes);if(!valid(r,token)){await token.writable.abort();return;}
            await token.writable.close();token.writable=null;if(!valid(r,token))return;
            const saved=await handle.getFile();if(!valid(r,token))return;
            r.file=saved;r.metadata=metadata(saved);r.operation=null;emit(r,'onComplete');
          }catch(error){failed(r,token,error);}
        })();});
        const request=new net.URLRequest(target);if(r.postData){request.method='POST';request.data=r.postData;}token.loader.load(request);
      }catch(error){if(error?.name==='AbortError'&&valid(r,token)){r.operation=null;emit(r,'onCancel');}else failed(r,token,error);}
    }
    class FileReference {
      constructor(){make(this,files);}
      addListener(listener){return state(this,files).bus.addListener(listener);}
      removeListener(listener){return state(this,files).bus.removeListener(listener);}
      browse(types){return browse(state(this,files),types,false);}
      cancel(){cancel(state(this,files));}
      upload(target,field='Filedata',testUpload=false){const r=state(this,files);if(!r.file)throw new Error('Select a file before calling upload.');if(r.operation)throw new Error('A file operation is already active.');if(typeof field!=='string'||!field||field.length>128)throw new TypeError('Upload data field name must contain 1..128 characters.');if(testUpload!==false)unsupported('Windows test-upload preflight');const destination=url(target);r.operation={};void upload(r,destination,field,testUpload);return true;}
      download(target,defaultFileName='download'){const r=state(this,files);if(!gesture())throw new Error('File download requires an actual user click.');if(r.operation)throw new Error('A file operation is already active.');if(typeof global.showSaveFilePicker!=='function')unsupported('File download: use a Chromium browser with the File System Access API');const destination=url(target),suggested=core({op:'name',name:defaultFileName}).name;r.operation={};void download(r,destination,suggested);return true;}
      get name(){return state(this,files).metadata?.name;}get size(){return state(this,files).metadata?.size;}get type(){return state(this,files).metadata?.type;}
      get modificationDate(){const value=state(this,files).metadata?.modified;return value===null||value===undefined?undefined:new Date(value);}
      get creationDate(){const meta=state(this,files).metadata,value=meta?.created??meta?.modified;return value===null||value===undefined?undefined:new Date(value);}
      get creator(){return state(this,files).metadata?.creator;}
      get postData(){return state(this,files).postData;}set postData(value){const r=state(this,files);if(typeof value!=='string'||value.length>64*1024)throw new TypeError('postData requires a URL-encoded string of at most 64 KiB.');r.postData=value;}
    }
    class FileReferenceList {
      constructor(){make(this,lists);}
      addListener(listener){return state(this,lists).bus.addListener(listener);}
      removeListener(listener){return state(this,lists).bus.removeListener(listener);}
      browse(types){return browse(state(this,lists),types,true);}
      get fileList(){return state(this,lists).selection.slice();}
    }
    for(const [Type,table]of [[FileReference,files],[FileReferenceList,lists]])Object.defineProperties(Type,{__nfIs:{value:value=>table.has(value)},__nfInitialize:{value:()=>unsupported(Type.name+' native subclassing')},[Symbol.hasInstance]:{value:value=>table.has(value)}});
    function reset(){if(disposed)return;for(const r of records)cancel(r);records.clear();generation++;picker=null;}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({FileReference,FileReferenceList,reset,dispose});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashAS2Files=api;
})(globalThis);
