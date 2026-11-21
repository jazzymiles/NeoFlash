'use strict';

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const http = require('node:http');
const net = require('node:net');
const { homedir } = require('node:os');
const { resolve } = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

const root = resolve(__dirname, '../..');
const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

function compileBatch(list, prefix) {
  const localCargo = resolve(homedir(), '.cargo/bin/cargo');
  const cargoBin = process.env.CARGO || (fs.existsSync(localCargo) ? localCargo : 'cargo');
  const result = spawnSync(
    cargoBin,
    ['run', '--quiet', '--locked', '-p', 'neoflash-script', '--example', 'compile_batch'],
    { cwd: root, encoding: 'utf8', input: JSON.stringify(list), maxBuffer: 8 * 1024 * 1024 },
  );
  assert.equal(result.status, 0, result.stderr);
  const outputs = JSON.parse(result.stdout);
  return list.map((script, index) => {
    assert.ok(outputs[index].Ok, JSON.stringify(outputs[index]));
    return { script, code: outputs[index].Ok, location: `${prefix} ${index}` };
  });
}

async function until(predicate, label, timeoutMs = 4000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise(r => setTimeout(r, 10));
  }
  assert.ok(predicate(), `Timed out waiting for: ${label}`);
}

function encodeServerWsFrame(payload, opcode = 0x01) {
  const buf = Buffer.isBuffer(payload) ? payload : Buffer.from(payload, 'utf8');
  let header;
  if (buf.length < 126) {
    header = Buffer.from([0x80 | opcode, buf.length]);
  } else if (buf.length < 65536) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(buf.length, 2);
  } else {
    throw new Error('Test frame too large');
  }
  return Buffer.concat([header, buf]);
}

function startRfc6455Server(onClientConnected) {
  const connections = new Set();
  const server = http.createServer((_req, res) => {
    res.writeHead(426);
    res.end('Upgrade Required');
  });

  server.on('upgrade', (req, socket, head) => {
    socket.setNoDelay(true);
    connections.add(socket);
    socket.on('close', () => connections.delete(socket));

    const key = req.headers['sec-websocket-key'];
    const accept = crypto.createHash('sha1').update(String(key) + WS_GUID).digest('base64');
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\n' +
        'Upgrade: websocket\r\n' +
        'Connection: Upgrade\r\n' +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );

    let rx = head && head.length ? Buffer.from(head) : Buffer.alloc(0);
    const clientApi = {
      url: req.url,
      rawFrames: [],
      closedByClient: false,
      sendText(str) {
        socket.write(encodeServerWsFrame(Buffer.from(str, 'utf8'), 0x01));
      },
      sendBinary(bytes) {
        socket.write(encodeServerWsFrame(Buffer.from(bytes), 0x02));
      },
      closeFromServer() {
        try {
          socket.write(Buffer.from([0x88, 0x02, 0x03, 0xe8]));
          socket.end();
        } catch (_) {}
      },
      onFrame: null,
    };

    const parseIncoming = () => {
      while (rx.length >= 2) {
        const b0 = rx[0];
        const b1 = rx[1];
        const opcode = b0 & 0x0f;
        const masked = (b1 & 0x80) !== 0;
        let len = b1 & 0x7f;
        let offset = 2;
        if (len === 126) {
          if (rx.length < 4) return;
          len = rx.readUInt16BE(2);
          offset = 4;
        } else if (len === 127) {
          if (rx.length < 10) return;
          len = Number(rx.readBigUInt64BE(2));
          offset = 10;
        }
        const maskLen = masked ? 4 : 0;
        if (rx.length < offset + maskLen + len) return;
        const mask = masked ? rx.subarray(offset, offset + 4) : null;
        offset += maskLen;
        const payload = Buffer.from(rx.subarray(offset, offset + len));
        rx = rx.subarray(offset + len);
        if (mask) {
          for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i & 3];
        }
        if (opcode === 0x08) {
          clientApi.closedByClient = true;
          try {
            if (!socket.writableEnded && !socket.destroyed) {
              socket.write(Buffer.from([0x88, 0x00]));
              socket.end();
            }
          } catch (_) {}
          return;
        }
        if (opcode === 0x01 || opcode === 0x02) {
          clientApi.rawFrames.push(payload);
          clientApi.onFrame?.(payload);
        }
      }
    };

    socket.on('data', chunk => {
      rx = Buffer.concat([rx, chunk]);
      parseIncoming();
    });
    onClientConnected?.(clientApi, socket);
    if (rx.length) parseIncoming();
  });

  return new Promise((resolveServer, reject) => {
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      resolveServer({
        port,
        close: () =>
          new Promise(r => {
            for (const s of connections) {
              try {
                s.destroy();
              } catch (_) {}
            }
            server.close(() => r());
          }),
      });
    });
    server.once('error', reject);
  });
}

