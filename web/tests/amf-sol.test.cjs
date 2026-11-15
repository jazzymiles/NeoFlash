// Native SOL exchange and the browser SharedObject boundary share one family.
// External files retain their pinned provenance beside the Rust codec fixtures.
const test = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const AMF = require('../amf.js');
const Binary = require('../byte-array.js');
const XML = require('../xml.js');
const Storage = require('../storage.js');
const Events = require('../events.js');
const root = resolve(__dirname, '../..');
const fixture = name => readFileSync(resolve(root, 'crates/neoflash-core/tests/fixtures/sol', name));

async function harness(binaryOptions = {}) {
  const { runtimeWasm: wasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');
  const binary = Binary.create({ wasm, ...binaryOptions }), saved = new Map(), errors = [];
  let xml;
  const codecs = { binary, encode: wasm.amf_encode, decode: wasm.amf_decode };
  const amf = AMF.create({ ...codecs, encodeSol: wasm.sol_encode, decodeSol: wasm.sol_decode, getXML: () => xml });
  // The runtime initializes XML after AMF; the hook must remain lazy.
  xml = XML.create({ parse: wasm.parse_xml_json, parseLegacy: wasm.parse_as2_xml_json });
  class QuietChannel { postMessage() {} close() {} }
  const storage = Storage.create({ amf, events: Events, url: 'https://example.test/app/index.html',
    BroadcastChannel: QuietChannel, onError: error => errors.push(error),
    storage: { getItem: key => saved.get(key) ?? null, setItem: (key, value) => saved.set(key, value), removeItem: key => saved.delete(key) } });
  return { wasm, binary, xml, amf, codecs, storage, saved, errors,
    close() { storage.dispose(); xml.dispose(); binary.dispose(); } };
}

test('SOL: independent AMF0/AMF3 files retain dates, XML, sparse keys and typed aliases', async () => {
  const h = await harness();
  try {
    const old = h.storage.LegacySharedObject.fromSolBytes(fixture('amf0_shared_object.sol'), '/');
    assert.ok(old, h.errors.map(String).join('\n'));
    assert.equal(old.data.saved, true); assert.equal(old.data.num, 10); assert.equal(old.data.str, 'hello');
    assert.equal(old.data.array.length, 5); assert.equal(old.data.array[4], 'elem4');
    assert.equal(Object.hasOwn(old.data.array, '1'), false);
    assert.equal(old.data.array.prop, 'property'); assert.equal(old.data.array[-1], 'elem negative one');
    assert.deepEqual(old.data.denseArray, [1, 2, 3]); assert.equal(old.data.date.getTime(), 2147483647);
    assert.equal(h.xml.LegacyXML.__nfIs(old.data.testxml), true);
    assert.equal(old.data.testxml.toString(), '<test>Test</test>');
    assert.equal(h.saved.size, 0, 'file import alone does not persist anything');
    assert.equal(old.flush(), true);
    h.storage.reset();
    const reopened = h.storage.LegacySharedObject.getLocal('RuffleTest', '/');
    assert.equal(h.xml.LegacyXML.__nfIs(reopened.data.testxml), true);
    assert.equal(reopened.data.date.getTime(), 2147483647);

    const modern = h.storage.SharedObject.fromSolBytes(fixture('amf3_shared_object.sol'), '/');
    assert.equal(modern.objectEncoding, 3); assert.equal(modern.data.props[0], true);
    assert.equal(modern.data.props[1], 'hello'); assert.equal(modern.data.props[5], -5.1);
    assert.equal(modern.data.props[6][4], 'elem4'); assert.equal(Object.hasOwn(modern.data.props[6], '1'), false);
    assert.deepEqual(modern.data.props[7], [1, 2, 3]);
    assert.equal(modern.data.props[8].aPropertyThatIsNotANumber, 'strange property');
    assert.equal(modern.data.props[9].getTime(), 2147483647);
    assert.equal(Object.hasOwn(modern.data, 'hidden'), false);

    const arrays = h.amf.decodeSol(fixture('amf0_arrays.sol')).value;
    assert.deepEqual(arrays.denseArray, ['dense_0', 'dense_1']);
    assert.equal(arrays.sparseArray[5], 'sparse_5');
    assert.equal(Object.hasOwn(arrays.sparseArray, '1'), false, 'the independent file uses ECMA array keys, despite its fixture name');
    assert.equal(arrays.sparseArray[1], undefined);
    assert.equal(arrays.mixedArray.custom_prop, 'custom_value');
    assert.equal(Array.isArray(arrays.fakeArray), false);
    assert.equal(arrays.fakeArray.length, 1);
    const strict = h.amf.decode(Uint8Array.of(10, 0, 0, 0, 3, 6, 6, 1, 1), 0).value;
    assert.deepEqual(strict, [undefined, undefined, true]);
    assert.equal(Object.hasOwn(strict, '1'), true, 'AMF0 strict-array gaps are explicit Undefined');

    class Registered { constructor() { this.fromConstructor = true; } }
    h.amf.registerClassAlias('com.test.RegisteredClass', Registered);
    const typed = h.amf.decodeSol(fixture('amf0_typed_objects.sol')).value;
    assert.ok(typed.objRegistered instanceof Registered); assert.equal(typed.objRegistered.foo, 'baz');
    assert.equal(typed.objPlain.foo, 'plain'); assert.equal(typed.objUnregistered.foo, 'bar');
    assert.equal(typed.objMismatched.foo, 'qux', 'unregistered aliases remain ordinary data');
    assert.equal(typed.objForgedConstructor instanceof Registered, true, 'the external file explicitly stores this alias');
    assert.equal(typed.objForgedConstructor.foo, 'forged_ctor');
    assert.equal(typed.objForgedProto instanceof Registered, false, 'the external file stores this as an anonymous record');
    assert.equal(typed.objDoubleRegistered.foo, 'double');
  } finally { h.close(); }
});

test('SOL: shared graph references, sparse arrays, XML types and bytes cross each native encoding', async () => {
  const h = await harness();
  try {
    let constructorReads = 0;
    const ordinary = { score: 19 };
    Object.defineProperty(ordinary, 'constructor', { get() { constructorReads++; throw new Error('Must not run'); } });
    class Hero { constructor() { this.rank = 0; } get title() { return this._title; } set title(v) { this._title = v; } }
    h.amf.registerClassAlias('save.Hero', Hero);
    for (const mode of [0, 3]) {
      const shared = new Hero(); shared.rank = 7; shared.title = 'pilot'; shared.self = shared;
      const sparse = []; sparse[0] = undefined; sparse[5] = 'fifth'; sparse.tag = 'inventory'; sparse.self = sparse;
      const byteArray = h.binary.fromBytes(Uint8Array.of(0, 127, 255));
      const xml = new h.xml.XML('<save rank="7"/>'), document = new h.xml.XMLDocument('<legacy>data</legacy>');
      const data = { a: shared, b: shared, ordinary, sparse, bytes: byteArray, bytesAgain: byteArray,
        when: new Date(2147483647), invalidDate: new Date(NaN), xml, xmlAgain: xml, document, documentAgain: document,
        unset: undefined, nan: NaN, infinity: Infinity, minusZero: -0 };
      data.self = data;
      const wire = h.amf.encodeSol(`graphs-${mode}`, data, mode), decoded = h.amf.decodeSol(wire);
      assert.equal(decoded.consumed, wire.length); assert.equal(decoded.encoding, mode);
      const value = decoded.value;
      assert.ok(value.a instanceof Hero); assert.equal(value.a, value.b); assert.equal(value.a.self, value.a);
      assert.equal(value.a.rank, 7); assert.equal(value.a.title, 'pilot');
      assert.equal(value.sparse.length, 6); assert.equal(Object.hasOwn(value.sparse, '0'), true);
      assert.equal(value.sparse[0], undefined); assert.equal(Object.hasOwn(value.sparse, '1'), false);
      assert.equal(value.sparse[5], 'fifth'); assert.equal(value.sparse.tag, 'inventory'); assert.equal(value.sparse.self, value.sparse);
      assert.equal(value.bytes, value.bytesAgain); assert.deepEqual(h.binary.toBytes(value.bytes), Uint8Array.of(0, 127, 255));
      assert.equal(value.when.getTime(), 2147483647); assert.ok(Number.isNaN(value.invalidDate.getTime()));
      assert.equal(h.xml.XML.__nfIs(value.xml), true); assert.equal(value.xml, value.xmlAgain);
      assert.equal(value.xml.toXMLString(), '<save rank="7"/>');
      assert.equal(h.xml.XMLDocument.__nfIs(value.document), true); assert.equal(value.document.toString(), '<legacy>data</legacy>');
      if (mode === 3) assert.equal(value.document, value.documentAgain);
      else assert.notEqual(value.document, value.documentAgain, 'AMF0 XMLDocument is an inline wire value');
      assert.equal(Object.hasOwn(value, 'unset'), true); assert.ok(Number.isNaN(value.nan));
      assert.equal(value.infinity, Infinity); assert.ok(Object.is(value.minusZero, -0));
      assert.notEqual(value.self, value, 'the SOL container root has no reference-table slot');
      assert.equal(value.self.self, value.self, 'nested serialized root references still form a coherent cycle');
      assert.equal(value.self.a, value.a);
      const legacy = h.amf.decodeSol(wire, { legacyXML: true }).value;
      assert.equal(h.xml.LegacyXML.__nfIs(legacy.document), true);
      assert.equal(h.xml.XML.__nfIs(legacy.xml), true);
      const trailing = new Array(12); trailing[0] = 'kept';
      const trimmed = h.amf.decodeSol(h.amf.encodeSol('trailing', { trailing }, mode)).value.trailing;
      assert.equal(trimmed.length, 1, 'trailing absent indices have no materialized wire fields');
    }
    assert.equal(constructorReads, 0);
    assert.throws(() => h.amf.encodeSol('xml-list', { value: new h.xml.XMLList('<a/><b/>') }), /XMLList/);
    const noNative = AMF.create(h.codecs);
    assert.throws(() => noNative.encodeSol('missing', {}), /Rebuild.*WASM/);
    assert.throws(() => noNative.decodeSol(Uint8Array.of(0)), /Rebuild.*WASM/);
  } finally { h.close(); }
});

test('SOL: failed imports are atomic, AS2 data identity is retained and stale callbacks cannot commit', async () => {
  const h = await harness({ maxArrays: 3 });
  try {
    const { SharedObject, LegacySharedObject } = h.storage;
    const modern = SharedObject.getLocal('atomic'), legacy = LegacySharedObject.getLocal('atomic');
    legacy.data.first = 'original'; Object.defineProperty(legacy.data, 'fixed', { value: 2, enumerable: true });
    modern.objectEncoding = 0; assert.equal(legacy.flush(), true);
    const priorData = legacy.data, priorFields = Object.getOwnPropertyDescriptors(priorData), persisted = new Map(h.saved);
    const replacement = h.amf.encodeSol('atomic', { next: 3 }, 3);
    assert.throws(() => legacy.importSolBytes(replacement), /nonconfigurable/);
    assert.equal(LegacySharedObject.fromSolBytes(replacement), null);
    assert.equal(legacy.data, priorData); assert.deepEqual(Object.getOwnPropertyDescriptors(priorData), priorFields);
    assert.equal(modern.objectEncoding, 0); assert.deepEqual(h.saved, persisted);

    const sealed = LegacySharedObject.getLocal('nonextensible'); sealed.data.keep = 1; Object.preventExtensions(sealed.data);
    assert.throws(() => sealed.importSolBytes(h.amf.encodeSol('nonextensible', { keep: 2, added: 3 })), /nonextensible/);
    assert.deepEqual(sealed.data, { keep: 1 });
    sealed.importSolBytes(h.amf.encodeSol('nonextensible', { keep: 2 })); assert.equal(sealed.data.keep, 2);
    const safe = LegacySharedObject.getLocal('safe'), safeData = safe.data; let setters = 0;
    Object.defineProperty(safeData, 'value', { configurable: true, set() { setters++; } });
    const ownProto = { value: 8 }; Object.defineProperty(ownProto, '__proto__', { value: { injected: true }, enumerable: true });
    safe.importSolBytes(h.amf.encodeSol('safe', ownProto));
    assert.equal(safe.data, safeData); assert.equal(setters, 0); assert.equal(safe.data.value, 8);
    assert.equal(Object.getPrototypeOf(safe.data), Object.prototype); assert.equal(Object.hasOwn(safe.data, '__proto__'), true);
    assert.equal(safe.data.__proto__.injected, true); assert.equal(safe.data.injected, undefined);

    const sourceBytes = h.binary.fromBytes(Uint8Array.of(4, 5)), sourceXML = new h.xml.XML('<inventory/>');
    const resourceWire = h.amf.encodeSol('atomic', { bytes: sourceBytes, xml: sourceXML });
    const xmlBefore = h.xml.stats();
    for (let attempt = 0; attempt < 8; attempt++) {
      assert.throws(() => legacy.importSolBytes(resourceWire), /nonconfigurable/);
      assert.deepEqual(h.xml.stats(), xmlBefore, 'failed commit releases newly decoded XML nodes');
    }
    const importedResources = SharedObject.getLocal('resources'); importedResources.importSolBytes(resourceWire);
    assert.deepEqual(h.binary.toBytes(importedResources.data.bytes), Uint8Array.of(4, 5), 'repeated rejected imports do not consume ByteArray instance budget');
    const badXML = h.wasm.sol_encode(3, 'atomic', JSON.stringify({ root: { type: 'ref', value: 0 }, nodes: [
      { type: 'object', entries: [['first', { type: 'ref', value: 1 }], ['bad', { type: 'ref', value: 2 }]] },
      { type: 'xml', text: '<ok/>' }, { type: 'xml_document', text: '<x>' + 'x'.repeat(1024 * 1024) + '</x>' },
    ] }));
    const afterSuccess = h.xml.stats();
    assert.throws(() => modern.importSolBytes(badXML), /XML|limit|MiB|bytes/i);
    assert.deepEqual(h.xml.stats(), afterSuccess, 'a later parser failure also releases the first XML and the empty document');

    const before = modern.data, header = Uint8Array.from(replacement); header[10] = 1;
    const malformed = [replacement.subarray(0, replacement.length - 1), Uint8Array.from([...replacement, 0]), header];
    for (const wire of malformed) {
      assert.throws(() => modern.importSolBytes(wire), /SOL|AMF|trunc|trailing|header|length|version|reserved/i);
      assert.equal(modern.data, before); assert.equal(modern.objectEncoding, 0); assert.deepEqual(h.saved, persisted);
    }
    for (const name of ['../escape', 'bad name', 'x'.repeat(1025), '\ud800']) assert.throws(() => SharedObject.getLocal(name), /name|Unicode/);
    for (const path of ['/app/../app', '/app/%2e%2e/app', '/app/%', '/app/\ud800', '/other', '/'.repeat(4097)]) {
      assert.throws(() => SharedObject.fromSolBytes(replacement, path), /path|Path|Unicode/);
      assert.equal(modern.data, before); assert.deepEqual(h.saved, persisted);
    }
    assert.throws(() => h.amf.encodeSol('\ud800', {}), /Unicode/);
    assert.throws(() => h.amf.encodeSol('é'.repeat(40000), {}), /65535/);

    let hydrate = () => {};
    class Callback { constructor() { hydrate(); this.rank = 4; } }
    h.amf.registerClassAlias('save.Callback', Callback);
    const callbackWire = h.amf.encodeSol('atomic', { callback: new Callback() });
    const sentinel = { authored: true };
    hydrate = () => { throw sentinel; };
    assert.throws(() => modern.importSolBytes(callbackWire), error => error === sentinel);
    assert.equal(modern.data, before); assert.deepEqual(h.saved, persisted);
    hydrate = () => { h.storage.reset(); };
    assert.throws(() => modern.importSolBytes(callbackWire), /reset|changed/);
    assert.throws(() => modern.data, /session/);
    assert.deepEqual(h.saved, persisted, 'Restart during hydration cannot replace persisted data');
    hydrate = () => {};
    const retried = SharedObject.fromSolBytes(callbackWire);
    assert.equal(retried.data.callback.rank, 4); assert.equal(retried.flush(), 'flushed');
    h.storage.reset(); assert.equal(SharedObject.getLocal('atomic').data.callback.rank, 4);

    let reject = () => { throw sentinel; }, survivorBytes, survivorXML;
    class Reject { set value(value) { reject(value); } }
    h.amf.registerClassAlias('save.Reject', Reject);
    const rejecting = Object.create(Reject.prototype);
    Object.defineProperty(rejecting, 'value', { value: sourceBytes, enumerable: true });
    const rejectedWire = h.amf.encodeSol('setter', { xml: sourceXML, reject: rejecting });
    const beforeSetter = h.xml.stats();
    for (let attempt = 0; attempt < 5; attempt++) assert.throws(() => h.amf.decodeSol(rejectedWire), error => error === sentinel);
    assert.deepEqual(h.xml.stats(), beforeSetter);
    reject = () => {
      h.binary.reset(); h.xml.reset();
      survivorBytes = h.binary.fromBytes(Uint8Array.of(9)); survivorXML = new h.xml.XML('<new-session/>');
      throw sentinel;
    };
    assert.throws(() => h.amf.decodeSol(rejectedWire), error => error === sentinel);
    assert.deepEqual(h.binary.toBytes(survivorBytes), Uint8Array.of(9));
    assert.equal(survivorXML.toXMLString(), '<new-session/>', 'old transaction cleanup leaves new-session resources intact');
  } finally { h.close(); }
});
