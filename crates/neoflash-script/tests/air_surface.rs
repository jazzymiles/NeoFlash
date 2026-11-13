use neoflash_core::{Project, ResourceUnit, Script, ScriptLanguage, SourceUnit};
use std::io::{self, Write};
use std::process::{Command, Output, Stdio};

fn run_node_module(source: &str) -> io::Result<Output> {
    let mut child = Command::new("node")
        .args(["--input-type=module", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Node stdin was not piped"))?;
    stdin.write_all(source.as_bytes())?;
    drop(stdin);
    child.wait_with_output()
}

fn air_project(sources: &[(&str, &str)]) -> Project {
    let mut project = Project::blank();
    project.name = "AIR Desktop Suite".into();
    project.width = 1024;
    project.height = 768;
    project.format_version = 3;
    project.sources = sources
        .iter()
        .enumerate()
        .map(|(i, (path, source))| SourceUnit {
            id: format!("air-src-{i}"),
            path: (*path).into(),
            language: ScriptLanguage::ActionScript3,
            source: (*source).into(),
        })
        .collect();
    project
}

#[test]
fn as3_air_packages_interfaces_and_typed_vectors_compile_cleanly() {
    let proj = air_project(&[
        (
            "app/MemoryPromise.as",
            r#"
            package app {
                import flash.desktop.IFilePromise;
                import flash.events.ErrorEvent;
                import flash.utils.ByteArray;
                import flash.utils.IDataInput;

                public class MemoryPromise implements IFilePromise {
                    private var _path:String;
                    private var _bytes:ByteArray;
                    public var lastError:String = "";

                    public function MemoryPromise(relPath:String, payload:String) {
                        this._path = relPath;
                        this._bytes = new ByteArray();
                        this._bytes.writeUTFBytes(payload);
                        this._bytes.position = 0;
                    }

                    public function get relativePath():String {
                        return this._path;
                    }

                    public function get isAsync():Boolean {
                        return false;
                    }

                    public function open():IDataInput {
                        this._bytes.position = 0;
                        return this._bytes;
                    }

                    public function close():void {}

                    public function reportError(e:ErrorEvent):void {
                        this.lastError = e.text;
                    }
                }
            }
            "#,
        ),
        (
            "app/TodoRow.as",
            r#"
            package app {
                public class TodoRow {
                    public var id:int;
                    public var title:String;
                    public var done:int;
                    public function TodoRow() {}
                }
            }
            "#,
        ),
        (
            "app/AirController.as",
            r#"
            package app {
                import flash.filesystem.*;
                import flash.desktop.*;
                import flash.display.*;
                import flash.data.*;
                import flash.errors.*;
                import flash.events.*;
                import flash.permissions.PermissionStatus;
                import flash.net.dns.*;
                import flash.notifications.*;
                import flash.sensors.*;
                import flash.utils.ByteArray;
                import flash.utils.IDataInput;
                import flash.utils.IDataOutput;

                public class AirController {
                    public static function verifyTypes():String {
                        var dir:File = File.applicationStorageDirectory.resolvePath("notes");
                        dir.createDirectory();
                        var noteFile:File = dir.resolvePath("hello.txt");
                        var fs:FileStream = new FileStream();
                        fs.open(noteFile, FileMode.WRITE);
                        var out:IDataOutput = fs as IDataOutput;
                        out.writeUTF("NeoFlash AIR");
                        fs.close();

                        fs.open(noteFile, FileMode.READ);
                        var inp:IDataInput = fs as IDataInput;
                        var msg:String = inp.readUTF();
                        fs.close();

                        var vols:Vector.<StorageVolume> = StorageVolumeInfo.storageVolumeInfo.getStorageVolumes();
                        var promise:IFilePromise = new MemoryPromise("export/item.txt", msg);
                        var promiseStream:IDataInput = promise.open();
                        var promisedText:String = promiseStream.readUTFBytes(msg.length);
                        promise.close();

                        var elsBytes:ByteArray = new ByteArray();
                        elsBytes.writeUTFBytes("secret-token");
                        EncryptedLocalStore.setItem("auth", elsBytes);
                        var loadedEls:ByteArray = EncryptedLocalStore.getItem("auth");
                        var secret:String = loadedEls.readUTFBytes(loadedEls.length);

                        var dbFile:File = File.applicationStorageDirectory.resolvePath("app.db");
                        var conn:SQLConnection = new SQLConnection();
                        conn.open(dbFile, SQLMode.CREATE);

                        var ddl:SQLStatement = new SQLStatement();
                        ddl.sqlConnection = conn;
                        ddl.text = "CREATE TABLE todos (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, done INTEGER)";
                        ddl.execute();

                        var ins:SQLStatement = new SQLStatement();
                        ins.sqlConnection = conn;
                        ins.text = "INSERT INTO todos (title, done) VALUES (:title, :done)";
                        ins.parameters[":title"] = "Ship AIR";
                        ins.parameters[":done"] = 1;
                        ins.execute();

                        var sel:SQLStatement = new SQLStatement();
                        sel.sqlConnection = conn;
                        sel.itemClass = TodoRow;
                        sel.text = "SELECT id, title, done FROM todos";
                        sel.execute();
                        var res:SQLResult = sel.getResult();
                        var firstRow:TodoRow = res.data[0] as TodoRow;
                        conn.close();

                        var app:NativeApplication = NativeApplication.nativeApplication;
                        var desc:XML = app.applicationDescriptor;
                        var airNs:Namespace = desc.namespace();
                        var childOpts:NativeWindowInitOptions = new NativeWindowInitOptions();
                        childOpts.type = NativeWindowType.UTILITY;
                        childOpts.systemChrome = NativeWindowSystemChrome.NONE;
                        childOpts.transparent = true;
                        childOpts.owner = app.activeWindow;
                        var childWin:NativeWindow = new NativeWindow(childOpts);
                        var owned:Vector.<NativeWindow> = app.activeWindow.listOwnedWindows();

                        var menu:NativeMenu = new NativeMenu();
                        var item:NativeMenuItem = new NativeMenuItem("Save", false);
                        menu.addItem(item);
                        childWin.menu = menu;

                        var startup:NativeProcessStartupInfo = new NativeProcessStartupInfo();
                        startup.executable = noteFile;
                        var args:Vector.<String> = new Vector.<String>();
                        args.push("--headless");
                        startup.arguments = args;

                        var aRec:ARecord = new ARecord();
                        aRec.address = "127.0.0.1";
                        var subOpts:RemoteNotifierSubscribeOptions = new RemoteNotifierSubscribeOptions();
                        var geo:Geolocation = new Geolocation();
                        geo.setRequestedUpdateInterval(100);
                        var rot:DeviceRotation = new DeviceRotation();

                        childWin.close();

                        return [
                            promisedText,
                            vols.length,
                            secret,
                            firstRow.id,
                            firstRow.title,
                            firstRow.done,
                            desc.airNs::versionNumber.toString(),
                            owned.length,
                            menu.numItems,
                            NativeProcess.isSupported,
                            PermissionStatus.GRANTED,
                            aRec.address,
                            subOpts.notificationStyles.length,
                            rot.isSupported ? "1" : "0"
                        ].join("|");
                    }
                }
            }
            "#,
        ),
    ]);

    let program = neoflash_script::compile_project(&proj).expect("AIR AS3 project must compile");
    let frame_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: "import app.AirController;\ntrace(AirController.verifyTypes());".into(),
    };
    let compiled_frame = neoflash_script::compile_with_program(&frame_script, &program)
        .expect("AIR AS3 timeline script must compile");
    assert!(!compiled_frame.is_empty());

    // Execute end-to-end through Node + createRuntimeHost + real WASM WebPlayer & AirKernel.
    let mut exec_project = proj;
    exec_project.resources.push(
        ResourceUnit::from_bytes(
            "bundled-cfg",
            "config/app.txt",
            "text/plain",
            b"readonly-ok",
        )
        .unwrap(),
    );
    let project_json = serde_json::to_string(&exec_project).unwrap();
    let bundle_json = serde_json::json!({
        "project": exec_project,
        "program": {
            "code": program.code,
            "class_names": program.class_names,
        }
    })
    .to_string();
    let script_json = serde_json::to_string(&frame_script).unwrap();
    let code_json = serde_json::to_string(&compiled_frame).unwrap();

    let node_runner = format!(
        r#"
        import assert from 'node:assert/strict';
        import {{ resolve }} from 'node:path';
        import {{ runtimeWasm }} from './tests/rust-runtime-host.mjs';
        import {{ createRuntimeHost }} from './tests/host-integration.mjs';

        const root = resolve('../..');
        const projectJson = {project_json};
        const bundle = {bundle_json};
        const script = {script_json};
        const code = {code_json};

        const player = new runtimeWasm.WebPlayer(JSON.stringify(projectJson));
        const {{ host, traceCalls }} = createRuntimeHost(
            root,
            player,
            [{{ script, code, location: 'Scene 1 / frame 1' }}],
            {{ bundle, project: projectJson }}
        );

        // Execute our compiled frame script against the root MovieClip and verify Stage AIR extensions too.
        const rootClip = host.proxyFor([]);
        assert.equal(rootClip.stage.supportsOrientationChange, true);
        assert.ok(rootClip.stage.nativeWindow);
        assert.equal(rootClip.stage.supportedOrientations.length, 4);

        // Enqueue and dispatch via WebPlayer or direct program check:
        const SummaryClass = host.program().type('app.AirController');
        const out = SummaryClass.verifyTypes();
        assert.equal(
            out,
            'NeoFlash AIR|3|secret-token|1|Ship AIR|1|1.0.0|1|1|false|granted|127.0.0.1|0|0'
        );

        host.disposeFamily();
        player.free();
        "#,
    );

    let output = run_node_module(&node_runner)
        .expect("node must be available to execute compiled AS3 AIR program");
    assert!(
        output.status.success(),
        "Node AS3 AIR execution failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn as3_air_timeline_auto_imports_and_interface_diagnostics_work() {
    // Timeline scripts auto-import flash.* AIR classes without explicit import statements.
    let blank = air_project(&[]);
    let program = neoflash_script::compile_project(&blank).unwrap();
    let timeline_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: r#"
            var f:File = File.applicationDirectory.resolvePath("readme.txt");
            var s:FileStream = new FileStream();
            var c:SQLConnection = new SQLConnection();
            var st:SQLStatement = new SQLStatement();
            var inv:InvokeEvent = new InvokeEvent(InvokeEvent.INVOKE);
            var menu:NativeMenu = new NativeMenu();
            trace(f.url, FileMode.READ, SQLMode.CREATE, PermissionStatus.GRANTED, inv.type, menu.numItems);
        "#
        .into(),
    };
    assert!(
        neoflash_script::compile_with_program(&timeline_script, &program).is_ok(),
        "Timeline script must auto-import flash.* AIR classes unambiguously"
    );

    // Incomplete IFilePromise implementation must fail compile-time interface contract checking.
    let bad_promise = air_project(&[(
        "bad/BrokenPromise.as",
        r#"
        package bad {
            import flash.desktop.IFilePromise;
            public class BrokenPromise implements IFilePromise {
                public function get relativePath():String { return "a.txt"; }
            }
        }
        "#,
    )]);
    #[allow(clippy::err_expect)]
    let err = neoflash_script::compile_project(&bad_promise)
        .err()
        .expect("Missing IFilePromise methods must fail compilation");
    assert!(
        err.iter()
            .any(|d| d.message.contains("open") || d.message.contains("IFilePromise")),
        "Expected actionable IFilePromise diagnostic, got: {err:?}"
    );

    // Extending final/constant/native AIR facades must produce actionable compile-time diagnostics.
    for (cls_name, base_qname) in [
        ("BadFile", "flash.filesystem.File"),
        ("BadMode", "flash.filesystem.FileMode"),
        ("BadWin", "flash.display.NativeWindow"),
        ("BadConn", "flash.data.SQLConnection"),
        ("BadEls", "flash.data.EncryptedLocalStore"),
        ("BadSqlEv", "flash.events.SQLEvent"),
        ("BadPerm", "flash.permissions.PermissionStatus"),
        ("BadDns", "flash.net.dns.DNSResolver"),
        ("BadGeo", "flash.sensors.Geolocation"),
    ] {
        let p = air_project(&[(
            "bad/Sub.as",
            &format!("package bad {{ public class {cls_name} extends {base_qname} {{}} }}"),
        )]);
        assert!(
            neoflash_script::compile_project(&p).is_err(),
            "Extending {base_qname} should be rejected with an actionable diagnostic"
        );
    }

    // Incomplete IURIDereferencer implementation must fail compile-time interface contract checking.
    let bad_deref = air_project(&[(
        "bad/BrokenDeref.as",
        r#"
        package bad {
            import flash.security.IURIDereferencer;
            public class BrokenDeref implements IURIDereferencer {
            }
        }
        "#,
    )]);
    #[allow(clippy::err_expect)]
    let deref_err = neoflash_script::compile_project(&bad_deref)
        .err()
        .expect("Missing IURIDereferencer.dereference must fail compilation");
    assert!(
        deref_err
            .iter()
            .any(|d| d.message.contains("dereference") || d.message.contains("IURIDereferencer")),
        "Expected actionable IURIDereferencer diagnostic, got: {deref_err:?}"
    );
}

#[test]
fn as3_air_remaining_surface_compiles_and_executes_end_to_end() {
    let proj = air_project(&[
        (
            "app/CustomHtmlHost.as",
            r#"
            package app {
                import flash.html.HTMLHost;
                import flash.html.HTMLLoader;
                import flash.html.HTMLWindowCreateOptions;
                import flash.geom.Rectangle;

                public class CustomHtmlHost extends HTMLHost {
                    public var lastTitle:String = "";
                    public var lastLocation:String = "";
                    public var lastStatus:String = "";
                    public var focusedCount:int = 0;

                    public function CustomHtmlHost() {
                        super(true);
                    }

                    override public function updateTitle(title:String):void {
                        this.lastTitle = title;
                    }

                    override public function updateLocation(locationURL:String):void {
                        this.lastLocation = locationURL;
                    }

                    override public function updateStatus(status:String):void {
                        this.lastStatus = status;
                    }

                    override public function windowFocus():void {
                        this.focusedCount++;
                    }

                    override public function createWindow(windowCreateOptions:HTMLWindowCreateOptions):HTMLLoader {
                        return HTMLLoader.createRootWindow(true, windowCreateOptions, true, new Rectangle(0, 0, 300, 200));
                    }
                }
            }
            "#,
        ),
        (
            "app/MemDereferencer.as",
            r#"
            package app {
                import flash.security.IURIDereferencer;
                import flash.utils.ByteArray;
                import flash.utils.IDataInput;

                public class MemDereferencer implements IURIDereferencer {
                    private var _payload:String;

                    public function MemDereferencer(payload:String) {
                        this._payload = payload;
                    }

                    public function dereference(uri:String):IDataInput {
                        var ba:ByteArray = new ByteArray();
                        ba.writeUTFBytes(this._payload);
                        ba.position = 0;
                        return ba;
                    }
                }
            }
            "#,
        ),
        (
            "app/AirRemainingVerifier.as",
            r#"
            package app {
                import flash.data.*;
                import flash.desktop.*;
                import flash.display.*;
                import flash.display3D.*;
                import flash.display3D.textures.*;
                import flash.events.*;
                import flash.external.*;
                import flash.filesystem.*;
                import flash.geom.*;
                import flash.html.*;
                import flash.media.*;
                import flash.net.*;
                import flash.net.drm.*;
                import flash.printing.*;
                import flash.security.*;
                import flash.system.*;
                import flash.text.*;
                import flash.ui.*;
                import flash.utils.ByteArray;

                public class AirRemainingVerifier {
                    public static function verifyAll(rootClip:MovieClip):String {
                        // 1. DisplayObject.cacheAsBitmapMatrix, InteractiveObject.softKeyboard, Loader.loadFilePromise
                        rootClip.cacheAsBitmapMatrix = new Matrix(1, 0, 0, 1, 5, 10);
                        rootClip.softKeyboard = SoftKeyboardType.EMAIL;

                        // 2. Encrypted SQLConnection (open, reencrypt, attach)
                        var key1:ByteArray = new ByteArray();
                        key1.writeUTFBytes("0123456789abcdef");
                        var key2:ByteArray = new ByteArray();
                        key2.writeUTFBytes("fedcba9876543210");
                        var encDb:File = File.applicationStorageDirectory.resolvePath("enc_as3.db");
                        var sql:SQLConnection = new SQLConnection();
                        sql.open(encDb, SQLMode.CREATE, false, 1024, key1);
                        var st:SQLStatement = new SQLStatement();
                        st.sqlConnection = sql;
                        st.text = "CREATE TABLE items (name TEXT)";
                        st.execute();
                        st.text = "INSERT INTO items (name) VALUES ('encrypted-ok')";
                        st.execute();
                        sql.reencrypt(key2);
                        sql.close();

                        var sql2:SQLConnection = new SQLConnection();
                        sql2.open(null);
                        sql2.attach("vault", encDb, null, key2);
                        st.sqlConnection = sql2;
                        st.text = "SELECT name FROM vault.items";
                        st.execute();
                        var encVal:String = st.getResult().data[0].name;
                        sql2.close();

                        // 3. HTMLLoader + CustomHtmlHost + DisplayObject container integration
                        var host:CustomHtmlHost = new CustomHtmlHost();
                        var html:HTMLLoader = new HTMLLoader();
                        html.htmlHost = host;
                        html.width = 400;
                        html.height = 300;
                        rootClip.addChild(html);
                        html.loadString("<html><head><title>AS3 HTML</title></head><body>Content</body></html>");
                        html.window.focus();
                        var hist0:HTMLHistoryItem = html.getHistoryAt(0);
                        rootClip.removeChild(html);

                        // 4. CameraRoll, CameraUI, MediaPromise, Loader.loadFilePromise, StageWebView
                        var roll:CameraRoll = new CameraRoll();
                        var bmd:BitmapData = new BitmapData(16, 16, true, 0xff112233);
                        roll.addBitmapData(bmd);
                        var camUi:CameraUI = new CameraUI();
                        var capturedPromise:MediaPromise = null;
                        camUi.addEventListener(MediaEvent.COMPLETE, function(ev:MediaEvent):void {
                            capturedPromise = ev.data;
                        });
                        camUi.launch(MediaType.IMAGE);
                        var loader:Loader = new Loader();
                        loader.loadFilePromise(capturedPromise);

                        var swv:StageWebView = new StageWebView(true);
                        swv.viewPort = new Rectangle(0, 0, 64, 24);
                        swv.loadString("<html><head><title>SWV</title></head><body>Hello</body></html>");
                        var swvBmd:BitmapData = new BitmapData(64, 24, true, 0x00000000);
                        swv.drawViewPortToBitmapData(swvBmd);
                        var swvPixel:uint = swvBmd.getPixel32(0, 0);
                        swv.dispose();

                        // 5. NetworkInfo, DatagramSocket, ServerSocket, Socket, URLRequestDefaults, URLStream
                        var ifaces:Vector.<NetworkInterface> = NetworkInfo.networkInfo.findInterfaces();
                        var udpRecv:DatagramSocket = new DatagramSocket();
                        var udpSend:DatagramSocket = new DatagramSocket();
                        udpRecv.bind(45001, "127.0.0.1");
                        udpRecv.receive();
                        var udpGot:String = "";
                        udpRecv.addEventListener(DatagramSocketDataEvent.DATA, function(ev:DatagramSocketDataEvent):void {
                            udpGot = ev.data.readUTFBytes(ev.data.bytesAvailable);
                        });
                        var udpBytes:ByteArray = new ByteArray();
                        udpBytes.writeUTFBytes("as3-udp");
                        udpSend.send(udpBytes, 0, 0, "127.0.0.1", 45001);
                        udpRecv.close();
                        udpSend.close();

                        var srv:ServerSocket = new ServerSocket();
                        srv.bind(45002, "127.0.0.1");
                        srv.listen(2);
                        var accepted:Socket = null;
                        srv.addEventListener(ServerSocketConnectEvent.CONNECT, function(ev:ServerSocketConnectEvent):void {
                            accepted = ev.socket;
                        });
                        var cli:Socket = new Socket();
                        cli.connect("127.0.0.1", 45002);
                        cli.writeUTFBytes("as3-tcp");
                        cli.flush();
                        var tcpGot:String = accepted.readUTFBytes(accepted.bytesAvailable);
                        cli.close();
                        accepted.close();
                        srv.close();

                        URLRequestDefaults.userAgent = "AS3Verifier/1.0";
                        var req:URLRequest = new URLRequest(capturedPromise.file.url);
                        req.method = URLRequestMethod.GET;
                        var ustream:URLStream = new URLStream();
                        ustream.load(req);
                        var streamLen:uint = ustream.bytesAvailable;
                        ustream.close();

                        // 6. PrintJob & PaperSize
                        var pj:PrintJob = new PrintJob();
                        pj.selectPaperSize(PaperSize.A4);
                        var uiOpts:PrintUIOptions = new PrintUIOptions();
                        var jobOpts:PrintJobOptions = new PrintJobOptions(false);
                        jobOpts.printMethod = PrintMethod.VECTOR;
                        pj.start2(uiOpts, false);
                        pj.addPage(rootClip, null, jobOpts, 1);
                        pj.send();

                        // 7. XMLSignatureValidator + IURIDereferencer
                        var sigVal:XMLSignatureValidator = new XMLSignatureValidator();
                        var certBa:ByteArray = new ByteArray();
                        certBa.writeUTFBytes("CN=AS3 Signer,O=NeoFlash");
                        sigVal.addCertificate(certBa, true);
                        sigVal.uriDereferencer = new MemDereferencer("Hello World");
                        var sigXml:XML = new XML(
                            "<Signature xmlns='http://www.w3.org/2000/09/xmldsig#'>" +
                            "<SignedInfo>" +
                            "<CanonicalizationMethod Algorithm='http://www.w3.org/TR/2001/REC-xml-c14n-20010315'/>" +
                            "<SignatureMethod Algorithm='http://www.w3.org/2001/04/xmldsig-more#hmac-sha256'/>" +
                            "<Reference URI='urn:doc1'>" +
                            "<DigestMethod Algorithm='http://www.w3.org/2001/04/xmlenc#sha256'/>" +
                            "<DigestValue>a591a6d40bf420404a011733cfb7b190d62c65bf0bcda32b57b277d9ad9f146e</DigestValue>" +
                            "</Reference>" +
                            "</SignedInfo>" +
                            "<SignatureValue>wbXPdq0Uaj7yWQPakjzSlQf2l5yIfh5gM4Z5PPEuHBI=</SignatureValue>" +
                            "</Signature>"
                        );
                        sigVal.verify(sigXml);

                        // 8. StageText, ExtensionContext, ContextMenu, GameInput, Context3D, System
                        var stOpts:StageTextInitOptions = new StageTextInitOptions(false);
                        var stText:StageText = new StageText(stOpts);
                        stText.viewPort = new Rectangle(0, 0, 80, 20);
                        stText.restrict = "A-Z";
                        stText.text = "AbC123";
                        var stBmd:BitmapData = new BitmapData(80, 20, true, 0);
                        stText.drawViewPortToBitmapData(stBmd);
                        var filteredText:String = stText.text;
                        stText.dispose();

                        var ext:ExtensionContext = ExtensionContext.createExtensionContext("com.neoflash.as3", "main");
                        var pong:Object = ext.call("echo", "ok");
                        ext.dispose();

                        var cm:ContextMenu = new ContextMenu();
                        var cmi:ContextMenuItem = new ContextMenuItem("Run");
                        cm.customItems = [cmi];

                        var dev:GameInputDevice = GameInput.getDeviceAt(0);
                        var ctx3d:Context3D = new Context3D();
                        var tex:Texture = ctx3d.createTexture(32, 32, "bgra", false);
                        var rtex:RectangleTexture = ctx3d.createRectangleTexture(64, 32, "bgra", false);
                        tex.dispose();
                        rtex.dispose();

                        var lc:LoaderContext = new LoaderContext();
                        lc.imageDecodingPolicy = ImageDecodingPolicy.ON_LOAD;

                        return [
                            rootClip.softKeyboard,
                            encVal,
                            host.lastTitle,
                            hist0.title,
                            swvPixel != 0 ? "1" : "0",
                            ifaces.length > 0 ? "1" : "0",
                            udpGot,
                            tcpGot,
                            streamLen > 0 ? "1" : "0",
                            pj.paperWidth,
                            sigVal.validityStatus,
                            sigVal.signerCN,
                            filteredText,
                            String(pong),
                            dev.numControls > 0 ? "1" : "0",
                            Security.APPLICATION,
                            Capabilities.languages.length > 0 ? "1" : "0",
                            lc.imageDecodingPolicy
                        ].join("|");
                    }
                }
            }
            "#,
        ),
    ]);

    let program = neoflash_script::compile_project(&proj)
        .expect("All remaining AS3 AIR classes and interfaces must compile cleanly");
    let frame_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: "import app.AirRemainingVerifier;\ntrace('ready');".into(),
    };
    let compiled_frame = neoflash_script::compile_with_program(&frame_script, &program)
        .expect("Timeline frame script must compile");

    let project_json = serde_json::to_string(&proj).unwrap();
    let bundle_json = serde_json::json!({
        "project": proj,
        "program": {
            "code": program.code,
            "class_names": program.class_names,
        }
    })
    .to_string();
    let script_json = serde_json::to_string(&frame_script).unwrap();
    let code_json = serde_json::to_string(&compiled_frame).unwrap();

    let node_runner = format!(
        r#"
        import assert from 'node:assert/strict';
        import {{ resolve }} from 'node:path';
        import {{ runtimeWasm }} from './tests/rust-runtime-host.mjs';
        import {{ createRuntimeHost }} from './tests/host-integration.mjs';

        const root = resolve('../..');
        const projectJson = {project_json};
        const bundle = {bundle_json};
        const script = {script_json};
        const code = {code_json};

        const player = new runtimeWasm.WebPlayer(JSON.stringify(projectJson));
        const {{ host }} = createRuntimeHost(
            root,
            player,
            [{{ script, code, location: 'Scene 1 / frame 1' }}],
            {{ bundle, project: projectJson }}
        );

        const rootClip = host.proxyFor([]);
        const Verifier = host.program().type('app.AirRemainingVerifier');
        const summary = Verifier.verifyAll(rootClip);
        assert.equal(
            summary,
            'email|encrypted-ok|AS3 HTML|AS3 HTML|1|1|as3-udp|as3-tcp|1|595|valid|AS3 Signer|AC|ok|1|application|1|onLoad'
        );

        host.disposeFamily();
        player.free();
        "#,
    );

    let output = run_node_module(&node_runner)
        .expect("node must be available to execute compiled AS3 AIR program");
    assert!(
        output.status.success(),
        "Node AS3 AIR remaining surface execution failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