function makeProjectWithAs2Script(script) {
  return {
    format_version: 6,
    name: 'AS2Area8Conformance',
    width: 200,
    height: 150,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'scene-1',
    scenes: ['scene-1'],
    symbols: [
      {
        id: 'scene-1',
        name: 'Scene 1',
        kind: 'movie_clip',
        timeline: {
          frame_count: 1,
          layers: [{ id: 'l1', name: 'Layer 1', visible: true, locked: false, keyframes: [{ frame: 0, elements: [], label: '', tween: null }] }],
          scripts: [{ frame: 0, script }],
        },
      },
    ],
  };
}

test('AS2 XMLSocket end-to-end over real loopback RFC 6455 WebSocket server (connect, timeout, \\0 framing, XML/XMLNode send, onXML, onData override, remote vs local close)', async () => {
  assert.equal(typeof globalThis.WebSocket, 'function', 'Node runtime provides native RFC 6455 WebSocket');
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');

  const serverClients = [];
  const wsServer = await startRfc6455Server(client => {
    serverClients.push(client);
    client.onFrame = payload => {
      const text = payload.subarray(0, payload.length - 1).toString('utf8');
      if (text.includes('<hello')) {
        // Split one XML frame across 3 chunks, then pack 2 XML frames into a single WebSocket message
        client.sendText('<reply status="');
        setTimeout(() => {
          client.sendText('ok"><item code="1"/></rep');
          setTimeout(() => {
            client.sendText('ly>\0<batch n="2"/>\0<batch n="3"/>\0');
          }, 10);
        }, 10);
      } else if (text.includes('trigger-remote-close')) {
        client.closeFromServer();
      } else if (client.url === '/raw-endpoint') {
        // Send binary frame containing two NUL-delimited raw strings
        client.sendBinary(Buffer.from('alpha-raw\0beta-raw\0', 'utf8'));
      }
    };
  });

  // Stalled TCP listener that accepts TCP SYN/ACK but never replies to HTTP Upgrade (proves real timeout!)
  const stalledSockets = new Set();
  const stalledServer = net.createServer(sock => {
    stalledSockets.add(sock);
    sock.on('close', () => stalledSockets.delete(sock));
  });
  await new Promise(r => stalledServer.listen(0, '127.0.0.1', r));
  const stalledPort = stalledServer.address().port;

  // Unused port (closed immediately to guarantee ECONNREFUSED)
  const closedDummy = net.createServer();
  await new Promise(r => closedDummy.listen(0, '127.0.0.1', r));
  const refusedPort = closedDummy.address().port;
  await new Promise(r => closedDummy.close(r));

  const as2Source = `
    stop();
    var events:Array = [];

    // 1) Primary XMLSocket using default prototype onData -> XML parse -> onXML + remote close
    var s1:XMLSocket = new XMLSocket();
    events.push("defaultTimeout:" + s1.timeout);
    var lowPort:Boolean = s1.connect("127.0.0.1", 80);
    events.push("lowPort:" + lowPort);
    s1.onConnect = function(ok:Boolean):Void {
      events.push("s1.connect:" + ok);
      if (ok) {
        var doc:XML = new XML('<hello from="as2"><child v="7"/></hello>');
        this.send(doc);
        this.send(doc.firstChild.firstChild);
      }
    };
    s1.onXML = function(parsed:XML):Void {
      events.push("s1.xml:" + parsed.firstChild.nodeName + ":" + (parsed.firstChild.attributes.status || parsed.firstChild.attributes.n));
      if (parsed.firstChild.attributes.n == "3") {
        this.send("trigger-remote-close");
      }
    };
    s1.onClose = function():Void {
      events.push("s1.close:" + arguments.length);
    };
    var s1Started:Boolean = s1.connect("127.0.0.1", ${wsServer.port});
    events.push("s1.started:" + s1Started);

    // 2) Explicit ws:// URL + custom onData override intercepting raw frames BEFORE onXML + local close()
    var s2:XMLSocket = new XMLSocket();
    s2.onConnect = function(ok:Boolean):Void {
      events.push("s2.connect:" + ok);
      if (ok) this.send("raw-ping");
    };
    s2.onData = function(raw:String):Void {
      events.push("s2.raw:" + raw);
      if (raw == "beta-raw") {
        this.close();
        events.push("s2.localClosed");
      }
    };
    s2.onXML = function(unexpected:XML):Void {
      events.push("s2.unexpectedOnXML");
    };
    s2.onClose = function():Void {
      events.push("s2.unexpectedOnClose");
    };
    var s2Started:Boolean = s2.connect("ws://127.0.0.1:${wsServer.port}/raw-endpoint", ${wsServer.port});
    events.push("s2.started:" + s2Started);

    // 3) Connection refusal -> onConnect(false)
    var sRefused:XMLSocket = new XMLSocket();
    sRefused.onConnect = function(ok:Boolean):Void {
      events.push("refused.connect:" + ok);
    };
    sRefused.connect("127.0.0.1", ${refusedPort});

    // 4) Stalled handshake exceeding timeout -> onConnect(false)
    var sTimeout:XMLSocket = new XMLSocket();
    sTimeout.timeout = 90;
    sTimeout.onConnect = function(ok:Boolean):Void {
      events.push("timeout.connect:" + ok);
    };
    sTimeout.connect("127.0.0.1", ${stalledPort});
  `;

  const script = { language: 'action_script2', source: as2Source };
  const compiled = compileBatch([script], 'as2-xmlsocket');
  const project = makeProjectWithAs2Script(script);
  const player = new runtimeWasm.WebPlayer(JSON.stringify(project));

  try {
    const { host } = createRuntimeHost(root, player, compiled, {
      connections: {
        url: 'http://127.0.0.1:8080/index.html',
      },
    });
    host.dispatchScripts();
    const rootClip = host.proxyFor([]);

    await until(() => {
      const ev = Array.from(rootClip.events || []);
      return (
        ev.includes('s1.close:0') &&
        ev.includes('s2.localClosed') &&
        ev.includes('refused.connect:false') &&
        ev.includes('timeout.connect:false')
      );
    }, 'all 4 real loopback XMLSocket scenarios complete');

    // Give a brief grace window to verify local close() never triggers s2.onClose asynchronously
    await new Promise(r => setTimeout(r, 40));

    const log = Array.from(rootClip.events);
    assert.ok(log.includes('defaultTimeout:20000'));
    assert.ok(log.includes('lowPort:false'));
    assert.ok(log.includes('s1.started:true'));
    assert.ok(log.includes('s1.connect:true'));
    assert.ok(log.includes('s1.xml:reply:ok'));
    assert.ok(log.includes('s1.xml:batch:2'));
    assert.ok(log.includes('s1.xml:batch:3'));
    assert.ok(log.includes('s1.close:0'), 'remote close fires onClose() with 0 args');

    assert.ok(log.includes('s2.started:true'));
    assert.ok(log.includes('s2.connect:true'));
    assert.ok(log.includes('s2.raw:alpha-raw'));
    assert.ok(log.includes('s2.raw:beta-raw'));
    assert.ok(log.includes('s2.localClosed'));
    assert.ok(!log.includes('s2.unexpectedOnXML'), 'overriding onData intercepts raw string before onXML');
    assert.ok(!log.includes('s2.unexpectedOnClose'), 'local close() must NOT fire onClose');

    assert.ok(log.includes('refused.connect:false'));
    assert.ok(log.includes('timeout.connect:false'));

    // Verify every outbound Flash XMLSocket frame carried the mandatory trailing 0x00 NUL byte on the wire
    const s1ServerSide = serverClients.find(c => c.url === '/');
    assert.ok(s1ServerSide, 'primary XMLSocket connected to /');
    assert.equal(s1ServerSide.rawFrames.length, 3);
    for (const rawFrame of s1ServerSide.rawFrames) {
      assert.equal(rawFrame[rawFrame.length - 1], 0x00, 'outbound XMLSocket frame ends with mandatory 0x00 byte');
    }
    assert.equal(
      s1ServerSide.rawFrames[0].subarray(0, -1).toString('utf8'),
      '<hello from="as2"><child v="7" /></hello>'
        .replace(' />', '/>') === s1ServerSide.rawFrames[0].subarray(0, -1).toString('utf8')
        ? '<hello from="as2"><child v="7"/></hello>'
        : s1ServerSide.rawFrames[0].subarray(0, -1).toString('utf8'),
    );
    assert.match(s1ServerSide.rawFrames[0].subarray(0, -1).toString('utf8'), /^<hello from="as2"><child v="7"\s*\/><\/hello>$/);
    assert.match(s1ServerSide.rawFrames[1].subarray(0, -1).toString('utf8'), /^<child v="7"\s*\/>$/);
  } finally {
    player.free();
    for (const s of stalledSockets) s.destroy();
    await new Promise(r => stalledServer.close(r));
    await wsServer.close();
  }
});

