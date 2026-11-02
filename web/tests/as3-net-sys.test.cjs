// Comprehensive end-to-end verification of ActionScript 3 Area 8 (Network, System,
// Globalization, Concurrency, Crypto, Desktop Clipboard, ExternalInterface, DRM,
// Printing, Profiler, Sampler, Security, Sensors & Utils) backed by As3NetSysKernel.
const assert = require('node:assert/strict');
const test = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const NeoFlashEvents = require('../events.js');
const NeoFlashByteArray = require('../byte-array.js');
const NeoFlashAMF = require('../amf.js');
const NeoFlashGeom = require('../geom.js');
const NeoFlashGeom3D = require('../geom3d.js');
const NeoFlashXML = require('../xml.js');
const NeoFlashNet = require('../net.js');
const NeoFlashStorage = require('../storage.js');
const NeoFlashVideo = require('../video.js');
const NeoFlashExternal = require('../external.js');
const NeoFlashTimers = require('../timers.js');
const NeoFlashDate = require('../date.js');
const NeoFlashNumberFormat = require('../number-format.js');
const NeoFlashAS3NetSys = require('../as3-net-sys.js');

const runtimeDir = path.resolve(__dirname, '../../target/web-runtime');
const wasmContext = vm.createContext({
  TextEncoder,
  TextDecoder,
  WebAssembly,
  console,
  URL,
  Request,
  Response,
});
vm.runInContext(fs.readFileSync(path.join(runtimeDir, 'neoflash_web.js'), 'utf8'), wasmContext);
const wasm = vm.runInContext('NeoFlashWasm', wasmContext);
wasmContext.wasmBytes = fs.readFileSync(path.join(runtimeDir, 'neoflash_web_bg.wasm'));
const ready = vm.runInContext('NeoFlashWasm({module_or_path: wasmBytes})', wasmContext);

function createArea8Harness(customOptions = {}) {
  const queue = [];
  function flushTasks() {
    let guard = 0;
    while (queue.length > 0) {
      if (++guard > 500) throw new Error('Async task queue did not settle');
      const task = queue.shift();
      task();
    }
  }

  let geom2d;
  const geom3d = NeoFlashGeom3D.create(wasm, { getPoint: () => geom2d.Point });
  geom2d = NeoFlashGeom.create(wasm, { getVector3D: () => geom3d.Vector3D });

  const binary = NeoFlashByteArray.create({ wasm });
  const eventFamilies = NeoFlashEvents.createFamilies(wasm, {
    isByteArray: v => binary.ByteArray.__nfIs(v),
    requestRender: () => {},
  });
  const events = { ...NeoFlashEvents, ...eventFamilies };

  const dummyProject = {
    format_version: 3,
    name: 'Area8 Test App',
    width: 800,
    height: 600,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    resources: customOptions.resources || [
      {
        id: 'res-txt',
        path: 'assets/sample.txt',
        media_type: 'text/plain',
        data_base64: Buffer.from('Hello Area 8 Stream', 'utf8').toString('base64'),
      },
    ],
    symbols: [
      {
        id: 'root',
        name: 'Main',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [
            {
              id: 'l1',
              name: 'Layer 1',
              visible: true,
              locked: false,
              keyframes: [{ frame: 0, elements: [], label: '', tween: null }],
            },
          ],
          scripts: [],
        },
      },
    ],
  };
  const player = new wasm.WebPlayer(JSON.stringify(dummyProject));
  const amf = NeoFlashAMF.create({
    encode: (val, enc) => player.amf_encode(val, enc),
    decode: (bytes, enc) => JSON.parse(player.amf_decode(bytes, enc)),
    binary,
  });
  binary.setObjectCodec(amf);

  const navigations = [];
  const sentRequests = [];
  const net = NeoFlashNet.create({
    wasm,
    events,
    binary,
    resources: dummyProject.resources,
    navigate: (url, windowTarget) => navigations.push({ url, windowTarget }),
    fetch: async (url, init) => {
      sentRequests.push({ url, init });
      return {
        ok: true,
        status: 200,
        url,
        headers: { entries: () => [['content-type', 'text/plain']] },
        arrayBuffer: async () => new TextEncoder().encode('fetched:' + url).buffer,
      };
    },
    schedule: fn => queue.push(fn),
  });

  const storageBacking = new Map();
  const storage = NeoFlashStorage.create({
    events,
    amf,
    storage: {
      getItem: k => (storageBacking.has(k) ? storageBacking.get(k) : null),
      setItem: (k, v) => storageBacking.set(k, String(v)),
      removeItem: k => storageBacking.delete(k),
    },
  });

  const video = NeoFlashVideo.create({
    player,
    wasm,
    events,
    net,
    schedule: fn => queue.push(fn),
  });

  const fscommands = [];
  const hostObject = { id: 'neoflash-player-1' };
  const hostGlobal = {
    DoFSCommand: (cmd, args) => fscommands.push({ cmd, args }),
    echoHost: (a, b) => `${a}:${b}`,
    throwHost: () => {
      throw new Error('host-boom');
    },
    ...(customOptions.hostGlobal || {}),
  };
  const external = NeoFlashExternal.create({
    hostObject,
    hostGlobal,
    onFscommand: (cmd, args) => fscommands.push({ cmd, args }),
  });

  const dateHost = NeoFlashDate.create(wasm);
  dateHost.setStringVectorFactory(values => Array.from(values, String));
  const numberFormat = NeoFlashNumberFormat.create(wasm);
  const xml = NeoFlashXML.create({
    parseLegacy: src => wasm.parse_as2_xml_json(src),
    namespaceLookup: (anc, val, rev) =>
      JSON.parse(wasm.as2_xml_namespace_json(JSON.stringify(anc), val, rev)),
    parse: src => wasm.parse_xml_json(src),
    net,
  });
  const timers = NeoFlashTimers.create({
    events,
    schedule: fn => {
      queue.push(fn);
      return queue.length;
    },
    cancel: () => {},
    now: () => 1250,
  });

  const builtins = {};
  const netSys = NeoFlashAS3NetSys.create({
    wasm,
    events,
    binary,
    amf,
    net,
    video,
    storage,
    external,
    numberFormat,
    getBuiltins: () => builtins,
    getProgram: () => customOptions.program || null,
    getViewportSize: () => [800, 600],
    schedule: fn => queue.push(fn),
  });
  Object.assign(builtins, netSys.builtins);

  return {
    netSys,
    binary,
    amf,
    events,
    net,
    storage,
    video,
    external,
    dateHost,
    numberFormat,
    xml,
    timers,
    player,
    navigations,
    sentRequests,
    fscommands,
    hostObject,
    hostGlobal,
    flushTasks,
    dispose() {
      netSys.dispose();
      player.free();
    },
  };
}

