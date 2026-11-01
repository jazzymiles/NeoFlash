// End-to-end verification of ActionScript 3 AIR runtime classes (web/air.js)
// backed by the shared Rust AirKernel in WASM (crates/neoflash-core/src/as3_air.rs).
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
const NeoFlashStage = require('../stage.js');
const NeoFlashNet = require('../net.js');
const NeoFlashAIR = require('../air.js');

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

function createAirHarness(customProject = {}) {
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
  const dummyProject = {
    format_version: 1,
    name: customProject.name || 'AIR Test App',
    width: customProject.width || 800,
    height: customProject.height || 600,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
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
    binary,
    encode: (fmt, graph) => player.amf_encode(fmt, graph),
    decode: (fmt, bytes) => player.amf_decode(fmt, bytes),
    getProgram: () => null,
    isUnsupported: () => false,
  });
  binary.setObjectCodec(amf);

  const eventFamilies = NeoFlashEvents.createFamilies(wasm, {
    isByteArray: v => binary.ByteArray.__nfIs(v),
    requestRender: () => {},
  });
  const allEvents = { ...NeoFlashEvents, ...eventFamilies };
  const net = NeoFlashNet.create({
    events: allEvents,
    binary,
    resources: customProject.resources || [],
  });
  const xml = NeoFlashXML.create({
    parseLegacy: src => wasm.parse_as2_xml_json(src),
    namespaceLookup: (anc, val, rev) =>
      JSON.parse(wasm.as2_xml_namespace_json(JSON.stringify(anc), val, rev)),
    parse: src => wasm.parse_xml_json(src),
    net,
  });

  const stageHub = NeoFlashEvents.create({ alive: () => true, parentOf: () => null });
  const stageHost = NeoFlashStage.create({
    events: stageHub,
    width: dummyProject.width,
    height: dummyProject.height,
    frameRate: 24,
  });

  const air = NeoFlashAIR.create({
    wasm,
    events: allEvents,
    eventFamilies,
    binary,
    geometry: geom2d,
    net,
    getXML: () => xml.XML,
    getStage: () => stageHost.stage,
    getViewportSize: () => [dummyProject.width, dummyProject.height],
    project: {
      name: dummyProject.name,
      width: dummyProject.width,
      height: dummyProject.height,
      resources: customProject.resources || [],
    },
    schedule: fn => queue.push(fn),
  });

  return {
    air,
    B: air.builtins,
    binary,
    geom: geom2d,
    xml,
    net,
    events: allEvents,
    stage: stageHost.stage,
    flushTasks,
    dispose() {
      air.dispose();
      stageHost.dispose();
      net.dispose();
      xml.dispose();
      binary.dispose();
      player.free();
    },
  };
}

