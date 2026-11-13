mod support;

use neoflash_core::{Project, ResourceUnit, Script, ScriptLanguage, SourceUnit};
use support::run_node_from_stdin;

fn media_events_project(sources: &[(&str, &str)]) -> Project {
    let mut project = Project::blank();
    project.name = "AS3 Area 8 Media Events Suite".into();
    project.width = 800;
    project.height = 600;
    project.format_version = 3;
    project.sources = sources
        .iter()
        .enumerate()
        .map(|(i, (path, source))| SourceUnit {
            id: format!("media-ev-src-{i}"),
            path: (*path).into(),
            language: ScriptLanguage::ActionScript3,
            source: (*source).into(),
        })
        .collect();
    project
}

#[test]
fn as3_media_events_ui_and_accessibility_compile_and_execute_end_to_end() {
    let proj = media_events_project(&[
        (
            "app/CustomAccImpl.as",
            r#"
            package app {
                import flash.accessibility.AccessibilityImplementation;
                import flash.accessibility.ISearchableText;
                import flash.accessibility.ISimpleTextSelection;
                import flash.geom.Rectangle;

                public class CustomAccImpl extends AccessibilityImplementation implements ISearchableText, ISimpleTextSelection {
                    private var _text:String;

                    public function CustomAccImpl(text:String) {
                        super();
                        this._text = text;
                        this.stub = false;
                        this.errno = 0;
                    }

                    override public function get_accRole(childID:uint):uint {
                        return 0x2a;
                    }

                    override public function get_accState(childID:uint):uint {
                        return 0x00100000;
                    }

                    override public function get_accName(childID:uint):String {
                        return "AccessibleMediaControl";
                    }

                    override public function get_accValue(childID:uint):String {
                        return this._text;
                    }

                    override public function getChildIDArray():Array {
                        return [1, 2];
                    }

                    override public function isLabeledBy(labelBounds:Rectangle):Boolean {
                        return labelBounds.width > 0;
                    }

                    public function get searchText():String {
                        return this._text;
                    }

                    public function get selectionAnchorIndex():int {
                        return 0;
                    }

                    public function get selectionActiveIndex():int {
                        return this._text.length;
                    }
                }
            }
            "#,
        ),
        (
            "app/MediaEventsController.as",
            r#"
            package app {
                import flash.accessibility.*;
                import flash.display.*;
                import flash.events.*;
                import flash.geom.*;
                import flash.media.*;
                import flash.net.*;
                import flash.ui.*;
                import flash.utils.ByteArray;
                import flash.utils.IDataInput;

                public class MediaEventsController {
                    public static function verifyAll(stage:Stage):String {
                        // 1. Accessibility & interfaces
                        var sp:Sprite = new Sprite();
                        var accProps:AccessibilityProperties = new AccessibilityProperties();
                        accProps.name = "Video Toolbar";
                        accProps.description = "Playback controls";
                        accProps.shortcut = "Space";
                        accProps.forceSimple = true;
                        accProps.noAutoLabeling = true;
                        accProps.silent = false;
                        sp.accessibilityProperties = accProps;

                        var accImpl:CustomAccImpl = new CustomAccImpl("NeoFlash Player");
                        sp.accessibilityImplementation = accImpl;
                        var searchable:ISearchableText = accImpl as ISearchableText;
                        var selectable:ISimpleTextSelection = accImpl as ISimpleTextSelection;
                        Accessibility.updateProperties();

                        // 2. Camera, Video, BitmapData, Vector.<uint>, ByteArray
                        var cam:Camera = Camera.getCamera("0");
                        cam.setMode(32, 24, 30, true);
                        cam.setQuality(65536, 90);
                        cam.setKeyFrameInterval(15);
                        cam.setLoopback(true);
                        cam.setMotionLevel(40, 1200);

                        var subRect:Rectangle = new Rectangle(0, 0, 4, 4);
                        var camBytes:ByteArray = new ByteArray();
                        cam.copyToByteArray(subRect, camBytes);
                        var camVec:Vector.<uint> = new Vector.<uint>();
                        cam.copyToVector(subRect, camVec);
                        var bmd:BitmapData = new BitmapData(32, 24, true, 0);
                        cam.drawToBitmapData(bmd);

                        var vid:Video = new Video(160, 120);
                        vid.attachCamera(cam);
                        var attachedW:int = vid.videoWidth;
                        vid.attachCamera(null);

                        // 3. Microphone & Enhanced Microphone
                        var mic:Microphone = Microphone.getMicrophone(0);
                        mic.rate = 22;
                        mic.codec = SoundCodec.SPEEX;
                        mic.setLoopBack(false);
                        mic.setSilenceLevel(20, 1500);
                        mic.setUseEchoSuppression(true);

                        var emic:Microphone = Microphone.getEnhancedMicrophone(0);
                        var eopts:MicrophoneEnhancedOptions = emic.enhancedOptions;
                        eopts.mode = MicrophoneEnhancedMode.HALF_DUPLEX;
                        eopts.echoPath = 256;
                        emic.enhancedOptions = eopts;

                        // 4. VideoStreamSettings, H264, VP6, AudioDeviceManager, SoundMixer, AV*
                        var h264:H264VideoStreamSettings = new H264VideoStreamSettings();
                        h264.setMode(1280, 720, 60);
                        h264.setQuality(0, 95);
                        h264.setKeyFrameInterval(30);
                        h264.setProfileLevel(H264Profile.MAIN, H264Level.LEVEL_4_1);
                        var vp6:VP6VideoStreamSettings = new VP6VideoStreamSettings();
                        var adm:AudioDeviceManager = AudioDeviceManager.audioDeviceManager;
                        var avStream:AVURLStream = new AVURLStream();
                        var streamAsInput:Boolean = avStream is IDataInput;
                        var tag:AVTagData = new AVTagData("EXT-X-KEY", 3.5);

                        // 5. Stage.stageVideos & StageVideo
                        scanHardware();
                        var svAvail:String = "";
                        stage.addEventListener(StageVideoAvailabilityEvent.STAGE_VIDEO_AVAILABILITY, function(e:StageVideoAvailabilityEvent):void {
                            svAvail = e.availability;
                        });
                        var svList:Vector.<StageVideo> = stage.stageVideos;
                        var sv:StageVideo = svList[0];
                        sv.viewPort = new Rectangle(0, 0, 800, 450);
                        sv.pan = new Point(0.25, -0.25);
                        sv.zoom = new Point(1.5, 1.5);
                        sv.depth = 2;
                        var cs:Vector.<String> = sv.colorSpaces;
                        var renderSeen:String = "";
                        sv.addEventListener(StageVideoEvent.RENDER_STATE, function(e:StageVideoEvent):void {
                            renderSeen = e.status + ":" + e.colorSpace;
                        });
                        sv.attachCamera(cam);

                        var micSampleBytes:int = 0;
                        mic.addEventListener(SampleDataEvent.SAMPLE_DATA, function(e:SampleDataEvent):void {
                            micSampleBytes = e.data.length;
                        });
                        mic.setLoopBack(true);

                        // 6. ContextMenu, Multitouch, GameInput
                        var cm:ContextMenu = new ContextMenu();
                        cm.hideBuiltInItems();
                        cm.clipboardMenu = true;
                        cm.clipboardItems.copy = true;
                        var cmi:ContextMenuItem = new ContextMenuItem("About Player", true, true, true);
                        cm.customItems = [cmi];
                        sp.contextMenu = cm;
                        var cmCopy:ContextMenu = cm.clone();

                        Multitouch.inputMode = MultitouchInputMode.TOUCH_POINT;
                        var gestures:Vector.<String> = Multitouch.supportedGestures;
                        var giDev:GameInputDevice = GameInput.getDeviceAt(0);
                        giDev.enabled = true;
                        var giCtrl:GameInputControl = giDev.getControlAt(0);

                        // 7. Events & LoaderInfo.uncaughtErrorEvents
                        var ldr:Loader = new Loader();
                        var uee:UncaughtErrorEvents = ldr.contentLoaderInfo.uncaughtErrorEvents;
                        var caughtMsg:String = "";
                        uee.addEventListener(UncaughtErrorEvent.UNCAUGHT_ERROR, function(e:UncaughtErrorEvent):void {
                            caughtMsg = String(e.error);
                        });
                        uee.dispatchEvent(new UncaughtErrorEvent(UncaughtErrorEvent.UNCAUGHT_ERROR, true, true, "err-ok"));

                        var te:TouchEvent = new TouchEvent(
                            TouchEvent.TOUCH_TAP, true, false, 3, true, 12, 18, 4, 4, 0.8, sp,
                            false, false, false, true, false
                        );
                        var tge:TransformGestureEvent = new TransformGestureEvent(
                            TransformGestureEvent.GESTURE_ZOOM, true, false, GesturePhase.UPDATE,
                            10, 20, 2.0, 2.0, 0, 0, 0
                        );
                        var drmEv:DRMErrorEvent = new DRMErrorEvent(
                            DRMErrorEvent.DRM_ERROR, false, false, "drm-msg", 3300, 9, null, true, false
                        );
                        var vidEv:VideoEvent = new VideoEvent(
                            VideoEvent.RENDER_STATE, false, false, VideoEvent.RENDER_STATUS_ACCELERATED
                        );
                        var shEv:ShaderEvent = new ShaderEvent(ShaderEvent.COMPLETE, false, false, bmd, camBytes);
                        var httpEv:HTTPStatusEvent = new HTTPStatusEvent(HTTPStatusEvent.HTTP_STATUS, false, false, 200, true);

                        bmd.dispose();

                        return [
                            searchable.searchText,
                            selectable.selectionActiveIndex,
                            accImpl.get_accRole(0),
                            camBytes.length,
                            camVec.length,
                            attachedW,
                            vid.videoWidth,
                            emic.enhancedOptions.echoPath,
                            h264.codec,
                            h264.profile,
                            h264.level,
                            vp6.codec,
                            SoundMixer.areSoundsInaccessible(),
                            AudioDecoder.DOLBY_DIGITAL,
                            adm.selectedDeviceIndex,
                            streamAsInput,
                            tag.localTime,
                            svList.length,
                            cs.length > 0,
                            renderSeen,
                            cmCopy.builtInItems.play,
                            cmCopy.customItems.length,
                            Multitouch.inputMode,
                            gestures.length > 0,
                            giCtrl.id,
                            caughtMsg,
                            te.commandKey,
                            tge.scaleX,
                            drmEv.subErrorID,
                            vidEv.status,
                            shEv.byteArray.length,
                            httpEv.redirected,
                            ProgressEvent.SOCKET_DATA,
                            svAvail,
                            micSampleBytes
                        ].join("|");
                    }
                }
            }
            "#,
        ),
    ]);

    let program =
        neoflash_script::compile_project(&proj).expect("AS3 media/events project must compile");
    let frame_script = Script {
        language: ScriptLanguage::ActionScript3,
        source:
            "import app.MediaEventsController;\ntrace(MediaEventsController.verifyAll(this.stage));"
                .into(),
    };
    let compiled_frame = neoflash_script::compile_with_program(&frame_script, &program)
        .expect("AS3 media/events timeline script must compile");
    assert!(!compiled_frame.is_empty());

    let mut exec_project = proj;
    exec_project.resources.push(
        ResourceUnit::from_bytes(
            "hls-playlist",
            "media/playlist.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n",
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
        const {{ host }} = createRuntimeHost(
            root,
            player,
            [{{ script, code, location: 'Scene 1 / frame 1' }}],
            {{ bundle, project: projectJson }}
        );

        const rootClip = host.proxyFor([]);
        const Controller = host.program().type('app.MediaEventsController');
        const summary = Controller.verifyAll(rootClip.stage);
        assert.equal(
            summary,
            'NeoFlash Player|15|42|64|16|32|0|256|H264Avc|main|4.1|VP6|false|DolbyDigital|0|true|3.5|4|true|software:BT.709|false|1|touchPoint|true|AXIS_0|err-ok|true|2|9|accelerated|64|true|socketData|available|2048'
        );

        host.disposeFamily();
        player.free();
        "#,
    );

    let output = run_node_from_stdin(&node_runner, &["--input-type=module"])
        .expect("node must be available to execute compiled AS3 media/events program");
    assert!(
        output.status.success(),
        "Node AS3 media/events execution failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn as3_media_events_timeline_auto_imports_and_compile_diagnostics_work() {
    // 1. Timeline frame scripts auto-import flash.accessibility.*, flash.events.*, flash.media.*, and flash.ui.*
    let blank = media_events_project(&[]);
    let program = neoflash_script::compile_project(&blank).unwrap();
    let timeline_script = Script {
        language: ScriptLanguage::ActionScript3,
        source: r#"
            var cam:Camera = Camera.getCamera();
            var mic:Microphone = Microphone.getMicrophone();
            var h264:H264VideoStreamSettings = new H264VideoStreamSettings();
            var cm:ContextMenu = new ContextMenu();
            var item:ContextMenuItem = new ContextMenuItem("Play");
            var ap:AccessibilityProperties = new AccessibilityProperties();
            var drm:DRMErrorEvent = new DRMErrorEvent(DRMErrorEvent.DRM_ERROR);
            var sve:StageVideoEvent = new StageVideoEvent(StageVideoEvent.RENDER_STATE);
            var ve:VideoEvent = new VideoEvent(VideoEvent.RENDER_STATE);
            var she:ShaderEvent = new ShaderEvent(ShaderEvent.COMPLETE);
            trace(
                Camera.isSupported,
                Microphone.isSupported,
                h264.codec,
                StageVideoAvailability.AVAILABLE,
                cm.builtInItems.play,
                item.caption,
                Multitouch.supportsTouchEvents,
                MultitouchInputMode.GESTURE,
                Accessibility.active,
                ap.silent,
                drm.type,
                sve.type,
                ve.type,
                she.type
            );
        "#
        .into(),
    };
    assert!(
        neoflash_script::compile_with_program(&timeline_script, &program).is_ok(),
        "Timeline script must auto-import flash.accessibility/events/media/ui classes unambiguously"
    );

    // 2. Incomplete ISearchableText implementation must fail compile-time interface contract checking.
    let bad_searchable = media_events_project(&[(
        "bad/BrokenSearch.as",
        r#"
        package bad {
            import flash.accessibility.ISearchableText;
            public class BrokenSearch implements ISearchableText {
                public function BrokenSearch() {}
            }
        }
        "#,
    )]);
    #[allow(clippy::err_expect)]
    let err_search = neoflash_script::compile_project(&bad_searchable)
        .err()
        .expect("Missing ISearchableText.searchText getter must fail compilation");
    assert!(
        err_search
            .iter()
            .any(|d| d.message.contains("searchText") || d.message.contains("ISearchableText")),
        "Expected actionable ISearchableText diagnostic, got: {err_search:?}"
    );

    // 3. Incomplete ISimpleTextSelection implementation must fail compile-time interface contract checking.
    let bad_selection = media_events_project(&[(
        "bad/BrokenSelection.as",
        r#"
        package bad {
            import flash.accessibility.ISimpleTextSelection;
            public class BrokenSelection implements ISimpleTextSelection {
                public function get selectionActiveIndex():int { return 0; }
            }
        }
        "#,
    )]);
    #[allow(clippy::err_expect)]
    let err_sel = neoflash_script::compile_project(&bad_selection)
        .err()
        .expect("Missing ISimpleTextSelection.selectionAnchorIndex getter must fail compilation");
    assert!(
        err_sel.iter().any(|d| {
            d.message.contains("selectionAnchorIndex") || d.message.contains("ISimpleTextSelection")
        }),
        "Expected actionable ISimpleTextSelection diagnostic, got: {err_sel:?}"
    );

    // 4. Overriding AccessibilityImplementation.get_accRole without `override` must produce a diagnostic.
    let missing_override = media_events_project(&[(
        "bad/NoOverrideAcc.as",
        r#"
        package bad {
            import flash.accessibility.AccessibilityImplementation;
            public class NoOverrideAcc extends AccessibilityImplementation {
                public function get_accRole(childID:uint):uint { return 1; }
            }
        }
        "#,
    )]);
    assert!(
        neoflash_script::compile_project(&missing_override).is_err(),
        "Redefining AccessibilityImplementation.get_accRole without `override` must fail compilation"
    );

    // 5. Extending final/static/native media, UI, accessibility, and event classes must be rejected.
    for (cls_name, base_qname) in [
        ("BadCam", "flash.media.Camera"),
        ("BadMic", "flash.media.Microphone"),
        ("BadAdm", "flash.media.AudioDeviceManager"),
        ("BadCm", "flash.ui.ContextMenu"),
        ("BadMt", "flash.ui.Multitouch"),
        ("BadAcc", "flash.accessibility.Accessibility"),
        ("BadAccProps", "flash.accessibility.AccessibilityProperties"),
        ("BadSve", "flash.events.StageVideoEvent"),
        ("BadDrm", "flash.events.DRMErrorEvent"),
    ] {
        let p = media_events_project(&[(
            "bad/Sub.as",
            &format!("package bad {{ public class {cls_name} extends {base_qname} {{}} }}"),
        )]);
        assert!(
            neoflash_script::compile_project(&p).is_err(),
            "Extending {base_qname} should be rejected with an actionable diagnostic"
        );
    }
}
