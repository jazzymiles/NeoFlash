use neoflash_core::{Project, ResourceUnit, Script, ScriptLanguage, SourceUnit};
use std::process::Command;

fn net_sys_project(sources: &[(&str, &str)]) -> Project {
    let mut project = Project::blank();
    project.name = "AS3 Area 8 Net Sys Glob Suite".into();
    project.width = 1024;
    project.height = 768;
    project.format_version = 3;
    project.sources = sources
        .iter()
        .enumerate()
        .map(|(i, (path, source))| SourceUnit {
            id: format!("net-sys-src-{i}"),
            path: (*path).into(),
            language: ScriptLanguage::ActionScript3,
            source: (*source).into(),
        })
        .collect();
    project
}

#[test]
fn as3_area8_packages_interfaces_vectors_and_package_functions_compile_and_execute() {
    let proj = net_sys_project(&[(
        "app/NetSysAudit.as",
        r#"
        package app {
            import flash.concurrent.Condition;
            import flash.concurrent.Mutex;
            import flash.crypto.generateRandomBytes;
            import flash.desktop.Clipboard;
            import flash.desktop.ClipboardFormats;
            import flash.desktop.ClipboardTransferMode;
            import flash.external.ExternalInterface;
            import flash.globalization.Collator;
            import flash.globalization.CollatorMode;
            import flash.globalization.CurrencyFormatter;
            import flash.globalization.CurrencyParseResult;
            import flash.globalization.DateTimeFormatter;
            import flash.globalization.DateTimeNameContext;
            import flash.globalization.DateTimeNameStyle;
            import flash.globalization.DateTimeStyle;
            import flash.globalization.LastOperationStatus;
            import flash.globalization.LocaleID;
            import flash.globalization.NationalDigitsType;
            import flash.globalization.NumberFormatter;
            import flash.globalization.NumberParseResult;
            import flash.globalization.StringTools;
            import flash.media.scanHardware;
            import flash.net.FileFilter;
            import flash.net.FileReference;
            import flash.net.FileReferenceList;
            import flash.net.GroupSpecifier;
            import flash.net.LocalConnection;
            import flash.net.NetConnection;
            import flash.net.NetGroup;
            import flash.net.NetGroupInfo;
            import flash.net.NetGroupReceiveMode;
            import flash.net.NetGroupReplicationStrategy;
            import flash.net.NetGroupSendMode;
            import flash.net.NetGroupSendResult;
            import flash.net.NetMonitor;
            import flash.net.NetStream;
            import flash.net.NetStreamAppendBytesAction;
            import flash.net.NetStreamInfo;
            import flash.net.NetStreamMulticastInfo;
            import flash.net.NetStreamPlayOptions;
            import flash.net.NetStreamPlayTransitions;
            import flash.net.ObjectEncoding;
            import flash.net.Responder;
            import flash.net.SecureSocket;
            import flash.net.SharedObject;
            import flash.net.SharedObjectFlushStatus;
            import flash.net.Socket;
            import flash.net.URLLoader;
            import flash.net.URLLoaderDataFormat;
            import flash.net.URLRequest;
            import flash.net.URLRequestHeader;
            import flash.net.URLRequestMethod;
            import flash.net.URLStream;
            import flash.net.URLVariables;
            import flash.net.XMLSocket;
            import flash.net.drm.AuthenticationMethod;
            import flash.net.drm.DRMContentData;
            import flash.net.drm.DRMDeviceGroup;
            import flash.net.drm.DRMManager;
            import flash.net.drm.DRMPlaybackTimeWindow;
            import flash.net.drm.DRMVoucher;
            import flash.net.drm.LoadVoucherSetting;
            import flash.net.drm.VoucherAccessInfo;
            import flash.net.getClassByAlias;
            import flash.net.navigateToURL;
            import flash.net.registerClassAlias;
            import flash.net.sendToURL;
            import flash.printing.PrintJob;
            import flash.printing.PrintJobOptions;
            import flash.printing.PrintJobOrientation;
            import flash.profiler.Telemetry;
            import flash.profiler.showRedrawRegions;
            import flash.sampler.DeleteObjectSample;
            import flash.sampler.NewObjectSample;
            import flash.sampler.Sample;
            import flash.sampler.StackFrame;
            import flash.sampler.clearSamples;
            import flash.sampler.getGetterInvocationCount;
            import flash.sampler.getInvocationCount;
            import flash.sampler.getMemberNames;
            import flash.sampler.getSampleCount;
            import flash.sampler.getSamples;
            import flash.sampler.getSetterInvocationCount;
            import flash.sampler.getSize;
            import flash.sampler.isGetterSetter;
            import flash.sampler.pauseSampling;
            import flash.sampler.startSampling;
            import flash.sampler.stopSampling;
            import flash.security.CertificateStatus;
            import flash.security.X500DistinguishedName;
            import flash.security.X509Certificate;
            import flash.sensors.Accelerometer;
            import flash.system.ApplicationDomain;
            import flash.system.Capabilities;
            import flash.system.IME;
            import flash.system.IMEConversionMode;
            import flash.system.JPEGLoaderContext;
            import flash.system.LoaderContext;
            import flash.system.MessageChannel;
            import flash.system.MessageChannelState;
            import flash.system.Security;
            import flash.system.SecurityDomain;
            import flash.system.SecurityPanel;
            import flash.system.System;
            import flash.system.SystemUpdater;
            import flash.system.SystemUpdaterType;
            import flash.system.TouchscreenType;
            import flash.system.Worker;
            import flash.system.WorkerDomain;
            import flash.system.WorkerState;
            import flash.system.fscommand;
            import flash.utils.ByteArray;
            import flash.utils.CompressionAlgorithm;
            import flash.utils.Dictionary;
            import flash.utils.Endian;
            import flash.utils.IDataInput;
            import flash.utils.IDataOutput;
            import flash.utils.Timer;

            public class NetSysAudit {
                public static function run():String {
                    var mtx:Mutex = new Mutex();
                    mtx.lock();
                    var cond:Condition = new Condition(mtx);
                    cond.notify();
                    var waitTimedOut:Boolean = cond.wait(5);
                    cond.notifyAll();
                    mtx.unlock();

                    var rnd:ByteArray = generateRandomBytes(16);
                    rnd.shareable = true;

                    System.setClipboard("Area8Clipboard");
                    var clipText:Object = Clipboard.generalClipboard.getData(ClipboardFormats.TEXT_FORMAT, ClipboardTransferMode.CLONE_PREFERRED);

                    var loc:LocaleID = new LocaleID("tr-TR");
                    var want:Vector.<String> = new Vector.<String>();
                    want.push("fr-CA");
                    var have:Vector.<String> = Collator.getAvailableLocaleIDNames();
                    var pref:Vector.<String> = LocaleID.determinePreferredLocales(want, have);

                    var cf:CurrencyFormatter = new CurrencyFormatter("en-US");
                    cf.setCurrency("USD", "$");
                    var formattedCur:String = cf.format(1234.5, true);
                    var cpr:CurrencyParseResult = cf.parse("$1,234.50");

                    var col:Collator = new Collator("en-US", CollatorMode.MATCHING);
                    var colEq:Boolean = col.equals("Resume", "resume");

                    var st:StringTools = new StringTools("tr-TR");
                    var trUpper:String = st.toUpperCase("i");

                    var nfLocales:Vector.<String> = NumberFormatter.getAvailableLocaleIDNames();
                    var nf:NumberFormatter = new NumberFormatter("en-US");
                    nf.digitsType = NationalDigitsType.OSMANYA;
                    var osmanyaNine:String = nf.formatInt(9);

                    scanHardware();
                    showRedrawRegions(true, 0xFF0000);
                    fscommand("audit", "ready");

                    var sock:Socket = new Socket();
                    sock.connect("127.0.0.1", 9000);
                    var sockOut:IDataOutput = sock as IDataOutput;
                    sockOut.writeUTF("loopback-ok");
                    sock.flush();
                    var sockIn:IDataInput = sock as IDataInput;
                    var echoed:String = sockIn.readUTF();
                    sock.close();

                    var tls:SecureSocket = new SecureSocket();
                    tls.connect("example.test", 443);
                    var cert:X509Certificate = tls.serverCertificate;
                    var certSubject:X500DistinguishedName = cert.subject;
                    var isSockSub:Boolean = (tls is Socket) && (tls is IDataInput) && (tls is IDataOutput);
                    tls.close();

                    var us:URLStream = new URLStream();
                    var isUsInput:Boolean = (us is IDataInput);

                    var nc:NetConnection = new NetConnection();
                    nc.connect("rtmfp://p2p.example.test/app");
                    var gs:GroupSpecifier = new GroupSpecifier("wave5-room");
                    gs.postingEnabled = true;
                    gs.routingEnabled = true;
                    gs.objectReplicationEnabled = true;
                    var ng:NetGroup = new NetGroup(nc, gs.groupspecWithAuthorizations());
                    var ngInfo:NetGroupInfo = ng.info;

                    var nm:NetMonitor = new NetMonitor();
                    var ns:NetStream = new NetStream(nc);
                    var streams:Vector.<NetStream> = nm.listStreams();
                    var nsInfo:NetStreamInfo = ns.info;
                    var nsMulti:NetStreamMulticastInfo = ns.multicastInfo;
                    var playOpts:NetStreamPlayOptions = new NetStreamPlayOptions();
                    playOpts.transition = NetStreamPlayTransitions.SWITCH;

                    var fr:FileReference = new FileReference();
                    var frList:FileReferenceList = new FileReferenceList();
                    var ff:FileFilter = new FileFilter("Images", "*.png;*.jpg");

                    var drmMgr:DRMManager = DRMManager.getDRMManager();
                    var pj:PrintJob = new PrintJob();
                    var pjOpts:PrintJobOptions = new PrintJobOptions(true);

                    startSampling();
                    var sampleCount:Number = getSampleCount();
                    pauseSampling();
                    stopSampling();
                    clearSamples();

                    var jlc:JPEGLoaderContext = new JPEGLoaderContext(0.75, true);
                    var isLcSub:Boolean = (jlc is LoaderContext);
                    var w:Worker = WorkerDomain.current.createWorker(rnd, false);
                    var ch:MessageChannel = Worker.current.createMessageChannel(w);
                    ch.send("ping-worker");
                    var msgBack:String = String(ch.receive(false));
                    var runningWorkers:Vector.<Worker> = WorkerDomain.current.listWorkers();

                    return [
                        !waitTimedOut ? "wait0" : "wait1",
                        rnd.length,
                        rnd.shareable ? "share1" : "share0",
                        String(clipText),
                        loc.getLanguage(),
                        pref.length > 0 ? pref[0] : "none",
                        formattedCur,
                        cpr.value,
                        colEq ? "col1" : "col0",
                        trUpper,
                        nfLocales.length > 5 ? "loc1" : "loc0",
                        osmanyaNine.length > 0 ? "osm1" : "osm0",
                        echoed,
                        tls.serverCertificateStatus,
                        certSubject.commonName,
                        isSockSub ? "sub1" : "sub0",
                        isUsInput ? "us1" : "us0",
                        ng.receiveMode,
                        streams.length,
                        playOpts.transition,
                        ff.description,
                        DRMManager.isSupported ? "drm1" : "drm0",
                        PrintJob.isSupported ? "pj1" : "pj0",
                        pjOpts.printAsBitmap ? "bmp1" : "bmp0",
                        sampleCount >= 1 ? "smp1" : "smp0",
                        isLcSub ? "lc1" : "lc0",
                        msgBack,
                        runningWorkers.length,
                        Capabilities.language,
                        Security.sandboxType
                    ].join("|");
                }
            }
        }
        "#,
    )]);

    let program =
        neoflash_script::compile_project(&proj).expect("AS3 Area 8 project must compile cleanly");
    let frame_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: "import app.NetSysAudit;\ntrace(NetSysAudit.run());".into(),
    };
    let compiled_frame = neoflash_script::compile_with_program(&frame_script, &program)
        .expect("AS3 Area 8 timeline script must compile cleanly");
    assert!(!compiled_frame.is_empty());

    let mut exec_project = proj;
    exec_project.resources.push(
        ResourceUnit::from_bytes("sample-txt", "data/hello.txt", "text/plain", b"hello-area8")
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
        const {{ host }} = createRuntimeHost(
            root,
            player,
            [{{ script, code, location: 'Scene 1 / frame 1' }}],
            {{ bundle, project: projectJson }}
        );

        const AuditClass = host.program().type('app.NetSysAudit');
        const out = AuditClass.run();
        assert.equal(
            out,
            'wait1|16|share1|Area8Clipboard|tr|fr-FR|$1,234.50|1234.5|col1|\u0130|loc1|osm1|loopback-ok|trusted|example.test|sub1|us1|exact|1|switch|Images|drm1|pj1|bmp1|smp1|lc1|ping-worker|1|en|localTrusted'
        );

        host.disposeFamily();
        player.free();
        "#,
    );

    let output = Command::new("node")
        .args(["--input-type=module", "-e", &node_runner])
        .output()
        .expect("node must be available to execute compiled AS3 Area 8 program");
    assert!(
        output.status.success(),
        "Node AS3 Area 8 execution failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn as3_area8_timeline_auto_imports_and_subclass_diagnostics_work() {
    let blank = net_sys_project(&[]);
    let program = neoflash_script::compile_project(&blank).unwrap();
    let timeline_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: r#"
            var m:Mutex = new Mutex();
            var c:Condition = new Condition(m);
            var l:LocaleID = new LocaleID(LocaleID.DEFAULT);
            var cf:CurrencyFormatter = new CurrencyFormatter("en-US");
            var col:Collator = new Collator("en-US", CollatorMode.SORTING);
            var st:StringTools = new StringTools("en-US");
            var gs:GroupSpecifier = new GroupSpecifier("g1");
            var sock:Socket = new Socket();
            var ss:SecureSocket = new SecureSocket();
            var us:URLStream = new URLStream();
            var xs:XMLSocket = new XMLSocket();
            var lc:LocalConnection = new LocalConnection();
            var fr:FileReference = new FileReference();
            var frl:FileReferenceList = new FileReferenceList();
            var pj:PrintJob = new PrintJob();
            var acc:Accelerometer = new Accelerometer();
            var ctx:JPEGLoaderContext = new JPEGLoaderContext(0.5);
            var su:SystemUpdater = new SystemUpdater();
            trace(
                LastOperationStatus.NO_ERROR,
                NationalDigitsType.OSMANYA,
                NetStreamAppendBytesAction.RESET_BEGIN,
                NetStreamPlayTransitions.SWITCH,
                CertificateStatus.TRUSTED,
                IMEConversionMode.JAPANESE_HIRAGANA,
                MessageChannelState.OPEN,
                SecurityPanel.PRIVACY,
                SystemUpdaterType.SYSTEM,
                TouchscreenType.FINGER,
                WorkerState.RUNNING,
                Telemetry.connected,
                Capabilities.version,
                System.totalMemoryNumber,
                m != null, c != null, l.name, cf.currencyISOCode, col.actualLocaleIDName,
                st.actualLocaleIDName, gs.groupspecWithoutAuthorizations(),
                sock.connected, ss.serverCertificateStatus, us.connected, xs.connected,
                lc.domain, fr != null, frl != null, pj.orientation, acc.muted,
                ctx.deblockingFilter, su != null
            );
        "#
        .into(),
    };
    assert!(
        neoflash_script::compile_with_program(&timeline_script, &program).is_ok(),
        "Timeline script must auto-import flash.* Area 8 classes unambiguously"
    );

    for (cls_name, base_qname) in [
        ("BadMutex", "flash.concurrent.Mutex"),
        ("BadCond", "flash.concurrent.Condition"),
        ("BadLocale", "flash.globalization.LocaleID"),
        ("BadCurr", "flash.globalization.CurrencyFormatter"),
        ("BadSock", "flash.net.Socket"),
        ("BadSecSock", "flash.net.SecureSocket"),
        ("BadGroup", "flash.net.NetGroup"),
        ("BadDrm", "flash.net.drm.DRMManager"),
        ("BadPrint", "flash.printing.PrintJob"),
        ("BadTelemetry", "flash.profiler.Telemetry"),
        ("BadSample", "flash.sampler.Sample"),
        ("BadCert", "flash.security.X509Certificate"),
        ("BadAcc", "flash.sensors.Accelerometer"),
        ("BadSys", "flash.system.System"),
        ("BadWorker", "flash.system.Worker"),
    ] {
        let p = net_sys_project(&[(
            "bad/Sub.as",
            &format!("package bad {{ public class {cls_name} extends {base_qname} {{}} }}"),
        )]);
        assert!(
            neoflash_script::compile_project(&p).is_err(),
            "Extending {base_qname} should be rejected with an actionable diagnostic"
        );
    }
}
