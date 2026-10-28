/* Value coercion and UTF-16 transport only; String/URI/XML-name algorithms are Rust.
 * Provenance and retained MIT notice: docs/RUNTIME-STRINGS-PROVENANCE.md. */
(function(global){
  'use strict';
  const LIMIT=1000000, toString=String, toNumber=Number;
  function units(value){if(value.length>LIMIT)throw new TypeError('String exceeds the supported string limit.');return Array.from({length:value.length},(_,i)=>value.charCodeAt(i));}
  function text(value){let output='';for(let i=0;i<value.length;i+=8192)output+=String.fromCharCode(...value.slice(i,i+8192));return output;}
  function primitive(value,operation){if(value!==null&&(typeof value==='object'||typeof value==='function'))throw new TypeError(operation+' requires a primitive pattern; native RegExp and custom pattern objects are unsupported.');return toString(value);}
  function create(options){
    if(typeof options?.execute!=='function')throw new TypeError('AS3 String requires the shared Rust WASM service. Re-export this project.');
    function send(request){
      let raw;try{raw=options.execute(JSON.stringify(request));}catch(error){const Type=/^(en|de)codeURI/.test(request.operation)?URIError:TypeError;throw new Type(toString(error));}
      const result=JSON.parse(raw);
      switch(result.kind){case 'string':return text(result.value);case 'strings':return result.value.map(text);case 'number':return toNumber(result.value);case 'boolean':return result.value;default:throw new TypeError('Invalid Rust AS3 String response.');}
    }
    function invoke(operation,receiver,args){
      if(args.length>LIMIT)throw new TypeError('String argument count exceeds the supported string limit.');
      const request={operation,receiver:units(receiver),strings:[],numbers:[],arg_count:args.length,null_pattern:false};
      const numeric=value=>toString(toNumber(value));
      switch(operation){
        case 'charAt':case 'charCodeAt':request.numbers.push(numeric(args.length?args[0]:0));break;
        case 'concat':for(const value of args)request.strings.push(units(toString(value)));break;
        case 'fromCharCode':for(const value of args)request.numbers.push(numeric(value));break;
        case 'indexOf':case 'lastIndexOf':
          request.null_pattern=args.length>0&&args[0]==null;
          request.strings.push(units(request.null_pattern?'':toString(args.length?args[0]:undefined)));
          if(args.length>1&&!(operation==='lastIndexOf'&&args[1]===undefined))request.numbers.push(numeric(args[1]));
          break;
        case 'localeCompare':request.strings.push(units(toString(args.length?args[0]:undefined)));break;
        case 'slice':case 'substring':case 'substr':
          if(args.length)request.numbers.push(numeric(args[0]));
          if(args.length>1)request.numbers.push(numeric(args[1]));
          break;
        case 'split':
          if(args.length>1&&args[1]!=null)request.numbers.push(numeric(args[1]));
          // Rust decides whether the delimiter conversion is observable.
          if(send({...request,operation:'splitEarly'}))return send(request);
          request.strings.push(units(primitive(args[0],'split')));break;
        case 'replace':{
          const callback=typeof args[1]==='function'?args[1]:null;
          // Preserve the existing documented replacement-before-pattern order.
          let replacement=callback?null:units(toString(args[1]));
          const pattern=primitive(args[0],'replace');request.strings.push(units(pattern));
          if(callback){const at=send({...request,operation:'replaceIndex'});if(at<0)return receiver;replacement=units(toString(Reflect.apply(callback,undefined,[pattern,at,receiver])));}
          request.strings.push(replacement);break;
        }
        case 'escape':case 'unescape':case 'encodeURI':case 'encodeURIComponent':case 'decodeURI':case 'decodeURIComponent':
          if(args.length>1)throw new TypeError('AS3 '+operation+' supports at most one string argument.');
          request.receiver=units(args.length===0?'undefined':args[0]==null?'null':toString(args[0]));break;
        case 'escapeMultiByte':case 'unescapeMultiByte':
          if(args.length!==1)throw new TypeError('AS3 '+operation+' requires one string argument.');
          request.receiver=units(args[0]==null?'null':toString(args[0]));break;
        case 'isXMLName':
          if(args.length>1)throw new TypeError('AS3 isXMLName supports at most one argument.');
          request.null_pattern=args.length===0||args[0]==null;
          request.receiver=request.null_pattern?[]:units(toString(args[0]));break;
      }
      return send(request);
    }
    return Object.freeze({invoke,escapeMultiByte(value){return invoke('escapeMultiByte','',Array.from(arguments));},unescapeMultiByte(value){return invoke('unescapeMultiByte','',Array.from(arguments));}});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;global.NeoFlashAS3Strings=api;
})(globalThis);