test('flash.filesystem: virtual roots, app:/ read-only protection, directory operations, and FileStream IDataInput/IDataOutput', async () => {
  await ready;
  const embeddedBytes = Buffer.from('bundled-asset-payload', 'utf8').toString('base64');
  const h = createAirHarness({
    name: 'Filesystem Verification',
    resources: [{ path: 'assets/manifest.txt', data_base64: embeddedBytes }],
  });
  try {
    const File = h.B['flash.filesystem.File'];
    const FileMode = h.B['flash.filesystem.FileMode'];
    const FileStream = h.B['flash.filesystem.FileStream'];
    const StorageVolumeInfo = h.B['flash.filesystem.StorageVolumeInfo'];

    assert.equal(File.applicationDirectory.url, 'app:/');
    assert.equal(File.applicationStorageDirectory.url, 'app-storage:/');
    assert.ok(File.cacheDirectory.exists);
    assert.ok(File.userDirectory.exists);
    assert.ok(File.documentsDirectory.exists);
    assert.ok(File.desktopDirectory.exists);
    assert.equal(File.getRootDirectories().length, 1);
    assert.equal(File.getRootDirectories()[0].nativePath, '/');

    const bundled = File.applicationDirectory.resolvePath('assets/manifest.txt');
    assert.equal(bundled.exists, true);
    assert.equal(bundled.isDirectory, false);
    assert.equal(bundled.name, 'manifest.txt');
    assert.equal(bundled.extension, 'txt');
    assert.equal(bundled.size, 21);

    const fsRead = new FileStream();
    assert.equal(h.air.isDataInput(fsRead), true);
    assert.equal(h.air.isDataOutput(fsRead), true);
    fsRead.open(bundled, FileMode.READ);
    assert.equal(fsRead.bytesAvailable, 21);
    assert.equal(fsRead.readUTFBytes(21), 'bundled-asset-payload');
    assert.throws(() => fsRead.readByte(), err => err.name === 'EOFError');
    fsRead.close();

    assert.throws(
      () => bundled.deleteFile(),
      err => err.name === 'SecurityError',
      'app:/ must reject mutations'
    );
    assert.throws(
      () => new FileStream().open(bundled, FileMode.WRITE),
      err => err.name === 'SecurityError'
    );

    const workDir = File.applicationStorageDirectory.resolvePath('workspace/sub');
    assert.equal(workDir.exists, false);
    workDir.createDirectory();
    assert.equal(workDir.exists, true);
    assert.equal(workDir.isDirectory, true);

    const binFile = workDir.resolvePath('record.bin');
    const fsOut = new FileStream();
    fsOut.open(binFile, FileMode.WRITE);
    fsOut.writeBoolean(true);
    fsOut.writeByte(-12);
    fsOut.writeShort(12345);
    fsOut.writeInt(-987654);
    fsOut.writeUnsignedInt(0xdeadbeef);
    fsOut.writeDouble(3.25);
    fsOut.writeUTF('AIR binary');
    fsOut.writeObject({ tag: 'amf', score: 99 });
    fsOut.close();

    assert.equal(binFile.exists, true);
    assert.ok(binFile.size > 20);

    const fsIn = new FileStream();
    fsIn.open(binFile, FileMode.READ);
    assert.equal(fsIn.readBoolean(), true);
    assert.equal(fsIn.readByte(), -12);
    assert.equal(fsIn.readShort(), 12345);
    assert.equal(fsIn.readInt(), -987654);
    assert.equal(fsIn.readUnsignedInt(), 0xdeadbeef);
    assert.equal(fsIn.readDouble(), 3.25);
    assert.equal(fsIn.readUTF(), 'AIR binary');
    assert.deepEqual(fsIn.readObject(), { tag: 'amf', score: 99 });
    fsIn.close();

    const txtFile = workDir.resolvePath('log.txt');
    const fsUpd = new FileStream();
    fsUpd.open(txtFile, FileMode.WRITE);
    fsUpd.writeUTFBytes('HelloWorld12345');
    fsUpd.position = 10;
    fsUpd.truncate();
    fsUpd.close();
    assert.equal(txtFile.size, 10);

    fsUpd.open(txtFile, FileMode.APPEND);
    fsUpd.writeUTFBytes('!');
    fsUpd.close();
    fsUpd.open(txtFile, FileMode.READ);
    assert.equal(fsUpd.readUTFBytes(fsUpd.bytesAvailable), 'HelloWorld!');
    fsUpd.close();

    assert.equal(workDir.getRelativePath(txtFile), 'log.txt');
    assert.equal(txtFile.getRelativePath(File.applicationStorageDirectory, true), '../../..');

    const copyDir = File.applicationStorageDirectory.resolvePath('backup');
    workDir.copyTo(copyDir, true);
    assert.equal(copyDir.resolvePath('log.txt').exists, true);

    const movedFile = copyDir.resolvePath('renamed.txt');
    copyDir.resolvePath('log.txt').moveTo(movedFile, false);
    assert.equal(movedFile.exists, true);
    assert.equal(copyDir.resolvePath('log.txt').exists, false);

    const listing = copyDir.getDirectoryListing();
    assert.equal(listing.length, 2);

    let asyncListed = null;
    copyDir.addEventListener('directoryListing', ev => {
      asyncListed = ev.files.map(f => f.name).sort();
    });
    copyDir.getDirectoryListingAsync();
    h.flushTasks();
    assert.deepEqual(asyncListed, ['record.bin', 'renamed.txt']);

    let asyncOpened = false;
    const fsAsync = new FileStream();
    fsAsync.addEventListener('complete', () => {
      asyncOpened = true;
    });
    fsAsync.openAsync(movedFile, FileMode.READ);
    h.flushTasks();
    assert.equal(asyncOpened, true);
    assert.equal(fsAsync.readUTFBytes(fsAsync.bytesAvailable), 'HelloWorld!');
    fsAsync.close();

    let loadedComplete = false;
    movedFile.addEventListener('complete', () => {
      loadedComplete = true;
    });
    movedFile.load();
    h.flushTasks();
    assert.equal(loadedComplete, true);
    assert.equal(movedFile.data.readUTFBytes(movedFile.data.bytesAvailable), 'HelloWorld!');

    const tmpFile = File.createTempFile();
    const tmpDir = File.createTempDirectory();
    assert.equal(tmpFile.exists, true);
    assert.equal(tmpDir.exists, true);
    tmpFile.deleteFile();
    tmpDir.deleteDirectory(true);
    assert.equal(tmpFile.exists, false);
    assert.equal(tmpDir.exists, false);

    assert.throws(() => copyDir.deleteDirectory(false), /not empty/i);
    copyDir.deleteDirectory(true);
    assert.equal(copyDir.exists, false);

    const volumes = StorageVolumeInfo.storageVolumeInfo.getStorageVolumes();
    assert.equal(volumes.length, 3);
    assert.equal(volumes[0].isWritable, false);
    assert.equal(volumes[1].isWritable, true);
  } finally {
    h.dispose();
  }
});