test('AS2 System.security (sandboxType, allowDomain/allowInsecureDomain wildcards, and real HTTP /crossdomain.xml loadPolicyFile enforcement)', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');

  // Start two real loopback WebSocket servers: one on an allowed port, one on a disallowed port
  const allowedWs = await startRfc6455Server();
  const blockedWs = await startRfc6455Server();

  // Real local HTTP server serving /crossdomain.xml and /denied-crossdomain.xml
  const policyRequests = [];
  const policyServer = http.createServer((req, res) => {
    policyRequests.push(req.url);
    if (req.url === '/crossdomain.xml') {
      const xml = `<?xml version="1.0"?>
<!DOCTYPE cross-domain-policy SYSTEM "http://www.macromedia.com/xml/dtds/cross-domain-policy.dtd">
<cross-domain-policy>
  <site-control permitted-cross-domain-policies="all"/>
  <allow-access-from domain="*.trusted.example" secure="false" to-ports="${allowedWs.port}"/>
</cross-domain-policy>`;
      res.writeHead(200, { 'Content-Type': 'text/x-cross-domain-policy' });
      res.end(xml);
      return;
    }
    res.writeHead(404);
    res.end('Not found');
  });
  await new Promise(r => policyServer.listen(0, '127.0.0.1', r));
  const httpPort = policyServer.address().port;

  // Verify sandboxType across http:, file: (localWithFile), and file: + localTrusted (localTrusted)
  const NeoFlashAS2Platform = require('../as2-platform.js');
  const filePlatform = NeoFlashAS2Platform.create(runtimeWasm, {
    global: { location: { protocol: 'file:', hostname: '' }, navigator: {}, screen: {} },
  });
  const trustedPlatform = NeoFlashAS2Platform.create(runtimeWasm, {
    global: { location: { protocol: 'file:', hostname: '' }, navigator: {}, screen: {} },
    localTrusted: true,
  });
  assert.equal(filePlatform.System.security.sandboxType, 'localWithFile');
  assert.equal(trustedPlatform.System.security.sandboxType, 'localTrusted');
  filePlatform.dispose();
  trustedPlatform.dispose();

  // Run real AS2 script exercising System.security.sandboxType, allowDomain, allowInsecureDomain,
  // loadPolicyFile over real HTTP fetch, and XMLSocket port enforcement!
  const peers = new Set();
  class SharedBC {
    constructor() {
      this.onmessage = null;
      peers.add(this);
    }
    postMessage(data) {
      for (const peer of peers) if (peer !== this) peer.onmessage?.({ data });
    }
    close() {
      peers.delete(this);
    }
  }

  const secScript = {
    language: 'action_script2',
    source: `
      stop();
      var secLog:Array = [];
      secLog.push("sandbox:" + System.security.sandboxType);

      // Register wildcard & exact domains via System.security
      System.security.allowDomain("*.partner.example", "exact.org");
      System.security.allowInsecureDomain("*.http-allowed.example");

      var lc:LocalConnection = new LocalConnection();
      lc.onPing = function(tag:String):Void { secLog.push("lc:" + tag); };
      lc.connect("sec_bus");

      // Load real crossdomain.xml from local HTTP server, then connect to allowed vs disallowed port
      System.security.loadPolicyFile("http://127.0.0.1:${httpPort}/crossdomain.xml");

      var xsAllowed:XMLSocket = new XMLSocket();
      xsAllowed.onConnect = function(ok:Boolean):Void {
        secLog.push("xsAllowed:" + ok);
        if (ok) this.close();
      };
      xsAllowed.connect("127.0.0.1", ${allowedWs.port});

      var xsBlocked:XMLSocket = new XMLSocket();
      xsBlocked.onConnect = function(ok:Boolean):Void {
        secLog.push("xsBlocked:" + ok);
      };
      xsBlocked.connect("127.0.0.1", ${blockedWs.port});
    `,
  };

  const compiled = compileBatch([secScript], 'as2-security');
  const project = makeProjectWithAs2Script(secScript);
  const player = new runtimeWasm.WebPlayer(JSON.stringify(project));

  try {
    const { host } = createRuntimeHost(root, player, compiled, {
      platform: {
        global: {
          location: { protocol: 'https:', hostname: 'portal.trusted.example', host: 'portal.trusted.example' },
          navigator: { language: 'en-US', platform: 'MacIntel' },
          screen: { width: 1280, height: 800, colorDepth: 24 },
          fetch: globalThis.fetch,
        },
      },
      connections: {
        url: 'https://portal.trusted.example/app.html',
        BroadcastChannel: SharedBC,
        locks: { request: async (_n, _o, cb) => cb({}) },
      },
    });
    host.dispatchScripts();
    const rootClip = host.proxyFor([]);

    await until(() => {
      const log = Array.from(rootClip.secLog || []);
      return log.includes('xsAllowed:true') && log.includes('xsBlocked:false');
    }, 'real HTTP /crossdomain.xml policy fetch and XMLSocket port enforcement settle');

    assert.deepEqual(policyRequests, ['/crossdomain.xml']);

    // Inject cross-domain LocalConnection frames to prove System.security.allowDomain / allowInsecureDomain enforcement
    const injectLc = (domain, proto, id, tag) => {
      const wire = JSON.stringify({
        version: 1,
        sender: 'ext-peer',
        sender_domain: domain,
        sender_protocol: proto,
        id,
        target: 'https://portal.trusted.example|sec_bus',
        method: 'onPing',
        body: host.amf().base64(host.amf().encode([tag], 0)),
      });
      for (const peer of peers) peer.onmessage?.({ data: wire });
    };

    await until(() => peers.size >= 1, 'LocalConnection bus active');
    // Wait one microtask for async lock claim
    await new Promise(r => setTimeout(r, 15));

    injectLc('untrusted.evil.example', 'https:', 10, 'blocked-domain');
    injectLc('sub.partner.example', 'https:', 11, 'wildcard-https-ok');
    injectLc('sub.partner.example', 'http:', 12, 'blocked-insecure-http');
    injectLc('legacy.http-allowed.example', 'http:', 13, 'wildcard-insecure-ok');

    const finalLog = Array.from(rootClip.secLog);
    assert.equal(finalLog[0], 'sandbox:remote');
    assert.ok(finalLog.includes('xsAllowed:true'), 'allowed port in crossdomain.xml connects');
    assert.ok(finalLog.includes('xsBlocked:false'), 'disallowed port in crossdomain.xml is rejected');
    assert.deepEqual(finalLog.slice(3), [
      'lc:wildcard-https-ok',
      'lc:wildcard-insecure-ok',
    ]);
  } finally {
    player.free();
    await new Promise(r => policyServer.close(r));
    await allowedWs.close();
    await blockedWs.close();
  }
});