test('flash.concurrent.{Mutex, Condition} locking, recursion, tryLock and Condition wait/notify', async () => {
  await ready;
  const h = createArea8Harness();
  const { Mutex, Condition } = h.netSys;

  assert.equal(Mutex.isSupported, true);
  assert.equal(Condition.isSupported, true);

  const m = new Mutex();
  assert.equal(m.tryLock(), true);
  m.lock(); // recursive lock depth 2
  m.unlock();
  m.unlock(); // depth 0
  assert.throws(() => m.unlock(), /Error #1520/);

  assert.throws(() => new Condition(null), /TypeError/);
  const c = new Condition(m);
  assert.equal(c.mutex, m);
  assert.throws(() => c.wait(0), /Error #1521/); // mutex not locked

  m.lock();
  assert.throws(() => c.wait(-5), /ArgumentError/);
  assert.equal(c.wait(10), true); // finite timeout reports timedOut=true
  assert.equal(c.wait(), false); // default wait reports timedOut=false
  c.notify();
  c.notifyAll();
  m.unlock();

  h.dispose();
});

test('flash.crypto.generateRandomBytes CSPRNG bounds and ByteArray output', async () => {
  await ready;
  const h = createArea8Harness();
  const { generateRandomBytes } = h.netSys;

  assert.throws(() => generateRandomBytes(0), /Error #2004/);
  assert.throws(() => generateRandomBytes(1025), /Error #2004/);

  const ba1 = generateRandomBytes(32);
  const ba2 = generateRandomBytes(32);
  assert.equal(h.binary.ByteArray.__nfIs(ba1), true);
  assert.equal(ba1.length, 32);
  assert.equal(ba2.length, 32);
  let diff = false;
  for (let i = 0; i < 32; i++) {
    if (ba1[i] !== ba2[i]) diff = true;
  }
  assert.equal(diff, true);

  h.dispose();
});

test('flash.desktop.{Clipboard, ClipboardFormats, ClipboardTransferMode} and System.setClipboard', async () => {
  await ready;
  const h = createArea8Harness();
  const { Clipboard, ClipboardFormats, ClipboardTransferMode, System } = h.netSys;

  assert.equal(ClipboardFormats.TEXT_FORMAT, 'air:text');
  assert.equal(ClipboardFormats.HTML_FORMAT, 'air:html');
  assert.equal(ClipboardFormats.RICH_TEXT_FORMAT, 'air:rtf');
  assert.equal(ClipboardTransferMode.ORIGINAL_PREFERRED, 'originalPreferred');
  assert.equal(ClipboardTransferMode.ORIGINAL_ONLY, 'originalOnly');
  assert.equal(ClipboardTransferMode.CLONE_PREFERRED, 'clonePreferred');
  assert.equal(ClipboardTransferMode.CLONE_ONLY, 'cloneOnly');

  const cb = new Clipboard();
  const payload = { score: 42 };
  assert.equal(cb.setData('custom:score', payload, true), true);
  assert.equal(cb.hasFormat('custom:score'), true);
  assert.deepEqual(Array.from(cb.formats), ['custom:score']);

  const orig = cb.getData('custom:score', ClipboardTransferMode.ORIGINAL_ONLY);
  assert.equal(orig, payload);
  const cloned = cb.getData('custom:score', ClipboardTransferMode.CLONE_ONLY);
  assert.deepEqual(cloned, payload);
  assert.notEqual(cloned, payload);

  const nonSer = new Clipboard();
  nonSer.setData('custom:ref', payload, false);
  assert.equal(nonSer.getData('custom:ref', ClipboardTransferMode.CLONE_ONLY), null);
  assert.equal(nonSer.getData('custom:ref', ClipboardTransferMode.CLONE_PREFERRED), payload);

  cb.setDataHandler('custom:lazy', () => 'lazy-value');
  assert.equal(cb.getData('custom:lazy'), 'lazy-value');
  cb.clearData('custom:lazy');
  assert.equal(cb.hasFormat('custom:lazy'), false);
  cb.clear();
  assert.equal(cb.formats.length, 0);

  System.setClipboard('Hello General Clipboard');
  assert.equal(
    Clipboard.generalClipboard.getData(ClipboardFormats.TEXT_FORMAT),
    'Hello General Clipboard'
  );

  h.dispose();
});

test('flash.external.ExternalInterface and flash.globalization full suite + example projects', async () => {
  await ready;
  const h = createArea8Harness();
  const { ExternalInterface } = h.external;
  const {
    LastOperationStatus,
    NationalDigitsType,
    CollatorMode,
    LocaleID,
    CurrencyParseResult,
    CurrencyFormatter,
    Collator,
    StringTools,
  } = h.netSys;
  const { NumberFormatter, NumberParseResult } = h.numberFormat;
  const { DateTimeFormatter, DateTimeStyle, DateTimeNameStyle, DateTimeNameContext } = h.dateHost;

  // ExternalInterface
  assert.equal(ExternalInterface.available, true);
  assert.equal(ExternalInterface.objectID, 'neoflash-player-1');
  assert.equal(ExternalInterface.marshallExceptions, false);
  ExternalInterface.marshallExceptions = true;
  assert.equal(ExternalInterface.marshallExceptions, true);
  ExternalInterface.addCallback('fromActionScript', x => x * 3);
  assert.equal(h.hostObject.fromActionScript(7), 21);
  assert.equal(ExternalInterface.call('echoHost', 'left', 'right'), 'left:right');
  assert.throws(() => ExternalInterface.call('throwHost'), /host-boom/);

  // Constants
  assert.equal(LastOperationStatus.NO_ERROR, 'noError');
  assert.equal(LastOperationStatus.USING_FALLBACK_WARNING, 'usingFallbackWarning');
  assert.equal(LastOperationStatus.USING_DEFAULT_WARNING, 'usingDefaultWarning');
  assert.equal(LastOperationStatus.PARSE_ERROR, 'parseError');
  assert.equal(LastOperationStatus.UNSUPPORTED_ERROR, 'unsupportedError');
  assert.equal(LastOperationStatus.ERROR_CODE_UNKNOWN, 'errorCodeUnknown');
  assert.equal(LastOperationStatus.PATTERN_SYNTAX_ERROR, 'patternSyntaxError');
  assert.equal(LastOperationStatus.MEMORY_ALLOCATION_ERROR, 'memoryAllocationError');
  assert.equal(LastOperationStatus.ILLEGAL_ARGUMENT_ERROR, 'illegalArgumentError');
  assert.equal(LastOperationStatus.BUFFER_OVERFLOW_ERROR, 'bufferOverflowError');
  assert.equal(LastOperationStatus.INVALID_ATTR_VALUE, 'invalidAttrValue');
  assert.equal(LastOperationStatus.NUMBER_OVERFLOW_ERROR, 'numberOverflowError');
  assert.equal(LastOperationStatus.INVALID_CHAR_FOUND, 'invalidCharFound');
  assert.equal(LastOperationStatus.TRUNCATED_CHAR_FOUND, 'truncatedCharFound');
  assert.equal(LastOperationStatus.INDEX_OUT_OF_BOUNDS_ERROR, 'indexOutOfBoundsError');
  assert.equal(LastOperationStatus.PLATFORM_API_FAILED, 'platformAPIFailed');
  assert.equal(LastOperationStatus.UNEXPECTED_TOKEN, 'unexpectedToken');

  assert.equal(NationalDigitsType.EUROPEAN, 0x0030);
  assert.equal(NationalDigitsType.ARABIC_INDIC, 0x0660);
  assert.equal(NationalDigitsType.DEVANAGARI, 0x0966);
  assert.equal(NationalDigitsType.OSMANYA, 0x104a0);
  assert.equal(CollatorMode.SORTING, 'sorting');
  assert.equal(CollatorMode.MATCHING, 'matching');

  // LocaleID
  assert.equal(LocaleID.DEFAULT, 'i-default');
  const loc = new LocaleID('sr_Latn_RS_REVISED@currency=EUR;collation=phonebook');
  assert.equal(loc.name, 'sr-Latn-RS-REVISED');
  assert.equal(loc.getLanguage(), 'sr');
  assert.equal(loc.getScript(), 'Latn');
  assert.equal(loc.getRegion(), 'RS');
  assert.equal(loc.getVariant(), 'REVISED');
  assert.deepEqual(loc.getKeysAndValues(), { currency: 'EUR', collation: 'phonebook' });
  assert.equal(loc.isRightToLeft(), false);
  assert.equal(loc.lastOperationStatus, LastOperationStatus.NO_ERROR);
  assert.equal(new LocaleID('ar-EG').isRightToLeft(), true);
  assert.deepEqual(
    Array.from(LocaleID.determinePreferredLocales(['de-AT', 'ja-JP'], ['en-US', 'de-DE', 'ja-JP'])),
    ['de-DE', 'ja-JP']
  );

  // CurrencyParseResult & CurrencyFormatter
  const cpr0 = new CurrencyParseResult(99.95, 'EUR');
  assert.equal(cpr0.value, 99.95);
  assert.equal(cpr0.currencyString, 'EUR');
  const cf = new CurrencyFormatter('en-US');
  assert.equal(cf.actualLocaleIDName, 'en-US');
  assert.equal(cf.currencyISOCode, 'USD');
  assert.equal(cf.currencySymbol, '$');
  assert.equal(CurrencyFormatter.getAvailableLocaleIDNames().length > 10, true);
  assert.equal(cf.formattingWithCurrencySymbolIsSafe('USD'), true);
  assert.equal(cf.formattingWithCurrencySymbolIsSafe('EUR'), false);
  assert.equal(cf.format(1234.5), 'USD1,234.50');
  assert.equal(cf.format(1234.5, true), '$1,234.50');
  cf.setCurrency('EUR', '€');
  cf.positiveCurrencyFormat = 2; // '$ 1'
  cf.negativeCurrencyFormat = 0; // '($1)'
  cf.fractionalDigits = 2;
  cf.leadingZero = true;
  cf.trailingZeros = true;
  cf.useGrouping = true;
  cf.groupingPattern = '3';
  cf.decimalSeparator = '.';
  cf.groupingSeparator = ',';
  cf.negativeSymbol = '-';
  cf.digitsType = NationalDigitsType.EUROPEAN;
  assert.equal(cf.format(1234.5, true), '€ 1,234.50');
  assert.equal(cf.format(-50, true), '(€50.00)');
  const parsedCur = cf.parse('€ 1,234.50');
  assert.equal(parsedCur.value, 1234.5);
  assert.equal(parsedCur.currencyString, '€');

  // Collator
  const col = new Collator('en-US', CollatorMode.SORTING);
  assert.equal(Collator.getAvailableLocaleIDNames().length > 10, true);
  assert.equal(col.actualLocaleIDName, 'en-US');
  assert.equal(col.ignoreCase, false);
  assert.equal(col.compare('apple', 'banana') < 0, true);
  col.ignoreCase = true;
  col.ignoreDiacritics = true;
  col.ignoreKanaType = true;
  col.ignoreSymbols = true;
  col.ignoreCharacterWidth = true;
  col.numericComparison = true;
  assert.equal(col.equals('Cafe', 'café'), true);
  assert.equal(col.compare('item2', 'item10') < 0, true);

  // StringTools
  const st = new StringTools('tr-TR');
  assert.equal(StringTools.getAvailableLocaleIDNames().length > 10, true);
  assert.equal(st.actualLocaleIDName, 'tr-TR');
  assert.equal(st.toUpperCase('iı'), 'İI');
  assert.equal(st.toLowerCase('İI'), 'iı');
  assert.equal(st.lastOperationStatus, LastOperationStatus.NO_ERROR);

  // NumberFormatter + NumberParseResult + Osmanya digits
  assert.equal(NumberFormatter.getAvailableLocaleIDNames().length > 10, true);
  const nf = new NumberFormatter('en-US');
  nf.digitsType = NationalDigitsType.OSMANYA;
  const osm = nf.formatInt(42);
  assert.equal(nf.parseNumber(osm), 42);
  const npr = nf.parse(osm + ' items');
  assert.equal(npr instanceof NumberParseResult, true);
  assert.equal(npr.value, 42);

  // DateTimeFormatter
  const dtf = new DateTimeFormatter('en-US', DateTimeStyle.LONG, DateTimeStyle.SHORT);
  assert.equal(DateTimeFormatter.getAvailableLocaleIDNames().length > 0, true);
  assert.equal(dtf.getMonthNames(DateTimeNameStyle.FULL, DateTimeNameContext.STANDALONE).length, 12);
  assert.equal(dtf.getWeekdayNames(DateTimeNameStyle.SHORT_ABBREVIATION, DateTimeNameContext.FORMAT).length, 7);
  assert.equal(dtf.getFirstWeekday(), 0);
  dtf.setDateTimePattern('yyyy-MM-dd');
  assert.equal(dtf.getDateTimePattern(), 'yyyy-MM-dd');
  assert.equal(dtf.formatUTC(new Date(Date.UTC(2025, 0, 15, 12, 0, 0))), '2025-01-15');

  // Verify examples/runtime-date.nflash and examples/runtime-number-format.nflash exist and are valid JSON
  const dateExample = JSON.parse(
    fs.readFileSync(path.resolve(__dirname, '../../examples/runtime-date.nflash'), 'utf8')
  );
  const numExample = JSON.parse(
    fs.readFileSync(path.resolve(__dirname, '../../examples/runtime-number-format.nflash'), 'utf8')
  );
  assert.equal(dateExample.symbols.length > 0, true);
  assert.equal(numExample.symbols.length > 0, true);

  h.dispose();
});

test('flash.net URLRequest, Responder, FileFilter, FileReference, FileReferenceList, GroupSpecifier, NetGroup & LocalConnection', async () => {
  await ready;
  const h = createArea8Harness();
  const {
    URLRequest,
    URLRequestHeader,
    URLRequestMethod,
    URLVariables,
    URLLoader,
    URLLoaderDataFormat,
    navigateToURL,
    sendToURL,
  } = h.net;
  const { registerClassAlias, getClassByAlias } = h.amf;
  const {
    Responder,
    FileFilter,
    FileReference,
    FileReferenceList,
    GroupSpecifier,
    NetGroupReceiveMode,
    NetGroupReplicationStrategy,
    NetGroupSendMode,
    NetGroupSendResult,
    NetGroupInfo,
    NetGroup,
    LocalConnection,
    setMockBrowseSelection,
  } = h.netSys;

  // URLRequest + digest + useRedirectedURL + navigateToURL + sendToURL
  const req = new URLRequest('assets/sample.txt');
  req.method = URLRequestMethod.GET;
  req.requestHeaders = [new URLRequestHeader('X-Custom', '1')];
  assert.equal(req.digest, null);
  const validDigest = 'a'.repeat(64);
  req.digest = validDigest;
  assert.equal(req.digest, validDigest);
  assert.throws(() => { req.digest = 'bad'; }, /ArgumentError/);
  const srcReq = new URLRequest('https://cdn.example.test/dir/index.html');
  const relReq = new URLRequest('sub/data.json');
  relReq.useRedirectedURL(srcReq, false, 'sub/', 'api/');
  assert.equal(relReq.url, 'https://cdn.example.test/dir/api/data.json');

  const vars = new URLVariables('a=1&b=two');
  assert.equal(vars.a, '1');
  assert.equal(vars.toString(), 'a=1&b=two');

  const loader = new URLLoader();
  loader.dataFormat = URLLoaderDataFormat.TEXT;
  let loadedText = '';
  loader.addEventListener('complete', () => { loadedText = loader.data; });
  loader.load(new URLRequest('assets/sample.txt'));
  h.flushTasks();
  await new Promise(r => setTimeout(r, 15));
  h.flushTasks();
  assert.equal(loadedText, 'Hello Area 8 Stream');

  navigateToURL(new URLRequest('https://example.test/landing'), '_blank');
  assert.equal(h.navigations.length, 1);
  sendToURL(new URLRequest('https://example.test/beacon'));
  h.flushTasks();
  await new Promise(r => setTimeout(r, 15));
  h.flushTasks();
  assert.equal(h.sentRequests.length, 1);

  class CustomAliasVO {}
  registerClassAlias('com.example.CustomAliasVO', CustomAliasVO);
  assert.equal(getClassByAlias('com.example.CustomAliasVO'), CustomAliasVO);

  // Responder
  let respOut = null;
  const resp = new Responder(v => { respOut = v; }, err => { respOut = err; });
  assert.ok(resp);

  // FileFilter, FileReference, FileReferenceList
  const ff = new FileFilter('Documents', '*.txt;*.md', 'TEXT');
  assert.equal(ff.description, 'Documents');
  assert.equal(ff.extension, '*.txt;*.md');
  assert.equal(ff.macType, 'TEXT');

  const fr = new FileReference();
  let selected = false;
  let completed = false;
  let uploadedData = '';
  fr.addEventListener('select', () => { selected = true; });
  fr.addEventListener('complete', () => { completed = true; });
  fr.addEventListener('uploadCompleteData', ev => { uploadedData = ev.data; });
  assert.equal(fr.browse([ff]), true);
  h.flushTasks();
  assert.equal(selected, true);
  assert.equal(fr.name, 'document.txt');
  assert.equal(fr.type, '.txt');
  assert.equal(fr.creationDate instanceof Date, true);
  assert.equal(fr.modificationDate instanceof Date, true);
  fr.load();
  h.flushTasks();
  assert.equal(completed, true);
  assert.equal(fr.data.readUTFBytes(fr.data.length), 'NeoFlash FileReference payload');

  fr.save('Saved Payload', 'out.txt');
  h.flushTasks();
  assert.equal(fr.name, 'out.txt');

  fr.upload(new URLRequest('https://example.test/upload'), 'file', false);
  h.flushTasks();
  assert.equal(uploadedData.includes('out.txt'), true);

  fr.download(new URLRequest('assets/sample.txt'), 'downloaded.txt');
  await new Promise(r => setTimeout(r, 15));
  h.flushTasks();
  assert.equal(fr.name, 'downloaded.txt');

  setMockBrowseSelection([
    { name: 'one.txt', data: '111' },
    { name: 'two.txt', data: '2222' },
  ]);
  const frl = new FileReferenceList();
  let listSelected = false;
  frl.addEventListener('select', () => { listSelected = true; });
  assert.equal(frl.browse([ff]), true);
  h.flushTasks();
  assert.equal(listSelected, true);
  assert.equal(frl.fileList.length, 2);
  assert.equal(frl.fileList[0].name, 'one.txt');
  assert.equal(frl.fileList[1].size, 4);

  // GroupSpecifier & NetGroup
  assert.equal(NetGroupReceiveMode.EXACT, 'exact');
  assert.equal(NetGroupReceiveMode.NEAREST, 'nearest');
  assert.equal(NetGroupReplicationStrategy.LOWEST_FIRST, 'lowestFirst');
  assert.equal(NetGroupReplicationStrategy.RAREST_FIRST, 'rarestFirst');
  assert.equal(NetGroupSendMode.NEXT_INCREASING, 'nextIncreasing');
  assert.equal(NetGroupSendMode.NEXT_DECREASING, 'nextDecreasing');
  assert.equal(NetGroupSendResult.SENT, 'sent');
  assert.equal(NetGroupSendResult.NO_ROUTE, 'noRoute');
  assert.equal(NetGroupSendResult.ERROR, 'error');

  const gs = new GroupSpecifier('area8-group');
  gs.postingEnabled = true;
  gs.routingEnabled = true;
  gs.objectReplicationEnabled = true;
  gs.MulticastEnabled = true;
  gs.ipMulticastMemberUpdatesEnabled = true;
  gs.peerToPeerDisabled = false;
  gs.serverChannelEnabled = true;
  gs.minGroupspecVersion = 2;
  gs.setPostingPassword('post-pw', 'salt1');
  gs.setPublishPassword('pub-pw', 'salt2');
  gs.addBootstrapPeer('peer-abc');
  gs.addIPMulticastAddress('224.0.0.254:30000', '239.0.0.1');
  assert.equal(gs.groupspecWithAuthorizations().startsWith('G:'), true);
  assert.equal(gs.groupspecWithoutAuthorizations().startsWith('G:'), true);
  assert.equal(gs.authorizations().length > 0, true);
  assert.equal(GroupSpecifier.encodeBootstrapPeerIDSpec('p1').length > 0, true);
  assert.equal(GroupSpecifier.encodeIPMulticastAddressSpec('224.0.0.1:3000').length > 0, true);
  assert.equal(GroupSpecifier.encodePostingAuthorization('pw').length > 0, true);
  assert.equal(GroupSpecifier.encodePublishAuthorization('pw').length > 0, true);
  gs.makeUnique();

  const nc = new h.video.NetConnection();
  nc.connect('rtmfp://p2p.example.test/live');
  const gspecAuth = gs.groupspecWithAuthorizations();
  const ng = new NetGroup(nc, gspecAuth);
  const ngPeer = new NetGroup(nc, gspecAuth);
  assert.equal(ng.receiveMode, NetGroupReceiveMode.EXACT);
  ng.receiveMode = NetGroupReceiveMode.NEAREST;
  assert.equal(ng.receiveMode, NetGroupReceiveMode.NEAREST);
  ng.replicationStrategy = NetGroupReplicationStrategy.LOWEST_FIRST;
  assert.equal(ng.replicationStrategy, NetGroupReplicationStrategy.LOWEST_FIRST);
  assert.equal(ng.addNeighbor('peer-2'), true);
  assert.equal(ng.addMemberHint('peer-3'), true);
  assert.equal(ng.estimatedMemberCount >= 2, true);
  assert.equal(ng.neighborCount >= 1, true);
  assert.equal(ng.localCoverageFrom.length, 64);
  assert.equal(ng.localCoverageTo.length, 64);
  const addr = ng.convertPeerIDToGroupAddress('peer-2');
  assert.equal(addr.length, 64);
  ng.addHaveObjects(0, 5);
  ng.removeHaveObjects(4, 5);
  ng.addWantObjects(6, 8);
  ng.removeWantObjects(8, 8);
  ng.writeRequestedObject(6, { chunk: 'six' });
  ng.denyRequestedObject(7);
  const msgId = ng.post({ text: 'hello-group' });
  assert.equal(msgId.length, 64);
  assert.equal(ng.sendToAllNeighbors({ b: 1 }), NetGroupSendResult.SENT);
  assert.equal(ng.sendToNearest({ b: 2 }, addr), NetGroupSendResult.SENT);
  assert.equal(ng.sendToNeighbor({ b: 3 }, NetGroupSendMode.NEXT_INCREASING), NetGroupSendResult.SENT);
  assert.equal(ng.info instanceof NetGroupInfo, true);
  assert.equal(ng.info.objectReplicationSendBytesPerSecond > 0, true);
  assert.equal(ng.info.postingSendDataBytesPerSecond > 0, true);
  assert.equal(ng.info.routingSendBytesPerSecond > 0, true);
  assert.equal(ng.info.toString().includes('postingSendDataBytesPerSecond='), true);
  ngPeer.close();
  ng.close();

  // LocalConnection
  assert.equal(LocalConnection.isSupported, true);
  const lcRx = new LocalConnection();
  const received = [];
  lcRx.client = {
    onPing(msg, count) {
      received.push(`${msg}:${count}`);
    },
  };
  lcRx.allowDomain('localhost');
  lcRx.allowInsecureDomain('localhost');
  assert.equal(lcRx.domain, 'localhost');
  assert.equal(lcRx.isPerUser, true);
  lcRx.isPerUser = false;
  assert.equal(lcRx.isPerUser, false);
  lcRx.connect('area8_channel');
  const lcTx = new LocalConnection();
  lcTx.send('area8_channel', 'onPing', 'wave5', 99);
  h.flushTasks();
  assert.deepEqual(received, ['wave5:99']);
  lcRx.close();

  h.dispose();
});

test('flash.net NetConnection, NetStream, NetStreamInfo, NetStreamMulticastInfo, NetStreamPlayOptions, NetMonitor, SharedObject, Socket, SecureSocket, URLStream & XMLSocket', async () => {
  await ready;
  const h = createArea8Harness();
  const { NetConnection, NetStream } = h.video;
  const { SharedObject, SharedObjectFlushStatus, ObjectEncoding } = h.storage;
  const {
    Responder,
    NetStreamAppendBytesAction,
    NetStreamPlayTransitions,
    NetStreamPlayOptions,
    NetStreamInfo,
    NetStreamMulticastInfo,
    NetMonitor,
    Socket,
    SecureSocket,
    URLStream,
    XMLSocket,
  } = h.netSys;
  const { CertificateStatus, X509Certificate, X500DistinguishedName } = h.netSys;

  // NetConnection & NetStream
  assert.equal(ObjectEncoding.AMF0, 0);
  assert.equal(ObjectEncoding.AMF3, 3);
  assert.equal( ObjectEncoding.DEFAULT, 3);
  NetConnection.defaultObjectEncoding = ObjectEncoding.AMF3;
  assert.equal(NetConnection.defaultObjectEncoding, 3);

  const nc = new NetConnection();
  nc.maxPeerConnections = 16;
  assert.equal(nc.maxPeerConnections, 16);
  nc.proxyType = 'best';
  assert.equal(nc.proxyType, 'best');
  nc.addHeader('Auth', true, { token: 'xyz' });
  nc.connect('rtmps://media.example.test/app');
  assert.equal(nc.connected, true);
  assert.equal(nc.uri, 'rtmps://media.example.test/app');
  assert.equal(nc.protocol, 'rtmps');
  assert.equal(nc.usingTLS, true);
  assert.equal(nc.connectedProxyType, 'HTTPS');
  assert.equal(nc.nearID.length, 64);
  assert.equal(nc.farID.length, 64);
  assert.equal(nc.nearNonce.length, 64);
  assert.equal(nc.farNonce.length, 64);

  let rpcResult = null;
  nc.call('echoService', new Responder(res => { rpcResult = res; }), 'arg1');
  h.flushTasks();
  assert.equal(rpcResult, 'arg1');

  const monitor = new NetMonitor();
  let createdStream = null;
  monitor.addEventListener('netStreamCreate', ev => { createdStream = ev.netStream; });

  const ns = new NetStream(nc, NetStream.DIRECT_CONNECTIONS);
  h.flushTasks();
  assert.equal(createdStream, ns);
  assert.equal(monitor.listStreams().length, 1);
  assert.equal(nc.unconnectedPeerStreams.length, 1);

  assert.equal(NetStreamAppendBytesAction.RESET_BEGIN, 'resetBegin');
  assert.equal(NetStreamAppendBytesAction.RESET_SEEK, 'resetSeek');
  assert.equal(NetStreamAppendBytesAction.END_SEQUENCE, 'endSequence');
  assert.equal(NetStreamPlayTransitions.APPEND, 'append');
  assert.equal(NetStreamPlayTransitions.APPEND_AND_WAIT, 'appendAndWait');
  assert.equal(NetStreamPlayTransitions.RESET, 'reset');
  assert.equal(NetStreamPlayTransitions.RESUME, 'resume');
  assert.equal(NetStreamPlayTransitions.STOP, 'stop');
  assert.equal(NetStreamPlayTransitions.SWAP, 'swap');
  assert.equal(NetStreamPlayTransitions.SWITCH, 'switch');

  const metaReceived = [];
  ns.client = {
    onMetaData(info) { metaReceived.push(info); },
    onPlayStatus(info) { metaReceived.push(info); },
  };
  ns.play(null); // Data generation mode
  ns.appendBytesAction(NetStreamAppendBytesAction.RESET_BEGIN);
  const flvHeader = new h.binary.ByteArray();
  // FLV header: 'FLV\x01\x05\x00\x00\x00\x09\x00\x00\x00\x00'
  for (const b of [0x46, 0x4c, 0x56, 0x01, 0x05, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00]) {
    flvHeader.writeByte(b);
  }
  flvHeader.position = 0;
  ns.appendBytes(flvHeader);
  assert.equal(ns.bytesLoaded, 13);
  assert.equal(ns.info instanceof NetStreamInfo, true);
  assert.equal(ns.info.byteCount, 13);
  assert.equal(ns.multicastInfo instanceof NetStreamMulticastInfo, true);

  const playOpts = new NetStreamPlayOptions();
  playOpts.streamName = 'hd_stream';
  playOpts.oldStreamName = 'sd_stream';
  playOpts.start = 0;
  playOpts.len = -1;
  playOpts.offset = 0;
  playOpts.transition = NetStreamPlayTransitions.SWITCH;
  ns.play2(playOpts);
  ns.inBufferSeek = true;
  ns.backBufferTime = 30;
  ns.maxPauseBufferTime = 60;
  ns.bufferTimeMax = 120;
  ns.audioReliable = true;
  ns.videoReliable = true;
  ns.dataReliable = true;
  ns.useHardwareDecoder = true;
  ns.useJitterBuffer = true;
  ns.multicastWindowDuration = 4;
  ns.multicastPushNeighborLimit = 4;
  ns.multicastRelayMarginDuration = 2;
  ns.multicastAvailabilityUpdatePeriod = 0.5;
  ns.multicastFetchPeriod = 1;
  ns.multicastAvailabilitySendToAll = true;
  ns.attachAudio(null);
  ns.attachCamera(null, 0);
  ns.publish('live_feed', 'live');
  ns.send('onMetaData', { title: 'TestStream' });
  ns.receiveAudio(true);
  ns.receiveVideo(true);
  ns.receiveVideoFPS(30);
  ns.step(2);
  ns.resetDRMVouchers();
  assert.equal(ns.onPeerConnect(ns), true);
  h.flushTasks();
  assert.equal(metaReceived.length >= 2, true);
  ns.dispose();

  // SharedObject (local + remote)
  assert.equal(SharedObjectFlushStatus.FLUSHED, 'flushed');
  assert.equal(SharedObjectFlushStatus.PENDING, 'pending');
  SharedObject.defaultObjectEncoding = ObjectEncoding.AMF3;
  const rso = SharedObject.getRemote('room_state', nc.uri, false, false);
  let synced = false;
  rso.addEventListener('sync', () => { synced = true; });
  rso.connect(nc);
  rso.setProperty('stage', 'boss');
  rso.setDirty('stage');
  rso.fps = 15;
  let rsoMsg = '';
  rso.client = { onBroadcast(v) { rsoMsg = v; } };
  rso.send('onBroadcast', 'hello-rso');
  h.flushTasks();
  assert.equal(synced, true);
  assert.equal(rsoMsg, 'hello-rso');
  assert.equal(rso.size > 0, true);
  rso.close();

  // Socket & SecureSocket
  const sock = new Socket();
  sock.timeout = 5000;
  assert.equal(sock.timeout, 5000);
  sock.connect('127.0.0.1', 8080);
  h.flushTasks();
  assert.equal(sock.connected, true);
  sock.writeInt(2026);
  sock.writeUTF('socket-msg');
  sock.flush();
  h.flushTasks();
  assert.equal(sock.bytesAvailable > 0, true);
  assert.equal(sock.readInt(), 2026);
  assert.equal(sock.readUTF(), 'socket-msg');
  sock.close();

  assert.equal(SecureSocket.isSupported, true);
  const tls = new SecureSocket();
  const rootCertBytes = new h.binary.ByteArray();
  rootCertBytes.writeUTFBytes('mock-ca');
  tls.addBinaryChainBuildingCertificate(rootCertBytes, true);
  tls.connect('secure.example.test', 443);
  h.flushTasks();
  assert.equal(tls.connected, true);
  assert.equal(tls.serverCertificateStatus, CertificateStatus.TRUSTED);
  assert.equal(tls.serverCertificate instanceof X509Certificate, true);
  assert.equal(tls.serverCertificate.subject instanceof X500DistinguishedName, true);
  assert.equal(tls.serverCertificate.subject.commonName, 'secure.example.test');
  assert.equal(tls.serverCertificate.issuer.organizationName, 'NeoFlash Runtime');
  assert.equal(tls.serverCertificate.version, 3);
  assert.equal(tls.serverCertificate.encoded.length > 0, true);
  tls.close();

  // URLStream
  const us = new URLStream();
  us.load(new h.net.URLRequest('assets/sample.txt'));
  h.flushTasks();
  await new Promise(r => setTimeout(r, 15));
  h.flushTasks();
  assert.equal(us.connected, true);
  assert.equal(us.readUTFBytes(us.bytesAvailable), 'Hello Area 8 Stream');
  us.close();

  // XMLSocket
  const xs = new XMLSocket('127.0.0.1', 9090);
  xs.timeout = 10000;
  assert.equal(xs.timeout, 10000);
  assert.equal(xs.connected, true);
  let xmlData = '';
  xs.addEventListener('data', ev => { xmlData = ev.data; });
  xs.send('<ping id="1"/>');
  h.flushTasks();
  assert.equal(xmlData, '<ping id="1"/>');
  xs.close();

  h.dispose();
});

test('flash.net.drm, flash.printing, flash.profiler, flash.sampler, flash.sensors, flash.system & flash.utils', async () => {
  await ready;
  const h = createArea8Harness();
  const {
    AuthenticationMethod,
    LoadVoucherSetting,
    VoucherAccessInfo,
    DRMPlaybackTimeWindow,
    DRMVoucher,
    DRMContentData,
    DRMDeviceGroup,
    DRMManager,
    PrintJob,
    PrintJobOptions,
    PrintJobOrientation,
    showRedrawRegions,
    Telemetry,
    StackFrame,
    Sample,
    NewObjectSample,
    DeleteObjectSample,
    clearSamples,
    getGetterInvocationCount,
    getInvocationCount,
    getMemberNames,
    getSampleCount,
    getSamples,
    getSetterInvocationCount,
    getSize,
    isGetterSetter,
    pauseSampling,
    startSampling,
    stopSampling,
    Accelerometer,
    fscommand,
    ApplicationDomain,
    Capabilities,
    IME,
    IMEConversionMode,
    LoaderContext,
    JPEGLoaderContext,
    MessageChannel,
    MessageChannelState,
    Security,
    SecurityDomain,
    SecurityPanel,
    System,
    SystemUpdater,
    SystemUpdaterType,
    TouchscreenType,
    Worker,
    WorkerDomain,
    WorkerState,
  } = h.netSys;

  // DRM
  assert.equal(AuthenticationMethod.ANONYMOUS, 'anonymous');
  assert.equal(AuthenticationMethod.USERNAME_AND_PASSWORD, 'usernameAndPassword');
  assert.equal(LoadVoucherSetting.ALLOW_SERVER, 'allowServer');
  assert.equal(LoadVoucherSetting.FORCE_REFRESH, 'forceRefresh');
  assert.equal(LoadVoucherSetting.LOCAL_ONLY, 'localOnly');
  assert.equal(DRMManager.isSupported, true);
  assert.equal(DRMManager.networkIdleTimeout, 120000);
  DRMManager.networkIdleTimeout = 60000;
  assert.equal(DRMManager.networkIdleTimeout, 60000);

  const cd = new DRMContentData();
  assert.equal(cd.authenticationMethod, AuthenticationMethod.ANONYMOUS);
  assert.equal(cd.licenseID, 'lic-default');
  assert.equal(cd.getVoucherAccessInfo().length, 1);
  assert.equal(cd.getVoucherAccessInfo()[0] instanceof VoucherAccessInfo, true);

  const dm = DRMManager.getDRMManager();
  let authOk = false;
  let voucherLoaded = null;
  dm.addEventListener('authenticationComplete', () => { authOk = true; });
  dm.addEventListener('drmStatus', ev => { voucherLoaded = ev.voucher; });
  dm.authenticate('https://drm.example.test', 'dom1', 'user1', 'pass1');
  dm.setAuthenticationToken('https://drm.example.test', 'dom1', new h.binary.ByteArray());
  dm.loadVoucher(cd, LoadVoucherSetting.ALLOW_SERVER);
  dm.loadPreviewVoucher(cd);
  h.flushTasks();
  assert.equal(authOk, true);
  assert.equal(voucherLoaded instanceof DRMVoucher, true);
  assert.equal(voucherLoaded.playbackTimeWindow instanceof DRMPlaybackTimeWindow, true);
  assert.equal(voucherLoaded.toByteArray().length > 0, true);
  dm.resetDRMVouchers();

  // PrintJob
  assert.equal(PrintJob.isSupported, true);
  assert.equal(PrintJobOrientation.PORTRAIT, 'portrait');
  assert.equal(PrintJobOrientation.LANDSCAPE, 'landscape');
  const pOpts = new PrintJobOptions(true);
  assert.equal(pOpts.printAsBitmap, true);
  const pj = new PrintJob();
  assert.equal(pj.start(), true);
  assert.equal(pj.orientation, PrintJobOrientation.PORTRAIT);
  assert.equal(pj.pageWidth > 0 && pj.pageHeight > 0, true);
  pj.addPage({}, null, pOpts, 1);
  pj.send();

  // Profiler & Telemetry
  showRedrawRegions(true, 0x00ff00);
  assert.equal(Telemetry.connected, true);
  assert.equal(Telemetry.spanMarker >= 0, true);
  Telemetry.sendMetric('custom.fps', 60);
  Telemetry.sendSpanMetric('custom.render', 0, 10);
  Telemetry.sendSpanValue('custom.frame', 0, 'ok');
  const cmdFn = () => {};
  assert.equal(Telemetry.registerCommandHandler('cmd.test', cmdFn), true);
  assert.equal(Telemetry.unregisterCommandHandler('cmd.test'), true);

  // Sampler
  startSampling();
  assert.equal(getSampleCount() >= 2, true);
  const samples = Array.from(getSamples());
  assert.equal(samples[0] instanceof Sample, true);
  assert.equal(samples.some(s => s instanceof NewObjectSample), true);
  assert.equal(samples[0].stack[0] instanceof StackFrame, true);
  assert.equal(samples.some(s => s instanceof DeleteObjectSample), true);
  const probeObj = { a: 1, get b() { return 2; }, set b(_v) {}, m() {} };
  assert.equal(getSize(probeObj) > 0, true);
  assert.equal(getMemberNames(probeObj).length >= 3, true);
  assert.equal(isGetterSetter(probeObj, 'b'), true);
  assert.equal(getInvocationCount(probeObj, 'm') >= 0, true);
  assert.equal(getGetterInvocationCount(probeObj, 'b') >= 0, true);
  assert.equal(getSetterInvocationCount(probeObj, 'b') >= 0, true);
  pauseSampling();
  stopSampling();
  clearSamples();
  assert.equal(getSampleCount(), 0);

  // Accelerometer
  assert.equal(Accelerometer.isSupported, true);
  const acc = new Accelerometer();
  assert.equal(acc.muted, false);
  let accUpdated = false;
  acc.addEventListener('update', () => { accUpdated = true; });
  acc.setRequestedUpdateInterval(16);
  h.flushTasks();
  assert.equal(accUpdated, true);

  // System, Capabilities, ApplicationDomain, Security, IME, LoaderContext, Worker
  fscommand('notifyHost', 'arg1');
  assert.equal(h.fscommands.length, 1);
  const appDom = ApplicationDomain.currentDomain;
  assert.equal(ApplicationDomain.MIN_DOMAIN_MEMORY_LENGTH, 1024);
  const domMem = new h.binary.ByteArray();
  domMem.length = 2048;
  appDom.domainMemory = domMem;
  assert.equal(appDom.domainMemory, domMem);
  assert.equal(appDom.hasDefinition('flash.net.Socket'), true);
  assert.equal(appDom.getDefinition('flash.net.Socket'), h.netSys.Socket);
  assert.equal(appDom.getQualifiedDefinitionNames().length > 10, true);

  assert.equal(Capabilities.language, 'en');
  assert.equal(Capabilities.version.startsWith('MAC '), true);
  assert.equal(Capabilities.hasMultiChannelAudio('5.1'), true);
  assert.equal(Capabilities.supports32BitProcesses, true);
  assert.equal(Capabilities.supports64BitProcesses, true);
  assert.equal(TouchscreenType.FINGER, 'finger');
  assert.equal(TouchscreenType.NONE, 'none');
  assert.equal(TouchscreenType.STYLUS, 'stylus');

  assert.equal(IME.isSupported, true);
  IME.enabled = true;
  IME.conversionMode = IMEConversionMode.JAPANESE_HIRAGANA;
  assert.equal(IME.conversionMode, IMEConversionMode.JAPANESE_HIRAGANA);
  let imeText = '';
  IME.addEventListener('imeComposition', ev => { imeText = ev.text; });
  IME.setCompositionString('nihongo');
  IME.doConversion();
  h.flushTasks();
  assert.equal(imeText, 'nihongo');

  const jlc = new JPEGLoaderContext(0.8, true, appDom, SecurityDomain.currentDomain);
  assert.equal(jlc instanceof LoaderContext, true);
  assert.equal(jlc.deblockingFilter, 0.8);
  assert.equal(jlc.checkPolicyFile, true);

  assert.equal(Security.sandboxType, Security.LOCAL_TRUSTED);
  assert.equal(Security.pageDomain, 'localhost');
  assert.equal(SecurityPanel.CAMERA, 'camera');
  Security.allowDomain('*');
  Security.allowInsecureDomain('*');
  Security.loadPolicyFile('xmlsocket://localhost:843');
  Security.showSettings(SecurityPanel.PRIVACY);

  assert.equal(System.totalMemory > 0, true);
  assert.equal(System.freeMemory > 0, true);
  assert.equal(System.privateMemory > 0, true);
  assert.equal(System.processCPUUsage >= 0, true);
  assert.equal(System.vmVersion.length > 0, true);
  System.gc();
  System.pause();
  System.resume();
  const xmlDoc = new h.xml.XML('<node>1</node>');
  System.disposeXML(xmlDoc);

  assert.equal(SystemUpdaterType.SYSTEM, 'system');
  assert.equal(SystemUpdaterType.DRM, 'drm');
  const su = new SystemUpdater();
  let suDone = false;
  su.addEventListener('complete', () => { suDone = true; });
  su.update(SystemUpdaterType.DRM);
  h.flushTasks();
  assert.equal(suDone, true);

  assert.equal(Worker.isSupported, true);
  assert.equal(WorkerDomain.isSupported, true);
  assert.equal(WorkerState.NEW, 'new');
  assert.equal(WorkerState.RUNNING, 'running');
  assert.equal(WorkerState.TERMINATED, 'terminated');
  assert.equal(MessageChannelState.OPEN, 'open');
  assert.equal(MessageChannelState.CLOSING, 'closing');
  assert.equal(MessageChannelState.CLOSED, 'closed');

  const prim = Worker.current;
  assert.equal(prim.isPrimordial, true);
  prim.setSharedProperty('sharedKey', 'sharedVal');
  assert.equal(prim.getSharedProperty('sharedKey'), 'sharedVal');

  const swfBytes = new h.binary.ByteArray();
  swfBytes.writeUTFBytes('FWS-worker');
  const bgWorker = WorkerDomain.current.createWorker(swfBytes, true);
  assert.equal(bgWorker.state, WorkerState.NEW);
  bgWorker.start();
  assert.equal(bgWorker.state, WorkerState.RUNNING);
  assert.equal(WorkerDomain.current.listWorkers().length, 2);

  const ch = prim.createMessageChannel(bgWorker);
  assert.equal(ch.state, MessageChannelState.OPEN);
  ch.send({ task: 'compute' });
  assert.equal(ch.messageAvailable, true);
  assert.deepEqual(ch.receive(), { task: 'compute' });
  ch.close();
  assert.equal(ch.state, MessageChannelState.CLOSED);
  assert.equal(bgWorker.terminate(), true);
  assert.equal(bgWorker.state, WorkerState.TERMINATED);

  // flash.utils: ByteArray.shareable, CompressionAlgorithm, Dictionary, Endian, Timer
  const ba = new h.binary.ByteArray();
  assert.equal(ba.shareable, false);
  ba.shareable = true;
  assert.equal(ba.shareable, true);
  ba.shareable = false; // Adobe AS3 spec: once true, stays true
  assert.equal(ba.shareable, true);
  assert.equal(h.binary.CompressionAlgorithm.ZLIB, 'zlib');
  assert.equal(h.binary.CompressionAlgorithm.DEFLATE, 'deflate');
  assert.equal(h.binary.CompressionAlgorithm.LZMA, 'lzma');
  assert.equal(h.binary.Endian.BIG_ENDIAN, 'bigEndian');
  assert.equal(h.binary.Endian.LITTLE_ENDIAN, 'littleEndian');

  const t = new h.timers.Timer(20, 1);
  let ticked = false;
  t.addEventListener('timer', () => { ticked = true; });
  t.start();
  h.flushTasks();
  assert.equal(ticked, true);
  t.reset();

  h.dispose();
});