test('flash.desktop & flash.display: Clipboard, NativeDragManager, NativeApplication, Screen, NativeMenu, NativeWindow, and Stage AIR members', async () => {
  await ready;
  const h = createAirHarness({ name: 'My AIR Workbench', width: 960, height: 640 });
  try {
    const Clipboard = h.B['flash.desktop.Clipboard'];
    const ClipboardFormats = h.B['flash.desktop.ClipboardFormats'];
    const ClipboardTransferMode = h.B['flash.desktop.ClipboardTransferMode'];
    const NativeDragManager = h.B['flash.desktop.NativeDragManager'];
    const NativeDragOptions = h.B['flash.desktop.NativeDragOptions'];
    const NativeDragActions = h.B['flash.desktop.NativeDragActions'];
    const NativeApplication = h.B['flash.desktop.NativeApplication'];
    const NativeProcess = h.B['flash.desktop.NativeProcess'];
    const NativeProcessStartupInfo = h.B['flash.desktop.NativeProcessStartupInfo'];
    const Updater = h.B['flash.desktop.Updater'];
    const Screen = h.B['flash.display.Screen'];
    const NativeMenu = h.B['flash.display.NativeMenu'];
    const NativeMenuItem = h.B['flash.display.NativeMenuItem'];
    const NativeWindow = h.B['flash.display.NativeWindow'];
    const NativeWindowInitOptions = h.B['flash.display.NativeWindowInitOptions'];
    const NativeWindowType = h.B['flash.display.NativeWindowType'];
    const NativeWindowSystemChrome = h.B['flash.display.NativeWindowSystemChrome'];
    const NativeWindowDisplayState = h.B['flash.display.NativeWindowDisplayState'];
    const StageOrientation = h.B['flash.display.StageOrientation'];

    assert.equal(Clipboard.supportsFilePromise, true);
    const cb = Clipboard.generalClipboard;
    cb.clear();
    const payload = { label: 'item', count: 3 };
    assert.equal(cb.setData(ClipboardFormats.TEXT_FORMAT, 'plain text'), true);
    assert.equal(cb.setData('custom:obj', payload, true), true);
    let lazyCalls = 0;
    assert.equal(
      cb.setDataHandler(ClipboardFormats.URL_FORMAT, () => {
        lazyCalls++;
        return 'https://example.invalid/air';
      }),
      true
    );
    assert.equal(cb.hasFormat(ClipboardFormats.TEXT_FORMAT), true);
    assert.equal(cb.getData(ClipboardFormats.URL_FORMAT), 'https://example.invalid/air');
    assert.equal(lazyCalls, 1);

    const orig = cb.getData('custom:obj', ClipboardTransferMode.ORIGINAL_ONLY);
    const cloned = cb.getData('custom:obj', ClipboardTransferMode.CLONE_ONLY);
    assert.equal(orig, payload);
    assert.notEqual(cloned, payload);
    assert.deepEqual(cloned, payload);

    const initiator = new h.events.EventDispatcher();
    const dropTarget = new h.events.EventDispatcher();
    const dragEvents = [];
    initiator.addEventListener('nativeDragStart', ev => {
      dragEvents.push(['start', ev.dropAction]);
      NativeDragManager.acceptDragDrop(dropTarget);
      NativeDragManager.dropAction = NativeDragActions.MOVE;
    });
    dropTarget.addEventListener('nativeDragDrop', ev => {
      dragEvents.push([
        'drop',
        ev.clipboard.getData(ClipboardFormats.TEXT_FORMAT),
        ev.dropAction,
      ]);
    });
    initiator.addEventListener('nativeDragComplete', ev => {
      dragEvents.push(['complete', ev.dropAction]);
    });
    const dragOpts = new NativeDragOptions();
    dragOpts.allowLink = false;
    NativeDragManager.doDrag(initiator, cb, null, null, dragOpts);
    assert.equal(NativeDragManager.isDragging, true);
    h.flushTasks();
    assert.equal(NativeDragManager.isDragging, false);
    assert.deepEqual(dragEvents, [
      ['start', 'copy'],
      ['drop', 'plain text', 'move'],
      ['complete', 'move'],
    ]);

    const app = NativeApplication.nativeApplication;
    assert.equal(app.applicationID, 'org.neoflash.my-air-workbench');
    const desc = app.applicationDescriptor;
    const airUri = 'http://ns.adobe.com/air/application/32.0';
    assert.equal(desc.localName(), 'application');
    assert.equal(desc.child(new h.xml.QName(airUri, 'id')).toString(), 'org.neoflash.my-air-workbench');
    assert.equal(desc.child(new h.xml.QName(airUri, 'filename')).toString(), 'My AIR Workbench');
    assert.equal(app.icon.width, 128);
    assert.throws(
      () => {
        app.idleThreshold = 1;
      },
      err => err.name === 'ArgumentError'
    );
    app.idleThreshold = 60;
    assert.equal(app.idleThreshold, 60);
    assert.throws(
      () => {
        app.startAtLogin = true;
      },
      err => err.name === 'IllegalOperationError'
    );

    assert.equal(NativeProcess.isSupported, false);
    const procInfo = new NativeProcessStartupInfo();
    procInfo.arguments = ['--version'];
    assert.deepEqual(Array.from(procInfo.arguments), ['--version']);
    assert.throws(
      () => new NativeProcess().start(procInfo),
      err => err.name === 'IllegalOperationError'
    );
    assert.equal(Updater.isSupported, false);
    assert.throws(
      () => new Updater().update(null, '2.0'),
      err => err.name === 'IllegalOperationError'
    );

    const mainScreen = Screen.mainScreen;
    assert.ok(mainScreen.bounds.width >= 960);
    assert.equal(Screen.screens.length, 1);
    assert.equal(Screen.getScreensForRectangle(new h.geom.Rectangle(10, 10, 50, 50)).length, 1);

    const rootMenu = new NativeMenu();
    const fileSub = new NativeMenu();
    const openItem = new NativeMenuItem('Open', false);
    openItem.name = 'openCmd';
    openItem.keyEquivalent = 'o';
    fileSub.addItem(openItem);
    rootMenu.addSubmenu(fileSub, 'File');
    assert.equal(rootMenu.numItems, 1);
    assert.equal(fileSub.parent, rootMenu);
    assert.equal(fileSub.getItemByName('openCmd'), openItem);
    assert.throws(
      () => {
        openItem.submenu = rootMenu;
      },
      err => err.name === 'ArgumentError',
      'NativeMenu must reject ancestor cycles'
    );
    const clonedMenu = rootMenu.clone();
    assert.equal(clonedMenu.numItems, 1);
    assert.notEqual(clonedMenu.getItemAt(0), rootMenu.getItemAt(0));

    const mainWin = h.stage.nativeWindow;
    assert.equal(mainWin, app.activeWindow);
    assert.equal(mainWin.title, 'My AIR Workbench');

    assert.throws(
      () => {
        const bad = new NativeWindowInitOptions();
        bad.transparent = true;
        bad.systemChrome = NativeWindowSystemChrome.STANDARD;
        return new NativeWindow(bad);
      },
      err => err.name === 'ArgumentError'
    );

    const childOpts = new NativeWindowInitOptions();
    childOpts.type = NativeWindowType.UTILITY;
    childOpts.systemChrome = NativeWindowSystemChrome.NONE;
    childOpts.transparent = true;
    childOpts.owner = mainWin;
    const childWin = new NativeWindow(childOpts);
    assert.equal(mainWin.listOwnedWindows().length, 1);

    const winEvents = [];
    childWin.addEventListener('move', ev =>
      winEvents.push(['move', ev.afterBounds.x, ev.afterBounds.y])
    );
    childWin.addEventListener('resize', ev =>
      winEvents.push(['resize', ev.afterBounds.width, ev.afterBounds.height])
    );
    childWin.bounds = new h.geom.Rectangle(40, 50, 500, 360);
    assert.deepEqual(winEvents, [
      ['move', 40, 50],
      ['resize', 500, 360],
    ]);

    let blockNextStateChange = true;
    childWin.addEventListener('displayStateChanging', ev => {
      if (blockNextStateChange) ev.preventDefault();
    });
    childWin.minimize();
    assert.equal(childWin.displayState, NativeWindowDisplayState.NORMAL);
    blockNextStateChange = false;
    childWin.minimize();
    assert.equal(childWin.displayState, NativeWindowDisplayState.MINIMIZED);
    childWin.restore();
    assert.equal(childWin.displayState, NativeWindowDisplayState.NORMAL);

    const orientLog = [];
    h.stage.addEventListener('orientationChanging', ev => {
      orientLog.push(['changing', ev.beforeOrientation, ev.afterOrientation]);
    });
    h.stage.addEventListener('orientationChange', ev => {
      orientLog.push(['changed', ev.beforeOrientation, ev.afterOrientation]);
    });
    h.stage.setOrientation(StageOrientation.ROTATED_RIGHT);
    assert.equal(h.stage.orientation, StageOrientation.ROTATED_RIGHT);
    assert.deepEqual(orientLog, [
      ['changing', 'default', 'rotatedRight'],
      ['changed', 'default', 'rotatedRight'],
    ]);

    let preventExit = true;
    app.addEventListener('exiting', ev => {
      if (preventExit) ev.preventDefault();
    });
    app.exit();
    assert.equal(childWin.closed, false);
    preventExit = false;
    app.exit();
    assert.equal(childWin.closed, true);
    assert.equal(mainWin.closed, true);
  } finally {
    h.dispose();
  }
});