test('AS2 MMExecute routes through NeoFlash authoring bridge in Test Movie mode and returns empty string in standalone HTML5 mode', async () => {
  const { createRuntimeHost } = await import('../../crates/neoflash-script/tests/host-integration.mjs');
  const { runtimeWasm } = await import('../../crates/neoflash-script/tests/rust-runtime-host.mjs');

  const mmScript = {
    language: 'action_script2',
    source: `
      stop();
      var domName:String = MMExecute("fl.getDocumentDOM().name");
      var emptyCmd:String = MMExecute("");
      var resultSummary:String = domName + "|" + emptyCmd + "|" + typeof(domName);
    `,
  };
  const compiled = compileBatch([mmScript], 'as2-mmexecute');
  const project = makeProjectWithAs2Script(mmScript);

  // Mode 1: Standalone / published HTML5 mode (no authoring bridge -> returns "" without throwing)
  {
    const player = new runtimeWasm.WebPlayer(JSON.stringify(project));
    try {
      const standaloneGlobal = { location: { href: 'https://example.test/index.html', protocol: 'https:' } };
      const { host } = createRuntimeHost(root, player, compiled, {
        external: { hostObject: {}, hostGlobal: standaloneGlobal },
      });
      host.dispatchScripts();
      assert.equal(host.proxyFor([]).resultSummary, '||string');
    } finally {
      player.free();
    }
  }

  // Mode 2: Authoring Test Movie mode (window.__neoflashMMExecute + preview output relay active)
  {
    const player = new runtimeWasm.WebPlayer(JSON.stringify(project));
    const executedJsfl = [];
    const relayedRecords = [];
    try {
      const authoringGlobal = {
        location: { href: 'http://127.0.0.1:4123/index.html', protocol: 'http:' },
        __neoflashMMExecute(expr) {
          executedJsfl.push(expr);
          if (expr === 'fl.getDocumentDOM().name') return 'HeroBanner.nflash';
          return 'ok';
        },
        __neoflashOutputRelay: {
          disabled: false,
          record(kind, message) {
            relayedRecords.push([kind, message]);
          },
        },
      };
      const { host } = createRuntimeHost(root, player, compiled, {
        external: { hostObject: {}, hostGlobal: authoringGlobal },
      });
      host.dispatchScripts();
      assert.equal(host.proxyFor([]).resultSummary, 'HeroBanner.nflash||string');
      assert.deepEqual(executedJsfl, ['fl.getDocumentDOM().name']);
      assert.deepEqual(relayedRecords, [['mmexecute', 'fl.getDocumentDOM().name']]);
    } finally {
      player.free();
    }
  }
});
