/* ActionScript 3 Area 8 runtime host adapters backed by the shared Rust As3Fte3dKernel:
 * 1. Flash Text Engine (`flash.text.engine.*`) & IME (`flash.text.ime.*`)
 * 2. Stage3D (`flash.display.Stage3D`, `flash.display3D.*`, `flash.display3D.textures.*`)
 * 3. Pixel Bender Shaders (`flash.display.Shader*`, `flash.display.GraphicsShaderFill`)
 * 4. 3D DisplayObject & Transform projections, MorphShape, AVM1Movie, and SoftKeyboard state.
 *
 * Paragraph layout, atomization, AGAL v1/v2 shader execution, software triangle/wireframe
 * rasterization, Pixel Bender .pbj evaluation, and 4x4 matrix projections all execute in
 * Rust (`crates/neoflash-core/src/as3_fte_3d.rs`).
 */
(function (global) {
  'use strict';

  function argError(message, id = 2004) {
    const err = new RangeError(message);
    err.name = 'ArgumentError';
    err.errorID = id;
    return err;
  }

  function rangeError(message, id = 2006) {
    const err = new RangeError(message);
    err.name = 'RangeError';
    err.errorID = id;
    return err;
  }

  function illegalOpError(message, id = 2037) {
    const err = new Error(message);
    err.name = 'IllegalOperationError';
    err.errorID = id;
    return err;
  }

  function mapKernelError(raw) {
    const text = raw instanceof Error ? raw.message : String(raw);
    if (text.startsWith('ArgumentError:')) return argError(text);
    if (text.startsWith('RangeError:')) return rangeError(text);
    if (text.startsWith('IllegalOperationError:')) return illegalOpError(text);
    if (text.startsWith('TypeError:')) return new TypeError(text);
    return new Error(text);
  }

  function kcall(fn) {
    try {
      return fn();
    } catch (err) {
      throw mapKernelError(err);
    }
  }

  function defConstClass(name, constants) {
    const Ctor = {
      [name]: function () {
        throw argError(`ArgumentError: Error #2012: ${name}$ class cannot be instantiated.`, 2012);
      },
    }[name];
    for (const [k, v] of Object.entries(constants)) {
      Object.defineProperty(Ctor, k, { value: v, enumerable: true });
    }
    Object.defineProperties(Ctor, {
      __nfIs: { value: () => false },
      __nfInitialize: {
        value: () => {
          throw new TypeError(`${name} supplies constants only and cannot be extended.`);
        },
      },
      [Symbol.hasInstance]: { value: () => false },
    });
    return Object.freeze(Ctor);
  }

  function tagType(Ctor, name, check, initFn) {
    Object.defineProperties(Ctor, {
      __nfIs: { value: check },
      __nfInitialize: {
        value:
          initFn ||
          (() => {
            throw new TypeError(`${name} is final or player-instantiated and cannot be subclassed.`);
          }),
      },
      [Symbol.hasInstance]: { value: check },
    });
    return Ctor;
  }

  function bytesToBase64(u8) {
    if (typeof Buffer !== 'undefined') return Buffer.from(u8).toString('base64');
    let bin = '';
    for (let i = 0; i < u8.length; i++) bin += String.fromCharCode(u8[i]);
    return global.btoa(bin);
  }

  function base64ToBytes(b64) {
    if (typeof Buffer !== 'undefined') return new Uint8Array(Buffer.from(b64, 'base64'));
    const bin = global.atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  function rgbaToArgbBE(rgba) {
    const out = new Uint8Array(rgba.length);
    for (let i = 0; i + 3 < rgba.length; i += 4) {
      out[i] = rgba[i + 3];
      out[i + 1] = rgba[i];
      out[i + 2] = rgba[i + 1];
      out[i + 3] = rgba[i + 2];
    }
    return out;
  }

  // ---- Shared flash.display & Shader constant classes ----
  const ActionScriptVersion = defConstClass('ActionScriptVersion', {
    ACTIONSCRIPT2: 2,
    ACTIONSCRIPT3: 3,
  });

  const SWFVersion = defConstClass('SWFVersion', {
    FLASH1: 1,
    FLASH2: 2,
    FLASH3: 3,
    FLASH4: 4,
    FLASH5: 5,
    FLASH6: 6,
    FLASH7: 7,
    FLASH8: 8,
    FLASH9: 9,
    FLASH10: 10,
    FLASH11: 11,
    FLASH12: 12,
  });

  const ColorCorrection = defConstClass('ColorCorrection', {
    DEFAULT: 'default',
    ON: 'on',
    OFF: 'off',
  });

  const ColorCorrectionSupport = defConstClass('ColorCorrectionSupport', {
    UNSUPPORTED: 'unsupported',
    DEFAULT_ON: 'defaultOn',
    DEFAULT_OFF: 'defaultOff',
  });

  const ShaderPrecision = defConstClass('ShaderPrecision', {
    FULL: 'full',
    FAST: 'fast',
  });

  const ShaderParameterType = defConstClass('ShaderParameterType', {
    FLOAT: 'float',
    FLOAT2: 'float2',
    FLOAT3: 'float3',
    FLOAT4: 'float4',
    INT: 'int',
    INT2: 'int2',
    INT3: 'int3',
    INT4: 'int4',
    BOOL: 'bool',
    BOOL2: 'bool2',
    BOOL3: 'bool3',
    BOOL4: 'bool4',
    MATRIX2X2: 'matrix2x2',
    MATRIX3X3: 'matrix3x3',
    MATRIX4X4: 'matrix4x4',
  });

  // ---- Stage3D constant classes ----
  const Context3DBlendFactor = defConstClass('Context3DBlendFactor', {
    DESTINATION_ALPHA: 'destinationAlpha',
    DESTINATION_COLOR: 'destinationColor',
    ONE: 'one',
    ONE_MINUS_DESTINATION_ALPHA: 'oneMinusDestinationAlpha',
    ONE_MINUS_DESTINATION_COLOR: 'oneMinusDestinationColor',
    ONE_MINUS_SOURCE_ALPHA: 'oneMinusSourceAlpha',
    ONE_MINUS_SOURCE_COLOR: 'oneMinusSourceColor',
    SOURCE_ALPHA: 'sourceAlpha',
    SOURCE_COLOR: 'sourceColor',
    ZERO: 'zero',
  });

  const Context3DBufferUsage = defConstClass('Context3DBufferUsage', {
    DYNAMIC_DRAW: 'dynamicDraw',
    STATIC_DRAW: 'staticDraw',
  });

  const Context3DClearMask = defConstClass('Context3DClearMask', {
    COLOR: 1,
    DEPTH: 2,
    STENCIL: 4,
    ALL: 7,
  });

  const Context3DCompareMode = defConstClass('Context3DCompareMode', {
    ALWAYS: 'always',
    EQUAL: 'equal',
    GREATER: 'greater',
    GREATER_EQUAL: 'greaterEqual',
    LESS: 'less',
    LESS_EQUAL: 'lessEqual',
    NEVER: 'never',
    NOT_EQUAL: 'notEqual',
  });

  const Context3DFillMode = defConstClass('Context3DFillMode', {
    SOLID: 'solid',
    WIREFRAME: 'wireframe',
  });

  const Context3DMipFilter = defConstClass('Context3DMipFilter', {
    MIPLINEAR: 'miplinear',
    MIPNEAREST: 'mipnearest',
    MIPNONE: 'mipnone',
  });

  const Context3DProfile = defConstClass('Context3DProfile', {
    BASELINE: 'baseline',
    BASELINE_CONSTRAINED: 'baselineConstrained',
    BASELINE_EXTENDED: 'baselineExtended',
    ENHANCED: 'enhanced',
    STANDARD: 'standard',
    STANDARD_CONSTRAINED: 'standardConstrained',
    STANDARD_EXTENDED: 'standardExtended',
  });

  const Context3DProgramType = defConstClass('Context3DProgramType', {
    FRAGMENT: 'fragment',
    VERTEX: 'vertex',
  });

  const Context3DRenderMode = defConstClass('Context3DRenderMode', {
    AUTO: 'auto',
    SOFTWARE: 'software',
  });

  const Context3DStencilAction = defConstClass('Context3DStencilAction', {
    DECREMENT_SATURATE: 'decrementSaturate',
    DECREMENT_WRAP: 'decrementWrap',
    INCREMENT_SATURATE: 'incrementSaturate',
    INCREMENT_WRAP: 'incrementWrap',
    INVERT: 'invert',
    KEEP: 'keep',
    SET: 'set',
    ZERO: 'zero',
  });

  const Context3DTextureFilter = defConstClass('Context3DTextureFilter', {
    ANISOTROPIC2X: 'anisotropic2x',
    ANISOTROPIC4X: 'anisotropic4x',
    ANISOTROPIC8X: 'anisotropic8x',
    ANISOTROPIC16X: 'anisotropic16x',
    LINEAR: 'linear',
    NEAREST: 'nearest',
  });

  const Context3DTextureFormat = defConstClass('Context3DTextureFormat', {
    BGRA: 'bgra',
    BGRA_PACKED: 'bgraPacked4444',
    BGR_PACKED: 'bgrPacked565',
    COMPRESSED: 'compressed',
    COMPRESSED_ALPHA: 'compressedAlpha',
    RGBA_HALF_FLOAT: 'rgbaHalfFloat',
  });

  const Context3DTriangleFace = defConstClass('Context3DTriangleFace', {
    BACK: 'back',
    FRONT: 'front',
    FRONT_AND_BACK: 'frontAndBack',
    NONE: 'none',
  });

  const Context3DVertexBufferFormat = defConstClass('Context3DVertexBufferFormat', {
    BYTES_4: 'bytes4',
    FLOAT_1: 'float1',
    FLOAT_2: 'float2',
    FLOAT_3: 'float3',
    FLOAT_4: 'float4',
  });

  const Context3DWrapMode = defConstClass('Context3DWrapMode', {
    CLAMP: 'clamp',
    CLAMP_U_REPEAT_V: 'clamp_u_repeat_v',
    REPEAT: 'repeat',
    REPEAT_U_CLAMP_V: 'repeat_u_clamp_v',
  });

  // ---- Flash Text Engine constant classes ----
  const BreakOpportunity = defConstClass('BreakOpportunity', {
    ALL: 'all',
    ANY: 'any',
    AUTO: 'auto',
    NONE: 'none',
  });

  const CFFHinting = defConstClass('CFFHinting', {
    HORIZONTAL_STEM: 'horizontalStem',
    NONE: 'none',
  });

  const DigitCase = defConstClass('DigitCase', {
    DEFAULT: 'default',
    LINING: 'lining',
    OLD_STYLE: 'oldStyle',
  });

  const DigitWidth = defConstClass('DigitWidth', {
    DEFAULT: 'default',
    PROPORTIONAL: 'proportional',
    TABULAR: 'tabular',
  });

  const FontLookup = defConstClass('FontLookup', {
    DEVICE: 'device',
    EMBEDDED_CFF: 'embeddedCFF',
  });

  const FontPosture = defConstClass('FontPosture', {
    ITALIC: 'italic',
    NORMAL: 'normal',
  });

  const FontWeight = defConstClass('FontWeight', {
    BOLD: 'bold',
    NORMAL: 'normal',
  });

  const JustificationStyle = defConstClass('JustificationStyle', {
    PRIORITIZE_LEAST_ADJUSTMENT: 'prioritizeLeastAdjustment',
    PUSH_IN_KINSOKU: 'pushInKinsoku',
    PUSH_OUT_ONLY: 'pushOutOnly',
  });

  const Kerning = defConstClass('Kerning', {
    AUTO: 'auto',
    OFF: 'off',
    ON: 'on',
  });

  const LigatureLevel = defConstClass('LigatureLevel', {
    COMMON: 'common',
    EXOTIC: 'exotic',
    MINIMUM: 'minimum',
    NONE: 'none',
    UNCOMMON: 'uncommon',
  });

  const LineJustification = defConstClass('LineJustification', {
    ALL_BUT_LAST: 'allButLast',
    ALL_BUT_MANDATORY_BREAK: 'allButMandatoryBreak',
    ALL_INCLUDING_LAST: 'allIncludingLast',
    UNJUSTIFIED: 'unjustified',
  });

  const RenderingMode = defConstClass('RenderingMode', {
    CFF: 'cff',
    NORMAL: 'normal',
  });

  const TabAlignment = defConstClass('TabAlignment', {
    CENTER: 'center',
    DECIMAL: 'decimal',
    END: 'end',
    START: 'start',
  });

  const TextBaseline = defConstClass('TextBaseline', {
    ASCENT: 'ascent',
    DESCENT: 'descent',
    IDEOGRAPHIC_BOTTOM: 'ideographicBottom',
    IDEOGRAPHIC_CENTER: 'ideographicCenter',
    IDEOGRAPHIC_TOP: 'ideographicTop',
    ROMAN: 'roman',
    USE_DOMINANT_BASELINE: 'useDominantBaseline',
  });

  const TextLineCreationResult = defConstClass('TextLineCreationResult', {
    COMPLETE: 'complete',
    EMERGENCY: 'emergency',
    INSUFFICIENT_WIDTH: 'insufficientWidth',
    SUCCESS: 'success',
  });

  const TextLineValidity = defConstClass('TextLineValidity', {
    INVALID: 'invalid',
    POSSIBLY_INVALID: 'possiblyInvalid',
    STATIC: 'static',
    VALID: 'valid',
  });

  const TextRotation = defConstClass('TextRotation', {
    AUTO: 'auto',
    ROTATE_0: 'rotate0',
    ROTATE_90: 'rotate90',
    ROTATE_180: 'rotate180',
    ROTATE_270: 'rotate270',
  });

  const TypographicCase = defConstClass('TypographicCase', {
    CAPS: 'caps',
    CAPS_AND_SMALL_CAPS: 'capsAndSmallCaps',
    DEFAULT: 'default',
    LOWERCASE: 'lowercase',
    SMALL_CAPS: 'smallCaps',
    TITLE: 'title',
    UPPERCASE: 'uppercase',
  });

  // Global weak registry for Shader instances so NeoFlashFilters.ShaderFilter can inspect them without circular host initialization.
  const globalShaderStates = new WeakMap();

  function inspectShaderForFilter(shader) {
    const st = globalShaderStates.get(shader);
    if (!st) return null;
    if (!st.bytes || !st.data) {
      throw argError('ArgumentError: ShaderFilter requires a Shader with valid byteCode.');
    }
    const params = {};
    for (const p of st.paramList) {
      if (Array.isArray(p.value)) {
        params[p._name] = p.value.map(v => (typeof v === 'boolean' ? (v ? 1 : 0) : Number(v) || 0));
      }
    }
    return {
      isShader: true,
      bytecode_base64: bytesToBase64(st.bytes),
      precision_hint: st.precisionHint || 'full',
      params_json: JSON.stringify(params),
    };
  }

  function create(options = {}) {
    const wasm = options.wasm || global.NeoFlashWasm;
    const events = options.events || global.NeoFlashEvents;
    if (!wasm?.As3Fte3dKernel || !events?.Event || !events?.EventDispatcher) {
      throw new Error('NeoFlashFte3D requires As3Fte3dKernel WASM bindings and NeoFlashEvents.');
    }
    const player = options.player || null;
    const binary = options.binary || null;
    const geometry = options.geometry || null;
    const geometry3d = options.geometry3d || null;
    const getBitmap = options.getBitmap || (() => null);
    const getDisplay = options.getDisplay || (() => null);
    const getStage = options.getStage || (() => null);
    const getBuiltin = options.getBuiltin || (() => null);
    const proxyForRef = options.proxyForRef || (() => null);
    const referenceOf = options.referenceOf || (() => null);
    const schedule =
      options.schedule ||
      (cb => {
        if (typeof global.queueMicrotask === 'function') global.queueMicrotask(cb);
        else Promise.resolve().then(cb);
      });
    const afterEvent = options.afterEvent || (() => {});
    const onError = options.onError || (err => { throw err; });

    let kernel = new wasm.As3Fte3dKernel();
    let generation = 1;
    let disposed = false;
    let vectorFactory = (_typeName, values) => Array.from(values);

    function setVectorFactory(fn) {
      if (typeof fn === 'function') vectorFactory = fn;
    }

    function makeVector(typeName, values, fixed = false) {
      const vec = vectorFactory(typeName, Array.from(values));
      if (fixed && vec && typeof vec === 'object' && 'fixed' in vec) {
        try { vec.fixed = true; } catch (_) { /* array fallback */ }
      }
      return vec;
    }

    function isVectorNumber(val) {
      if (!val || typeof val !== 'object') return false;
      if (Array.isArray(val)) return false;
      return typeof val.length === 'number' && typeof val[Symbol.iterator] === 'function' && !binary?.ByteArray?.__nfIs?.(val);
    }

    function readByteArraySlice(ba, offset = 0, byteLength = null) {
      if (!binary?.ByteArray?.__nfIs?.(ba)) {
        throw new TypeError('Expected a flash.utils.ByteArray instance.');
      }
      const bytes = binary.toBytes(ba);
      const off = Number(offset) | 0;
      if (off < 0 || off > bytes.length) {
        throw rangeError('RangeError: Error #2006: The supplied ByteArray offset is out of bounds.');
      }
      if (byteLength === null || byteLength === undefined) return bytes.slice(off);
      const len = Number(byteLength) | 0;
      if (len < 0 || off + len > bytes.length) {
        throw rangeError('RangeError: Error #2006: ByteArray does not contain enough bytes.');
      }
      return bytes.slice(off, off + len);
    }

    function writeFloatsToByteArray(ba, floats) {
      ba.length = 0;
      const buf = new ArrayBuffer(floats.length * 4);
      const view = new DataView(buf);
      const little = ba.endian === 'littleEndian';
      for (let i = 0; i < floats.length; i++) {
        view.setFloat32(i * 4, floats[i], little);
      }
      const u8 = new Uint8Array(buf);
      for (let i = 0; i < u8.length; i++) ba.writeByte(u8[i]);
      ba.position = 0;
    }

    function extractFloatsFromByteArray(ba, byteOffset, floatCount) {
      const slice = readByteArraySlice(ba, byteOffset, floatCount * 4);
      const view = new DataView(slice.buffer, slice.byteOffset, slice.byteLength);
      const little = ba.endian === 'littleEndian';
      const out = new Float32Array(floatCount);
      for (let i = 0; i < floatCount; i++) {
        out[i] = view.getFloat32(i * 4, little);
      }
      return out;
    }

    function writeRgbaToBitmapData(bmp, width, height, rgba) {
      const bmpHost = getBitmap();
      if (!bmpHost || !bmpHost.BitmapData.__nfIs(bmp)) {
        throw new TypeError('Expected a flash.display.BitmapData instance.');
      }
      const pref = JSON.stringify(bmpHost.refOf(bmp));
      const w = Math.min(width, bmp.width);
      const h = Math.min(height, bmp.height);
      if (w <= 0 || h <= 0) return;
      let cropped = rgba;
      if (w !== width || h !== height) {
        cropped = new Uint8Array(w * h * 4);
        for (let y = 0; y < h; y++) {
          cropped.set(rgba.subarray(y * width * 4, y * width * 4 + w * 4), y * w * 4);
        }
      }
      player.bitmap_set_pixels(pref, 0, 0, w, h, rgbaToArgbBE(cropped), false);
    }

    // ---- Fallback sibling Wave 5 types (only used if sibling stream is not merged yet) ----
    class FallbackShaderEvent extends events.Event {
      constructor(type, bubbles = false, cancelable = false, bitmap = null, byteArray = null, vector = null) {
        super(type, Boolean(bubbles), Boolean(cancelable));
        this.bitmapData = bitmap;
        this.byteArray = byteArray;
        this.vector = vector;
      }
      clone() {
        return new FallbackShaderEvent(this.type, this.bubbles, this.cancelable, this.bitmapData, this.byteArray, this.vector);
      }
    }
    FallbackShaderEvent.COMPLETE = 'complete';

    function makeShaderCompleteEvent(bitmap, byteArray, vector) {
      const SE = getBuiltin('flash.events.ShaderEvent') || FallbackShaderEvent;
      return new SE('complete', false, false, bitmap, byteArray, vector);
    }

    const fallbackUncaughtSet = new WeakSet();
    class FallbackUncaughtErrorEvents extends events.EventDispatcher {
      constructor() {
        super();
        fallbackUncaughtSet.add(this);
      }
    }
    tagType(
      FallbackUncaughtErrorEvents,
      'UncaughtErrorEvents',
      v => fallbackUncaughtSet.has(v),
      receiver => { fallbackUncaughtSet.add(receiver); }
    );

    const fallbackStageVideoMap = new WeakMap();
    class FallbackStageVideo extends events.EventDispatcher {
      constructor() {
        super();
        fallbackStageVideoMap.set(this, {
          depth: 0,
          pan: geometry?.Point ? new geometry.Point(0, 0) : { x: 0, y: 0 },
          zoom: geometry?.Point ? new geometry.Point(1, 1) : { x: 1, y: 1 },
          viewPort: geometry?.Rectangle ? new geometry.Rectangle(0, 0, 0, 0) : { x: 0, y: 0, width: 0, height: 0 },
          videoWidth: 0,
          videoHeight: 0,
        });
      }
      attachNetStream(_ns) {}
      attachCamera(_cam) {}
      get colorSpaces() { return makeVector('String', ['BT.601', 'BT.709']); }
      get depth() { return fallbackStageVideoMap.get(this).depth; }
      set depth(v) { fallbackStageVideoMap.get(this).depth = Number(v) | 0; }
      get pan() { return fallbackStageVideoMap.get(this).pan; }
      set pan(v) { fallbackStageVideoMap.get(this).pan = v; }
      get zoom() { return fallbackStageVideoMap.get(this).zoom; }
      set zoom(v) { fallbackStageVideoMap.get(this).zoom = v; }
      get viewPort() { return fallbackStageVideoMap.get(this).viewPort; }
      set viewPort(v) { fallbackStageVideoMap.get(this).viewPort = v; }
      get videoWidth() { return fallbackStageVideoMap.get(this).videoWidth; }
      get videoHeight() { return fallbackStageVideoMap.get(this).videoHeight; }
    }
    tagType(FallbackStageVideo, 'StageVideo', v => fallbackStageVideoMap.has(v));

    // =========================================================================
    // 1. Pixel Bender Shaders (`Shader`, `ShaderData`, `ShaderInput`, `ShaderParameter`, `ShaderJob`, `GraphicsShaderFill`)
    // =========================================================================

    const shaderInputMap = new WeakMap();
    class ShaderInput {
      constructor() {
        if (new.target !== ShaderInput) {
          throw new TypeError('ShaderInput is final and cannot be subclassed.');
        }
        shaderInputMap.set(this, {
          name: '',
          channels: 0,
          index: 0,
          input: null,
          width: 0,
          height: 0,
        });
      }
      get channels() { return shaderInputMap.get(this).channels; }
      get index() { return shaderInputMap.get(this).index; }
      get input() { return shaderInputMap.get(this).input; }
      set input(val) {
        const rec = shaderInputMap.get(this);
        if (val === null || val === undefined) {
          rec.input = null;
          return;
        }
        const bmpHost = getBitmap();
        const isBmp = Boolean(bmpHost?.BitmapData?.__nfIs?.(val));
        const isBa = Boolean(binary?.ByteArray?.__nfIs?.(val));
        const isVec = isVectorNumber(val);
        if (!isBmp && !isBa && !isVec) {
          throw new TypeError('ShaderInput.input must be a BitmapData, ByteArray, Vector.<Number>, or null.');
        }
        rec.input = val;
        if (isBmp) {
          rec.width = val.width;
          rec.height = val.height;
        }
      }
      get width() {
        const rec = shaderInputMap.get(this);
        const bmpHost = getBitmap();
        if (rec.input && bmpHost?.BitmapData?.__nfIs?.(rec.input)) return rec.input.width;
        return rec.width;
      }
      set width(v) {
        const n = Number(v) | 0;
        if (n < 0) throw argError('ShaderInput.width must be non-negative.');
        shaderInputMap.get(this).width = n;
      }
      get height() {
        const rec = shaderInputMap.get(this);
        const bmpHost = getBitmap();
        if (rec.input && bmpHost?.BitmapData?.__nfIs?.(rec.input)) return rec.input.height;
        return rec.height;
      }
      set height(v) {
        const n = Number(v) | 0;
        if (n < 0) throw argError('ShaderInput.height must be non-negative.');
        shaderInputMap.get(this).height = n;
      }
    }
    tagType(ShaderInput, 'ShaderInput', v => shaderInputMap.has(v));

    const shaderParamMap = new WeakMap();
    class ShaderParameter {
      constructor() {
        if (new.target !== ShaderParameter) {
          throw new TypeError('ShaderParameter is final and cannot be subclassed.');
        }
        shaderParamMap.set(this, {
          name: '',
          index: 0,
          type: 'float',
          value: null,
        });
      }
      get index() { return shaderParamMap.get(this).index; }
      get type() { return shaderParamMap.get(this).type; }
      get value() {
        const v = shaderParamMap.get(this).value;
        return Array.isArray(v) ? v.slice() : null;
      }
      set value(arr) {
        const rec = shaderParamMap.get(this);
        if (arr === null || arr === undefined) {
          rec.value = null;
          return;
        }
        if (!Array.isArray(arr)) {
          throw new TypeError('ShaderParameter.value requires an Array or null.');
        }
        const isBool = rec.type.startsWith('bool');
        const isInt = rec.type.startsWith('int');
        rec.value = arr.map(x => (isBool ? Boolean(x) : isInt ? (Number(x) | 0) : Number(x)));
      }
    }
    tagType(ShaderParameter, 'ShaderParameter', v => shaderParamMap.has(v));

    const shaderDataMap = new WeakMap();
    class ShaderData {
      constructor(byteCode) {
        if (!binary?.ByteArray?.__nfIs?.(byteCode)) {
          throw new TypeError('ShaderData constructor requires a non-null ByteArray.');
        }
        const bytes = binary.toBytes(byteCode);
        const meta = JSON.parse(kcall(() => kernel.parse_shader_bytecode_json(bytes)));
        const inputList = [];
        const paramList = [];
        this.name = meta.name;
        if (meta.metadata && typeof meta.metadata === 'object') {
          for (const [mk, mv] of Object.entries(meta.metadata)) {
            this[mk] = mv;
          }
        }
        for (const inp of meta.inputs || []) {
          const si = new ShaderInput();
          const irec = shaderInputMap.get(si);
          irec.name = inp.name;
          irec.index = inp.index >>> 0;
          irec.channels = inp.channels >>> 0;
          this[inp.name] = si;
          inputList.push(si);
        }
        for (const par of meta.parameters || []) {
          const sp = new ShaderParameter();
          const prec = shaderParamMap.get(sp);
          prec.name = par.name;
          prec.index = par.index >>> 0;
          prec.type = par.type;
          const isBool = par.type.startsWith('bool');
          const isInt = par.type.startsWith('int');
          prec.value = (par.defaultValue || []).map(x => (isBool ? Boolean(x) : isInt ? (Number(x) | 0) : Number(x)));
          Object.defineProperty(sp, '_name', { value: par.name, enumerable: false });
          if (par.metadata && typeof par.metadata === 'object') {
            for (const [mk, mv] of Object.entries(par.metadata)) {
              sp[mk] = Array.isArray(mv) ? mv.slice() : mv;
            }
          }
          this[par.name] = sp;
          paramList.push(sp);
        }
        shaderDataMap.set(this, { bytes: bytes.slice(), meta, inputList, paramList });
      }
    }
    tagType(ShaderData, 'ShaderData', v => shaderDataMap.has(v));

    class Shader {
      constructor(code = null) {
        const st = {
          bytes: null,
          data: null,
          precisionHint: 'full',
          inputList: [],
          paramList: [],
        };
        globalShaderStates.set(this, st);
        if (code !== null && code !== undefined) {
          this.byteCode = code;
        }
      }
      set byteCode(code) {
        if (!binary?.ByteArray?.__nfIs?.(code)) {
          throw new TypeError('Shader.byteCode requires a ByteArray.');
        }
        const sd = new ShaderData(code);
        this.data = sd;
      }
      get data() {
        return globalShaderStates.get(this).data;
      }
      set data(val) {
        if (!shaderDataMap.has(val)) {
          throw new TypeError('Shader.data requires a ShaderData instance.');
        }
        const st = globalShaderStates.get(this);
        const sdState = shaderDataMap.get(val);
        st.data = val;
        st.bytes = sdState.bytes.slice();
        st.inputList = sdState.inputList;
        st.paramList = sdState.paramList;
      }
      get precisionHint() {
        return globalShaderStates.get(this).precisionHint;
      }
      set precisionHint(val) {
        const s = String(val);
        kcall(() => kernel.validate_shader_precision(s));
        globalShaderStates.get(this).precisionHint = s;
      }
    }
    tagType(
      Shader,
      'Shader',
      v => globalShaderStates.has(v),
      (receiver, args = []) => {
        globalShaderStates.set(receiver, {
          bytes: null,
          data: null,
          precisionHint: 'full',
          inputList: [],
          paramList: [],
        });
        if (args[0] !== undefined && args[0] !== null) receiver.byteCode = args[0];
      }
    );

    function evaluateShaderInstance(shader, outWidth, outHeight) {
      const st = globalShaderStates.get(shader);
      if (!st || !st.bytes || !st.data) {
        throw argError('ArgumentError: Shader has no byteCode or ShaderData assigned.');
      }
      const bmpHost = getBitmap();
      const inputs = [];
      for (const si of st.inputList) {
        const irec = shaderInputMap.get(si);
        if (!irec.input) {
          throw argError(`ArgumentError: ShaderInput '${irec.name}' has no input assigned.`);
        }
        if (bmpHost?.BitmapData?.__nfIs?.(irec.input)) {
          const pref = JSON.stringify(bmpHost.refOf(irec.input));
          const px = player.bitmap_pixels(pref);
          inputs.push({
            name: irec.name,
            width: irec.input.width,
            height: irec.input.height,
            channels: irec.channels,
            rgbaBase64: bytesToBase64(px),
          });
        } else if (binary?.ByteArray?.__nfIs?.(irec.input)) {
          const need = irec.width * irec.height * irec.channels;
          if (irec.width <= 0 || irec.height <= 0) {
            throw argError(`ArgumentError: ShaderInput '${irec.name}' requires positive width and height for ByteArray input.`);
          }
          const floats = Array.from(extractFloatsFromByteArray(irec.input, 0, need));
          inputs.push({
            name: irec.name,
            width: irec.width,
            height: irec.height,
            channels: irec.channels,
            floats,
          });
        } else if (isVectorNumber(irec.input)) {
          const need = irec.width * irec.height * irec.channels;
          if (irec.width <= 0 || irec.height <= 0 || irec.input.length < need) {
            throw argError(`ArgumentError: ShaderInput '${irec.name}' Vector.<Number> is smaller than width*height*channels.`);
          }
          const floats = [];
          for (let i = 0; i < need; i++) floats.push(Number(irec.input[i]) || 0);
          inputs.push({
            name: irec.name,
            width: irec.width,
            height: irec.height,
            channels: irec.channels,
            floats,
          });
        }
      }
      const parameters = {};
      for (const sp of st.paramList) {
        const prec = shaderParamMap.get(sp);
        if (Array.isArray(prec.value)) {
          parameters[prec.name] = prec.value.map(v => (typeof v === 'boolean' ? (v ? 1 : 0) : Number(v) || 0));
        }
      }
      const req = {
        bytecodeBase64: bytesToBase64(st.bytes),
        precisionHint: st.precisionHint,
        width: outWidth,
        height: outHeight,
        inputs,
        parameters,
      };
      return JSON.parse(kcall(() => kernel.evaluate_shader_json(JSON.stringify(req))));
    }

    function rasterizeShaderToBitmapData(shader, width = 64, height = 64) {
      if (!globalShaderStates.has(shader)) {
        throw new TypeError('Graphics shader fill requires a flash.display.Shader instance.');
      }
      const bmpHost = getBitmap();
      if (!bmpHost) throw new Error('Shader fill requires the BitmapData service.');
      const st = globalShaderStates.get(shader);
      if (!st.bytes || !st.data) {
        throw argError('ArgumentError: Graphics shader fill requires a Shader with valid byteCode.');
      }
      let w = width;
      let h = height;
      if (st.inputList.length > 0) {
        const first = st.inputList[0];
        if (first.width > 0 && first.height > 0) {
          w = first.width;
          h = first.height;
        }
      }
      const res = evaluateShaderInstance(shader, w, h);
      const rgba = base64ToBytes(res.rgbaBase64);
      const bmp = new bmpHost.BitmapData(w, h, true, 0);
      writeRgbaToBitmapData(bmp, w, h, rgba);
      return bmp;
    }

    const shaderJobMap = new WeakMap();
    class ShaderJob extends events.EventDispatcher {
      constructor(shader = null, target = null, width = 0, height = 0) {
        super();
        const st = {
          shader: null,
          target: null,
          width: 0,
          height: 0,
          progress: 0,
          running: false,
          jobToken: 0,
        };
        shaderJobMap.set(this, st);
        if (shader !== null) this.shader = shader;
        if (target !== null) this.target = target;
        this.width = width;
        this.height = height;
      }
      get shader() { return shaderJobMap.get(this).shader; }
      set shader(v) {
        if (v !== null && !globalShaderStates.has(v)) {
          throw new TypeError('ShaderJob.shader requires a Shader instance or null.');
        }
        shaderJobMap.get(this).shader = v;
      }
      get target() { return shaderJobMap.get(this).target; }
      set target(v) {
        if (v !== null && v !== undefined) {
          const bmpHost = getBitmap();
          const ok =
            Boolean(bmpHost?.BitmapData?.__nfIs?.(v)) ||
            Boolean(binary?.ByteArray?.__nfIs?.(v)) ||
            isVectorNumber(v);
          if (!ok) {
            throw new TypeError('ShaderJob.target must be a BitmapData, ByteArray, Vector.<Number>, or null.');
          }
        }
        shaderJobMap.get(this).target = v ?? null;
      }
      get width() { return shaderJobMap.get(this).width; }
      set width(v) { shaderJobMap.get(this).width = Number(v) | 0; }
      get height() { return shaderJobMap.get(this).height; }
      set height(v) { shaderJobMap.get(this).height = Number(v) | 0; }
      get progress() { return shaderJobMap.get(this).progress; }
      start(waitForCompletion = false) {
        const st = shaderJobMap.get(this);
        if (st.running) {
          throw illegalOpError('IllegalOperationError: ShaderJob is already running.');
        }
        if (!st.shader) throw argError('ArgumentError: ShaderJob.shader must be set before start().');
        if (!st.target) throw argError('ArgumentError: ShaderJob.target must be set before start().');
        const bmpHost = getBitmap();
        const isBmp = Boolean(bmpHost?.BitmapData?.__nfIs?.(st.target));
        const isBa = Boolean(binary?.ByteArray?.__nfIs?.(st.target));
        const outW = isBmp ? st.target.width : st.width;
        const outH = isBmp ? st.target.height : st.height;
        if (outW <= 0 || outH <= 0) {
          throw argError('ArgumentError: ShaderJob output width and height must be greater than zero.');
        }
        // Validate and evaluate through Rust immediately so invalid inputs throw synchronously per Flash ShaderJob rules.
        const res = evaluateShaderInstance(st.shader, outW, outH);
        const commitOutput = () => {
          if (isBmp) {
            writeRgbaToBitmapData(st.target, outW, outH, base64ToBytes(res.rgbaBase64));
          } else if (isBa) {
            writeFloatsToByteArray(st.target, res.floats);
          } else {
            for (let i = 0; i < res.floats.length; i++) {
              st.target[i] = res.floats[i];
            }
          }
          st.progress = 1;
          st.running = false;
        };
        if (Boolean(waitForCompletion)) {
          commitOutput();
          return;
        }
        st.running = true;
        st.progress = 0;
        const myGen = generation;
        const token = ++st.jobToken;
        schedule(() => {
          if (disposed || myGen !== generation || !st.running || st.jobToken !== token) return;
          try {
            commitOutput();
            this.dispatchEvent(
              makeShaderCompleteEvent(
                isBmp ? st.target : null,
                isBa ? st.target : null,
                !isBmp && !isBa ? st.target : null
              )
            );
            afterEvent();
          } catch (err) {
            onError(err);
          }
        });
      }
      cancel() {
        const st = shaderJobMap.get(this);
        if (st.running) {
          st.running = false;
          st.jobToken++;
          st.progress = 0;
        }
      }
    }
    tagType(
      ShaderJob,
      'ShaderJob',
      v => shaderJobMap.has(v),
      (receiver, args = []) => {
        shaderJobMap.set(receiver, {
          shader: null,
          target: null,
          width: 0,
          height: 0,
          progress: 0,
          running: false,
          jobToken: 0,
        });
        if (args[0] !== undefined && args[0] !== null) receiver.shader = args[0];
        if (args[1] !== undefined && args[1] !== null) receiver.target = args[1];
        if (args[2] !== undefined) receiver.width = args[2];
        if (args[3] !== undefined) receiver.height = args[3];
      }
    );

    const graphicsShaderFillMap = new WeakMap();
    class GraphicsShaderFill {
      constructor(shader = null, matrix = null) {
        if (new.target !== GraphicsShaderFill) {
          throw new TypeError('GraphicsShaderFill is final and cannot be subclassed.');
        }
        const st = { shader: null, matrix: null };
        graphicsShaderFillMap.set(this, st);
        this.shader = shader;
        this.matrix = matrix;
        return Object.seal(this);
      }
      get shader() { return graphicsShaderFillMap.get(this).shader; }
      set shader(v) {
        if (v !== null && v !== undefined && !globalShaderStates.has(v)) {
          throw new TypeError('GraphicsShaderFill.shader requires a Shader or null.');
        }
        graphicsShaderFillMap.get(this).shader = v ?? null;
      }
      get matrix() { return graphicsShaderFillMap.get(this).matrix; }
      set matrix(v) {
        if (v !== null && v !== undefined && !geometry?.Matrix?.__nfIs?.(v)) {
          throw new TypeError('GraphicsShaderFill.matrix requires a flash.geom.Matrix or null.');
        }
        graphicsShaderFillMap.get(this).matrix = v ?? null;
      }
    }
    tagType(GraphicsShaderFill, 'GraphicsShaderFill', v => graphicsShaderFillMap.has(v));


    // =========================================================================
    // 2. Stage3D (`Stage3D`, `Context3D`, buffers, programs, textures)
    // =========================================================================

    const stage3dToken = Symbol('stage3d internal');
    const vertexBufferMap = new WeakMap();
    const indexBufferMap = new WeakMap();
    const program3dMap = new WeakMap();
    const textureBaseMap = new WeakMap();
    const context3dMap = new WeakMap();
    const stage3dMap = new WeakMap();

    class VertexBuffer3D {
      constructor(secret, info) {
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: VertexBuffer3D$ class cannot be instantiated.', 2012);
        }
        vertexBufferMap.set(this, info);
      }
      uploadFromByteArray(data, byteArrayOffset, startVertex, numVertices) {
        const st = vertexBufferMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: VertexBuffer3D is disposed.');
        const sv = Number(startVertex) | 0;
        const nv = Number(numVertices) | 0;
        if (sv < 0 || nv < 0) throw rangeError('RangeError: VertexBuffer3D upload range is negative.');
        const floats = extractFloatsFromByteArray(data, byteArrayOffset, nv * st.data32PerVertex);
        kcall(() => kernel.upload_vertex_buffer(st.id, floats, sv, nv));
      }
      uploadFromVector(data, startVertex, numVertices) {
        const st = vertexBufferMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: VertexBuffer3D is disposed.');
        if (!data || typeof data.length !== 'number') {
          throw new TypeError('VertexBuffer3D.uploadFromVector requires a Vector.<Number>.');
        }
        const sv = Number(startVertex) | 0;
        const nv = Number(numVertices) | 0;
        const floats = new Float32Array( data.length );
        for (let i = 0; i < data.length; i++) floats[i] = Number(data[i]) || 0;
        kcall(() => kernel.upload_vertex_buffer(st.id, floats, sv, nv));
      }
      dispose() {
        const st = vertexBufferMap.get(this);
        if (st && !st.disposed) {
          st.disposed = true;
          kernel.dispose_vertex_buffer(st.id);
        }
      }
    }
    tagType(VertexBuffer3D, 'VertexBuffer3D', v => vertexBufferMap.has(v));

    class IndexBuffer3D {
      constructor(secret, info) {
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: IndexBuffer3D$ class cannot be instantiated.', 2012);
        }
        indexBufferMap.set(this, info);
      }
      uploadFromByteArray(data, byteArrayOffset, startOffset, count) {
        const st = indexBufferMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: IndexBuffer3D is disposed.');
        const so = Number(startOffset) | 0;
        const cnt = Number(count) | 0;
        if (so < 0 || cnt < 0) throw rangeError('RangeError: IndexBuffer3D upload range is negative.');
        const slice = readByteArraySlice(data, byteArrayOffset, cnt * 2);
        const view = new DataView(slice.buffer, slice.byteOffset, slice.byteLength);
        const little = data.endian === 'littleEndian';
        const indices = new Uint16Array(cnt);
        for (let i = 0; i < cnt; i++) indices[i] = view.getUint16(i * 2, little);
        kcall(() => kernel.upload_index_buffer(st.id, indices, so, cnt));
      }
      uploadFromVector(data, startOffset, count) {
        const st = indexBufferMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: IndexBuffer3D is disposed.');
        if (!data || typeof data.length !== 'number') {
          throw new TypeError('IndexBuffer3D.uploadFromVector requires a Vector.<uint>.');
        }
        const so = Number(startOffset) | 0;
        const cnt = Number(count) | 0;
        const indices = new Uint16Array(data.length);
        for (let i = 0; i < data.length; i++) indices[i] = (Number(data[i]) >>> 0) & 0xffff;
        kcall(() => kernel.upload_index_buffer(st.id, indices, so, cnt));
      }
      dispose() {
        const st = indexBufferMap.get(this);
        if (st && !st.disposed) {
          st.disposed = true;
          kernel.dispose_index_buffer(st.id);
        }
      }
    }
    tagType(IndexBuffer3D, 'IndexBuffer3D', v => indexBufferMap.has(v));

    class Program3D {
      constructor(secret, info) {
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: Program3D$ class cannot be instantiated.', 2012);
        }
        program3dMap.set(this, info);
      }
      upload(vertexProgram, fragmentProgram) {
        const st = program3dMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: Program3D is disposed.');
        const vs = readByteArraySlice(vertexProgram, 0);
        const fs = readByteArraySlice(fragmentProgram, 0);
        kcall(() => kernel.upload_program(st.id, vs, fs));
      }
      dispose() {
        const st = program3dMap.get(this);
        if (st && !st.disposed) {
          st.disposed = true;
          kernel.dispose_program(st.id);
        }
      }
    }
    tagType(Program3D, 'Program3D', v => program3dMap.has(v));

    class TextureBase extends events.EventDispatcher {
      constructor(secret, info) {
        super();
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: TextureBase$ class cannot be instantiated.', 2012);
        }
        textureBaseMap.set(this, info);
      }
      dispose() {
        const st = textureBaseMap.get(this);
        if (st && !st.disposed) {
          st.disposed = true;
          kernel.dispose_texture(st.id);
        }
      }
    }
    tagType(TextureBase, 'TextureBase', v => textureBaseMap.has(v));

    function uploadTextureFromBitmapDataInternal(tex, source, side, miplevel) {
      const st = textureBaseMap.get(tex);
      if (!st || st.disposed) throw illegalOpError('IllegalOperationError: Texture is disposed.');
      const bmpHost = getBitmap();
      if (!bmpHost?.BitmapData?.__nfIs?.(source)) {
        throw new TypeError('Texture uploadFromBitmapData requires a non-null BitmapData.');
      }
      const pref = JSON.stringify(bmpHost.refOf(source));
      const rgba = player.bitmap_pixels(pref);
      const mip = Number(miplevel) >>> 0;
      const expectedW = Math.max(1, st.width >>> mip);
      const expectedH = Math.max(1, st.height >>> mip);
      if (source.width < expectedW || source.height < expectedH) {
        throw argError('ArgumentError: BitmapData dimensions are smaller than the target mip level.');
      }
      let slice = rgba;
      if (source.width !== expectedW || source.height !== expectedH) {
        slice = new Uint8Array(expectedW * expectedH * 4);
        for (let y = 0; y < expectedH; y++) {
          slice.set(rgba.subarray(y * source.width * 4, y * source.width * 4 + expectedW * 4), y * expectedW * 4);
        }
      }
      kcall(() => kernel.upload_texture_rgba(st.id, side >>> 0, mip, slice));
    }

    function uploadTextureFromByteArrayInternal(tex, data, byteArrayOffset, side, miplevel) {
      const st = textureBaseMap.get(tex);
      if (!st || st.disposed) throw illegalOpError('IllegalOperationError: Texture is disposed.');
      const mip = Number(miplevel) >>> 0;
      const expectedW = Math.max(1, st.width >>> mip);
      const expectedH = Math.max(1, st.height >>> mip);
      const byteLen = expectedW * expectedH * 4;
      const bgra = readByteArraySlice(data, byteArrayOffset, byteLen);
      // Convert BGRA (Flash Texture.uploadFromByteArray format) to RGBA
      const rgba = new Uint8Array(byteLen);
      for (let i = 0; i < byteLen; i += 4) {
        rgba[i] = bgra[i + 2];
        rgba[i + 1] = bgra[i + 1];
        rgba[i + 2] = bgra[i];
        rgba[i + 3] = bgra[i + 3];
      }
      kcall(() => kernel.upload_texture_rgba(st.id, side >>> 0, mip, rgba));
    }

    class Texture extends TextureBase {
      constructor(secret, info) {
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: Texture$ class cannot be instantiated.', 2012);
        }
        super(stage3dToken, info);
      }
      uploadFromBitmapData(source, miplevel = 0) {
        uploadTextureFromBitmapDataInternal(this, source, 0, miplevel);
      }
      uploadFromByteArray(data, byteArrayOffset, miplevel = 0) {
        uploadTextureFromByteArrayInternal(this, data, byteArrayOffset, 0, miplevel);
      }
      uploadCompressedTextureFromByteArray(data, byteArrayOffset, async = false) {
        const st = textureBaseMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: Texture is disposed.');
        const bytes = readByteArraySlice(data, 0);
        const off = Number(byteArrayOffset) >>> 0;
        kcall(() => kernel.upload_compressed_texture(st.id, bytes, off));
        if (Boolean(async)) {
          const myGen = generation;
          schedule(() => {
            if (disposed || myGen !== generation || st.disposed) return;
            try {
              this.dispatchEvent(new events.Event('textureReady'));
              afterEvent();
            } catch (err) {
              onError(err);
            }
          });
        }
      }
    }
    tagType(Texture, 'Texture', v => textureBaseMap.get(v)?.kind === '2d');

    class CubeTexture extends TextureBase {
      constructor(secret, info) {
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: CubeTexture$ class cannot be instantiated.', 2012);
        }
        super(stage3dToken, info);
      }
      uploadFromBitmapData(source, side, miplevel = 0) {
        const s = Number(side) >>> 0;
        if (s > 5) throw argError('ArgumentError: CubeTexture side must be 0..5.');
        uploadTextureFromBitmapDataInternal(this, source, s, miplevel);
      }
      uploadFromByteArray(data, byteArrayOffset, side, miplevel = 0) {
        const s = Number(side) >>> 0;
        if (s > 5) throw argError('ArgumentError: CubeTexture side must be 0..5.');
        uploadTextureFromByteArrayInternal(this, data, byteArrayOffset, s, miplevel);
      }
      uploadCompressedTextureFromByteArray(data, byteArrayOffset, async = false) {
        const st = textureBaseMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: CubeTexture is disposed.');
        const bytes = readByteArraySlice(data, 0);
        const off = Number(byteArrayOffset) >>> 0;
        kcall(() => kernel.upload_compressed_texture(st.id, bytes, off));
        if (Boolean(async)) {
          const myGen = generation;
          schedule(() => {
            if (disposed || myGen !== generation || st.disposed) return;
            try {
              this.dispatchEvent(new events.Event('textureReady'));
              afterEvent();
            } catch (err) {
              onError(err);
            }
          });
        }
      }
    }
    tagType(CubeTexture, 'CubeTexture', v => textureBaseMap.get(v)?.kind === 'cube');

    class RectangleTexture extends TextureBase {
      constructor(secret, info) {
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: RectangleTexture$ class cannot be instantiated.', 2012);
        }
        super(stage3dToken, info);
      }
      uploadFromBitmapData(source) {
        uploadTextureFromBitmapDataInternal(this, source, 0, 0);
      }
      uploadFromByteArray(data, byteArrayOffset) {
        uploadTextureFromByteArrayInternal(this, data, byteArrayOffset, 0, 0);
      }
    }
    tagType(RectangleTexture, 'RectangleTexture', v => textureBaseMap.get(v)?.kind === 'rectangle');

    class VideoTexture extends TextureBase {
      constructor(secret, info) {
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: VideoTexture$ class cannot be instantiated.', 2012);
        }
        super(stage3dToken, info);
      }
      get videoWidth() { return textureBaseMap.get(this).videoWidth || 0; }
      get videoHeight() { return textureBaseMap.get(this).videoHeight || 0; }
      attachCamera(theCamera) {
        const st = textureBaseMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: VideoTexture is disposed.');
        st.camera = theCamera ?? null;
        st.netStream = null;
        if (theCamera) {
          st.videoWidth = Number(theCamera.width) > 0 ? Number(theCamera.width) | 0 : 320;
          st.videoHeight = Number(theCamera.height) > 0 ? Number(theCamera.height) | 0 : 240;
          const myGen = generation;
          schedule(() => {
            if (disposed || myGen !== generation || st.disposed) return;
            try {
              this.dispatchEvent(new events.Event('textureReady'));
              afterEvent();
            } catch (err) {
              onError(err);
            }
          });
        }
      }
      attachNetStream(netStream) {
        const st = textureBaseMap.get(this);
        if (!st || st.disposed) throw illegalOpError('IllegalOperationError: VideoTexture is disposed.');
        st.netStream = netStream ?? null;
        st.camera = null;
        if (netStream) {
          st.videoWidth = 320;
          st.videoHeight = 240;
          const myGen = generation;
          schedule(() => {
            if (disposed || myGen !== generation || st.disposed) return;
            try {
              this.dispatchEvent(new events.Event('textureReady'));
              afterEvent();
            } catch (err) {
              onError(err);
            }
          });
        }
      }
    }
    tagType(VideoTexture, 'VideoTexture', v => textureBaseMap.get(v)?.kind === 'video');

    function requireContext3D(ctx) {
      const st = context3dMap.get(ctx);
      if (!st) throw new TypeError('Invalid Context3D receiver.');
      if (st.disposed) throw illegalOpError('IllegalOperationError: Context3D is disposed.');
      return st;
    }

    class Context3D extends events.EventDispatcher {
      constructor(secret, info) {
        super();
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: Context3D$ class cannot be instantiated.', 2012);
        }
        context3dMap.set(this, info);
      }
      static get supportsVideoTexture() { return true; }
      static get SupportsVideoTexture() { return true; }
      get backBufferWidth() { return context3dMap.get(this).backBufferWidth; }
      get backBufferHeight() { return context3dMap.get(this).backBufferHeight; }
      get maxBackBufferWidth() { return context3dMap.get(this).maxBackBufferWidth; }
      set maxBackBufferWidth(v) {
        const st = requireContext3D(this);
        const n = Number(v) | 0;
        if (n < 50 || n > 16384) throw argError('ArgumentError: maxBackBufferWidth out of range.');
        st.maxBackBufferWidth = n;
      }
      get maxBackBufferHeight() { return context3dMap.get(this).maxBackBufferHeight; }
      set maxBackBufferHeight(v) {
        const st = requireContext3D(this);
        const n = Number(v) | 0;
        if (n < 50 || n > 16384) throw argError('ArgumentError: maxBackBufferHeight out of range.');
        st.maxBackBufferHeight = n;
      }
      get driverInfo() {
        const st = context3dMap.get(this);
        if (st.disposed) return 'Disposed';
        return st.renderMode === 'software' ? 'Software Hw_disabled=user' : 'NeoFlash Stage3D Software/WebGL Rasterizer';
      }
      get enableErrorChecking() { return context3dMap.get(this).enableErrorChecking; }
      set enableErrorChecking(v) {
        const st = requireContext3D(this);
        st.enableErrorChecking = Boolean(v);
      }
      get profile() { return context3dMap.get(this).profile; }
      get totalGPUMemory() {
        const st = context3dMap.get(this);
        if (st.disposed) return 0;
        return kernel.total_gpu_memory(st.id);
      }
      configureBackBuffer(width, height, antiAlias, enableDepthAndStencil = true, wantsBestResolution = false, wantsBestResolutionOnBrowserZoom = false) {
        void wantsBestResolution;
        void wantsBestResolutionOnBrowserZoom;
        const st = requireContext3D(this);
        const w = Number(width) | 0;
        const h = Number(height) | 0;
        const aa = Number(antiAlias) | 0;
        const ds = Boolean(enableDepthAndStencil);
        if (w > st.maxBackBufferWidth || h > st.maxBackBufferHeight) {
          throw argError('ArgumentError: BackBuffer dimensions exceed maxBackBufferWidth/maxBackBufferHeight.');
        }
        kcall(() => kernel.configure_back_buffer(st.id, w, h, aa, ds));
        st.backBufferWidth = w;
        st.backBufferHeight = h;
      }
      clear(red = 0.0, green = 0.0, blue = 0.0, alpha = 1.0, depth = 1.0, stencil = 0, mask = 0xffffffff) {
        const st = requireContext3D(this);
        kcall(() => kernel.clear(st.id, Number(red), Number(green), Number(blue), Number(alpha), Number(depth), Number(stencil) >>> 0, Number(mask) >>> 0));
      }
      createVertexBuffer(numVertices, data32PerVertex, bufferUsage = 'staticDraw') {
        const st = requireContext3D(this);
        const nv = Number(numVertices) | 0;
        const d32 = Number(data32PerVertex) | 0;
        const usage = String(bufferUsage);
        const id = kcall(() => kernel.create_vertex_buffer(st.id, nv, d32, usage));
        return new VertexBuffer3D(stage3dToken, { id, ctxId: st.id, numVertices: nv, data32PerVertex: d32, usage, disposed: false });
      }
      createIndexBuffer(numIndices, bufferUsage = 'staticDraw') {
        const st = requireContext3D(this);
        const ni = Number(numIndices) | 0;
        const usage = String(bufferUsage);
        const id = kcall(() => kernel.create_index_buffer(st.id, ni, usage));
        return new IndexBuffer3D(stage3dToken, { id, ctxId: st.id, numIndices: ni, usage, disposed: false });
      }
      createProgram() {
        const st = requireContext3D(this);
        const id = kcall(() => kernel.create_program(st.id));
        return new Program3D(stage3dToken, { id, ctxId: st.id, disposed: false });
      }
      createTexture(width, height, format, optimizeForRenderToTexture, streamingLevels = 0) {
        const st = requireContext3D(this);
        const w = Number(width) | 0;
        const h = Number(height) | 0;
        const fmt = String(format);
        const rtt = Boolean(optimizeForRenderToTexture);
        const sl = Number(streamingLevels) | 0;
        const id = kcall(() => kernel.create_texture(st.id, '2d', w, h, fmt, rtt, sl));
        return new Texture(stage3dToken, { id, ctxId: st.id, kind: '2d', width: w, height: h, format: fmt, rtt, disposed: false });
      }
      createCubeTexture(size, format, optimizeForRenderToTexture, streamingLevels = 0) {
        const st = requireContext3D(this);
        const sz = Number(size) | 0;
        const fmt = String(format);
        const rtt = Boolean(optimizeForRenderToTexture);
        const sl = Number(streamingLevels) | 0;
        const id = kcall(() => kernel.create_texture(st.id, 'cube', sz, sz, fmt, rtt, sl));
        return new CubeTexture(stage3dToken, { id, ctxId: st.id, kind: 'cube', width: sz, height: sz, format: fmt, rtt, disposed: false });
      }
      createRectangleTexture(width, height, format, optimizeForRenderToTexture) {
        const st = requireContext3D(this);
        const w = Number(width) | 0;
        const h = Number(height) | 0;
        const fmt = String(format);
        const rtt = Boolean(optimizeForRenderToTexture);
        const id = kcall(() => kernel.create_texture(st.id, 'rectangle', w, h, fmt, rtt, 0));
        return new RectangleTexture(stage3dToken, { id, ctxId: st.id, kind: 'rectangle', width: w, height: h, format: fmt, rtt, disposed: false });
      }
      createVideoTexture() {
        const st = requireContext3D(this);
        const id = kcall(() => kernel.create_texture(st.id, 'video', 320, 240, 'bgra', false, 0));
        return new VideoTexture(stage3dToken, { id, ctxId: st.id, kind: 'video', width: 320, height: 240, format: 'bgra', rtt: false, videoWidth: 320, videoHeight: 240, disposed: false });
      }
      setProgram(program) {
        const st = requireContext3D(this);
        let pid = null;
        if (program !== null && program !== undefined) {
          const pst = program3dMap.get(program);
          if (!pst) throw new TypeError('Context3D.setProgram requires a Program3D or null.');
          pid = pst.id;
        }
        kcall(() => kernel.set_state_json(st.id, 'setProgram', JSON.stringify({ programId: pid })));
      }
      setProgramConstantsFromVector(programType, firstRegister, data, numRegisters = -1) {
        const st = requireContext3D(this);
        if (!data || typeof data.length !== 'number') {
          throw new TypeError('Context3D.setProgramConstantsFromVector requires a Vector.<Number>.');
        }
        const values = [];
        for (let i = 0; i < data.length; i++) values.push(Number(data[i]) || 0);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setConstants',
            JSON.stringify({
              programType: String(programType),
              firstRegister: Number(firstRegister) | 0,
              values,
              numRegisters: Number(numRegisters) | 0,
            })
          )
        );
      }
      setProgramConstantsFromMatrix(programType, firstRegister, matrix, transposedMatrix = false) {
        const st = requireContext3D(this);
        if (!geometry3d?.Matrix3D?.__nfIs?.(matrix)) {
          throw new TypeError('Context3D.setProgramConstantsFromMatrix requires a flash.geom.Matrix3D.');
        }
        const raw = Array.from(matrix.rawData, Number);
        const values = Boolean(transposedMatrix)
          ? [
              raw[0], raw[4], raw[8], raw[12],
              raw[1], raw[5], raw[9], raw[13],
              raw[2], raw[6], raw[10], raw[14],
              raw[3], raw[7], raw[11], raw[15],
            ]
          : raw;
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setConstants',
            JSON.stringify({
              programType: String(programType),
              firstRegister: Number(firstRegister) | 0,
              values,
              numRegisters: 4,
            })
          )
        );
      }
      setProgramConstantsFromByteArray(programType, firstRegister, numRegisters, data, byteArrayOffset) {
        const st = requireContext3D(this);
        const nr = Number(numRegisters) | 0;
        if (nr < 0) throw rangeError('RangeError: numRegisters must be non-negative.');
        const floats = Array.from(extractFloatsFromByteArray(data, byteArrayOffset, nr * 4));
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setConstants',
            JSON.stringify({
              programType: String(programType),
              firstRegister: Number(firstRegister) | 0,
              values: floats,
              numRegisters: nr,
            })
          )
        );
      }
      setVertexBufferAt(index, buffer, bufferOffset = 0, format = 'float4') {
        const st = requireContext3D(this);
        let bid = null;
        if (buffer !== null && buffer !== undefined) {
          const bst = vertexBufferMap.get(buffer);
          if (!bst) throw new TypeError('Context3D.setVertexBufferAt requires a VertexBuffer3D or null.');
          bid = bst.id;
        }
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setVertexBufferAt',
            JSON.stringify({
              index: Number(index) | 0,
              bufferId: bid,
              bufferOffset: Number(bufferOffset) | 0,
              format: String(format),
            })
          )
        );
      }
      setTextureAt(sampler, texture) {
        const st = requireContext3D(this);
        let tid = null;
        if (texture !== null && texture !== undefined) {
          const tst = textureBaseMap.get(texture);
          if (!tst) throw new TypeError('Context3D.setTextureAt requires a TextureBase or null.');
          tid = tst.id;
        }
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setTextureAt',
            JSON.stringify({ sampler: Number(sampler) | 0, textureId: tid })
          )
        );
      }
      setSamplerStateAt(sampler, wrap, filter, mipfilter) {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setSamplerStateAt',
            JSON.stringify({
              sampler: Number(sampler) | 0,
              wrap: String(wrap),
              filter: String(filter),
              mipfilter: String(mipfilter),
            })
          )
        );
      }
      setBlendFactors(sourceFactor, destinationFactor) {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setBlendFactors',
            JSON.stringify({
              source: String(sourceFactor),
              destination: String(destinationFactor),
            })
          )
        );
      }
      setColorMask(red, green, blue, alpha) {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setColorMask',
            JSON.stringify({
              r: Boolean(red),
              g: Boolean(green),
              b: Boolean(blue),
              a: Boolean(alpha),
            })
          )
        );
      }
      setCulling(triangleFaceToCull) {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setCulling',
            JSON.stringify({ face: String(triangleFaceToCull) })
          )
        );
      }
      setDepthTest(depthMask, passCompareMode) {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setDepthTest',
            JSON.stringify({
              depthMask: Boolean(depthMask),
              compareMode: String(passCompareMode),
            })
          )
        );
      }
      setFillMode(fillMode) {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(st.id, 'setFillMode', JSON.stringify({ fillMode: String(fillMode) }))
        );
      }
      setScissorRectangle(rectangle) {
        const st = requireContext3D(this);
        let rect = null;
        if (rectangle !== null && rectangle !== undefined) {
          if (!geometry?.Rectangle?.__nfIs?.(rectangle)) {
            throw new TypeError('Context3D.setScissorRectangle requires a flash.geom.Rectangle or null.');
          }
          rect = [
            Math.round(rectangle.x) | 0,
            Math.round(rectangle.y) | 0,
            Math.round(rectangle.width) | 0,
            Math.round(rectangle.height) | 0,
          ];
        }
        kcall(() =>
          kernel.set_state_json(st.id, 'setScissorRectangle', JSON.stringify({ rect }))
        );
      }
      setStencilActions(triangleFace = 'frontAndBack', compareMode = 'always', actionOnBothPass = 'keep', actionOnDepthFail = 'keep', actionOnDepthPassStencilFail = 'keep') {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setStencilActions',
            JSON.stringify({
              triangleFace: String(triangleFace),
              compareMode: String(compareMode),
              actionOnBothPass: String(actionOnBothPass),
              actionOnDepthFail: String(actionOnDepthFail),
              actionOnDepthPassStencilFail: String(actionOnDepthPassStencilFail),
            })
          )
        );
      }
      setStencilReferenceValue(referenceValue, readMask = 255, writeMask = 255) {
        const st = requireContext3D(this);
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setStencilReferenceValue',
            JSON.stringify({
              referenceValue: Number(referenceValue) >>> 0,
              readMask: Number(readMask) >>> 0,
              writeMask: Number(writeMask) >>> 0,
            })
          )
        );
      }
      setRenderToTexture(texture, enableDepthAndStencil = false, antiAlias = 0, surfaceSelector = 0, colorOutputIndex = 0) {
        const st = requireContext3D(this);
        const tst = textureBaseMap.get(texture);
        if (!tst) throw new TypeError('Context3D.setRenderToTexture requires a TextureBase.');
        kcall(() =>
          kernel.set_state_json(
            st.id,
            'setRenderToTexture',
            JSON.stringify({
              textureId: tst.id,
              enableDepthAndStencil: Boolean(enableDepthAndStencil),
              antiAlias: Number(antiAlias) | 0,
              surfaceSelector: Number(surfaceSelector) | 0,
              colorOutputIndex: Number(colorOutputIndex) | 0,
            })
          )
        );
      }
      setRenderToBackBuffer() {
        const st = requireContext3D(this);
        kcall(() => kernel.set_state_json(st.id, 'setRenderToBackBuffer', '{}'));
      }
      drawTriangles(indexBuffer, firstIndex = 0, numTriangles = -1) {
        const st = requireContext3D(this);
        const ibSt = indexBufferMap.get(indexBuffer);
        if (!ibSt) throw new TypeError('Context3D.drawTriangles requires a non-null IndexBuffer3D.');
        kcall(() =>
          kernel.draw_triangles(st.id, ibSt.id, Number(firstIndex) | 0, Number(numTriangles) | 0)
        );
      }
      drawToBitmapData(destination) {
        const st = requireContext3D(this);
        const bmpHost = getBitmap();
        if (!bmpHost?.BitmapData?.__nfIs?.(destination)) {
          throw new TypeError('Context3D.drawToBitmapData requires a non-null BitmapData.');
        }
        const w = kcall(() => kernel.backbuffer_width(st.id));
        const h = kcall(() => kernel.backbuffer_height(st.id));
        const rgba = kcall(() => kernel.backbuffer_rgba(st.id));
        writeRgbaToBitmapData(destination, w, h, rgba);
      }
      present() {
        const st = requireContext3D(this);
        kcall(() => kernel.present(st.id));
      }
      dispose(recreate = true) {
        const st = context3dMap.get(this);
        if (!st || st.disposed) return;
        st.disposed = true;
        st.backBufferWidth = 0;
        st.backBufferHeight = 0;
        kernel.dispose_context(st.id);
        if (st.stage3d) {
          const s3dSt = stage3dMap.get(st.stage3d);
          if (s3dSt && s3dSt.context3D === this) {
            s3dSt.context3D = null;
            if (Boolean(recreate)) {
              st.stage3d.requestContext3D(st.renderMode, st.profile);
            }
          }
        }
      }
    }
    tagType(Context3D, 'Context3D', v => context3dMap.has(v));

    class Stage3D extends events.EventDispatcher {
      constructor(secret, slotIndex) {
        super();
        if (secret !== stage3dToken) {
          throw argError('ArgumentError: Error #2012: Stage3D$ class cannot be instantiated.', 2012);
        }
        stage3dMap.set(this, {
          slotIndex,
          context3D: null,
          visible: true,
          x: 0,
          y: 0,
          reqToken: 0,
        });
      }
      get context3D() { return stage3dMap.get(this).context3D; }
      get visible() { return stage3dMap.get(this).visible; }
      set visible(v) { stage3dMap.get(this).visible = Boolean(v); }
      get x() { return stage3dMap.get(this).x; }
      set x(v) {
        const n = Number(v);
        if (!Number.isFinite(n)) throw argError('ArgumentError: Stage3D.x must be finite.');
        stage3dMap.get(this).x = n;
      }
      get y() { return stage3dMap.get(this).y; }
      set y(v) {
        const n = Number(v);
        if (!Number.isFinite(n)) throw argError('ArgumentError: Stage3D.y must be finite.');
        stage3dMap.get(this).y = n;
      }
      requestContext3D(context3DRenderMode = 'auto', profile = 'baseline') {
        const st = stage3dMap.get(this);
        const rm = String(context3DRenderMode);
        const prof = String(profile);
        kcall(() => kernel.validate_stage3d_enum('renderMode', rm));
        kcall(() => kernel.validate_stage3d_enum('profile', prof));
        const myGen = generation;
        const tok = ++st.reqToken;
        if (st.context3D) {
          const oldSt = context3dMap.get(st.context3D);
          if (oldSt && !oldSt.disposed) {
            oldSt.disposed = true;
            kernel.dispose_context(oldSt.id);
          }
        }
        const ctxId = kcall(() => kernel.create_context(rm, prof));
        const maxDim = prof.endsWith('Extended') || prof.startsWith('standard') ? 4096 : 2048;
        const ctx = new Context3D(stage3dToken, {
          id: ctxId,
          stage3d: this,
          renderMode: rm,
          profile: prof,
          enableErrorChecking: false,
          backBufferWidth: 0,
          backBufferHeight: 0,
          maxBackBufferWidth: maxDim,
          maxBackBufferHeight: maxDim,
          disposed: false,
        });
        st.context3D = ctx;
        if (this.hasEventListener('context3DCreate')) {
          this.dispatchEvent(new events.Event('context3DCreate'));
          afterEvent();
        } else {
          schedule(() => {
            if (disposed || myGen !== generation || st.reqToken !== tok) return;
            try {
              if (this.hasEventListener('context3DCreate')) {
                this.dispatchEvent(new events.Event('context3DCreate'));
                afterEvent();
              }
            } catch (err) {
              onError(err);
            }
          });
        }
      }
      requestContext3DMatchingProfiles(profiles) {
        if (!profiles || typeof profiles.length !== 'number' || profiles.length === 0) {
          throw argError('ArgumentError: requestContext3DMatchingProfiles requires a non-empty Vector.<String>.');
        }
        let chosen = null;
        for (let i = 0; i < profiles.length; i++) {
          const p = String(profiles[i]);
          kcall(() => kernel.validate_stage3d_enum('profile', p));
          chosen = p;
        }
        this.requestContext3D('auto', chosen || 'baseline');
      }
    }
    tagType(Stage3D, 'Stage3D', v => stage3dMap.has(v));

    let stage3DList = null;
    let stageVideoList = null;

    function getStage3Ds() {
      if (!stage3DList) {
        stage3DList = [
          new Stage3D(stage3dToken, 0),
          new Stage3D(stage3dToken, 1),
          new Stage3D(stage3dToken, 2),
          new Stage3D(stage3dToken, 3),
        ];
      }
      return makeVector('flash.display.Stage3D', stage3DList, true);
    }

    function getStageVideos() {
      const SV = getBuiltin('flash.media.StageVideo') || FallbackStageVideo;
      if (!stageVideoList) {
        stageVideoList = [new SV(), new SV(), new SV(), new SV()];
      }
      return makeVector('flash.media.StageVideo', stageVideoList, true);
    }


    // =========================================================================
    // 3. Flash Text Engine (`flash.text.engine.*`) & IME (`flash.text.ime.*`)
    // =========================================================================

    const fteToken = Symbol('fte internal');
    const fontMetricsMap = new WeakMap();
    const fontDescMap = new WeakMap();
    const elemFormatMap = new WeakMap();
    const contentElemMap = new WeakMap();
    const justifierMap = new WeakMap();
    const tabStopMap = new WeakMap();
    const mirrorRegionMap = new WeakMap();
    const textBlockMap = new WeakMap();
    const textLineStateMap = new WeakMap();
    let nextElementId = 1;

    class FontMetrics {
      constructor(
        emBox = null,
        strikethroughOffset = 0,
        strikethroughThickness = 1,
        underlineOffset = 0,
        underlineThickness = 1,
        subscriptOffset = 0,
        subscriptScale = 1,
        superscriptOffset = 0,
        superscriptScale = 1,
        lineGap = 0
      ) {
        if (new.target !== FontMetrics) {
          throw new TypeError('FontMetrics is final and cannot be subclassed.');
        }
        const R = geometry?.Rectangle;
        const box =
          emBox && R?.__nfIs?.(emBox)
            ? new R(emBox.x, emBox.y, emBox.width, emBox.height)
            : R
            ? new R(0, -10, 12, 12)
            : { x: 0, y: -10, width: 12, height: 12 };
        fontMetricsMap.set(this, {
          emBox: box,
          strikethroughOffset: Number(strikethroughOffset) || 0,
          strikethroughThickness: Number(strikethroughThickness) || 0,
          underlineOffset: Number(underlineOffset) || 0,
          underlineThickness: Number(underlineThickness) || 0,
          subscriptOffset: Number(subscriptOffset) || 0,
          subscriptScale: Number(subscriptScale) || 0,
          superscriptOffset: Number(superscriptOffset) || 0,
          superscriptScale: Number(superscriptScale) || 0,
          lineGap: Number(lineGap) || 0,
        });
        return Object.seal(this);
      }
    }
    for (const prop of [
      'emBox',
      'strikethroughOffset',
      'strikethroughThickness',
      'underlineOffset',
      'underlineThickness',
      'subscriptOffset',
      'subscriptScale',
      'superscriptOffset',
      'superscriptScale',
      'lineGap',
    ]) {
      Object.defineProperty(FontMetrics.prototype, prop, {
        enumerable: true,
        get() { return fontMetricsMap.get(this)[prop]; },
        set(v) { fontMetricsMap.get(this)[prop] = prop === 'emBox' ? v : Number(v); },
      });
    }
    tagType(FontMetrics, 'FontMetrics', v => fontMetricsMap.has(v));

    class FontDescription {
      constructor(
        fontName = '_serif',
        fontWeight = 'normal',
        fontPosture = 'normal',
        fontLookup = 'device',
        renderingMode = 'cff',
        cffHinting = 'horizontalStem'
      ) {
        if (new.target !== FontDescription) {
          throw new TypeError('FontDescription is final and cannot be subclassed.');
        }
        const st = {
          fontName: fontName === null || fontName === undefined ? '_serif' : String(fontName),
          fontWeight: String(fontWeight),
          fontPosture: String(fontPosture),
          fontLookup: String(fontLookup),
          renderingMode: String(renderingMode),
          cffHinting: String(cffHinting),
          locked: false,
        };
        kcall(() => kernel.validate_fte_enum('fontWeight', st.fontWeight));
        kcall(() => kernel.validate_fte_enum('fontPosture', st.fontPosture));
        kcall(() => kernel.validate_fte_enum('fontLookup', st.fontLookup));
        kcall(() => kernel.validate_fte_enum('renderingMode', st.renderingMode));
        kcall(() => kernel.validate_fte_enum('cffHinting', st.cffHinting));
        fontDescMap.set(this, st);
        return Object.seal(this);
      }
      static isFontCompatible(fontName, fontWeight, fontPosture) {
        if (fontName === null || fontName === undefined) return false;
        const w = String(fontWeight);
        const p = String(fontPosture);
        kcall(() => kernel.validate_fte_enum('fontWeight', w));
        kcall(() => kernel.validate_fte_enum('fontPosture', p));
        return kernel.is_font_compatible(String(fontName), w, p);
      }
      static isDeviceFontCompatible(fontName, fontWeight, fontPosture) {
        return FontDescription.isFontCompatible(fontName, fontWeight, fontPosture);
      }
      get fontName() { return fontDescMap.get(this).fontName; }
      set fontName(v) {
        const st = fontDescMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: FontDescription is locked.');
        if (v === null || v === undefined) throw new TypeError('FontDescription.fontName must not be null.');
        st.fontName = String(v);
      }
      get fontWeight() { return fontDescMap.get(this).fontWeight; }
      set fontWeight(v) {
        const st = fontDescMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: FontDescription is locked.');
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('fontWeight', s));
        st.fontWeight = s;
      }
      get fontPosture() { return fontDescMap.get(this).fontPosture; }
      set fontPosture(v) {
        const st = fontDescMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: FontDescription is locked.');
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('fontPosture', s));
        st.fontPosture = s;
      }
      get fontLookup() { return fontDescMap.get(this).fontLookup; }
      set fontLookup(v) {
        const st = fontDescMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: FontDescription is locked.');
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('fontLookup', s));
        st.fontLookup = s;
      }
      get renderingMode() { return fontDescMap.get(this).renderingMode; }
      set renderingMode(v) {
        const st = fontDescMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: FontDescription is locked.');
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('renderingMode', s));
        st.renderingMode = s;
      }
      get cffHinting() { return fontDescMap.get(this).cffHinting; }
      set cffHinting(v) {
        const st = fontDescMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: FontDescription is locked.');
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('cffHinting', s));
        st.cffHinting = s;
      }
      get locked() { return fontDescMap.get(this).locked; }
      set locked(v) {
        const st = fontDescMap.get(this);
        if (st.locked && !v) throw illegalOpError('IllegalOperationError: FontDescription cannot be unlocked.');
        st.locked = Boolean(v);
      }
      clone() {
        const st = fontDescMap.get(this);
        return new FontDescription(
          st.fontName,
          st.fontWeight,
          st.fontPosture,
          st.fontLookup,
          st.renderingMode,
          st.cffHinting
        );
      }
    }
    tagType(FontDescription, 'FontDescription', v => fontDescMap.has(v));

    class ElementFormat {
      constructor(
        fontDescription = null,
        fontSize = 12.0,
        color = 0x000000,
        alpha = 1.0,
        textRotation = 'auto',
        dominantBaseline = 'roman',
        alignmentBaseline = 'useDominantBaseline',
        baselineShift = 0.0,
        kerning = 'on',
        trackingRight = 0.0,
        trackingLeft = 0.0,
        locale = 'en',
        breakOpportunity = 'auto',
        digitCase = 'default',
        digitWidth = 'default',
        ligatureLevel = 'common',
        typographicCase = 'default'
      ) {
        if (new.target !== ElementFormat) {
          throw new TypeError('ElementFormat is final and cannot be subclassed.');
        }
        const fd =
          fontDescription === null || fontDescription === undefined
            ? new FontDescription()
            : fontDescMap.has(fontDescription)
            ? fontDescription
            : (() => { throw new TypeError('ElementFormat.fontDescription requires a FontDescription.'); })();
        const fs = Number(fontSize);
        if (!Number.isFinite(fs) || fs < 0) throw argError('ArgumentError: ElementFormat.fontSize must be >= 0.');
        const st = {
          fontDescription: fd,
          fontSize: fs,
          color: (Number(color) >>> 0) & 0xffffff,
          alpha: Number(alpha),
          textRotation: String(textRotation),
          dominantBaseline: String(dominantBaseline),
          alignmentBaseline: String(alignmentBaseline),
          baselineShift: Number(baselineShift) || 0,
          kerning: String(kerning),
          trackingRight: Number(trackingRight) || 0,
          trackingLeft: Number(trackingLeft) || 0,
          locale: locale === null || locale === undefined ? 'en' : String(locale),
          breakOpportunity: String(breakOpportunity),
          digitCase: String(digitCase),
          digitWidth: String(digitWidth),
          ligatureLevel: String(ligatureLevel),
          typographicCase: String(typographicCase),
          locked: false,
        };
        kcall(() => kernel.validate_fte_enum('textRotation', st.textRotation));
        kcall(() => kernel.validate_fte_enum('textBaseline', st.dominantBaseline));
        kcall(() => kernel.validate_fte_enum('textBaseline', st.alignmentBaseline));
        kcall(() => kernel.validate_fte_enum('kerning', st.kerning));
        kcall(() => kernel.validate_fte_enum('breakOpportunity', st.breakOpportunity));
        kcall(() => kernel.validate_fte_enum('digitCase', st.digitCase));
        kcall(() => kernel.validate_fte_enum('digitWidth', st.digitWidth));
        kcall(() => kernel.validate_fte_enum('ligatureLevel', st.ligatureLevel));
        kcall(() => kernel.validate_fte_enum('typographicCase', st.typographicCase));
        elemFormatMap.set(this, st);
        return Object.seal(this);
      }
      get fontDescription() { return elemFormatMap.get(this).fontDescription; }
      set fontDescription(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        if (!fontDescMap.has(v)) throw new TypeError('ElementFormat.fontDescription requires a non-null FontDescription.');
        st.fontDescription = v;
      }
      get fontSize() { return elemFormatMap.get(this).fontSize; }
      set fontSize(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        const n = Number(v);
        if (!Number.isFinite(n) || n < 0) throw argError('ArgumentError: ElementFormat.fontSize must be >= 0.');
        st.fontSize = n;
      }
      get color() { return elemFormatMap.get(this).color; }
      set color(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        st.color = (Number(v) >>> 0) & 0xffffff;
      }
      get alpha() { return elemFormatMap.get(this).alpha; }
      set alpha(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        st.alpha = Number(v);
      }
      get baselineShift() { return elemFormatMap.get(this).baselineShift; }
      set baselineShift(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        st.baselineShift = Number(v);
      }
      get trackingRight() { return elemFormatMap.get(this).trackingRight; }
      set trackingRight(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        st.trackingRight = Number(v);
      }
      get trackingLeft() { return elemFormatMap.get(this).trackingLeft; }
      set trackingLeft(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        st.trackingLeft = Number(v);
      }
      get locale() { return elemFormatMap.get(this).locale; }
      set locale(v) {
        const st = elemFormatMap.get(this);
        if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
        if (v === null || v === undefined) throw new TypeError('ElementFormat.locale must not be null.');
        st.locale = String(v);
      }
      get locked() { return elemFormatMap.get(this).locked; }
      set locked(v) {
        const st = elemFormatMap.get(this);
        if (st.locked && !v) throw illegalOpError('IllegalOperationError: ElementFormat cannot be unlocked.');
        st.locked = Boolean(v);
        if (st.locked) st.fontDescription.locked = true;
      }
      getFontMetrics() {
        const st = elemFormatMap.get(this);
        const fd = fontDescMap.get(st.fontDescription);
        const raw = JSON.parse(
          kernel.compute_font_metrics_json(fd.fontName, st.fontSize, fd.fontWeight, fd.fontPosture)
        );
        const R = geometry?.Rectangle;
        const box = R
          ? new R(raw.em_box[0], raw.em_box[1], raw.em_box[2], raw.em_box[3])
          : { x: raw.em_box[0], y: raw.em_box[1], width: raw.em_box[2], height: raw.em_box[3] };
        return new FontMetrics(
          box,
          raw.strikethrough_offset,
          raw.strikethrough_thickness,
          raw.underline_offset,
          raw.underline_thickness,
          raw.subscript_offset,
          raw.subscript_scale,
          raw.superscript_offset,
          raw.superscript_scale,
          raw.line_gap
        );
      }
      clone() {
        const st = elemFormatMap.get(this);
        return new ElementFormat(
          st.fontDescription.locked ? st.fontDescription.clone() : st.fontDescription,
          st.fontSize,
          st.color,
          st.alpha,
          st.textRotation,
          st.dominantBaseline,
          st.alignmentBaseline,
          st.baselineShift,
          st.kerning,
          st.trackingRight,
          st.trackingLeft,
          st.locale,
          st.breakOpportunity,
          st.digitCase,
          st.digitWidth,
          st.ligatureLevel,
          st.typographicCase
        );
      }
    }
    for (const [prop, enumKind] of [
      ['textRotation', 'textRotation'],
      ['dominantBaseline', 'textBaseline'],
      ['alignmentBaseline', 'textBaseline'],
      ['kerning', 'kerning'],
      ['breakOpportunity', 'breakOpportunity'],
      ['digitCase', 'digitCase'],
      ['digitWidth', 'digitWidth'],
      ['ligatureLevel', 'ligatureLevel'],
      ['typographicCase', 'typographicCase'],
    ]) {
      Object.defineProperty(ElementFormat.prototype, prop, {
        enumerable: true,
        get() { return elemFormatMap.get(this)[prop]; },
        set(v) {
          const st = elemFormatMap.get(this);
          if (st.locked) throw illegalOpError('IllegalOperationError: ElementFormat is locked.');
          const s = String(v);
          kcall(() => kernel.validate_fte_enum(enumKind, s));
          st[prop] = s;
        },
      });
    }
    tagType(ElementFormat, 'ElementFormat', v => elemFormatMap.has(v));

    function markBlockDirty(block) {
      if (!block) return;
      const bst = textBlockMap.get(block);
      if (!bst) return;
      for (const line of bst.lines) {
        const lst = textLineStateMap.get(line);
        if (lst && lst.validity === 'valid') {
          lst.validity = 'invalid';
          if (!bst.firstInvalidLine) bst.firstInvalidLine = line;
        }
      }
    }

    function recomputeTreeIndices(rootElem, textBlock) {
      let cursor = 0;
      function walk(el, parentGroup) {
        const est = contentElemMap.get(el);
        est.groupElement = parentGroup;
        est.textBlock = textBlock;
        est.textBlockBeginIndex = textBlock ? cursor : -1;
        if (est.kind === 'group') {
          let raw = '';
          let txt = '';
          for (const child of est.elements) {
            walk(child, el);
            const cst = contentElemMap.get(child);
            raw += cst.rawText;
            if (cst.text !== null) txt += cst.text;
          }
          est.rawText = raw;
          est.text = txt;
        } else if (est.kind === 'graphic') {
          est.rawText = '\uFDEF';
          est.text = null;
          cursor += 1;
        } else {
          est.rawText = est.text || '';
          cursor += est.rawText.length;
        }
      }
      walk(rootElem, null);
    }

    function notifyHierarchyChange(el) {
      let cur = el;
      while (cur) {
        const st = contentElemMap.get(cur);
        if (!st) break;
        if (st.textBlock) {
          const bst = textBlockMap.get(st.textBlock);
          if (bst && bst.content) recomputeTreeIndices(bst.content, st.textBlock);
          markBlockDirty(st.textBlock);
          return;
        }
        if (!st.groupElement) {
          recomputeTreeIndices(cur, null);
          return;
        }
        cur = st.groupElement;
      }
    }

    class ContentElement {
      constructor(elementFormat = null, eventMirror = null, textRotation = 'rotate0') {
        if (new.target === ContentElement) {
          throw argError('ArgumentError: Error #2012: ContentElement$ class cannot be instantiated.', 2012);
        }
        if (elementFormat !== null && elementFormat !== undefined && !elemFormatMap.has(elementFormat)) {
          throw new TypeError('ContentElement.elementFormat requires an ElementFormat or null.');
        }
        if (eventMirror !== null && eventMirror !== undefined && !(eventMirror instanceof events.EventDispatcher)) {
          throw new TypeError('ContentElement.eventMirror requires an EventDispatcher or null.');
        }
        const rot = String(textRotation);
        kcall(() => kernel.validate_fte_enum('textRotation', rot));
        if (elementFormat) elementFormat.locked = true;
        contentElemMap.set(this, {
          id: nextElementId++,
          kind: 'base',
          elementFormat: elementFormat ?? null,
          eventMirror: eventMirror ?? null,
          textRotation: rot,
          groupElement: null,
          textBlock: null,
          textBlockBeginIndex: -1,
          rawText: '',
          text: '',
          userData: undefined,
        });
      }
      get elementFormat() { return contentElemMap.get(this).elementFormat; }
      set elementFormat(v) {
        if (v !== null && v !== undefined && !elemFormatMap.has(v)) {
          throw new TypeError('ContentElement.elementFormat requires an ElementFormat or null.');
        }
        if (v) v.locked = true;
        contentElemMap.get(this).elementFormat = v ?? null;
        notifyHierarchyChange(this);
      }
      get eventMirror() { return contentElemMap.get(this).eventMirror; }
      set eventMirror(v) {
        if (v !== null && v !== undefined && !(v instanceof events.EventDispatcher)) {
          throw new TypeError('ContentElement.eventMirror requires an EventDispatcher or null.');
        }
        contentElemMap.get(this).eventMirror = v ?? null;
        notifyHierarchyChange(this);
      }
      get groupElement() { return contentElemMap.get(this).groupElement; }
      get rawText() { return contentElemMap.get(this).rawText; }
      get text() { return contentElemMap.get(this).text; }
      get textBlock() { return contentElemMap.get(this).textBlock; }
      get textBlockBeginIndex() { return contentElemMap.get(this).textBlockBeginIndex; }
      get textRotation() { return contentElemMap.get(this).textRotation; }
      set textRotation(v) {
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('textRotation', s));
        contentElemMap.get(this).textRotation = s;
        notifyHierarchyChange(this);
      }
      get userData() { return contentElemMap.get(this).userData; }
      set userData(v) { contentElemMap.get(this).userData = v; }
    }
    Object.defineProperty(ContentElement, 'GRAPHIC_ELEMENT', { value: 0xFDEF, enumerable: true });
    tagType(ContentElement, 'ContentElement', v => contentElemMap.has(v));

    class TextElement extends ContentElement {
      constructor(text = null, elementFormat = null, eventMirror = null, textRotation = 'rotate0') {
        if (new.target !== TextElement) {
          throw new TypeError('TextElement is final and cannot be subclassed.');
        }
        super(elementFormat, eventMirror, textRotation);
        const st = contentElemMap.get(this);
        st.kind = 'text';
        st.text = text === null || text === undefined ? '' : String(text);
        st.rawText = st.text;
        return Object.seal(this);
      }
      get text() { return contentElemMap.get(this).text; }
      set text(v) {
        const st = contentElemMap.get(this);
        st.text = v === null || v === undefined ? '' : String(v);
        st.rawText = st.text;
        notifyHierarchyChange(this);
      }
      replaceText(beginIndex, endIndex, newText) {
        const st = contentElemMap.get(this);
        const b = Number(beginIndex) | 0;
        const e = Number(endIndex) | 0;
        if (b < 0 || e < b || e > st.text.length) {
          throw rangeError('RangeError: TextElement.replaceText index out of bounds.');
        }
        const rep = newText === null || newText === undefined ? '' : String(newText);
        st.text = st.text.slice(0, b) + rep + st.text.slice(e);
        st.rawText = st.text;
        notifyHierarchyChange(this);
      }
    }
    tagType(TextElement, 'TextElement', v => contentElemMap.get(v)?.kind === 'text');

    class GraphicElement extends ContentElement {
      constructor(
        graphic = null,
        elementWidth = 15.0,
        elementHeight = 15.0,
        elementFormat = null,
        eventMirror = null,
        textRotation = 'rotate0'
      ) {
        if (new.target !== GraphicElement) {
          throw new TypeError('GraphicElement is final and cannot be subclassed.');
        }
        super(elementFormat, eventMirror, textRotation);
        const w = Number(elementWidth);
        const h = Number(elementHeight);
        if (!Number.isFinite(w) || w < 0 || !Number.isFinite(h) || h < 0) {
          throw argError('ArgumentError: GraphicElement dimensions must be non-negative.');
        }
        const st = contentElemMap.get(this);
        st.kind = 'graphic';
        st.graphic = graphic ?? null;
        st.elementWidth = w;
        st.elementHeight = h;
        st.rawText = '\uFDEF';
        st.text = null;
        return Object.seal(this);
      }
      get graphic() { return contentElemMap.get(this).graphic; }
      set graphic(v) {
        contentElemMap.get(this).graphic = v ?? null;
        notifyHierarchyChange(this);
      }
      get elementWidth() { return contentElemMap.get(this).elementWidth; }
      set elementWidth(v) {
        const n = Number(v);
        if (!Number.isFinite(n) || n < 0) throw argError('ArgumentError: GraphicElement.elementWidth must be >= 0.');
        contentElemMap.get(this).elementWidth = n;
        notifyHierarchyChange(this);
      }
      get elementHeight() { return contentElemMap.get(this).elementHeight; }
      set elementHeight(v) {
        const n = Number(v);
        if (!Number.isFinite(n) || n < 0) throw argError('ArgumentError: GraphicElement.elementHeight must be >= 0.');
        contentElemMap.get(this).elementHeight = n;
        notifyHierarchyChange(this);
      }
    }
    tagType(GraphicElement, 'GraphicElement', v => contentElemMap.get(v)?.kind === 'graphic');

    class GroupElement extends ContentElement {
      constructor(elements = null, elementFormat = null, eventMirror = null, textRotation = 'rotate0') {
        if (new.target !== GroupElement) {
          throw new TypeError('GroupElement is final and cannot be subclassed.');
        }
        super(elementFormat, eventMirror, textRotation);
        const st = contentElemMap.get(this);
        st.kind = 'group';
        st.elements = [];
        st.rawText = '';
        st.text = '';
        if (elements !== null && elements !== undefined) {
          this.setElements(elements);
        }
        return Object.seal(this);
      }
      get elementCount() { return contentElemMap.get(this).elements.length; }
      getElementAt(index) {
        const els = contentElemMap.get(this).elements;
        const i = Number(index) | 0;
        if (i < 0 || i >= els.length) throw rangeError('RangeError: GroupElement index out of bounds.');
        return els[i];
      }
      getElementIndex(element) {
        return contentElemMap.get(this).elements.indexOf(element);
      }
      getElementAtCharIndex(charIndex) {
        const st = contentElemMap.get(this);
        const ci = Number(charIndex) | 0;
        if (ci < 0 || ci >= st.rawText.length) {
          throw rangeError('RangeError: GroupElement.getElementAtCharIndex out of bounds.');
        }
        let acc = 0;
        for (const child of st.elements) {
          const cst = contentElemMap.get(child);
          const len = cst.rawText.length;
          if (ci >= acc && ci < acc + len) return child;
          acc += len;
        }
        throw rangeError('RangeError: GroupElement.getElementAtCharIndex out of bounds.');
      }
      setElements(value) {
        const st = contentElemMap.get(this);
        for (const old of st.elements) {
          const ost = contentElemMap.get(old);
          ost.groupElement = null;
          ost.textBlock = null;
          ost.textBlockBeginIndex = -1;
        }
        st.elements = [];
        if (value !== null && value !== undefined) {
          if (typeof value[Symbol.iterator] !== 'function') {
            throw new TypeError('GroupElement.setElements requires a Vector.<ContentElement> or null.');
          }
          for (const item of value) {
            const ist = contentElemMap.get(item);
            if (!ist) throw new TypeError('GroupElement elements must be ContentElement instances.');
            if (ist.groupElement || ist.textBlock) {
              throw argError('ArgumentError: ContentElement already belongs to a GroupElement or TextBlock.');
            }
            st.elements.push(item);
          }
        }
        notifyHierarchyChange(this);
      }
      replaceElements(beginIndex, endIndex, newElements) {
        const st = contentElemMap.get(this);
        const b = Number(beginIndex) | 0;
        const e = Number(endIndex) | 0;
        if (b < 0 || e < b || e > st.elements.length) {
          throw rangeError('RangeError: GroupElement.replaceElements range out of bounds.');
        }
        const incoming = [];
        if (newElements !== null && newElements !== undefined) {
          for (const item of newElements) {
            const ist = contentElemMap.get(item);
            if (!ist) throw new TypeError('GroupElement.replaceElements requires ContentElement items.');
            incoming.push(item);
          }
        }
        const removed = st.elements.splice(b, e - b, ...incoming);
        for (const old of removed) {
          const ost = contentElemMap.get(old);
          if (!st.elements.includes(old)) {
            ost.groupElement = null;
            ost.textBlock = null;
            ost.textBlockBeginIndex = -1;
          }
        }
        notifyHierarchyChange(this);
        return makeVector('flash.text.engine.ContentElement', removed);
      }
      splitTextElement(elementIndex, splitIndex) {
        const st = contentElemMap.get(this);
        const ei = Number(elementIndex) | 0;
        if (ei < 0 || ei >= st.elements.length) {
          throw rangeError('RangeError: GroupElement.splitTextElement elementIndex out of bounds.');
        }
        const target = st.elements[ei];
        const tst = contentElemMap.get(target);
        if (!tst || tst.kind !== 'text') {
          throw argError('ArgumentError: splitTextElement target must be a TextElement.');
        }
        const si = Number(splitIndex) | 0;
        if (si <= 0 || si >= tst.text.length) {
          throw rangeError('RangeError: splitTextElement splitIndex out of bounds.');
        }
        const rightText = tst.text.slice(si);
        tst.text = tst.text.slice(0, si);
        tst.rawText = tst.text;
        const second = new TextElement(rightText, tst.elementFormat, tst.eventMirror, tst.textRotation);
        st.elements.splice(ei + 1, 0, second);
        notifyHierarchyChange(this);
        return second;
      }
      mergeTextElements(beginIndex, endIndex) {
        const st = contentElemMap.get(this);
        const b = Number(beginIndex) | 0;
        const e = Number(endIndex) | 0;
        if (b < 0 || e <= b || e > st.elements.length) {
          throw rangeError('RangeError: GroupElement.mergeTextElements range out of bounds.');
        }
        for (let i = b; i < e; i++) {
          if (contentElemMap.get(st.elements[i])?.kind !== 'text') {
            throw argError('ArgumentError: mergeTextElements requires all elements in range to be TextElement.');
          }
        }
        const first = st.elements[b];
        const fst = contentElemMap.get(first);
        let merged = fst.text;
        for (let i = b + 1; i < e; i++) {
          const cst = contentElemMap.get(st.elements[i]);
          merged += cst.text;
          cst.groupElement = null;
          cst.textBlock = null;
          cst.textBlockBeginIndex = -1;
        }
        st.elements.splice(b + 1, e - b - 1);
        fst.text = merged;
        fst.rawText = merged;
        notifyHierarchyChange(this);
        return first;
      }
      groupElements(beginIndex, endIndex) {
        const st = contentElemMap.get(this);
        const b = Number(beginIndex) | 0;
        const e = Number(endIndex) | 0;
        if (b < 0 || e < b || e > st.elements.length) {
          throw rangeError('RangeError: GroupElement.groupElements range out of bounds.');
        }
        const slice = st.elements.splice(b, e - b);
        for (const item of slice) {
          const ist = contentElemMap.get(item);
          ist.groupElement = null;
          ist.textBlock = null;
        }
        const sub = new GroupElement(slice, st.elementFormat, st.eventMirror, st.textRotation);
        st.elements.splice(b, 0, sub);
        notifyHierarchyChange(this);
        return sub;
      }
      ungroupElements(groupIndex) {
        const st = contentElemMap.get(this);
        const gi = Number(groupIndex) | 0;
        if (gi < 0 || gi >= st.elements.length) {
          throw rangeError('RangeError: GroupElement.ungroupElements index out of bounds.');
        }
        const sub = st.elements[gi];
        const sst = contentElemMap.get(sub);
        if (!sst || sst.kind !== 'group') {
          throw argError('ArgumentError: ungroupElements target must be a GroupElement.');
        }
        const children = sst.elements.slice();
        sst.elements = [];
        sst.groupElement = null;
        sst.textBlock = null;
        st.elements.splice(gi, 1, ...children);
        notifyHierarchyChange(this);
      }
    }
    tagType(GroupElement, 'GroupElement', v => contentElemMap.get(v)?.kind === 'group');

    class TextJustifier {
      constructor(locale = 'en', lineJustification = 'unjustified') {
        if (new.target === TextJustifier) {
          throw argError('ArgumentError: Error #2012: TextJustifier$ class cannot be instantiated.', 2012);
        }
        if (locale === null || locale === undefined || String(locale).length === 0) {
          throw argError('ArgumentError: TextJustifier.locale must be a non-empty String.');
        }
        const lj = String(lineJustification);
        kcall(() => kernel.validate_fte_enum('lineJustification', lj));
        justifierMap.set(this, {
          kind: 'space',
          locale: String(locale),
          lineJustification: lj,
          letterSpacing: false,
          minimumSpacing: 0.5,
          optimumSpacing: 1.0,
          maximumSpacing: 1.5,
          justificationStyle: 'pushInKinsoku',
          composeTrailingIdeographicSpaces: false,
        });
      }
      static getJustifierForLocale(locale) {
        if (locale === null || locale === undefined) {
          throw argError('ArgumentError: Locale must not be null.');
        }
        const loc = String(locale);
        const kind = kcall(() => kernel.justifier_kind_for_locale(loc));
        return kind === 'east_asian' || kind === 'eastAsian'
          ? new EastAsianJustifier(loc, 'unjustified', 'pushInKinsoku')
          : new SpaceJustifier(loc, 'unjustified', false);
      }
      static createJustifierForLocale(locale) {
        return TextJustifier.getJustifierForLocale(locale);
      }
      get locale() { return justifierMap.get(this).locale; }
      get lineJustification() { return justifierMap.get(this).lineJustification; }
      set lineJustification(v) {
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('lineJustification', s));
        justifierMap.get(this).lineJustification = s;
      }
      clone() {
        return new SpaceJustifier(this.locale, this.lineJustification);
      }
    }
    tagType(TextJustifier, 'TextJustifier', v => justifierMap.has(v));

    class SpaceJustifier extends TextJustifier {
      constructor(locale = 'en', lineJustification = 'unjustified', letterSpacing = false) {
        if (new.target !== SpaceJustifier) {
          throw new TypeError('SpaceJustifier is final and cannot be subclassed.');
        }
        super(locale, lineJustification);
        const st = justifierMap.get(this);
        st.kind = 'space';
        st.letterSpacing = Boolean(letterSpacing);
        return Object.seal(this);
      }
      get letterSpacing() { return justifierMap.get(this).letterSpacing; }
      set letterSpacing(v) { justifierMap.get(this).letterSpacing = Boolean(v); }
      get minimumSpacing() { return justifierMap.get(this).minimumSpacing; }
      set minimumSpacing(v) {
        const n = Number(v);
        if (!Number.isFinite(n) || n < 0 || n > justifierMap.get(this).optimumSpacing) {
          throw argError('ArgumentError: SpaceJustifier.minimumSpacing must be in 0..optimumSpacing.');
        }
        justifierMap.get(this).minimumSpacing = n;
      }
      get optimumSpacing() { return justifierMap.get(this).optimumSpacing; }
      set optimumSpacing(v) {
        const n = Number(v);
        const st = justifierMap.get(this);
        if (!Number.isFinite(n) || n < st.minimumSpacing || n > st.maximumSpacing) {
          throw argError('ArgumentError: SpaceJustifier.optimumSpacing must be between minimumSpacing and maximumSpacing.');
        }
        st.optimumSpacing = n;
      }
      get maximumSpacing() { return justifierMap.get(this).maximumSpacing; }
      set maximumSpacing(v) {
        const n = Number(v);
        if (!Number.isFinite(n) || n < justifierMap.get(this).optimumSpacing) {
          throw argError('ArgumentError: SpaceJustifier.maximumSpacing must be >= optimumSpacing.');
        }
        justifierMap.get(this).maximumSpacing = n;
      }
      clone() {
        const st = justifierMap.get(this);
        const c = new SpaceJustifier(st.locale, st.lineJustification, st.letterSpacing);
        const cst = justifierMap.get(c);
        cst.minimumSpacing = st.minimumSpacing;
        cst.optimumSpacing = st.optimumSpacing;
        cst.maximumSpacing = st.maximumSpacing;
        return c;
      }
    }
    tagType(SpaceJustifier, 'SpaceJustifier', v => justifierMap.get(v)?.kind === 'space');

    class EastAsianJustifier extends TextJustifier {
      constructor(
        locale = 'ja',
        lineJustification = 'allButLast',
        justificationStyle = 'pushInKinsoku'
      ) {
        if (new.target !== EastAsianJustifier) {
          throw new TypeError('EastAsianJustifier is final and cannot be subclassed.');
        }
        super(locale, lineJustification);
        const js = String(justificationStyle);
        kcall(() => kernel.validate_fte_enum('justificationStyle', js));
        const st = justifierMap.get(this);
        st.kind = 'eastAsian';
        st.justificationStyle = js;
        return Object.seal(this);
      }
      get justificationStyle() { return justifierMap.get(this).justificationStyle; }
      set justificationStyle(v) {
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('justificationStyle', s));
        justifierMap.get(this).justificationStyle = s;
      }
      get composeTrailingIdeographicSpaces() {
        return justifierMap.get(this).composeTrailingIdeographicSpaces;
      }
      set composeTrailingIdeographicSpaces(v) {
        justifierMap.get(this).composeTrailingIdeographicSpaces = Boolean(v);
      }
      clone() {
        const st = justifierMap.get(this);
        const c = new EastAsianJustifier(st.locale, st.lineJustification, st.justificationStyle);
        justifierMap.get(c).composeTrailingIdeographicSpaces = st.composeTrailingIdeographicSpaces;
        return c;
      }
    }
    tagType(EastAsianJustifier, 'EastAsianJustifier', v => justifierMap.get(v)?.kind === 'eastAsian');

    class TabStop {
      constructor(alignment = 'start', position = 0.0, decimalAlignmentToken = '') {
        if (new.target !== TabStop) {
          throw new TypeError('TabStop is final and cannot be subclassed.');
        }
        const al = String(alignment);
        kcall(() => kernel.validate_fte_enum('tabAlignment', al));
        const pos = Number(position);
        if (!Number.isFinite(pos) || pos < 0) {
          throw argError('ArgumentError: TabStop.position must be non-negative.');
        }
        tabStopMap.set(this, {
          alignment: al,
          position: pos,
          decimalAlignmentToken: decimalAlignmentToken === null || decimalAlignmentToken === undefined ? '' : String(decimalAlignmentToken),
        });
        return Object.seal(this);
      }
      get alignment() { return tabStopMap.get(this).alignment; }
      set alignment(v) {
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('tabAlignment', s));
        tabStopMap.get(this).alignment = s;
      }
      get position() { return tabStopMap.get(this).position; }
      set position(v) {
        const n = Number(v);
        if (!Number.isFinite(n) || n < 0) throw argError('ArgumentError: TabStop.position must be >= 0.');
        tabStopMap.get(this).position = n;
      }
      get decimalAlignmentToken() { return tabStopMap.get(this).decimalAlignmentToken; }
      set decimalAlignmentToken(v) {
        tabStopMap.get(this).decimalAlignmentToken = v === null || v === undefined ? '' : String(v);
      }
    }
    tagType(TabStop, 'TabStop', v => tabStopMap.has(v));

    class TextLineMirrorRegion {
      constructor(secret, info) {
        if (secret !== fteToken) {
          throw argError('ArgumentError: Error #2012: TextLineMirrorRegion$ class cannot be instantiated.', 2012);
        }
        mirrorRegionMap.set(this, info);
        return Object.seal(this);
      }
      get bounds() {
        const b = mirrorRegionMap.get(this).bounds;
        const R = geometry?.Rectangle;
        return R ? new R(b[0], b[1], b[2], b[3]) : { x: b[0], y: b[1], width: b[2], height: b[3] };
      }
      get element() { return mirrorRegionMap.get(this).element; }
      get mirror() { return mirrorRegionMap.get(this).mirror; }
      get nextRegion() { return mirrorRegionMap.get(this).nextRegion; }
      get previousRegion() { return mirrorRegionMap.get(this).previousRegion; }
      get textLine() { return mirrorRegionMap.get(this).textLine; }
    }
    tagType(TextLineMirrorRegion, 'TextLineMirrorRegion', v => mirrorRegionMap.has(v));

    // TextLine class tag: actual instances are display proxies with owner === 'text_line'.
    function TextLine() {
      throw argError('ArgumentError: Error #2012: TextLine$ class cannot be instantiated.', 2012);
    }
    Object.defineProperty(TextLine, 'MAX_LINE_WIDTH', { value: 1000000, enumerable: true });
    tagType(TextLine, 'TextLine', v => textLineStateMap.has(v));

    function requireTextLineState(line) {
      const st = textLineStateMap.get(line);
      if (!st) throw new TypeError('Invalid TextLine receiver.');
      return st;
    }

    function requireAtomIndex(st, idx) {
      const i = Number(idx) | 0;
      if (i < 0 || i >= st.atoms.length) {
        throw rangeError('RangeError: Error #2006: The supplied atom index is out of bounds.');
      }
      return st.atoms[i];
    }

    const TEXT_LINE_READONLY_PROPS = new Set([
      'ascent',
      'atomCount',
      'descent',
      'hasGraphicElement',
      'hasTabs',
      'mirrorRegions',
      'nextLine',
      'previousLine',
      'rawTextLength',
      'specifiedWidth',
      'textBlock',
      'textBlockBeginIndex',
      'textHeight',
      'textWidth',
      'totalAscent',
      'totalDescent',
      'totalHeight',
      'unjustifiedTextWidth',
    ]);

    function textLineBounds(target) {
      const st = textLineStateMap.get(target);
      if (!st || st.textWidth <= 0) return null;
      return {
        x: 0,
        y: -st.ascent,
        width: st.textWidth,
        height: st.totalHeight || st.ascent + st.descent,
      };
    }

    function textLineGet(target, name) {
      const st = textLineStateMap.get(target);
      if (!st) return undefined;
      switch (name) {
        case 'ascent': return st.ascent;
        case 'atomCount': return st.atoms.length;
        case 'descent': return st.descent;
        case 'hasGraphicElement': return st.hasGraphicElement;
        case 'hasTabs': return st.hasTabs;
        case 'mirrorRegions':
          return st.mirrorRegions.length > 0
            ? makeVector('flash.text.engine.TextLineMirrorRegion', st.mirrorRegions, true)
            : null;
        case 'nextLine': return st.nextLine;
        case 'previousLine': return st.previousLine;
        case 'rawTextLength': return st.rawTextLength;
        case 'specifiedWidth': return st.specifiedWidth;
        case 'textBlock': return st.textBlock;
        case 'textBlockBeginIndex': return st.textBlockBeginIndex;
        case 'textHeight': return st.textHeight;
        case 'textWidth': return st.textWidth;
        case 'totalAscent': return st.totalAscent;
        case 'totalDescent': return st.totalDescent;
        case 'totalHeight': return st.totalHeight;
        case 'unjustifiedTextWidth': return st.unjustifiedTextWidth;
        case 'userData': return st.userData;
        case 'validity': return st.validity;
        default: return undefined;
      }
    }

    function textLineSet(target, name, value) {
      const st = textLineStateMap.get(target);
      if (!st) return false;
      if (name === 'userData') {
        st.userData = value;
        return true;
      }
      if (name === 'validity') {
        const s = String(value);
        kcall(() => kernel.validate_fte_enum('textLineValidity', s));
        st.validity = s;
        if (st.textBlock && s === 'invalid') {
          const bst = textBlockMap.get(st.textBlock);
          if (bst && !bst.firstInvalidLine) bst.firstInvalidLine = target;
        }
        return true;
      }
      if (TEXT_LINE_READONLY_PROPS.has(name)) {
        throw new TypeError(`TextLine.${name} is read-only.`);
      }
      return false;
    }

    function textLineMethods(target) {
      return {
        dump: () => requireTextLineState(target).dump,
        flushAtomData: () => {
          requireTextLineState(target);
        },
        getAtomBidiLevel: atomIndex => requireAtomIndex(requireTextLineState(target), atomIndex).bidi_level,
        getAtomBounds: atomIndex => {
          const a = requireAtomIndex(requireTextLineState(target), atomIndex);
          const R = geometry?.Rectangle;
          return R
            ? new R(a.bounds[0], a.bounds[1], a.bounds[2], a.bounds[3])
            : { x: a.bounds[0], y: a.bounds[1], width: a.bounds[2], height: a.bounds[3] };
        },
        getAtomCenter: atomIndex => requireAtomIndex(requireTextLineState(target), atomIndex).center,
        getAtomGraphic: atomIndex => {
          const st = requireTextLineState(target);
          const a = requireAtomIndex(st, atomIndex);
          if (!a.is_graphic) return null;
          return st.elementMap.get(a.element_id)?.graphic ?? null;
        },
        getAtomIndexAtCharIndex: charIndex => {
          const st = requireTextLineState(target);
          const ci = Number(charIndex) | 0;
          for (let i = 0; i < st.atoms.length; i++) {
            const a = st.atoms[i];
            if (ci >= a.char_begin && ci < a.char_end) return i;
          }
          return -1;
        },
        getAtomIndexAtPoint: (stageX, stageY) => {
          const st = requireTextLineState(target);
          const P = geometry?.Point;
          let lx = Number(stageX);
          let ly = Number(stageY);
          if (P && typeof target.globalToLocal === 'function') {
            const localPt = target.globalToLocal(new P(lx, ly));
            lx = localPt.x;
            ly = localPt.y;
          }
          for (let i = 0; i < st.atoms.length; i++) {
            const b = st.atoms[i].bounds;
            if (lx >= b[0] && lx <= b[0] + b[2] && ly >= b[1] && ly <= b[1] + b[3]) {
              return i;
            }
          }
          return -1;
        },
        getAtomTextBlockBeginIndex: atomIndex =>
          requireAtomIndex(requireTextLineState(target), atomIndex).char_begin,
        getAtomTextBlockEndIndex: atomIndex =>
          requireAtomIndex(requireTextLineState(target), atomIndex).char_end,
        getAtomTextRotation: atomIndex =>
          requireAtomIndex(requireTextLineState(target), atomIndex).text_rotation,
        getAtomWordBoundaryOnLeft: atomIndex =>
          Boolean(requireAtomIndex(requireTextLineState(target), atomIndex).word_boundary_left),
        getBaselinePosition: baseline => {
          const st = requireTextLineState(target);
          const b = String(baseline);
          kcall(() => kernel.validate_fte_enum('textBaseline', b));
          if (Object.hasOwn(st.baselines, b)) return st.baselines[b];
          return st.baselines.roman || 0;
        },
        getMirrorRegion: mirror => {
          const st = requireTextLineState(target);
          for (const mr of st.mirrorRegions) {
            if (mr.mirror === mirror) return mr;
          }
          return null;
        },
      };
    }

    function collectElementRuns(rootElem, elementMap, elementObjMap) {
      const runs = [];
      function visit(el, inheritedFormat, inheritedMirror, inheritedRotation) {
        const est = contentElemMap.get(el);
        const fmtObj = est.elementFormat || inheritedFormat;
        const fmt = fmtObj ? elemFormatMap.get(fmtObj) : null;
        const fd = fmt ? fontDescMap.get(fmt.fontDescription) : null;
        const mirror = est.eventMirror || inheritedMirror;
        const rot = est.textRotation !== 'rotate0' ? est.textRotation : inheritedRotation;
        elementMap.set(est.id, est);
        elementObjMap.set(est.id, el);
        if (est.kind === 'group') {
          for (const child of est.elements) {
            visit(child, fmtObj, mirror, rot);
          }
        } else if (est.kind === 'graphic') {
          runs.push({
            element_id: est.id,
            is_graphic: true,
            text: '\uFDEF',
            graphic_width: est.elementWidth,
            graphic_height: est.elementHeight,
            font_name: fd ? fd.fontName : '_serif',
            font_size: fmt ? fmt.fontSize : 12,
            font_weight: fd ? fd.fontWeight : 'normal',
            font_posture: fd ? fd.fontPosture : 'normal',
            color: fmt ? fmt.color : 0,
            alpha: fmt ? fmt.alpha : 1,
            tracking_left: fmt ? fmt.trackingLeft : 0,
            tracking_right: fmt ? fmt.trackingRight : 0,
            baseline_shift: fmt ? fmt.baselineShift : 0,
            text_rotation: rot || 'rotate0',
            break_opportunity: fmt ? fmt.breakOpportunity : 'auto',
            digit_width: fmt ? fmt.digitWidth : 'default',
            typographic_case: fmt ? fmt.typographicCase : 'default',
            has_mirror: Boolean(mirror),
          });
        } else {
          runs.push({
            element_id: est.id,
            is_graphic: false,
            text: est.text || '',
            graphic_width: 0,
            graphic_height: 0,
            font_name: fd ? fd.fontName : '_serif',
            font_size: fmt ? fmt.fontSize : 12,
            font_weight: fd ? fd.fontWeight : 'normal',
            font_posture: fd ? fd.fontPosture : 'normal',
            color: fmt ? fmt.color : 0,
            alpha: fmt ? fmt.alpha : 1,
            tracking_left: fmt ? fmt.trackingLeft : 0,
            tracking_right: fmt ? fmt.trackingRight : 0,
            baseline_shift: fmt ? fmt.baselineShift : 0,
            text_rotation: rot || 'rotate0',
            break_opportunity: fmt ? fmt.breakOpportunity : 'auto',
            digit_width: fmt ? fmt.digitWidth : 'default',
            typographic_case: fmt ? fmt.typographicCase : 'default',
            has_mirror: Boolean(mirror),
          });
        }
      }
      visit(rootElem, null, null, 'rotate0');
      return runs;
    }

    function populateTextLineFromRust(lineProxy, block, prevLine, lineData, elementMap, elementObjMap) {
      const bst = textBlockMap.get(block);
      const mirrorRegions = [];
      for (const mr of lineData.mirror_regions || []) {
        const elObj = elementObjMap.get(mr.element_id) || null;
        const est = elementMap.get(mr.element_id);
        const region = new TextLineMirrorRegion(fteToken, {
          bounds: mr.bounds,
          element: elObj,
          mirror: est ? est.eventMirror : null,
          nextRegion: null,
          previousRegion: mirrorRegions.length > 0 ? mirrorRegions[mirrorRegions.length - 1] : null,
          textLine: lineProxy,
        });
        if (mirrorRegions.length > 0) {
          mirrorRegionMap.get(mirrorRegions[mirrorRegions.length - 1]).nextRegion = region;
        }
        mirrorRegions.push(region);
      }

      const prevSt = prevLine ? textLineStateMap.get(prevLine) : null;
      let nextLine = null;
      if (prevSt) {
        nextLine = prevSt.nextLine;
        prevSt.nextLine = lineProxy;
        if (nextLine) {
          const nst = textLineStateMap.get(nextLine);
          if (nst) nst.previousLine = lineProxy;
        }
      } else if (bst.firstLine && bst.firstLine !== lineProxy) {
        nextLine = bst.firstLine;
        const fst = textLineStateMap.get(nextLine);
        if (fst) fst.previousLine = lineProxy;
      }

      const existing = textLineStateMap.get(lineProxy);
      const st = {
        textBlock: block,
        previousLine: prevLine || null,
        nextLine: existing ? existing.nextLine : nextLine,
        textBlockBeginIndex: lineData.text_block_begin_index,
        rawTextLength: lineData.raw_text_length,
        specifiedWidth: lineData.specified_width,
        textWidth: lineData.text_width,
        textHeight: lineData.text_height,
        unjustifiedTextWidth: lineData.unjustified_text_width,
        ascent: lineData.ascent,
        descent: lineData.descent,
        totalAscent: lineData.total_ascent,
        totalDescent: lineData.total_descent,
        totalHeight: lineData.total_height,
        hasGraphicElement: lineData.has_graphic_element,
        hasTabs: lineData.has_tabs,
        atoms: lineData.atoms || [],
        baselines: lineData.baselines || {},
        mirrorRegions,
        dump: lineData.dump || '',
        validity: 'valid',
        userData: existing ? existing.userData : undefined,
        elementMap,
        elementObjMap,
      };
      textLineStateMap.set(lineProxy, st);

      if (player && typeof player.set_text_line_draws === 'function') {
        const ref = referenceOf(lineProxy);
        if (ref) {
          player.set_text_line_draws(ref.key, JSON.stringify(lineData.draw_runs || []));
        }
      }

      // Attach any GraphicElement display objects to the TextLine container
      for (const atom of st.atoms) {
        if (atom.is_graphic) {
          const est = elementMap.get(atom.element_id);
          if (est && est.graphic && typeof lineProxy.addChild === 'function') {
            try {
              lineProxy.addChild(est.graphic);
              est.graphic.x = atom.bounds[0];
              est.graphic.y = atom.bounds[1];
            } catch (_) {
              /* non-DisplayObject graphic mock */
            }
          }
        }
      }

      // Rebuild bst.lines doubly-linked list from firstLine
      if (!prevLine) {
        bst.firstLine = lineProxy;
      }
      const rebuilt = [];
      let cur = bst.firstLine;
      const seen = new Set();
      while (cur && !seen.has(cur)) {
        seen.add(cur);
        rebuilt.push(cur);
        cur = textLineStateMap.get(cur)?.nextLine || null;
      }
      bst.lines = rebuilt;
      bst.lastLine = rebuilt.length > 0 ? rebuilt[rebuilt.length - 1] : null;
      bst.firstInvalidLine = rebuilt.find(l => textLineStateMap.get(l)?.validity === 'invalid') || null;
    }

    class TextBlock {
      constructor(
        content = null,
        tabStops = null,
        textJustifier = null,
        lineRotation = 'rotate0',
        baselineZero = 'roman',
        bidiLevel = 0,
        applyNonLinearFontScaling = true,
        baselineFontDescription = null,
        baselineFontSize = 12.0
      ) {
        if (new.target !== TextBlock) {
          throw new TypeError('TextBlock is final and cannot be subclassed.');
        }
        const st = {
          content: null,
          tabStops: null,
          textJustifier: new SpaceJustifier('en', 'unjustified', false),
          lineRotation: 'rotate0',
          baselineZero: 'roman',
          bidiLevel: 0,
          applyNonLinearFontScaling: true,
          baselineFontDescription: null,
          baselineFontSize: 12.0,
          textLineCreationResult: null,
          firstLine: null,
          lastLine: null,
          firstInvalidLine: null,
          lines: [],
          userData: undefined,
        };
        textBlockMap.set(this, st);
        if (content !== null && content !== undefined) this.content = content;
        if (tabStops !== null && tabStops !== undefined) this.tabStops = tabStops;
        if (textJustifier !== null && textJustifier !== undefined) this.textJustifier = textJustifier;
        this.lineRotation = lineRotation;
        this.baselineZero = baselineZero;
        this.bidiLevel = bidiLevel;
        this.applyNonLinearFontScaling = applyNonLinearFontScaling;
        if (baselineFontDescription !== null && baselineFontDescription !== undefined) {
          this.baselineFontDescription = baselineFontDescription;
        }
        this.baselineFontSize = baselineFontSize;
        return Object.seal(this);
      }
      get applyNonLinearFontScaling() { return textBlockMap.get(this).applyNonLinearFontScaling; }
      set applyNonLinearFontScaling(v) {
        textBlockMap.get(this).applyNonLinearFontScaling = Boolean(v);
        markBlockDirty(this);
      }
      get baselineFontDescription() { return textBlockMap.get(this).baselineFontDescription; }
      set baselineFontDescription(v) {
        if (v !== null && v !== undefined && !fontDescMap.has(v)) {
          throw new TypeError('TextBlock.baselineFontDescription requires a FontDescription or null.');
        }
        if (v) v.locked = true;
        textBlockMap.get(this).baselineFontDescription = v ?? null;
        markBlockDirty(this);
      }
      get baselineFontSize() { return textBlockMap.get(this).baselineFontSize; }
      set baselineFontSize(v) {
        const n = Number(v);
        if (!Number.isFinite(n) || n < 0) throw argError('ArgumentError: baselineFontSize must be >= 0.');
        textBlockMap.get(this).baselineFontSize = n;
        markBlockDirty(this);
      }
      get baselineZero() { return textBlockMap.get(this).baselineZero; }
      set baselineZero(v) {
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('baselineZero', s));
        textBlockMap.get(this).baselineZero = s;
        markBlockDirty(this);
      }
      get bidiLevel() { return textBlockMap.get(this).bidiLevel; }
      set bidiLevel(v) {
        const n = Number(v) | 0;
        if (n < 0) throw argError('ArgumentError: TextBlock.bidiLevel must be non-negative.');
        textBlockMap.get(this).bidiLevel = n;
        markBlockDirty(this);
      }
      get content() { return textBlockMap.get(this).content; }
      set content(v) {
        const st = textBlockMap.get(this);
        if (v !== null && v !== undefined) {
          const cst = contentElemMap.get(v);
          if (!cst) throw new TypeError('TextBlock.content requires a ContentElement or null.');
          if (cst.groupElement || (cst.textBlock && cst.textBlock !== this)) {
            throw argError('ArgumentError: ContentElement already belongs to a GroupElement or another TextBlock.');
          }
        }
        if (st.content && st.content !== v) {
          recomputeTreeIndices(st.content, null);
        }
        st.content = v ?? null;
        if (st.content) {
          recomputeTreeIndices(st.content, this);
        }
        markBlockDirty(this);
      }
      get firstInvalidLine() { return textBlockMap.get(this).firstInvalidLine; }
      get firstLine() { return textBlockMap.get(this).firstLine; }
      get lastLine() { return textBlockMap.get(this).lastLine; }
      get lineRotation() { return textBlockMap.get(this).lineRotation; }
      set lineRotation(v) {
        const s = String(v);
        kcall(() => kernel.validate_fte_enum('lineRotation', s));
        textBlockMap.get(this).lineRotation = s;
        markBlockDirty(this);
      }
      get tabStops() {
        const ts = textBlockMap.get(this).tabStops;
        return ts ? makeVector('flash.text.engine.TabStop', ts) : null;
      }
      set tabStops(v) {
        if (v === null || v === undefined) {
          textBlockMap.get(this).tabStops = null;
          markBlockDirty(this);
          return;
        }
        if (typeof v[Symbol.iterator] !== 'function') {
          throw new TypeError('TextBlock.tabStops requires a Vector.<TabStop> or null.');
        }
        const list = [];
        for (const item of v) {
          if (!tabStopMap.has(item)) throw new TypeError('TextBlock.tabStops items must be TabStop instances.');
          list.push(item);
        }
        textBlockMap.get(this).tabStops = list;
        markBlockDirty(this);
      }
      get textJustifier() {
        return textBlockMap.get(this).textJustifier.clone();
      }
      set textJustifier(v) {
        if (!justifierMap.has(v)) {
          throw new TypeError('TextBlock.textJustifier requires a TextJustifier instance.');
        }
        textBlockMap.get(this).textJustifier = v.clone();
        markBlockDirty(this);
      }
      get textLineCreationResult() { return textBlockMap.get(this).textLineCreationResult; }
      get userData() { return textBlockMap.get(this).userData; }
      set userData(v) { textBlockMap.get(this).userData = v; }

      createTextLine(previousLine = null, width = 1000000, lineOffset = 0.0, fitSomething = false) {
        return this._buildTextLine(null, previousLine, width, lineOffset, fitSomething);
      }

      recreateTextLine(textLine, previousLine = null, width = 1000000, lineOffset = 0.0, fitSomething = false) {
        if (!textLineStateMap.has(textLine)) {
          throw argError('ArgumentError: recreateTextLine requires a valid TextLine.');
        }
        return this._buildTextLine(textLine, previousLine, width, lineOffset, fitSomething);
      }

      _buildTextLine(recycleLine, previousLine, width, lineOffset, fitSomething) {
        const st = textBlockMap.get(this);
        const w = Number(width);
        const fit = Boolean(fitSomething);
        if (!Number.isFinite(w) || (!fit && w < 0) || w > 1000000) {
          throw argError('ArgumentError: createTextLine width out of bounds (0..1000000).');
        }
        let startChar = 0;
        if (previousLine !== null && previousLine !== undefined) {
          const pst = textLineStateMap.get(previousLine);
          if (!pst || pst.textBlock !== this || pst.validity === 'static') {
            throw argError('ArgumentError: previousLine must belong to this TextBlock and not be static.');
          }
          if (recycleLine && previousLine === recycleLine) {
            throw argError('ArgumentError: textLine and previousLine cannot be the same.');
          }
          startChar = pst.textBlockBeginIndex + pst.rawTextLength;
        }
        if (!st.content) {
          st.textLineCreationResult = 'complete';
          return null;
        }
        const elementMap = new Map();
        const elementObjMap = new Map();
        const runs = collectElementRuns(st.content, elementMap, elementObjMap);
        const jst = justifierMap.get(st.textJustifier);
        const req = {
          runs,
          start_char: startChar,
          max_width: w,
          line_offset: Number(lineOffset) || 0,
          fit_something: fit,
          justifier: {
            kind: jst.kind,
            locale: jst.locale,
            line_justification: jst.lineJustification,
            letter_spacing: jst.letterSpacing,
            minimum_spacing: jst.minimumSpacing,
            optimum_spacing: jst.optimumSpacing,
            maximum_spacing: jst.maximumSpacing,
            justification_style: jst.justificationStyle,
            compose_trailing_ideographic_spaces: jst.composeTrailingIdeographicSpaces,
          },
          tab_stops: (st.tabStops || []).map(t => {
            const tst = tabStopMap.get(t);
            return {
              alignment: tst.alignment,
              position: tst.position,
              decimal_alignment_token: tst.decimalAlignmentToken,
            };
          }),
          baseline_zero: st.baselineZero,
          baseline_font_size: st.baselineFontSize,
          line_rotation: st.lineRotation,
          bidi_level: st.bidiLevel,
          apply_non_linear_font_scaling: st.applyNonLinearFontScaling,
        };
        const resp = JSON.parse(kcall(() => kernel.layout_text_line_json(JSON.stringify(req))));
        st.textLineCreationResult = resp.creation_result;
        if (!resp.line) {
          return null;
        }
        const lineProxy =
          recycleLine || proxyForRef(JSON.parse(player.create_display('text_line')));
        populateTextLineFromRust(lineProxy, this, previousLine, resp.line, elementMap, elementObjMap);
        return lineProxy;
      }

      releaseLineCreationData() {}

      releaseLines(firstLine, lastLine) {
        const st = textBlockMap.get(this);
        const i0 = st.lines.indexOf(firstLine);
        const i1 = st.lines.indexOf(lastLine);
        if (i0 < 0 || i1 < i0) {
          throw argError('ArgumentError: releaseLines requires valid firstLine and lastLine in this TextBlock.');
        }
        const before = i0 > 0 ? st.lines[i0 - 1] : null;
        const after = i1 + 1 < st.lines.length ? st.lines[i1 + 1] : null;
        for (let i = i0; i <= i1; i++) {
          const line = st.lines[i];
          const lst = textLineStateMap.get(line);
          if (lst) {
            lst.validity = 'static';
            lst.textBlock = null;
            lst.previousLine = null;
            lst.nextLine = null;
          }
        }
        if (before) textLineStateMap.get(before).nextLine = after;
        else st.firstLine = after;
        if (after) textLineStateMap.get(after).previousLine = before;
        else st.lastLine = before;
        st.lines.splice(i0, i1 - i0 + 1);
        st.firstInvalidLine = st.lines.find(l => textLineStateMap.get(l)?.validity === 'invalid') || null;
      }

      findNextAtomBoundary(afterCharIndex) {
        const st = textBlockMap.get(this);
        const raw = st.content ? contentElemMap.get(st.content).rawText : '';
        return kcall(() => kernel.find_next_atom_boundary(raw, Number(afterCharIndex) | 0));
      }

      findPreviousAtomBoundary(beforeCharIndex) {
        const st = textBlockMap.get(this);
        const raw = st.content ? contentElemMap.get(st.content).rawText : '';
        return kcall(() => kernel.find_previous_atom_boundary(raw, Number(beforeCharIndex) | 0));
      }

      findNextWordBoundary(afterCharIndex) {
        const st = textBlockMap.get(this);
        const raw = st.content ? contentElemMap.get(st.content).rawText : '';
        return kcall(() => kernel.find_next_word_boundary(raw, Number(afterCharIndex) | 0));
      }

      findPreviousWordBoundary(beforeCharIndex) {
        const st = textBlockMap.get(this);
        const raw = st.content ? contentElemMap.get(st.content).rawText : '';
        return kcall(() => kernel.find_previous_word_boundary(raw, Number(beforeCharIndex) | 0));
      }

      getTextLineAtCharIndex(charIndex) {
        const st = textBlockMap.get(this);
        const ci = Number(charIndex) | 0;
        const rawLen = st.content ? contentElemMap.get(st.content).rawText.length : 0;
        if (ci < 0 || ci >= rawLen) {
          throw rangeError('RangeError: getTextLineAtCharIndex out of bounds.');
        }
        for (const line of st.lines) {
          const lst = textLineStateMap.get(line);
          if (lst && ci >= lst.textBlockBeginIndex && ci < lst.textBlockBeginIndex + lst.rawTextLength) {
            return line;
          }
        }
        return null;
      }

      dump() {
        const st = textBlockMap.get(this);
        const linesXml = st.lines.map(l => textLineStateMap.get(l)?.dump || '').join('');
        return `<block>${linesXml}</block>`;
      }
    }
    tagType(TextBlock, 'TextBlock', v => textBlockMap.has(v));

    // ---- flash.text.ime ----
    const compRangeMap = new WeakMap();
    class CompositionAttributeRange {
      constructor(relativeStart, relativeEnd, selected, converted) {
        if (new.target !== CompositionAttributeRange) {
          throw new TypeError('CompositionAttributeRange is final and cannot be subclassed.');
        }
        compRangeMap.set(this, {
          relativeStart: Number(relativeStart) | 0,
          relativeEnd: Number(relativeEnd) | 0,
          selected: Boolean(selected),
          converted: Boolean(converted),
        });
        return Object.seal(this);
      }
      get relativeStart() { return compRangeMap.get(this).relativeStart; }
      set relativeStart(v) { compRangeMap.get(this).relativeStart = Number(v) | 0; }
      get relativeEnd() { return compRangeMap.get(this).relativeEnd; }
      set relativeEnd(v) { compRangeMap.get(this).relativeEnd = Number(v) | 0; }
      get selected() { return compRangeMap.get(this).selected; }
      set selected(v) { compRangeMap.get(this).selected = Boolean(v); }
      get converted() { return compRangeMap.get(this).converted; }
      set converted(v) { compRangeMap.get(this).converted = Boolean(v); }
    }
    tagType(CompositionAttributeRange, 'CompositionAttributeRange', v => compRangeMap.has(v));

    const IIMEClient = function (value) {
      if (new.target) throw new TypeError('IIMEClient is an interface and cannot be instantiated.');
      if (!IIMEClient.__nfIs(value)) throw new TypeError('Type Coercion failed: cannot convert value to flash.text.ime.IIMEClient.');
      return value;
    };
    const IME_CLIENT_MEMBERS = [
      'compositionStartIndex',
      'compositionEndIndex',
      'selectionActiveIndex',
      'selectionAnchorIndex',
      'verticalTextLayout',
      'confirmComposition',
      'getTextBounds',
      'getTextInRange',
      'selectRange',
      'updateComposition',
    ];
    Object.defineProperties(IIMEClient, {
      IME_START_COMPOSITION: { value: 'imeStartComposition', enumerable: true },
      TEXT_INPUT: { value: 'textInput', enumerable: true },
    });
    tagType(
      IIMEClient,
      'IIMEClient',
      v =>
        Boolean(
          v &&
            typeof v === 'object' &&
            IME_CLIENT_MEMBERS.every(m => m in v)
        )
    );

    // =========================================================================
    // 4. 3D DisplayObject / Transform / SoftKeyboard / MorphShape / AVM1Movie
    // =========================================================================

    let pose3DMap = new WeakMap();
    let projMap = new WeakMap();
    let blendShaderMap = new WeakMap();
    let softKeyboardMap = new WeakMap();
    let activeSoftKeyboardRect = null;

    function get3dPose(target) {
      let rec = pose3DMap.get(target);
      if (!rec) {
        rec = {
          is3D: false,
          z: 0,
          scaleZ: 1,
          rotationX: 0,
          rotationY: 0,
          customMatrix3D: null,
        };
        pose3DMap.set(target, rec);
      }
      return rec;
    }

    function composeTargetMatrix3DArray(target) {
      const rec = get3dPose(target);
      const tx = Number(target.x) || 0;
      const ty = Number(target.y) || 0;
      const sx = Number(target.scaleX) || 1;
      const sy = Number(target.scaleY) || 1;
      const rz = Number(target.rotation) || 0;
      if (
        rec.customMatrix3D &&
        rec.custom2D &&
        Math.abs(tx - rec.custom2D[0]) < 1e-6 &&
        Math.abs(ty - rec.custom2D[1]) < 1e-6 &&
        Math.abs(sx - rec.custom2D[2]) < 1e-6 &&
        Math.abs(sy - rec.custom2D[3]) < 1e-6 &&
        Math.abs(rz - rec.custom2D[4]) < 1e-6
      ) {
        return rec.customMatrix3D.slice();
      }
      const pose = {
        x: tx,
        y: ty,
        z: rec.z,
        scaleX: sx,
        scaleY: sy,
        scaleZ: rec.scaleZ,
        rotationX: rec.rotationX,
        rotationY: rec.rotationY,
        rotationZ: rz,
      };
      return JSON.parse(kernel.compose_display_matrix3d_json(JSON.stringify(pose)));
    }

    function getMatrix3DForDisplay(target) {
      const rec = get3dPose(target);
      if (!rec.is3D) return null;
      const raw = composeTargetMatrix3DArray(target);
      const M3D = geometry3d?.Matrix3D;
      return M3D ? new M3D(raw) : null;
    }

    function setMatrix3DForDisplay(target, value) {
      const rec = get3dPose(target);
      if (value === null || value === undefined) {
        rec.is3D = false;
        rec.z = 0;
        rec.scaleZ = 1;
        rec.rotationX = 0;
        rec.rotationY = 0;
        rec.customMatrix3D = null;
        rec.custom2D = null;
        return;
      }
      if (!geometry3d?.Matrix3D?.__nfIs?.(value)) {
        throw new TypeError('Transform.matrix3D requires a flash.geom.Matrix3D or null.');
      }
      const raw = Array.from(value.rawData, Number);
      const dec = JSON.parse(kernel.decompose_display_matrix3d_json(JSON.stringify(raw)));
      rec.is3D = true;
      rec.customMatrix3D = raw;
      rec.z = dec.z;
      rec.scaleZ = dec.scaleZ;
      rec.rotationX = dec.rotationX;
      rec.rotationY = dec.rotationY;
      target.x = dec.x;
      target.y = dec.y;
      target.scaleX = dec.scaleX;
      target.scaleY = dec.scaleY;
      target.rotation = dec.rotationZ;
      rec.custom2D = [
        Number(target.x) || 0,
        Number(target.y) || 0,
        Number(target.scaleX) || 1,
        Number(target.scaleY) || 1,
        Number(target.rotation) || 0,
      ];
    }

    function getPerspectiveProjectionForDisplay(target) {
      if (projMap.has(target)) return projMap.get(target);
      const disp = getDisplay();
      if (disp) {
        try {
          const node = disp.descriptor(target);
          if (node.authored_path && node.authored_path.length === 0 && geometry3d?.PerspectiveProjection) {
            const pp = new geometry3d.PerspectiveProjection();
            projMap.set(target, pp);
            return pp;
          }
        } catch (_) { /* ignore */ }
      }
      return null;
    }

    function setPerspectiveProjectionForDisplay(target, value) {
      if (value !== null && value !== undefined && !geometry3d?.PerspectiveProjection?.__nfIs?.(value)) {
        throw new TypeError('Transform.perspectiveProjection requires a PerspectiveProjection or null.');
      }
      projMap.set(target, value ?? null);
    }

    function buildChainRootToLeaf(target) {
      const chain = [];
      let cur = target;
      const stg = getStage();
      for (let depth = 0; cur && cur !== stg && depth < 64; depth++) {
        chain.unshift(composeTargetMatrix3DArray(cur));
        cur = cur.parent;
      }
      if (chain.length === 0) chain.push(composeTargetMatrix3DArray(target));
      return chain;
    }

    function activeProjectionFor(target) {
      let cur = target;
      const stg = getStage();
      for (let depth = 0; cur && cur !== stg && depth < 64; depth++) {
        const pp = getPerspectiveProjectionForDisplay(cur);
        if (pp) {
          const c = pp.projectionCenter;
          return { focalLength: pp.focalLength, cx: c.x, cy: c.y };
        }
        cur = cur.parent;
      }
      return { focalLength: 480.245, cx: 250, cy: 250 };
    }

    function getRelativeMatrix3D(target, relativeTo) {
      if (!relativeTo || typeof relativeTo !== 'object') {
        throw new TypeError('Transform.getRelativeMatrix3D requires a non-null DisplayObject relativeTo.');
      }
      const targetChain = buildChainRootToLeaf(target);
      const relChain = relativeTo === getStage() ? [] : buildChainRootToLeaf(relativeTo);
      const raw = JSON.parse(
        kcall(() => kernel.relative_matrix_3d_json(JSON.stringify(targetChain), JSON.stringify(relChain)))
      );
      const M3D = geometry3d?.Matrix3D;
      return M3D ? new M3D(raw) : null;
    }

    function local3DToGlobal(target, point3d) {
      if (!geometry3d?.Vector3D?.__nfIs?.(point3d)) {
        throw new TypeError('DisplayObject.local3DToGlobal requires a flash.geom.Vector3D.');
      }
      const chain = buildChainRootToLeaf(target);
      const proj = activeProjectionFor(target);
      const rec = get3dPose(target);
      const focal = rec.is3D || point3d.z !== 0 ? proj.focalLength : 0;
      const [sx, sy] = JSON.parse(
        kcall(() =>
          kernel.local3d_to_global_json(
            JSON.stringify(chain),
            point3d.x,
            point3d.y,
            point3d.z,
            focal,
            proj.cx,
            proj.cy
          )
        )
      );
      return new geometry.Point(sx, sy);
    }

    function globalToLocal3D(target, point) {
      if (!geometry?.Point?.__nfIs?.(point)) {
        throw new TypeError('DisplayObject.globalToLocal3D requires a flash.geom.Point.');
      }
      const chain = buildChainRootToLeaf(target);
      const proj = activeProjectionFor(target);
      const rec = get3dPose(target);
      const focal = rec.is3D ? proj.focalLength : 0;
      const [lx, ly, lz] = JSON.parse(
        kcall(() =>
          kernel.global_to_local3d_json(
            JSON.stringify(chain),
            point.x,
            point.y,
            focal,
            proj.cx,
            proj.cy
          )
        )
      );
      return new geometry3d.Vector3D(lx, ly, lz);
    }

    function displayGet(target, name) {
      switch (name) {
        case 'z': return get3dPose(target).z;
        case 'scaleZ': return get3dPose(target).scaleZ;
        case 'rotationX': return get3dPose(target).rotationX;
        case 'rotationY': return get3dPose(target).rotationY;
        case 'rotationZ': return Number(target.rotation) || 0;
        case 'blendShader': return blendShaderMap.get(target)?.shader ?? null;
        case 'needsSoftKeyboard': return Boolean(softKeyboardMap.get(target)?.needs);
        case 'softKeyboardInputAreaOfInterest': {
          const r = softKeyboardMap.get(target)?.area || null;
          return r && geometry?.Rectangle ? new geometry.Rectangle(r.x, r.y, r.width, r.height) : null;
        }
        default: return undefined;
      }
    }

    function displaySet(target, name, value) {
      if (['z', 'scaleZ', 'rotationX', 'rotationY', 'rotationZ'].includes(name)) {
        const n = Number(value);
        if (!Number.isFinite(n)) throw argError(`ArgumentError: DisplayObject.${name} requires a finite Number.`);
        const rec = get3dPose(target);
        rec.is3D = true;
        rec.customMatrix3D = null;
        if (name === 'z') rec.z = n;
        else if (name === 'scaleZ') rec.scaleZ = n;
        else if (name === 'rotationX') rec.rotationX = n;
        else if (name === 'rotationY') rec.rotationY = n;
        else if (name === 'rotationZ') target.rotation = n;
        return true;
      }
      if (name === 'blendShader') {
        if (value !== null && value !== undefined && !globalShaderStates.has(value)) {
          throw new TypeError('DisplayObject.blendShader requires a flash.display.Shader instance.');
        }
        if (value === null || value === undefined) {
          blendShaderMap.delete(target);
        } else {
          blendShaderMap.set(target, { shader: value, active: false });
        }
        return true;
      }
      if (name === 'needsSoftKeyboard') {
        const sk = softKeyboardMap.get(target) || { needs: false, area: null };
        sk.needs = Boolean(value);
        softKeyboardMap.set(target, sk);
        return true;
      }
      if (name === 'softKeyboardInputAreaOfInterest') {
        if (value !== null && value !== undefined && !geometry?.Rectangle?.__nfIs?.(value)) {
          throw new TypeError('InteractiveObject.softKeyboardInputAreaOfInterest requires a Rectangle or null.');
        }
        const sk = softKeyboardMap.get(target) || { needs: false, area: null };
        sk.area = value ? { x: value.x, y: value.y, width: value.width, height: value.height } : null;
        softKeyboardMap.set(target, sk);
        return true;
      }
      return false;
    }

    function hasBlendShader(target) {
      return Boolean(blendShaderMap.get(target)?.shader);
    }

    function setBlendModeShaderActive(target, active) {
      const entry = blendShaderMap.get(target);
      if (entry) entry.active = Boolean(active);
    }

    function isBlendModeShaderActive(target) {
      return Boolean(blendShaderMap.get(target)?.active);
    }

    function requestSoftKeyboard(target) {
      if (!target) return false;
      const stg = getStage();
      const sw = stg ? stg.stageWidth : 550;
      const sh = stg ? stg.stageHeight : 400;
      const kh = Math.min(220, Math.floor(sh * 0.4));
      activeSoftKeyboardRect = { x: 0, y: sh - kh, width: sw, height: kh };
      return true;
    }

    function getSoftKeyboardRect() {
      const R = geometry?.Rectangle;
      const r = activeSoftKeyboardRect || { x: 0, y: 0, width: 0, height: 0 };
      return R ? new R(r.x, r.y, r.width, r.height) : { ...r };
    }

    function MorphShape() {
      if (!new.target) {
        throw new TypeError('Class MorphShape cannot be called as a function.');
      }
      if (new.target !== MorphShape) {
        throw new TypeError('MorphShape is final and cannot be subclassed.');
      }
      const ref = JSON.parse(player.create_display('morph_shape'));
      return proxyForRef(ref);
    }
    tagType(MorphShape, 'MorphShape', v => {
      try { return getDisplay()?.descriptor(v)?.owner === 'morph_shape'; } catch (_) { return false; }
    });

    function AVM1Movie() {
      if (!new.target) {
        throw new TypeError('Class AVM1Movie cannot be called as a function.');
      }
      if (new.target !== AVM1Movie) {
        throw new TypeError('AVM1Movie is final and cannot be subclassed.');
      }
      const ref = JSON.parse(player.create_display('avm1_movie'));
      return proxyForRef(ref);
    }
    tagType(AVM1Movie, 'AVM1Movie', v => {
      try { return getDisplay()?.descriptor(v)?.owner === 'avm1_movie'; } catch (_) { return false; }
    });

    function reset() {
      kernel.reset();
      stage3DList = null;
      stageVideoList = null;
      pose3DMap = new WeakMap();
      projMap = new WeakMap();
      blendShaderMap = new WeakMap();
      softKeyboardMap = new WeakMap();
      activeSoftKeyboardRect = null;
      generation++;
    }

    function dispose() {
      if (disposed) return;
      reset();
      disposed = true;
    }

    const builtins = {
      // flash.display
      'flash.display.ActionScriptVersion': ActionScriptVersion,
      'flash.display.SWFVersion': SWFVersion,
      'flash.display.ColorCorrection': ColorCorrection,
      'flash.display.ColorCorrectionSupport': ColorCorrectionSupport,
      'flash.display.MorphShape': MorphShape,
      'flash.display.AVM1Movie': AVM1Movie,
      'flash.display.Shader': Shader,
      'flash.display.ShaderData': ShaderData,
      'flash.display.ShaderInput': ShaderInput,
      'flash.display.ShaderParameter': ShaderParameter,
      'flash.display.ShaderParameterType': ShaderParameterType,
      'flash.display.ShaderPrecision': ShaderPrecision,
      'flash.display.ShaderJob': ShaderJob,
      'flash.display.GraphicsShaderFill': GraphicsShaderFill,
      'flash.display.Stage3D': Stage3D,

      // flash.display3D & textures
      'flash.display3D.Context3D': Context3D,
      'flash.display3D.Context3DBlendFactor': Context3DBlendFactor,
      'flash.display3D.Context3DBufferUsage': Context3DBufferUsage,
      'flash.display3D.Context3DClearMask': Context3DClearMask,
      'flash.display3D.Context3DCompareMode': Context3DCompareMode,
      'flash.display3D.Context3DFillMode': Context3DFillMode,
      'flash.display3D.Context3DMipFilter': Context3DMipFilter,
      'flash.display3D.Context3DProfile': Context3DProfile,
      'flash.display3D.Context3DProgramType': Context3DProgramType,
      'flash.display3D.Context3DRenderMode': Context3DRenderMode,
      'flash.display3D.Context3DStencilAction': Context3DStencilAction,
      'flash.display3D.Context3DTextureFilter': Context3DTextureFilter,
      'flash.display3D.Context3DTextureFormat': Context3DTextureFormat,
      'flash.display3D.Context3DTriangleFace': Context3DTriangleFace,
      'flash.display3D.Context3DVertexBufferFormat': Context3DVertexBufferFormat,
      'flash.display3D.Context3DWrapMode': Context3DWrapMode,
      'flash.display3D.IndexBuffer3D': IndexBuffer3D,
      'flash.display3D.Program3D': Program3D,
      'flash.display3D.VertexBuffer3D': VertexBuffer3D,
      'flash.display3D.textures.TextureBase': TextureBase,
      'flash.display3D.textures.Texture': Texture,
      'flash.display3D.textures.CubeTexture': CubeTexture,
      'flash.display3D.textures.RectangleTexture': RectangleTexture,
      'flash.display3D.textures.VideoTexture': VideoTexture,

      // flash.text.engine
      'flash.text.engine.BreakOpportunity': BreakOpportunity,
      'flash.text.engine.CFFHinting': CFFHinting,
      'flash.text.engine.ContentElement': ContentElement,
      'flash.text.engine.DigitCase': DigitCase,
      'flash.text.engine.DigitWidth': DigitWidth,
      'flash.text.engine.EastAsianJustifier': EastAsianJustifier,
      'flash.text.engine.ElementFormat': ElementFormat,
      'flash.text.engine.FontDescription': FontDescription,
      'flash.text.engine.FontLookup': FontLookup,
      'flash.text.engine.FontMetrics': FontMetrics,
      'flash.text.engine.FontPosture': FontPosture,
      'flash.text.engine.FontWeight': FontWeight,
      'flash.text.engine.GraphicElement': GraphicElement,
      'flash.text.engine.GroupElement': GroupElement,
      'flash.text.engine.JustificationStyle': JustificationStyle,
      'flash.text.engine.Kerning': Kerning,
      'flash.text.engine.LigatureLevel': LigatureLevel,
      'flash.text.engine.LineJustification': LineJustification,
      'flash.text.engine.RenderingMode': RenderingMode,
      'flash.text.engine.SpaceJustifier': SpaceJustifier,
      'flash.text.engine.TabAlignment': TabAlignment,
      'flash.text.engine.TabStop': TabStop,
      'flash.text.engine.TextBaseline': TextBaseline,
      'flash.text.engine.TextBlock': TextBlock,
      'flash.text.engine.TextElement': TextElement,
      'flash.text.engine.TextJustifier': TextJustifier,
      'flash.text.engine.TextLine': TextLine,
      'flash.text.engine.TextLineCreationResult': TextLineCreationResult,
      'flash.text.engine.TextLineMirrorRegion': TextLineMirrorRegion,
      'flash.text.engine.TextLineValidity': TextLineValidity,
      'flash.text.engine.TextRotation': TextRotation,
      'flash.text.engine.TypographicCase': TypographicCase,

      // flash.text.ime
      'flash.text.ime.CompositionAttributeRange': CompositionAttributeRange,
      'flash.text.ime.IIMEClient': IIMEClient,

      // Fallbacks for sibling Wave 5 types if not already registered
      'flash.events.ShaderEvent': FallbackShaderEvent,
      'flash.events.UncaughtErrorEvents': FallbackUncaughtErrorEvents,
      'flash.media.StageVideo': FallbackStageVideo,
    };

    return Object.freeze({
      builtins,
      setVectorFactory,
      getStage3Ds,
      getStageVideos,
      getSoftKeyboardRect,
      requestSoftKeyboard,
      textLineBounds,
      textLineGet,
      textLineSet,
      textLineMethods,
      displayGet,
      displaySet,
      hasBlendShader,
      setBlendModeShaderActive,
      isBlendModeShaderActive,
      getMatrix3DForDisplay,
      setMatrix3DForDisplay,
      getPerspectiveProjectionForDisplay,
      setPerspectiveProjectionForDisplay,
      getRelativeMatrix3D,
      local3DToGlobal,
      globalToLocal3D,
      rasterizeShaderToBitmapData,
      GraphicsShaderFill,
      Shader,
      FallbackUncaughtErrorEvents,
      createMorphShape: () => proxyForRef(JSON.parse(player.create_display('morph_shape'))),
      createAvm1Movie: () => proxyForRef(JSON.parse(player.create_display('avm1_movie'))),
      reset,
      dispose,
    });
  }

  const api = Object.freeze({
    create,
    inspectShaderForFilter,
    ActionScriptVersion,
    SWFVersion,
    ColorCorrection,
    ColorCorrectionSupport,
    ShaderPrecision,
    ShaderParameterType,
  });

  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else global.NeoFlashFte3D = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