test('flash.data: EncryptedLocalStore, relational SQL engine, transactions, savepoints, joins, aggregates, schema, prefetch, async mode, and VFS persistence', async () => {
  await ready;
  const h = createAirHarness({ name: 'SQL Suite' });
  try {
    const EncryptedLocalStore = h.B['flash.data.EncryptedLocalStore'];
    const File = h.B['flash.filesystem.File'];
    const SQLConnection = h.B['flash.data.SQLConnection'];
    const SQLStatement = h.B['flash.data.SQLStatement'];
    const SQLMode = h.B['flash.data.SQLMode'];
    const SQLTableSchema = h.B['flash.data.SQLTableSchema'];
    const SQLError = h.B['flash.errors.SQLError'];

    assert.equal(EncryptedLocalStore.isSupported, true);
    const ba = new h.binary.ByteArray();
    ba.writeUTFBytes('els-value-42');
    EncryptedLocalStore.setItem('token', ba);
    const outBa = EncryptedLocalStore.getItem('token');
    assert.equal(outBa.readUTFBytes(outBa.bytesAvailable), 'els-value-42');
    EncryptedLocalStore.removeItem('token');
    assert.equal(EncryptedLocalStore.getItem('token'), null);

    const dbFile = File.applicationStorageDirectory.resolvePath('data/store.sqlite');
    const conn = new SQLConnection();
    const sqlUpdates = [];
    conn.addEventListener('insert', ev => sqlUpdates.push([ev.type, ev.table, ev.rowID]));
    conn.addEventListener('update', ev => sqlUpdates.push([ev.type, ev.table, ev.rowID]));
    conn.addEventListener('delete', ev => sqlUpdates.push([ev.type, ev.table, ev.rowID]));

    conn.open(dbFile, SQLMode.CREATE);
    assert.equal(conn.connected, true);

    const exec = (sql, params = {}, itemClass = null) => {
      const st = new SQLStatement();
      st.sqlConnection = conn;
      st.text = sql;
      st.itemClass = itemClass;
      Object.assign(st.parameters, params);
      st.execute();
      return st.getResult();
    };

    exec(
      'CREATE TABLE departments (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL COLLATE NOCASE)'
    );
    exec(
      'CREATE TABLE employees (id INTEGER PRIMARY KEY AUTOINCREMENT, dept_id INTEGER, name TEXT, salary REAL, badge BLOB)'
    );
    exec('CREATE INDEX idx_emp_dept ON employees (dept_id)');
    exec('CREATE VIEW high_earners AS SELECT name, salary FROM employees WHERE salary >= 100');

    exec('INSERT INTO departments (name) VALUES (:name)', { ':name': 'Engineering' });
    exec('INSERT INTO departments (name) VALUES (:name)', { ':name': 'Design' });

    const badgeBytes = new h.binary.ByteArray();
    badgeBytes.writeByte(0xca);
    badgeBytes.writeByte(0xfe);

    exec('INSERT INTO employees (dept_id, name, salary, badge) VALUES (?, ?, ?, ?)', {
      0: 1,
      1: 'Ada',
      2: 150.5,
      3: badgeBytes,
    });
    exec('INSERT INTO employees (dept_id, name, salary) VALUES (:d, :n, :s)', {
      ':d': 1,
      ':n': 'Grace',
      ':s': 130.0,
    });
    exec('INSERT INTO employees (dept_id, name, salary) VALUES (:d, :n, :s)', {
      ':d': 2,
      ':n': 'Dieter',
      ':s': 95.0,
    });

    assert.equal(conn.lastInsertRowID, 3);
    assert.equal(sqlUpdates.filter(u => u[0] === 'insert').length, 5);

    const nocase = exec("SELECT id, name FROM departments WHERE name = 'engineering'");
    assert.equal(nocase.data.length, 1);
    assert.equal(nocase.data[0].name, 'Engineering');

    const grouped = exec(`
      SELECT d.name AS dept, COUNT(e.id) AS headcount, SUM(e.salary) AS total_pay
      FROM departments d
      INNER JOIN employees e ON d.id = e.dept_id
      GROUP BY d.name
      HAVING COUNT(e.id) >= 1
      ORDER BY total_pay DESC
    `);
    assert.equal(grouped.data.length, 2);
    assert.equal(grouped.data[0].dept, 'Engineering');
    assert.equal(grouped.data[0].headcount, 2);
    assert.equal(grouped.data[0].total_pay, 280.5);

    const viewRes = exec('SELECT name, salary FROM high_earners ORDER BY salary DESC');
    assert.deepEqual(
      viewRes.data.map(r => r.name),
      ['Ada', 'Grace']
    );

    const blobRes = exec("SELECT badge FROM employees WHERE name = 'Ada'");
    const readBadge = blobRes.data[0].badge;
    assert.equal(readBadge.readUnsignedByte(), 0xca);
    assert.equal(readBadge.readUnsignedByte(), 0xfe);

    conn.begin();
    assert.equal(conn.inTransaction, true);
    exec("UPDATE employees SET salary = 200 WHERE name = 'Dieter'");
    conn.setSavepoint('sp1');
    exec("DELETE FROM employees WHERE name = 'Grace'");
    assert.equal(exec('SELECT COUNT(*) AS c FROM employees').data[0].c, 2);
    conn.rollbackToSavepoint('sp1');
    assert.equal(exec('SELECT COUNT(*) AS c FROM employees').data[0].c, 3);
    conn.commit();
    assert.equal(conn.inTransaction, false);
    assert.equal(exec("SELECT salary FROM employees WHERE name = 'Dieter'").data[0].salary, 200);

    const pagedStmt = new SQLStatement();
    pagedStmt.sqlConnection = conn;
    pagedStmt.text = 'SELECT name FROM employees ORDER BY id ASC';
    pagedStmt.execute(2);
    assert.equal(pagedStmt.executing, true);
    const page1 = pagedStmt.getResult();
    assert.equal(page1.complete, false);
    assert.deepEqual(
      page1.data.map(r => r.name),
      ['Ada', 'Grace']
    );
    pagedStmt.next(2);
    assert.equal(pagedStmt.executing, false);
    const page2 = pagedStmt.getResult();
    assert.equal(page2.complete, true);
    assert.deepEqual(
      page2.data.map(r => r.name),
      ['Dieter']
    );

    conn.loadSchema(SQLTableSchema, 'employees');
    const schema = conn.getSchemaResult();
    assert.equal(schema.tables.length, 1);
    assert.equal(schema.tables[0].name, 'employees');
    assert.equal(schema.tables[0].columns.length, 5);
    assert.equal(schema.tables[0].columns[0].primaryKey, true);

    conn.attach('aux', null);
    exec('CREATE TABLE aux.tags (label TEXT)');
    exec("INSERT INTO aux.tags (label) VALUES ('featured')");
    assert.equal(exec('SELECT label FROM aux.tags').data[0].label, 'featured');
    conn.detach('aux');

    conn.close();
    assert.equal(conn.connected, false);

    const reader = new SQLConnection();
    reader.open(dbFile, SQLMode.READ);
    const persisted = new SQLStatement();
    persisted.sqlConnection = reader;
    persisted.text = 'SELECT COUNT(*) AS c FROM employees';
    persisted.execute();
    assert.equal(persisted.getResult().data[0].c, 3);

    const illegalWrite = new SQLStatement();
    illegalWrite.sqlConnection = reader;
    illegalWrite.text = "DELETE FROM employees WHERE name = 'Ada'";
    assert.throws(
      () => illegalWrite.execute(),
      err => err instanceof SQLError
    );
    reader.close();

    const asyncConn = new SQLConnection();
    let asyncResult = null;
    asyncConn.openAsync(dbFile, SQLMode.UPDATE, {
      result() {
        const st = new SQLStatement();
        st.sqlConnection = asyncConn;
        st.text = 'SELECT name FROM departments ORDER BY id ASC';
        st.execute(-1, {
          result(res) {
            asyncResult = res.data.map(r => r.name);
          },
          status(err) {
            throw err;
          },
        });
      },
      status(err) {
        throw err;
      },
    });
    h.flushTasks();
    assert.deepEqual(asyncResult, ['Engineering', 'Design']);
    asyncConn.close();
    h.flushTasks();
  } finally {
    h.dispose();
  }
});

