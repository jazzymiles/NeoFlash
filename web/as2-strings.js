/* Thin value/UTF-16 transport for NeoFlash's shared Rust AS2 String functions.
 * String algorithms, SWF case tables and URL codecs live in neoflash-core.
 * This adapter only preserves existing coercion order and marshals values.
 */
(function(global) {
  'use strict';
  const LIMIT = 1000000;
  const toString = String, toNumber = Number;
  function units(value) {
    if (value.length > LIMIT) throw new RangeError('AS2 String exceeds 1000000 UTF-16 code units.');
    return Array.from({length: value.length}, (_, index) => value.charCodeAt(index));
  }
  function text(value) {
    let result = '';
    for (let index = 0; index < value.length; index += 8192) result += String.fromCharCode(...value.slice(index, index + 8192));
    return result;
  }
  function create(options) {
    if (typeof options?.execute !== 'function') throw new TypeError('AS2 String requires the shared Rust WASM service. Re-export this project.');
    function send(request) {
      let response;
      try { response = options.execute(JSON.stringify(request)); }
      catch (error) { throw new TypeError(String(error)); }
      const result = JSON.parse(response);
      switch (result.kind) {
        case 'string': return text(result.value);
        case 'strings': return result.value.map(text);
        case 'number': return toNumber(result.value);
        case 'undefined': return undefined;
        default: throw new TypeError('Invalid AS2 String result from Rust.');
      }
    }
    function invoke(operation, receiver, args) {
      if (args.length > LIMIT) throw new RangeError('AS2 String exceeds 1000000 arguments.');
      const request = {operation, receiver: units(receiver), strings: [], numbers: [], arg_count: args.length, undefined: []};
      for (let i = 0; i < Math.min(args.length, 2); i++) if (args[i] === undefined) request.undefined.push(i);
      const numeric = value => toString(toNumber(value));
      switch (operation) {
        case 'charAt': case 'charCodeAt': request.numbers.push(numeric(args[0])); break;
        case 'concat': for (const value of args) request.strings.push(units(toString(value))); break;
        case 'indexOf': case 'lastIndexOf':
          if (args.length) request.strings.push(units(toString(args[0])));
          if (args.length > 1 && args[1] !== undefined) request.numbers.push(numeric(args[1]));
          break;
        case 'slice': case 'substring': case 'substr':
          if (args.length) request.numbers.push(numeric(args[0]));
          if (args.length > 1 && args[1] !== undefined) request.numbers.push(numeric(args[1]));
          break;
        case 'split':
          // The VM converts limit before delimiter; preserve observable calls.
          if (args.length > 1 && args[1] !== undefined) request.numbers.push(numeric(args[1]));
          if (args.length && args[0] !== undefined) {
            if (args[0] !== null && (typeof args[0] === 'object' || typeof args[0] === 'function')) throw new TypeError('AS2 split requires a primitive delimiter; native RegExp and custom pattern objects are unsupported.');
            request.strings.push(units(toString(args[0])));
          }
          break;
        case 'fromCharCode': {
          // Coercion after a NUL must never execute. Ask Rust to convert each
          // number, then assemble its returned units without reimplementing it.
          const parts = [];
          for (const value of args) {
            const part = send({...request, arg_count: 1, numbers: [numeric(value)]});
            if (part.length === 0) break;
            parts.push(part);
          }
          return parts.join('');
        }
        case 'escape': case 'unescape':
          request.receiver = args.length ? units(toString(args[0])) : [];
          break;
        case 'global.chr': case 'global.mbchr': request.numbers.push(numeric(args[0])); break;
        case 'global.ord': case 'global.mbord': case 'global.length': case 'global.mblength':
          request.receiver = units(toString(args[0])); break;
        case 'global.substring': case 'global.mbsubstring': {
          // The AVM1 opcode pops/converts count, then start, then source.
          const count = numeric(args[2]), start = numeric(args[1]);
          request.receiver = units(toString(args[0]));
          request.numbers.push(start, count);
          break;
        }
      }
      return send(request);
    }
    return Object.freeze({invoke});
  }
  global.NeoFlashAS2Strings = Object.freeze({create});
  if (typeof module !== 'undefined' && module.exports) module.exports = global.NeoFlashAS2Strings;
})(globalThis);
