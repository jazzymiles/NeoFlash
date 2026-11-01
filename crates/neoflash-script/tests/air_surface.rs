use neoflash_core::{Project, ResourceUnit, Script, ScriptLanguage, SourceUnit};
use std::process::Command;

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

    let output = Command::new("node")
        .args(["--input-type=module", "-e", &node_runner])
        .output()
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
}