test('flash.events, flash.errors & companion AIR classes: event cloning/formatting, DNS records, notifications, permissions, and sensors', async () => {
  await ready;
  const h = createAirHarness();
  try {
    const B = h.B;
    const sampleEvents = [
      new B['flash.events.InvokeEvent']('invoke', false, false, null, ['--open'], 'standard'),
      new B['flash.events.BrowserInvokeEvent'](
        'browserInvoke',
        false,
        false,
        ['a'],
        'app1',
        'pub1',
        'https://example.invalid'
      ),
      new B['flash.events.FileListEvent']('directoryListing', false, false, []),
      new B['flash.events.OutputProgressEvent']('outputProgress', false, false, 10, 50),
      new B['flash.events.NativeWindowBoundsEvent'](
        'move',
        false,
        false,
        new h.geom.Rectangle(0, 0, 10, 10),
        new h.geom.Rectangle(5, 5, 10, 10)
      ),
      new B['flash.events.NativeWindowDisplayStateEvent'](
        'displayStateChange',
        false,
        false,
        'normal',
        'maximized'
      ),
      new B['flash.events.NativeDragEvent']('nativeDragDrop'),
      new B['flash.events.ScreenMouseEvent']('click', false, false, 12, 34),
      new B['flash.events.SQLEvent']('result'),
      new B['flash.events.SQLErrorEvent'](
        'error',
        false,
        false,
        new B['flash.errors.SQLError']('execute', 'syntax error', '', 3115)
      ),
      new B['flash.events.SQLUpdateEvent']('insert', false, false, 'todos', 7),
      new B['flash.events.StorageVolumeChangeEvent']('storageVolumeMount'),
      new B['flash.events.NativeProcessExitEvent']('exit', false, false, 0),
      new B['flash.events.LocationChangeEvent'](
        'locationChange',
        false,
        false,
        'https://example.invalid'
      ),
      new B['flash.events.HTMLUncaughtScriptExceptionEvent'](new Error('boom')),
      new B['flash.events.MediaEvent']('complete'),
      new B['flash.events.DatagramSocketDataEvent'](
        'data',
        false,
        false,
        '127.0.0.1',
        9000,
        '127.0.0.1',
        9001,
        new h.binary.ByteArray()
      ),
      new B['flash.events.ServerSocketConnectEvent']('connect'),
      new B['flash.events.DNSResolverEvent']('lookup', false, false, 'example.com', []),
      new B['flash.events.DRMAuthenticateEvent'](
        'drmAuthenticate',
        false,
        false,
        'hdr',
        'u',
        'p',
        'url',
        'drm'
      ),
      new B['flash.events.DeviceRotationEvent']('update', false, false, 100, 1, 2, 3, [0, 0, 0, 1]),
      new B['flash.events.PermissionEvent']('permissionStatus', false, false, 'granted'),
      new B['flash.events.RemoteNotificationEvent'](
        'notification',
        false,
        false,
        { alert: 'hi' },
        'tok'
      ),
      new B['flash.events.GameInputEvent']('deviceAdded'),
      new B['flash.events.VsyncStateChangeAvailabilityEvent'](
        'VsyncStateChangeAvailability',
        false,
        false,
        true
      ),
    ];

    for (const ev of sampleEvents) {
      const copy = ev.clone();
      assert.notEqual(copy, ev);
      assert.equal(copy.type, ev.type);
      assert.ok(copy.toString().includes(ev.type), copy.toString());
    }

    assert.equal(h.net.ProgressEvent.STANDARD_OUTPUT_DATA, 'standardOutputData');
    assert.equal(h.net.IOErrorEvent.STANDARD_ERROR_IO_ERROR, 'standardErrorIoError');
    assert.equal(h.net.HTTPStatusEvent.HTTP_RESPONSE_STATUS, 'httpResponseStatus');

    const permErr = new B['flash.errors.PermissionError']('denied', 3800);
    assert.equal(permErr.errorID, 3800);
    const drmErr = new B['flash.errors.DRMManagerError']('drm fail', 3300, 12);
    assert.equal(drmErr.subErrorID, 12);

    assert.equal(B['flash.permissions.PermissionStatus'].ONLY_WHEN_IN_USE, 'onlyWhenInUse');
    const srv = new B['flash.net.dns.SRVRecord']();
    srv.target = 'sip.example.com';
    srv.port = 5060;
    assert.equal(srv.port, 5060);
    assert.equal(B['flash.net.dns.DNSResolver'].isSupported, false);
    assert.throws(
      () => new B['flash.net.dns.DNSResolver']().lookup('example.com'),
      err => err.name === 'IllegalOperationError'
    );

    const notifier = new B['flash.notifications.RemoteNotifier']();
    let statusReceived = false;
    notifier.addEventListener('status', ev => {
      statusReceived = ev.level === 'error';
    });
    notifier.subscribe(new B['flash.notifications.RemoteNotifierSubscribeOptions']());
    h.flushTasks();
    assert.equal(statusReceived, true);

    const geo = new B['flash.sensors.Geolocation']();
    assert.equal(B['flash.sensors.Geolocation'].LOCATION_ACCURACY_HUNDRED_METERS, 100);
    let geoPerm = null;
    geo.addEventListener('permissionStatus', ev => {
      geoPerm = ev.status;
    });
    geo.requestPermission();
    h.flushTasks();
    assert.equal(geoPerm, 'denied');

    const preResetFile = B['flash.filesystem.File'].applicationStorageDirectory.resolvePath(
      'ephemeral.txt'
    );
    const fsOut = new B['flash.filesystem.FileStream']();
    fsOut.open(preResetFile, 'write');
    fsOut.writeUTFBytes('temp');
    fsOut.close();
    assert.equal(preResetFile.exists, true);
    h.air.reset();
    assert.equal(preResetFile.exists, false);
  } finally {
    h.dispose();
  }
});
