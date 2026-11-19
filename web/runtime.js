/* NeoFlash HTML5 host. Timeline evaluation and display-list construction run in shared Rust/WASM. */
(function (global) {
  'use strict';

  function localPoint(matrix, x, y) {
    const [a, b, c, d, tx, ty] = matrix;
    const determinant = a * d - b * c;
    if (!Number.isFinite(determinant) || Math.abs(determinant) < 1e-12) return null;
    return [(d * (x - tx) - c * (y - ty)) / determinant, (-b * (x - tx) + a * (y - ty)) / determinant];
  }
  function hitShape(kind, x, y, measureText, testPath, testBitmap, testText) {
    // Runtime colors already include the complete ColorTransform. A leaf can
    // have draw.alpha=1 while every actual painted channel is transparent.
    if ((kind.type==='rectangle'||kind.type==='ellipse'||kind.type==='path')&&
        (kind.fill?.[3]??255)<=0&&(!(kind.stroke?.[3]>0)||!(kind.stroke_width>0||kind.stroke_hairline))) return false;
    if (kind.type === 'rectangle') return x >= 0 && y >= 0 && x <= kind.width && y <= kind.height;
    if (kind.type === 'ellipse') {
      if (kind.width <= 0 || kind.height <= 0) return false;
      return Math.pow((x - kind.width / 2) / (kind.width / 2), 2) + Math.pow((y - kind.height / 2) / (kind.height / 2), 2) <= 1;
    }
    if (kind.type === 'text') {
      if ((kind.color?.[3]??255)<=0) return false;
      const lines = kind.text.split('\n');
      return x >= 0 && y >= 0 && y <= lines.length * kind.font_size * 1.2 &&
        x <= Math.max(0, ...lines.map(line => measureText(line, kind)));
    }
    if (kind.type === 'path' || kind.type === 'gradient_path') return testPath(kind, x, y);
    if (kind.type === 'bitmap_pixels') return testBitmap?testBitmap(kind,x,y):false;
    if (kind.type === 'video_frame') return Boolean(kind.alpha_bounds)&&x>=0&&y>=0&&x<kind.width&&y<kind.height;
    if (kind.type === 'text_field') return testText?testText(kind.field,x,y):false;
    return false;
  }
  function oneBasedFrame(value, total, resolveLabel) {
    if (typeof value === 'string') {
      const label = resolveLabel(value);
      if (label >= 0) return label;
      if (!/^\d+$/.test(value)) throw new Error(`Frame label '${value}' does not exist.`);
      value = Number(value);
    }
    if (!Number.isInteger(value) || value < 1 || value > total) throw new Error(`Frame must be between 1 and ${total}.`);
    return value - 1;
  }
  // AS2 label lookup is separate from the strict JavaScript/AS3 bridge.
  function as2Frame(value, total, resolveLabel) {
    // AVM1 parses numeric strings before consulting labels. Keep the numeric
    // compatibility boundary explicit instead of guessing clamping/wrapping.
    if (typeof value === 'string' && /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$|^[+-]?(?:Infinity|NaN)$/.test(value)) value = Number(value);
    if (typeof value === 'number') {
      if (!Number.isInteger(value) || value < 1 || value > total) {
        throw new Error(`AS2 numeric goto currently supports integer frames 1 through ${total}; legacy clamping, wrapping and other numeric coercions are not supported.`);
      }
      return value - 1;
    }
    if (typeof value === 'string') {
      const frame = resolveLabel(value);
      return frame >= 0 ? frame : null;
    }
    throw new Error('AS2 goto requires a frame label or integer frame; other argument coercions are not supported.');
  }
  function createAs2Methods() {
    const variants = new WeakMap();
    let resolve = (receiver, name) => receiver[name];
    let resolveKey = (receiver, name) => {
      const lower = String(name).toLowerCase();
      for (let cur = Object(receiver), d = 0; cur && d < 256; cur = Object.getPrototypeOf(cur), d++) {
        for (const k of Object.getOwnPropertyNames(cur)) if (k.toLowerCase() === lower) return k;
      }
      return name;
    };
    function aliasError() {
      return new TypeError('AS2 goto method aliases need receiver semantics not supported yet. Call gotoAndPlay/gotoAndStop directly on the target timeline.');
    }
    return {
      register(method, receiver, name, implementation) { variants.set(method, { receiver, name, implementation }); },
      setResolver(resolver) { resolve = resolver; },
      setKeyResolver(resolver) { resolveKey = resolver; },
      method(receiver, name, legacyCase = false) {
        if (receiver === null || receiver === undefined) return () => undefined;
        // Resolve the getter before arguments, then check callability after
        // argument evaluation, preserving ordinary call evaluation order.
        const actual = legacyCase ? resolveKey(receiver, name) : name;
        const method = resolve(receiver, actual);
        return (...args) => {
          if (typeof method !== 'function') {
            throw new TypeError(`AS2 timeline function '${name}' is missing or is not callable. Define it on this timeline before calling it.`);
          }
          const variant = variants.get(method);
          if (variant && (variant.receiver !== receiver || variant.name !== actual)) throw aliasError();
          return Reflect.apply(variant ? variant.implementation : method, receiver, args);
        };
      },
      value(method, receiver) {
        return (...args) => {
          if (variants.has(method)) throw aliasError();
          if (typeof method !== 'function') throw new TypeError('AS2 function value is not callable. Assign a function before calling it.');
          return Reflect.apply(method, receiver, args);
        };
      },
    };
  }
  function color(rgba) { return `rgba(${rgba[0]},${rgba[1]},${rgba[2]},${rgba[3] / 255})`; }
  function sourceKey(script) { return script.language + '\u0000' + script.source; }

  // Generated Function wrappers differ between JavaScript engines, and Safari / WebKit
  // omits sourceURL + line/column from `new Function` stacks when Web Inspector is closed.
  // In browser DOM hosts we compile user scripts via disjoint-line `<script>` elements so
  // Safari, Chrome, and Firefox all report exact 1-based line & column numbers; non-DOM
  // hosts (Node/VM tests) transparently fall back to calibrated `new Function`.
  let nextDomScriptLineBase = 200;
  function tryCompileDomScriptUnit(unit, paramList, headerLines, bodyCode, footerCode, url) {
    if (typeof document === 'undefined' || !document?.head || typeof document.createElement !== 'function' || typeof document.head.appendChild !== 'function') {
      return null;
    }
    const lineBase = nextDomScriptLineBase;
    const padding = '\n'.repeat(lineBase - 1);
    const bodyStartLine = lineBase + headerLines.length;
    const codeLineCount = Math.max(1, String(bodyCode || '').split('\n').length);
    const bodyEndLine = bodyStartLine + codeLineCount - 1;
    const scriptText = `${padding}globalThis.__nfCompiledSlot = function(${paramList.join(', ')}) {${headerLines[0] ? ' ' + headerLines[0] : ''}${headerLines.length > 1 ? '\n' + headerLines.slice(1).join('\n') : ''}\n${bodyCode}\n${footerCode}\n//# sourceURL=${url}`;
    const hadPrevSuppress = Object.prototype.hasOwnProperty.call(global, '__neoflashSuppressWindowError');
    const prevSuppress = global.__neoflashSuppressWindowError;
    const hadPrevSlot = Object.prototype.hasOwnProperty.call(global, '__nfCompiledSlot');
    const prevSlot = global.__nfCompiledSlot;
    global.__neoflashSuppressWindowError = true;
    global.__nfCompiledSlot = null;
    try {
      const el = document.createElement('script');
      el.textContent = scriptText;
      document.head.appendChild(el);
      if (typeof el.remove === 'function') el.remove();
      else if (el.parentNode && typeof el.parentNode.removeChild === 'function') el.parentNode.removeChild(el);
      const compiledFn = global.__nfCompiledSlot;
      if (typeof compiledFn === 'function') {
        nextDomScriptLineBase = bodyEndLine + 8;
        if (unit && typeof unit === 'object') {
          try {
            Object.defineProperties(unit, {
              __domBodyStartLine: { value: bodyStartLine, configurable: true, enumerable: false, writable: true },
              __domBodyEndLine: { value: bodyEndLine, configurable: true, enumerable: false, writable: true },
            });
          } catch (_) {}
        }
        return compiledFn;
      }
    } catch (_) {
      // Syntax errors fall back to `new Function` so standard diagnostics fire cleanly.
    } finally {
      if (hadPrevSlot) global.__nfCompiledSlot = prevSlot;
      else delete global.__nfCompiledSlot;
      if (hadPrevSuppress) global.__neoflashSuppressWindowError = prevSuppress;
      else delete global.__neoflashSuppressWindowError;
    }
    return null;
  }
  function functionBodyLineOffset() {
    const stack = new Function('return new Error().stack;\n//# sourceURL=neoflash-offset.js')();
    const match = String(stack || '').match(/neoflash-offset\.js:(\d+):\d+/);
    return match ? Number(match[1]) - 1 : null;
  }
  function scriptFunction(script, bindings) {
    const url = /^neoflash-script-\d+\.js$/.test(script.source_url || '') ? script.source_url : 'neoflash-user-script.js';
    const domFn = tryCompileDomScriptUnit(
      script,
      bindings,
      ['"use strict";', 'return (function () {'],
      script.code,
      '}).call(this);\n};',
      url,
    );
    if (domFn) return domFn;
    return new Function(...bindings, '"use strict";\nreturn (function () {\n' + script.code + '\n}).call(this);\n//# sourceURL=' + url);
  }
  function compileProgramUnit(programUnit) {
    const url = programUnit?.source_url || 'neoflash-project.js';
    const domFn = tryCompileDomScriptUnit(
      programUnit,
      ['__nfBuiltins'],
      [''],
      programUnit.code,
      '};',
      url,
    );
    if (domFn) return domFn;
    return new Function('__nfBuiltins', programUnit.code + '\n//# sourceURL=' + url);
  }
  const runtimeInternalHostFiles = new Set([
    'runtime.js', 'preview.js', 'neoflash_web.js', 'display.js', 'stage.js', 'events.js',
    'text.js', 'text-input.js', 'audio.js', 'video.js', 'bitmap.js', 'path.js', 'filters.js',
    'geom.js', 'geom3d.js', 'net.js', 'xml.js', 'loader.js', 'storage.js', 'external.js',
    'air.js', 'legacy-input.js', 'dom-layout.js', 'accessibility.js', 'byte-array.js',
    'fte3d.js', 'number-format.js', 'date.js', 'math.js', 'easing.js', 'broadcaster.js',
    'timers.js', 'amf.js',
  ]);
  function parseRuntimeStackFrame(rawLine) {
    const trimmed = String(rawLine || '').trim();
    if (!trimmed) return null;
    const isV8 = /^\s*at\s/.test(rawLine);
    if (!isV8 && !trimmed.includes('@')) return null;
    let locPart = '';
    if (isV8) {
      const rest = trimmed.replace(/^at\s+/, '').trim();
      if (rest.endsWith(')')) {
        let depth = 0, openIdx = -1;
        for (let i = rest.length - 1; i >= 0; i--) {
          if (rest[i] === ')') depth++;
          else if (rest[i] === '(' && --depth === 0) { openIdx = i; break; }
        }
        locPart = openIdx >= 0 ? rest.slice(openIdx + 1, -1).trim() : rest;
      } else {
        locPart = rest;
      }
    } else {
      const atIdx = trimmed.indexOf('@');
      if (atIdx < 0) return null;
      locPart = trimmed.slice(atIdx + 1).trim();
    }
    if (!locPart || locPart === '[native code]' || (locPart.startsWith('[') && locPart.endsWith(']'))) return null;
    const match = locPart.match(/^(.*?):(\d+):(\d+)$/);
    if (!match) return null;
    const rawUrl = match[1].trim();
    const line = Number(match[2]);
    const column = Number(match[3]);
    if (!rawUrl || !Number.isInteger(line) || line < 1 || !Number.isInteger(column) || column < 0) return null;
    const cleanUrl = rawUrl.split('#')[0].split('?')[0];
    const slash = cleanUrl.lastIndexOf('/');
    const fileName = slash >= 0 ? cleanUrl.slice(slash + 1) : cleanUrl;
    return { rawUrl, cleanUrl, fileName, line, column: Math.max(1, column) };
  }
  function extractAuthoredSourceLine(unit, sourceName, line1) {
    if (!(line1 >= 1) || !unit || typeof unit !== 'object') return null;
    if (typeof unit.source === 'string' && (!unit.location || unit.location === sourceName)) {
      const text = unit.source.split('\n')[line1 - 1];
      if (typeof text === 'string' && text.trim()) return text.trim();
    }
    const sm = unit.source_map;
    if (sm && Array.isArray(sm.sources) && Array.isArray(sm.sourcesContent)) {
      const idx = sm.sources.indexOf(sourceName);
      if (idx >= 0 && typeof sm.sourcesContent[idx] === 'string') {
        const text = sm.sourcesContent[idx].split('\n')[line1 - 1];
        if (typeof text === 'string' && text.trim()) return text.trim();
      }
    }
    return null;
  }
  function lookupUnitSourceLocation(unit, generatedLine, generatedColumn) {
    const locations = Array.isArray(unit.source_locations) && unit.source_locations.length > 0
      ? unit.source_locations
      : (global?.NeoFlashPreview?.decodeSourceMapLocations?.(unit.source_map) || []);
    let found = null, firstOnLine = null;
    for (const location of locations) {
      if (location.generated_line !== generatedLine) continue;
      if (!firstOnLine || location.generated_column < firstOnLine.generated_column) firstOnLine = location;
      if (location.generated_column <= generatedColumn && (!found || location.generated_column >= found.generated_column)) {
        found = location;
      }
    }
    return found || firstOnLine;
  }
  function mapScriptStack(stack, sources, bodyOffset) {
    if (!Array.isArray(sources) || sources.length === 0) return [];
    const hasDomUnits = sources.some(s => s && Number.isInteger(s.__domBodyStartLine));
    const pageCleanUrl = typeof global?.location?.href === 'string' ? global.location.href.split('#')[0].split('?')[0] : '';
    const frames = [];
    for (const rawLine of String(stack || '').split('\n')) {
      const parsed = parseRuntimeStackFrame(rawLine);
      if (!parsed) continue;
      const lowerFile = parsed.fileName.toLowerCase();
      if (runtimeInternalHostFiles.has(lowerFile) || /^as[23]-[a-z0-9_-]+\.js$/.test(lowerFile)) continue;
      let unit = null;
      let generatedLine = null;
      const namedMatch = parsed.fileName.match(/^(neoflash-(?:script-\d+|project)\.js)$/);
      if (namedMatch) {
        unit = sources.find(source => source && source.source_url === namedMatch[1]) || null;
        if (!unit) continue;
        if (Number.isInteger(unit.__domBodyStartLine)) {
          generatedLine = parsed.line - unit.__domBodyStartLine + 1;
        } else if (bodyOffset !== null && bodyOffset !== undefined) {
          generatedLine = parsed.line - bodyOffset - (namedMatch[1] === 'neoflash-project.js' ? 0 : 2);
        } else {
          continue;
        }
      } else if (hasDomUnits && (!pageCleanUrl || parsed.cleanUrl === pageCleanUrl || lowerFile === 'index.html')) {
        unit = sources.find(source =>
          source &&
          Number.isInteger(source.__domBodyStartLine) &&
          Number.isInteger(source.__domBodyEndLine) &&
          parsed.line >= source.__domBodyStartLine &&
          parsed.line <= source.__domBodyEndLine
        ) || null;
        if (!unit) continue;
        generatedLine = parsed.line - unit.__domBodyStartLine + 1;
      } else if (lowerFile === 'bundle.js') {
        unit = sources.find(source => source && (source.source_url === 'neoflash-project.js' || source.source_map)) || null;
        if (!unit) continue;
        generatedLine = parsed.line;
      } else {
        continue;
      }
      if (!(generatedLine >= 1)) continue;
      const found = lookupUnitSourceLocation(unit, generatedLine, parsed.column);
      if (found) {
        const sourceLine = found.sourceLine || extractAuthoredSourceLine(unit, found.source, found.line);
        frames.push({
          source: found.source,
          line: found.line,
          column: found.column,
          ...(sourceLine ? { sourceLine } : {}),
          source_url: unit.source_url,
          locations: unit.locations || [found.source],
        });
      }
    }
    return frames;
  }
  const nativeStackGetter = Object.getOwnPropertyDescriptor(new Error(), 'stack')?.get || Object.getOwnPropertyDescriptor(Error.prototype, 'stack')?.get;
  function mappedErrorFrames(error, sources, bodyOffset) {
    const visited = new Set();
    for (let depth = 0; error && depth < 16 && !visited.has(error); depth++) {
      visited.add(error);
      try {
        const stackProperty = Object.getOwnPropertyDescriptor(Object(error), 'stack');
        let stack = stackProperty?.value;
        if (stackProperty?.get && stackProperty.get === nativeStackGetter) stack = nativeStackGetter.call(error);
        if (typeof stack === 'string') {
          const frames = mapScriptStack(stack, sources, bodyOffset);
          if (frames.length) return frames;
        }
        // Safari/JSC also exposes own `line`, `column`, and `sourceURL` data properties on Error instances.
        const lineDesc = Object.getOwnPropertyDescriptor(Object(error), 'line');
        const colDesc = Object.getOwnPropertyDescriptor(Object(error), 'column');
        const urlDesc = Object.getOwnPropertyDescriptor(Object(error), 'sourceURL');
        const ownLine = lineDesc && Object.hasOwn(lineDesc, 'value') ? lineDesc.value : null;
        const ownCol = colDesc && Object.hasOwn(colDesc, 'value') ? colDesc.value : 1;
        const ownUrl = urlDesc && Object.hasOwn(urlDesc, 'value') ? urlDesc.value : null;
        if (Number.isInteger(ownLine) && ownLine >= 1 && typeof ownUrl === 'string' && ownUrl) {
          const synthetic = mapScriptStack(`@${ownUrl}:${ownLine}:${Number.isInteger(ownCol) && ownCol >= 1 ? ownCol : 1}`, sources, bodyOffset);
          if (synthetic.length) return synthetic;
        }
        // Do not invoke an authored getter while reporting the original exception.
        error = Object.getOwnPropertyDescriptor(Object(error), 'cause')?.value;
      } catch (_) { break; }
    }
    return [];
  }
  function sourceLocationText(location) {
    return location ? `${location.source}:${location.line}:${location.column}` : '';
  }

  const as2Thrown = Symbol.for('neoflash.as2.thrown.v1');
  function unwrapScriptThrow(value) {
    // Only compiler transport envelopes have these own data properties. Do not
    // invoke arbitrary accessors while trying to report an author's exception.
    try {
      if (value !== null && (typeof value === 'object' || typeof value === 'function')) {
        const brand = Object.getOwnPropertyDescriptor(value, as2Thrown);
        const payload = Object.getOwnPropertyDescriptor(value, 'value');
        if (brand && brand.value === true && payload && Object.hasOwn(payload, 'value')) return payload.value;
      }
    } catch (_) { /* A revoked or hostile Proxy is still the original failure. */ }
    return value;
  }
  function describeThrown(value, includeStack = false) {
    if (value !== null && value !== undefined) {
      let message = null;
      try { if (typeof value.message === 'string') message = value.message; } catch (_) {}
      if (includeStack) {
        try {
          const stack = value.stack;
          if (typeof stack === 'string' && stack) {
            return message && !stack.includes(message) ? `${message}\n\n${stack}` : stack;
          }
        } catch (_) {}
      }
      if (message !== null) return message;
    }
    try { return String(value); } catch (_) { return '<unprintable thrown value>'; }
  }
  // The Stage and BitmapData.draw share this exact painter. Rust supplies
  // the authoritative snapshot; this adapter never reconstructs a scene graph.
  function createScenePainter({global,paths,gradients,clippings,bitmapHost,textHost,filterHost,videoHost,context,matrix=item=>item.transform,hidden=()=>false,snapBitmaps=true}) {
    const bitmapMatrix=matrix;
    let blendScratch=null;
    const font=kind=>`${kind.font_size}px ${global.NeoFlashText?.cssFontFamily?global.NeoFlashText.cssFontFamily(kind.font_family):`${JSON.stringify(kind.font_family)}, sans-serif`}`;
    function paintItem(context,item) {
        const kind = item.kind;
        if(kind.type==='text_field'&&hidden(item.owner_ref))return;
        context.save();
        try {
        context.transform(...bitmapMatrix(item));
        context.globalAlpha = Math.max(0, Math.min(1, item.alpha));
        if (kind.type === 'rectangle' || kind.type === 'ellipse') {
          context.beginPath();
          if (kind.type === 'rectangle') context.rect(0, 0, kind.width, kind.height);
          else context.ellipse(kind.width / 2, kind.height / 2, Math.max(0, kind.width / 2), Math.max(0, kind.height / 2), 0, 0, Math.PI * 2);
          context.fillStyle = color(kind.fill);
          context.fill();
          if (kind.stroke_width > 0 && kind.stroke[3]) {
            context.strokeStyle = color(kind.stroke);
            context.lineWidth = kind.stroke_width;
            context.stroke();
          }
        } else if (kind.type === 'path' || kind.type === 'gradient_path') {
          gradients.draw(context,kind,paths.get(kind.commands),bitmapHost?.cache);
        } else if (kind.type === 'bitmap_pixels') {
          if(!bitmapHost)throw new Error('Bitmap runtime is missing. Re-export this project.');
          bitmapHost.cache.draw(context,snapBitmaps?kind:{...kind,pixel_snapping:'never'});
        } else if (kind.type === 'video_frame') {
          if(!videoHost)throw new Error('Video runtime is missing. Re-export this project.');
          videoHost.draw(context,kind,item.owner_ref);
        } else if (kind.type === 'text_field') {
          if(!textHost)throw new Error('Text layout runtime is missing. Re-export this project.');
          textHost.draw(context,kind.field);
        } else if (kind.type === 'text') {
          context.font = font(kind);
          context.textAlign = 'left';
          context.textBaseline = 'top';
          context.fillStyle = color(kind.color);
          kind.text.split('\n').forEach((line, index) => context.fillText(line, 0, index * kind.font_size * 1.2));
        } else throw new Error(`Unsupported display primitive: ${kind.type}`);
        } finally { context.restore(); }
    }
    function paintBounds(item,geometry=false) {
      let kind=item.kind.type==='text_field'?{...item.kind,text_bounds:textHost.bounds(item.kind.field,true)}:item.kind;
      if(geometry){
        const opaque=rgba=>[...(rgba||[0,0,0,0]).slice(0,3),255];
        if(['rectangle','ellipse','path','gradient_path'].includes(kind.type))kind={...kind,type:kind.type==='gradient_path'?'path':kind.type,fill:opaque(kind.fill),stroke:opaque(kind.stroke)};
        else if(kind.type==='bitmap_pixels'||kind.type==='video_frame')kind={...kind,alpha_bounds:{left:0,top:0,right:kind.width,bottom:kind.height}};
        else if(kind.type==='text_field')kind={...kind,field:{...kind.field,alpha:255},text_bounds:kind.field.box?{left:0,top:0,right:kind.field.box.width,bottom:kind.field.box.height}:textHost.bounds({...kind.field,alpha:255},true)};
        else if(kind.type==='text')kind={...kind,color:opaque(kind.color)};
      }
      let bounded=global.NeoFlashAccessibility.itemBounds({...item,alpha:geometry?1:item.alpha,kind,transform:bitmapMatrix(item)},(line,textKind)=>{context.font=font(textKind);return context.measureText(line).width;});
      // Scratch bounds also include actual glyph overhang; layout advance
      // widths alone could crop italic text before applying the real mask.
      if((kind.type==='text_field'&&kind.field.alpha>0)||(kind.type==='text'&&kind.color[3]>0)){
        const field=kind.type==='text_field'?kind.field:null,lines=field?textHost.layout(field).lines:kind.text.split('\n').map((text,index)=>({text,x:0,y:index*kind.font_size*1.2})),m=bitmapMatrix(item);
        context.save();try{context.font=field?global.NeoFlashText.cssFont(field.format):font(kind);context.textAlign='left';context.textBaseline='top';
          for(const line of lines){if(!line.text.trim())continue;const metric=context.measureText(line.text),values=[metric.actualBoundingBoxLeft,metric.actualBoundingBoxRight,metric.actualBoundingBoxAscent,metric.actualBoundingBoxDescent];if(!values.every(Number.isFinite))continue;
            let left=line.x-values[0],right=line.x+values[1],top=line.y-values[2],bottom=line.y+values[3];if(field?.box){left=Math.max(0,left);top=Math.max(0,top);right=Math.min(field.box.width,right);bottom=Math.min(field.box.height,bottom);}if(right<=left||bottom<=top)continue;
            for(const[x,y]of [[left,top],[right,top],[right,bottom],[left,bottom]]){const px=m[0]*x+m[2]*y+m[4],py=m[1]*x+m[3]*y+m[5];bounded=bounded?{left:Math.min(bounded.left,px),top:Math.min(bounded.top,py),right:Math.max(bounded.right,px),bottom:Math.max(bounded.bottom,py)}:{left:px,top:py,right:px,bottom:py};}
          }
        }finally{context.restore();}
      }
      return bounded;
    }
    function draw(context,snapshot,{minX=0,minY=0,width=context.canvas.width,height=context.canvas.height,semantic=true}={}) {
      const scene=snapshot.draws;
      clippings.beginFrame();
      const clips=global.NeoFlashPaths.clipTable(snapshot?.clips||[],snapshot?.filter_groups||[]),objects=snapshot?(snapshot.objects?.__byKey||new Map((snapshot.objects||[]).map(node=>[JSON.stringify(node.object_ref),node]))):null;
      const semanticOwners=new Set((semantic?snapshot?.semantics||[]:[]).map(node=>JSON.stringify(node.owner_ref))),needsCoverage=new Map(),clippedBounds=new Map();
      const semanticCoverage=item=>{const key=JSON.stringify(item.owner_ref);if(needsCoverage.has(key))return needsCoverage.get(key);let needed=false;for(let node=objects?.get(key);node;node=objects.get(JSON.stringify(node.parent_ref)))if(semanticOwners.has(JSON.stringify(node.object_ref))){needed=true;break;}needsCoverage.set(key,needed);return needed;};
      const paintBoundsCache=new Map(),geometryBoundsCache=new Map();
      const bounded=(item,geometry=false)=>{const cache=geometry?geometryBoundsCache:paintBoundsCache;if(!cache.has(item))cache.set(item,paintBounds(item,geometry));return cache.get(item);};
      const paintLeaf=(target,item,deferred=[])=>{
        const clip_ids=(item.clip_ids||[]).filter(id=>!deferred.includes(id));
        if(!clip_ids.length){paintItem(target,item);return;}
        const readBounds=!item.filter_id&&semanticCoverage(item);
        const visible=clippings.draw(target,{...item,clip_ids},clips,paths,canvas=>paintItem(canvas,item),{bounds:bounded(item),readBounds,bitmaps:bitmapHost?.cache});
        if(readBounds)clippedBounds.set(item,visible||null);
      };
      const composite=(target,surface,bounds,clip_ids,operation='source-over')=>{
        if(operation==='subtract'||operation==='invert'){
          const tm=target.getTransform();
          const pts=[[bounds.left,bounds.top],[bounds.right,bounds.top],[bounds.right,bounds.bottom],[bounds.left,bounds.bottom]].map(([x,y])=>[tm.a*x+tm.c*y+tm.e,tm.b*x+tm.d*y+tm.f]);
          const dx0=Math.max(0,Math.floor(Math.min(...pts.map(p=>p[0])))),dy0=Math.max(0,Math.floor(Math.min(...pts.map(p=>p[1]))));
          const dx1=Math.min(target.canvas.width,Math.ceil(Math.max(...pts.map(p=>p[0])))),dy1=Math.min(target.canvas.height,Math.ceil(Math.max(...pts.map(p=>p[1]))));
          const dw=dx1-dx0,dh=dy1-dy0;
          if(!(dw>0&&dh>0))return;
          const sc=blendScratch||(blendScratch=typeof OffscreenCanvas==='function'?new OffscreenCanvas(1,1):global.document.createElement('canvas'));
          if(sc.width!==dw||sc.height!==dh){sc.width=1;sc.height=dh;sc.width=dw;}
          const sctx=sc.getContext('2d');
          sctx.setTransform(1,0,0,1,0,0);sctx.globalAlpha=1;sctx.globalCompositeOperation='source-over';sctx.clearRect(0,0,dw,dh);
          sctx.setTransform(tm.a,tm.b,tm.c,tm.d,tm.e-dx0,tm.f-dy0);
          const drawSrc=canvas=>canvas.drawImage(surface,bounds.left,bounds.top);
          if(clip_ids.length)clippings.draw(sctx,{clip_ids},clips,paths,drawSrc,{bounds,bitmaps:bitmapHost?.cache});else drawSrc(sctx);
          const srcData=sctx.getImageData(0,0,dw,dh).data,dstImg=target.getImageData(dx0,dy0,dw,dh),dstData=dstImg.data;
          for(let i=0;i<dstData.length;i+=4){
            const sa=srcData[i+3];if(sa===0)continue;
            const da=dstData[i+3],invSa=255-sa;
            const outA=Math.min(255,sa+((da*invSa+127)/255|0));
            if(outA===0){dstData[i]=dstData[i+1]=dstData[i+2]=dstData[i+3]=0;continue;}
            for(let c=0;c<3;c++){
              const dp=(dstData[i+c]*da+127)/255|0;
              let op;
              if(operation==='subtract'){
                const sp=(srcData[i+c]*sa+127)/255|0;
                op=dp>sp?dp-sp:0;
              }else{
                op=Math.min(255,((sa*(255-dp)+invSa*dp+127)/255)|0);
              }
              dstData[i+c]=Math.min(255,Math.round(op*255/outA));
            }
            dstData[i+3]=outA;
          }
          target.save();try{target.setTransform(1,0,0,1,0,0);target.putImageData(dstImg,dx0,dy0);}finally{target.restore();}
          return;
        }
        if(operation==='destination-in'&&clip_ids.length){
          const tw=target.canvas.width,th=target.canvas.height;
          if(!(tw>0&&th>0))return;
          const sc=blendScratch||(blendScratch=typeof OffscreenCanvas==='function'?new OffscreenCanvas(1,1):global.document.createElement('canvas'));
          if(sc.width!==tw||sc.height!==th){sc.width=1;sc.height=th;sc.width=tw;}
          const sctx=sc.getContext('2d'),tm=target.getTransform();
          sctx.setTransform(1,0,0,1,0,0);sctx.globalAlpha=1;sctx.globalCompositeOperation='source-over';sctx.clearRect(0,0,tw,th);
          sctx.setTransform(tm.a,tm.b,tm.c,tm.d,tm.e,tm.f);
          clippings.draw(sctx,{clip_ids},clips,paths,canvas=>canvas.drawImage(surface,bounds.left,bounds.top),{bounds,bitmaps:bitmapHost?.cache});
          target.save();try{target.setTransform(1,0,0,1,0,0);target.globalCompositeOperation='destination-in';target.drawImage(sc,0,0);}finally{target.restore();}
          return;
        }
        target.save();try{target.globalCompositeOperation=operation;const draw=canvas=>canvas.drawImage(surface,bounds.left,bounds.top);
          if(clip_ids.length)clippings.draw(target,{clip_ids},clips,paths,draw,{bounds,bitmaps:bitmapHost?.cache});else draw(target);
        }finally{target.restore();}
      };
      if(snapshot?.filter_groups?.length){
        if(!filterHost)throw new Error('Display filters require the shared filter runtime. Re-export this project.');
        filterHost.renderer.draw(context,snapshot,{bounds:item=>bounded(item,true),paint:paintLeaf,composite});
        // Filter effects are not new interaction geometry. Read the original
        // fully clipped source only for semantic ancestors, even on cache hits.
        for(const item of scene)if(item.filter_id&&item.clip_ids?.length&&semanticCoverage(item)){
          const area=bounded(item),visible=area&&{left:Math.max(minX,area.left),top:Math.max(minY,area.top),right:Math.min(width,area.right),bottom:Math.min(height,area.bottom)};
          const result=filterHost.renderer.coverage(visible,target=>clippings.draw(target,item,clips,paths,canvas=>paintItem(canvas,item),{bounds:visible,readBounds:true,bitmaps:bitmapHost?.cache}));
          clippedBounds.set(item,result||null);
        }
      }else{
        filterHost?.renderer.clear();
        for(const item of scene)paintLeaf(context,item);
      }
      return {clippedBounds,objects};
    }
    function clear(){gradients.clear();clippings.clear();if(blendScratch){blendScratch.width=1;blendScratch.height=1;blendScratch=null;}}
    return {draw,paintItem,paintBounds,clear};
  }
  const traceGetterBrand = Symbol.for('neoflash.trace.getter.v1');
  const vectorInstanceBrand = Symbol.for('neoflash.as3.vector.instance.v1');
  const dictionaryInstanceBrand = Symbol.for('neoflash.as3.dictionary.v1');
  function formatTraceKey(key) {
    const s = String(key);
    return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(s) ? s : JSON.stringify(s);
  }
  function resolveInheritedDescriptor(obj, prop) {
    for (let cur = obj, guard = 0; cur && guard < 64; cur = Object.getPrototypeOf(cur), guard++) {
      const desc = Object.getOwnPropertyDescriptor(cur, prop);
      if (desc) return desc;
    }
    return null;
  }
  function isDefaultPlainOrClassObject(val) {
    const tagDesc = resolveInheritedDescriptor(val, Symbol.toStringTag);
    if (tagDesc) return false;
    const tsDesc = resolveInheritedDescriptor(val, 'toString');
    if (!tsDesc) return true;
    return Object.hasOwn(tsDesc, 'value') && tsDesc.value === Object.prototype.toString;
  }
  function isNestedTraceContainer(val, program) {
    if (val === null || typeof val !== 'object') return false;
    try {
      if (Object.getOwnPropertyDescriptor(val, traceGetterBrand)?.value === true) return false;
      if (Array.isArray(val)) return val.length > 0 || Object.keys(val).length > 0;
      const vecState = Object.getOwnPropertyDescriptor(val, vectorInstanceBrand)?.value;
      if (vecState?.version === 1 && Array.isArray(vecState.values)) return vecState.values.length > 0;
      const dictState = Object.getOwnPropertyDescriptor(val, dictionaryInstanceBrand)?.value;
      if (dictState?.version === 1) return (dictState.entries?.size || 0) > 0;
      if (val instanceof Map || val instanceof Set) return val.size > 0;
      if (ArrayBuffer.isView(val) && !(val instanceof DataView)) return val.length > 0;
      if (val instanceof Date || val instanceof RegExp || val instanceof Error) return false;
      const clsInfo = program?.traceObjectInfo?.(val);
      if (clsInfo) return !clsInfo.customToString && clsInfo.entries.length > 0;
      if (isDefaultPlainOrClassObject(val)) return Object.keys(val).length > 0;
    } catch (_) {}
    return false;
  }
  function formatTraceBlock(open, close, items, hasNested, depth, maxInlineItems = 24, maxInlineChars = 88) {
    if (items.length === 0) return open + close;
    const hasNewline = items.some(item => item.includes('\n'));
    if (!hasNested && !hasNewline && items.length <= maxInlineItems) {
      const joined = items.join(', ');
      if (open.length + joined.length + close.length <= maxInlineChars) {
        return open === '{' ? `{ ${joined} }` : open.endsWith('{') ? `${open} ${joined} }` : `${open}${joined}${close}`;
      }
    }
    const childIndent = '  '.repeat(depth + 1);
    const closeIndent = '  '.repeat(depth);
    return `${open}\n${items.map(item => childIndent + item).join(',\n')}\n${closeIndent}${close}`;
  }
  function formatTraceValue(value, options = {}) {
    const topLevel = options.topLevel !== false;
    const depth = options.depth || 0;
    const maxDepth = options.maxDepth ?? 10;
    const maxItems = options.maxItems ?? 200;
    const program = options.program || null;
    const seen = options.seen || new Set();

    if (value === undefined) return 'undefined';
    if (value === null) return 'null';
    const kind = typeof value;
    if (kind === 'string') return topLevel ? value : JSON.stringify(value);
    if (kind === 'boolean' || kind === 'number') return String(value);
    if (kind === 'bigint') return topLevel ? String(value) : `${value}n`;
    if (kind === 'symbol') return String(value);
    if (kind === 'function') {
      try {
        const clsName = program?.className?.(value);
        if (clsName) return `[class ${clsName}]`;
        return value.name ? `[Function: ${value.name}]` : '[Function]';
      } catch (_) {
        return '[Function]';
      }
    }
    try {
      if (Object.getOwnPropertyDescriptor(value, traceGetterBrand)?.value === true) return '[Getter]';
      if (value instanceof Number || value instanceof String || value instanceof Boolean) {
        const prim = value.valueOf();
        return formatTraceValue(prim, { ...options, topLevel });
      }
      if (value instanceof Date) {
        const t = value.getTime();
        return Number.isNaN(t) ? 'Invalid Date' : value.toISOString();
      }
      if (value instanceof RegExp) return String(value);
      if (value instanceof Error) return describeThrown(value);
      if (value[Symbol.toStringTag] === 'NeoFlashDisplayObject') return String(value);
      if (value[Symbol.toStringTag] === 'XML' || value[Symbol.toStringTag] === 'XMLList') {
        return typeof value.toXMLString === 'function' ? String(value.toXMLString()) : String(value);
      }

      if (seen.has(value)) return '[Circular]';

      const childOpts = { topLevel: false, depth: depth + 1, maxDepth, maxItems, program, seen };

      if (Array.isArray(value)) {
        if (depth >= maxDepth) return `[Array(${value.length})]`;
        seen.add(value);
        try {
          const items = [];
          let hasNested = false;
          const limit = Math.min(value.length, maxItems);
          for (let i = 0; i < limit; i++) {
            const desc = Object.getOwnPropertyDescriptor(value, i);
            if (!desc) {
              items.push('<empty>');
            } else if (!Object.hasOwn(desc, 'value')) {
              items.push('[Getter]');
            } else {
              if (isNestedTraceContainer(desc.value, program)) hasNested = true;
              items.push(formatTraceValue(desc.value, childOpts));
            }
          }
          if (value.length > maxItems) items.push(`... +${value.length - maxItems} more`);
          for (const key of Object.keys(value)) {
            if (/^(?:0|[1-9]\d*)$/.test(key) && Number(key) < 4294967295) continue;
            if (items.length >= maxItems + 32) { items.push('...'); break; }
            const desc = Object.getOwnPropertyDescriptor(value, key);
            if (!desc) continue;
            const elem = Object.hasOwn(desc, 'value') ? desc.value : { [traceGetterBrand]: true };
            if (isNestedTraceContainer(elem, program)) hasNested = true;
            items.push(`${formatTraceKey(key)}: ${formatTraceValue(elem, childOpts)}`);
          }
          return formatTraceBlock('[', ']', items, hasNested, depth);
        } finally {
          seen.delete(value);
        }
      }

      const vecState = Object.getOwnPropertyDescriptor(value, vectorInstanceBrand)?.value;
      if (vecState?.version === 1 && Array.isArray(vecState.values)) {
        const typeName = vecState.element?.name || '*';
        const prefix = `Vector.<${typeName}>`;
        if (depth >= maxDepth) return `${prefix}[...(${vecState.values.length})]`;
        seen.add(value);
        try {
          const items = [];
          let hasNested = false;
          const vals = vecState.values;
          const limit = Math.min(vals.length, maxItems);
          for (let i = 0; i < limit; i++) {
            if (isNestedTraceContainer(vals[i], program)) hasNested = true;
            items.push(formatTraceValue(vals[i], childOpts));
          }
          if (vals.length > maxItems) items.push(`... +${vals.length - maxItems} more`);
          return formatTraceBlock(`${prefix}[`, ']', items, hasNested, depth);
        } finally {
          seen.delete(value);
        }
      }

      const dictState = Object.getOwnPropertyDescriptor(value, dictionaryInstanceBrand)?.value;
      if (dictState?.version === 1 && dictState.entries instanceof Map) {
        if (depth >= maxDepth) return 'Dictionary {...}';
        seen.add(value);
        try {
          const items = [];
          let hasNested = false;
          let count = 0, total = 0;
          for (const [token, entry] of dictState.entries) {
            const isObj = Object.hasOwn(entry, 'object') || Object.hasOwn(entry, 'weak');
            const rawKey = entry.weak ? entry.weak.deref() : isObj ? entry.object : entry.primitive;
            if (isObj && rawKey === undefined) continue;
            const rec = (isObj ? dictState.objects : dictState.primitives)?.get(rawKey);
            if (!rec || rec.token !== token) continue;
            total++;
            if (count >= maxItems) continue;
            count++;
            const displayKey = isObj ? formatTraceValue(rawKey, childOpts) : (/^(?:0|[1-9]\d*)$/.test(rawKey) && Number(rawKey) <= 268435455 ? rawKey : JSON.stringify(rawKey));
            if (isNestedTraceContainer(rawKey, program) || isNestedTraceContainer(rec.value, program)) hasNested = true;
            items.push(`${displayKey} => ${formatTraceValue(rec.value, childOpts)}`);
          }
          if (total > maxItems) items.push(`... +${total - maxItems} more`);
          return items.length === 0 ? 'Dictionary {}' : formatTraceBlock('Dictionary {', '}', items, hasNested, depth, 6, 88);
        } finally {
          seen.delete(value);
        }
      }

      if (ArrayBuffer.isView(value) && !(value instanceof DataView)) {
        const ctor = value.constructor?.name || 'TypedArray';
        if (depth >= maxDepth) return `${ctor}(${value.length}) [...]`;
        const items = [];
        const limit = Math.min(value.length, maxItems);
        for (let i = 0; i < limit; i++) items.push(String(value[i]));
        if (value.length > maxItems) items.push(`... +${value.length - maxItems} more`);
        return `${ctor}(${value.length}) [${items.join(', ')}]`;
      }

      if (value instanceof Map) {
        if (depth >= maxDepth) return `Map(${value.size}) {...}`;
        seen.add(value);
        try {
          const items = [];
          let hasNested = false, i = 0;
          for (const [k, v] of value.entries()) {
            if (i++ >= maxItems) { items.push(`... +${value.size - maxItems} more`); break; }
            if (isNestedTraceContainer(k, program) || isNestedTraceContainer(v, program)) hasNested = true;
            items.push(`${formatTraceValue(k, childOpts)} => ${formatTraceValue(v, childOpts)}`);
          }
          return items.length === 0 ? 'Map(0) {}' : formatTraceBlock(`Map(${value.size}) {`, '}', items, hasNested, depth, 6, 88);
        } finally {
          seen.delete(value);
        }
      }

      if (value instanceof Set) {
        if (depth >= maxDepth) return `Set(${value.size}) {...}`;
        seen.add(value);
        try {
          const items = [];
          let hasNested = false, i = 0;
          for (const v of value.values()) {
            if (i++ >= maxItems) { items.push(`... +${value.size - maxItems} more`); break; }
            if (isNestedTraceContainer(v, program)) hasNested = true;
            items.push(formatTraceValue(v, childOpts));
          }
          return items.length === 0 ? 'Set(0) {}' : formatTraceBlock(`Set(${value.size}) {`, '}', items, hasNested, depth, 12, 88);
        } finally {
          seen.delete(value);
        }
      }

      const clsInfo = program?.traceObjectInfo?.(value);
      if (clsInfo) {
        if (clsInfo.customToString) return String(value.toString());
        const open = `${clsInfo.className} {`;
        if (depth >= maxDepth) return `${open}...}`;
        seen.add(value);
        try {
          const items = [];
          let hasNested = false;
          const limit = Math.min(clsInfo.entries.length, maxItems);
          for (let i = 0; i < limit; i++) {
            const [k, v] = clsInfo.entries[i];
            if (isNestedTraceContainer(v, program)) hasNested = true;
            items.push(`${formatTraceKey(k)}: ${formatTraceValue(v, childOpts)}`);
          }
          if (clsInfo.entries.length > maxItems) items.push(`... +${clsInfo.entries.length - maxItems} more`);
          return items.length === 0 ? `${clsInfo.className} {}` : formatTraceBlock(open, '}', items, hasNested, depth, 6, 88);
        } finally {
          seen.delete(value);
        }
      }

      if (isDefaultPlainOrClassObject(value)) {
        const proto = Object.getPrototypeOf(value);
        const rawCtorName = proto && proto !== Object.prototype ? proto.constructor?.name : '';
        const prefix = rawCtorName && rawCtorName !== 'Object' ? `${rawCtorName} ` : '';
        const open = `${prefix}{`;
        if (depth >= maxDepth) return `${open}...}`;
        seen.add(value);
        try {
          const keys = Object.keys(value);
          const items = [];
          let hasNested = false;
          const limit = Math.min(keys.length, maxItems);
          for (let i = 0; i < limit; i++) {
            const k = keys[i];
            const desc = Object.getOwnPropertyDescriptor(value, k);
            if (!desc) continue;
            const elem = Object.hasOwn(desc, 'value') ? desc.value : { [traceGetterBrand]: true };
            if (isNestedTraceContainer(elem, program)) hasNested = true;
            items.push(`${formatTraceKey(k)}: ${formatTraceValue(elem, childOpts)}`);
          }
          if (keys.length > maxItems) items.push(`... +${keys.length - maxItems} more`);
          return items.length === 0 ? `${prefix}{}` : formatTraceBlock(open, '}', items, hasNested, depth, 6, 88);
        } finally {
          seen.delete(value);
        }
      }

      return String(value);
    } catch (_) {
      return '<unprintable object>';
    }
  }
  function formatTraceArgs(values, options = {}) {
    return values.map(v => formatTraceValue(v, { ...options, topLevel: true, depth: 0 })).join(' ');
  }
  function emitScriptMessage(kind, message, frames, host = global, rawError = null) {
    try {
      if (typeof host.CustomEvent === 'function') host.dispatchEvent(new host.CustomEvent('neoflash-script-message', {
        detail: {kind, message, location: frames[0] || null, stack: frames, ...(rawError ? { error: rawError } : {})},
      }));
    } catch (_) { /* Diagnostic observers must never change authored playback. */ }
  }
  function completeCleanup(operations) {
    const errors = [];
    for (const operation of operations) {
      try { operation(); } catch (error) { errors.push(error); }
    }
    if (errors.length === 1) throw errors[0];
    if (errors.length) throw new AggregateError(errors, 'Multiple runtime cleanup operations failed.', {cause: errors[0]});
  }
  const sharedWasmModules = (global && global.__neoflashSharedWasmModules instanceof Map)
    ? global.__neoflashSharedWasmModules
    : new Map();
  if (global && typeof global === 'object' && !(global.__neoflashSharedWasmModules instanceof Map)) {
    try {
      Object.defineProperty(global, '__neoflashSharedWasmModules', {
        value: sharedWasmModules,
        configurable: true,
        enumerable: false,
        writable: true,
      });
    } catch (_) {}
  }
  const wasmInitPromises = new WeakMap();
  const mountedContainers = new WeakMap();

  function hashString64(text) {
    let h1 = 0x811c9dc5 >>> 0;
    let h2 = 5381 >>> 0;
    const len = text.length;
    for (let i = 0; i < len; i++) {
      const code = text.charCodeAt(i);
      h1 ^= code;
      h1 = Math.imul(h1, 0x01000193) >>> 0;
      h2 = (Math.imul(h2, 33) ^ code) >>> 0;
    }
    return `${h1.toString(16).padStart(8, '0')}${h2.toString(16).padStart(8, '0')}`;
  }

  function decodeBase64Bytes(b64) {
    if (typeof global.atob === 'function') {
      const bin = global.atob(b64);
      const out = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
      return out;
    }
    if (typeof Buffer !== 'undefined') {
      return new Uint8Array(Buffer.from(b64, 'base64'));
    }
    throw new Error('Base64 decoding is unavailable in this JavaScript environment.');
  }

  function compileWasmBase64Once(b64) {
    const cacheKey = `b64:${b64.length}:${hashString64(b64)}`;
    let pending = sharedWasmModules.get(cacheKey);
    if (!pending) {
      pending = Promise.resolve()
        .then(() => WebAssembly.compile(decodeBase64Bytes(b64)))
        .catch(err => {
          sharedWasmModules.delete(cacheKey);
          throw err;
        });
      sharedWasmModules.set(cacheKey, pending);
    }
    return pending;
  }

  function compileWasmUrlOnce(wasmUrl) {
    const cacheKey = `url:${wasmUrl}`;
    let pending = sharedWasmModules.get(cacheKey);
    if (!pending) {
      pending = (async () => {
        if (typeof global.fetch !== 'function') {
          throw new Error('This environment does not provide fetch() for loading WebAssembly.');
        }
        const resp = await global.fetch(wasmUrl);
        if (!resp.ok) {
          throw new Error(`Failed to fetch WebAssembly runtime (${wasmUrl}: ${resp.status}).`);
        }
        const contentType = String(resp.headers?.get?.('Content-Type') || '');
        if (typeof WebAssembly.compileStreaming === 'function' && /^application\/wasm\b/i.test(contentType)) {
          return WebAssembly.compileStreaming(resp);
        }
        const buf = await resp.arrayBuffer();
        return WebAssembly.compile(buf);
      })().catch(err => {
        sharedWasmModules.delete(cacheKey);
        throw err;
      });
      sharedWasmModules.set(cacheKey, pending);
    }
    return pending;
  }

  function normalizeModuleBaseUrl(rawBase, doc) {
    const fallback = (typeof doc?.baseURI === 'string' && doc.baseURI)
      ? doc.baseURI
      : (typeof global.location?.href === 'string' ? global.location.href : null);
    if (typeof rawBase !== 'string' || !rawBase.trim()) {
      if (!fallback) return '';
      try { return new URL('.', fallback).href; } catch (_) { return fallback; }
    }
    let spec = rawBase.trim();
    const clean = spec.split('#')[0].split('?')[0];
    const lastSeg = clean.slice(clean.lastIndexOf('/') + 1);
    if (!clean.endsWith('/') && lastSeg && !/\.[A-Za-z0-9]+$/.test(lastSeg)) {
      spec += '/';
    }
    if (fallback) {
      try {
        const resolved = new URL(spec, fallback);
        if (!resolved.pathname.endsWith('/')) {
          resolved.pathname = resolved.pathname.slice(0, resolved.pathname.lastIndexOf('/') + 1);
        }
        resolved.search = '';
        resolved.hash = '';
        return resolved.href;
      } catch (_) {}
    }
    return spec.endsWith('/') ? spec : `${spec}/`;
  }

  function resolveModuleAssetUrl(relUrl, baseUrl) {
    if (typeof relUrl !== 'string') return relUrl;
    const trimmed = relUrl.trim();
    if (!trimmed) return trimmed;
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(trimmed)) return trimmed;
    if (baseUrl) {
      try { return new URL(trimmed, baseUrl).href; } catch (_) {}
      if (!trimmed.startsWith('/')) {
        return baseUrl.replace(/\/?$/, '/') + trimmed.replace(/^\.\//, '');
      }
    }
    return trimmed;
  }

  function parsePublishedBundleText(text) {
    const trimmed = String(text || '').trim();
    if (!trimmed) throw new TypeError('NeoFlash bundle.js is empty.');
    const prefix = /^(?:\/\*[\s\S]*?\*\/\s*)*(?:globalThis|window|self)\.NeoFlashBundle\s*=\s*/.exec(trimmed);
    const jsonSource = prefix
      ? trimmed.slice(prefix[0].length).replace(/(?:;\s*)?(?:\/\/#[^\r\n]*\s*)*$/, '').replace(/;\s*$/, '')
      : trimmed;
    const parsed = JSON.parse(jsonSource);
    if (!parsed || typeof parsed !== 'object' || !parsed.project || typeof parsed.project !== 'object') {
      throw new TypeError('Malformed NeoFlash bundle payload.');
    }
    return parsed;
  }

  async function fetchModuleBundle(bundleUrl) {
    if (typeof global.fetch !== 'function') {
      throw new Error('fetch() is unavailable to load NeoFlash bundle.js.');
    }
    const resp = await global.fetch(bundleUrl);
    if (!resp.ok) {
      throw new Error(`Failed to load NeoFlash bundle (${bundleUrl}: ${resp.status}).`);
    }
    return parsePublishedBundleText(await resp.text());
  }

  const MODULE_SCOPED_CSS = [
    '[data-neoflash-module], [data-neoflash-module] * { box-sizing: border-box; }',
    '[data-neoflash-module] { position: relative; width: 100%; height: 100%; display: grid; grid-template-rows: minmax(0,1fr) auto; overflow: hidden; font: 13px system-ui, sans-serif; color: #d2d6dd; }',
    '[data-neoflash-module][data-clean="true"] { grid-template-rows: minmax(0,1fr); }',
    '[data-neoflash-module][data-clean="true"] > [data-neoflash-role="footer"] { display: none !important; }',
    '[data-neoflash-module] [data-neoflash-role="viewport"] { position: relative; width: 100%; height: 100%; overflow: auto; min-height: 0; min-width: 0; overscroll-behavior: contain; touch-action: pan-x pan-y; outline: none; }',
    '[data-neoflash-module] [data-neoflash-role="viewport"]:fullscreen, [data-neoflash-module] [data-neoflash-role="viewport"]:-webkit-full-screen { background: #000; width: 100vw; height: 100vh; overflow: hidden; }',
    '[data-neoflash-module] [data-neoflash-role="stage-space"] { position: relative; }',
    '[data-neoflash-module] [data-neoflash-role="stage-surface"] { position: absolute; }',
    '[data-neoflash-module] [data-neoflash-role="stage"] { display: block; width: 100%; height: 100%; outline: none; }',
    '[data-neoflash-module][data-render-mode="dom"] [data-neoflash-role="stage"] { position: absolute; inset: 0; pointer-events: none; opacity: 0; visibility: hidden; }',
    '[data-neoflash-module] neoflash-stage[data-render-mode="dom"] { position: absolute; inset: 0; display: block; width: 100%; height: 100%; overflow: hidden; transform-origin: 0 0; }',
    '[data-neoflash-module] [data-neoflash-text-input] { outline: none; scrollbar-width: none; -ms-overflow-style: none; }',
    '[data-neoflash-module] [data-neoflash-text-input]::-webkit-scrollbar { display: none; width: 0; height: 0; }',
    '[data-neoflash-module] [data-neoflash-role="semantic-layer"] { position: absolute; inset: 0; pointer-events: none; }',
    '[data-neoflash-module] .neoflash-a11y-overlay { position: absolute; inset: 0; pointer-events: none; overflow: hidden; }',
    '[data-neoflash-module] .semantic-target { position: absolute; pointer-events: none; padding: 0; margin: 0; border: 0; border-radius: 3px; background: transparent; color: transparent; min-height: 0; font-size: 0; }',
    '[data-neoflash-module] .semantic-target:hover, [data-neoflash-module] .semantic-target:disabled { background: transparent; }',
    '[data-neoflash-module] .semantic-target:focus-visible { outline: 3px solid #d4e9ff; outline-offset: 3px; box-shadow: 0 0 0 6px #183d71; }',
    '[data-neoflash-module] .pan-mode, [data-neoflash-module] .pan-mode canvas { cursor: grab; }',
    '[data-neoflash-module] .nf-dom-scene { position: absolute; left: 0; top: 0; width: 100%; height: 100%; transform-origin: 0 0; pointer-events: auto; }',
    '[data-neoflash-module] .nf-dom-container { position: absolute; left: 0; top: 0; transform-origin: 0 0; pointer-events: none; }',
    '[data-neoflash-module] .nf-dom-button { position: absolute; left: 0; top: 0; transform-origin: 0 0; padding: 0; margin: 0; border: 0; border-radius: 0; background: transparent; color: inherit; min-height: 0; pointer-events: auto; cursor: pointer; text-align: left; }',
    '[data-neoflash-module] .nf-dom-button:hover, [data-neoflash-module] .nf-dom-button:disabled { background: transparent; }',
    '[data-neoflash-module] .nf-dom-button:focus-visible { outline: 2px solid #7ba9ef; outline-offset: 2px; }',
    '[data-neoflash-module] .nf-dom-text { position: absolute; left: 0; top: 0; transform-origin: 0 0; margin: 0; padding: 0; line-height: 1.2; }',
    '[data-neoflash-module] .nf-dom-input { position: absolute; left: 0; top: 0; transform-origin: 0 0; margin: 0; box-sizing: border-box; pointer-events: auto; }',
    '[data-neoflash-module] .nf-dom-input:focus-visible { outline: 2px solid #7ba9ef; outline-offset: 1px; }',
    '[data-neoflash-module] .nf-dom-svg { position: absolute; left: 0; top: 0; overflow: visible; transform-origin: 0 0; pointer-events: none; }',
    '[data-neoflash-module] .nf-dom-bitmap, [data-neoflash-module] .nf-dom-video, [data-neoflash-module] .nf-dom-canvas-island { position: absolute; left: 0; top: 0; transform-origin: 0 0; pointer-events: none; }',
    '[data-neoflash-module] [data-neoflash-role="footer"] { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; padding: 8px 12px; background: #25272b; border-top: 1px solid #ffffff12; min-width: 0; }',
    '[data-neoflash-module] [data-neoflash-role="transport"] { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }',
    '[data-neoflash-module] [data-neoflash-role="footer"] button, [data-neoflash-module] [data-neoflash-role="footer"] select { background: #34373e; color: #f3f4f6; border: 1px solid #555a64; border-radius: 4px; font: inherit; padding: 6px 10px; cursor: pointer; min-height: 30px; }',
    '[data-neoflash-module] [data-neoflash-role="preview-info"] { display: flex; flex-wrap: wrap; gap: 4px 12px; min-width: 0; flex: 1; }',
    '[data-neoflash-module] [data-neoflash-role="status"] { font-variant-numeric: tabular-nums; color: #a5acb7; white-space: nowrap; }',
    '[data-neoflash-module] [data-neoflash-role="trace"] { flex-basis: 100%; overflow-wrap: anywhere; min-height: 1em; color: #c6d8ef; }',
    '[data-neoflash-module] [data-neoflash-role="error"] { position: absolute; inset: 12px; max-height: calc(100% - 24px); overflow: auto; margin: auto; z-index: 2; padding: 16px; border: 1px solid #f19393; background: #351d22; color: #ffe0e0; white-space: pre-wrap; display: none; }',
    '[data-neoflash-module] [data-neoflash-role="loading"] { position: absolute; inset: 0; display: grid; place-items: center; pointer-events: none; }',
  ].join('\n');

  function ensureModuleStylesLoaded(doc) {
    if (!doc?.head || typeof doc.createElement !== 'function') return;
    if (typeof doc.querySelector === 'function' && doc.querySelector('style[data-neoflash-module-styles="true"]')) return;
    const styleEl = doc.createElement('style');
    styleEl.setAttribute?.('data-neoflash-module-styles', 'true');
    styleEl.textContent = MODULE_SCOPED_CSS;
    doc.head.appendChild(styleEl);
  }

  function buildScopedModuleDom(doc, container, project, playerConfig, isDomMode, isClean) {
    ensureModuleStylesLoaded(doc);
    const existingCanvas = typeof container.querySelector === 'function'
      ? container.querySelector('[data-neoflash-role="stage"], canvas')
      : null;
    const existingViewport = typeof container.querySelector === 'function'
      ? container.querySelector('[data-neoflash-role="viewport"]')
      : null;
    if (existingCanvas && existingViewport) {
      const existingSurface = container.querySelector?.('[data-neoflash-role="stage-surface"]') || existingCanvas.parentNode || container;
      const existingSpace = container.querySelector?.('[data-neoflash-role="stage-space"]') || existingSurface.parentNode || existingViewport;
      let existingSemantic = container.querySelector?.('[data-neoflash-role="semantic-layer"]');
      if (!existingSemantic && typeof doc?.createElement === 'function') {
        existingSemantic = doc.createElement('div');
        existingSemantic.setAttribute?.('data-neoflash-role', 'semantic-layer');
        existingSurface?.appendChild?.(existingSemantic);
      }
      return {
        ownedRoot: null,
        rootWrapper: container,
        viewport: existingViewport,
        stageSpace: existingSpace,
        stageSurface: existingSurface,
        canvas: existingCanvas,
        semanticEl: existingSemantic,
        a11yOverlay: container.querySelector?.('.neoflash-a11y-overlay') || null,
        loadingEl: container.querySelector?.('[data-neoflash-role="loading"]') || null,
        errorEl: container.querySelector?.('[data-neoflash-role="error"]') || null,
        footerEl: container.querySelector?.('[data-neoflash-role="footer"]') || null,
        transportEl: container.querySelector?.('[data-neoflash-role="transport"]') || null,
        toggleBtn: container.querySelector?.('[data-neoflash-role="toggle"]') || null,
        restartBtn: container.querySelector?.('[data-neoflash-role="restart"]') || null,
        zoomSelect: container.querySelector?.('[data-neoflash-role="zoom"]') || null,
        panBtn: container.querySelector?.('[data-neoflash-role="pan"]') || null,
        fullscreenBtn: container.querySelector?.('[data-neoflash-role="fullscreen-toggle"]') || null,
        audioToggleBtn: container.querySelector?.('[data-neoflash-role="audio-toggle"]') || null,
        audioStatusEl: container.querySelector?.('[data-neoflash-role="audio-status"]') || null,
        reportToggleBtn: container.querySelector?.('[data-neoflash-role="report-toggle"]') || null,
        sizeReportEl: container.querySelector?.('[data-neoflash-role="size-report"]') || null,
        nameEl: container.querySelector?.('[data-neoflash-role="name"]') || null,
        statusEl: container.querySelector?.('[data-neoflash-role="status"]') || null,
        loadInfoEl: container.querySelector?.('[data-neoflash-role="load-info"]') || null,
        traceEl: container.querySelector?.('[data-neoflash-role="trace"]') || null,
      };
    }

    const title = String(project?.name || 'NeoFlash publication');
    const width = Number(project?.width) || 800;
    const height = Number(project?.height) || 600;
    const rootWrapper = doc.createElement('div');
    rootWrapper.setAttribute?.('data-neoflash-module', 'true');
    if (isClean) rootWrapper.setAttribute?.('data-clean', 'true');
    if (isDomMode) rootWrapper.setAttribute?.('data-render-mode', 'dom');

    const viewport = doc.createElement('div');
    viewport.setAttribute?.('data-neoflash-role', 'viewport');
    viewport.tabIndex = -1;
    viewport.setAttribute?.('aria-label', title);

    const stageSpace = doc.createElement('div');
    stageSpace.setAttribute?.('data-neoflash-role', 'stage-space');

    const stageSurface = doc.createElement('div');
    stageSurface.setAttribute?.('data-neoflash-role', 'stage-surface');
    stageSurface.setAttribute?.('role', 'group');
    stageSurface.setAttribute?.('aria-label', title);

    const canvas = doc.createElement('canvas');
    canvas.setAttribute?.('data-neoflash-role', 'stage');
    if (isDomMode) {
      canvas.setAttribute?.('aria-hidden', 'true');
    } else {
      canvas.setAttribute?.('aria-label', title);
    }
    stageSurface.appendChild(canvas);

    if (isDomMode) {
      const domStage = doc.createElement('neoflash-stage');
      domStage.setAttribute?.('data-render-mode', 'dom');
      domStage.setAttribute?.('width', String(width));
      domStage.setAttribute?.('height', String(height));
      stageSurface.appendChild(domStage);
    }

    const semanticEl = doc.createElement('div');
    semanticEl.setAttribute?.('data-neoflash-role', 'semantic-layer');
    stageSurface.appendChild(semanticEl);

    const a11yOverlay = doc.createElement('div');
    a11yOverlay.className = 'neoflash-a11y-overlay';
    if (a11yOverlay.style) {
      a11yOverlay.style.position = 'absolute';
      a11yOverlay.style.inset = '0';
      a11yOverlay.style.pointerEvents = 'none';
      a11yOverlay.style.overflow = 'hidden';
    }
    stageSurface.appendChild(a11yOverlay);

    stageSpace.appendChild(stageSurface);
    viewport.appendChild(stageSpace);
    rootWrapper.appendChild(viewport);

    let footerEl = null, transportEl = null, toggleBtn = null, restartBtn = null, zoomSelect = null;
    let panBtn = null, fullscreenBtn = null, audioToggleBtn = null, audioStatusEl = null;
    let reportToggleBtn = null, sizeReportEl = null, nameEl = null, statusEl = null, loadInfoEl = null, traceEl = null;

    if (!isClean) {
      footerEl = doc.createElement('div');
      footerEl.setAttribute?.('data-neoflash-role', 'footer');
      transportEl = doc.createElement('div');
      transportEl.setAttribute?.('data-neoflash-role', 'transport');
      toggleBtn = doc.createElement('button');
      toggleBtn.type = 'button';
      toggleBtn.setAttribute?.('data-neoflash-role', 'toggle');
      toggleBtn.textContent = 'Pause';
      restartBtn = doc.createElement('button');
      restartBtn.type = 'button';
      restartBtn.setAttribute?.('data-neoflash-role', 'restart');
      restartBtn.textContent = 'Restart';
      audioToggleBtn = doc.createElement('button');
      audioToggleBtn.type = 'button';
      audioToggleBtn.setAttribute?.('data-neoflash-role', 'audio-toggle');
      audioToggleBtn.hidden = true;
      audioToggleBtn.textContent = 'Enable audio';
      transportEl.appendChild(toggleBtn);
      transportEl.appendChild(restartBtn);
      transportEl.appendChild(audioToggleBtn);

      const infoEl = doc.createElement('div');
      infoEl.setAttribute?.('data-neoflash-role', 'preview-info');
      nameEl = doc.createElement('span');
      nameEl.setAttribute?.('data-neoflash-role', 'name');
      nameEl.textContent = title;
      statusEl = doc.createElement('span');
      statusEl.setAttribute?.('data-neoflash-role', 'status');
      audioStatusEl = doc.createElement('span');
      audioStatusEl.setAttribute?.('data-neoflash-role', 'audio-status');
      audioStatusEl.hidden = true;
      traceEl = doc.createElement('span');
      traceEl.setAttribute?.('data-neoflash-role', 'trace');
      infoEl.appendChild(nameEl);
      infoEl.appendChild(statusEl);
      infoEl.appendChild(audioStatusEl);
      infoEl.appendChild(traceEl);

      footerEl.appendChild(transportEl);
      footerEl.appendChild(infoEl);
      rootWrapper.appendChild(footerEl);
    }

    let loadingEl = null;
    if (!isClean && playerConfig?.preloader !== false) {
      loadingEl = doc.createElement('div');
      loadingEl.setAttribute?.('data-neoflash-role', 'loading');
      loadingEl.setAttribute?.('role', 'status');
      loadingEl.textContent = 'Loading project…';
      rootWrapper.appendChild(loadingEl);
    }

    const errorEl = doc.createElement('pre');
    errorEl.setAttribute?.('data-neoflash-role', 'error');
    errorEl.setAttribute?.('role', 'alert');
    if (errorEl.style) errorEl.style.display = 'none';
    rootWrapper.appendChild(errorEl);

    container.appendChild(rootWrapper);
    if (rootWrapper.style && !(rootWrapper.clientHeight > 0) && width > 0 && height > 0) {
      rootWrapper.style.aspectRatio = `${width} / ${height}`;
    }

    return {
      ownedRoot: rootWrapper,
      rootWrapper,
      viewport,
      stageSpace,
      stageSurface,
      canvas,
      semanticEl,
      a11yOverlay,
      loadingEl,
      errorEl,
      footerEl,
      transportEl,
      toggleBtn,
      restartBtn,
      zoomSelect,
      panBtn,
      fullscreenBtn,
      audioToggleBtn,
      audioStatusEl,
      reportToggleBtn,
      sizeReportEl,
      nameEl,
      statusEl,
      loadInfoEl,
      traceEl,
    };
  }

  function mount(container, options = {}) {
    return mountInstance(container, options || {});
  }

  const neoflashApi = Object.freeze({
    mount,
    sharedWasmModules,
  });
  if (global && typeof global === 'object') {
    try {
      Object.defineProperty(global, 'NeoFlash', { value: neoflashApi, configurable: true, enumerable: true, writable: true });
      Object.defineProperty(global, 'NeoFlashRuntime', { value: neoflashApi, configurable: true, enumerable: true, writable: true });
    } catch (_) {
      global.NeoFlash = neoflashApi;
      global.NeoFlashRuntime = neoflashApi;
    }
  }

  const helpers = { createScenePainter, localPoint, hitShape, oneBasedFrame, as2Frame, createAs2Methods, sourceKey, describeThrown, unwrapScriptThrow, functionBodyLineOffset, scriptFunction, mapScriptStack, sourceLocationText, mappedErrorFrames, formatTraceValue, formatTraceArgs, emitScriptMessage, completeCleanup, ensureBundleFontsLoaded, mountAfterBundleFonts, mount, sharedWasmModules };
  if (typeof module !== 'undefined' && module.exports) module.exports = helpers;
  if (global && typeof global === 'object') {
    try { Object.defineProperty(global, 'NeoFlashFormatTrace', { value: Object.freeze({ formatTraceValue, formatTraceArgs }), configurable: true }); } catch (_) {}
  }
  if (typeof document === 'undefined') return;
  global.NeoFlashPreview?.installOutputRelay?.(global);

  const errorElement = document.getElementById('error');
  let failed = false;
  let stopAsyncOnFailure = () => {};
  let sourceFrames = () => [];
  function fail(error) {
    // Cleanup and already queued host callbacks may fail too. Keep the first
    // authored failure visible until an explicit Restart, and do not recurse.
    if (failed) {
      global.__neoflashSuppressConsoleRelay = true;
      try { console.error(error); } finally { global.__neoflashSuppressConsoleRelay = false; }
      return;
    }
    failed = true;
    document.getElementById('loading').style.display = 'none';
    errorElement.style.display = 'block';
    const omitted = global.NeoFlashBundle?.publish?.omitted_modules || [];
    const hint = omitted.length ? `\n\nThis package was published at the Compact size level without: ${omitted.join('; ')}. If a script needs one of them, publish at the Balanced level.` : '';
    const frames = sourceFrames(error);
    const location = sourceLocationText(frames[0]);
    errorElement.textContent = 'NeoFlash could not run this project.\n\n' + (location ? location + '\n' : '') + describeThrown(error, true) + hint;
    global.__neoflashSuppressConsoleRelay = true;
    try {
      console.error(error);
      try { stopAsyncOnFailure(); } catch (cleanupError) { console.error(cleanupError); }
    } finally {
      global.__neoflashSuppressConsoleRelay = false;
    }
    emitScriptMessage('error', describeThrown(error), frames, typeof global === 'undefined' ? undefined : global, error);
  }

  function bundleFontDescriptors(cssRules) {
    const descriptors=[];
    const raw=typeof cssRules==='string'?cssRules:'';
    const fontFaceBodies=()=>{
      const bodies=[];
      const marker=/@font-face\b/gi;
      for(let match=marker.exec(raw);match;match=marker.exec(raw)){
        let open=marker.lastIndex;
        while(open<raw.length&&/\s/.test(raw[open]))open++;
        if(raw[open]!=='{')continue;
        let quote='',escaped=false,depth=1;
        for(let index=open+1;index<raw.length;index++){
          const ch=raw[index];
          if(quote){
            if(escaped){escaped=false;continue;}
            if(ch==='\\'){escaped=true;continue;}
            if(ch===quote)quote='';
            continue;
          }
          if(ch==='"'||ch==="'"){quote=ch;continue;}
          if(ch==='{'){depth++;continue;}
          if(ch==='}'&&--depth===0){
            bodies.push(raw.slice(open+1,index));
            marker.lastIndex=index+1;
            break;
          }
        }
      }
      return bodies;
    };
    const properties=body=>{
      const result=new Map();
      let start=0,quote='',escaped=false,parentheses=0;
      const add=end=>{
        const declaration=body.slice(start,end);
        const colon=declaration.indexOf(':');
        if(colon<0)return;
        const name=declaration.slice(0,colon).trim().toLowerCase();
        if(name&&!result.has(name))result.set(name,declaration.slice(colon+1).trim());
      };
      for(let index=0;index<body.length;index++){
        const ch=body[index];
        if(quote){
          if(escaped){escaped=false;continue;}
          if(ch==='\\'){escaped=true;continue;}
          if(ch===quote)quote='';
          continue;
        }
        if(ch==='"'||ch==="'"){quote=ch;continue;}
        if(ch==='('){parentheses++;continue;}
        if(ch===')'&&parentheses>0){parentheses--;continue;}
        if(ch===';'&&parentheses===0){add(index);start=index+1;}
      }
      add(body.length);
      return result;
    };
    const familyValue=value=>{
      let family=String(value||'').trim();
      const quote=family[0];
      if((quote==='"'||quote==="'")&&family.endsWith(quote)){
        const encoded=family.slice(1,-1);
        family='';
        for(let index=0;index<encoded.length;index++){
          if(encoded[index]==='\\'&&index+1<encoded.length)index++;
          family+=encoded[index];
        }
      }
      return family.trim();
    };
    const styleValue=value=>String(value||'normal').trim().toLowerCase()||'normal';
    const weightValue=value=>{
      const weight=String(value||'normal').trim().toLowerCase()||'normal';
      return weight==='normal'?'400':weight==='bold'?'700':weight;
    };
    for(const body of fontFaceBodies()){
      const property=properties(body);
      const family=familyValue(property.get('font-family'));
      if(!family)continue;
      const style=styleValue(property.get('font-style'));
      const weight=weightValue(property.get('font-weight'));
      const key=`${family.toLowerCase()}\u0000${style}\u0000${weight}`;
      if(!descriptors.some(item=>item.key===key))descriptors.push({family,style,weight,key});
    }
    return descriptors;
  }

  async function ensureBundleFontsLoaded(doc, cssRules) {
    if (!doc) return;
    const trimmed = typeof cssRules === 'string' ? cssRules.trim() : '';
    const descriptors=bundleFontDescriptors(trimmed);
    if (trimmed && doc.head) {
      const styles = typeof doc.querySelectorAll === 'function' ? Array.from(doc.querySelectorAll('style')) : [];
      const alreadyPresent = styles.some(el => typeof el.textContent === 'string' && el.textContent.includes(trimmed));
      if (!alreadyPresent && typeof doc.createElement === 'function') {
        const styleEl = doc.createElement('style');
        styleEl.setAttribute('data-neoflash-fonts', 'true');
        styleEl.textContent = trimmed;
        doc.head.appendChild(styleEl);
      }
    }
    const fontSet = doc.fonts;
    if (!fontSet || descriptors.length===0) return;
    const loads = [];
    const matchedDescriptors=new Set();
    const normalizeFamily=value=>{
      let family=String(value||'').trim();
      if((family.startsWith('"')&&family.endsWith('"'))||(family.startsWith("'")&&family.endsWith("'")))family=family.slice(1,-1);
      return family.replace(/\\([\\"'])/g,'$1').trim().toLowerCase();
    };
    const normalizeStyle=value=>String(value||'normal').trim().toLowerCase()||'normal';
    const normalizeWeight=value=>{
      const weight=String(value||'normal').trim().toLowerCase()||'normal';
      return weight==='normal'?'400':weight==='bold'?'700':weight;
    };
    if (typeof fontSet[Symbol.iterator] === 'function') {
      for (const face of fontSet) {
        const family=normalizeFamily(face?.family);
        const style=normalizeStyle(face?.style);
        const weight=normalizeWeight(face?.weight);
        const matches=descriptors.filter(item=>item.family.toLowerCase()===family&&item.style===style&&item.weight===weight);
        if(matches.length===0)continue;
        for(const descriptor of matches)matchedDescriptors.add(descriptor.key);
        if (face.status !== 'loaded' && typeof face.load === 'function') {
          loads.push(Promise.resolve().then(() => face.load()).catch(() => null));
        }
      }
    }
    if(typeof fontSet.load==='function'){
      for(const descriptor of descriptors){
        if(matchedDescriptors.has(descriptor.key))continue;
        const family=descriptor.family.replace(/([\\"])/g,'\\$1');
        loads.push(Promise.resolve().then(()=>fontSet.load(`${descriptor.style} ${descriptor.weight} 16px "${family}"`)).catch(()=>null));
      }
    }
    if (loads.length > 0) {
      await Promise.all(loads);
    }
  }

  function mountAfterBundleFonts(doc, childBundle, mount, isCurrent, afterFonts) {
    if(typeof mount!=='function')throw new TypeError('Published bundle mount callback is required.');
    const current=typeof isCurrent==='function'?isCurrent:()=>true;
    if(!current())return null;
    const cssRules=typeof childBundle?.fonts?.css_rules==='string'?childBundle.fonts.css_rules:'';
    if(!cssRules.trim())return mount();
    return ensureBundleFontsLoaded(doc,cssRules).then(()=>{
      if(!current())return null;
      if(typeof afterFonts==='function')afterFonts();
      return mount();
    });
  }

  async function boot() {
    return mountInstance(null, {
      __standalone: true,
      externalFail: fail,
      setExternalFailed: value => { failed = value; },
      setExternalStopAsync: fn => { stopAsyncOnFailure = fn; },
      setExternalSourceFrames: fn => { sourceFrames = fn; },
    });
  }

  async function mountInstance(containerTarget, mountOptions = {}) {
    const isStandaloneInstance = Boolean(mountOptions?.__standalone);
    let container = null;
    let instanceDoc = typeof document !== 'undefined' ? document : global.document;
    if (!isStandaloneInstance) {
      const lookupDoc = mountOptions?.document || instanceDoc;
      if (typeof containerTarget === 'string') {
        container = lookupDoc?.querySelector?.(containerTarget) || null;
        if (!container) {
          throw new Error(`NeoFlash.mount target container '${containerTarget}' was not found.`);
        }
      } else if (containerTarget && typeof containerTarget === 'object') {
        container = containerTarget;
      } else {
        throw new TypeError('NeoFlash.mount requires a target container element or CSS selector string.');
      }
      instanceDoc = mountOptions?.document || container.ownerDocument || lookupDoc;
      const previousMounted = mountedContainers.get(container);
      if (previousMounted && !previousMounted.disposed) {
        try { previousMounted.destroy(); } catch (_) {}
      }
    }

    const ownedListeners = [];
    function addOwnedListener(target, type, handler, listenerOpts) {
      if (!target || typeof target.addEventListener !== 'function') return;
      target.addEventListener(type, handler, listenerOpts);
      ownedListeners.push([target, type, handler, listenerOpts]);
    }

    let onPageHide = null;
    addOwnedListener(global, 'pagehide', event => onPageHide?.(event));

    const instanceBaseUrl = isStandaloneInstance ? '' : normalizeModuleBaseUrl(mountOptions?.baseUrl, instanceDoc);
    let rawBundle = isStandaloneInstance ? global.NeoFlashBundle : (mountOptions?.bundle || null);
    if (!isStandaloneInstance && !rawBundle) {
      const explicitBundleUrl = typeof mountOptions?.bundleUrl === 'string' && mountOptions.bundleUrl.trim()
        ? resolveModuleAssetUrl(mountOptions.bundleUrl.trim(), instanceDoc?.baseURI || global.location?.href || instanceBaseUrl)
        : (instanceBaseUrl ? resolveModuleAssetUrl('bundle.js', instanceBaseUrl) : '');
      if (explicitBundleUrl && typeof mountOptions?.baseUrl === 'string') {
        rawBundle = await fetchModuleBundle(explicitBundleUrl);
      } else {
        rawBundle = global.NeoFlashBundle || null;
      }
    }

    let bundle = rawBundle;
    if (!isStandaloneInstance && rawBundle && typeof rawBundle === 'object') {
      const clonedProject = rawBundle.project ? {
        ...rawBundle.project,
        resources: Array.isArray(rawBundle.project.resources) ? rawBundle.project.resources.slice() : [],
      } : null;
      const resolvedAssetUrls = {};
      if (rawBundle.asset_urls && typeof rawBundle.asset_urls === 'object') {
        for (const [resPath, relUrl] of Object.entries(rawBundle.asset_urls)) {
          resolvedAssetUrls[resPath] = resolveModuleAssetUrl(relUrl, instanceBaseUrl);
        }
      }
      if (Array.isArray(rawBundle.video_manifest)) {
        for (const entry of rawBundle.video_manifest) {
          const rel = entry?.source?.url_or_path;
          if (typeof rel === 'string' && rel && !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(rel) && !resolvedAssetUrls[rel]) {
            resolvedAssetUrls[rel] = resolveModuleAssetUrl(rel, instanceBaseUrl);
          }
        }
      }
      const resolvedSpriteSheet = rawBundle.sprite_sheet ? {
        ...rawBundle.sprite_sheet,
        image_url: resolveModuleAssetUrl(rawBundle.sprite_sheet.image_url, instanceBaseUrl),
        manifest_url: resolveModuleAssetUrl(rawBundle.sprite_sheet.manifest_url, instanceBaseUrl),
      } : null;
      const basePlayerCfg = rawBundle.player_config || {};
      const baseChromeCfg = basePlayerCfg.chrome || {};
      const cleanFlag = mountOptions?.clean !== undefined ? Boolean(mountOptions.clean) : true;
      const autoplayFlag = mountOptions?.autoplay !== undefined
        ? Boolean(mountOptions.autoplay)
        : (basePlayerCfg.autoplay !== false);
      const loopFlag = mountOptions?.loop !== undefined
        ? Boolean(mountOptions.loop)
        : (basePlayerCfg.loop_timeline !== false);
      let resolvedWasmUrl = rawBundle.wasm_url || null;
      if (typeof mountOptions?.wasmUrl === 'string' && mountOptions.wasmUrl.trim()) {
        resolvedWasmUrl = resolveModuleAssetUrl(mountOptions.wasmUrl.trim(), instanceDoc?.baseURI || global.location?.href || instanceBaseUrl);
      } else if (rawBundle.runtime_id && typeof mountOptions?.runtimeUrl === 'string' && mountOptions.runtimeUrl.trim()) {
        const rtDir = normalizeModuleBaseUrl(mountOptions.runtimeUrl.trim(), instanceDoc);
        resolvedWasmUrl = rtDir ? `${rtDir}neoflash_web_bg.wasm` : resolvedWasmUrl;
      } else if (typeof resolvedWasmUrl === 'string' && resolvedWasmUrl) {
        resolvedWasmUrl = resolveModuleAssetUrl(resolvedWasmUrl, instanceBaseUrl);
      }
      const effectiveWasmB64 = typeof mountOptions?.wasmBase64 === 'string' && mountOptions.wasmBase64
        ? mountOptions.wasmBase64
        : rawBundle.wasm;
      bundle = {
        ...rawBundle,
        project: clonedProject,
        scripts: Array.isArray(rawBundle.scripts) ? rawBundle.scripts.map(s => ({ ...s })) : [],
        program: rawBundle.program ? { ...rawBundle.program } : undefined,
        warnings: Array.isArray(rawBundle.warnings) ? rawBundle.warnings.slice() : [],
        wasm: effectiveWasmB64,
        wasm_url: resolvedWasmUrl,
        sprite_sheet: resolvedSpriteSheet,
        asset_urls: Object.keys(resolvedAssetUrls).length > 0 ? resolvedAssetUrls : (rawBundle.asset_urls || null),
        player_config: {
          ...basePlayerCfg,
          autoplay: autoplayFlag,
          loop_timeline: loopFlag,
          include_trace: cleanFlag ? basePlayerCfg.include_trace : true,
          chrome: cleanFlag
            ? { ...baseChromeCfg, clean: true, show_fps: false, show_dimensions: false, show_controls: false, show_load_summary: false, show_trace: false }
            : (baseChromeCfg.clean
                ? { ...baseChromeCfg, clean: false, show_fps: true, show_dimensions: true, show_controls: true, show_load_summary: true, show_trace: true }
                : { ...baseChromeCfg, clean: false }),
        },
      };
    }

    const baseWasmBindings = mountOptions?.wasmBindings || (typeof NeoFlashWasm !== 'undefined' ? NeoFlashWasm : global.NeoFlashWasm);
    const hasWasmFactory = !isStandaloneInstance && typeof global.NeoFlashWasmFactory === 'function' && !mountOptions?.wasmBindings;
    if (!bundle || (typeof baseWasmBindings !== 'function' && !hasWasmFactory)) {
      throw new Error('The HTML5 runtime is incomplete. Re-export this project from NeoFlash.');
    }
    if (bundle.runtime_id && global.NeoFlashRuntimeId !== bundle.runtime_id) {
      throw new Error(`This project was published against NeoFlash runtime ${bundle.runtime_id}, but the shared runtime provides ${global.NeoFlashRuntimeId || 'none'}. Publish the shared runtime again from the NeoFlash build that published this project.`);
    }

    const instanceWasm = hasWasmFactory ? global.NeoFlashWasmFactory() : baseWasmBindings;
    const playerConfig = bundle.player_config || {};
    const chromeConfig = playerConfig.chrome || {};
    const isDomModeBundle = playerConfig.render_mode === 'dom' || bundle?.publish?.renderMode === 'dom';
    const isCleanMount = chromeConfig.clean === true || (
      chromeConfig.show_fps === false &&
      chromeConfig.show_dimensions === false &&
      chromeConfig.show_controls === false &&
      chromeConfig.show_load_summary === false &&
      chromeConfig.show_trace === false
    );

    let scopedDom = null;
    if (!isStandaloneInstance) {
      scopedDom = buildScopedModuleDom(instanceDoc, container, bundle.project, playerConfig, isDomModeBundle, isCleanMount);
    }

    if (playerConfig.preloader === false) {
      const loadingTarget = isStandaloneInstance ? instanceDoc?.getElementById?.('loading') : scopedDom?.loadingEl;
      if (loadingTarget?.style) loadingTarget.style.display = 'none';
    }

    let activePlayer = null;
    const cleanupEarlySetup = () => {
      for (const [target, type, handler, listenerOpts] of ownedListeners.splice(0, ownedListeners.length)) {
        try { target.removeEventListener?.(type, handler, listenerOpts); } catch (_) {}
      }
      if (scopedDom?.ownedRoot) {
        try {
          if (typeof scopedDom.ownedRoot.remove === 'function') scopedDom.ownedRoot.remove();
          else if (scopedDom.ownedRoot.parentNode) scopedDom.ownedRoot.parentNode.removeChild(scopedDom.ownedRoot);
        } catch (_) {}
        scopedDom.ownedRoot = null;
      }
      if (activePlayer) {
        const p = activePlayer;
        activePlayer = null;
        try { p.free?.(); } catch (_) {}
      }
    };

    try {
      const canCacheWasmModule = typeof WebAssembly !== 'undefined'
        && typeof WebAssembly.compile === 'function'
        && typeof instanceWasm?.initSync === 'function';
      if (canCacheWasmModule) {
        let compiledModule;
        if (typeof bundle.wasm === 'string') {
          compiledModule = await compileWasmBase64Once(bundle.wasm);
        } else if (global.location?.protocol === 'file:' && !/^https?:/i.test(bundle.wasm_url || '') && typeof global.NeoFlashWasmBase64 === 'string') {
          compiledModule = await compileWasmBase64Once(global.NeoFlashWasmBase64);
        } else {
          if (global.location?.protocol === 'file:' && !/^https?:/i.test(bundle.wasm_url || '')) {
            throw new Error('This export loads its WebAssembly runtime from a separate file, which browsers block for pages opened from disk. Serve the folder over HTTP(S), or publish at the Full size level to open index.html directly.');
          }
          try {
            compiledModule = await compileWasmUrlOnce(bundle.wasm_url);
          } catch (wasmErr) {
            if (typeof global.NeoFlashWasmBase64 === 'string') {
              compiledModule = await compileWasmBase64Once(global.NeoFlashWasmBase64);
            } else {
              throw wasmErr;
            }
          }
        }
        let initPromise = wasmInitPromises.get(instanceWasm);
        if (!initPromise) {
          initPromise = Promise.resolve(instanceWasm({ module_or_path: compiledModule })).catch(err => {
            wasmInitPromises.delete(instanceWasm);
            throw err;
          });
          wasmInitPromises.set(instanceWasm, initPromise);
        }
        await initPromise;
      } else if (typeof bundle.wasm === 'string') {
        await instanceWasm({ module_or_path: Uint8Array.from(atob(bundle.wasm), character => character.charCodeAt(0)) });
      } else if (global.location?.protocol === 'file:' && !/^https?:/i.test(bundle.wasm_url || '') && typeof global.NeoFlashWasmBase64 === 'string') {
        await instanceWasm({ module_or_path: Uint8Array.from(atob(global.NeoFlashWasmBase64), character => character.charCodeAt(0)) });
      } else {
        // Smaller size levels ship WebAssembly as its own file, which pages opened from disk cannot fetch without a script fallback.
        if (global.location?.protocol === 'file:' && !/^https?:/i.test(bundle.wasm_url || '')) {
          throw new Error('This export loads its WebAssembly runtime from a separate file, which browsers block for pages opened from disk. Serve the folder over HTTP(S), or publish at the Full size level to open index.html directly.');
        }
        try {
          await instanceWasm({ module_or_path: bundle.wasm_url });
        } catch (wasmErr) {
          if (typeof global.NeoFlashWasmBase64 === 'string') {
            await instanceWasm({ module_or_path: Uint8Array.from(atob(global.NeoFlashWasmBase64), character => character.charCodeAt(0)) });
          } else {
            throw wasmErr;
          }
        }
      }
    } catch (wasmBootErr) {
      cleanupEarlySetup();
      throw wasmBootErr;
    }

    const project = bundle.project;
    const bundleAssetUrls = bundle.asset_urls || null;
    const bundleResourceAliases = bundle.resource_aliases || null;
    const needsPublishedValidator = Boolean(bundle.sprite_sheet || (bundleAssetUrls && Object.keys(bundleAssetUrls).length > 0));
    // Published bundles carry a flattened main timeline plus its scene ranges.
    const scenesJson = bundle.scenes ? JSON.stringify(bundle.scenes) : undefined;
    let player;
    try {
      player = (needsPublishedValidator && typeof instanceWasm.WebPlayer.new_published === 'function')
        ? instanceWasm.WebPlayer.new_published(JSON.stringify(project), scenesJson)
        : new instanceWasm.WebPlayer(JSON.stringify(project), scenesJson);
      activePlayer = player;
      if (bundle.sprite_sheet && typeof player.hydrate_sprite_sheet === 'function') {
        if (global.location?.protocol === 'file:' && !/^https?:/i.test(bundle.sprite_sheet.image_url || '')) {
          throw new Error('This export packs bitmaps into external sprite sheets, which browsers block over file://. Serve the folder over HTTP(S), or disable Sprite sheets in Publish settings for direct file opening.');
        }
        const [sheetImgResp, sheetMetaResp] = await Promise.all([
          global.fetch(bundle.sprite_sheet.image_url),
          global.fetch(bundle.sprite_sheet.manifest_url),
        ]);
        if (!sheetImgResp.ok || !sheetMetaResp.ok) {
          throw new Error(`Failed to load published sprite sheet (${bundle.sprite_sheet.image_url}: ${sheetImgResp.status}, ${bundle.sprite_sheet.manifest_url}: ${sheetMetaResp.status}).`);
        }
        const [sheetPngBuf, sheetManifestText] = await Promise.all([
          sheetImgResp.arrayBuffer(),
          sheetMetaResp.text(),
        ]);
        player.hydrate_sprite_sheet(new Uint8Array(sheetPngBuf), sheetManifestText);
      }
    } catch (playerBootErr) {
      cleanupEarlySetup();
      throw playerBootErr;
    }

    const canvas = isStandaloneInstance ? instanceDoc.getElementById('stage') : scopedDom.canvas;
    const accelMode = playerConfig.acceleration || canvas?.getAttribute?.('data-acceleration') || canvas?.dataset?.acceleration || '';
    if (accelMode && canvas?.dataset) canvas.dataset.acceleration = accelMode;
    const contextAttrs = accelMode === 'gpu_web_gl'
      ? { alpha: false, desynchronized: true }
      : accelMode === 'software'
        ? { alpha: false, willReadFrequently: true }
        : accelMode === 'canvas2d'
          ? { alpha: false }
          : undefined;
    const context = canvas ? ((contextAttrs ? canvas.getContext('2d', contextAttrs) : null) || canvas.getContext('2d')) : null;
    if (!context) {
      cleanupEarlySetup();
      throw new Error('This browser does not provide Canvas 2D.');
    }
    if (!global.NeoFlashPaths) {
      cleanupEarlySetup();
      throw new Error('The vector path runtime is missing. Re-export this project.');
    }
    const paths = new global.NeoFlashPaths.PathCache();
    const gradients = new global.NeoFlashPaths.GradientRenderer();
    const clippings = new global.NeoFlashPaths.ClipRenderer();
    const status = isStandaloneInstance ? instanceDoc.getElementById('status') : scopedDom.statusEl;
    const toggle = isStandaloneInstance ? instanceDoc.getElementById('toggle') : scopedDom.toggleBtn;
    const traceElement = isStandaloneInstance ? instanceDoc.getElementById('trace') : scopedDom.traceEl;
    const viewport = isStandaloneInstance ? instanceDoc.getElementById('viewport') : scopedDom.viewport;
    const stageSpace = isStandaloneInstance ? instanceDoc.getElementById('stage-space') : scopedDom.stageSpace;
    const stageSurface = isStandaloneInstance
      ? (instanceDoc.getElementById('stage-surface') || instanceDoc.getElementById('stage-wrap'))
      : scopedDom.stageSurface;
    const semanticTargetEl = isStandaloneInstance ? instanceDoc.getElementById('semantic-layer') : scopedDom.semanticEl;
    const audioToggleButton = isStandaloneInstance ? instanceDoc.getElementById?.('audio-toggle') : scopedDom.audioToggleBtn;
    const audioStatusLabel = isStandaloneInstance ? instanceDoc.getElementById?.('audio-status') : scopedDom.audioStatusEl;
    const restartButton = isStandaloneInstance ? instanceDoc.getElementById?.('restart') : scopedDom.restartBtn;
    const zoomSelect = isStandaloneInstance ? instanceDoc.getElementById?.('zoom') : scopedDom.zoomSelect;
    const panButton = isStandaloneInstance ? instanceDoc.getElementById?.('pan') : scopedDom.panBtn;
    const fullscreenToggle = isStandaloneInstance ? instanceDoc.getElementById?.('fullscreen-toggle') : scopedDom.fullscreenBtn;
    const footerElement = isStandaloneInstance ? instanceDoc.querySelector?.('footer') : scopedDom.footerEl;
    const nameElement = isStandaloneInstance ? instanceDoc.getElementById?.('name') : scopedDom.nameEl;
    const transportElement = isStandaloneInstance ? instanceDoc.querySelector?.('.transport') : scopedDom.transportEl;
    const loadInfoElement = isStandaloneInstance ? instanceDoc.getElementById?.('load-info') : scopedDom.loadInfoEl;
    const reportToggle = isStandaloneInstance ? instanceDoc.getElementById?.('report-toggle') : scopedDom.reportToggleBtn;
    const sizeReportElement = isStandaloneInstance ? instanceDoc.getElementById?.('size-report') : scopedDom.sizeReportEl;
    const loadingElement = isStandaloneInstance ? instanceDoc.getElementById?.('loading') : scopedDom.loadingEl;
    const instanceErrorElement = isStandaloneInstance ? errorElement : scopedDom.errorEl;
    const instanceEventHost = isStandaloneInstance ? global : (scopedDom?.rootWrapper || container || global);
    const instanceOnTrace = typeof mountOptions?.onTrace === 'function' ? mountOptions.onTrace : null;
    const instanceOnError = typeof mountOptions?.onError === 'function' ? mountOptions.onError : null;
    const instanceParameters = (!isStandaloneInstance && mountOptions?.parameters && typeof mountOptions.parameters === 'object')
      ? { ...mountOptions.parameters }
      : null;
    const instanceRootUrl = (!isStandaloneInstance && instanceBaseUrl)
      ? resolveModuleAssetUrl('bundle.js', instanceBaseUrl)
      : null;
    const instanceKeyTarget = isStandaloneInstance
      ? (typeof global.addEventListener === 'function' ? global : null)
      : (viewport || (typeof global.addEventListener === 'function' ? global : null));
    const domHostDocument = (!isStandaloneInstance && instanceDoc && typeof Proxy === 'function')
      ? new Proxy(instanceDoc, {
          get(target, prop) {
            if (prop === 'getElementById') {
              return id => id === 'stage-dom-root'
                ? (stageSurface?.querySelector?.('neoflash-stage[data-render-mode="dom"]') || null)
                : target.getElementById?.(id) || null;
            }
            if (prop === 'body' && scopedDom?.rootWrapper) {
              return scopedDom.rootWrapper;
            }
            const val = Reflect.get(target, prop, target);
            return typeof val === 'function' ? val.bind(target) : val;
          },
        })
      : instanceDoc;

    let failed = false;
    let stopAsyncOnFailure = () => {};
    let sourceFrames = () => [];
    const fail = error => {
      if (isStandaloneInstance && typeof mountOptions.externalFail === 'function') {
        failed = true;
        mountOptions.externalFail(error);
        if (instanceOnError) {
          try { instanceOnError(error); } catch (_) {}
        }
        return;
      }
      if (failed) {
        global.__neoflashSuppressConsoleRelay = true;
        try { console.error(error); } finally { global.__neoflashSuppressConsoleRelay = false; }
        return;
      }
      failed = true;
      if (loadingElement?.style) loadingElement.style.display = 'none';
      if (instanceErrorElement) {
        if (instanceErrorElement.style) instanceErrorElement.style.display = 'block';
        const omitted = bundle?.publish?.omitted_modules || [];
        const hint = omitted.length ? `\n\nThis package was published at the Compact size level without: ${omitted.join('; ')}. If a script needs one of them, publish at the Balanced level.` : '';
        const frames = sourceFrames(error);
        const location = sourceLocationText(frames[0]);
        instanceErrorElement.textContent = 'NeoFlash could not run this project.\n\n' + (location ? location + '\n' : '') + describeThrown(error, true) + hint;
      }
      global.__neoflashSuppressConsoleRelay = true;
      try {
        console.error(error);
        try { stopAsyncOnFailure(); } catch (cleanupError) { console.error(cleanupError); }
      } finally {
        global.__neoflashSuppressConsoleRelay = false;
      }
      if (instanceOnError) {
        try { instanceOnError(error); } catch (_) {}
      }
      const frames = sourceFrames(error);
      emitScriptMessage('error', describeThrown(error), frames, instanceEventHost, error);
    };

    const semanticLayer = new global.NeoFlashAccessibility.SemanticLayer(semanticTargetEl, node => {
      if (failed) return;
      try {activateSemantic(node);} catch (error) {fail(error);}
    }, {onFocus:semanticNativeFocus,onBlur:semanticNativeBlur});
    let a11yOverlay = isStandaloneInstance ? instanceDoc.getElementById('stage-a11y-overlay') : scopedDom.a11yOverlay;
    if (!a11yOverlay && typeof instanceDoc?.createElement === 'function') {
      a11yOverlay = instanceDoc.createElement('div');
      if (isStandaloneInstance) a11yOverlay.id = 'stage-a11y-overlay';
      a11yOverlay.className = 'neoflash-a11y-overlay';
      if (a11yOverlay.style) {
        a11yOverlay.style.position = 'absolute';
        a11yOverlay.style.inset = '0';
        a11yOverlay.style.pointerEvents = 'none';
        a11yOverlay.style.overflow = 'hidden';
      }
      stageSurface?.appendChild?.(a11yOverlay);
    }
    const preview = new global.NeoFlashPreview.Preview(viewport, stageSpace, stageSurface, canvas, project, () => {
      if (!failed) { try { render(); } catch (error) { fail(error); } }
    });
    const functions = new Map();
    const sourceUnits = (bundle.scripts || []).concat(bundle.program ? [bundle.program] : []);
    const bodyOffset = functionBodyLineOffset();
    sourceFrames = error => mappedErrorFrames(error, sourceUnits, bodyOffset);
    if (isStandaloneInstance) mountOptions.setExternalSourceFrames?.(sourceFrames);
    const proxies = new Map();
    let scene = [];
    let lastTime = null;
    let accumulated = 0;
    let lastTrace = '', lastTraceLocation = '';
    const bindings = ['event', '_root', '_parent', 'trace', 'play', 'stop', 'gotoAndPlay', 'gotoAndStop', 'nextFrame', 'prevFrame', '__nfAs2Method', '__nfAs2Value', '__nfProgram', '__nfDisplay', '__nfLegacy', '__nfXML'];
    let compiledProgramFn = null;
    try {
      compiledProgramFn = bundle.program ? (() => {
        try { return compileProgramUnit(bundle.program); }
        catch (error) { throw new Error(`Project script syntax error: ${describeThrown(error)}`, { cause: error }); }
      })() : null;
      for (const script of (bundle.scripts || [])) {
        try {
          functions.set(sourceKey(script), {
            fn: scriptFunction(script, bindings),
            location: script.location,
          });
        } catch (error) { throw new Error(`Script syntax error in ${script.location}: ${describeThrown(error)}`, { cause: error }); }
      }
    } catch (compileErr) {
      cleanupEarlySetup();
      throw compileErr;
    }
    global.__neoflashSuppressConsoleRelay = true;
    try {
      for (const warning of (bundle.warnings || [])) console.warn('NeoFlash:', warning);
    } finally {
      global.__neoflashSuppressConsoleRelay = false;
    }

    const propertyMap = {
      x: 'x', y: 'y', rotation: 'rotation', alpha: 'alpha', scaleX: 'scale_x', scaleY: 'scale_y',
      _x: 'x', _y: 'y', _rotation: 'rotation', _alpha: 'alpha', _xscale: 'scale_x', _yscale: 'scale_y',
    };
    const resolveRuntimeWasm = () => (typeof instanceWasm !== 'undefined' && instanceWasm) ? instanceWasm : (typeof NeoFlashWasm === 'undefined' ? global.NeoFlashWasm : NeoFlashWasm);
    const graphMode=typeof player.runtime_snapshot_json==='function';
    const hasLifecycleApi = typeof player.enable_lifecycle === 'function' && typeof player.lifecycle_next_step_json === 'function';
    const references = new WeakMap();
    const attachedClasses = new Map();
    const constructedNodes = new Map();
    const superConstructedNodes = new Map();
    const constructedNames = new WeakMap();
    const offstageLinkedTargets = new Set();
    const retiringTargets = new Set();
    const pendingRetirementRefs = new Map();
    const constructSessions = [];
    const frameLabelDispatchedScopes = new Set();
    let knownResetGeneration = 0;
    const boundTextVariables = new Map();
    const registeredClasses = new Map();
    const executedInitClips = new Set();
    const clipFrameLabels = new Map();
    let callFrameDepth = 0, inDispatch = false;
    let display = null, eventHub = null, stageHost = null, legacyHost = null, legacyGlobals = null, timerHost = null, binaryHost = null, netHost = null, bitmapHost = null, textHost = null, filterHost = null, audioHost = null, videoHost = null, xmlHost = null, loaderHost = null, amfHost = null, storageHost = null, textInputHost = null, externalHost = null, program = null, as2PlatformHost = null, as2ObjectHost = null, as2TextHost = null, as2DataHost = null, as2BindingHost = null, as2NetworkHost = null, as2FilesHost = null, as2LocaleHost = null, as2MediaHost = null, as2ServicesHost = null, as2TransitionsHost = null, as2DepthManagerHost = null, as2StageHost = null, as2CaptureHost = null, as2ConnectionsHost = null, as2BoxingHost = null, as2PrintHost = null;
    let geometryHost = null, geometry3dHost = null, easingHost = null, broadcasterHost = null, airHost = null, as3NetSysHost = null, fte3dHost = null, as3MediaEventsHost = null, domStageHost = null, currentBuiltins = null;
    let frameLabelType = null, sceneValueFactory = null;
    const consoleFormatHook = args => ({
      message: formatTraceArgs(args, { program }),
      location: sourceFrames(new Error())[0] || null,
    });
    if (global && typeof global === 'object' && (typeof isStandaloneInstance === 'undefined' || isStandaloneInstance)) {
      global.__neoflashConsoleFormat = consoleFormatHook;
    }
    const timelineMetadataProperties=Object.freeze({currentLabel:'current_label',currentFrameLabel:'current_frame_label',currentLabels:'labels',framesLoaded:'frames_loaded',isPlaying:'is_playing',currentScene:'scene_name',scenes:'scene_name'});
    let eventFamilies = {};
    let timerRenderGeneration=0, timerRenderRafId=0, familyDisposed=false, capturePainter=null, islandPainter=null, inputPaths=null;
    let legacyCursorVisible = true, pointerOverContent = false;
    let runtimeFrameRate = typeof project === 'undefined' ? 24 : project.fps;
    const hostPlayerConfig = typeof bundle !== 'undefined' && bundle?.player_config ? bundle.player_config : {};
    const isDomRenderMode = hostPlayerConfig.render_mode === 'dom' || (typeof bundle !== 'undefined' && bundle?.publish?.renderMode === 'dom');
    const initialFlashScaleMode = hostPlayerConfig.scale_mode === 'no_scale' ? 'noScale'
      : hostPlayerConfig.scale_mode === 'exact_fit' ? 'exactFit'
      : hostPlayerConfig.scale_mode === 'fill_crop' ? 'noBorder'
      : 'showAll';
    const alignCodeMap = { center: '', top: 'T', bottom: 'B', left: 'L', right: 'R', top_left: 'TL', top_right: 'TR', bottom_left: 'BL', bottom_right: 'BR' };
    const initialFlashAlign = Object.hasOwn(alignCodeMap, hostPlayerConfig.stage_align || '') ? alignCodeMap[hostPlayerConfig.stage_align] : '';
    const allowFullscreen = hostPlayerConfig.allow_fullscreen !== false;
    const initialViewportOverflow = typeof viewport !== 'undefined' ? viewport?.style?.overflow : undefined;
    let hostStageLayoutPinned = false;
    function ownsViewportFullscreen() {
      const docRef = typeof instanceDoc !== 'undefined' && instanceDoc ? instanceDoc : (typeof document === 'undefined' ? global.document : document);
      const fsEl = docRef?.fullscreenElement || docRef?.webkitFullscreenElement || null;
      return Boolean(typeof viewport !== 'undefined' && viewport && fsEl === viewport);
    }
    function shouldAutoEnableStageLayout() {
      const chromeCfg = hostPlayerConfig.chrome || {};
      const clean = chromeCfg.clean === true || (chromeCfg.show_fps === false && chromeCfg.show_dimensions === false && chromeCfg.show_controls === false && chromeCfg.show_load_summary === false && chromeCfg.show_trace === false);
      return Boolean(isDomRenderMode || clean || hostPlayerConfig.template === 'fullscreen' || (hostPlayerConfig.stage_align && hostPlayerConfig.stage_align !== 'center') || (hostPlayerConfig.scale_mode && hostPlayerConfig.scale_mode !== 'fit_keep_ratio'));
    }
    function computeHostStageLayout(vw, vh) {
      let docW = typeof project === 'undefined' ? 800 : project.width;
      let docH = typeof project === 'undefined' ? 600 : project.height;
      let mode = stageHost?.stage?.scaleMode || initialFlashScaleMode;
      let align = stageHost?.stage?.align ?? initialFlashAlign;
      if (domStageHost?.isActive?.()) {
        const bpOut = domStageHost.evaluateViewport(vw, vh, { docW, docH, scaleMode: mode, align });
        if (bpOut) { docW = bpOut.docW; docH = bpOut.docH; mode = bpOut.scaleMode; align = bpOut.align; }
      }
      const fsRect = stageHost?.fullScreenSourceRect?.() || null;
      const isFs = ownsViewportFullscreen() || Boolean(stageHost?.stage?.displayState && stageHost.stage.displayState !== 'normal');
      const next = global.NeoFlashStage.computeStageLayout(docW, docH, vw, vh, {
        scaleMode: mode,
        align,
        fullscreen: isFs,
        fullScreenSourceRect: fsRect,
      });
      if (stageHost) {
        const wantW = fsRect && isFs ? docW : next.logicalWidth;
        const wantH = fsRect && isFs ? docH : next.logicalHeight;
        if (stageHost.stage.stageWidth !== wantW || stageHost.stage.stageHeight !== wantH) {
          queueMicrotask(() => { if ((typeof failed === 'undefined' || !failed) && stageHost) stageHost.resize(wantW, wantH); });
        }
      }
      if (as2StageHost && !as2StageHost.isActive()) {
        try { as2StageHost.syncPassiveLayout?.(vw, vh); } catch (_) {}
      }
      return next;
    }
    function enableHostStageLayout(pin = true) {
      if (pin) hostStageLayoutPinned = true;
      if (as2StageHost?.isActive?.()) return;
      if (typeof preview !== 'undefined' && preview) {
        if (typeof viewport !== 'undefined' && viewport?.style) viewport.style.overflow = 'hidden';
        preview.setStageLayout?.(computeHostStageLayout);
      }
    }
    function syncInitialStageLayout() {
      hostStageLayoutPinned = shouldAutoEnableStageLayout();
      if (hostStageLayoutPinned || ownsViewportFullscreen()) enableHostStageLayout(hostStageLayoutPinned);
      else if (!as2StageHost?.isActive?.() && typeof preview !== 'undefined' && preview) {
        preview.setStageLayout?.(null);
        if (typeof viewport !== 'undefined' && viewport?.style) viewport.style.overflow = initialViewportOverflow;
      }
    }
    function logicalSize(){return [typeof preview==='undefined'||!preview?.view?typeof project==='undefined'?800:project.width:preview.view.logicalWidth||project.width,typeof preview==='undefined'||!preview?.view?typeof project==='undefined'?600:project.height:preview.view.logicalHeight||project.height];}
    function stageOrigin(){return [typeof preview==='undefined'||!preview?.view?0:preview.view.originX||0,typeof preview==='undefined'||!preview?.view?0:preview.view.originY||0];}
    function stagePointFromEvent(event){
      if(typeof canvas==='undefined'||!canvas)return null;
      const r=canvas.getBoundingClientRect();
      if(!(r.width>0&&r.height>0))return null;
      const [width,height]=logicalSize(),[ox,oy]=stageOrigin();
      const vx=(event.clientX-r.left)*width/r.width,vy=(event.clientY-r.top)*height/r.height;
      return {x:vx-ox,y:vy-oy,inside:vx>=0&&vy>=0&&vx<=width&&vy<=height};
    }
    function updateCursor() {
      if(familyDisposed||typeof canvas==='undefined')return;
      const as3Mouse=stageHost?.mouseState?.();
      if(!legacyCursorVisible||(as3Mouse&&!as3Mouse.visible)){canvas.style.cursor='none';return;}
      if(typeof preview!=='undefined'&&preview.panMode){canvas.style.cursor='grab';return;}
      const cursor=as3Mouse?.cursor||'auto';
      canvas.style.cursor=cursor==='arrow'?'default':cursor==='button'?'pointer':cursor==='hand'?'grab':cursor==='ibeam'?'text':pointerOverContent?'pointer':'default';
    }
    function inputFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Input callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else throw failure;
    }
    function cancelTimerRender() {
      const pending=timerRenderRafId;
      timerRenderRafId = 0;
      if (pending && typeof global.cancelAnimationFrame === 'function') global.cancelAnimationFrame(pending);
    }
    function stopTimers() {
      timerRenderGeneration++;
      completeCleanup([cancelTimerRender, () => easingHost?.reset?.(), () => broadcasterHost?.reset?.(), () => timerHost?.reset()]);
    }
    function flushStageRender() {
      if(!stageHost)return;
      if(display&&(typeof player.runtime_snapshot_json==='function'||typeof player.display_nodes_json==='function'))stageHost.flushRender(()=>display.nodes().map(node=>display.nodeProxy(node)));
      else stageHost.flushRender([]);
    }
    function afterTimerEvent() {if(familyDisposed||inDispatch||(typeof failed!=='undefined'&&failed))return;dispatchScripts();flushStageRender();if(typeof context!=='undefined')render();}
    function timerFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Timer callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {try { stopTimers(); } finally { throw failure; }}
    }
    function requestTimerRender() {
      if(familyDisposed||timerRenderRafId)return;
      const generation=timerRenderGeneration;
      timerRenderRafId=global.requestAnimationFrame(()=>{
        timerRenderRafId=0;
        if(familyDisposed||generation!==timerRenderGeneration||(typeof failed!=='undefined'&&failed))return;
        try {afterTimerEvent();}catch(error){timerFailure(error);}
      })||0;
    }
    function netFailure(error) {
      const cause=unwrapScriptThrow(error),failure=new Error(`Resource callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {try { completeCleanup([() => as3MediaEventsHost?.reset(), () => fte3dHost?.reset(), () => as3NetSysHost?.reset(), () => airHost?.reset(), () => externalHost?.reset(), () => domStageHost?.reset(), () => textInputHost?.reset(), () => storageHost?.reset(), () => amfHost?.reset?.(), () => loaderHost?.reset(), () => xmlHost?.reset(), () => videoHost?.reset(), () => as2CaptureHost?.reset(), () => audioHost?.reset(), () => netHost?.reset(), () => stopTimers()]); } finally { throw failure; }}
    }
    function audioFailure(error){
      const cause=unwrapScriptThrow(error),failure=new Error(`Audio callback: ${describeThrown(cause)}`,{cause});
      if(typeof fail==='function')fail(failure);else {try { audioHost?.reset(); } finally { throw failure; }}
    }
    function updateAudioStatus(){
      const activeDoc=typeof instanceDoc!=='undefined'&&instanceDoc?instanceDoc:(typeof document==='undefined'?null:document);
      if(!activeDoc)return;
      const videoState=videoHost?.status();
      const states=[audioHost?.status(),videoState?.audible===false?null:videoState].filter(value=>value&&value.state!=='idle');
      const effectivelyUnlocked=value=>Boolean(value.unlocked||value.requiresGesture===false);
      const state={state:states.some(value=>value.state==='awaiting_gesture')?'awaiting_gesture':states.some(value=>value.state==='loading')?'loading':states.length?'ready':'idle',unlocked:states.every(effectivelyUnlocked),muted:states.length>0&&states.every(value=>value.muted),pending:states.reduce((n,value)=>n+value.pending,0),active:states.reduce((n,value)=>n+value.active,0),loading:states.reduce((n,value)=>n+value.loading,0),error:states.find(value=>value.error)?.error};
      const button=typeof audioToggleButton!=='undefined'?audioToggleButton:activeDoc.getElementById?.('audio-toggle'),label=typeof audioStatusLabel!=='undefined'?audioStatusLabel:activeDoc.getElementById?.('audio-status');if(!button||!label)return;
      button.hidden=label.hidden=state.state==='idle';
      button.textContent=!state.unlocked||state.state==='awaiting_gesture'?'Enable audio':state.muted?'Unmute audio':'Mute audio';
      button.setAttribute('aria-pressed',String(state.muted));
      label.textContent=state.state==='awaiting_gesture'?`Audio waiting for a gesture · ${state.pending} pending`:state.error?`Audio: ${state.error}`:state.state==='loading'?`Loading audio · ${state.loading}`:state.muted?'Audio muted':state.active?`Audio playing · ${state.active}`:'Audio ready';
    }
    function assertUnfilteredEditing(target,addingTo=null){
      if(!target||target===stageHost?.stage||!display?.alive(target))return;
      const nodes=display.nodes(),node=display.descriptor(target,nodes);if(node.owner!=='text'||(node.text?.field_type!=='input'&&node.text?.selectable===false))return;
      const addedKey=addingTo?references.get(addingTo)?.key:null,byRef=new Map(nodes.map(node=>[JSON.stringify(node.object_ref),node]));
      const rasterRefs=new Set(graphMode?(JSON.parse(player.runtime_snapshot_json()).filter_groups||[]).map(group=>JSON.stringify(group.owner_ref)):[]);
      for(let current=node;current;current=byRef.get(JSON.stringify(current.parent_ref))){
        if(current.filters?.length||rasterRefs.has(JSON.stringify(current.object_ref))||JSON.stringify(current.object_ref)===addedKey)throw new TypeError('Native TextField editing inside a filtered, cached or blended subtree is not supported. Clear its raster effects or move focus before editing.');
      }
    }
    function legacyNavigationVariables(owner){
      const ref=references.get(owner);if(!ref)return owner;
      display.descriptor(owner);
      const result=Object.create(null),keys=new Set([...(ref.customKeys?.()||[]),...(program?.jsonKeys?.(owner)||[])]);
      for(const name of keys){
        const as2Prop=ref.as2TimelineRef?.(name),value=as2Prop?as2Prop.get():owner[name];
        // Methods and child-display references are not MovieClip form vars.
        // Other non-data values still reach the transport's explicit validator.
        if(typeof value==='function'||(value!==null&&typeof value==='object'&&references.has(value)))continue;
        Object.defineProperty(result,name,{value,enumerable:true,configurable:true,writable:true});
      }
      return result;
    }
    let fontLoadingDoneHandler=null;
    function initializeFamily(initialize = true) {
      if (!global.NeoFlashDisplay || typeof player.display_nodes_json !== 'function') return;
      const wasm=resolveRuntimeWasm();
      const activeDoc=typeof instanceDoc!=='undefined'&&instanceDoc?instanceDoc:(typeof document==='undefined'?null:document);
      if(global.NeoFlashAS2Platform)as2PlatformHost=global.NeoFlashAS2Platform.create(wasm,{global,document:activeDoc||global.document,onError:inputFailure,requestRender:requestTimerRender});
      eventFamilies=global.NeoFlashEvents?.createFamilies?.(wasm,{isByteArray:value=>Boolean(binaryHost?.ByteArray.__nfIs(value)),requestRender:requestTimerRender})||{};
      geometryHost=global.NeoFlashGeom.create(wasm,{getVector3D:()=>geometry3dHost.Vector3D});
      if(global.NeoFlashGeom3D)geometry3dHost=global.NeoFlashGeom3D.create(wasm,{getPoint:()=>geometryHost.Point});
      if(global.NeoFlashBitmap)bitmapHost=global.NeoFlashBitmap.create({player,geometry:geometryHost,as2,
        getBinary:()=>binaryHost,getFilters:()=>filterHost,
        displayRef:source=>{const ref=references.get(source);if(!ref)return null;display.descriptor(source);return ref.objectRef;},
        paintScene:(target,snapshot)=>{
          if(!global.NeoFlashPaths)throw new Error('BitmapData.draw requires the shared path renderer.');
          if(!capturePainter)capturePainter=createScenePainter({global,paths:new global.NeoFlashPaths.PathCache(),
            gradients:new global.NeoFlashPaths.GradientRenderer({createCanvas:()=>bitmapHost.createCanvas(1,1)}),
            clippings:new global.NeoFlashPaths.ClipRenderer({createCanvas:()=>bitmapHost.createCanvas(1,1)}),bitmapHost,textHost,filterHost,videoHost,context:target,snapBitmaps:false});
          snapshot=videoHost?videoHost.prepareSnapshot(snapshot):snapshot;
          bitmapHost.cache.beginFrame();try{capturePainter.draw(target,snapshot,{semantic:false});}finally{bitmapHost.cache.endFrame();}
        }});
      if(global.NeoFlashFilters&&typeof player.filter_rgba==='function')filterHost=global.NeoFlashFilters.create({player,getBitmap:()=>bitmapHost,geometry:geometryHost});
      if(global.NeoFlashText&&typeof player.object_text_info==='function'){
        textHost=global.NeoFlashText.create({player,wasm,events:global.NeoFlashEvents,project:typeof project==='undefined'?null:project,document:activeDoc,getProgram:()=>program,geometry:geometryHost,fontMetrics:bundle?.fonts?.metrics||null,onUpdate:ref=>(domStageHost||textInputHost)?.sync(ref),onScroll:ref=>{if(eventHub&&global.NeoFlashEvents&&player.object_alive(ref))eventHub.dispatch(proxyForRef(JSON.parse(ref)),new global.NeoFlashEvents.Event('scroll',false,false));},measureText:(text,format)=>{if(typeof context==='undefined')throw new Error('Text layout requires the Canvas font measurement service.');context.save();try{global.NeoFlashText.applyFont(context,format,true);return context.measureText(text).width;}finally{context.restore();}}});
        if(activeDoc?.fonts?.addEventListener){
          if(fontLoadingDoneHandler)activeDoc.fonts.removeEventListener('loadingdone',fontLoadingDoneHandler);
          fontLoadingDoneHandler=()=>{textHost?.invalidateCache?.();if(typeof context!=='undefined')render();};
          activeDoc.fonts.addEventListener('loadingdone',fontLoadingDoneHandler);
        }
      }
      let invokeAsfunction=()=>{};
      if(textHost&&global.NeoFlashAS2Text)as2TextHost=global.NeoFlashAS2Text.create({broadcaster:global.NeoFlashBroadcaster,wasm,player,text:textHost,getNet:()=>netHost,onAsfunction:(ref,spec)=>{const tf=proxyForRef(JSON.parse(ref));invokeAsfunction(tf?._parent||proxyFor([]),spec);afterTimerEvent();},targetForRef:key=>proxyForRef(JSON.parse(key)),afterCallback:afterTimerEvent,project:typeof project==='undefined'?null:project,document:activeDoc});
      display=global.NeoFlashDisplay.create({player,geometry:geometryHost,getGeom3D:()=>geometry3dHost,proxyFor,proxyForRef,onMutation:deliverDisplayEvents,onSuperConstruct:handleSuperConstruct,isRetiring:target=>retiringTargets.has(target),isRetiringReference:key=>pendingRetirementRefs.has(key)||retiringTargets.has(proxies.get(key)?.proxy),onTabChange:(target,type)=>{if(eventHub&&global.NeoFlashEvents)eventHub.dispatch(target,new global.NeoFlashEvents.Event(type,true,false));},onTextLink:(target,text)=>{if(eventHub&&global.NeoFlashEvents&&display.alive(target)){eventHub.dispatch(target,new global.NeoFlashEvents.TextEvent('link',true,false,text));afterTimerEvent();}},project:typeof project==='undefined'?undefined:project,prepareLinked,getBitmap:()=>bitmapHost,getText:()=>textHost,getAS2Text:()=>as2TextHost,getLoader:()=>loaderHost,getVideo:()=>videoHost,getFilters:()=>filterHost,getNet:()=>netHost,getAudio:()=>audioHost,getFte3D:()=>fte3dHost,as2,
        shapeContains:(kind,x,y,world)=>{const hasCanvas=typeof context!=='undefined'&&typeof context.isPointInPath==='function'&&typeof Path2D==='function';if(hasCanvas)inputPaths??=new global.NeoFlashPaths.PathCache();return global.NeoFlashPaths.contains(hasCanvas?context:null,kind,x,y,hasCanvas?inputPaths.get(kind.commands):null,world);},
        clipsContain:(snapshot,ids,x,y)=>{if(typeof context==='undefined')throw new Error('Clipped input requires Canvas Path2D support.');inputPaths??=new global.NeoFlashPaths.PathCache();return global.NeoFlashPaths.clipsContain(context,ids,global.NeoFlashPaths.clipTable(snapshot.clips||[],snapshot.filter_groups||[]),x,y,inputPaths);},
        beforeRaster:target=>assertUnfilteredEditing(stageHost?.stage.focus,target),beforeFilters:(target,specs)=>{if(specs.length)assertUnfilteredEditing(stageHost?.stage.focus,target);},referenceOf:target=>references.get(target),getStage:()=>stageHost?.stage||null,getStageHost:()=>stageHost||null,
        measureText:(line,kind)=>{if(typeof context==='undefined')throw new Error('Text bounds require the Canvas font measurement service.');context.font=font(kind);return context.measureText(line).width;},
        pathBounds:(commands,matrix)=>global.NeoFlashAccessibility.pathBounds(commands,matrix)});
      if(global.NeoFlashEvents&&global.NeoFlashStage) {
        global.NeoFlashStage.setStageAdapters?.({makePoint:(x,y)=>new geometryHost.Point(x,y),isPoint:value=>geometryHost.Point.__nfIs(value),isRectangle:value=>geometryHost.Rectangle.__nfIs(value),makeRectangle:(x,y,w,h)=>new geometryHost.Rectangle(x,y,w,h),isBitmapData:value=>Boolean(bitmapHost?.BitmapData.__nfIs(value))});
        const alive=target=>retiringTargets.has(target)||target===stageHost?.stage||Boolean(frameLabelType?.__nfIs(target))||display.alive(target);
        const local=(target,x,y)=>{const p=(target===stageHost?.stage||frameLabelType?.__nfIs(target))?{x,y}:display.convert(target,{x,y},false);return[p.x,p.y];};
        eventHub=global.NeoFlashEvents.create({alive,parentOf:target=>(target===stageHost?.stage||frameLabelType?.__nfIs(target))?null:target.parent,
          localPoint:local,globalPoint:(target,x,y)=>{const p=(target===stageHost?.stage||frameLabelType?.__nfIs(target))?{x,y}:display.convert(target,{x,y},true);return[p.x,p.y];}});
        const initialStageColor=typeof project!=='undefined'&&Array.isArray(project.background)?(((project.background[0]&255)<<16)|((project.background[1]&255)<<8)|(project.background[2]&255))>>>0:0xffffff;
        stageHost=global.NeoFlashStage.create({events:eventHub,FullScreenEvent:eventFamilies.FullScreenEvent,TouchEvent:eventFamilies.TouchEvent,IMEEvent:eventFamilies.IMEEvent,width:typeof project==='undefined'?800:project.width,height:typeof project==='undefined'?600:project.height,
          scaleMode:initialFlashScaleMode,align:initialFlashAlign,allowsFullScreen:allowFullscreen?undefined:false,allowsFullScreenInteractive:allowFullscreen?undefined:false,
          color:initialStageColor,viewport:typeof viewport==='undefined'?null:viewport,screen:global.screen,devicePixelRatio:global.devicePixelRatio,
          stageContainer:display.stageContainer,getStageContentSize:()=>display.stageContainer.contentSize(),
          getStage3Ds:()=>fte3dHost?.getStage3Ds()||[],getStageVideos:()=>as3MediaEventsHost?as3MediaEventsHost.getStageVideos():(fte3dHost?.getStageVideos()||[]),getSoftKeyboardRect:()=>fte3dHost?fte3dHost.getSoftKeyboardRect():new geometryHost.Rectangle(0,0,0,0),
          numChildren:()=>{if(!display||(typeof player.runtime_snapshot_json!=='function'&&typeof player.display_nodes_json!=='function'))return 1;return graphMode?display.stageContainer.numChildren():(display.nodes().filter(node=>node.on_stage!==false&&node.path.length===0).length||1);},
          frameRate:runtimeFrameRate,getFrameRate:()=>runtimeFrameRate,setFrameRate:value=>{runtimeFrameRate=value;},alive:target=>alive(target)&&(target===stageHost?.stage||display.descriptor(target).on_stage!==false),localPoint:local,
          canvas:typeof canvas==='undefined'?null:canvas,keyboardTarget:typeof viewport==='undefined'?null:viewport,pointerTarget:global,
          parentOf:target=>display.parentSpace(target),resolveInput:target=>display.resolveInput(target),beforePointer:()=>display.updateDrag(),hasDrag:()=>display.hasDrag(),cancelPointer:()=>display.cancelPointer(),
          onPointerTarget:target=>{pointerOverContent=display.handCursor(target);updateCursor();},onMouseChanged:updateCursor,
          onScaleModeChanged:()=>{enableHostStageLayout(true);if(typeof context!=='undefined')render();},
          onAlignChanged:()=>{enableHostStageLayout(true);if(typeof context!=='undefined')render();},
          onFullScreenSourceRectChanged:()=>{if(ownsViewportFullscreen()||(stageHost?.stage?.displayState&&stageHost.stage.displayState!=='normal')){enableHostStageLayout(false);if(typeof context!=='undefined')render();}},
          onDisplayStateChanged:state=>{if(state!=='normal')enableHostStageLayout(false);else if(!hostStageLayoutPinned)syncInitialStageLayout();else if(typeof preview!=='undefined'&&preview)preview.resize();if(typeof context!=='undefined')render();},
          onColorChanged:()=>{if(typeof context!=='undefined')render();},onInvalidate:requestTimerRender,
          onFocusChanged:(next,previous)=>{if(familyDisposed)return;fte3dHost?.handleFocusChange?.(next,previous);const handled=(domStageHost||textInputHost)?.focusChanged(next,previous);return syncSemanticFocus(next,previous,handled)||handled;},ownsTextInput:node=>(domStageHost||textInputHost)?.owns(node),canFocus:value=>{const ih=domStageHost||textInputHost;if(ih&&!ih.canFocus(value))return false;assertUnfilteredEditing(value);return true;},
          hitTest:event=>{const p=stagePointFromEvent(event);return p&&p.inside?display.pick(p.x,p.y):null;},
          coordinates:event=>{const p=stagePointFromEvent(event);return p?[p.x,p.y]:null;},
          isInputEnabled:()=>!familyDisposed&&(typeof failed==='undefined'||(!failed&&!preview.panMode&&!preview.suppressClick)),
          onLegacyInput:(type,target,event)=>{if(familyDisposed)return;display.pointer(type,target,event);},
          afterInput:()=>{if(familyDisposed)return;dispatchScripts();if(typeof context!=='undefined')render();},onError:inputFailure});
        if(global.NeoFlashLegacyInput){
          legacyHost=global.NeoFlashLegacyInput.create({stage:stageHost.stage,platform:as2PlatformHost,
            keyTarget:typeof instanceKeyTarget!=='undefined'?instanceKeyTarget:(typeof global.addEventListener==='function'?global:null),
            alive:listener=>!references.has(listener)||display.alive(listener),
            onVisibility:visible=>{legacyCursorVisible=visible;updateCursor();},onError:inputFailure});
          if(global.NeoFlashAS2Stage)as2StageHost=global.NeoFlashAS2Stage.create({wasm,legacy:legacyHost,geometry:geometryHost,width:typeof project==='undefined'?800:project.width,height:typeof project==='undefined'?600:project.height,scaleMode:initialFlashScaleMode,align:initialFlashAlign,allowsFullScreen:allowFullscreen,preview:typeof preview==='undefined'?null:preview,viewport:typeof viewport==='undefined'?null:viewport,canvas:typeof canvas==='undefined'?null:canvas,display,getRoot:()=>proxyFor([]),contextTarget:event=>{if(typeof hitAt==='function')return hitAt(event,true)?.target||proxyFor([]);const p=stagePointFromEvent(event);if(p&&p.inside&&display?.pickDetails)return display.pickDetails(p.x,p.y)?.target||proxyFor([]);return proxyFor([]);},onError:inputFailure,afterEvent:afterTimerEvent});
          legacyGlobals=Object.freeze({Key:legacyHost.Key,Mouse:legacyHost.Mouse,Stage:as2StageHost?.Stage||legacyHost.Stage,...(as2StageHost?{ContextMenu:as2StageHost.ContextMenu,ContextMenuItem:as2StageHost.ContextMenuItem}:{})});
        }
      }
      legacyGlobals=Object.freeze({...legacyGlobals,_global:Object.create(null),newline:"\n",
      hasOwnDeleteTarget(receiver,name){
        const ref=references.get(receiver);
        if(ref)return ref.customKeys().includes(name)||(name==='_accProps'&&Boolean(display?.readAccProps?.(receiver)));
        return receiver!==null&&receiver!==undefined&&Object.hasOwn(Object(receiver),name);
      },
      enumerateKeys(receiver){
        const ref=references.get(receiver);
        if(!ref||!display?.alive(receiver))return[];
        const seen=new Set(ref.customKeys());
        if(display.descriptor(receiver).owner==='movie_clip')for(const node of display.children(receiver))if(node.name)seen.add(node.name);
        for(let p=Object.getPrototypeOf(receiver);p&&p!==Object.prototype;p=Object.getPrototypeOf(p))for(const k of Object.keys(p))seen.add(k);
        return[...seen];
      },
      caseKey(receiver,name){
        if(typeof wasm.legacy_case_name_json!=='function')throw new Error('The SWF6 case-name service is missing. Re-export this project.');
        const keys=[],seen=new Set(),add=key=>{if(typeof key==='string'&&!seen.has(key)){if(keys.length>=100000)throw new RangeError('Legacy property lookup exceeds 100000 names.');seen.add(key);keys.push(key);}};
        const ref=references.get(receiver);
        if(ref){for(const key of ref.nativeKeys())add(key);for(const key of ref.customKeys())add(key);for(const node of display.children(receiver))add(node.name);}
        for(let object=Object(receiver),depth=0;object;object=Object.getPrototypeOf(object),depth++){
          if(depth>=256)throw new RangeError('Legacy property prototype lookup exceeds 256 levels.');
          for(const key of Object.getOwnPropertyNames(object))add(key);
        }
        return wasm.legacy_case_name_json(name,JSON.stringify(keys));
      }});
      const notifySystemStatus=info=>{const fn=as2PlatformHost?.System?.onStatus;if(typeof fn==='function')Reflect.apply(fn,as2PlatformHost.System,[info]);};
      if(as2PlatformHost)legacyGlobals=Object.freeze({...legacyGlobals,System:as2PlatformHost.System,getVersion:as2PlatformHost.getVersion,updateAfterEvent:as2PlatformHost.updateAfterEvent});
      const legacyContext=caller=>{
        if(caller!==null&&typeof caller==='object'&&Object.hasOwn(caller,'__nfTimeline')){
          const t=caller.__nfTimeline;
          return {raw:t,base:references.has(t)&&display?.alive(t)?t:proxyFor([]),legacyCase:Boolean(caller.__nfLegacyCase)};
        }
        return {raw:caller,base:references.has(caller)&&display?.alive(caller)?caller:proxyFor([]),legacyCase:false};
      };
      function legacyProperty(target, index, base=proxyFor([]), legacyCase=false) {
        if(!Number.isInteger(Number(index))||Number(index)<0||Number(index)>21)throw new RangeError('AS2 property index must be between 0 and 21.');
        const receiver=display.resolveTarget(target,base,legacyCase),name=wasm.as2_property_name(Number(index));
        const native=legacyGlobals.nativeRef?.(receiver,name);
        if(native)return {receiver,name,native};
        if(!references.get(receiver)?.nativeHas(name))throw new Error('AS2 getProperty/setProperty does not yet support '+name+'.');
        return {receiver,name};
      }
      function updateAs2Accessibility(){
        if(typeof player.as2_accessibility_update!=='function'||!display)return;
        const entries=[];
        for(const node of display.nodes()){
          if(!['movie_clip','button','text','sprite','shape','bitmap','video','loader'].includes(node.owner))continue;
          const cached=proxies.get(JSON.stringify(node.object_ref));if(!cached)continue;
          const p=cached.proxy,acc=display.readAccProps?.(p)||p.accessibilityProperties;
          if(!acc||typeof acc!=='object')continue;
          const interactive=node.owner==='button'||['onPress','onRelease','onReleaseOutside','onRollOver','onRollOut','onDragOver','onDragOut'].some(k=>typeof p[k]==='function');
          entries.push({object_ref:node.object_ref,props:{silent:Boolean(acc.silent),force_simple:Boolean(acc.forceSimple),name:typeof acc.name==='string'?acc.name:null,description:typeof acc.description==='string'?acc.description:null,interactive}});
        }
        player.as2_accessibility_update(JSON.stringify(entries));
        if(typeof semanticLayer!=='undefined'&&semanticLayer?.update&&graphMode){
          const snap=JSON.parse(player.runtime_snapshot_json()),byRef=new Map(snap.objects.map(n=>[JSON.stringify(n.object_ref),n]));
          const semScene=snap.draws.map(item=>{const ancestry_refs=[];for(let n=byRef.get(JSON.stringify(item.owner_ref));n;n=byRef.get(JSON.stringify(n.parent_ref)))ancestry_refs.push(n.object_ref);return {...item,ancestry_refs,kind:item.kind.type==='text_field'&&textHost?{...item.kind,text_bounds:textHost.bounds(item.kind.field,true)}:item.kind};});
          const [w,h]=logicalSize(),[ox,oy]=stageOrigin(),sc=typeof preview==='undefined'?1:{x:preview.view.scale,y:preview.view.scaleY||preview.view.scale};
          const rawNodes=snap.semantics.filter(node=>Boolean(semanticRoute(node,snap.objects)));
          const semNodes=as3MediaEventsHost?.applyAccessibilityPropertiesToSemanticNodes?as3MediaEventsHost.applyAccessibilityPropertiesToSemanticNodes(rawNodes,node=>node.owner_ref?proxyForRef(node.owner_ref):proxyFor(node.instance_path||[])):rawNodes;
          const prepared=prepareStageAccessibility(semNodes,snap);
          semanticLayer.update(prepared.nodes,semScene,{width:w,height:h},sc,(line,kind)=>{if(typeof context==='undefined')return line.length*kind.font_size*0.6;context.font=font(kind);return context.measureText(line).width;},{x:ox,y:oy});
          syncStageAccessibilityOverlay(prepared.fallback,semScene);
        }
        if(!inDispatch&&typeof context!=='undefined')render();
      }
      invokeAsfunction=(caller,rawSpec)=>{
        const spec=String(rawSpec??''),comma=spec.indexOf(','),path=(comma<0?spec:spec.slice(0,comma)).trim(),arg=comma<0?undefined:spec.slice(comma+1);
        if(!path)return;
        const parts=path.split('.');
        if(parts.some(p=>!p||['__proto__','prototype','constructor'].includes(p)))throw new TypeError('Invalid asfunction target path.');
        const resolveFrom=base=>{
          let cur=base,parent=base;
          for(let i=0;i<parts.length;i++){
            if(cur===null||cur===undefined)return null;
            parent=cur;
            const part=parts[i];
            if(part==='_root')cur=references.has(cur)?as2RootFor(cur):proxyFor([]);
            else if(part==='_level0')cur=proxyFor([]);
            else if(part==='_global')cur=legacyGlobals?._global;
            else cur=as2ObjectHost?as2ObjectHost.get(cur,part):cur[part];
          }
          return typeof cur==='function'?{receiver:parent,fn:cur}:null;
        };
        const origin=references.has(caller)&&display?.alive(caller)?caller:proxyFor([]);
        const found=resolveFrom(origin)||(origin!==proxyFor([])?resolveFrom(proxyFor([])):null)||resolveFrom(legacyGlobals?._global);
        if(!found)return;
        Reflect.apply(found.fn,found.receiver,comma<0?[]:[arg]);
      };
      const resolveLegacyMovieTarget=(caller,target)=>{
        const ctx=legacyContext(caller);
        let receiver;
        if(typeof target==='string'){
          const m=/^_level(\d+)$/i.exec(target);
          if(m){if(Number(m[1])!==0)throw new RangeError('HTML5 loadMovie/unloadMovie supports level 0 only; multi-SWF levels are unavailable.');receiver=proxyFor([]);}
          else receiver=display.resolveTarget(target,ctx.base,ctx.legacyCase);
        }else receiver=target;
        if(!references.has(receiver)||!display.alive(receiver)||display.descriptor(receiver).owner!=='movie_clip')throw new TypeError('AS2 movie target must resolve to a live MovieClip.');
        return receiver;
      };
      const legacyPrint=(caller,targetOrLevel,boundingBox='bmovie',byLevel=false)=>{
        const mode=boundingBox===undefined?'bmovie':boundingBox;
        if(!['bmovie','bmax','bframe'].includes(mode))throw new TypeError('AS2 print boundingBox requires "bmovie", "bmax" or "bframe".');
        let receiver;
        if(byLevel){
          if(!Number.isInteger(targetOrLevel)||targetOrLevel<0)throw new RangeError('AS2 printNum requires a non-negative integer level.');
          if(targetOrLevel!==0)throw new RangeError('HTML5 printNum supports level 0 only; multi-SWF levels are unavailable.');
          receiver=proxyFor([]);
        }else receiver=resolveLegacyMovieTarget(caller,targetOrLevel);
        if(!as2PrintHost)throw new Error('AS2 printing requires the PrintJob service.');
        const job=new as2PrintHost.PrintJob();
        if(!job.start())return;
        if(job.addPage(receiver,null,{printAsBitmap:true}))job.send();
      };
      const legacyDisplayCall=(caller,target,method,args)=>{const ctx=legacyContext(caller);return as2.method(display.resolveTarget(target,ctx.base,ctx.legacyCase),method)(...args);};
      legacyGlobals=Object.freeze({...legacyGlobals,
        __nfCallStack:[],
        MovieClip:display.builtins['flash.display.MovieClip'],
        Button:display.builtins['flash.display.SimpleButton'],
        Accessibility:Object.freeze({
          isActive:()=>Boolean(typeof semanticLayer!=='undefined'&&semanticLayer?.container),
          updateProperties:()=>updateAs2Accessibility(),
        }),
        level:n=>n===0?proxyFor([]):undefined,
        levelRef:n=>({has:()=>n===0,delete:()=>false,get:()=>n===0?proxyFor([]):undefined,set:()=>{throw new TypeError('AS2 _level references are read-only.');}}),
        resolveTarget:(value,relative,legacyCase=false)=>display.resolveTarget(value,relative??proxyFor([]),legacyCase),
        runInitClip:(key,root,fn)=>{if(executedInitClips.has(key))return;executedInitClips.add(key);fn(root);},
        callFrame(target,spec){
          const ref=references.get(target);
          if(!ref||!display.alive(target)||display.descriptor(target).owner!=='movie_clip'||spec===null||spec===undefined)return undefined;
          let normalized;
          if(typeof spec==='number'){if(!Number.isFinite(spec))return undefined;normalized=Math.trunc(spec);}
          else normalized=String(spec);
          if(callFrameDepth>=64)throw new RangeError('AS2 call() recursion limit exceeded.');
          const packet=JSON.parse(player.as2_call_frame_scripts(ref.key,JSON.stringify(normalized)));
          if(!packet||!packet.scripts?.length)return undefined;
          const scope=proxyForRef(packet.target_ref);
          callFrameDepth++;
          try{
            for(const script of packet.scripts){
              if(!script.source.trim())continue;
              const compiled=functions.get(sourceKey(script));
              if(!compiled)throw new Error('A script is missing from the compiled export. Re-export the project.');
              const event=Object.freeze({type:'frame',target:scope,currentTarget:scope});
              const scriptRoot=['action_script1','action_script2','ActionScript1','ActionScript2'].includes(script.language)?as2RootFor(scope):proxyFor([]);
              compiled.fn.call(scope,event,scriptRoot,scope._parent,trace,scope.play,scope.stop,scope.gotoAndPlay,scope.gotoAndStop,scope.nextFrame,scope.prevFrame,as2.method,as2.value,program,display,legacyGlobals,xmlHost);
              display?.flushClipLoads?.();
            }
          }finally{callFrameDepth--;}
          return undefined;
        },
        isFrameLoaded(target,args=[]){
          const ref=references.get(target);
          if(!ref||!display.alive(target)||display.descriptor(target).owner!=='movie_clip')return false;
          const hasScene=args.length>=2,scene=hasScene?(args[0]==null?null:String(args[0])):null,raw=hasScene?args[1]:args[0];
          const targetRef=hasScene?references.get(proxyFor([])):ref;
          let frame;
          if(typeof raw==='number'){if(!Number.isFinite(raw)||!Number.isInteger(raw))return false;frame=raw;}
          else if(typeof raw==='string')frame=raw;
          else return false;
          return Boolean(player.as2_is_frame_loaded(targetRef.key,JSON.stringify({scene,frame})));
        },
        globalPropRef:name=>{const ref=display.as2GlobalPropRef(name);if(!ref)throw new ReferenceError(`Unknown AS2 global property ${name}.`);return ref;},
        toggleHighQuality:display.toggleHighQuality,
        showRedrawRegions:display.showRedrawRegions,
        registerClass(id,ctor){
          if(typeof id!=='string'||!id.length||id.length>1024)return false;
          if(ctor===null||ctor===undefined){registeredClasses.set(id,null);return true;}
          if(typeof ctor!=='function')return false;
          const name=program?.className?.(ctor);
          if(name){
            if(program.dialect?.(name)!=='as2')throw new TypeError(`Object.registerClass requires an AS2 class or constructor function; ${name} is not an AS2 class.`);
            registeredClasses.set(id,{kind:'project',name,ctor});
            return true;
          }
          registeredClasses.set(id,{kind:'function',ctor});
          return true;
        },
        duplicateMovieClip(target,name,depth){return legacyDisplayCall(this,target,'duplicateMovieClip',[name,depth]);},
        removeMovieClip(target){return legacyDisplayCall(this,target,'removeMovieClip',[]);},
        startDrag(target,...args){return legacyDisplayCall(this,target,'startDrag',args);},
        stopDrag(){return legacyDisplayCall(this,proxyFor([]),'stopDrag',[]);},
        targetPath(target){const ctx=legacyContext(this);return display.targetPath(display.resolveTarget(target,ctx.base,ctx.legacyCase));},
        getProperty(target,index){const ctx=legacyContext(this),{receiver,name,native}=legacyProperty(target,index,ctx.base,ctx.legacyCase);return native?native.get():receiver[name];},
        setProperty(target,index,value){const ctx=legacyContext(this),{receiver,name,native}=legacyProperty(target,index,ctx.base,ctx.legacyCase);if(native)native.set(value);else receiver[name]=value;},
        asfunction(func,param){if(typeof func!=='string'||!func)throw new TypeError('AS2 asfunction requires a function path String.');return param===undefined?`asfunction:${func}`:`asfunction:${func},${String(param)}`;},
        fscommand(command,parameters=''){externalHost?.fscommand(command,parameters);},
        MMExecute(command){if(typeof command!=='string')throw new TypeError('AS2 MMExecute requires a JSFL command String.');return '';},
        loadMovie(url,target,method){return as2.method(resolveLegacyMovieTarget(this,target),'loadMovie')(url,method);},
        unloadMovie(target){return as2.method(resolveLegacyMovieTarget(this,target),'unloadMovie')();},
        loadMovieNum(url,level,method){if(!Number.isInteger(level)||level<0)throw new RangeError('AS2 loadMovieNum requires a non-negative integer level.');if(level!==0)throw new RangeError('HTML5 loadMovieNum supports level 0 only; multi-SWF levels are unavailable.');return as2.method(proxyFor([]),'loadMovie')(url,method);},
        unloadMovieNum(level){if(!Number.isInteger(level)||level<0)throw new RangeError('AS2 unloadMovieNum requires a non-negative integer level.');if(level!==0)throw new RangeError('HTML5 unloadMovieNum supports level 0 only; multi-SWF levels are unavailable.');return as2.method(proxyFor([]),'unloadMovie')();},
        print(target,boundingBox){legacyPrint(this,target,boundingBox,false);},
        printAsBitmap(target,boundingBox){legacyPrint(this,target,boundingBox,false);},
        printNum(level,boundingBox){legacyPrint(this,level,boundingBox,true);},
        printAsBitmapNum(level,boundingBox){legacyPrint(this,level,boundingBox,true);}});
      if(isDomRenderMode&&global.NeoFlashDomLayout?.createDomStageHost){
        domStageHost=global.NeoFlashDomLayout.createDomStageHost({
          document:typeof domHostDocument!=='undefined'?domHostDocument:activeDoc,
          surface:typeof stageSurface!=='undefined'?stageSurface:(activeDoc?activeDoc.getElementById('stage-surface'):null),
          viewport:typeof viewport==='undefined'?null:viewport,
          canvas:typeof canvas==='undefined'?null:canvas,
          project:typeof project==='undefined'?null:project,
          config:hostPlayerConfig,
          player,
          getDisplay:()=>display,
          getStageHost:()=>familyDisposed?null:stageHost,
          getTextHost:()=>textHost,
          getBitmapHost:()=>bitmapHost,
          getVideoHost:()=>videoHost,
          getEventHub:()=>eventHub,
          getEvents:()=>({...global.NeoFlashEvents,...eventFamilies}),
          getAs2ObjectHost:()=>as2ObjectHost,
          getAs2TextHost:()=>as2TextHost,
          getAudioHost:()=>audioHost,
          proxyFor,
          proxyForRef,
          referenceOf:target=>references.get(target),
          afterInput:afterTimerEvent,
          onError:inputFailure,
          onTextLink:(ownerKey,text)=>{const target=proxyForRef(JSON.parse(ownerKey));if(eventHub&&global.NeoFlashEvents&&display?.alive(target)){eventHub.dispatch(target,new global.NeoFlashEvents.TextEvent('link',true,false,text));afterTimerEvent();}},
          onAsfunction:(ownerKey,spec)=>{const tf=proxyForRef(JSON.parse(ownerKey));invokeAsfunction(tf?._parent||proxyFor([]),spec);afterTimerEvent();},
          onNavigateUrl:(url,target)=>{if(netHost){const req=new netHost.URLRequest(url);if(externalHost)netHost.navigateToURL(req,target||null);else{const prepared=netHost.prepareNavigation(req,target||null);if(prepared.target==='_self'||prepared.target==='_parent'||prepared.target==='_top'||/^mailto:/i.test(prepared.url))global.location?.assign?.(prepared.url);else global.open?.(prepared.url,prepared.target,'noopener,noreferrer');}}},
          onActivateObject:(target,authored)=>{if(!display?.alive(target)||!display?.alive(authored))return;if(graphMode&&typeof player.object_click==='function')player.object_click(references.get(authored).key);else{let owner=authored,node=display.descriptor(owner);while(graphMode&&node.authored_path===null){owner=display.parentSpace(owner);if(!owner||owner===stageHost?.stage)break;node=display.descriptor(owner);}const path=graphMode?node.authored_path:node.path;if(path)player.click(JSON.stringify(path));}dispatchScripts(authored);if(typeof context!=='undefined')render();},
          paintCanvasIsland:(islandCanvas,islandDraws,fullSnapshot,stageBox)=>{
            if(typeof context==='undefined'||!global.NeoFlashPaths)return null;
            if(!islandPainter)islandPainter=createScenePainter({global,paths:new global.NeoFlashPaths.PathCache(),gradients:new global.NeoFlashPaths.GradientRenderer({createCanvas:()=>bitmapHost?bitmapHost.createCanvas(1,1):document.createElement('canvas')}),clippings:new global.NeoFlashPaths.ClipRenderer({createCanvas:()=>bitmapHost?bitmapHost.createCanvas(1,1):document.createElement('canvas')}),bitmapHost,textHost,filterHost,videoHost,context,snapBitmaps:false});
            const allGroups=new Map((fullSnapshot?.filter_groups||[]).map(g=>[g.id,g])),neededGroups=new Set();
            for(const d of islandDraws){for(let cur=d.filter_id?allGroups.get(d.filter_id):null;cur&&!neededGroups.has(cur.id);cur=cur.parent_id?allGroups.get(cur.parent_id):null)neededGroups.add(cur.id);}
            const subGroups=(fullSnapshot?.filter_groups||[]).filter(g=>neededGroups.has(g.id)).map(g=>stageBox?.rootCssBlend&&g.parent_id===null?{...g,composite_operation:'source-over'}:g);
            let area=null;
            for(const d of islandDraws){const b=islandPainter.paintBounds(d,true);if(b&&b.right>b.left&&b.bottom>b.top)area=area?{left:Math.min(area.left,b.left),top:Math.min(area.top,b.top),right:Math.max(area.right,b.right),bottom:Math.max(area.bottom,b.bottom)}:{...b};}
            if(!area)return null;
            if(subGroups.length&&typeof player.filter_padding==='function'){
              for(const fg of subGroups){if(fg.filters?.length){const pad=JSON.parse(player.filter_padding(JSON.stringify(fg.filters)));area={left:area.left-pad.left,top:area.top-pad.top,right:area.right+pad.right,bottom:area.bottom+pad.bottom};}}
            }
            const minX=-(stageBox?.originX||0),minY=-(stageBox?.originY||0),maxX=(stageBox?.width||800)+minX,maxY=(stageBox?.height||600)+minY;
            const left=Math.max(minX,Math.floor(area.left)-1),top=Math.max(minY,Math.floor(area.top)-1),right=Math.min(maxX,Math.ceil(area.right)+1),bottom=Math.min(maxY,Math.ceil(area.bottom)+1);
            if(!(right>left&&bottom>top))return null;
            const w=right-left,h=bottom-top,scaleMax=Math.max(1,Number(stageBox?.scale?.x)||1,Number(stageBox?.scale?.y)||1),dpr=Math.min(4,Math.max(1,(global.devicePixelRatio||1)*scaleMax),8192/Math.max(1,w),8192/Math.max(1,h));
            const pw=Math.max(1,Math.round(w*dpr)),ph=Math.max(1,Math.round(h*dpr));
            if(islandCanvas.width!==pw||islandCanvas.height!==ph){islandCanvas.width=pw;islandCanvas.height=ph;}
            const ictx=islandCanvas.getContext('2d');if(!ictx)return null;
            ictx.setTransform(1,0,0,1,0,0);ictx.clearRect(0,0,pw,ph);ictx.setTransform(dpr,0,0,dpr,-left*dpr,-top*dpr);
            islandPainter.draw(ictx,{draws:islandDraws,objects:fullSnapshot?.objects||[],semantics:[],clips:fullSnapshot?.clips||[],filter_groups:subGroups},{minX:left,minY:top,width:w,height:h,semantic:false});
            return {left,top,width:w,height:h};
          },
        });
        legacyGlobals=Object.freeze({...legacyGlobals,Selection:Object.freeze({...domStageHost.Selection,setFocus:value=>{const target=typeof value==='string'?(value?display.resolveTarget(value,proxyFor([])):null):value;if(!display.as2CanFocus?.(target))return false;return domStageHost.Selection.setFocus(target);}})});
      }else if(global.NeoFlashTextInput&&textHost&&stageHost&&typeof player.object_text_input==='function'){
        textInputHost=global.NeoFlashTextInput.create({getSize:logicalSize,getOrigin:stageOrigin,player,text:textHost,display,events:{...global.NeoFlashEvents,...eventFamilies},hub:eventHub,stage:stageHost,referenceOf:target=>references.get(target),proxyForRef,onLegacyTab:event=>display.as2Tab?.(event)||false,beforeLegacyTextEvent:(target,name)=>{if(name==='onChanged')as2ObjectHost?.textChanged(target);},onLegacyTextEvent:(target,name,args)=>as2TextHost?.textEvent(references.get(target).key,name,args),
          canvas:typeof canvas==='undefined'?null:canvas,surface:typeof stageSurface!=='undefined'?stageSurface:(activeDoc?activeDoc.getElementById('stage-surface'):null),document:activeDoc,project:typeof project==='undefined'?null:project,
          onError:inputFailure,afterInput:afterTimerEvent,onClick:(target,event)=>{target=display.resolveInput(target);if(!target)return;stageHost.handlePointer('click',event,target);if(display.alive(target)){player.object_click(references.get(target).key);dispatchScripts(target);if(typeof context!=='undefined')render();}}});
        legacyGlobals=Object.freeze({...legacyGlobals,Selection:Object.freeze({...textInputHost.Selection,setFocus:value=>{const target=typeof value==='string'?(value?display.resolveTarget(value,proxyFor([])):null):value;if(!display.as2CanFocus?.(target))return false;return textInputHost.Selection.setFocus(target);}})});
      }
      if(global.NeoFlashTimers&&global.NeoFlashEvents)timerHost=global.NeoFlashTimers.create({wasm,events:global.NeoFlashEvents,
        onError:timerFailure,afterEvent:afterTimerEvent,requestRender:requestTimerRender});
      if(timerHost?.legacy)legacyGlobals=Object.freeze({...legacyGlobals,...timerHost.legacy});
      if(global.NeoFlashEasing)easingHost=global.NeoFlashEasing.create(wasm,{timers:timerHost,events:global.NeoFlashEvents,getFrameRate:()=>runtimeFrameRate,onError:timerFailure,afterEvent:afterTimerEvent});
      if(global.NeoFlashBroadcaster){broadcasterHost=global.NeoFlashBroadcaster.create(wasm);legacyGlobals=Object.freeze({...legacyGlobals,AsBroadcaster:broadcasterHost.AsBroadcaster});}
      if(global.NeoFlashByteArray)binaryHost=global.NeoFlashByteArray.create({wasm});
      if(global.NeoFlashAMF&&binaryHost&&typeof player.amf_encode==='function'){
        amfHost=global.NeoFlashAMF.create({binary:binaryHost,encode:(format,graph)=>player.amf_encode(format,graph),decode:(format,bytes)=>player.amf_decode(format,bytes),encodeSol:typeof wasm?.sol_encode==='function'?(format,name,graph)=>wasm.sol_encode(format,name,graph):undefined,decodeSol:typeof wasm?.sol_decode==='function'?bytes=>wasm.sol_decode(bytes):undefined,getXML:()=>xmlHost,getProgram:()=>program,isUnsupported:value=>references.has(value)});
        binaryHost.setObjectCodec(amfHost);
        if(global.NeoFlashStorage&&global.NeoFlashEvents){
          storageHost=global.NeoFlashStorage.create({amf:amfHost,events:global.NeoFlashEvents,onError:error=>console.warn(error),onSystemStatus:notifySystemStatus});
          legacyGlobals=Object.freeze({...legacyGlobals,SharedObject:storageHost.LegacySharedObject});
        }
      }
      if(global.NeoFlashExternal){
        externalHost=global.NeoFlashExternal.create({hostObject:typeof canvas==='undefined'?null:canvas,hostGlobal:global,getProgram:()=>program,isUnsupported:value=>references.has(value),afterCallback:afterTimerEvent,unwrapError:unwrapScriptThrow,onDiagnostic:error=>console.warn?.('NeoFlash ExternalInterface:',error)});
        legacyGlobals=Object.freeze({...legacyGlobals,ExternalInterface:externalHost.LegacyExternalInterface});
      }
      if(global.NeoFlashNet&&binaryHost&&global.NeoFlashEvents){
        netHost=global.NeoFlashNet.create({events:{...global.NeoFlashEvents,...eventFamilies},binary:binaryHost,resources:typeof project==='undefined'?[]:project.resources||[],assetUrls:typeof bundleAssetUrls==='undefined'?null:bundleAssetUrls,resourceAliases:typeof bundleResourceAliases==='undefined'?null:bundleResourceAliases,onError:netFailure,onAfterDispatch:afterTimerEvent,navigate:prepared=>{if(!externalHost)throw new Error('Browser navigation adapter is unavailable. Re-export this publication.');externalHost.navigate(prepared);},legacyVariables:legacyNavigationVariables});
        legacyGlobals=Object.freeze({...legacyGlobals,LoadVars:netHost.LoadVars,getURL:function(url,target,method){const ctx=legacyContext(this);if(typeof url==='string'&&/^asfunction:/i.test(url)){invokeAsfunction(ctx.base,url.slice(11));return;}const owner=ctx.raw!==null&&typeof ctx.raw==='object'?ctx.raw:proxyFor([]);return netHost.getURL(owner,url,target,method);}});
      }
      if(global.NeoFlashAS2Network&&netHost){
        as2NetworkHost=global.NeoFlashAS2Network.create({wasm,getNet:()=>netHost,resolveTarget:(target,source,legacyCase=false)=>{const receiver=display.resolveTarget(target,references.has(source)&&display.alive(source)?source:proxyFor([]),legacyCase);if(display.descriptor(receiver).owner!=='movie_clip')throw new TypeError('AS2 loadVariables target must be a live MovieClip.');return receiver;},fieldsOf:legacyNavigationVariables,alive:value=>display.alive(value),notifyData:target=>display.notifyData?.(target),afterCallback:afterTimerEvent});
        legacyGlobals=Object.freeze({...legacyGlobals,loadVariables:function(url,target,method){const ctx=legacyContext(this);return as2NetworkHost.load(ctx.raw,url,target,method,ctx.legacyCase);},loadVariablesNum:function(url,level,method){if(level!==0)throw new RangeError('HTML5 loadVariablesNum supports level 0 only; additional SWF levels are unavailable.');const ctx=legacyContext(this);return as2NetworkHost.load(ctx.raw,url,proxyFor([]),method,ctx.legacyCase);}});
      }
      if(global.NeoFlashAS2Files&&netHost&&binaryHost&&broadcasterHost)as2FilesHost=global.NeoFlashAS2Files.create({wasm,net:netHost,binary:binaryHost,AsBroadcaster:broadcasterHost.AsBroadcaster,afterEvent:afterTimerEvent,onError:netFailure});
      if(global.NeoFlashAS2Locale&&netHost)as2LocaleHost=global.NeoFlashAS2Locale.create(wasm,{URLLoader:netHost.URLLoader,URLRequest:netHost.URLRequest,language:()=>as2PlatformHost.System.capabilities.language,afterEvent:afterTimerEvent,fields:{isText:value=>references.has(value)&&display.alive(value)&&display.descriptor(value).owner==='text'&&!as2TextHost.as2Ref(references.get(value).key,'html').get(),alive:value=>display.alive(value),write:(value,next)=>{if(as2TextHost.as2Ref(references.get(value).key,'html').get())throw new Error('Locale automatic replacement currently requires plain text.');textHost.update(references.get(value).key,{text:next});as2ObjectHost?.textChanged(value);}}});
      if(global.NeoFlashLoader&&netHost&&binaryHost&&global.NeoFlashEvents){
        const dynamicScriptBindings=['event','_root','_parent','trace','play','stop','gotoAndPlay','gotoAndStop','nextFrame','prevFrame','__nfAs2Method','__nfAs2Value','__nfProgram','__nfDisplay','__nfLegacy','__nfXML'];
        const compileDynamicBundleScript=unit=>{
          if(typeof scriptFunction==='function')return scriptFunction(unit,dynamicScriptBindings);
          const url=/^neoflash-script-\d+\.js$/.test(unit.source_url||'')?unit.source_url:'neoflash-user-script.js';
          return new Function(...dynamicScriptBindings,'"use strict";\nreturn (function () {\n'+unit.code+'\n}).call(this);\n//# sourceURL='+url);
        };
        const mountedByPrefix=new Map();
        const mountedTargetUrl=new Map();
        const releaseCleanEntry=clean=>{
          if(!clean)return;
          netHost?.unregisterResources?.(clean.resourcePaths);
          audioHost?.unregisterResources?.(clean.resourceIds);
          if(typeof project!=='undefined'&&Array.isArray(project?.resources)&&clean.resourceIds?.length){
            const drop=new Set(clean.resourceIds);
            project.resources=project.resources.filter(r=>!drop.has(r.id));
          }
          if(clean.targetKey)mountedTargetUrl.delete(clean.targetKey);
        };
        const drainRetiredPublished=()=>{
          if(typeof player.drain_retired_published_prefixes!=='function')return;
          let prefixes;
          try{prefixes=JSON.parse(player.drain_retired_published_prefixes());}catch(_){prefixes=[];}
          for(const pfx of prefixes||[]){
            const clean=mountedByPrefix.get(pfx);
            if(clean){mountedByPrefix.delete(pfx);releaseCleanEntry(clean);}
          }
        };
        const cleanupPublishedResources=record=>{
          const clean=record?.publishedCleanup;if(!clean)return;
          record.publishedCleanup=null;
          if(clean.prefix)mountedByPrefix.delete(clean.prefix);
          releaseCleanEntry(clean);
        };
        const resolveChildAssetUrl=(relUrl,bundleUrl)=>{
          const raw=String(relUrl||'').trim();
          if(!raw||/^(?:javascript|data|vbscript|blob):/i.test(raw))return null;
          if(/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(raw))return raw;
          if(bundleUrl&&/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(String(bundleUrl))){
            try{return new URL(raw,String(bundleUrl)).href;}catch(_){}
          }
          const clean=String(bundleUrl||'').split('#')[0];
          const qIdx=clean.indexOf('?'),pathPart=qIdx>=0?clean.slice(0,qIdx):clean;
          const slash=pathPart.lastIndexOf('/'),dir=slash>=0?pathPart.slice(0,slash+1):'';
          return dir+raw.replace(/^\.\//,'');
        };
        loaderHost=global.NeoFlashLoader.create({
          player,events:global.NeoFlashEvents,net:netHost,binary:binaryHost,project:typeof project==='undefined'?null:project,
          rootParameters:typeof instanceParameters!=='undefined'?instanceParameters:null,
          rootUrl:typeof instanceRootUrl!=='undefined'?instanceRootUrl:null,
          legacyVariables:legacyNavigationVariables,display,proxyForRef,referenceOf:target=>references.get(target),
          getRoot:()=>proxyFor([]),
          getBuiltin:qname=>{try{return program?.type?.(qname);}catch(_){return as3MediaEventsHost?.builtins?.[qname]||as3NetSysHost?.builtins?.[qname]||fte3dHost?.builtins?.[qname]||null;}},
          inspectSwfBytes:bytes=>fte3dHost?fte3dHost.inspectSwfBytes(bytes):null,
          getFrameRate:()=>runtimeFrameRate,getRootDisplay:()=>proxyFor([]),
          getStageSize:()=>{const [w,h]=logicalSize();return {width:w,height:h};},
          mountPublishedBundle:(record,childBundle,childUrl='',isCurrent)=>{
            const mount=()=>{
            if(typeof player.loader_mount_project!=='function')throw new Error('Published bundle mounting requires the NeoFlash WASM runtime.');
            const normUrl=String(childUrl||'').split('#')[0];
            if(display?.nodes&&record?.ref){
              const nodes=display.nodes(),byRef=new Map(nodes.map(n=>[JSON.stringify(n.object_ref),n]));
              let cur=byRef.get(JSON.stringify(record.ref)),mountDepth=0;
              while(cur){
                const k=JSON.stringify(cur.object_ref);
                if(mountedTargetUrl.has(k)){
                  mountDepth++;
                  if(mountDepth>=8)throw new RangeError('Nested published bundle mounting exceeds the 8-level depth limit.');
                  if(normUrl&&mountedTargetUrl.get(k)===normUrl)throw new TypeError(`Recursive published bundle cycle detected for '${normUrl}'.`);
                }
                cur=cur.parent_ref?byRef.get(JSON.stringify(cur.parent_ref)):null;
              }
            }
            cleanupPublishedResources(record);
            const mounted=JSON.parse(player.loader_mount_project(JSON.stringify(record.ref),JSON.stringify(childBundle.project),Boolean(record.as2)));
            drainRetiredPublished();
            const resMap=mounted.resource_id_map||mounted.resource_map||{};
            const pathMap=mounted.resource_path_map||{};
            const remappedResources=[];
            const childAssetUrls={};
            const childAliases={};
            const origToRemapped=new Map();
            const rawChildAssets=(childBundle.asset_urls&&typeof childBundle.asset_urls==='object')?childBundle.asset_urls:null;
            const rawChildAliases=(childBundle.resource_aliases&&typeof childBundle.resource_aliases==='object')?childBundle.resource_aliases:null;
            for(const res of childBundle.project?.resources||[]){
              const mapped=resMap[res.id];
              const nextId=Array.isArray(mapped)?mapped[0]:(typeof mapped==='string'?mapped:res.id);
              const nextPath=typeof pathMap[res.id]==='string'?pathMap[res.id]:(Array.isArray(mapped)?mapped[1]:res.path);
              remappedResources.push({...res,id:nextId,path:nextPath});
              origToRemapped.set(res.path,nextPath);
              if(rawChildAssets&&typeof rawChildAssets[res.path]==='string'){
                const resolvedExt=resolveChildAssetUrl(rawChildAssets[res.path],normUrl);
                if(resolvedExt)childAssetUrls[nextPath]=resolvedExt;
              }
            }
            if(rawChildAliases){
              for(const[aliasKey,canonTarget]of Object.entries(rawChildAliases)){
                const remappedTarget=origToRemapped.get(canonTarget);
                if(typeof aliasKey==='string'&&typeof remappedTarget==='string'){
                  const prefix=(typeof canonTarget==='string'&&remappedTarget.endsWith(canonTarget))?remappedTarget.slice(0,remappedTarget.length-canonTarget.length):'';
                  childAliases[prefix+aliasKey]=remappedTarget;
                }
              }
            }
            if(childBundle.__hydratedSpriteSheet&&typeof player.hydrate_sprite_sheet==='function'){
              try{
                const parsedSheet=JSON.parse(childBundle.__hydratedSpriteSheet.manifestText);
                if(Array.isArray(parsedSheet?.bitmaps)){
                  parsedSheet.bitmaps=parsedSheet.bitmaps.map(b=>{
                    const m=b&&typeof b.id==='string'?resMap[b.id]:null;
                    const nid=Array.isArray(m)?m[0]:(typeof m==='string'?m:b?.id);
                    return nid?{...b,id:nid}:b;
                  });
                }
                player.hydrate_sprite_sheet(childBundle.__hydratedSpriteSheet.pngBytes,JSON.stringify(parsedSheet));
              }catch(err){
                throw new Error(`Failed to hydrate child bundle sprite sheet: ${describeThrown(err)}`, {cause: err});
              }
            }
            const addedPaths=netHost?.registerResources?netHost.registerResources(remappedResources,childAssetUrls,childAliases):[];
            const addedIds=audioHost?.registerResources?audioHost.registerResources(remappedResources):[];
            if(typeof project!=='undefined'&&Array.isArray(project?.resources)){
              const existingIds=new Set(project.resources.map(r=>r.id));
              for(const res of remappedResources){if(!existingIds.has(res.id))project.resources.push(res);}
            }
            const mut=mounted.mutation||mounted;
            const contentTargetKey=JSON.stringify(mut.content_ref||record.ref);
            if(normUrl)mountedTargetUrl.set(contentTargetKey,normUrl);
            const cleanEntry={prefix:mounted.mounted_prefix||null,targetKey:contentTargetKey,resourcePaths:addedPaths,resourceIds:addedIds.length?addedIds:remappedResources.map(r=>r.id)};
            if(cleanEntry.prefix)mountedByPrefix.set(cleanEntry.prefix,cleanEntry);
            record.publishedCleanup=cleanEntry;
            for(const unit of childBundle.scripts||[]){
              const item=unit.script?unit:{...unit,language:unit.language,source:unit.source};
              const k=sourceKey(item.script||item);
              if(!functions.has(k)){
                functions.set(k,{fn:compileDynamicBundleScript(unit),location:unit.location||'published-bundle'});
              }
            }
            return {
              contentRef:mut.content_ref||null,
              events:mut.events||[],
              width:Number(mounted.width||childBundle.project?.width)||550,
              height:Number(mounted.height||childBundle.project?.height)||400,
              frameRate:Number(mounted.fps||childBundle.project?.fps)||runtimeFrameRate,
            };
            };
            const gateFonts=typeof mountAfterBundleFonts==='function'?mountAfterBundleFonts:null;
            if(!gateFonts)return mount();
            return gateFonts(
              activeDoc,
              childBundle,
              mount,
              isCurrent,
              ()=>textHost?.invalidateCache?.(),
            );
          },
          unloadPublishedBundle:(record,stopDetached=false)=>{
            cleanupPublishedResources(record);
            if(typeof player.loader_unload_published!=='function')return {events:[]};
            const out=JSON.parse(player.loader_unload_published(JSON.stringify(record.ref),Boolean(record.as2),Boolean(stopDetached)));
            drainRetiredPublished();
            drainSounds();drainVideos();
            return out;
          },
          onAfterMountPublished:()=>{attachNewInstances();drainSounds();drainVideos();dispatchScripts();},
          onMutation:ev=>{drainRetiredPublished();deliverDisplayEvents(ev);},onError:netFailure,afterEvent:afterTimerEvent
        });
        legacyGlobals=Object.freeze({
          ...legacyGlobals,
          MovieClipLoader:loaderHost.MovieClipLoader,
          loadPublish:function(url,target){
            const ctx=legacyContext(this);
            const dest=target===undefined||target===null?(ctx.raw!==null&&typeof ctx.raw==='object'?ctx.raw:proxyFor([])):target;
            return loaderHost.loadPublish(url,dest);
          }
        });
      }
      if(global.NeoFlashAudio&&netHost&&binaryHost&&global.NeoFlashEvents){
        audioHost=global.NeoFlashAudio.create({player,wasm,events:{...global.NeoFlashEvents,...eventFamilies},net:netHost,binary:binaryHost,resources:typeof project==='undefined'?[]:project.resources||[],resolveSoundTarget:value=>{if(value===undefined||value===null)return null;const resolved=typeof value==='string'?(value===''?proxyFor([]):display.resolveTarget(value,proxyFor([]))):value;if(!resolved||!references.has(resolved)||!display.alive(resolved)||display.descriptor(resolved).owner!=='movie_clip')throw new TypeError('AS2 Sound target must be undefined, null, or a live MovieClip.');return resolved;},onStopAll:stopTimelineSounds,onError:audioFailure,afterEvent:afterTimerEvent,onState:updateAudioStatus});
        legacyGlobals=Object.freeze({...legacyGlobals,Sound:audioHost.LegacySound,stopAllSounds:stopAllSounds});
      }
      if(global.NeoFlashAS2Connections&&netHost&&amfHost&&binaryHost){as2ConnectionsHost=global.NeoFlashAS2Connections.create({wasm,amf:amfHost,net:netHost,binary:binaryHost,getXML:()=>xmlHost?.LegacyXML,onError:netFailure,onSystemStatus:notifySystemStatus,afterEvent:afterTimerEvent});legacyGlobals=Object.freeze({...legacyGlobals,LocalConnection:as2ConnectionsHost.LocalConnection,XMLSocket:as2ConnectionsHost.XMLSocket});}
      if(global.NeoFlashAS2Print&&bitmapHost){as2PrintHost=global.NeoFlashAS2Print.create(wasm,{
        resolveTarget:value=>{const target=value===0?proxyFor([]):typeof value==='string'?display.resolveTarget(value,proxyFor([])):value;if(!references.has(target)||!display.alive(target)||display.descriptor(target).owner!=='movie_clip')throw new TypeError('PrintJob requires level 0 or a live MovieClip.');return target;},
        bounds:target=>{if(target===proxyFor([])){const [width,height]=logicalSize();return {xMin:0,yMin:0,xMax:width,yMax:height};}const b=target.getBounds(target);return {xMin:b.x,yMin:b.y,xMax:b.x+b.width,yMax:b.y+b.height};},
        capture:(target,plan)=>{const bitmap=new bitmapHost.BitmapData(plan.width,plan.height,false,0xffffff);try{bitmap.draw(target,new geometryHost.Matrix(1,0,0,1,-plan.x,-plan.y));return player.bitmap_encode_png(JSON.stringify(bitmapHost.refOf(bitmap)),0,0,plan.width,plan.height,false);}finally{bitmap.dispose();}},onError:netFailure});legacyGlobals=Object.freeze({...legacyGlobals,PrintJob:as2PrintHost.PrintJob});}
      if(global.NeoFlashAS2Capture){as2CaptureHost=global.NeoFlashAS2Capture.create(wasm,{referenceOf:target=>references.get(target),display,getAudio:()=>audioHost,afterEvent:afterTimerEvent,onError:netFailure,onSystemStatus:notifySystemStatus});legacyGlobals=Object.freeze({...legacyGlobals,Camera:as2CaptureHost.Camera,Microphone:as2CaptureHost.Microphone});}
      if(global.NeoFlashVideo&&netHost&&global.NeoFlashEvents&&typeof player.object_video_info==='function'){
        videoHost=global.NeoFlashVideo.create({getCapture:()=>as2CaptureHost,getAs3Camera:()=>as3MediaEventsHost?.cameraVideoAdapter,getRemoting:()=>as2ConnectionsHost?.remoting,player,wasm,events:global.NeoFlashEvents,net:netHost,getSoundTransform:()=>audioHost?.SoundTransform,getFrameRate:()=>runtimeFrameRate,resourcePath:id=>(typeof project==='undefined'?null:project.resources?.find(resource=>resource.id===id)?.path)||null,referenceOf:target=>references.get(target),proxyForRef,onError:netFailure,onSystemStatus:notifySystemStatus,afterEvent:afterTimerEvent,onFrame:()=>{if(typeof context!=='undefined')render();},onState:updateAudioStatus});
        const LegacyVideo=function(){throw new TypeError('AS2 Video must be an authored Video placement; new Video() is not available.');};
        Object.defineProperties(LegacyVideo,{__nfIs:{value:videoHost.Video.__nfIs},[Symbol.hasInstance]:{value:videoHost.Video.__nfIs}});
        legacyGlobals=Object.freeze({...legacyGlobals,Video:LegacyVideo,NetConnection:videoHost.LegacyNetConnection,NetStream:videoHost.LegacyNetStream});
      }
      if(global.NeoFlashAS2DepthManager&&typeof player.as2_depth_manager==='function')as2DepthManagerHost=global.NeoFlashAS2DepthManager.create({player,referenceOf:target=>references.get(target),display,getRoot:()=>proxyFor([]),afterEvent:afterTimerEvent});
      if(global.NeoFlashAS2Transitions&&easingHost)as2TransitionsHost=global.NeoFlashAS2Transitions.create({wasm,timers:timerHost,easing:easingHost,EventDispatcher:as2PlatformHost.EventDispatcher,stage:stageHost.stage,getFrameRate:()=>runtimeFrameRate,isMovieClip:target=>display.alive(target)&&display.descriptor(target).owner==='movie_clip',afterEvent:afterTimerEvent,onError:timerFailure});
      if(global.NeoFlashAS2Media&&videoHost)as2MediaHost=global.NeoFlashAS2Media.create(wasm,{video:videoHost,EventDispatcher:as2PlatformHost.EventDispatcher,afterEvent:afterTimerEvent});
      if(global.NeoFlashXML){
        xmlHost=global.NeoFlashXML.create({parseLegacy:source=>wasm.parse_as2_xml_json(source),namespaceLookup:(ancestors,value,reverse)=>JSON.parse(wasm.as2_xml_namespace_json(JSON.stringify(ancestors),value,reverse)),parse:source=>{const wasm=resolveRuntimeWasm();if(typeof wasm?.parse_xml_json!=='function')throw new Error('XML parser is missing. Re-export this project.');return wasm.parse_xml_json(source);},net:netHost,onError:netFailure});
        legacyGlobals=Object.freeze({...legacyGlobals,XML:xmlHost.LegacyXML,XMLNode:xmlHost.XMLNode});
      }
      if(global.NeoFlashAIR&&typeof wasm?.AirKernel==='function'){
        airHost=global.NeoFlashAIR.create({wasm,events:{...global.NeoFlashEvents,...eventFamilies},eventFamilies,binary:binaryHost,geometry:geometryHost,net:netHost,getXML:()=>xmlHost?.XML||null,getStage:()=>stageHost?.stage||null,getDisplay:()=>display||null,getBitmap:()=>bitmapHost||null,getLoader:()=>loaderHost||null,getViewportSize:logicalSize,project:typeof project==='undefined'?null:project,afterEvent:afterTimerEvent,onError:netFailure});
      }
      if(global.NeoFlashAS3NetSys&&typeof wasm?.As3NetSysKernel==='function'){
        as3NetSysHost=global.NeoFlashAS3NetSys.create({wasm,events:{...global.NeoFlashEvents,...eventFamilies},binary:binaryHost,amf:amfHost,net:netHost,video:videoHost,storage:storageHost,external:externalHost,xml:xmlHost,display,geometry:geometryHost,getBitmap:()=>bitmapHost,getAir:()=>airHost,getBuiltins:()=>currentBuiltins,getProgram:()=>program,getViewportSize:logicalSize,afterEvent:afterTimerEvent,onError:netFailure});
      }
      if(global.NeoFlashFte3D&&typeof wasm?.As3Fte3dKernel==='function'){
        fte3dHost=global.NeoFlashFte3D.create({wasm,player,events:{...global.NeoFlashEvents,...eventFamilies},eventFamilies,binary:binaryHost,geometry:geometryHost,geometry3d:geometry3dHost,getBitmap:()=>bitmapHost,getDisplay:()=>display,getStage:()=>stageHost?.stage||null,getStageContext:()=>typeof context!=='undefined'?context:null,getVideo:()=>videoHost,proxyForRef,referenceOf:target=>references.get(target),afterEvent:afterTimerEvent,onError:netFailure});
      }
      if(global.NeoFlashAS3MediaEvents&&typeof wasm?.As3MediaEventsKernel==='function'){
        const rootProxy={
          seek(frame){
            const target=Math.max(0,Math.trunc(Number(frame)||0));
            if(typeof player.goto_frame==='function')player.goto_frame(target);
            else if(typeof player.control==='function')player.control('[]','goto_and_stop',target);
            else proxyFor([])?.gotoAndStop?.(target+1);
          }
        };
        as3MediaEventsHost=global.NeoFlashAS3MediaEvents.create({wasm,events:{...global.NeoFlashEvents,...eventFamilies},eventHub,binary:binaryHost,geometry:geometryHost,net:netHost,getAudio:()=>audioHost,getStage:()=>stageHost?.stage||null,StageClass:global.NeoFlashStage?.Stage,LoaderInfoClass:loaderHost?.LoaderInfo,airBuiltins:airHost?.builtins||null,canvas:typeof canvas==='undefined'?null:canvas,menuContainer:typeof viewport==='undefined'?null:viewport,document:activeDoc,hitTest:event=>{if(typeof canvas==='undefined')return stageHost?.stage||null;const h=hitAt(event,true);return h?.target||(h?(graphMode?proxyForRef(h):proxyFor(h)):stageHost?.stage||null);},coordinates:event=>{const p=stagePointFromEvent(event);return p?[p.x,p.y]:[Number(event?.clientX)||0,Number(event?.clientY)||0];},localPoint:(target,x,y)=>{if(!target||target===stageHost?.stage||!display)return[x,y];const p=display.convert(target,{x,y},false);return[p.x,p.y];},onBuiltInMenuAction:key=>{if(key==='play'){if(typeof player.set_playing==='function')player.set_playing(!player.is_playing());else if(typeof player.play==='function')player.play();}else if(key==='rewind'){if(typeof player.set_playing==='function')player.set_playing(false);else if(typeof player.stop==='function')player.stop();if(typeof player.seek==='function')player.seek(0);else rootProxy.seek(0);}else if(key==='forwardAndBack'){const next=Math.min(player.total_frames()-1,player.current_frame()+1);if(typeof player.seek==='function')player.seek(next);else rootProxy.seek(next);}drainSounds();drainVideos();if(typeof context!=='undefined')render();},getAccessibilityActive:()=>Boolean(typeof semanticLayer!=='undefined'&&semanticLayer?.container),onAccessibilityUpdate:()=>updateAs2Accessibility(),afterEvent:afterTimerEvent,onError:netFailure});
      }
      if(textHost)legacyGlobals=Object.freeze({...legacyGlobals,TextFormat:textHost.As2TextFormat||textHost.TextFormat});
      if(as2TextHost){
        const LegacyTextField=function(){throw new TypeError('Create an AS2 TextField with MovieClip.createTextField.');};
        LegacyTextField.StyleSheet=as2TextHost.StyleSheet;
        LegacyTextField.getFontList=as2TextHost.getFontList;
        const isTextField=display.builtins['flash.text.TextField'].__nfIs;
        Object.defineProperties(LegacyTextField,{__nfIs:{value:isTextField},[Symbol.hasInstance]:{value:isTextField}});
        legacyGlobals=Object.freeze({...legacyGlobals,TextField:LegacyTextField,TextSnapshot:as2TextHost.TextSnapshot,nativeRef:(receiver,name)=>{
          if(name==='getFontList'&&(receiver===LegacyTextField||receiver===display.builtins['flash.text.TextField']))return {has:()=>true,delete:()=>false,get:()=>as2TextHost.getFontList,set:()=>{throw new TypeError('TextField.getFontList is read-only.');}};
          const menu=as2StageHost?.menuRef(receiver,name);if(menu)return menu;
          const geometry=geometryHost.as2Ref?.(receiver,name);if(geometry)return geometry;
          const ref=references.get(receiver);if(!ref)return null;const input=display.as2Ref?.(receiver,name);if(input)return input;
          const owner=display.ownerOf?display.ownerOf(receiver):display.descriptor(receiver).owner;
          if(as2DepthManagerHost?.methodNames.includes(name)&&owner==='movie_clip')return {has:()=>true,delete:()=>false,get:()=>as2DepthManagerHost.methods(receiver)[name],set:()=>{throw new TypeError('DepthManager MovieClip methods are read-only.');}};
          if(name==='attachAudio'&&as2CaptureHost&&owner==='movie_clip')return {get:()=>value=>as2CaptureHost.attachAudio(receiver,value),has:()=>true,delete:()=>false,set:()=>{throw new TypeError('MovieClip.attachAudio is read-only.');}};
          if(name==='attachVideo'&&videoHost&&owner==='video')return {get:()=>value=>videoHost.legacyAttachVideo(receiver,value),has:()=>true,delete:()=>false,set:()=>{throw new TypeError('Video.attachVideo is read-only.');}};
          if(name==='loadVariables'&&as2NetworkHost&&owner==='movie_clip')return {get:()=>((url,method)=>as2NetworkHost.load(receiver,url,receiver,method)),has:()=>true,delete:()=>false,set:()=>{throw new TypeError('MovieClip.loadVariables is read-only.');}};if(owner!=='text')return null;
          if(name==='variable')return as2ObjectHost?.variableRef(receiver)||null;
          const property=as2TextHost.as2Ref(ref.key,name);
          if(name==='text'&&property)return {...property,get:()=>{as2ObjectHost?.syncTextVariables();return property.get();},set:value=>{const result=property.set(value);as2ObjectHost?.textChanged(receiver);return result;}};
          return property;
        }});
      }
      if(global.NeoFlashAS2Object){const ownerOf=value=>display.ownerOf?display.ownerOf(value):display.descriptor(value).owner;as2ObjectHost=global.NeoFlashAS2Object.create(wasm,{textVariable:{isText:value=>references.has(value)&&display.alive(value)&&ownerOf(value)==='text',isMovieClip:value=>references.has(value)&&display.alive(value)&&ownerOf(value)==='movie_clip',alive:value=>display.alive(value),parentOf:value=>value._parent,readText:value=>textHost.get(references.get(value).key,'text'),writeText:(value,next)=>textHost.update(references.get(value).key,{text:next}),isHTML:value=>as2TextHost.as2Ref(references.get(value).key,'html').get()},nativeRef:(receiver,name)=>legacyGlobals.nativeRef?.(receiver,name),caseKey:(receiver,name)=>legacyGlobals.caseKey(receiver,name),globals:frame=>({_root:as2RootFor(frame),_level0:proxyFor([]),_global:legacyGlobals._global}),globalProp:name=>display.as2GlobalPropRef(name),isNative:value=>references.has(value),isProject:value=>program?.enumerationKeys?.(value)!==null&&program?.enumerationKeys?.(value)!==undefined});legacyGlobals=Object.freeze({...legacyGlobals,objects:as2ObjectHost});}
      if(global.NeoFlashAS2Binding)as2BindingHost=global.NeoFlashAS2Binding.create(wasm,{EventDispatcher:as2PlatformHost.EventDispatcher});
      if(global.NeoFlashAS2Data)as2DataHost=global.NeoFlashAS2Data.create(wasm,{EventDispatcher:as2PlatformHost.EventDispatcher});
      if(global.NeoFlashAS2Services&&netHost&&xmlHost)as2ServicesHost=global.NeoFlashAS2Services.create({wasm,net:netHost,LegacyXML:xmlHost.LegacyXML,EventDispatcher:as2PlatformHost.EventDispatcher,ComponentMixins:as2BindingHost?.builtins["mx.data.binding.ComponentMixins"],onError:netFailure,afterEvent:afterTimerEvent});
      as2.setResolver((receiver,name)=>{const ref=legacyGlobals.nativeRef?.(receiver,name);return ref?ref.get():as2ObjectHost?as2ObjectHost.get(receiver,name):receiver[name];});
      as2.setKeyResolver((receiver,name)=>legacyGlobals.caseKey(receiver,name));
      if(global.NeoFlashAS2Strings){
        const wasm=resolveRuntimeWasm();
        legacyGlobals=Object.freeze({...legacyGlobals,strings:global.NeoFlashAS2Strings.create({execute:request=>wasm.as2_string_json(request)})});
      }
      if(global.NeoFlashAS2Boxing&&legacyGlobals.strings){as2BoxingHost=global.NeoFlashAS2Boxing.create({execute:request=>wasm.as2_box_json(request),strings:legacyGlobals.strings});legacyGlobals=Object.freeze({...legacyGlobals,boxing:as2BoxingHost});}
      if(global.NeoFlashAS3Strings)legacyGlobals=Object.freeze({...legacyGlobals,strings3:global.NeoFlashAS3Strings.create({execute:request=>wasm.as3_string_json(request)})});
      if(global.NeoFlashMath)legacyGlobals=Object.freeze({...legacyGlobals,math:global.NeoFlashMath.create(wasm)});
      legacyGlobals=Object.freeze({...legacyGlobals,objects3:Object.freeze({
        execute:request=>JSON.parse(wasm.as3_object_json(JSON.stringify(request))),
        arrayPlan:(method,length,index)=>JSON.parse(wasm.as3_array_plan_json(method,length,index)),
        arrays:{classes:new Map(),methods:new WeakMap()},
      })});
      if(global.NeoFlashDate)legacyGlobals=Object.freeze({...legacyGlobals,date:global.NeoFlashDate.create(wasm)});
      if(geometryHost.Color)legacyGlobals=Object.freeze({...legacyGlobals,Color:geometryHost.Color});
      enableLifecycleIfReady();
      if (initialize) initializeProgram();
    }
    function initializeProgram() {
      if(!display)return;
      const builtins=currentBuiltins={...display.builtins};
      // FrameLabel stores a Rust value and uses the existing event dispatcher.
      if(global.NeoFlashEvents){
        const wasm=resolveRuntimeWasm();
        const values=new WeakMap();
        frameLabelType=class FrameLabel extends global.NeoFlashEvents.EventDispatcher {
          constructor(name,frame){
            if(arguments.length!==2||typeof name!=='string'||!Number.isInteger(frame)||frame < -2147483648||frame > 2147483647)throw new TypeError('FrameLabel requires a primitive String name and a signed 32-bit integer frame.');
            if(typeof wasm?.frame_label_json!=='function')throw new Error('Rust FrameLabel values are unavailable; rebuild and re-export this project.');
            let value;try{value=JSON.parse(wasm.frame_label_json(name,frame));}catch(error){throw new TypeError(describeThrown(error), {cause: error});}
            super();values.set(this,value);
            Object.freeze(this);
          }
          get name(){const value=values.get(this);if(!value)throw new TypeError('Invalid FrameLabel receiver.');return value.name;}
          get frame(){const value=values.get(this);if(!value)throw new TypeError('Invalid FrameLabel receiver.');return value.frame;}
        };
        frameLabelType.__nfIs=value=>values.has(value);
        frameLabelType.__nfInitialize=()=>{throw new TypeError('FrameLabel is final and cannot initialize a subclass.');};
        Object.defineProperty(frameLabelType,Symbol.hasInstance,{value:frameLabelType.__nfIs});
        builtins['flash.display.FrameLabel']=frameLabelType;
        const sceneValues=new WeakMap(),sceneToken={};
        class Scene {
          constructor(token,value){
            if(token!==sceneToken)throw new TypeError('Scene values are produced by MovieClip.currentScene/scenes; public Scene construction is not supported.');
            sceneValues.set(this,value);Object.freeze(this);
          }
          get name(){const value=sceneValues.get(this);if(!value)throw new TypeError('Invalid Scene receiver.');return value.name;}
          get labels(){const value=sceneValues.get(this);if(!value)throw new TypeError('Invalid Scene receiver.');return value.labels;}
          get numFrames(){const value=sceneValues.get(this);if(!value)throw new TypeError('Invalid Scene receiver.');return value.numFrames;}
        }
        Scene.__nfIs=value=>sceneValues.has(value);
        Scene.__nfInitialize=()=>{throw new TypeError('Scene is final and cannot initialize a subclass.');};
        Object.defineProperty(Scene,Symbol.hasInstance,{value:Scene.__nfIs});
        sceneValueFactory=(value,labelsOverride)=>new Scene(sceneToken,{name:value.name,numFrames:value.num_frames,labels:labelsOverride?[...labelsOverride]:value.labels.map(label=>new frameLabelType(label.name,label.frame))});
        builtins['flash.display.Scene']=Scene;
      }
      builtins.__nfTrace=trace;
      if(as2StageHost){builtins.ContextMenu=as2StageHost.ContextMenu;builtins.ContextMenuItem=as2StageHost.ContextMenuItem;}
      if(as2PrintHost)builtins.PrintJob=as2PrintHost.PrintJob;
      if(as2CaptureHost){builtins.Camera=as2CaptureHost.Camera;builtins.Microphone=as2CaptureHost.Microphone;}
      if(as2ConnectionsHost){builtins.LocalConnection=as2ConnectionsHost.LocalConnection;builtins.XMLSocket=as2ConnectionsHost.XMLSocket;}
      if(as2DataHost)Object.assign(builtins,as2DataHost.builtins);
      if(as2DepthManagerHost)builtins['mx.managers.DepthManager']=as2DepthManagerHost.DepthManager;
      if(as2TransitionsHost)Object.assign(builtins,as2TransitionsHost.builtins);
      if(as2MediaHost)Object.assign(builtins,as2MediaHost.builtins);
      if(as2ServicesHost)Object.assign(builtins,as2ServicesHost.builtins);
      if(as2BindingHost)Object.assign(builtins,as2BindingHost.builtins);
      if(as2LocaleHost)Object.assign(builtins,as2LocaleHost.builtins);
      if(as2FilesHost){builtins['flash.net.FileReference']=as2FilesHost.FileReference;builtins['flash.net.FileReferenceList']=as2FilesHost.FileReferenceList;}
      if(as2TextHost){builtins['TextField.StyleSheet']=as2TextHost.StyleSheet;builtins['flash.text.TextRenderer']=as2TextHost.TextRenderer;builtins.TextSnapshot=as2TextHost.TextSnapshot;}
      if(filterHost?.DisplacementMapFilter)builtins['flash.filters.DisplacementMapFilter']=filterHost.DisplacementMapFilter;
      if(as2PlatformHost){builtins['mx.utils.Delegate']=as2PlatformHost.Delegate;builtins['mx.events.EventDispatcher']=as2PlatformHost.EventDispatcher;}
      builtins.__nfAs2Method=as2.method;builtins.__nfAs2Value=as2.value;
      builtins.__nfDisplay=display;builtins.__nfLegacy=legacyGlobals;
      builtins.__nfObjectExecute=legacyGlobals.objects3.execute;
      if(global.NeoFlashGeom3D){
        const wasm=resolveRuntimeWasm();
        geometry3dHost??=global.NeoFlashGeom3D.create(wasm,{getPoint:()=>geometryHost.Point});
        for(const name of ['Vector3D','Matrix3D','Utils3D','PerspectiveProjection','Orientation3D'])if(geometry3dHost[name])builtins['flash.geom.'+name]=geometry3dHost[name];
        builtins.__nfGeometryNumberVector=geometry3dHost.setNumberVectorFactory;
        builtins.__nfGeometryVector3DVector=geometry3dHost.setVector3DVectorFactory;
      }
      if(geometryHost.MotionColor)builtins['fl.motion.Color']=geometryHost.MotionColor;
      if(geometryHost.MatrixTransformer)builtins['fl.motion.MatrixTransformer']=geometryHost.MatrixTransformer;
      if(easingHost)Object.assign(builtins,easingHost.builtins);
      if(global.NeoFlashNumberFormat){
        const wasm=resolveRuntimeWasm();
        const numberFormat=global.NeoFlashNumberFormat.create(wasm);
        as3NetSysHost?.hookNumberFormatter?.(numberFormat.NumberFormatter);
        for(const name of ['NumberFormatter','NumberParseResult'])builtins['flash.globalization.'+name]=numberFormat[name];
      }
      if(legacyGlobals?.date){
        for(const name of ['DateTimeFormatter','DateTimeStyle','DateTimeNameStyle','DateTimeNameContext'])builtins['flash.globalization.'+name]=legacyGlobals.date[name];
        builtins.__nfDateStringVector=legacyGlobals.date.setStringVectorFactory;
      }
      if(legacyGlobals?.strings3)for(const name of ['escapeMultiByte','unescapeMultiByte'])builtins['flash.utils.'+name]=legacyGlobals.strings3[name];
      builtins.__nfHasNativeProperty=(receiver,name)=>Boolean(references.get(receiver)?.nativeHas(name));
      builtins.__nfIsNativeDisplay=receiver=>references.has(receiver);
      if(xmlHost){
        builtins.__nfXML=xmlHost;builtins.XML=xmlHost.XML;builtins.XMLList=xmlHost.XMLList;
        for(const name of ['XMLNodeType','XMLNode','XMLDocument'])if(xmlHost[name])builtins['flash.xml.'+name]=xmlHost[name];
      }
      const nativeInterface=(qname,baseIface)=>{
        const facade=function(value){
          if(new.target)throw new TypeError(`Interface ${qname} cannot be constructed.`);
          if(arguments.length!==1)throw new TypeError(`Interface ${qname} conversion expects one value.`);
          return program?program.cast(qname,value):baseIface(value);
        };
        Object.defineProperties(facade,{
          __nfIs:{value:baseIface.__nfIs},
          __nfInitialize:{value:baseIface.__nfInitialize},
          [Symbol.hasInstance]:{value:value=>program?program.is(value,qname):baseIface.__nfIs(value)},
        });
        return Object.freeze(facade);
      };
      for(const qname of ['flash.display.IBitmapDrawable','flash.display.IGraphicsData','flash.display.IGraphicsFill','flash.display.IGraphicsPath','flash.display.IGraphicsStroke']){
        if(builtins[qname])builtins[qname]=nativeInterface(qname,builtins[qname]);
      }
      if(display.setGraphicsVectorFactories)builtins.__nfGraphicsVectors=display.setGraphicsVectorFactories;
      if(global.NeoFlashEvents){
        for(const name of ['Event','MouseEvent','KeyboardEvent','FocusEvent','TextEvent','EventDispatcher','EventPhase'])builtins['flash.events.'+name]=global.NeoFlashEvents[name];
        if(global.NeoFlashEvents.IEventDispatcher)builtins['flash.events.IEventDispatcher']=nativeInterface('flash.events.IEventDispatcher',global.NeoFlashEvents.IEventDispatcher);
      }
      for(const [name,type] of Object.entries(eventFamilies))builtins['flash.events.'+name]=type;
      if(global.NeoFlashStage){
        for(const name of ['Stage','StageScaleMode','StageAlign','StageDisplayState','StageQuality'])builtins['flash.display.'+name]=global.NeoFlashStage[name];
        builtins['flash.ui.Keyboard']=stageHost?.Keyboard||global.NeoFlashStage.Keyboard;
        builtins['flash.ui.Mouse']=stageHost?.Mouse||global.NeoFlashStage.Mouse;
        for(const name of ['KeyLocation','KeyboardType','MouseCursor','MouseCursorData','ContextMenu','ContextMenuItem','ContextMenuBuiltInItems','ContextMenuClipboardItems']){const val=stageHost?.[name]||global.NeoFlashStage[name];if(val)builtins['flash.ui.'+name]=val;}
        const accProps=stageHost?.AccessibilityProperties||global.NeoFlashStage.AccessibilityProperties;
        if(accProps)builtins['flash.accessibility.AccessibilityProperties']=accProps;
        builtins.__nfMouseCursorVector=fn=>global.NeoFlashStage.setStageAdapters?.({cursorVector:fn});
      }
      if(timerHost){builtins['flash.utils.Timer']=timerHost.Timer;builtins['flash.events.TimerEvent']=timerHost.TimerEvent;builtins['flash.utils.getTimer']=timerHost.getTimer;}
      if(timerHost)for(const name of ['setInterval','setTimeout','clearInterval','clearTimeout'])builtins['flash.utils.'+name]=timerHost[name];
      if(bitmapHost){
        builtins['flash.display.BitmapData']=bitmapHost.BitmapData;builtins['flash.display.BitmapDataChannel']=bitmapHost.BitmapDataChannel;builtins['flash.display.PixelSnapping']=bitmapHost.PixelSnapping;builtins['flash.display.PNGEncoderOptions']=bitmapHost.PNGEncoderOptions;
        for(const name of ['JPEGEncoderOptions','BitmapEncodingColorSpace','JPEGXREncoderOptions'])if(bitmapHost[name])builtins['flash.display.'+name]=bitmapHost[name];
        builtins.__nfBitmapUintVector=bitmapHost.setUintVectorFactory;
        if(bitmapHost.setHistogramVectorFactory)builtins.__nfBitmapHistogramVector=bitmapHost.setHistogramVectorFactory;
      }
      else if(global.NeoFlashBitmap?.BitmapDataChannel){
        builtins['flash.display.BitmapDataChannel']=global.NeoFlashBitmap.BitmapDataChannel;
        for(const name of ['JPEGEncoderOptions','BitmapEncodingColorSpace','JPEGXREncoderOptions'])if(global.NeoFlashBitmap[name])builtins['flash.display.'+name]=global.NeoFlashBitmap[name];
      }
      const textSource=textHost||global.NeoFlashText;
      if(textSource)for(const name of ['TextFormat','TextFormatAlign','TextFormatDisplay','TextFieldType','TextFieldAutoSize','AntiAliasType','GridFitType','FontStyle','FontType','TextColorType','TextDisplayMode','TextInteractionMode','CSMSettings','TextRenderer','Font','StyleSheet','TextLineMetrics','TextSnapshot'])if(textSource[name])builtins['flash.text.'+name]=textSource[name];
      if(filterHost)for(const name of ['BitmapFilter','BlurFilter','GlowFilter','DropShadowFilter','ColorMatrixFilter','BitmapFilterQuality','ConvolutionFilter','BevelFilter','GradientGlowFilter','GradientBevelFilter','BitmapFilterType','DisplacementMapFilter','DisplacementMapFilterMode','ShaderFilter'])if(filterHost[name])builtins['flash.filters.'+name]=filterHost[name];
      else if(global.NeoFlashFilters?.DisplacementMapFilterMode)builtins['flash.filters.DisplacementMapFilterMode']=global.NeoFlashFilters.DisplacementMapFilterMode;
      if(global.NeoFlashFilters?.ShaderFilter&&!builtins['flash.filters.ShaderFilter'])builtins['flash.filters.ShaderFilter']=global.NeoFlashFilters.ShaderFilter;
      if(storageHost)for(const name of ['SharedObject','SharedObjectFlushStatus','ObjectEncoding'])builtins['flash.net.'+name]=storageHost[name];
      if(amfHost){
        if(amfHost.registerClassAlias)builtins['flash.net.registerClassAlias']=amfHost.registerClassAlias;
        if(amfHost.getClassByAlias)builtins['flash.net.getClassByAlias']=amfHost.getClassByAlias;
      }
      if(loaderHost)for(const name of ['Loader','LoaderInfo'])builtins['flash.display.'+name]=loaderHost[name];
      if(videoHost){builtins['flash.media.Video']=videoHost.Video;if(videoHost.VideoStatus)builtins['flash.media.VideoStatus']=videoHost.VideoStatus;builtins['flash.events.NetStatusEvent']=videoHost.NetStatusEvent;for(const name of ['NetConnection','NetStream'])builtins['flash.net.'+name]=videoHost[name];}
      else if(global.NeoFlashVideo?.VideoStatus)builtins['flash.media.VideoStatus']=global.NeoFlashVideo.VideoStatus;
      if(audioHost){for(const name of ['Sound','SoundChannel','SoundTransform','ID3Info','SoundLoaderContext'])if(audioHost[name])builtins['flash.media.'+name]=audioHost[name];builtins['flash.media.SoundMixer']=audioHost.SoundMixer||SoundMixer;}
      if(binaryHost){
        builtins['flash.utils.ByteArray']=binaryHost.ByteArray;
        builtins['flash.utils.Endian']=binaryHost.Endian;
        if(binaryHost.CompressionAlgorithm)builtins['flash.utils.CompressionAlgorithm']=binaryHost.CompressionAlgorithm;
        if(binaryHost.IDataInput)builtins['flash.utils.IDataInput']=nativeInterface('flash.utils.IDataInput',{__nfIs:value=>binaryHost.IDataInput.__nfIs(value)||Boolean(airHost?.isDataInput(value))||Boolean(as3NetSysHost?.isDataInput(value))||Boolean(as3MediaEventsHost?.builtins['flash.media.AVURLStream']?.__nfIs(value)),__nfInitialize:binaryHost.IDataInput.__nfInitialize});
        if(binaryHost.IDataOutput)builtins['flash.utils.IDataOutput']=nativeInterface('flash.utils.IDataOutput',{__nfIs:value=>binaryHost.IDataOutput.__nfIs(value)||Boolean(airHost?.isDataOutput(value))||Boolean(as3NetSysHost?.isDataOutput(value)),__nfInitialize:binaryHost.IDataOutput.__nfInitialize});
      }
      if(externalHost)builtins['flash.external.ExternalInterface']=externalHost.ExternalInterface;
      if(netHost){
        for(const name of ['URLRequest','URLRequestHeader','URLRequestMethod','URLVariables','URLLoader','URLLoaderDataFormat'])builtins['flash.net.'+name]=netHost[name];
        for(const name of ['navigateToURL','sendToURL'])builtins['flash.net.'+name]=netHost[name];
        for(const name of ['ProgressEvent','IOErrorEvent','HTTPStatusEvent'])builtins['flash.events.'+name]=netHost[name];
      }
      if(airHost){
        for(const [qname,type] of Object.entries(airHost.builtins)){
          if(qname==='flash.desktop.IFilePromise'||qname==='flash.security.IURIDereferencer')builtins[qname]=nativeInterface(qname,type);
          else if(!(qname in builtins))builtins[qname]=type;
        }
        airHost.augmentBuiltins?.(builtins);
        builtins.__nfAirVectorFactory=airHost.setVectorFactory;
      }
      if(as3NetSysHost){
        const isAs2Publication=Boolean(
          (typeof project!=='undefined'&&project&&(
            project.sources?.some(s=>s?.language==='action_script2'||s?.language==='action_script1'||s?.script?.language==='action_script2'||s?.script?.language==='action_script1')||
            project.symbols?.some(sym=>sym?.timeline?.scripts?.some(e=>e?.script?.language==='action_script2'||e?.script?.language==='action_script1'))
          ))||
          (typeof functions!=='undefined'&&typeof functions?.keys==='function'&&[...functions.keys()].some(k=>/^(?:action_script[12]|ActionScript[12])\u0000/.test(k)))||
          (typeof bundle!=='undefined'&&typeof bundle?.program?.code==='string'&&bundle.program.code.includes('"as2"'))
        );
        for(const [qname,type] of Object.entries(as3NetSysHost.builtins)){
          if((qname==='flash.net.FileReference'||qname==='flash.net.FileReferenceList')&&isAs2Publication)continue;
          if(qname.startsWith('flash.desktop.Clipboard')&&(qname in builtins))continue;
          if(qname.startsWith('flash.printing.')&&(qname in builtins))continue;
          builtins[qname]=type;
        }
        builtins.__nfAs3NetSysVectorFactory=as3NetSysHost.setVectorFactory;
      }
      if(fte3dHost){
        for(const [qname,type] of Object.entries(fte3dHost.builtins)){
          if(qname==='flash.text.ime.IIMEClient')builtins[qname]=nativeInterface(qname,type);
          else builtins[qname]=type;
        }
        builtins.__nfFte3dVectorFactory=fte3dHost.setVectorFactory;
      }
      if(as3MediaEventsHost){
        for(const [qname,type] of Object.entries(as3MediaEventsHost.builtins)){
          if(qname==='flash.accessibility.ISearchableText'||qname==='flash.accessibility.ISimpleTextSelection')builtins[qname]=nativeInterface(qname,type);
          else builtins[qname]=type;
        }
        builtins.__nfMediaEventsVectorFactory=as3MediaEventsHost.setVectorFactory;
      }
      program=typeof bundle!=='undefined'&&bundle.program ? (typeof compiledProgramFn==='function'?compiledProgramFn:(typeof compileProgramUnit==='function'?compileProgramUnit(bundle.program):new Function('__nfBuiltins', bundle.program.code + '\n//# sourceURL=' + (bundle.program.source_url || 'neoflash-project.js'))))(builtins) : {
        type:name=>{if(!builtins[name])throw new TypeError(`Unsupported built-in class ${name}.`);return builtins[name];},
        is:(value,name)=>Boolean(builtins[name]?.__nfIs?.(value)),
      };
      if(typeof instanceParameters!=='undefined'&&instanceParameters&&typeof instanceParameters==='object'){
        try{
          const rootClip=proxyFor([]);
          for(const [paramKey,paramVal] of Object.entries(instanceParameters)){
            if(paramKey&&!(paramKey in rootClip)){
              try{rootClip[paramKey]=paramVal===undefined||paramVal===null?'':String(paramVal);}catch(_){}
            }
          }
        }catch(_){}
      }
    }
    function cleanupAdapters(method) {
      // Invalidate pending render work first; every adapter must get a chance to
      // cancel its own async work even when another adapter cannot clean up.
      const hosts = [as2PrintHost, as2BoxingHost, as2StageHost, as2ConnectionsHost,
        as2DepthManagerHost, as2TransitionsHost, as2MediaHost, as2ServicesHost,
        as2FilesHost, as2NetworkHost, as2LocaleHost, as2TextHost, as2BindingHost,
        as2DataHost, as2ObjectHost, as3MediaEventsHost, fte3dHost, as3NetSysHost,
        airHost, externalHost, domStageHost, textInputHost, storageHost, loaderHost,
        xmlHost, videoHost, as2CaptureHost, audioHost, netHost, easingHost,
        broadcasterHost, timerHost, binaryHost, bitmapHost, textHost];
      timerRenderGeneration++;
      completeCleanup([cancelTimerRender, ...hosts.map(host => () => host?.[method]?.()),
        () => amfHost?.reset?.()]);
    }
    function guardHostCallback(callback) {
      return (...args) => {
        if(familyDisposed||(typeof failed!=='undefined'&&failed))return;
        try { return callback(...args); } catch(error) { fail(error); }
      };
    }
    function mediaGesture(host, method, ...args) {
      const generation=timerRenderGeneration;
      Promise.resolve(host[method](...args)).catch(error => {
        if(!familyDisposed&&generation===timerRenderGeneration)audioFailure(error);
      });
    }
    function clearFamilyState() {
      proxies.clear(); attachedClasses.clear(); constructedNodes.clear(); superConstructedNodes.clear();
      offstageLinkedTargets.clear(); retiringTargets.clear(); pendingRetirementRefs.clear(); constructSessions.length = 0;
      frameLabelDispatchedScopes.clear(); boundTextVariables.clear(); registeredClasses.clear();
      executedInitClips.clear(); clipFrameLabels.clear(); callFrameDepth = 0;
      if(legacyGlobals?.__nfCallStack)legacyGlobals.__nfCallStack.length=0;
    }
    function clearPainters() {
      const capture = capturePainter, island = islandPainter;
      capturePainter=null; islandPainter=null; inputPaths=null;
      completeCleanup([() => capture?.clear(), () => island?.clear(), () => filterHost?.renderer.clear()]);
    }
    function resetProject() {
      pointerOverContent=false;legacyCursorVisible=true;
      runtimeFrameRate = typeof project === 'undefined' ? 24 : project.fps;
      completeCleanup([
        updateCursor, clearPainters, () => cleanupAdapters('reset'), clearFamilyState,
        () => { if(typeof gradients!=='undefined')gradients.clear(); },
        () => { if(typeof clippings!=='undefined')clippings.clear(); },
        () => { if(typeof paths!=='undefined')paths?.paths?.clear?.(); },
        () => { if(typeof semanticLayer!=='undefined')semanticLayer?.clear?.(); },
        () => { if(typeof a11yOverlay!=='undefined'&&a11yOverlay){if(typeof a11yOverlay.replaceChildren==='function')a11yOverlay.replaceChildren();else if('textContent' in a11yOverlay)a11yOverlay.textContent='';} },
        () => display?.clear(), () => eventHub?.clear(), () => stageHost?.reset(),
        () => legacyHost?.reset(), () => as2PlatformHost?.reset(),
        () => { for(const key of Object.keys(legacyGlobals?._global||{}))delete legacyGlobals._global[key]; },
      ]);
      lastTrace = '';lastTraceLocation = '';
      hostStageLayoutPinned=false;
      syncInitialStageLayout();
      if(typeof resize==='function')resize();
      soundsPreloaded=false;
      player.reset();
      if(hostPlayerConfig.autoplay===false)player.set_playing?.(false);
      enableLifecycleIfReady();
      initializeProgram();
      dispatchScripts();
    }
    function disposeFamily() {
      if(familyDisposed)return;
      pointerOverContent=false;legacyCursorVisible=true;
      familyDisposed=true;
      const activeDoc=typeof instanceDoc!=='undefined'&&instanceDoc?instanceDoc:(typeof document==='undefined'?null:document);
      completeCleanup([
        () => { if(typeof canvas!=='undefined'&&canvas)canvas.style.cursor='default'; },
        () => {
          const handler=fontLoadingDoneHandler; fontLoadingDoneHandler=null;
          if(handler&&activeDoc)activeDoc.fonts?.removeEventListener?.('loadingdone',handler);
        },
        () => {
          if(global&&typeof global==='object'&&global.__neoflashConsoleFormat===consoleFormatHook){
            delete global.__neoflashConsoleFormat;
          }
        },
        clearPainters, () => cleanupAdapters('dispose'), clearFamilyState,
        () => as2PlatformHost?.dispose(), () => legacyHost?.dispose(), () => stageHost?.dispose(),
        () => eventHub?.clear(), () => display?.clear(),
      ]);
    }
    function as2RootFor(target) {
      const ref=references.get(target);
      return graphMode&&ref?.objectRef&&typeof player.as2_root==='function'?proxyForRef(JSON.parse(player.as2_root(ref.key))):proxyFor([]);
    }
    function enableLifecycleIfReady() {
      if (!hasLifecycleApi) return;
      player.enable_lifecycle();
      if (typeof player.reset_generation === 'function') knownResetGeneration = player.reset_generation();
    }
    function syncResetGeneration(stepGeneration = null) {
      if (!hasLifecycleApi) return;
      const current = stepGeneration !== null ? stepGeneration : (typeof player.reset_generation === 'function' ? player.reset_generation() : knownResetGeneration);
      if (current === knownResetGeneration) return;
      knownResetGeneration = current;
      proxies.clear(); attachedClasses.clear(); constructedNodes.clear(); superConstructedNodes.clear(); offstageLinkedTargets.clear(); retiringTargets.clear(); pendingRetirementRefs.clear(); constructSessions.length = 0; frameLabelDispatchedScopes.clear(); boundTextVariables.clear(); clipFrameLabels.clear();
      display?.clear(); eventHub?.clear();
    }
    function nodeKeyOf(node) {
      return graphMode ? JSON.stringify(node.object_ref) : JSON.stringify(node.path);
    }
    function nodeLifetimeOf(node) {
      return graphMode ? node.object_ref.generation : node.lifetime_id;
    }
    function hasAnyAuthoredTextVariables() {
      return Boolean(graphMode&&as2ObjectHost&&typeof project!=='undefined'&&project?.symbols?.some(s=>s?.timeline?.layers?.some(l=>l?.keyframes?.some(kf=>kf?.elements?.some(el=>el?.kind?.format?.fieldOptions?.variable||el?.kind?.format?.field_options?.variable)))));
    }
    function assignTimelineChild(parentProxy, childNode, childProxy) {
      const name = childNode?.name;
      if (!parentProxy || !childProxy || !name) return;
      if (program?.hasFixedProperty?.(parentProxy, name)) parentProxy[name] = childProxy;
    }
    function createConstructSession(events = []) {
      const addedByTarget = new Map();
      const addedToStage = [];
      const retired = [];
      const added = [];
      for (const item of events) {
        if (item.kind === 'added') {
          added.push(item);
          const k = JSON.stringify(item.target_ref);
          let list = addedByTarget.get(k);
          if (!list) addedByTarget.set(k, list = []);
          list.push(item);
        } else if (item.kind === 'added_to_stage') {
          addedToStage.push(item);
        } else if (item.kind === 'removed' || item.kind === 'removed_from_stage') {
          retired.push(item);
        }
      }
      return { added, addedByTarget, addedToStage, retired, deliveredAdded: new Set() };
    }
    function deliverAddedFor(targetKey, session) {
      if (!session) return;
      const list = session.addedByTarget.get(targetKey);
      if (!list?.length) return;
      const pending = [];
      for (const item of list) {
        if (!session.deliveredAdded.has(item)) {
          session.deliveredAdded.add(item);
          pending.push(item);
        }
      }
      if (pending.length) deliverDisplayEvents(pending);
    }
    function pruneDeadRuntimeCaches() {
      if (!graphMode) {
        for (const key of constructedNodes.keys()) {
          if (player.lifetime_id(key) !== constructedNodes.get(key)) { constructedNodes.delete(key); superConstructedNodes.delete(key); }
        }
        return;
      }
      for (const key of attachedClasses.keys()) if (!player.object_alive(key)) attachedClasses.delete(key);
      for (const key of constructedNodes.keys()) if (!player.object_alive(key)) constructedNodes.delete(key);
      for (const key of superConstructedNodes.keys()) if (!player.object_alive(key)) superConstructedNodes.delete(key);
      for (const key of offstageLinkedTargets.keys()) if (!player.object_alive(key)) offstageLinkedTargets.delete(key);
      for (const key of boundTextVariables.keys()) if (!player.object_alive(key)) boundTextVariables.delete(key);
      for (const [key,entry] of proxies) if (!player.object_alive(key)&&!pendingRetirementRefs.has(key)&&!retiringTargets.has(entry.proxy)) proxies.delete(key);
      display?.prune();
      if(!pendingRetirementRefs.size)eventHub?.prune();
    }
    function directChildrenOf(node, nodes, byKey) {
      if (graphMode && Array.isArray(node.children)) {
        const result = [];
        for (const childRef of node.children) {
          const child = byKey.get(JSON.stringify(childRef));
          if (child) result.push(child);
        }
        return result;
      }
      if (!graphMode) {
        const prefix = node.path || [];
        return nodes.filter(candidate => Array.isArray(candidate.path) && candidate.path.length === prefix.length + 1 && prefix.every((seg, i) => candidate.path[i] === seg));
      }
      const key = nodeKeyOf(node);
      return nodes.filter(candidate => candidate.parent_ref && JSON.stringify(candidate.parent_ref) === key);
    }
    function nodeRegistrationInfo(node) {
      const symbolId = node.symbol_id || node.kind?.symbol_id;
      const symLinkage = symbolId && registeredClasses.size > 0 && typeof project !== 'undefined' ? project?.symbols?.find(s => s.id === symbolId)?.linkage_id : null;
      const hasRegistered = Boolean(symLinkage && registeredClasses.has(symLinkage));
      return { symLinkage, hasRegistered };
    }
    function attachLinkedClass(className, receiver, key, lifetime, symLinkage, hasRegistered) {
      if (attachedClasses.get(key) === lifetime) return;
      attachedClasses.set(key, lifetime);
      if (hasRegistered) {
        const registered = registeredClasses.get(symLinkage);
        if (registered !== null) {
          if (registered.kind === 'project') program.attach(registered.name, receiver);
          else {
            if (registered.ctor.prototype && typeof registered.ctor.prototype === 'object') Object.setPrototypeOf(receiver, registered.ctor.prototype);
            Object.defineProperty(receiver, '__constructor__', { configurable: true, writable: true, enumerable: false, value: registered.ctor });
            Reflect.apply(registered.ctor, receiver, []);
          }
          display.refreshLegacyCallbacks(receiver, true);
        }
      } else if (className) {
        if (!program?.attach) throw new Error(`Linked class ${className} has no compiled project program.`);
        program.attach(className, receiver);
        if (program.dialect?.(className) === 'as2') display.refreshLegacyCallbacks(receiver, true);
      }
    }
    function attachNodeClass(node, receiver, key, lifetime, symLinkage, hasRegistered) {
      attachLinkedClass(node.class_name, receiver, key, lifetime, symLinkage, hasRegistered);
    }
    const constructingNodeKeys = new Set();
    function constructChildrenOf(node, session, nodes, byKey, hasTextVars, needAddedProxy) {
      for (const childNode of directChildrenOf(node, nodes, byKey)) {
        constructSubtree(childNode, session, nodes, byKey, hasTextVars, needAddedProxy);
      }
    }
    function constructSubtree(node, session, nodes, byKey, hasTextVars, needAddedProxy) {
      const key = nodeKeyOf(node), lifetime = nodeLifetimeOf(node);
      const isCurrent=()=>graphMode?player.object_alive(key):player.lifetime_id(key)===lifetime;
      if(!isCurrent())return;
      if (constructedNodes.get(key) === lifetime) {
        constructChildrenOf(node, session, nodes, byKey, hasTextVars, needAddedProxy);
        return;
      }
      constructedNodes.set(key, lifetime);
      if (hasTextVars && node.owner === 'text' && node.text?.variable && !node.text?.html) {
        const tLife = node.object_ref.generation;
        if (boundTextVariables.get(key) !== tLife) {
          boundTextVariables.set(key, tLife);
          as2ObjectHost.variableRef(proxyForRef(node.object_ref))?.set(node.text.variable);
        }
      }
      if(!isCurrent())return;
      const { symLinkage, hasRegistered } = nodeRegistrationInfo(node);
      const hasClass = Boolean(node.class_name || hasRegistered);
      const hasChildren = Boolean(node.children?.length || (!graphMode && nodes.some(c => c.path.length === node.path.length + 1 && node.path.every((seg, i) => c.path[i] === seg))));
      const hasAddedEvent = Boolean(session?.addedByTarget.has(key));
      const needsProxy = hasClass || hasChildren || Boolean(node.name) || (hasAddedEvent && needAddedProxy);
      const receiver = needsProxy ? (graphMode ? proxyForRef(node.object_ref) : proxyFor(node.path)) : null;
      if (receiver && node.name) constructedNames.set(receiver, node.name);
      constructingNodeKeys.add(key);
      try {
        if (hasClass) {
          attachNodeClass(node, receiver, key, lifetime, symLinkage, hasRegistered);
          if(!isCurrent())return;
          if (superConstructedNodes.get(key) !== lifetime) {
            superConstructedNodes.set(key, lifetime);
            constructChildrenOf(node, session, nodes, byKey, hasTextVars, needAddedProxy);
          }
        } else {
          superConstructedNodes.set(key, lifetime);
          if (hasChildren) constructChildrenOf(node, session, nodes, byKey, hasTextVars, needAddedProxy);
        }
      } finally {
        constructingNodeKeys.delete(key);
      }
      if(!isCurrent())return;
      if (receiver) {
        const parentNode = graphMode
          ? (node.parent_ref ? byKey.get(JSON.stringify(node.parent_ref)) : null)
          : (node.path.length ? byKey.get(JSON.stringify(node.path.slice(0, -1))) : null);
        if (parentNode&&(graphMode?player.object_alive(nodeKeyOf(parentNode)):player.lifetime_id(nodeKeyOf(parentNode))===nodeLifetimeOf(parentNode))) {
          const parentProxy = graphMode ? proxyForRef(parentNode.object_ref) : proxyFor(parentNode.path);
          assignTimelineChild(parentProxy, node, receiver);
        }
      }
      deliverAddedFor(key, session);
    }
    function handleSuperConstruct(receiver) {
      if (!display || !receiver || !references.has(receiver)) return;
      const ref = references.get(receiver);
      const key = ref.key;
      let node = null, nodes = null;
      try { nodes = display.nodes(); node = (nodes.__byKey ? nodes.__byKey.get(key) : nodes.find(n => nodeKeyOf(n) === key)) || null; } catch (_) { return; }
      if (!node) return;
      const lifetime = nodeLifetimeOf(node);
      if (superConstructedNodes.get(key) === lifetime) return;
      superConstructedNodes.set(key, lifetime);
      const byKey = nodes.__byKey || new Map(nodes.map(n => [nodeKeyOf(n), n]));
      let session = constructSessions[constructSessions.length - 1] || null;
      let localSession = null;
      const wasOffstageLinked = offstageLinkedTargets.delete(key);
      if (wasOffstageLinked && graphMode && typeof player.display_events_json === 'function') {
        const extra = JSON.parse(player.display_events_json()).events || [];
        if (session) {
          for (const item of extra) {
            if (item.kind === 'added') {
              session.added.push(item);
              const k = JSON.stringify(item.target_ref);
              let list = session.addedByTarget.get(k);
              if (!list) session.addedByTarget.set(k, list = []);
              list.push(item);
            } else deliverDisplayEvents([item]);
          }
        } else {
          localSession = session = createConstructSession(extra);
        }
      }
      const needAddedProxy = Boolean(eventHub && (eventHub.targetsWith('added')?.length || eventHub.targetsWith('removed')?.length || eventHub.targetsWith('addedToStage')?.length || eventHub.targetsWith('removedFromStage')?.length));
      if (localSession) constructSessions.push(localSession);
      try {
        constructChildrenOf(node, session, nodes, byKey, hasAnyAuthoredTextVariables(), needAddedProxy);
      } finally {
        if (localSession) constructSessions.pop();
      }
      if (localSession) {
        const leftover = localSession.added.filter(item => !localSession.deliveredAdded.has(item));
        if (leftover.length) deliverDisplayEvents(leftover);
        if (localSession.addedToStage.length) deliverDisplayEvents(localSession.addedToStage);
      }
    }
    function runConstructEvents(events = [], allowAddedToStage = true) {
      const session = createConstructSession(events);
      if (session.retired.length) deliverDisplayEvents(session.retired, { timelineRetired: true });
      pruneDeadRuntimeCaches();
      if (!display) {
        deliverDisplayEvents(session.added);
        if (allowAddedToStage) deliverDisplayEvents(session.addedToStage);
        return;
      }
      const nodes = display.nodes();
      const byKey = nodes.__byKey || new Map(nodes.map(n => [nodeKeyOf(n), n]));
      const isConstructed = n => constructedNodes.get(nodeKeyOf(n)) === nodeLifetimeOf(n);
      const parentOfNode = n => graphMode
        ? (n.parent_ref ? byKey.get(JSON.stringify(n.parent_ref)) : null)
        : (n.path.length ? byKey.get(JSON.stringify(n.path.slice(0, -1))) : null);
      const roots = [];
      for (const node of nodes) {
        if (isConstructed(node)) continue;
        const parentNode = parentOfNode(node);
        if (!parentNode || isConstructed(parentNode)) roots.push(node);
      }
      const hasTextVars = hasAnyAuthoredTextVariables();
      const needAddedProxy = Boolean(eventHub && (eventHub.targetsWith('added')?.length || eventHub.targetsWith('removed')?.length || eventHub.targetsWith('addedToStage')?.length || eventHub.targetsWith('removedFromStage')?.length));
      constructSessions.push(session);
      try {
        for (const rootNode of roots) constructSubtree(rootNode, session, nodes, byKey, hasTextVars, needAddedProxy);
      } finally {
        constructSessions.pop();
      }
      const remainingAdded = session.added.filter(item => !session.deliveredAdded.has(item));
      if (remainingAdded.length) deliverDisplayEvents(remainingAdded);
      if (allowAddedToStage && session.addedToStage.length) deliverDisplayEvents(session.addedToStage);
    }
    function prepareLinked(receiver,allocatedByProgram,linkageId=null) {
      const nodeClassName=display.descriptor(receiver)?.class_name||null;
      const hasRegistered=linkageId!==null&&registeredClasses.has(linkageId);
      if(!hasRegistered&&!program?.attach&&nodeClassName)throw new Error('Linked symbol class construction requires an initialized project program; creation during static field initialization is not supported yet.');
      const ref=references.get(receiver);
      if(allocatedByProgram){
        attachedClasses.set(ref.key,ref.lifetime);
        constructedNodes.set(ref.key,ref.lifetime);
        if(ref.key)offstageLinkedTargets.add(ref.key);
        return;
      }
      constructedNodes.set(ref.key,ref.lifetime);
      if(hasRegistered||nodeClassName)attachLinkedClass(nodeClassName,receiver,ref.key,ref.lifetime,linkageId,hasRegistered);
      handleSuperConstruct(receiver);
    }
    function attachNewInstances(scope=null) {
      if(!display)return;
      display.prune();if(!pendingRetirementRefs.size)eventHub?.prune();
      if(!scope){
        for(const key of attachedClasses.keys())if(graphMode?!player.object_alive(key):!player.lifetime_id(key))attachedClasses.delete(key);
        if(graphMode)for(const key of boundTextVariables.keys())if(!player.object_alive(key))boundTextVariables.delete(key);
      }
      const hasSymbolClasses=Boolean(typeof project!=='undefined'&&project?.symbols?.some(s=>s.class_name));
      const hasAuthoredTextVariables=Boolean(graphMode&&as2ObjectHost&&typeof project!=='undefined'&&project?.symbols?.some(s=>s?.timeline?.layers?.some(l=>l?.keyframes?.some(kf=>kf?.elements?.some(el=>el?.kind?.format?.fieldOptions?.variable||el?.kind?.format?.field_options?.variable)))));
      const needRootClass=Boolean(typeof project!=='undefined'&&project?.document_class&&attachedClasses.size===0);
      const needNodeScan=!graphMode||Boolean(scope)||hasSymbolClasses||hasAuthoredTextVariables||registeredClasses.size>0||needRootClass;
      if(needNodeScan){
        const nodes=display.nodes(),byRef=nodes.__byKey||new Map(nodes.map(node=>[JSON.stringify(node.object_ref),node]));
        const depth=node=>{let result=0;while(node?.parent_ref){node=byRef.get(JSON.stringify(node.parent_ref));result++;}return result;};
        const withinScope=node=>{
          if(!scope)return true;
          const key=references.get(scope).key;
          for(let current=node;current;current=byRef.get(JSON.stringify(current.parent_ref)))if(JSON.stringify(current.object_ref)===key)return true;
          return false;
        };
        const candidates=[];
        for(const node of nodes){
          if(graphMode&&!withinScope(node))continue;
          if(hasAuthoredTextVariables&&node.owner==='text'&&node.text?.variable&&!node.text?.html){
            const tKey=JSON.stringify(node.object_ref),tLife=node.object_ref.generation;
            if(boundTextVariables.get(tKey)!==tLife){
              boundTextVariables.set(tKey,tLife);
              as2ObjectHost.variableRef(proxyForRef(node.object_ref))?.set(node.text.variable);
            }
          }
          const symbolId=node.symbol_id||node.kind?.symbol_id;
          const symLinkage=symbolId&&registeredClasses.size>0&&typeof project!=='undefined'?project?.symbols?.find(s=>s.id===symbolId)?.linkage_id:null;
          const hasRegistered=Boolean(symLinkage&&registeredClasses.has(symLinkage));
          if(graphMode&&node.authored_path===null&&!node.class_name&&!hasRegistered)continue;
          if(!node.class_name&&!hasRegistered)continue;
          const key=graphMode?JSON.stringify(node.object_ref):JSON.stringify(node.path),previous=attachedClasses.get(key);
          const lifetime=graphMode?node.object_ref.generation:node.lifetime_id;
          if(previous===lifetime)continue;
          candidates.push({node,key,lifetime,symLinkage,hasRegistered});
        }
        // Initialize authored descendants in each namespace before their linked
        // container constructor. Native allocation reserves its own root first.
        candidates.sort((a,b)=>graphMode?depth(b.node)-depth(a.node):b.node.path.length-a.node.path.length);
        for(const {node,key,lifetime,symLinkage,hasRegistered} of candidates) {
          if(graphMode?!player.object_alive(key):player.lifetime_id(key)!==lifetime)continue;
          if(attachedClasses.get(key)===lifetime)continue;
          attachedClasses.set(key,lifetime);
          const receiver=graphMode?proxyForRef(node.object_ref):proxyFor(node.path);
          const registered=hasRegistered?registeredClasses.get(symLinkage):undefined;
          if(hasRegistered){
            if(registered!==null){
              if(registered.kind==='project')program.attach(registered.name,receiver);
              else{
                if(registered.ctor.prototype&&typeof registered.ctor.prototype==='object')Object.setPrototypeOf(receiver,registered.ctor.prototype);
                Object.defineProperty(receiver,'__constructor__',{configurable:true,writable:true,enumerable:false,value:registered.ctor});
                Reflect.apply(registered.ctor,receiver,[]);
              }
              display.refreshLegacyCallbacks(receiver,true);
            }
          }else if(node.class_name){
            if(!program?.attach)throw new Error(`Linked class ${node.class_name} has no compiled project program.`);
            program.attach(node.class_name,receiver);
            if(program.dialect?.(node.class_name)==='as2')display.refreshLegacyCallbacks(receiver,true);
          }
        }
      }
      if(scope)return;
      drainDisplayEvents();
      // Canonical refs are never reused; retire cache entries without retaining every
      // prior authored placement forever. Externally held stale proxies still reject access.
      if(graphMode)for(const[key,entry]of proxies)if(!player.object_alive(key)&&!pendingRetirementRefs.has(key)&&!retiringTargets.has(entry.proxy))proxies.delete(key);
    }
    function deliverDisplayEvents(events, options = null) {
      if(!eventHub||!events?.length)return;
      const rootRef=JSON.parse(player.object_ref('[]')),rootKey=JSON.stringify(rootRef);
      const types={added:'added',removed:'removed',added_to_stage:'addedToStage',removed_from_stage:'removedFromStage'};
      const timelineRetired=Boolean(options?.timelineRetired);
      // Nested navigation must not discard later siblings from this captured packet.
      // Retention does not make them live for property access or frame broadcasts.
      const retained=new Set();
      for(const item of events)if(item.kind==='removed'||item.kind==='removed_from_stage'){
        retained.add(JSON.stringify(item.target_ref));
        for(const ref of item.route)retained.add(JSON.stringify(ref));
      }
      for(const key of retained)pendingRetirementRefs.set(key,(pendingRetirementRefs.get(key)||0)+1);
      try{
      for(const item of events) {
        const key=JSON.stringify(item.target_ref);
        const type=types[item.kind];if(!type)throw new Error(`Unsupported display lifecycle event ${item.kind}.`);
        const isRetirement=type==='removed'||type==='removedFromStage';
        const aliveNow=player.object_alive(key);
        if(!aliveNow&&!isRetirement)continue;
        const target=aliveNow?proxyForRef(item.target_ref):proxies.get(key)?.proxy;
        if(!target)continue;
        if(type==='addedToStage'&&item.route.length>1){
          // ADDED may detach the original parent before its pending Stage event.
          // Test that parent, rather than the child: self-removal from a parent
          // still on Stage retains the reference dispatch behavior.
          const parentRef=item.route[1],parentKey=JSON.stringify(parentRef);
          if(!player.object_alive(parentKey)||display.descriptor(proxyForRef(parentRef)).on_stage===false)continue;
        }
        const route=item.route.map(ref=>{const rk=JSON.stringify(ref);return player.object_alive(rk)?proxyForRef(ref):proxies.get(rk)?.proxy;}).filter(Boolean);
        if(!route.length||route[0]!==target)route.unshift(target);
        const stageRoute=typeof item.stage_route==='boolean'?item.stage_route:item.route.some(ref=>JSON.stringify(ref)===rootKey);
        if(stageRoute&&stageHost)route.push(stageHost.stage);
        const temporarilyAlive=[];
        if(isRetirement){
          for(const entry of route){
            if(!retiringTargets.has(entry)){
              const entryRef=references.get(entry);
              if(entryRef&&!player.object_alive(entryRef.key)){retiringTargets.add(entry);temporarilyAlive.push(entry);}
            }
          }
          if(!aliveNow&&!retiringTargets.has(target)){retiringTargets.add(target);temporarilyAlive.push(target);}
        }
        try {
          eventHub.dispatch(target,new global.NeoFlashEvents.Event(type,type==='added'||type==='removed'),route);
          display?.lifecycle?.(item.kind,target);
        } finally {
          for(const entry of temporarilyAlive)retiringTargets.delete(entry);
        }
        if(timelineRetired&&type==='removed'&&item.route.length>=2){
          const childName=constructedNames.get(target);
          if(childName){
            const pRef=item.route[1],pKey=JSON.stringify(pRef);
            if(player.object_alive(pKey)){
              const parentProxy=proxyForRef(pRef);
              if(program?.hasFixedProperty?.(parentProxy,childName)&&parentProxy[childName]===target)parentProxy[childName]=null;
            }
          }
        }
      }
      }finally{
        for(const key of retained){
          const count=pendingRetirementRefs.get(key)||0;
          if(count>1)pendingRetirementRefs.set(key,count-1);else pendingRetirementRefs.delete(key);
        }
      }
    }
    function drainDisplayEvents() {
      if(graphMode&&typeof player.display_events_json==='function')deliverDisplayEvents(JSON.parse(player.display_events_json()).events);
    }
    function semanticRoute(node,list){
      if(node.owner_ref&&!player.object_alive(JSON.stringify(node.owner_ref)))return null;
      const owner=node.owner_ref?proxyForRef(node.owner_ref):proxyFor(node.instance_path);
      const route=display?.inputRoute?display.inputRoute(owner,list):{owner,target:owner};
      return route?.owner===owner?route:null;
    }
    function activateSemantic(node) {
      const route=semanticRoute(node);if(!route)return false;
      const accepted=node.owner_ref&&typeof player.object_activate_accessible==='function'
        ?player.object_activate_accessible(JSON.stringify(node.owner_ref))
        :player.activate_accessible(JSON.stringify(node.instance_path),node.lifetime_id);
      if(!accepted)return false;
      const target=route.target;
      // Native semantic buttons have one click activation and no Canvas pointerdown.
      // Deliver the legacy press action once, alongside the AS3 click route.
      if(display){display.pointer('mouseDown',target);display.pointer('mouseUp',target);}
      if(stageHost&&display.alive(target)){stageHost.stage.focus=target;stageHost.handlePointer('click',{},target);}
      dispatchScripts(route.owner);if(typeof context!=='undefined')render();return true;
    }
    function interactiveTarget(hit) {
      let target=graphMode?proxyForRef(hit):proxyFor(hit);
      return display.resolveInput(target);
    }

    function dispatchFrameLabelEvents() {
      if(!eventHub||!global.NeoFlashEvents||!clipFrameLabels.size)return;
      for(const [cacheKey,rec] of clipFrameLabels){
        if(!rec.live()){clipFrameLabels.delete(cacheKey);continue;}
        const {scene,frame}=rec.currentSceneAndFrame();
        if(scene===rec.lastScene&&frame===rec.lastFrame)continue;
        rec.lastScene=scene;rec.lastFrame=frame;
        const labels=rec.labelsByScene.get(scene||'');
        if(!labels)continue;
        for(const label of labels){
          if(label.frame===frame&&label.hasEventListener('frameLabel')){
            label.dispatchEvent(new global.NeoFlashEvents.Event('frameLabel',false,false));
          }
        }
      }
    }
    function broadcastDisplayPhase(eventType) {
      if(!display||!stageHost)return;
      const targets=()=>{
        if(graphMode&&typeof display.snapshot==='function'){
          const objects=display.snapshot()?.objects;
          if(Array.isArray(objects))return objects.map(obj=>proxyForRef(obj.object_ref));
        }
        return display.nodes().map(node=>display.nodeProxy(node));
      };
      stageHost.broadcastPhase(eventType,targets);
    }
    function broadcastFrame() {
      if(hasLifecycleApi){
        syncResetGeneration();
        try{
          if(display){
            display.enterFrame();
            broadcastDisplayPhase(global.NeoFlashEvents?.Event?.ENTER_FRAME||'enterFrame');
          }
        }
        catch(error){const cause=unwrapScriptThrow(error);throw new Error(`EnterFrame callback: ${describeThrown(cause)}`,{cause});}
        dispatchScripts();
        return;
      }
      attachNewInstances();
      try {
        if(display){
          display.enterFrame();
          if(stageHost)stageHost.tick(()=>display.nodes().map(node=>display.nodeProxy(node)),dispatchFrameLabelEvents);
          else dispatchFrameLabelEvents();
        }
      }
      catch(error){const cause=unwrapScriptThrow(error);throw new Error(`EnterFrame callback: ${describeThrown(cause)}`,{cause});}
      dispatchScripts();
    }
    const as2 = createAs2Methods();
    const percentage = new Set(['_alpha', '_xscale', '_yscale']);
    function as2Visibility(value) {
      // AVM1's legacy display-property setter uses a number, not ToBoolean.
      // Evidence: Ruffle fe6cfc545405b888ebb478ce1c211d63147ca49b,
      // core/src/avm1/object/stage_object.rs (set_visible / property_coerce_to_number).
      // null means preserve the current value. Never invoke object conversion
      // hooks: their AVM1 semantics differ from JavaScript's Number(value).
      if (value === null || value === undefined) return null;
      if (typeof value === 'boolean') return value;
      if (typeof value === 'string') {
        // Bounded ordinary decimal mantissas; legacy radix, exponent, whitespace
        // and other parser quirks require their own compatibility implementation.
        if (value === '' || /^[+-]?[A-Za-z]+$/.test(value)) return null;
        if (!/^[+-]?(?:(?:0|[1-9][0-9]*)(?:\.[0-9]*)?|\.[0-9]+)$/.test(value)) {
          throw new TypeError('AS2 _visible string conversion currently supports plain signed decimal strings without exponent, radix, extra leading zeroes or whitespace. Pass a number or Boolean for other forms.');
        }
        value = Number(value);
      }
      if (typeof value !== 'number') {
        throw new TypeError('AS2 _visible object, function, Symbol and BigInt conversions are not supported. Pass a Boolean, number, or supported decimal string explicitly.');
      }
      return Number.isNaN(value) ? null : value !== 0;
    }
    function proxyFor(path) {
      if(graphMode){const ref=JSON.parse(player.object_ref(JSON.stringify(path)));if(!ref)throw new Error(`Display object ${JSON.stringify(path)} is no longer active.`);return proxyForRef(ref);}
      return createProxy(path,null);
    }
    function proxyForRef(objectRef) {
      if(!graphMode)throw new Error('Stable display references require the current Rust runtime. Re-export this project.');
      return createProxy(null,objectRef);
    }
    function createProxy(path,objectRef) {
      const key = JSON.stringify(objectRef||path);
      const lifetime = objectRef?objectRef.generation:player.lifetime_id(key);
      const cached = proxies.get(key);
      if (cached && cached.lifetime === lifetime) return cached.proxy;
      let resolvedProxy = null;
      const live=()=>retiringTargets.has(resolvedProxy)||(!familyDisposed&&(typeof runtimeDisposed==='undefined'||!runtimeDisposed)&&(objectRef?player.object_alive(key):player.lifetime_id(key)===lifetime));
      if (!lifetime||!live()) {
        if (cached) return cached.proxy;
        throw new Error(`Display object ${key} is no longer active.`);
      }
      const call=(name,...args)=>player[objectRef?'object_'+({clip_frame:'frame',clip_frame_count:'frame_count',clip_metadata:'metadata'}[name]||name):name](key,...args);
      const custom = Object.create(null);
      function assertAlive() {
        if (!live()) throw new ReferenceError(`Display object ${key} was removed or replaced.`);
      }
      function frameValue(total=false){
        assertAlive();
        if(objectRef&&retiringTargets.has(resolvedProxy)&&!player.object_alive(key)){
          const node=display.descriptor(resolvedProxy);
          return total?node.total_frames:node.frame+1;
        }
        return total?call('clip_frame_count'):call('clip_frame')+1;
      }
      function proxyOwner(){return display?(display.ownerOf?display.ownerOf(proxy):display.descriptor(proxy).owner):null;}
      function ownsTimelineMetadata(name){
        if(!Object.hasOwn(timelineMetadataProperties,name))return false;
        assertAlive();
        return !objectRef||proxyOwner()==='movie_clip';
      }
      function currentSceneAndFrame(meta=null){
        if(sceneCount()>1){
          const m=meta||JSON.parse(call('clip_metadata'));
          return {scene:m.scene_name||'',frame:m.scene_frame};
        }
        return {scene:'',frame:call('clip_frame')+1};
      }
      function ensureClipLabels(meta){
        const cacheKey=`${key}\0${lifetime}`;
        let rec=clipFrameLabels.get(cacheKey);
        if(!rec){
          const cur=currentSceneAndFrame(meta);
          rec={live,currentSceneAndFrame:()=>currentSceneAndFrame(),lastScene:cur.scene,lastFrame:cur.frame,labelsByScene:new Map()};
          clipFrameLabels.set(cacheKey,rec);
        }
        return rec;
      }
      function cachedSceneLabels(rec,sceneKey,rawLabels){
        if(!rec.labelsByScene.has(sceneKey)){
          rec.labelsByScene.set(sceneKey,rawLabels.map(label=>new frameLabelType(label.name,label.frame)));
        }
        return rec.labelsByScene.get(sceneKey);
      }
      function timelineMetadata(name){
        assertAlive();
        let value;try{value=JSON.parse(call('clip_metadata'));}catch(error){throw new TypeError(describeThrown(error), {cause: error});}
        if(name==='currentScene'||name==='scenes'){
          if(!sceneValueFactory||!frameLabelType)throw new Error('Scene values are unavailable; rebuild and re-export this project.');
          const rec=ensureClipLabels(value);
          if(name==='scenes'){
            return value.scenes.map(sceneItem=>{const sk=sceneCount()>1?(sceneItem.name||''):'';return sceneValueFactory(sceneItem,cachedSceneLabels(rec,sk,sceneItem.labels));});
          }
          const sk=sceneCount()>1?(value.scene_name||''):'';
          return sceneValueFactory({name:value.scene_name,num_frames:value.total_frames,labels:value.labels},cachedSceneLabels(rec,sk,value.labels));
        }
        const field=value[timelineMetadataProperties[name]];
        if(name!=='currentLabels')return field;
        if(!frameLabelType)throw new Error('FrameLabel type is unavailable; rebuild and re-export this project.');
        const rec=ensureClipLabels(value);
        const sk=sceneCount()>1?(value.scene_name||''):'';
        return [...cachedSceneLabels(rec,sk,field)];
      }
      const target = Object.create(null);
      function dynamicGet(name){
        if(Object.hasOwn(target,name))return Reflect.get(target,name,proxy);
        if(Object.hasOwn(custom,name))return custom[name];
        if(Reflect.has(target,name))return Reflect.get(target,name,proxy);
        if(objectRef){const child=display.childByName?display.childByName(proxy,name):display.children(proxy).find(node=>node.name===name);return child&&!constructingNodeKeys.has(JSON.stringify(child.object_ref))?display.nodeProxy(child):undefined;}
        const child=JSON.parse(player.child_path(key,name));return child?proxyFor(child):undefined;
      }
      function dynamicHas(name){
        if(Reflect.has(target,name)||Object.hasOwn(custom,name))return true;
        if(typeof name!=='string')return false;
        if(objectRef)return display.hasChild?display.hasChild(proxy,name):display.children(proxy).some(node=>node.name===name);
        return JSON.parse(player.child_path(key,name))!==null;
      }
      function dynamicSet(name,value){
        if(Object.hasOwn(target,name)||(!Object.hasOwn(custom,name)&&Reflect.has(target,name)))return Reflect.set(target,name,value,proxy);
        program?.assertDynamicWrite?.(proxy,name);custom[name]=value;return true;
      }
      function dynamicDelete(name){
        if(program?.hasFixedProperty?.(proxy,name))return false;
        if(Object.hasOwn(custom,name))return delete custom[name];
        return Reflect.deleteProperty(target,name);
      }
      const as3OnlyDisplayTraits=new Set(['upState','overState','downState','hitTestState','soundTransform']);
      function ownsAs2OrdinaryTrait(name){
        if(ownsTimelineMetadata(name))return true;
        if(!as3OnlyDisplayTraits.has(name))return false;
        assertAlive();
        return !objectRef||['movie_clip','sprite','button'].includes(proxyOwner());
      }
      // These names are native AS3 traits but remain ordinary AS2 properties.
      // The per-access AS2 reference preserves mixed-dialect instance identity.
      function as2TimelineRef(name){
        if(!ownsAs2OrdinaryTrait(name))return null;
        return {
          get(){assertAlive();return dynamicGet(name);},
          has(){assertAlive();return dynamicHas(name);},
          set(value){assertAlive();if(!dynamicSet(name,value))throw new TypeError(`Cannot write AS2 property ${name}.`);return value;},
          delete(){assertAlive();if(!Object.hasOwn(custom,name)&&!Object.hasOwn(target,name))return false;return dynamicDelete(name);},
        };
      }
      const methods = {
        play: () => control('play'),
        stop: () => control('stop'),
        gotoAndPlay: (value, scene) => goto('goto_and_play', value, false, scene),
        gotoAndStop: (value, scene) => goto('goto_and_stop', value, false, scene),
        nextFrame: () => seek('goto_and_stop', call('clip_frame') + 1, false),
        prevFrame: () => seek('goto_and_stop', call('clip_frame') - 1, false),
        nextScene: () => sceneStep(1, 'goto_and_play', false),
        prevScene: () => sceneStep(-1, 'goto_and_play', false),
        trace: (...values) => trace(...values),
      };
      function control(command) {assertAlive();if(!call('control',command,0))throw new Error(`Cannot control timeline at ${key}.`);}
      // Only a multi-scene main timeline reports more than one scene.
      function sceneCount(){return typeof player[objectRef?'object_scene_count':'scene_count']==='function'?call('scene_count'):1;}
      function goto(command, value, legacy = false, scene = null) {
        assertAlive();
        const total = call('clip_frame_count');
        if (!total) throw new Error('Timeline controls apply to symbols and the root scene.');
        let frame;
        if (scene != null || sceneCount() > 1) {
          // Rust resolves scene names, scene-relative numbers and labels.
          if (typeof value !== 'number' && typeof value !== 'string') {
            throw new Error(legacy ? 'AS2 goto requires a frame label or integer frame; other argument coercions are not supported.' : 'Frame must be a frame number or a frame label.');
          }
          const request = JSON.stringify({frame: typeof value === 'number' && !Number.isFinite(value) ? String(value) : value, scene: scene == null ? null : String(scene), as2: legacy});
          try { frame = call('resolve_goto', request); } catch (error) { throw new Error(describeThrown(error), {cause: error}); }
        } else {
          frame = legacy
            ? as2Frame(value, total, label => call('resolve_frame_as2',label))
            : oneBasedFrame(value, total, label => call('resolve_frame',label));
        }
        if (frame === null || frame < 0) return; // An unknown AS2 label changes neither frame nor play state.
        if (!legacy && hasLifecycleApi) {
          enableLifecycleIfReady();
          const depth = objectRef
            ? player.object_begin_inner_goto(key, command, frame)
            : player.begin_inner_goto(key, command, frame);
          if (!depth) throw new Error(`Cannot control timeline at ${key}.`);
          driveLifecycle(depth);
          return;
        }
        if (!call('control',command,frame)) throw new Error(`Cannot control timeline at ${key}.`);
        if (!hasLifecycleApi) dispatchFrameLabelEvents();
      }
      // Zero-based seek used by nextFrame/prevFrame, which cross scene boundaries.
      function seek(command, frame, legacy = false) {
        assertAlive();
        const total = call('clip_frame_count');
        if (!total) throw new Error('Timeline controls apply to symbols and the root scene.');
        const targetFrame = Math.max(0, Math.min(frame, total - 1));
        if (!legacy && hasLifecycleApi) {
          enableLifecycleIfReady();
          const depth = objectRef
            ? player.object_begin_inner_goto(key, command, targetFrame)
            : player.begin_inner_goto(key, command, targetFrame);
          if (!depth) throw new Error(`Cannot control timeline at ${key}.`);
          driveLifecycle(depth);
          return;
        }
        if (!call('control',command,targetFrame)) throw new Error(`Cannot control timeline at ${key}.`);
        if (!hasLifecycleApi) dispatchFrameLabelEvents();
      }
      function sceneStep(step, command, legacy = false) {
        assertAlive();
        if (sceneCount() <= 1) return; // No adjacent scene: the playhead stays.
        const frame = call('scene_step', step);
        if (frame >= 0) seek(command, frame, legacy);
      }
      function nativeHas(name){
        if(typeof name!=='string')return name===Symbol.toStringTag;
        if(ownsTimelineMetadata(name))return true;
        if(['toString','toJSON','root','_root','parent','_parent','currentFrame','_currentframe','totalFrames','_totalframes','visible','_visible'].includes(name)||Object.hasOwn(methods,name)||Object.hasOwn(propertyMap,name))return true;
        if(airHost?.hasDisplayProperty?.(proxy,name))return true;
        return Boolean(display&&display.get(proxy,name)!==display.UNHANDLED);
      }
      const proxy = new Proxy(target, {
        has(_target,name){
          if(typeof name==='string'&&(Object.hasOwn(custom,name)||Object.hasOwn(target,name))&&live())return true;
          return nativeHas(name)||dynamicHas(name);
        },
        deleteProperty(_target,name){
          if(name==='menu'&&as2StageHost&&display&&['movie_clip','button','text'].includes(proxyOwner()))return as2StageHost.menuRef(proxy,'menu').delete();
          return !nativeHas(name)&&dynamicDelete(name);
        },
        get(_target, name) {
          if (name === Symbol.toStringTag) return 'NeoFlashDisplayObject';
          if (name === 'toString') return () => `[NeoFlash ${key}]`;
          if (name === 'toJSON') return () => objectRef?({object_ref:objectRef}):({path});
          if (typeof name !== 'string') return undefined;
          if (name === 'menu' && as2StageHost && display && ['movie_clip','button','text'].includes(proxyOwner())) return as2StageHost.menuRef(proxy,'menu').get();
          if (name === '_root' || name === 'root') {assertAlive();return name==='root'&&objectRef&&!display.descriptor(proxy).on_stage?null:name==='_root'?as2RootFor(proxy):proxyFor([]);}
          if (name === '_parent' || name === 'parent') {assertAlive();const parent=display?display.parentSpace(proxy):path.length?proxyFor(path.slice(0,-1)):stageHost?.stage||null;return name==='_parent'&&parent===stageHost?.stage?null:parent;}
          if (name === '_currentframe' || name === 'currentFrame') {
            assertAlive();
            const retiring=objectRef&&retiringTargets.has(resolvedProxy)&&!player.object_alive(key);
            if(name==='currentFrame'&&!retiring&&sceneCount()>1)return JSON.parse(call('clip_metadata')).scene_frame;
            return frameValue();
          }
          if (name === '_totalframes' || name === 'totalFrames') return frameValue(true);
          if (Object.hasOwn(propertyMap, name)) {
            assertAlive();
            const value = call('property',propertyMap[name]);
            return percentage.has(name) ? value * 100 : propertyMap[name]==='rotation' ? ((value%360)>180?value%360-360:(value%360)<-180?value%360+360:value%360) : value;
          }
          if (name === 'visible' || name === '_visible') {
            assertAlive();
            return call('visible');
          }
          if (Object.hasOwn(methods, name)) return methods[name];
          if (ownsTimelineMetadata(name)) return timelineMetadata(name);
          if(airHost?.hasDisplayProperty?.(proxy,name)){assertAlive();return airHost.getDisplayProperty(proxy,name);}
          if(display){const value=display.get(proxy,name);if(value!==display.UNHANDLED)return value;}
          return dynamicGet(name);
        },
        set(_target, name, value) {
          if (typeof name !== 'string') return false;
          if (Object.hasOwn(propertyMap, name)) {
            assertAlive();
            if (name.startsWith('_')) {
              value = Number(value);
              if (!Number.isFinite(value)) return true;
            }
            if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error(`${name} must be a finite number.`);
            if (!call('set_property',propertyMap[name],percentage.has(name)?value/100:value)) {
              throw new Error(`Cannot change ${name} on inactive display object ${key}.`);
            }
            return true;
          }
          if (name === 'visible' || name === '_visible') {
            assertAlive();
            const visible = name === '_visible' ? as2Visibility(value) : Boolean(value);
            if (visible === null) return true;
            if (!call('set_visible',visible)) {
              throw new Error(`Cannot change ${name} on inactive display object ${key}.`);
            }
            return true;
          }
          if(name==='menu'&&as2StageHost&&display&&['movie_clip','button','text'].includes(proxyOwner())){as2StageHost.menuRef(proxy,'menu').set(value);return true;}
          if(airHost?.hasDisplayProperty?.(proxy,name)){assertAlive();if(airHost.setDisplayProperty(proxy,name,value))return true;}
          if(display&&display.set(proxy,name,value)){if(name==='text')as2ObjectHost?.textChanged(proxy);return true;}
          if (ownsTimelineMetadata(name)||['currentFrame', 'totalFrames', '_currentframe', '_totalframes', '_root', '_parent', 'root', 'parent'].includes(name)) {
            throw new Error(`${name} is read-only.`);
          } else return dynamicSet(name,value);
        },
      });
      resolvedProxy = proxy;
      const ownerNativeKeys=()=>{
        const owner=objectRef&&display&&live()?proxyOwner():null;
        const common=['_width','_height','_name','_target','_framesloaded','_url','_droptarget','_highquality','_focusrect','_soundbuftime','_quality','_xmouse','_ymouse'];
        if(owner==='movie_clip')return [...common,'enabled','useHandCursor','tabEnabled','tabIndex','tabChildren','trackAsMenu','focusEnabled','hitArea','menu','_lockroot','loadVariables','onLoad','onData','onUnload','onKeyDown','onKeyUp','onMouseDown','onMouseUp','onMouseMove','onSetFocus','onKillFocus'];
        if(owner==='button')return [...common,'enabled','useHandCursor','tabEnabled','tabIndex','trackAsMenu','menu','onKeyDown','onKeyUp','onSetFocus','onKillFocus'];
        if(owner==='text')return [...common,'text','html','htmlText','variable','selectable','type','multiline','wordWrap','border','background','textColor','borderColor','backgroundColor','scroll','maxscroll','hscroll','maxhscroll','bottomScroll','length','textWidth','textHeight','tabEnabled','tabIndex','menu','onChanged','onScroller','onSetFocus','onKillFocus'];
        return common;
      };
      references.set(proxy,{path:path?[...path]:null,objectRef,key,lifetime,nativeHas,as2TimelineRef,nativeKeys:()=>[...Object.keys(methods),...Object.keys(propertyMap),'onEnterFrame','onPress','onRelease','onReleaseOutside','onRollOver','onRollOut','onDragOver','onDragOut','_root','_parent','_visible','_currentframe','_totalframes',...ownerNativeKeys()],customKeys:()=>Object.keys(custom)});
      if(display)Object.assign(methods,display.methods(proxy));
      if(eventHub)Object.assign(methods,eventHub.methods(proxy));
      if(methods.dispatchEvent&&fte3dHost?.forwardTextLineEvent){
        const baseDispatch=methods.dispatchEvent;
        methods.dispatchEvent=event=>{
          const ok=baseDispatch(event);
          if(objectRef&&display&&live()&&proxyOwner()==='text_line')fte3dHost.forwardTextLineEvent(proxy,event);
          return ok;
        };
      }
      // AS2 gotoAndPlay("Scene 2", frame) names the scene first and jumps on the root timeline.
      const as2Goto = command => (...args) => {
        if (args.length >= 2) {
          assertAlive();
          const root = proxyFor([]);
          if (proxy !== root) return as2.method(root, command === 'goto_and_play' ? 'gotoAndPlay' : 'gotoAndStop')(...args);
          return goto(command, args[1], true, args[0]);
        }
        return goto(command, args[0], true);
      };
      as2.register(methods.gotoAndPlay, proxy, 'gotoAndPlay', as2Goto('goto_and_play'));
      as2.register(methods.gotoAndStop, proxy, 'gotoAndStop', as2Goto('goto_and_stop'));
      as2.register(methods.nextFrame, proxy, 'nextFrame', () => seek('goto_and_stop', call('clip_frame') + 1, true));
      as2.register(methods.prevFrame, proxy, 'prevFrame', () => seek('goto_and_stop', call('clip_frame') - 1, true));
      // AS2 nextScene/prevScene compile to a frame jump on the root timeline that stops the playhead.
      as2.register(methods.nextScene, proxy, 'nextScene', () => { assertAlive(); const root = proxyFor([]); if (proxy !== root) return as2.method(root, 'nextScene')(); return sceneStep(1, 'goto_and_stop', true); });
      as2.register(methods.prevScene, proxy, 'prevScene', () => { assertAlive(); const root = proxyFor([]); if (proxy !== root) return as2.method(root, 'prevScene')(); return sceneStep(-1, 'goto_and_stop', true); });
      proxies.set(key, { lifetime, proxy });
      return proxy;
    }
    function trace(...values) {
      if (typeof bundle !== 'undefined' && bundle?.player_config?.include_trace === false) return;
      const frames = sourceFrames(new Error());
      const location = sourceLocationText(frames[0]);
      if (global && typeof global === 'object') global.__neoflashSuppressConsoleRelay = true;
      try {
        console.log('[NeoFlash]', ...values, ...(location ? [location] : []));
      } finally {
        if (global && typeof global === 'object') global.__neoflashSuppressConsoleRelay = false;
      }
      const message = formatTraceArgs(values, { program });
      lastTrace = message.replace(/\r?\n\s*/g, ' ').slice(0, 160);
      lastTraceLocation = location;
      if (typeof instanceOnTrace === 'function') {
        try { instanceOnTrace(message, values, frames[0] || null); } catch (_) {}
      }
      if (typeof instanceEventHost !== 'undefined' && instanceEventHost && instanceEventHost !== global) {
        emitScriptMessage('trace', message, frames, instanceEventHost);
      }
      emitScriptMessage('trace', message, frames);
    }
    let lifecycleStepCount = 0;
    function executeDispatchedScript(item, clickTargetScope) {
      if (!item.script.source.trim() || !player.is_dispatch_current(JSON.stringify(item))) return;
      const compiled = functions.get(sourceKey(item.script));
      if (!compiled) throw new Error('A script is missing from the compiled export. Re-export the project.');
      const scope = item.target_ref ? proxyForRef(item.target_ref) : proxyFor(item.target_path);
      const event = Object.freeze({
        type: typeof item.event === 'string' ? item.event : 'frame',
        target: item.event === 'click' && clickTargetScope ? clickTargetScope : scope,
        currentTarget: scope,
      });
      try {
        const isAs2 = ['action_script1', 'action_script2', 'ActionScript1', 'ActionScript2'].includes(item.script.language);
        const scriptRoot = isAs2 ? as2RootFor(scope) : proxyFor([]);
        const gotoPlay = isAs2 ? as2.method(scope, 'gotoAndPlay') : scope.gotoAndPlay;
        const gotoStop = isAs2 ? as2.method(scope, 'gotoAndStop') : scope.gotoAndStop;
        const nextF = isAs2 ? as2.method(scope, 'nextFrame') : scope.nextFrame;
        const prevF = isAs2 ? as2.method(scope, 'prevFrame') : scope.prevFrame;
        compiled.fn.call(scope, event, scriptRoot, scope._parent, trace,
          scope.play, scope.stop, gotoPlay, gotoStop, nextF, prevF, as2.method, as2.value, program, display,
          legacyGlobals, xmlHost);
        display?.flushClipLoads?.();
      } catch (error) {
        const cause = unwrapScriptThrow(error);
        throw new Error(`${compiled.location}: ${describeThrown(cause)}`, { cause });
      }
    }
    function driveLifecycle(minDepth = 1, clickTargetScope = null) {
      if (!hasLifecycleApi) return;
      const wasInDispatch = inDispatch;
      const resetCounter = minDepth <= 1 && !wasInDispatch;
      if (resetCounter) lifecycleStepCount = 0;
      inDispatch = true;
      try {
        for (;;) {
          const depth = typeof player.lifecycle_depth === 'function' ? player.lifecycle_depth() : 0;
          if (depth < minDepth) break;
          if (++lifecycleStepCount > 4096 || player.script_overflowed()) {
            throw new Error('Script event limit exceeded. Check recursive gotoAndPlay/gotoAndStop calls.');
          }
          const step = JSON.parse(player.lifecycle_next_step_json());
          if (!step) break;
          syncResetGeneration(step.generation);
          const scopeKey = `${step.generation}:${step.depth}`;
          if (step.phase === 'construct') {
            frameLabelDispatchedScopes.delete(scopeKey);
            runConstructEvents(step.display_events || [], true);
          } else if (step.phase === 'frame_constructed') {
            if (step.display_events?.length) runConstructEvents(step.display_events, true);
            try {
              broadcastDisplayPhase(global.NeoFlashEvents?.Event?.FRAME_CONSTRUCTED || 'frameConstructed');
            } catch (error) {
              const cause = unwrapScriptThrow(error);
              throw new Error(`FrameConstructed callback: ${describeThrown(cause)}`, { cause });
            }
          } else if (step.phase === 'frame_scripts') {
            if (step.display_events?.length) runConstructEvents(step.display_events, true);
            if (!frameLabelDispatchedScopes.has(scopeKey)) {
              frameLabelDispatchedScopes.add(scopeKey);
              try {
                dispatchFrameLabelEvents();
              } catch (error) {
                const cause = unwrapScriptThrow(error);
                throw new Error(`FrameLabel callback: ${describeThrown(cause)}`, { cause });
              }
            }
            for (const item of (step.scripts || [])) {
              executeDispatchedScript(item, clickTargetScope);
            }
          } else if (step.phase === 'exit_frame') {
            frameLabelDispatchedScopes.delete(scopeKey);
            if (step.display_events?.length) runConstructEvents(step.display_events, true);
            try {
              broadcastDisplayPhase(global.NeoFlashEvents?.Event?.EXIT_FRAME || 'exitFrame');
            } catch (error) {
              const cause = unwrapScriptThrow(error);
              throw new Error(`ExitFrame callback: ${describeThrown(cause)}`, { cause });
            }
          } else if (step.phase === 'render') {
            if (step.display_events?.length) runConstructEvents(step.display_events, true);
            flushStageRender();
          }
        }
        if (player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive timeline controls.');
      } finally {
        inDispatch = wasInDispatch;
      }
    }
    function dispatchScripts(clickTarget = null) {
      const wasInDispatch = inDispatch;
      inDispatch = true;
      try {
      syncResetGeneration();
      program?.runInitClips?.(proxyFor([]));
      const clickTargetScope = clickTarget ? references.has(clickTarget)?clickTarget:proxyFor(clickTarget) : null;
      if (hasLifecycleApi) {
        enableLifecycleIfReady();
        driveLifecycle(1, clickTargetScope);
      } else {
        attachNewInstances();
      }
      let count = 0;
      for (;;) {
        const events = JSON.parse(player.scripts_json());
        if (!events.length) break;
        for (const item of events) {
          if (++count > 4096 || player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive gotoAndPlay/gotoAndStop calls.');
          executeDispatchedScript(item, clickTargetScope);
        }
        if (hasLifecycleApi && graphMode && typeof player.display_events_json === 'function') {
          const extra = JSON.parse(player.display_events_json()).events;
          if (extra?.length) runConstructEvents(extra, true);
          else attachNewInstances();
        } else {
          attachNewInstances();
        }
      }
      if (player.script_overflowed()) throw new Error('Script event limit exceeded. Check recursive timeline controls.');
      as2ObjectHost?.syncTextVariables();
      drainSounds();
      drainVideos();
      } finally {
        inDispatch = wasInDispatch;
      }
    }
    // Timeline sounds: the Rust Player queues SoundCommands as frames are entered; the audio
    // service schedules them. Resources are decoded once per session before their keyframe.
    let soundsPreloaded=false;
    function drainSounds(){
      if(typeof player.sound_commands_json!=='function')return;
      const commands=JSON.parse(player.sound_commands_json());
      if(!audioHost)return;
      if(!soundsPreloaded){soundsPreloaded=true;const ids=JSON.parse(player.timeline_sound_resources_json());if(ids.length)audioHost.preloadTimeline(ids);}
      if(commands.length)audioHost.timeline(commands);
    }
    // Authored timeline video uses the same Rust-owned command boundary as frame sound. The
    // browser owns only native decoding, frame presentation and user-activation state.
    function drainVideos(){
      if(typeof player.video_commands_json!=='function')return;
      const commands=JSON.parse(player.video_commands_json());
      if(commands.length&&videoHost)videoHost.timeline(commands);
    }
    // AS2 stopAllSounds() and AS3 SoundMixer.stopAll() stop scripted and timeline sounds.
    function stopTimelineSounds(){if(typeof player.stop_all_sounds==='function'){player.stop_all_sounds();drainSounds();}}
    function stopAllSounds(){audioHost?.stopAllLegacySounds();stopTimelineSounds();}
    const SoundMixer=Object.freeze({stopAll(){audioHost?.stopAllChannels();stopTimelineSounds();},areSoundsInaccessible(){return false;}});
    function semanticNodeForTarget(target) {
      const ref=target && references.get(target);
      return ref ? {owner_ref:ref.objectRef,instance_path:ref.path,lifetime_id:ref.lifetime} : null;
    }
    function syncSemanticFocus(next,previous,inputHandled=false) {
      if(typeof semanticLayer==='undefined' || !semanticLayer || domStageHost?.isActive?.())return false;
      if(previous && previous!==next)semanticLayer.blurTarget(semanticNodeForTarget(previous));
      return Boolean(next && !inputHandled && semanticLayer.focusTarget(semanticNodeForTarget(next)));
    }
    function semanticNativeFocus(node,event={}) {
      if(familyDisposed || failed || !stageHost)return;
      try {
        const target=node.owner_ref?proxyForRef(node.owner_ref):proxyFor(node.instance_path);
        if(!display?.alive(target))return;
        if(stageHost.stage.focus!==target && !stageHost.changeFocus(target,'key',event)){
          syncSemanticFocus(stageHost.stage.focus,null);
          return;
        }
        requestTimerRender();
      } catch(error) {inputFailure(error);}
    }
    function semanticNativeBlur(node) {
      if(familyDisposed || failed || !stageHost)return;
      try {
        const focused=stageHost.stage.focus,ref=focused && references.get(focused);
        if(ref && (node.owner_ref?JSON.stringify(node.owner_ref)===ref.key:JSON.stringify(node.instance_path)===ref.key && node.lifetime_id===ref.lifetime)){
          stageHost.changeFocus(null);
          requestTimerRender();
        }
      } catch(error) {inputFailure(error);}
    }
    function prepareStageAccessibility(semNodes = [], snapshot = null) {
      const currentFrame = typeof player.current_frame === 'function' ? player.current_frame() : 0;
      const objects = snapshot?.objects || (graphMode && display?.nodes ? display.nodes() : []);
      const byRef = new Map(objects.map(object => [JSON.stringify(object.object_ref), object]));
      const byTarget = new Map();
      for (const object of objects) if (object.object_ref) byTarget.set(proxyForRef(object.object_ref), object);
      const raw = (Array.isArray(semNodes) ? semNodes : []).filter(node => node && typeof node === 'object');
      const targetFor = node => node.owner_ref ? proxyForRef(node.owner_ref) : proxyFor(node.instance_path || []);
      const rawTargets = new Map(raw.map(node => [node, targetFor(node)]));
      const entries = global.NeoFlashStage?.collectProjectAccessibilityEntries
        ? global.NeoFlashStage.collectProjectAccessibilityEntries(project, currentFrame, {
            resolveTarget: el => {
              if (!el) return null;
              if (graphMode) {
                // Root authored IDs cannot accidentally select a same-named linked/nested instance.
                const matches = objects.filter(object => Array.isArray(object.authored_path) && object.authored_path.length === 1 && object.authored_path[0] === el.id);
                return matches.length === 1 ? proxyForRef(matches[0].object_ref) : null;
              }
              const matches = raw.filter(node => node.instance_path?.length === 1 && (node.instance_path[0] === el.id || (el.name && node.instance_path[0] === el.name)));
              return matches.length === 1 ? rawTargets.get(matches[0]) : null;
            },
          })
        : [];
      const entriesByTarget = new Map(entries.map(entry => [entry.target, entry]));
      const covered = new Set();
      const merge = (node, entry, object = null) => {
        const base = node.accessibility || {};
        const accessibility = entry ? {
          ...base,
          role: entry.roleOverride || !base.role ? entry.role : base.role,
          name: entry.autoTextLabel ? entry.ariaLabel : (base.name || entry.ariaLabel),
          description: base.description || entry.ariaDescription,
          live: entry.ariaLive,
          shortcut: entry.shortcut || base.shortcut || '',
          reading_order: entry.readingOrder,
          tab_order: entry.tabIndex ?? base.tab_order ?? 0,
          tabEnabled: entry.tabEnabled ?? base.tabEnabled,
          disabled: Boolean(base.disabled || entry.disabled),
          silent: Boolean(base.silent || entry.silent),
          forceSimple: Boolean(base.forceSimple || base.force_simple || entry.forceSimple),
          noAutoLabeling: Boolean(base.noAutoLabeling || base.no_auto_labeling || entry.noAutoLabeling),
          activateButton: Boolean(base.activateButton || base.role === 'button'),
        } : base;
        const ancestry_refs = [];
        for (let parent = object; parent; parent = byRef.get(JSON.stringify(parent.parent_ref))) ancestry_refs.push(parent.object_ref);
        return {...node, accessibility, ...(object ? {input:object.input, ancestry_refs} : {})};
      };
      const nodes = raw.map(node => {
        const target = rawTargets.get(node), entry = entriesByTarget.get(target);
        if (entry) covered.add(entry);
        return merge(node, entry, byTarget.get(target));
      });
      const fallback = [];
      for (const entry of entries) {
        if (covered.has(entry)) continue;
        const object = byTarget.get(entry.target);
        if (object) {
          // Static text/artwork with extended authoring metadata may have no legacy semantic node.
          // The runtime reference and draw coverage still own its identity and visibility.
          if (object.on_stage === false || object.effective_visible === false) continue;
          nodes.push(merge({owner_ref:object.object_ref,instance_path:object.source_path || object.authored_path || object.path || [],lifetime_id:object.object_ref.generation,accessibility:{}},entry,object));
        } else if (!graphMode) fallback.push(entry);
      }
      return {nodes, fallback};
    }
    function syncStageAccessibilityOverlay(entries = [], semanticScene = []) {
      if (typeof a11yOverlay === 'undefined' || !a11yOverlay || !global.NeoFlashStage?.syncAccessibilityOverlay) return 0;
      const [width,height]=logicalSize(),[x,y]=stageOrigin();
      const visible = global.NeoFlashAccessibility.sceneBoundsByPath(semanticScene,width,height,(line,kind)=>{context.font=font(kind);return context.measureText(line).width;},{x,y});
      // Legacy static-only fallback requires actual visible draw coverage too. A missing graph
      // object or a clipped semantic target must never reappear as an inert overlay tab stop.
      entries = entries.filter(entry => !entry.silent && (visible.has(JSON.stringify([entry.id])) || (entry.name && visible.has(JSON.stringify([entry.name])))));
      if (typeof stageHost !== 'undefined' && stageHost?.syncAccessibilityOverlay) {
        stageHost.syncAccessibilityOverlay(a11yOverlay, entries);
      } else {
        global.NeoFlashStage.syncAccessibilityOverlay(a11yOverlay, entries);
      }
      return entries.length;
    }
    function resize() { preview.resize(); }
    function font(kind) { return `${kind.font_size}px ${global.NeoFlashText?.cssFontFamily ? global.NeoFlashText.cssFontFamily(kind.font_family) : `${JSON.stringify(kind.font_family)}, sans-serif`}`; }
    function bitmapMatrix(item){
      if(item.kind.type!=='bitmap_pixels')return item.transform;
      const [width,height]=logicalSize(),[ox,oy]=stageOrigin(),sx=canvas.width/width,sy=canvas.height/height,[a,b,c,d,e,f]=item.transform;
      const m=bitmapHost.cache.transform([a*sx,b*sy,c*sx,d*sy,(e+ox)*sx,(f+oy)*sy],item.kind.pixel_snapping);
      return [m[0]/sx,m[1]/sy,m[2]/sx,m[3]/sy,m[4]/sx-ox,m[5]/sy-oy];
    }
    let measuredFps = 0;
    let lastTickNow = null;
    const fpsIntervals = [];
    function resetMeasuredFps() {
      measuredFps = 0;
      lastTickNow = null;
      fpsIntervals.length = 0;
    }
    function render() {
      flushStageRender();
      let snapshot=graphMode?(display?.snapshot?display.snapshot():JSON.parse(player.runtime_snapshot_json())):null;
      if(snapshot&&typeof videoHost!=='undefined'&&videoHost)snapshot=videoHost.prepareSnapshot(snapshot);
      scene = snapshot?snapshot.draws:JSON.parse(player.scene_json());
      assertUnfilteredEditing(stageHost?.stage.focus);
      const [width,height]=logicalSize();
      const [originX,originY]=stageOrigin();
      const rawSemNodes=snapshot?(snapshot.semantics.length?snapshot.semantics.filter(node=>Boolean(semanticRoute(node,snapshot.objects))):[]):JSON.parse(player.accessibility_json());
      const runtimeSemNodes=as3MediaEventsHost?.applyAccessibilityPropertiesToSemanticNodes?as3MediaEventsHost.applyAccessibilityPropertiesToSemanticNodes(rawSemNodes,node=>node.owner_ref?proxyForRef(node.owner_ref):proxyFor(node.instance_path||[])):rawSemNodes;
      const preparedAccessibility=prepareStageAccessibility(runtimeSemNodes,snapshot);
      const enrichedSemNodes=preparedAccessibility.nodes;
      if(domStageHost?.isActive?.()){
        semanticLayer.clear?.();
        if(typeof a11yOverlay!=='undefined'&&a11yOverlay)a11yOverlay.replaceChildren?.();
        canvas.setAttribute('aria-hidden','true');
        const stageRgb=stageHost?.stage?.color;
        bitmapHost?.cache.beginFrame();
        try{
          domStageHost.render({snapshot:snapshot||{draws:scene,objects:[],semantics:[],clips:[],filter_groups:[]},semantics:enrichedSemNodes,width,height,originX,originY,scale:{x:preview.view.scale,y:preview.view.scaleY||preview.view.scale},stageColor:Number.isInteger(stageRgb)&&stageHost?.colorAssigned?.()?stageRgb:null,background:project.background});
        }finally{bitmapHost?.cache.endFrame();}
      }else{
      if(snapshot)textInputHost?.update(snapshot);
      const sx=canvas.width/width,sy=canvas.height/height;
      if(typeof player.as2_settings_command==='function'&&display){
        const quality=display.as2GlobalPropRef('_quality').get();
        context.imageSmoothingEnabled=quality!=='LOW';
        if('imageSmoothingQuality' in context)context.imageSmoothingQuality=quality==='BEST'?'high':quality==='MEDIUM'?'low':'medium';
      }
      context.setTransform(sx, 0, 0, sy, 0, 0);
      context.clearRect(0, 0, width, height);
      const stageRgb=stageHost?.stage?.color;
      context.fillStyle = Number.isInteger(stageRgb)&&stageHost?.colorAssigned?.() ? color([(stageRgb>>>16)&255,(stageRgb>>>8)&255,stageRgb&255,255]) : color(project.background);
      context.fillRect(0, 0, width, height);
      if (originX !== 0 || originY !== 0) {
        context.setTransform(sx, 0, 0, sy, originX * sx, originY * sy);
      }
      as3MediaEventsHost?.paintStageVideos?.(context, width, height);
      fte3dHost?.paintStage3Ds?.(context);
      bitmapHost?.cache.beginFrame();
      const painter=createScenePainter({global,paths,gradients,clippings,bitmapHost,textHost,filterHost,videoHost,context,matrix:bitmapMatrix,hidden:ref=>Boolean(textInputHost?.hidden(ref))});
      const {clippedBounds,objects}=painter.draw(context,snapshot||{draws:scene,objects:[],semantics:[],clips:[],filter_groups:[]},{minX:-originX,minY:-originY,width:width-originX,height:height-originY});
      const focusBounds=display?.as2FocusBounds?.();if(focusBounds){context.save();context.strokeStyle='#ffcc00';context.lineWidth=2;context.strokeRect(focusBounds.x,focusBounds.y,focusBounds.width,focusBounds.height);context.restore();}
      const semanticScene=enrichedSemNodes.length || preparedAccessibility.fallback.length?(snapshot?scene.map(item=>{const ancestry_refs=[];for(let node=objects.get(JSON.stringify(item.owner_ref));node;node=objects.get(JSON.stringify(node.parent_ref)))ancestry_refs.push(node.object_ref);return {...item,...(clippedBounds.has(item)?{clipped_bounds:clippedBounds.get(item)}:{}),transform:bitmapMatrix(item),ancestry_refs,kind:item.kind.type==='bitmap_pixels'?{...item.kind,alpha_bounds:bitmapHost.cache.bounds(item.kind)}:item.kind.type==='text_field'?{...item.kind,text_bounds:textHost.bounds(item.kind.field,true)}:item.kind};}):scene):[];
      const targetCount = semanticLayer.update(enrichedSemNodes, semanticScene, {...project,width,height}, {x:preview.view.scale,y:preview.view.scaleY||preview.view.scale}, (line, kind) => { context.font = font(kind); return context.measureText(line).width; }, {x:originX,y:originY});
      syncSemanticFocus(stageHost?.stage?.focus,null,textInputHost?.owns?.(instanceDoc?.activeElement));
      const fallbackTargetCount=syncStageAccessibilityOverlay(preparedAccessibility.fallback,semanticScene);
      bitmapHost?.cache.endFrame();
      if (targetCount || fallbackTargetCount) canvas.setAttribute('aria-hidden', 'true'); else canvas.removeAttribute('aria-hidden');
      }
      if (status) {
        status.textContent = `${player.current_frame() + 1} / ${player.total_frames()} · ${runtimeFrameRate} fps`;
        if (typeof measuredFps === 'number' && measuredFps > 0) {
          if (status.dataset) {
            status.dataset.measuredFps = measuredFps.toFixed(1);
            status.dataset.targetFps = String(runtimeFrameRate);
          }
          status.title = `Measured: ${measuredFps.toFixed(1)} fps · Target: ${runtimeFrameRate} fps`;
        } else {
          if (status.dataset) {
            delete status.dataset.measuredFps;
            delete status.dataset.targetFps;
          }
          status.removeAttribute?.('title');
        }
      }
      const traceText = lastTrace + (lastTraceLocation ? ` (${lastTraceLocation})` : '');
      if (traceElement && traceElement.textContent !== traceText) traceElement.textContent = traceText;
      if (toggle) toggle.textContent = player.is_playing() ? 'Pause' : 'Play';
    }
    function hitAt(event,details=false) {
      let snapshot=graphMode?(display?.snapshot?display.snapshot():JSON.parse(player.runtime_snapshot_json())):null;
      if(snapshot&&typeof videoHost!=='undefined'&&videoHost)snapshot=videoHost.prepareSnapshot(snapshot);
      scene = snapshot?snapshot.draws:JSON.parse(player.scene_json());
      const clips=snapshot?.clips?.length?global.NeoFlashPaths.clipTable(snapshot.clips,snapshot.filter_groups||[]):new Map();
      const pt = stagePointFromEvent(event);
      if (!pt) return null;
      const { x, y, inside } = pt;
      if(typeof display!=='undefined'&&display?.pickDetails){const result=inside?display.pickDetails(x,y):null;return details?result:result?references.get(result.target).objectRef:null;}
      if (!inside) return null;
      for (let index = scene.length - 1; index >= 0; index--) {
        const item = scene[index];
        if (item.alpha <= 0) continue;
        if(item.clip_ids?.length&&!global.NeoFlashPaths.clipsContain(context,item.clip_ids,clips,x,y,paths))continue;
        const local = localPoint(bitmapMatrix(item), x, y);
        if (!local) continue;
        if (hitShape(item.kind, ...local, (line, kind) => {
          context.font = font(kind);
          return context.measureText(line).width;
        }, (kind, x, y) => global.NeoFlashPaths.contains(context, kind, x, y, paths.get(kind.commands)), (kind,x,y)=>bitmapHost?.cache.hit(kind,x,y)||false,(field,x,y)=>textHost?.hit(field,x,y)||false)) return graphMode?item.owner_ref:item.instance_path;
      }
      return null;
    }
    canvas.addEventListener('pointerdown', guardHostCallback(() => {
      for (const host of [audioHost, videoHost]) {
        if (host && host.status?.().state === 'awaiting_gesture') {
          mediaGesture(host, 'resumeFromGesture');
        }
      }
    }));
    canvas.addEventListener('click', event => {
      if (familyDisposed || failed || preview.panMode || preview.suppressClick) return;
      try {
        const hit=hitAt(event,true),target=hit?.target||(hit?(graphMode?proxyForRef(hit):proxyFor(hit)):null),authored=hit?.owner||target;
        if(stageHost?.handlePointer('click',event,target||stageHost.stage,true)===false)return;
        if(target&&display.alive(target)&&display.alive(authored)){
          if(graphMode&&typeof player.object_click==='function')player.object_click(references.get(authored).key);
          else {
            let owner=authored,node=display.descriptor(owner);
            while(graphMode&&node.authored_path===null){owner=display.parentSpace(owner);if(!owner||owner===stageHost?.stage)break;node=display.descriptor(owner);}
            const path=graphMode?node.authored_path:node.path;
            if(path)player.click(JSON.stringify(path));
          }
          dispatchScripts(authored);
          render();
        }

      } catch (error) { fail(error); }
    });
    canvas.addEventListener('pointermove',guardHostCallback(event=>{const hit=hitAt(event);pointerOverContent=Boolean(hit&&display.handCursor(graphMode?proxyForRef(hit):proxyFor(hit)));updateCursor();}));
    canvas.addEventListener('pointerleave', guardHostCallback(() => { pointerOverContent=false;updateCursor(); }));
    toggle?.addEventListener('click', guardHostCallback(() => {
      player.set_playing(!player.is_playing());
      drainSounds();drainVideos();
      lastTime = null;
      accumulated = 0;
      resetMeasuredFps();
      render();
    }));
    function restartInstance() {
      if (familyDisposed || runtimeDisposed) return;
      failed = false;
      if (isStandaloneInstance) mountOptions.setExternalFailed?.(false);
      if (instanceErrorElement?.style) instanceErrorElement.style.display = 'none';
      accumulated = 0; lastTime = null; resetMeasuredFps();
      if (instanceEventHost !== global) emitScriptMessage('restart', 'Player restarted', [], instanceEventHost);
      emitScriptMessage('restart', 'Player restarted', []);
      try { resetProject(); render(); } catch (error) { fail(error); }
    }
    restartButton?.addEventListener('click', restartInstance);
    audioToggleButton?.addEventListener('click',guardHostCallback(()=>{
      const services=[audioHost,videoHost].filter(host=>{if(!host)return false;const state=host.status();return state.state!=='idle'&&state.audible!==false;}),mute=services.length&&services.every(host=>{const state=host.status();return (state.unlocked||state.requiresGesture===false)&&!state.muted&&state.state!=='awaiting_gesture';});
      for(const host of services)mediaGesture(host,mute?'setMuted':'resumeFromGesture',...(mute?[true]:[]));
    }));
    addOwnedListener(global, 'resize', guardHostCallback(() => { resize(); render(); }));
    addOwnedListener(instanceDoc, 'visibilitychange', () => { lastTime = null; accumulated = 0; resetMeasuredFps(); });
    zoomSelect?.addEventListener('change', event => {
      hostStageLayoutPinned = false;
      if (!as2StageHost?.isActive?.()) {
        preview.setStageLayout?.(null);
        if (viewport?.style) viewport.style.overflow = initialViewportOverflow;
      }
      preview.setMode(event.target.value);
    });
    if (playerConfig.scale_mode) {
      const initialMode = playerConfig.scale_mode === 'no_scale' ? '1'
        : playerConfig.scale_mode === 'exact_fit' ? 'stretch'
        : playerConfig.scale_mode === 'fill_crop' ? 'fill'
        : 'fit';
      preview.mode = initialMode;
      if (zoomSelect && (initialMode === 'fit' || initialMode === '1')) zoomSelect.value = initialMode;
    }
    panButton?.addEventListener('click', () => { preview.setPan(!preview.panMode); panButton.setAttribute('aria-pressed', String(preview.panMode)); updateCursor(); });
    if (fullscreenToggle) {
      if (!allowFullscreen) fullscreenToggle.hidden = true;
      const syncFullscreenToggle = () => {
        const active = ownsViewportFullscreen();
        fullscreenToggle.textContent = active ? 'Exit fullscreen' : 'Fullscreen';
        fullscreenToggle.setAttribute('aria-pressed', String(active));
        if (active) enableHostStageLayout(false);
        else if (!as2StageHost?.isActive?.()) syncInitialStageLayout();
        resize();
        render();
      };
      fullscreenToggle.addEventListener('click', () => {
        if (failed || !allowFullscreen) return;
        try {
          if (ownsViewportFullscreen()) {
            const exitFn = instanceDoc?.exitFullscreen || instanceDoc?.webkitExitFullscreen;
            if (typeof exitFn === 'function') Promise.resolve(exitFn.call(instanceDoc)).catch(fail);
          } else {
            const reqFn = viewport?.requestFullscreen || viewport?.webkitRequestFullscreen;
            if (typeof reqFn !== 'function') throw new Error('Fullscreen is unavailable in this browser context.');
            enableHostStageLayout(false);
            Promise.resolve(reqFn.call(viewport)).catch(fail);
          }
        } catch (error) { fail(error); }
      });
      addOwnedListener(instanceDoc, 'fullscreenchange', syncFullscreenToggle);
      addOwnedListener(instanceDoc, 'webkitfullscreenchange', syncFullscreenToggle);
    }
    const isCleanChrome = chromeConfig.clean === true || (
      chromeConfig.show_fps === false &&
      chromeConfig.show_dimensions === false &&
      chromeConfig.show_controls === false &&
      chromeConfig.show_load_summary === false &&
      chromeConfig.show_trace === false
    );
    if (isCleanChrome) {
      if (isStandaloneInstance) instanceDoc?.body?.setAttribute?.('data-clean', 'true');
      else scopedDom?.rootWrapper?.setAttribute?.('data-clean', 'true');
      if (footerElement) footerElement.hidden = true;
    } else {
      if (chromeConfig.show_controls === false && transportElement) transportElement.hidden = true;
      if (chromeConfig.show_fps === false && status) status.hidden = true;
      if (chromeConfig.show_load_summary === false) {
        if (loadInfoElement) loadInfoElement.hidden = true;
        if (reportToggle) reportToggle.hidden = true;
      }
      if (chromeConfig.show_trace === false && traceElement) traceElement.hidden = true;
      if (nameElement && chromeConfig.show_dimensions !== false && project?.width && project?.height) {
        const baseTitle = nameElement.textContent || project.name || '';
        const dimSuffix = `${project.width}×${project.height}`;
        if (!baseTitle.includes(dimSuffix)) {
          nameElement.textContent = baseTitle ? `${baseTitle} · ${dimSuffix}` : dimSuffix;
        }
      }
    }
    const fmtBytes = b => b < 1024 ? `${b} B` : b < 1048576 ? `${(b / 1024).toFixed(1)} KB` : `${(b / 1048576).toFixed(2)} MB`;
    if (bundle.load_summary) {
      const s = bundle.load_summary;
      const weightText = s.total_package_bytes > 0
        ? `Weight: ${fmtBytes(s.bundle_payload_bytes)} (package ${fmtBytes(s.total_package_bytes)})`
        : `Weight: ${fmtBytes(s.bundle_payload_bytes)}`;
      const resParts = [];
      if (s.audio_count) resParts.push(`Audio ${s.audio_count} (${fmtBytes(s.audio_bytes)})`);
      if (s.bitmap_count) resParts.push(`Bitmap ${s.bitmap_count} (${fmtBytes(s.bitmap_bytes)})`);
      if (s.data_count) resParts.push(`Data ${s.data_count} (${fmtBytes(s.data_bytes)})`);
      const resText = s.resources_count ? `${s.resources_count} res [${resParts.join(', ')}]` : '0 res';
      const scriptsTotal = (s.source_files_count || 0) + (s.handler_scripts_count || 0);
      const summaryText = `${weightText} · Loads: ${s.scenes_count} scene(s) (${s.frames_count}f), ${s.symbols_count} sym (${fmtBytes(s.symbols_bytes)}), ${resText}, ${(s.fonts || []).length} font(s), ${scriptsTotal} script(s) (${fmtBytes(s.scripts_compiled_bytes)})`;
      if (loadInfoElement && chromeConfig.show_load_summary !== false && !isCleanChrome) {
        loadInfoElement.textContent = summaryText;
        loadInfoElement.title = summaryText;
      }
      if (!isCleanChrome) {
        global.__neoflashSuppressConsoleRelay = true;
        try { console.info(`[NeoFlash] ${summaryText}`); } finally { global.__neoflashSuppressConsoleRelay = false; }
      }
    }
    if (bundle.size_report && sizeReportElement && reportToggle && chromeConfig.show_load_summary !== false && !isCleanChrome) {
      sizeReportElement.textContent = bundle.size_report;
      reportToggle.hidden = false;
      reportToggle.addEventListener('click', () => {
        const open = sizeReportElement.style.display !== 'block';
        sizeReportElement.style.display = open ? 'block' : 'none';
        reportToggle.setAttribute('aria-pressed', String(open));
      });
    }
    let runtimeDisposed = false;
    let rafId = 0;
    let resizeRafId = 0;
    let resizeObserver = null;
    let moduleHandle = null;
    function teardownRuntime(freeWasmPlayer = !isStandaloneInstance) {
      if (runtimeDisposed) return;
      runtimeDisposed = true;
      const animation = rafId, resizeAnimation = resizeRafId, observer = resizeObserver;
      const listenersToRemove = ownedListeners.splice(0, ownedListeners.length);
      const domRootToRemove = scopedDom?.ownedRoot || null;
      const wasmPlayer = activePlayer;
      rafId = 0; resizeRafId = 0; resizeObserver = null;
      if (scopedDom) scopedDom.ownedRoot = null;
      if (freeWasmPlayer) activePlayer = null;
      completeCleanup([
        () => { if (animation) global.cancelAnimationFrame?.(animation); },
        () => { if (resizeAnimation) global.cancelAnimationFrame?.(resizeAnimation); },
        () => observer?.disconnect(),
        ...listenersToRemove.map(([target, type, handler, listenerOpts]) => () => target.removeEventListener?.(type, handler, listenerOpts)),
        disposeFamily,
        () => semanticLayer?.clear?.(),
        () => {
          if (a11yOverlay) {
            if (typeof a11yOverlay.replaceChildren === 'function') a11yOverlay.replaceChildren();
            else if ('textContent' in a11yOverlay) a11yOverlay.textContent = '';
          }
        },
        () => gradients.clear(),
        () => clippings.clear(),
        () => paths?.paths?.clear?.(),
        () => {
          if (domRootToRemove) {
            if (typeof domRootToRemove.remove === 'function') domRootToRemove.remove();
            else if (domRootToRemove.parentNode) domRootToRemove.parentNode.removeChild(domRootToRemove);
          }
        },
        () => {
          if (container && mountedContainers.get(container) === moduleHandle) {
            mountedContainers.delete(container);
          }
        },
        () => {
          if (freeWasmPlayer && wasmPlayer) {
            wasmPlayer.free?.();
          }
        },
      ]);
    }
    onPageHide = event => {
      if (runtimeDisposed) return;
      try { completeCleanup([() => storageHost?.flushAll(), () => { if(!event?.persisted)teardownRuntime(true); }]); }
      catch(error) { console.error(error); }
    };
    stopAsyncOnFailure=()=>completeCleanup([
      () => cleanupAdapters('reset'), () => gradients.clear(), () => clippings.clear(),
      () => filterHost?.renderer.clear(),
    ]);
    if (isStandaloneInstance) mountOptions.setExternalStopAsync?.(stopAsyncOnFailure);
    await ensureBundleFontsLoaded(instanceDoc || null, bundle?.fonts?.css_rules || '');
    if (runtimeDisposed) return null;
    initializeFamily(false);
    canvas.tabIndex=-1;
    stageHost?.attach();
    if (playerConfig.autoplay === false) {
      player.set_playing(false);
    }
    syncInitialStageLayout();
    resize();
    // Startup script failures use the same stopped-but-restartable policy as
    // later events. Keep installing the animation loop after reporting them.
    try { initializeProgram(); dispatchScripts(); render(); } catch (error) { fail(error); }
    if (typeof ResizeObserver === 'function' && viewport) {
      let lastObservedWidth = viewport.clientWidth;
      let lastObservedHeight = viewport.clientHeight;
      resizeObserver = new ResizeObserver(guardHostCallback(() => {
        if (runtimeDisposed) return;
        const nextWidth = viewport.clientWidth;
        const nextHeight = viewport.clientHeight;
        if (nextWidth === lastObservedWidth && nextHeight === lastObservedHeight) return;
        lastObservedWidth = nextWidth;
        lastObservedHeight = nextHeight;
        if (typeof global.requestAnimationFrame === 'function') {
          if (resizeRafId) return;
          resizeRafId = global.requestAnimationFrame(() => {
            resizeRafId = 0;
            guardHostCallback(() => { if (!runtimeDisposed) {
              lastObservedWidth = viewport.clientWidth;
              lastObservedHeight = viewport.clientHeight;
              resize();
              render();
            } })();
          });
        } else {
          resize();
          render();
        }
      }));
      resizeObserver.observe(viewport);
    }
    if (loadingElement?.style) loadingElement.style.display = 'none';
    function animate(now) {
      rafId = 0;
      if (runtimeDisposed) return;
      if (!failed && !instanceDoc?.hidden) {
        try {
          if (lastTime !== null && player.is_playing()) {
            accumulated += Math.min(now - lastTime, 250);
            const interval = 1000 / runtimeFrameRate;
            let ticks = 0;
            while (accumulated >= interval && ticks++ < 12) {
              accumulated -= interval;
              if (playerConfig.loop_timeline === false && player.total_frames() > 1 && player.current_frame() + 1 >= player.total_frames()) {
                player.set_playing(false);
                break;
              }
              if (lastTickNow !== null && now > lastTickNow) {
                fpsIntervals.push((now - lastTickNow) / ticks);
                if (fpsIntervals.length > 30) fpsIntervals.shift();
                const avgMs = fpsIntervals.reduce((a, b) => a + b, 0) / fpsIntervals.length;
                if (avgMs > 0) measuredFps = 1000 / avgMs;
              }
              lastTickNow = now;
              broadcastFrame();
              player.tick();
              dispatchScripts();
            }
            // Avoid a runaway catch-up after a suspended browser tab.
            if (ticks >= 12) accumulated = 0;
          }
          lastTime = now;
          render();
        } catch (error) { fail(error); }
      }
      if (!runtimeDisposed) {
        rafId = global.requestAnimationFrame(animate) || 0;
      }
    }
    if (!runtimeDisposed) {
      rafId = global.requestAnimationFrame(animate) || 0;
    }

    const assertLiveInstance = () => {
      if (runtimeDisposed) throw new Error('This NeoFlash module instance has been destroyed.');
    };
    const pauseInstance = () => {
      assertLiveInstance();
      player.set_playing(false);
      lastTime = null;
      accumulated = 0;
      resetMeasuredFps();
      drainSounds();
      drainVideos();
      if (toggle) toggle.textContent = 'Play';
    };
    const resumeInstance = () => {
      assertLiveInstance();
      resetMeasuredFps();
      lastTime = null;
      accumulated = 0;
      player.set_playing(true);
      drainSounds();
      drainVideos();
      if (toggle) toggle.textContent = 'Pause';
    };

    if (isStandaloneInstance) {
      const standaloneHandle = Object.freeze({
        get root() { return proxyFor([]); },
        get measuredFps() { return measuredFps; },
        get targetFps() { return runtimeFrameRate; },
        loadPublish: (url, target = null) => {
          if (!loaderHost?.loadPublish) throw new Error('Loader host is unavailable in this publication.');
          return loaderHost.loadPublish(url, target);
        },
        pause: () => { player.set_playing(false); resetMeasuredFps(); drainSounds(); drainVideos(); },
        resume: () => { resetMeasuredFps(); player.set_playing(true); drainVideos(); },
        restart: () => { if (restartButton?.click) restartButton.click(); else restartInstance(); },
      });
      global.NeoFlashPlayer = standaloneHandle;
      return standaloneHandle;
    }

    let rootStoppedByHost = false;
    moduleHandle = Object.freeze({
      get root() { return runtimeDisposed ? null : proxyFor([]); },
      container,
      get measuredFps() { return runtimeDisposed ? 0 : measuredFps; },
      get targetFps() { return runtimeDisposed ? 0 : runtimeFrameRate; },
      get isPlaying() { return !runtimeDisposed && !failed && !rootStoppedByHost && Boolean(player.is_playing()); },
      get currentFrame() { return runtimeDisposed ? 0 : player.current_frame() + 1; },
      get totalFrames() { return runtimeDisposed ? 0 : player.total_frames(); },
      get disposed() { return runtimeDisposed; },
      play: () => {
        const wasStoppedByHost = rootStoppedByHost;
        rootStoppedByHost = false;
        resumeInstance();
        const root = proxyFor([]);
        if ((wasStoppedByHost || root?.isPlaying === false) && typeof root?.play === 'function') {
          root.play();
          dispatchScripts();
          if (!failed) render();
        }
      },
      pause: () => pauseInstance(),
      resume: () => {
        const wasStoppedByHost = rootStoppedByHost;
        rootStoppedByHost = false;
        resumeInstance();
        if (wasStoppedByHost) {
          const root = proxyFor([]);
          if (typeof root?.play === 'function') {
            root.play();
            dispatchScripts();
            if (!failed) render();
          }
        }
      },
      restart: () => {
        assertLiveInstance();
        rootStoppedByHost = false;
        restartInstance();
      },
      gotoAndPlay: (frame, scene) => {
        assertLiveInstance();
        rootStoppedByHost = false;
        player.set_playing(true);
        if (toggle) toggle.textContent = 'Pause';
        const root = proxyFor([]);
        if (scene !== undefined && scene !== null) root.gotoAndPlay(frame, scene);
        else root.gotoAndPlay(frame);
        lastTime = null;
        accumulated = 0;
        resetMeasuredFps();
        dispatchScripts();
        if (!failed) render();
      },
      gotoAndStop: (frame, scene) => {
        assertLiveInstance();
        rootStoppedByHost = true;
        const root = proxyFor([]);
        if (scene !== undefined && scene !== null) root.gotoAndStop(frame, scene);
        else root.gotoAndStop(frame);
        lastTime = null;
        accumulated = 0;
        resetMeasuredFps();
        dispatchScripts();
        if (!failed) render();
      },
      loadPublish: (url, target = null) => {
        assertLiveInstance();
        if (!loaderHost?.loadPublish) throw new Error('Loader host is unavailable in this publication.');
        return loaderHost.loadPublish(url, target);
      },
      destroy: () => {
        teardownRuntime(true);
      },
    });
    if (container) {
      mountedContainers.set(container, moduleHandle);
    }
    return moduleHandle;
  }
  if (global.NeoFlashBundle && !global.__NEOFLASH_MANUAL_MOUNT__ && !(global.__neoflashManualMountDepth > 0)) {
    boot().catch(fail);
  }
})(globalThis);
