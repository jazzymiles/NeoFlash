/* Original bounded XML tree service; parsing is exclusively the shared Rust
 * parse_xml_json export. No DOMParser, regexp parser or external entity resolver.
 * Contracts: AIR XML/XMLList; Adobe AS2 XML/XMLNode reference (open-flash mirror).
 * AS2 DOM insertion moves nodes. AS3 insertion copies XML/XMLList inputs, and
 * primitive input becomes text: an explicit profile, not historical SWF-version
 * insertion/aliasing conformance. QName mutation, E4X operators and native XML
 * subclassing are pending. Namespace names/declarations survive serialization.
 * Per publication: 50000 allocated nodes/declarations, 8MiB owned text/name data,
 * 1M list references, depth128; parser input1MiB, serialized output4MiB.
 * Detached nodes count until Restart, so held wrappers retain coherent identity.
 */
(function(global){
  'use strict';
  const encoder=new TextEncoder(),MAX_NODES=50000,MAX_BYTES=8*1024*1024,MAX_OUTPUT=4*1024*1024,MAX_REFS=1000000;
  const defaults=Object.freeze({ignoreComments:true,ignoreProcessingInstructions:true,ignoreWhitespace:true,prettyPrinting:true,prettyIndent:2});
  const unsupported=name=>{throw new TypeError(`XML ${name} is not supported in this bounded DOM/XMLList profile.`);};
  const escapeText=value=>value.replace(/[&<>\r]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','\r':'&#xD;'}[c]));
  const escapeAttribute=value=>value.replace(/[&<"\r\n\t]/g,c=>({'&':'&amp;','<':'&lt;','"':'&quot;','\r':'&#xD;','\n':'&#xA;','\t':'&#x9;'}[c]));
  function text(value){
    const result=String(value);if(encoder.encode(result).length>MAX_OUTPUT)throw new RangeError('XML text exceeds 4 MiB.');
    for(const c of result){const n=c.codePointAt(0);if(!(n===9||n===10||n===13||(n>=32&&n<=0xd7ff)||(n>=0xe000&&n<=0xfffd)||(n>=0x10000&&n<=0x10ffff)))throw new TypeError('XML text contains an invalid XML 1.0 character.');}
    return result;
  }
  function node(kind,name=null,value=''){return {kind,name,value,attributes:[],declarations:[],children:[],parent:null};}
  function create({parse,net,afterCallback,onError}={}){
    if(typeof parse!=='function')throw new TypeError('XML requires the shared Rust parse_xml_json service. Re-export this project.');
    let generation=1,disposed=false,nodeCount=0,stringBytes=0,listRefs=0,settings={...defaults};
    const nodes=new Set(),records=new WeakMap(),xmlWrappers=new WeakMap(),legacyWrappers=new WeakMap(),attributeWrappers=new WeakMap(),loads=new Set();
    function active(){if(disposed)throw new Error('XML service has been disposed.');}
    function state(value){active();const r=records.get(value);if(!r)throw new TypeError('Expected XML from this publication.');if(r.generation!==generation)throw new Error('XML belongs to a previous publication session.');return r;}
    const is=value=>records.has(value);
    const kind=value=>records.get(value)?.kind||null;
    function units(n){return 1+n.declarations.length;}
    function bytes(n){return encoder.encode(n.value+(n.declaration||'')+(n.name?Object.values(n.name).join(''):'')+n.declarations.map(d=>d.prefix+d.uri).join('')).length;}
    function walk(roots){
      const found=[],seen=new Set(),stack=roots.map(n=>[n,n.kind==='element'?1:0]);
      while(stack.length){const[n,depth]=stack.pop();if(depth>128)throw new RangeError('XML tree exceeds 128 levels.');if(seen.has(n))throw new TypeError('XML tree contains a cycle or duplicate child.');seen.add(n);found.push(n);if(found.length>MAX_NODES)throw new RangeError('XML tree exceeds 50000 nodes.');for(const a of n.attributes)stack.push([a,depth]);for(const child of n.children)stack.push([child,depth+(child.kind==='element'?1:0)]);}
      return found;
    }
    function commit(roots,extraBytes=0){
      const added=walk(roots).filter(n=>!nodes.has(n));let count=0,size=0;
      for(const n of added){count+=units(n);size+=bytes(n);}
      if(nodeCount+count>MAX_NODES||stringBytes+size+extraBytes>MAX_BYTES)throw new RangeError('XML publication allocation exceeds 50000 nodes/declarations or 8 MiB of text. Restart to release detached trees.');
      for(const n of added)nodes.add(n);nodeCount+=count;stringBytes+=size;return roots;
    }
    function change(n,property,value){const old=bytes(n),saved=n[property];n[property]=value;const next=bytes(n);n[property]=saved;if(stringBytes-old+next>MAX_BYTES)throw new RangeError('XML publication text exceeds 8 MiB.');n[property]=value;stringBytes+=next-old;}
    function rawParse(source){
      source=text(source);if(encoder.encode(source).length>1024*1024)throw new RangeError('XML parser input exceeds 1 MiB.');
      const result=JSON.parse(parse(source));if(result.error){const e=new SyntaxError(`XML ${result.error.kind} at UTF-8 byte ${result.error.offset}: ${result.error.message}`);e.xmlCategory=result.error.kind;throw e;}return result.ok;
    }
    function parsed(source,policy){
      const ir=rawParse(source),map=new Map();
      function build(index){
        if(map.has(index))return map.get(index);const p=ir.nodes[index];
        if((p.kind==='comment'&&policy.ignoreComments)||(p.kind==='pi'&&policy.ignoreProcessingInstructions))return null;
        let value=p.value;
        if(p.kind==='text'){
          if(policy.trim)value=value.replace(/^[\t\r\n ]+|[\t\r\n ]+$/g,'');
          if(policy.ignoreWhitespace&&/^[\t\r\n ]*$/.test(value))return null;
        }
        const n=node(p.kind,p.name?{...p.name}:null,value);map.set(index,n);n.declarations=p.declarations.map(d=>({...d}));
        n.attributes=p.attributes.map(a=>{const child=node('attribute',{...a.name},a.value);child.parent=n;return child;});
        n.children=p.children.map(build).filter(Boolean);for(const child of n.children)child.parent=n;return n;
      }
      return {roots:ir.roots.map(build).filter(Boolean),declaration:ir.declaration};
    }
    function clone(n,deep=true){
      const copy=node(n.kind,n.name?{...n.name}:null,n.value);copy.declarations=n.declarations.map(d=>({...d}));copy.declaration=n.declaration;
      copy.attributes=n.attributes.map(a=>{const child=clone(a);child.parent=copy;return child;});
      if(deep)copy.children=n.children.map(c=>{const child=clone(c);child.parent=copy;return child;});return copy;
    }
    function name(value){
      if(typeof value!=='string'||value.includes(':')||value==='xmlns')unsupported('namespace/QName name mutation; use an unqualified name');
      const ir=rawParse('<'+value+'/>');if(ir.roots.length!==1||ir.nodes[ir.roots[0]].name?.qualified!==value)throw new TypeError('Invalid XML element/attribute name.');return {...ir.nodes[ir.roots[0]].name};
    }
    function depth(n){let result=0;for(let p=n;p;p=p.parent)if(p.kind==='element')result++;return result;}
    function insertionDepth(parent,children){const base=depth(parent);let deepest=0;const stack=children.map(n=>[n,n.kind==='element'?1:0]);while(stack.length){const[n,d]=stack.pop();deepest=Math.max(deepest,d);for(const c of n.children)stack.push([c,d+(c.kind==='element'?1:0)]);}if(base+deepest>128)throw new RangeError('XML insertion exceeds 128 element levels.');}
    function detach(n){if(!n.parent)return;const p=n.parent,list=n.kind==='attribute'?p.attributes:p.children;list.splice(list.indexOf(n),1);n.parent=null;}
    function descendants(n){const result=[],stack=[...n.children].reverse();while(stack.length){const child=stack.pop();result.push(child);if(result.length>MAX_NODES)throw new RangeError('XML traversal exceeds its node limit.');stack.push(...[...child.children].reverse());}return result;}
    function serialize(roots,pretty=settings.prettyPrinting,declaration=null){
      const chunks=[];let size=0,visits=0;
      const emit=s=>{size+=encoder.encode(s).length;if(size>MAX_OUTPUT)throw new RangeError('XML serialization exceeds 4 MiB.');chunks.push(s);};
      function render(n,bindings,level){
        if(++visits>MAX_NODES||level>128)throw new RangeError('XML serialization exceeds its node/depth limit.');
        if(n.kind==='document'){for(const c of n.children)render(c,bindings,level);return;}
        if(n.kind==='text'){emit(escapeText(n.value));return;}if(n.kind==='attribute'){emit(escapeAttribute(n.value));return;}
        if(n.kind==='comment'){emit('<!--'+n.value+'-->');return;}if(n.kind==='pi'){emit('<?'+n.name.qualified+(n.value?' '+n.value:'')+'?>');return;}
        const local=new Map(bindings),decl=new Map(n.declarations.map(d=>[d.prefix,d.uri]));
        for(const[prefix,uri]of decl)local.set(prefix,uri);
        const ensure=q=>{if(!q||q.prefix==='xml')return;if(local.get(q.prefix)!==q.uri){decl.set(q.prefix,q.uri);local.set(q.prefix,q.uri);}};
        ensure(n.name);for(const a of n.attributes)if(a.name.prefix)ensure(a.name);
        emit('<'+n.name.qualified);for(const[prefix,uri]of decl)emit(' xmlns'+(prefix?':'+prefix:'')+'="'+escapeAttribute(uri)+'"');
        for(const a of n.attributes)emit(' '+a.name.qualified+'="'+escapeAttribute(a.value)+'"');
        if(!n.children.length){emit('/>');return;}emit('>');
        // Avoid injecting formatting whitespace into mixed text/element content.
        const indent=pretty&&n.children.every(c=>c.kind!=='text');
        for(const c of n.children){if(indent)emit('\n'+' '.repeat(settings.prettyIndent*(level+1)));render(c,local,level+1);}
        if(indent)emit('\n'+' '.repeat(settings.prettyIndent*level));emit('</'+n.name.qualified+'>');
      }
      if(declaration)emit(declaration);
      const bindings=new Map([['',''],['xml','http://www.w3.org/XML/1998/namespace']]);
      roots.forEach((n,i)=>{if(i&&pretty)emit('\n');render(n,bindings,0);});return chunks.join('');
    }
    const simple=n=>n.kind==='text'||n.kind==='attribute'||n.kind==='element'&&!n.children.some(c=>c.kind==='element');
    const simpleText=n=>n.kind==='element'?n.children.filter(c=>c.kind==='text').map(c=>c.value).join(''):n.value;
    function stringOf(value){const r=state(value),list=r.nodes;if(list.length===1)return simple(list[0])?simpleText(list[0]):serialize(list);if(!list.length)return '';if(!list.some(n=>n.kind==='element'))return list.filter(simple).map(simpleText).join('');return serialize(list);}
    function single(value){const r=state(value);if(r.nodes.length!==1)throw new TypeError('This XML operation requires exactly one XMLList item.');return r.nodes[0];}
    function checkList(count){if(listRefs+count>MAX_REFS)throw new RangeError('XMLList snapshots exceed 1000000 retained references per publication.');}
    function list(items){active();checkList(items.length);listRefs+=items.length;return facade('list',items.slice());}
    function wrapped(n){active();if(!xmlWrappers.has(n))xmlWrappers.set(n,facade('xml',[n]));return xmlWrappers.get(n);}
    function selector(value='*'){if(typeof value!=='string'||value.includes(':')||value.startsWith('@'))unsupported('QName/namespace selectors; use an empty-namespace name or *');return value;}
    function matches(n,key){return key==='*'||(n.name&&n.name.uri===''&&n.name.local===key);}
    function selected(value,operation,arg){
      const source=state(value).nodes,result=[];
      const index=operation==='child'&&((typeof arg==='number'&&Number.isInteger(arg)&&arg>=0)||(typeof arg==='string'&&/^(0|[1-9]\d*)$/.test(arg)))?Number(arg):null;
      const key=index!==null||operation==='children'||operation==='text'||operation==='comments'?'*':selector(arg);
      for(const n of source){
        if(index!==null){if(n.children[index])result.push(n.children[index]);continue;}
        const candidates=operation==='attributes'||operation==='attribute'?n.attributes:operation==='descendants'?descendants(n):n.children;
        for(const child of candidates){
          if(operation==='elements'&&child.kind!=='element')continue;
          if(operation==='text'&&child.kind!=='text')continue;if(operation==='comments'&&child.kind!=='comment')continue;
          if(operation==='processingInstructions'&&child.kind!=='pi')continue;
          if(matches(child,key))result.push(child);
        }
      }
      return list(result);
    }
    function insert(value,input,mode){
      const parent=single(value);if(parent.kind!=='element')throw new TypeError('XML child insertion requires an element.');
      let children;
      if(is(input)){
        const r=state(input);if(r.kind==='legacy')unsupported('mixing legacy DOM nodes into AS3 XML insertion');
        for(const n of r.nodes){for(let ancestor=parent;ancestor;ancestor=ancestor.parent)if(ancestor===n)throw new TypeError('Cannot insert an XML ancestor into itself.');}
        children=r.nodes.map(n=>n.kind==='attribute'?node('text',null,n.value):clone(n));
      }else children=[node('text',null,text(input))];
      insertionDepth(parent,children);commit(children);
      if(mode==='replace'){for(const old of parent.children)old.parent=null;parent.children=[];}
      for(const child of children)child.parent=parent;
      parent.children=mode==='prepend'?[...children,...parent.children]:[...parent.children,...children];return value;
    }
    function normalize(value){
      const plans=[],seen=new Set();let extra=0;
      for(const root of state(value).nodes)for(const n of [root,...descendants(root)]){
        if(n.kind!=='element'||seen.has(n))continue;seen.add(n);const children=[],updates=[];
        for(let i=0;i<n.children.length;i++){
          const child=n.children[i];if(child.kind!=='text'){children.push(child);continue;}
          let content=child.value;while(i+1<n.children.length&&n.children[i+1].kind==='text')content+=n.children[++i].value;
          if(content){children.push(child);updates.push([child,content]);extra+=encoder.encode(content).length-encoder.encode(child.value).length;}
        }
        plans.push([n,children,updates]);
      }
      if(stringBytes+extra>MAX_BYTES)throw new RangeError('XML normalized text exceeds publication bounds.');
      for(const[n,children,updates]of plans){for(const old of n.children)if(!children.includes(old))old.parent=null;n.children=children;for(const[child,value]of updates)change(child,'value',value);}return value;
    }
    const xmlMethods={
      length:value=>state(value).nodes.length,
      toString:stringOf,valueOf:value=>value,toXMLString:value=>serialize(state(value).nodes),toJSON:value=>state(value).kind==='list'?'XMLList':'XML',
      copy:value=>{const r=state(value);if(r.kind==='list')checkList(r.nodes.length);const copies=r.nodes.map(n=>clone(n));commit(copies);return r.kind==='xml'?wrapped(copies[0]):list(copies);},
      children:value=>selected(value,'children'),child:(value,arg)=>selected(value,'child',arg),elements:(value,arg='*')=>selected(value,'elements',arg),
      descendants:(value,arg='*')=>selected(value,'descendants',arg),attributes:value=>selected(value,'attributes'),attribute:(value,arg)=>selected(value,'attribute',arg),
      text:value=>selected(value,'text'),comments:value=>selected(value,'comments'),processingInstructions:(value,arg='*')=>selected(value,'processingInstructions',arg),
      localName:value=>single(value).name?.local??null,nodeKind:value=>{const k=single(value).kind;return k==='pi'?'processing-instruction':k;},
      childIndex:value=>{const n=single(value);return n.parent?n.parent.children.indexOf(n):-1;},
      parent:value=>{const items=state(value).nodes;if(!items.length)return undefined;const parent=items[0].parent;return parent&&items.every(n=>n.parent===parent)?wrapped(parent):undefined;},
      hasSimpleContent:value=>{const items=state(value).nodes;return items.length===1?simple(items[0]):!items.some(n=>n.kind==='element');},
      hasComplexContent:value=>{const items=state(value).nodes;return items.length===1?items[0].children.some(n=>n.kind==='element'):items.some(n=>n.kind==='element');},
      appendChild:(value,input)=>insert(value,input,'append'),prependChild:(value,input)=>insert(value,input,'prepend'),setChildren:(value,input)=>insert(value,input,'replace'),normalize,
    };
    function facade(type,items){
      const target=Object.create(type==='xml'?XML.prototype:XMLList.prototype),r={kind:type,nodes:items,generation,methods:new Map()};let value;
      value=new Proxy(target,{
        get(target,key){state(value);if(key===Symbol.toStringTag)return type==='xml'?'XML':'XMLList';if(key===Symbol.toPrimitive)return ()=>stringOf(value);if(typeof key==='symbol')return undefined;
          if(/^(0|[1-9]\d*)$/.test(key))return items[Number(key)]?wrapped(items[Number(key)]):undefined;
          if(Object.hasOwn(xmlMethods,key)){if(!r.methods.has(key))r.methods.set(key,(...args)=>xmlMethods[key](value,...args));return r.methods.get(key);}
          if(key==='then')return undefined;unsupported(`property ${key}; use explicit child()/attribute() methods`);
        },
        set(){unsupported('direct XML/XMLList property assignment; use tree methods');},deleteProperty(){unsupported('E4X delete syntax');},
      });records.set(value,r);return value;
    }
    function XML(value=''){
      active();if(is(value)){const r=state(value);if(r.kind==='legacy')unsupported('legacy DOM to AS3 XML conversion; serialize explicitly');if(r.nodes.length!==1)throw new TypeError('XML construction requires exactly one node.');if(!new.target)return wrapped(r.nodes[0]);const copy=clone(r.nodes[0]);commit([copy]);return wrapped(copy);}
      const result=parsed(value==null?'':String(value),{...settings,trim:settings.ignoreWhitespace});
      if(result.roots.length>1)throw new TypeError('XML construction requires one root; use XMLList for fragments.');
      if(!result.roots.length)result.roots=[node('text')];commit(result.roots);return wrapped(result.roots[0]);
    }
    function XMLList(value=''){
      active();if(is(value)){const r=state(value);if(r.kind==='legacy')unsupported('legacy DOM to XMLList conversion');return list(r.nodes);}
      const result=parsed(value==null?'':String(value),{...settings,trim:settings.ignoreWhitespace});checkList(result.roots.length);commit(result.roots);return list(result.roots);
    }
    function setSettings(value){
      active();if(value==null){settings={...defaults};return;}
      if(typeof value!=='object')throw new TypeError('XML settings requires an object or null.');const next={...settings};
      for(const key of Object.keys(defaults)){const v=value[key];if(v!==undefined){if(key==='prettyIndent'){if(!Number.isInteger(v)||v<0||v>32)throw new RangeError('XML.prettyIndent requires an integer from 0 through 32.');next[key]=v;}else if(typeof v==='boolean')next[key]=v;else throw new TypeError(`XML.${key} requires a Boolean.`);}}
      settings=next;
    }
    Object.assign(XML,{settings:()=>{active();return {...settings};},defaultSettings:()=>({...defaults}),setSettings});
    for(const key of Object.keys(defaults))Object.defineProperty(XML,key,{get(){active();return settings[key];},set(value){setSettings({[key]:value});}});

    function legacyRecord(value){const r=state(value);if(r.kind!=='legacy')throw new TypeError('Expected a legacy XMLNode.');return r;}
    function legacyNode(value){return legacyRecord(value).nodes[0];}
    function legacyWrap(n,document=false){
      if(legacyWrappers.has(n))return legacyWrappers.get(n);
      const value=Object.create(document?LegacyXML.prototype:XMLNode.prototype);
      records.set(value,{kind:'legacy',nodes:[n],generation,ignoreWhite:false,status:0,loaded:undefined,operation:null,bytesLoaded:undefined,bytesTotal:undefined});legacyWrappers.set(n,value);return value;
    }
    function attributes(value){
      const n=legacyNode(value);if(attributeWrappers.has(n))return attributeWrappers.get(n);
      const born=generation,target=Object.create(null);
      const ensure=()=>{active();if(generation!==born)throw new Error('XML attributes belong to a previous publication session.');};
      const find=key=>n.attributes.find(a=>a.name.qualified===key);
      const proxy=new Proxy(target,{
        get(target,key){ensure();if(typeof key==='symbol')return undefined;return find(key)?.value;},
        set(target,key,value){ensure();if(n.kind!=='element')throw new TypeError('Attributes require an XML element.');const q=name(key),content=text(value),existing=find(key);if(existing)change(existing,'value',content);else{if(n.attributes.length+n.declarations.length>=256)throw new RangeError('XML element exceeds 256 attributes/declarations.');const a=node('attribute',q,content);commit([a]);a.parent=n;n.attributes.push(a);}return true;},
        deleteProperty(target,key){ensure();const a=find(key);if(a)detach(a);return true;},
        has(target,key){ensure();return Boolean(find(key));},ownKeys(){ensure();return n.attributes.map(a=>a.name.qualified);},
        getOwnPropertyDescriptor(target,key){ensure();const a=find(key);return a?{value:a.value,writable:true,enumerable:true,configurable:true}:undefined;},
        defineProperty(){unsupported('custom attribute descriptors');},setPrototypeOf(){unsupported('attribute prototype mutation');},preventExtensions(){unsupported('attribute sealing');},
      });attributeWrappers.set(n,proxy);return proxy;
    }
    const documentConstruction=Symbol('XML document construction');
    class XMLNode {
      constructor(type,value){active();if(type===documentConstruction)return;let n;if(type===1)n=node('element',name(value));else if(type===3)n=node('text',null,text(value));else unsupported('XMLNode constructor nodeType; supported values are 1 and 3');commit([n]);return legacyWrap(n);}
      get nodeType(){return {element:1,attribute:2,text:3,pi:7,comment:8,document:1}[legacyNode(this).kind];}
      get nodeName(){return legacyNode(this).name?.qualified??null;}
      set nodeName(value){const n=legacyNode(this);if(n.kind!=='element'||n.name.uri)unsupported('nodeName mutation on this node');change(n,'name',name(value));}
      get nodeValue(){const n=legacyNode(this);return n.kind==='text'||n.kind==='comment'||n.kind==='pi'?n.value:null;}
      set nodeValue(value){const n=legacyNode(this);if(n.kind!=='text')unsupported('nodeValue writes except text nodes');change(n,'value',text(value));}
      get localName(){return legacyNode(this).name?.local??null;}get prefix(){return legacyNode(this).name?.prefix??null;}get namespaceURI(){return legacyNode(this).name?.uri??null;}
      get attributes(){return attributes(this);}set attributes(value){unsupported('replacing the attributes map; assign individual attributes');}
      get childNodes(){return legacyNode(this).children.map(n=>legacyWrap(n));}
      get firstChild(){const n=legacyNode(this).children[0];return n?legacyWrap(n):null;}get lastChild(){const n=legacyNode(this).children.at(-1);return n?legacyWrap(n):null;}
      get parentNode(){const p=legacyNode(this).parent;return p?legacyWrap(p,p.kind==='document'):null;}
      get previousSibling(){const n=legacyNode(this),i=n.parent?.children.indexOf(n);return i>0?legacyWrap(n.parent.children[i-1]):null;}
      get nextSibling(){const n=legacyNode(this),i=n.parent?.children.indexOf(n),s=i===undefined?null:n.parent.children[i+1];return s?legacyWrap(s):null;}
      hasChildNodes(){return legacyNode(this).children.length>0;}
      appendChild(child){legacyInsert(this,child,null);}
      insertBefore(child,before){legacyInsert(this,child,before);}
      removeNode(){detach(legacyNode(this));}
      cloneNode(deep=false){const n=clone(legacyNode(this),Boolean(deep));commit([n]);return legacyWrap(n,n.kind==='document');}
      toString(){const n=legacyNode(this);return serialize(n.kind==='document'?n.children:[n],false,n.kind==='document'?n.declaration:null);}
    }
    function legacyInsert(value,input,before){
      const parent=legacyNode(value),child=legacyNode(input);if(!['element','document'].includes(parent.kind)||child.kind==='document')throw new TypeError('XML DOM insertion requires an element/document parent and a non-document child.');
      const sibling=before==null?null:legacyNode(before);if(sibling&&sibling.parent!==parent)throw new TypeError('insertBefore target is not a child of this parent.');
      if(child===sibling)return;for(let p=parent;p;p=p.parent)if(p===child)throw new TypeError('Cannot move an XML ancestor beneath itself.');
      insertionDepth(parent,[child]);detach(child);const index=sibling?parent.children.indexOf(sibling):parent.children.length;parent.children.splice(index,0,child);child.parent=parent;
    }
    class LegacyXML extends XMLNode {
      constructor(source=''){
        super(documentConstruction);const n=node('document');let status=0;
        if(source!==undefined&&source!==''){
          try{const result=parsed(source,{ignoreComments:false,ignoreProcessingInstructions:false,ignoreWhitespace:false,trim:false});n.children=result.roots;n.declaration=result.declaration;for(const child of n.children)child.parent=n;}
          catch(error){if(error.xmlCategory&&['syntax','namespace','character'].includes(error.xmlCategory))status=-6;else throw error;}
        }
        commit([n]);const value=legacyWrap(n,true);legacyRecord(value).status=status;return value;
      }
      get ignoreWhite(){return legacyRecord(this).ignoreWhite;}set ignoreWhite(value){legacyRecord(this).ignoreWhite=Boolean(value);}
      get status(){return legacyRecord(this).status;}get loaded(){return legacyRecord(this).loaded;}set loaded(value){legacyRecord(this).loaded=Boolean(value);}
      get xmlDecl(){return legacyNode(this).declaration??null;}set xmlDecl(value){unsupported('XML declaration mutation');}
      get docTypeDecl(){legacyRecord(this);return null;}set docTypeDecl(value){unsupported('DOCTYPE declarations');}
      createElement(value){return new XMLNode(1,value);}createTextNode(value){return new XMLNode(3,value);}
      parseXML(source){
        const r=legacyRecord(this),n=r.nodes[0];let result;
        try{result=parsed(source,{ignoreComments:false,ignoreProcessingInstructions:false,ignoreWhitespace:r.ignoreWhite,trim:false});commit(result.roots,encoder.encode(result.declaration||'').length-encoder.encode(n.declaration||'').length);}
        catch(error){r.status=-6;if(error.xmlCategory&&['syntax','namespace','character'].includes(error.xmlCategory))return;throw error;}
        // Old nodes remain valid detached snapshots; failure above never mutates them.
        const declaration=result.declaration??null;
        for(const old of n.children)old.parent=null;n.children=result.roots;for(const child of n.children)child.parent=n;change(n,'declaration',declaration);r.status=0;
      }
      load(url){return load(this,url);}
      getBytesLoaded(){return legacyRecord(this).bytesLoaded;}getBytesTotal(){return legacyRecord(this).bytesTotal;}
      onData(source){
        const r=legacyRecord(this),born=generation,operation=r.operation;
        if(source!==undefined)this.parseXML(source);
        if(disposed||generation!==born||r.operation!==operation)return;
        r.loaded=source!==undefined;const callback=this.onLoad;
        if(!disposed&&generation===born&&r.operation===operation&&typeof callback==='function')Reflect.apply(callback,this,[source!==undefined]);
      }
      onLoad(success){}
    }
    function cancel(r){if(!r.operation)return;const op=r.operation;r.operation=null;loads.delete(op);try{op.loader.close();}catch(_){};}
    function load(receiver,url){
      const r=legacyRecord(receiver);if(!net)unsupported('load without URLLoader service');if(typeof url!=='string')throw new TypeError('XML.load URL requires a string.');cancel(r);r.loaded=false;r.bytesLoaded=0;r.bytesTotal=0;
      const loader=new net.URLLoader(),op={loader,record:r,receiver,generation};r.operation=op;loads.add(op);
      const current=()=>!disposed&&generation===op.generation&&r.operation===op;
      const invoke=(name,args)=>{if(!current())return;const fn=receiver[name];if(!current())return;if(typeof fn==='function')Reflect.apply(fn,receiver,args);if(current())afterCallback?.();};
      loader.addEventListener('progress',()=>{if(current()){r.bytesLoaded=loader.bytesLoaded;r.bytesTotal=loader.bytesTotal;}});
      loader.addEventListener('httpStatus',event=>invoke('onHTTPStatus',[event.status]));
      const done=success=>{if(!current())return;r.bytesLoaded=loader.bytesLoaded;r.bytesTotal=loader.bytesTotal;try{invoke('onData',[success?loader.data:undefined]);}finally{if(r.operation===op)r.operation=null;loads.delete(op);}};
      loader.addEventListener('complete',()=>done(true));loader.addEventListener('ioError',()=>done(false));
      try{loader.load(new net.URLRequest(url));}catch(error){cancel(r);throw error;}return true;
    }
    for(const name of ['send','sendAndLoad','addRequestHeader'])Object.defineProperty(LegacyXML.prototype,name,{value(){unsupported(name);}});
    for(const[Type,predicate]of [[XML,v=>records.get(v)?.kind==='xml'],[XMLList,v=>records.get(v)?.kind==='list'],[LegacyXML,v=>records.get(v)?.kind==='legacy'&&records.get(v).nodes[0].kind==='document'],[XMLNode,v=>records.get(v)?.kind==='legacy']])Object.defineProperties(Type,{__nfIs:{value:predicate},__nfInitialize:{value:()=>unsupported('native subclassing')},[Symbol.hasInstance]:{value:predicate}});
    function ref(value,key,as3){
      const r=state(value);if(r.kind==='legacy')return {get:()=>value[key],set:next=>{value[key]=next;return next;},delete:()=>Reflect.deleteProperty(value,key),has:()=>key in value};
      if(!as3)unsupported('AS3 XML access from the AS2 DOM interface');const property=String(key);
      return {get:()=>value[property],set:()=>unsupported('E4X property/index assignment'),delete:()=>unsupported('E4X delete'),has:()=>/^(0|[1-9]\d*)$/.test(property)?Number(property)<r.nodes.length:Object.hasOwn(xmlMethods,property)};
    }
    function enumerate(value,values,as3){const r=state(value);if(r.kind==='legacy')unsupported('XMLNode enumeration; use childNodes or attributes');if(!as3)unsupported('AS3 XMLList iteration from AS2');const items=r.nodes.slice();return (function*(){for(let i=0;i<items.length;i++){state(value);yield values?wrapped(items[i]):String(i);}})();}
    function method(value,key,as3){const fn=ref(value,key,as3).get();return(...args)=>Reflect.apply(fn,value,args);}
    function reset(){active();for(const op of [...loads])cancel(op.record);generation++;nodes.clear();nodeCount=0;stringBytes=0;listRefs=0;settings={...defaults};}
    function dispose(){if(disposed)return;reset();disposed=true;}
    return Object.freeze({XML,XMLList,LegacyXML,XMLNode,is,kind,ref,enumerate,method,reset,dispose,stats:()=>({nodes:nodeCount,stringBytes,listRefs})});
  }
  const api=Object.freeze({create});if(typeof module!=='undefined'&&module.exports)module.exports=api;else global.NeoFlashXML=api;
})(globalThis);
