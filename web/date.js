/* Date object storage and browser timezone facilities; Gregorian arithmetic is Rust. */
(function(global){
  'use strict';
  const NativeDate=Date, apply=Reflect.apply;
  const methods=Object.freeze(Object.fromEntries(Object.getOwnPropertyNames(Date.prototype).map(name=>[name,Date.prototype[name]]).filter(([,value])=>typeof value==='function')));
  function create(wasm) {
    if(typeof wasm?.as_date_get!=='function')throw new Error('Rust Date bindings are missing; rebuild and re-export this project.');
    const timeOf=value=>apply(methods.getTime,value,[]);
    const offsetAt=value=>apply(methods.getTimezoneOffset,value,[]);
    const formatterStates=new WeakMap(), localeProfiles=new Map();
    let stringVector=()=>{throw new Error('DateTimeFormatter requires the publication Vector.<String> binding.');};
    const localeOptions={calendar:'gregory',numberingSystem:'latn',timeZone:'UTC'};
    const dateAt=parts=>new NativeDate(wasm.as_date_clip(wasm.as_date_construct(parts)));
    const sample=dateAt([2006,10,22,13,45,56]);
    const styles=['none','short','medium','long'];
    const nameStyles=['long','short','narrow'];
    function localeProfile(requested) {
      if(typeof requested!=='string'||requested.length>256)throw new TypeError('DateTimeFormatter locale requires a string of at most 256 characters.');
      if(localeProfiles.has(requested))return localeProfiles.get(requested);
      const locale=requested==='i-default'?undefined:requested;
      const resolved=new Intl.DateTimeFormat(locale,localeOptions).resolvedOptions();
      const actual=resolved.locale;
      const patterns={};
      for(const dateStyle of styles)for(const timeStyle of styles){
        patterns[dateStyle+'/'+timeStyle]=dateStyle==='none'&&timeStyle==='none'?[]:new Intl.DateTimeFormat(actual,{...localeOptions,...(dateStyle==='none'?{}:{dateStyle}),...(timeStyle==='none'?{}:{timeStyle})}).formatToParts(sample);
      }
      function names(field,count,context){return nameStyles.map(style=>{
        const options={...localeOptions,[field]:style,...(context==='format'?{day:'numeric'}:{})};
        const formatter=new Intl.DateTimeFormat(actual,options);
        return Array.from({length:count},(_,i)=>formatter.formatToParts(dateAt(field==='month'?[2023,i,1,12]:[2023,0,i+1,12])).find(part=>part.type===field)?.value||'');
      });}
      const dayPeriod=new Intl.DateTimeFormat(actual,{...localeOptions,hour:'numeric',hour12:true});
      const intlLocale=new Intl.Locale(actual);
      const week=typeof intlLocale.getWeekInfo==='function'?intlLocale.getWeekInfo():intlLocale.weekInfo;
      if(!week)throw new Error('DateTimeFormatter requires browser Intl.Locale week information.');
      const profile={requested,actual,fallback:locale!==undefined&&Intl.DateTimeFormat.supportedLocalesOf([locale]).length===0,
        first_weekday:week.firstDay%7,hour_cycle:new Intl.DateTimeFormat(actual,{...localeOptions,hour:'numeric'}).resolvedOptions().hourCycle,
        patterns,month_standalone:names('month',12,'standalone'),month_format:names('month',12,'format'),weekdays:names('weekday',7,'standalone'),
        day_periods:[0,13].map(hour=>dayPeriod.formatToParts(dateAt([2023,0,1,hour])).find(part=>part.type==='dayPeriod')?.value||'')};
      const json=JSON.stringify(profile);localeProfiles.set(requested,json);return json;
    }
    function formatter(receiver){const state=formatterStates.get(receiver);if(!state)throw new TypeError('DateTimeFormatter operation requires its native instance.');return state;}
    function invoke(receiver,name,...args){try{return formatter(receiver)[name](...args);}catch(error){throw new TypeError(String(error));}}
    function formatted(receiver,date,utc){
      let time;try{time=timeOf(date);}catch(_){throw new TypeError('DateTimeFormatter requires a Date.');}
      const state=formatter(receiver), info=JSON.parse(state.snapshot());
      let short='UTC',long='Coordinated Universal Time';
      if(Number.isFinite(time)){
        const names={...localeOptions,...(utc?{timeZone:'UTC'}:{timeZone:undefined})};
        short=new Intl.DateTimeFormat(info.actualLocaleIDName,{...names,timeZoneName:'short'}).formatToParts(date).find(part=>part.type==='timeZoneName')?.value||'';
        long=new Intl.DateTimeFormat(info.actualLocaleIDName,{...names,timeZoneName:'long'}).formatToParts(date).find(part=>part.type==='timeZoneName')?.value||'';
      }
      return invoke(receiver,'format',time,utc?0:offsetAt(date),short,long);
    }
    class DateTimeFormatter {
      constructor(locale,dateStyle='long',timeStyle='long'){
        if(new.target!==DateTimeFormatter)throw new TypeError('DateTimeFormatter is final; native subclass construction is unsupported.');
        try{formatterStates.set(this,new wasm.DateFormatterState(localeProfile(locale),String(dateStyle),String(timeStyle)));}catch(error){throw new TypeError(String(error));}
        Object.preventExtensions(this);
      }
      format(date){return formatted(this,date,false);}
      formatUTC(date){return formatted(this,date,true);}
      getDateStyle(){invoke(this,'note_read');return JSON.parse(formatter(this).snapshot()).dateStyle;}
      getTimeStyle(){invoke(this,'note_read');return JSON.parse(formatter(this).snapshot()).timeStyle;}
      getDateTimePattern(){invoke(this,'note_read');return JSON.parse(formatter(this).snapshot()).pattern;}
      getFirstWeekday(){return invoke(this,'first_weekday');}
      getMonthNames(style='full',context='standalone'){return stringVector(JSON.parse(invoke(this,'names',true,String(style),String(context))));}
      getWeekdayNames(style='full',context='standalone'){return stringVector(JSON.parse(invoke(this,'names',false,String(style),String(context))));}
      setDateTimePattern(pattern){invoke(this,'set_pattern',String(pattern));}
      setDateTimeStyles(dateStyle,timeStyle){invoke(this,'set_styles',String(dateStyle),String(timeStyle));}
      static getAvailableLocaleIDNames(){
        const common=['en-US','en-GB','ru-RU','de-DE','fr-FR','es-ES','it-IT','pt-BR','pt-PT','nl-NL','pl-PL','uk-UA','cs-CZ','sk-SK','sv-SE','da-DK','fi-FI','nb-NO','el-GR','tr-TR','he-IL','ar-SA','hi-IN','th-TH','vi-VN','id-ID','ja-JP','ko-KR','zh-CN','zh-TW'];
        return stringVector(Intl.DateTimeFormat.supportedLocalesOf(common));
      }
    }
    for(const name of ['actualLocaleIDName','requestedLocaleIDName','lastOperationStatus'])Object.defineProperty(DateTimeFormatter.prototype,name,{get(){return JSON.parse(formatter(this).snapshot())[name];}});
    Object.defineProperty(DateTimeFormatter,'__nfIs',{value:value=>formatterStates.has(value)});
    Object.defineProperty(DateTimeFormatter,'__nfInitialize',{value:()=>{throw new TypeError('DateTimeFormatter is final; native subclass initialization is unsupported.');}});
    function constants(name,values){const type=function(){throw new TypeError(name+' is a constants class; construction is unsupported.');};for(const [key,value] of Object.entries(values))Object.defineProperty(type,key,{value,enumerable:true});Object.defineProperty(type,'__nfIs',{value:()=>false});return Object.freeze(type);}
    const DateTimeStyle=constants('DateTimeStyle',{CUSTOM:'custom',LONG:'long',MEDIUM:'medium',NONE:'none',SHORT:'short'});
    const DateTimeNameStyle=constants('DateTimeNameStyle',{FULL:'full',LONG_ABBREVIATION:'longAbbreviation',SHORT_ABBREVIATION:'shortAbbreviation'});
    const DateTimeNameContext=constants('DateTimeNameContext',{FORMAT:'format',STANDALONE:'standalone'});
    function localInstant(wall) {
      if(!Number.isFinite(wall))return NaN;
      const fields=wasm.as_date_parts(wall);
      if(!Number.isFinite(fields[0]))return NaN;
      // Resolve already-normalized civil fields against the browser/OS timezone
      // database, including its policy for gaps and overlaps. No arithmetic here.
      const date=new NativeDate(0);
      apply(methods.setFullYear,date,Array.from(fields.slice(0,3)));
      apply(methods.setHours,date,Array.from(fields.slice(3,7)));
      return wasm.as_date_clip(timeOf(date));
    }
    return Object.freeze({
      DateTimeFormatter,DateTimeStyle,DateTimeNameStyle,DateTimeNameContext,
      setStringVectorFactory(factory){if(typeof factory!=='function')throw new TypeError('String Vector factory must be callable.');stringVector=factory;},
      create(args) {
        if(args.length===0)return new NativeDate();
        if(args.length===1)return new NativeDate(wasm.as_date_clip(args[0]));
        return new NativeDate(localInstant(wasm.as_date_construct(args)));
      },
      utc(args){return wasm.as_date_clip(wasm.as_date_construct(args));},
      get(receiver,name){return wasm.as_date_get(name,timeOf(receiver),offsetAt(receiver));},
      set(receiver,name,args){
        let result=wasm.as_date_set(name,timeOf(receiver),args,offsetAt(receiver));
        if(name!=='setTime'&&!name.startsWith('setUTC'))result=localInstant(result);
        apply(methods.setTime,receiver,[result]);
        return result;
      },
      utcString(receiver){return wasm.as_date_utc_string(timeOf(receiver));}
    });
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashDate=api;
})(globalThis);
