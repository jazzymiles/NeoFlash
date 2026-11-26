//! Built-in ActionScript 2/3 class, function, namespace, and host method tables.

pub(crate) const AS2_BUILTINS: &[&str] = &[
    "mx.managers.DepthManager",
    "mx.video.VideoPlayer",
    "mx.transitions.TransitionManager",
    "mx.transitions.Transition",
    "mx.transitions.Fade",
    "mx.transitions.Fly",
    "mx.transitions.Rotate",
    "mx.transitions.Zoom",
    "mx.transitions.Squeeze",
    "mx.video.FLVPlayback",
    "mx.video.VideoError",
    "mx.services.WebService",
    "mx.services.SOAPCall",
    "mx.services.PendingCall",
    "mx.services.Log",
    "mx.data.components.XMLConnector",
    "mx.data.components.WebServiceConnector",
    "mx.utils.Delegate",
    "mx.events.EventDispatcher",
    "mx.data.binding.EndPoint",
    "mx.data.binding.Binding",
    "mx.data.binding.ComponentMixins",
    "mx.data.binding.DataType",
    "mx.data.binding.TypedValue",
    "flash.net.FileReference",
    "flash.net.FileReferenceList",
    "mx.lang.Locale",
    "TextSnapshot",
    "TextField.StyleSheet",
    "mx.utils.Collection",
    "mx.utils.CollectionImpl",
    "mx.utils.Iterator",
    "mx.utils.IteratorImpl",
    "mx.controls.listclasses.DataProvider",
    "mx.data.components.DataSet",
    "mx.data.components.datasetclasses.DataSetIterator",
    "mx.data.components.datasetclasses.DeltaPacket",
    "mx.data.components.datasetclasses.Delta",
    "mx.data.components.datasetclasses.DeltaItem",
    "mx.data.components.datasetclasses.DeltaPacketConsts",
    "mx.data.to.ValueListIterator",
];

pub(crate) const BUILTINS: &[&str] = &[
    "mx.managers.DepthManager",
    "mx.video.VideoPlayer",
    "mx.transitions.TransitionManager",
    "mx.transitions.Transition",
    "mx.transitions.Fade",
    "mx.transitions.Fly",
    "mx.transitions.Rotate",
    "mx.transitions.Zoom",
    "mx.transitions.Squeeze",
    "mx.video.FLVPlayback",
    "mx.video.VideoError",
    "mx.services.WebService",
    "mx.services.SOAPCall",
    "mx.services.PendingCall",
    "mx.services.Log",
    "mx.data.components.XMLConnector",
    "mx.data.components.WebServiceConnector",
    "mx.data.binding.EndPoint",
    "mx.data.binding.Binding",
    "mx.data.binding.ComponentMixins",
    "mx.data.binding.DataType",
    "mx.data.binding.TypedValue",
    "flash.net.FileReference",
    "flash.net.FileReferenceList",
    "mx.lang.Locale",
    "TextSnapshot",
    "TextField.StyleSheet",
    "mx.utils.Collection",
    "mx.utils.CollectionImpl",
    "mx.utils.Iterator",
    "mx.utils.IteratorImpl",
    "mx.controls.listclasses.DataProvider",
    "mx.data.components.DataSet",
    "mx.data.components.datasetclasses.DataSetIterator",
    "mx.data.components.datasetclasses.DeltaPacket",
    "mx.data.components.datasetclasses.Delta",
    "mx.data.components.datasetclasses.DeltaItem",
    "mx.data.components.datasetclasses.DeltaPacketConsts",
    "mx.data.to.ValueListIterator",
    "mx.utils.Delegate",
    "mx.events.EventDispatcher",
    "Object",
    "Array",
    "flash.display.DisplayObject",
    "flash.display.InteractiveObject",
    "flash.display.DisplayObjectContainer",
    "flash.display.MovieClip",
    "flash.display.FrameLabel",
    "flash.display.Scene",
    "flash.display.SimpleButton",
    "flash.display.Sprite",
    "flash.display.Shape",
    "flash.display.Graphics",
    "flash.display.GradientType",
    "flash.display.SpreadMethod",
    "flash.display.InterpolationMethod",
    "flash.display.GraphicsPathCommand",
    "flash.display.GraphicsPathWinding",
    "flash.display.BlendMode",
    "flash.display.CapsStyle",
    "flash.display.JointStyle",
    "flash.display.LineScaleMode",
    "flash.display.Bitmap",
    "flash.display.BitmapData",
    "flash.display.BitmapDataChannel",
    "flash.display.PixelSnapping",
    "flash.display.PNGEncoderOptions",
    "flash.display.JPEGEncoderOptions",
    "flash.display.BitmapEncodingColorSpace",
    "flash.display.JPEGXREncoderOptions",
    "flash.display.IBitmapDrawable",
    "flash.display.IGraphicsData",
    "flash.display.IGraphicsFill",
    "flash.display.IGraphicsPath",
    "flash.display.IGraphicsStroke",
    "flash.display.TriangleCulling",
    "flash.display.GraphicsSolidFill",
    "flash.display.GraphicsGradientFill",
    "flash.display.GraphicsBitmapFill",
    "flash.display.GraphicsEndFill",
    "flash.display.GraphicsStroke",
    "flash.display.GraphicsPath",
    "flash.display.GraphicsTrianglePath",
    "flash.display.Loader",
    "flash.display.LoaderInfo",
    "flash.display.Stage",
    "flash.display.StageScaleMode",
    "flash.display.StageAlign",
    "flash.display.StageDisplayState",
    "flash.display.StageQuality",
    "flash.text.TextField",
    "flash.text.StaticText",
    "flash.text.TextSnapshot",
    "flash.text.TextFormat",
    "flash.text.TextFormatAlign",
    "flash.text.TextFormatDisplay",
    "flash.text.TextFieldType",
    "flash.text.TextFieldAutoSize",
    "flash.text.AntiAliasType",
    "flash.text.GridFitType",
    "flash.text.TextLineMetrics",
    "flash.text.FontStyle",
    "flash.text.FontType",
    "flash.text.TextColorType",
    "flash.text.TextDisplayMode",
    "flash.text.TextInteractionMode",
    "flash.text.CSMSettings",
    "flash.text.Font",
    "flash.text.StyleSheet",
    "flash.text.TextRenderer",
    "flash.ui.Keyboard",
    "flash.ui.KeyLocation",
    "flash.ui.KeyboardType",
    "flash.ui.Mouse",
    "flash.ui.MouseCursor",
    "flash.ui.MouseCursorData",
    "flash.ui.ContextMenu",
    "flash.ui.ContextMenuBuiltInItems",
    "flash.ui.ContextMenuClipboardItems",
    "flash.ui.ContextMenuItem",
    "flash.media.Video",
    "flash.media.VideoStatus",
    "flash.net.NetConnection",
    "flash.net.NetStream",
    "flash.events.NetStatusEvent",
    "flash.media.Sound",
    "flash.media.SoundChannel",
    "flash.media.SoundTransform",
    "flash.media.SoundMixer",
    "flash.media.SoundLoaderContext",
    "flash.media.ID3Info",
    "flash.accessibility.AccessibilityProperties",
    "flash.utils.Proxy",
    "flash.utils.flash_proxy",
    "flash.utils.object_proxy",
    "mx.utils.object_proxy",
    "mx.utils.ObjectProxy",
    "flash.filters.BitmapFilter",
    "flash.filters.BitmapFilterQuality",
    "flash.filters.BlurFilter",
    "flash.filters.GlowFilter",
    "flash.filters.DropShadowFilter",
    "flash.filters.ColorMatrixFilter",
    "flash.filters.ConvolutionFilter",
    "flash.filters.BevelFilter",
    "flash.filters.GradientGlowFilter",
    "flash.filters.GradientBevelFilter",
    "flash.filters.BitmapFilterType",
    "flash.filters.DisplacementMapFilter",
    "flash.filters.DisplacementMapFilterMode",
    "flash.filters.ShaderFilter",
    "mx.transitions.easing.Back",
    "mx.transitions.easing.Bounce",
    "mx.transitions.easing.Elastic",
    "mx.transitions.easing.None",
    "mx.transitions.easing.Regular",
    "mx.transitions.easing.Strong",
    "fl.transitions.easing.Back",
    "fl.transitions.easing.Bounce",
    "fl.transitions.easing.Elastic",
    "fl.transitions.easing.None",
    "fl.transitions.easing.Regular",
    "fl.transitions.easing.Strong",
    "fl.motion.MatrixTransformer",
    "fl.motion.Color",
    "fl.motion.CustomEase",
    "fl.motion.easing.Back",
    "fl.motion.easing.Bounce",
    "fl.motion.easing.Circular",
    "fl.motion.easing.Cubic",
    "fl.motion.easing.Elastic",
    "fl.motion.easing.Exponential",
    "fl.motion.easing.Linear",
    "fl.motion.easing.Quadratic",
    "fl.motion.easing.Quartic",
    "fl.motion.easing.Quintic",
    "fl.motion.easing.Sine",
    "fl.transitions.Tween",
    "mx.transitions.Tween",
    "fl.transitions.TweenEvent",
    "flash.geom.Point",
    "flash.geom.Rectangle",
    "flash.geom.Matrix",
    "flash.geom.Vector3D",
    "flash.geom.Matrix3D",
    "flash.geom.Orientation3D",
    "flash.geom.Utils3D",
    "flash.geom.PerspectiveProjection",
    "flash.geom.Transform",
    "flash.errors.IOError",
    "flash.errors.EOFError",
    "flash.errors.IllegalOperationError",
    "flash.errors.InvalidSWFError",
    "flash.errors.MemoryError",
    "flash.errors.ScriptTimeoutError",
    "flash.errors.StackOverflowError",
    "flash.xml.XMLNodeType",
    "flash.xml.XMLNode",
    "flash.xml.XMLDocument",
    "flash.globalization.DateTimeFormatter",
    "flash.globalization.DateTimeStyle",
    "flash.globalization.DateTimeNameStyle",
    "flash.globalization.DateTimeNameContext",
    "flash.globalization.NumberFormatter",
    "flash.globalization.NumberParseResult",
    "flash.geom.ColorTransform",
    "flash.utils.Timer",
    "flash.utils.ByteArray",
    "flash.utils.Endian",
    "flash.utils.CompressionAlgorithm",
    "flash.utils.IDataInput",
    "flash.utils.IDataOutput",
    "flash.utils.IExternalizable",
    "flash.utils.Dictionary",
    "flash.net.URLRequest",
    "flash.net.URLRequestHeader",
    "flash.net.URLRequestMethod",
    "flash.net.URLVariables",
    "flash.net.URLLoader",
    "flash.net.URLLoaderDataFormat",
    "flash.net.SharedObject",
    "flash.net.SharedObjectFlushStatus",
    "flash.net.ObjectEncoding",
    "flash.events.ProgressEvent",
    "flash.events.IOErrorEvent",
    "flash.events.HTTPStatusEvent",
    "flash.events.TimerEvent",
    "flash.events.IEventDispatcher",
    "flash.events.EventDispatcher",
    "flash.events.Event",
    "flash.events.MouseEvent",
    "flash.events.KeyboardEvent",
    "flash.events.FocusEvent",
    "flash.events.TextEvent",
    "flash.events.ErrorEvent",
    "flash.events.SecurityErrorEvent",
    "flash.events.AsyncErrorEvent",
    "flash.events.DataEvent",
    "flash.events.StatusEvent",
    "flash.events.ActivityEvent",
    "flash.events.FullScreenEvent",
    "flash.events.SyncEvent",
    "flash.events.ContextMenuEvent",
    "flash.events.IMEEvent",
    "flash.events.UncaughtErrorEvent",
    "flash.events.SampleDataEvent",
    "flash.events.AccelerometerEvent",
    "flash.events.GestureEvent",
    "flash.events.TransformGestureEvent",
    "flash.events.PressAndTapGestureEvent",
    "flash.events.TouchEvent",
    "flash.events.GesturePhase",
    "flash.events.GeolocationEvent",
    "flash.events.NetDataEvent",
    "flash.events.StageOrientationEvent",
    "flash.events.ThrottleEvent",
    "flash.events.SoftKeyboardEvent",
    "flash.events.ThrottleType",
    "flash.events.SoftKeyboardTrigger",
    "flash.events.EventPhase",
    "flash.external.ExternalInterface",
    // AIR errors
    "flash.errors.SQLErrorOperation",
    "flash.errors.SQLError",
    "flash.errors.PermissionError",
    "flash.errors.DRMManagerError",
    // AIR events
    "flash.events.TouchEventIntent",
    "flash.events.InvokeEvent",
    "flash.events.BrowserInvokeEvent",
    "flash.events.FileListEvent",
    "flash.events.OutputProgressEvent",
    "flash.events.NativeWindowBoundsEvent",
    "flash.events.NativeWindowDisplayStateEvent",
    "flash.events.NativeDragEvent",
    "flash.events.ScreenMouseEvent",
    "flash.events.SQLEvent",
    "flash.events.SQLErrorEvent",
    "flash.events.SQLUpdateEvent",
    "flash.events.StorageVolumeChangeEvent",
    "flash.events.NativeProcessExitEvent",
    "flash.events.LocationChangeEvent",
    "flash.events.HTMLUncaughtScriptExceptionEvent",
    "flash.events.MediaEvent",
    "flash.events.DatagramSocketDataEvent",
    "flash.events.ServerSocketConnectEvent",
    "flash.events.DNSResolverEvent",
    "flash.events.DRMAuthenticateEvent",
    "flash.events.DeviceRotationEvent",
    "flash.events.PermissionEvent",
    "flash.events.RemoteNotificationEvent",
    "flash.events.GameInputEvent",
    "flash.events.VsyncStateChangeAvailabilityEvent",
    "flash.events.StageVideoAvailabilityEvent",
    // AIR filesystem
    "flash.filesystem.FileMode",
    "flash.filesystem.File",
    "flash.filesystem.FileStream",
    "flash.filesystem.StorageVolume",
    "flash.filesystem.StorageVolumeInfo",
    // AIR desktop
    "flash.desktop.ClipboardFormats",
    "flash.desktop.ClipboardTransferMode",
    "flash.desktop.Clipboard",
    "flash.desktop.IFilePromise",
    "flash.desktop.NativeDragActions",
    "flash.desktop.NativeDragOptions",
    "flash.desktop.NativeDragManager",
    "flash.desktop.InvokeEventReason",
    "flash.desktop.NotificationType",
    "flash.desktop.SystemIdleMode",
    "flash.desktop.Icon",
    "flash.desktop.InteractiveIcon",
    "flash.desktop.DockIcon",
    "flash.desktop.SystemTrayIcon",
    "flash.desktop.NativeApplication",
    "flash.desktop.NativeProcessStartupInfo",
    "flash.desktop.NativeProcess",
    "flash.desktop.Updater",
    // AIR display
    "flash.display.FocusDirection",
    "flash.display.StageAspectRatio",
    "flash.display.StageOrientation",
    "flash.display.ScreenMode",
    "flash.display.Screen",
    "flash.display.NativeMenuItem",
    "flash.display.NativeMenu",
    "flash.display.NativeWindowType",
    "flash.display.NativeWindowSystemChrome",
    "flash.display.NativeWindowDisplayState",
    "flash.display.NativeWindowResize",
    "flash.display.NativeWindowRenderMode",
    "flash.display.NativeWindowInitOptions",
    "flash.display.NativeWindow",
    // AIR data
    "flash.data.EncryptedLocalStore",
    "flash.data.SQLMode",
    "flash.data.SQLTransactionLockType",
    "flash.data.SQLCollationType",
    "flash.data.SQLColumnNameStyle",
    "flash.data.SQLResult",
    "flash.data.SQLColumnSchema",
    "flash.data.SQLSchema",
    "flash.data.SQLTableSchema",
    "flash.data.SQLViewSchema",
    "flash.data.SQLIndexSchema",
    "flash.data.SQLTriggerSchema",
    "flash.data.SQLSchemaResult",
    "flash.data.SQLConnection",
    "flash.data.SQLStatement",
    // AIR permissions, dns, notifications, sensors
    "flash.permissions.PermissionStatus",
    "flash.net.dns.ResourceRecord",
    "flash.net.dns.ARecord",
    "flash.net.dns.AAAARecord",
    "flash.net.dns.MXRecord",
    "flash.net.dns.PTRRecord",
    "flash.net.dns.SRVRecord",
    "flash.net.dns.DNSResolver",
    "flash.notifications.NotificationStyle",
    "flash.notifications.RemoteNotifierSubscribeOptions",
    "flash.notifications.RemoteNotifier",
    "flash.sensors.DeviceRotation",
    "flash.sensors.Geolocation",
    "flash.sensors.Accelerometer",
    // Area 8 concurrent, globalization, net, drm, printing, profiler, sampler, security, system
    "flash.concurrent.Mutex",
    "flash.concurrent.Condition",
    "flash.globalization.LastOperationStatus",
    "flash.globalization.NationalDigitsType",
    "flash.globalization.CollatorMode",
    "flash.globalization.LocaleID",
    "flash.globalization.CurrencyParseResult",
    "flash.globalization.CurrencyFormatter",
    "flash.globalization.Collator",
    "flash.globalization.StringTools",
    "flash.net.Responder",
    "flash.net.DynamicPropertyOutput",
    "flash.net.FileFilter",
    "flash.net.GroupSpecifier",
    "flash.net.NetGroupReceiveMode",
    "flash.net.NetGroupReplicationStrategy",
    "flash.net.NetGroupSendMode",
    "flash.net.NetGroupSendResult",
    "flash.net.NetGroupInfo",
    "flash.net.NetGroup",
    "flash.net.LocalConnection",
    "flash.net.NetStreamAppendBytesAction",
    "flash.net.NetStreamPlayTransitions",
    "flash.net.NetStreamPlayOptions",
    "flash.net.NetStreamInfo",
    "flash.net.NetStreamMulticastInfo",
    "flash.net.NetMonitor",
    "flash.net.Socket",
    "flash.net.SecureSocket",
    "flash.net.URLStream",
    "flash.net.XMLSocket",
    "flash.net.drm.AuthenticationMethod",
    "flash.net.drm.LoadVoucherSetting",
    "flash.net.drm.VoucherAccessInfo",
    "flash.net.drm.DRMPlaybackTimeWindow",
    "flash.net.drm.DRMVoucher",
    "flash.net.drm.DRMContentData",
    "flash.net.drm.DRMDeviceGroup",
    "flash.net.drm.DRMManager",
    "flash.printing.PrintJob",
    "flash.printing.PrintJobOptions",
    "flash.printing.PrintJobOrientation",
    "flash.profiler.Telemetry",
    "flash.sampler.StackFrame",
    "flash.sampler.Sample",
    "flash.sampler.NewObjectSample",
    "flash.sampler.DeleteObjectSample",
    "flash.security.CertificateStatus",
    "flash.security.X500DistinguishedName",
    "flash.security.X509Certificate",
    "flash.system.ApplicationDomain",
    "flash.system.Capabilities",
    "flash.system.IME",
    "flash.system.IMEConversionMode",
    "flash.system.LoaderContext",
    "flash.system.JPEGLoaderContext",
    "flash.system.MessageChannel",
    "flash.system.MessageChannelState",
    "flash.system.Security",
    "flash.system.SecurityDomain",
    "flash.system.SecurityPanel",
    "flash.system.System",
    "flash.system.SystemUpdater",
    "flash.system.SystemUpdaterType",
    "flash.system.TouchscreenType",
    "flash.system.Worker",
    "flash.system.WorkerDomain",
    "flash.system.WorkerState",
    // AS3 Area 8: FTE, IME, Stage3D, Shaders, Display/Loader 3D & metadata
    "flash.display.ActionScriptVersion",
    "flash.display.SWFVersion",
    "flash.display.ColorCorrection",
    "flash.display.ColorCorrectionSupport",
    "flash.display.MorphShape",
    "flash.display.AVM1Movie",
    "flash.display.Shader",
    "flash.display.ShaderData",
    "flash.display.ShaderInput",
    "flash.display.ShaderParameter",
    "flash.display.ShaderParameterType",
    "flash.display.ShaderPrecision",
    "flash.display.ShaderJob",
    "flash.display.GraphicsShaderFill",
    "flash.display.Stage3D",
    "flash.filters.ShaderFilter",
    "flash.display3D.Context3D",
    "flash.display3D.Context3DBlendFactor",
    "flash.display3D.Context3DBufferUsage",
    "flash.display3D.Context3DClearMask",
    "flash.display3D.Context3DCompareMode",
    "flash.display3D.Context3DFillMode",
    "flash.display3D.Context3DMipFilter",
    "flash.display3D.Context3DProfile",
    "flash.display3D.Context3DProgramType",
    "flash.display3D.Context3DRenderMode",
    "flash.display3D.Context3DStencilAction",
    "flash.display3D.Context3DTextureFilter",
    "flash.display3D.Context3DTextureFormat",
    "flash.display3D.Context3DTriangleFace",
    "flash.display3D.Context3DVertexBufferFormat",
    "flash.display3D.Context3DWrapMode",
    "flash.display3D.IndexBuffer3D",
    "flash.display3D.Program3D",
    "flash.display3D.VertexBuffer3D",
    "flash.display3D.textures.TextureBase",
    "flash.display3D.textures.Texture",
    "flash.display3D.textures.CubeTexture",
    "flash.display3D.textures.RectangleTexture",
    "flash.display3D.textures.VideoTexture",
    "flash.text.engine.BreakOpportunity",
    "flash.text.engine.CFFHinting",
    "flash.text.engine.ContentElement",
    "flash.text.engine.DigitCase",
    "flash.text.engine.DigitWidth",
    "flash.text.engine.EastAsianJustifier",
    "flash.text.engine.ElementFormat",
    "flash.text.engine.FontDescription",
    "flash.text.engine.FontLookup",
    "flash.text.engine.FontMetrics",
    "flash.text.engine.FontPosture",
    "flash.text.engine.FontWeight",
    "flash.text.engine.GraphicElement",
    "flash.text.engine.GroupElement",
    "flash.text.engine.JustificationStyle",
    "flash.text.engine.Kerning",
    "flash.text.engine.LigatureLevel",
    "flash.text.engine.LineJustification",
    "flash.text.engine.RenderingMode",
    "flash.text.engine.SpaceJustifier",
    "flash.text.engine.TabAlignment",
    "flash.text.engine.TabStop",
    "flash.text.engine.TextBaseline",
    "flash.text.engine.TextBlock",
    "flash.text.engine.TextElement",
    "flash.text.engine.TextJustifier",
    "flash.text.engine.TextLine",
    "flash.text.engine.TextLineCreationResult",
    "flash.text.engine.TextLineMirrorRegion",
    "flash.text.engine.TextLineValidity",
    "flash.text.engine.TextRotation",
    "flash.text.engine.TypographicCase",
    "flash.text.ime.CompositionAttributeRange",
    "flash.text.ime.IIMEClient",
    // AS3 Area 8: Accessibility, Media, UI, and Media/AV/DRM Events
    "flash.accessibility.Accessibility",
    "flash.accessibility.AccessibilityImplementation",
    "flash.accessibility.ISearchableText",
    "flash.accessibility.ISimpleTextSelection",
    "flash.events.AVDictionaryDataEvent",
    "flash.events.AVHTTPStatusEvent",
    "flash.events.AudioOutputChangeEvent",
    "flash.events.DRMAuthenticationCompleteEvent",
    "flash.events.DRMAuthenticationErrorEvent",
    "flash.events.DRMDeviceGroupErrorEvent",
    "flash.events.DRMDeviceGroupEvent",
    "flash.events.DRMErrorEvent",
    "flash.events.DRMLicenseRequestEvent",
    "flash.events.DRMMetadataEvent",
    "flash.events.DRMReturnVoucherCompleteEvent",
    "flash.events.DRMReturnVoucherErrorEvent",
    "flash.events.DRMStatusEvent",
    "flash.events.NetFilterEvent",
    "flash.events.NetMonitorEvent",
    "flash.events.ShaderEvent",
    "flash.events.StageVideoEvent",
    "flash.events.UncaughtErrorEvents",
    "flash.events.VideoEvent",
    "flash.events.VideoTextureEvent",
    "flash.media.AVTagData",
    "flash.media.AVURLLoader",
    "flash.media.AVURLStream",
    "flash.media.AudioDecoder",
    "flash.media.AudioDeviceManager",
    "flash.media.AudioOutputChangeReason",
    "flash.media.Camera",
    "flash.media.CameraPosition",
    "flash.media.H264Level",
    "flash.media.H264Profile",
    "flash.media.H264VideoStreamSettings",
    "flash.media.Microphone",
    "flash.media.MicrophoneEnhancedMode",
    "flash.media.MicrophoneEnhancedOptions",
    "flash.media.SoundCodec",
    "flash.media.StageVideo",
    "flash.media.StageVideoAvailability",
    "flash.media.StageVideoAvailabilityReason",
    "flash.media.VP6VideoStreamSettings",
    "flash.media.VideoCodec",
    "flash.media.VideoStreamSettings",
    // AIR HTML, media, networking, printing, security, text, ANE & system
    "flash.external.ExtensionContext",
    "flash.html.HTMLHistoryItem",
    "flash.html.HTMLHost",
    "flash.html.HTMLLoader",
    "flash.html.HTMLPDFCapability",
    "flash.html.HTMLSWFCapability",
    "flash.html.HTMLWindowCreateOptions",
    "flash.media.AudioPlaybackMode",
    "flash.media.CameraRoll",
    "flash.media.CameraRollBrowseOptions",
    "flash.media.CameraUI",
    "flash.media.MediaPromise",
    "flash.media.MediaType",
    "flash.media.StageWebView",
    "flash.net.DatagramSocket",
    "flash.net.IPVersion",
    "flash.net.InterfaceAddress",
    "flash.net.NetworkInfo",
    "flash.net.NetworkInterface",
    "flash.net.ServerSocket",
    "flash.net.URLRequestDefaults",
    "flash.printing.PaperSize",
    "flash.printing.PrintMethod",
    "flash.printing.PrintUIOptions",
    "flash.security.IURIDereferencer",
    "flash.security.ReferencesValidationSetting",
    "flash.security.RevocationCheckSettings",
    "flash.security.SignatureStatus",
    "flash.security.SignerTrustSettings",
    "flash.security.XMLSignatureValidator",
    "flash.system.ImageDecodingPolicy",
    "flash.text.AutoCapitalize",
    "flash.text.ReturnKeyLabel",
    "flash.text.SoftKeyboardType",
    "flash.text.StageText",
    "flash.text.StageTextClearButtonMode",
    "flash.text.StageTextInitOptions",
    "flash.ui.GameInput",
    "flash.ui.GameInputControl",
    "flash.ui.GameInputDevice",
    "flash.ui.Multitouch",
    "flash.ui.MultitouchInputMode",
];
pub(super) const BUILTIN_FUNCTIONS: &[&str] = &[
    "flash.utils.getQualifiedClassName",
    "flash.utils.getQualifiedSuperclassName",
    "flash.utils.getDefinitionByName",
    "flash.utils.describeType",
    "flash.utils.getTimer",
    "flash.utils.escapeMultiByte",
    "flash.utils.unescapeMultiByte",
    "flash.utils.setInterval",
    "flash.utils.setTimeout",
    "flash.utils.clearInterval",
    "flash.utils.clearTimeout",
    "flash.net.navigateToURL",
    "flash.net.sendToURL",
    "flash.net.registerClassAlias",
    "flash.net.getClassByAlias",
    "flash.crypto.generateRandomBytes",
    "flash.media.scanHardware",
    "flash.profiler.profile",
    "flash.profiler.showRedrawRegions",
    "flash.sampler.clearSamples",
    "flash.sampler.getGetterInvocationCount",
    "flash.sampler.getInvocationCount",
    "flash.sampler.getLexicalScopes",
    "flash.sampler.getMasterString",
    "flash.sampler.getMemberNames",
    "flash.sampler.getSampleCount",
    "flash.sampler.getSamples",
    "flash.sampler.getSavedThis",
    "flash.sampler.getSetterInvocationCount",
    "flash.sampler.getSize",
    "flash.sampler.isGetterSetter",
    "flash.sampler.pauseSampling",
    "flash.sampler.sampleInternalAllocs",
    "flash.sampler.setSamplerCallback",
    "flash.sampler.startSampling",
    "flash.sampler.stopSampling",
    "flash.system.fscommand",
];
pub(super) const BUILTIN_NAMESPACES: &[(&str, &str)] = &[
    ("AS3", "http://adobe.com/AS3/2006/builtin"),
    (
        "flash.utils.flash_proxy",
        "http://www.adobe.com/2006/actionscript/flash/proxy",
    ),
    (
        "flash.utils.object_proxy",
        "http://www.adobe.com/2006/flex/mx/internal",
    ),
    (
        "mx.utils.object_proxy",
        "http://www.adobe.com/2006/flex/mx/internal",
    ),
];
pub(super) fn builtin_namespace_uri(name: &str) -> Option<&'static str> {
    BUILTIN_NAMESPACES
        .iter()
        .find_map(|(k, uri)| (*k == name).then_some(*uri))
}
pub(in crate::actionscript) fn is_graphics_constants(name: &str) -> bool {
    matches!(
        name,
        "flash.display.GradientType"
            | "flash.display.SpreadMethod"
            | "flash.display.InterpolationMethod"
            | "flash.display.GraphicsPathCommand"
            | "flash.display.GraphicsPathWinding"
            | "flash.display.TriangleCulling"
            | "flash.display.BitmapEncodingColorSpace"
            | "flash.display.BlendMode"
            | "flash.display.CapsStyle"
            | "flash.display.JointStyle"
            | "flash.display.LineScaleMode"
    )
}
pub(super) fn is_display_method(name: &str) -> bool {
    matches!(
        name,
        "addEventListener"
            | "removeEventListener"
            | "hasEventListener"
            | "willTrigger"
            | "dispatchEvent"
            | "getBounds"
            | "getRect"
            | "localToGlobal"
            | "globalToLocal"
            | "hitTestObject"
            | "hitTestPoint"
            | "hitTest"
            | "startDrag"
            | "stopDrag"
            | "getURL"
            | "loadMovie"
            | "unloadMovie"
            | "getChildAt"
            | "getChildByName"
            | "getChildIndex"
            | "addChild"
            | "addChildAt"
            | "removeChild"
            | "removeChildAt"
            | "removeChildren"
            | "stopAllMovieClips"
            | "setChildIndex"
            | "setMask"
            | "swapChildren"
            | "swapChildrenAt"
            | "createEmptyMovieClip"
            | "createTextField"
            | "removeMovieClip"
            | "getNextHighestDepth"
            | "attachBitmap"
            | "beginFill"
            | "beginBitmapFill"
            | "beginGradientFill"
            | "lineGradientStyle"
            | "endFill"
            | "lineStyle"
            | "moveTo"
            | "lineTo"
            | "curveTo"
            | "clear"
            | "contains"
            | "stopPropagation"
            | "stopImmediatePropagation"
            | "preventDefault"
            | "isDefaultPrevented"
    )
}

pub(in crate::actionscript) fn is_host_method(name: &str) -> bool {
    is_display_method(name)
        || matches!(
            name,
            "clone"
                | "addCallback"
                | "send"
                | "cubicCurveTo"
                | "drawRect"
                | "drawRoundRect"
                | "drawRoundRectComplex"
                | "drawPath"
                | "drawEllipse"
                | "drawCircle"
                | "copyFrom"
                | "setTo"
                | "add"
                | "subtract"
                | "equals"
                | "offset"
                | "normalize"
                | "containsPoint"
                | "containsRect"
                | "containsRectangle"
                | "intersection"
                | "intersects"
                | "union"
                | "offsetPoint"
                | "inflate"
                | "inflatePoint"
                | "isEmpty"
                | "setEmpty"
                | "identity"
                | "concat"
                | "transformPoint"
                | "deltaTransformPoint"
                | "invert"
                | "rotate"
                | "scale"
                | "translate"
                | "createBox"
                | "createGradientBox"
                | "getRelativeMatrix"
                | "getRelativeMatrix3D"
                | "requestSoftKeyboard"
                | "getImageReference"
                | "extract"
                | "loadCompressedDataFromByteArray"
                | "loadPCMFromByteArray"
                | "computeSpectrum"
                | "stopAll"
                | "areSoundsInaccessible"
                | "attachCamera"
                | "start"
                | "reset"
                | "updateAfterEvent"
                | "isActive"
                | "updateProperties"
                | "toggleHighQuality"
                | "showRedrawRegions"
                | "load"
                | "loadBytes"
                | "connect"
                | "attachNetStream"
                | "attachVideo"
                | "pause"
                | "resume"
                | "togglePause"
                | "seek"
                | "unload"
                | "getLocal"
                | "flush"
                | "setProperty"
                | "setDirty"
                | "readObject"
                | "writeObject"
                | "readExternal"
                | "writeExternal"
                | "close"
                | "decode"
                | "getPixel"
                | "getPixel32"
                | "setPixel"
                | "setPixel32"
                | "fillRect"
                | "copyPixels"
                | "draw"
                | "applyFilter"
                | "generateFilterRect"
                | "colorTransform"
                | "getPixels"
                | "setPixels"
                | "copyPixelsToByteArray"
                | "encode"
                | "scroll"
                | "dispose"
                | "copyChannel"
                | "floodFill"
                | "getColorBoundsRect"
                | "threshold"
                | "merge"
                | "lock"
                | "unlock"
                | "getVector"
                | "setVector"
                | "compare"
                | "noise"
                | "perlinNoise"
                | "paletteMap"
                | "pixelDissolve"
                | "loadBitmap"
                | "hitTest"
                | "appendText"
                | "replaceText"
                | "replaceSel"
                | "replaceSelectedText"
                | "removeTextField"
                | "getDepth"
                | "getTextExtent"
                | "getStyleNames"
                | "setSelection"
                | "getLineText"
                | "getLineLength"
                | "getLineOffset"
                | "getLineIndexOfChar"
                | "getLineMetrics"
                | "getCharBoundaries"
                | "getCharIndexAtPoint"
                | "getLineIndexAtPoint"
                | "getFirstCharInParagraph"
                | "getParagraphLength"
                | "getTextFormat"
                | "setTextFormat"
                | "getNewTextFormat"
                | "setNewTextFormat"
                | "invalidate"
                | "hide"
                | "show"
                | "registerCursor"
                | "unregisterCursor"
                | "isAccessible"
                | "atomicCompareAndSwapIntAt"
                | "atomicCompareAndSwapLength"
                | "readBoolean"
                | "readByte"
                | "readBytes"
                | "readDouble"
                | "readFloat"
                | "readInt"
                | "readMultiByte"
                | "readShort"
                | "readUnsignedByte"
                | "readUnsignedInt"
                | "readUnsignedShort"
                | "readUTF"
                | "readUTFBytes"
                | "writeBoolean"
                | "writeByte"
                | "writeBytes"
                | "writeDouble"
                | "writeFloat"
                | "writeInt"
                | "writeMultiByte"
                | "writeShort"
                | "writeUnsignedInt"
                | "writeUTF"
                | "writeUTFBytes"
                | "compress"
                | "uncompress"
                | "deflate"
                | "parseCSS"
                | "getStyle"
                | "setStyle"
                | "transform"
                | "enumerateFonts"
                | "registerFont"
                | "hasGlyphs"
                | "setAdvancedAntiAliasingTable"
                | "setAdvancedAntialiasingTable"
                | "decompose"
                | "recompose"
                | "interpolate"
                | "interpolateTo"
                | "pointAt"
                | "pointTowards"
                | "projectVector"
                | "projectVectors"
                | "transformVector"
                | "deltaTransformVector"
                | "transformVectors"
                | "append"
                | "prepend"
                | "appendRotation"
                | "prependRotation"
                | "appendScale"
                | "prependScale"
                | "appendTranslation"
                | "prependTranslation"
                | "copyColumnFrom"
                | "copyColumnTo"
                | "copyRowFrom"
                | "copyRowTo"
                | "copyRawDataFrom"
                | "copyRawDataTo"
                | "copyToMatrix3D"
                | "transpose"
                | "toMatrix3D"
                | "addToOutput"
                | "subtractToOutput"
                | "crossProduct"
                | "crossProductToOutput"
                | "dotProduct"
                | "incrementBy"
                | "decrementBy"
                | "nearEquals"
                | "negate"
                | "project"
                | "scaleBy"
                | "angleBetween"
                | "distance"
                | "appendChild"
                | "cloneNode"
                | "getNamespaceForPrefix"
                | "getPrefixForNamespace"
                | "hasChildNodes"
                | "insertBefore"
                | "removeNode"
                | "createElement"
                | "createTextNode"
                | "parseXML"
                | "getObjectsUnderPoint"
                | "areInaccessibleObjectsUnderPoint"
                | "startTouchDrag"
                | "stopTouchDrag"
                | "assignFocus"
                | "isFocusInaccessible"
                | "drawGraphicsData"
                | "readGraphicsData"
                | "lineBitmapStyle"
                | "drawTriangles"
                | "wideLineTo"
                | "wideMoveTo"
                | "histogram"
                | "drawWithQuality"
                | "isFontCompatible"
                | "findText"
                | "getSelected"
                | "getSelectedText"
                | "getText"
                | "getTextRunInfo"
                | "hitTestTextNearPos"
                | "setSelectColor"
                | "setSelected"
                | "getStackTrace"
                // AIR host methods
                | "open"
                | "openAsync"
                | "truncate"
                | "resolvePath"
                | "getRelativePath"
                | "canonicalize"
                | "createDirectory"
                | "createTempFile"
                | "createTempDirectory"
                | "getDirectoryListing"
                | "getDirectoryListingAsync"
                | "copyTo"
                | "copyToAsync"
                | "moveTo"
                | "moveToAsync"
                | "deleteFile"
                | "deleteFileAsync"
                | "deleteDirectory"
                | "deleteDirectoryAsync"
                | "moveToTrash"
                | "moveToTrashAsync"
                | "openWithDefaultApplication"
                | "browseForOpen"
                | "browseForOpenMultiple"
                | "browseForSave"
                | "browseForDirectory"
                | "requestPermission"
                | "getRootDirectories"
                | "getStorageVolumes"
                | "save"
                | "browse"
                | "cancel"
                | "clearData"
                | "getData"
                | "setData"
                | "setDataHandler"
                | "hasFormat"
                | "reportError"
                | "acceptDragDrop"
                | "doDrag"
                | "bounce"
                | "activate"
                | "exit"
                | "copy"
                | "cut"
                | "paste"
                | "selectAll"
                | "isSetAsDefaultApplication"
                | "setAsDefaultApplication"
                | "removeAsDefaultApplication"
                | "getDefaultApplication"
                | "closeInput"
                | "isValidExecutable"
                | "update"
                | "getScreensForRectangle"
                | "addItem"
                | "addItemAt"
                | "addSubmenu"
                | "addSubmenuAt"
                | "containsItem"
                | "getItemAt"
                | "getItemByName"
                | "getItemIndex"
                | "removeItem"
                | "removeItemAt"
                | "removeAllItems"
                | "setItemIndex"
                | "display"
                | "globalToScreen"
                | "listOwnedWindows"
                | "maximize"
                | "minimize"
                | "restore"
                | "notifyUser"
                | "orderToFront"
                | "orderToBack"
                | "orderInFrontOf"
                | "orderInBackOf"
                | "startMove"
                | "startResize"
                | "setAspectRatio"
                | "setOrientation"
                | "getItem"
                | "setItem"
                | "attach"
                | "detach"
                | "begin"
                | "commit"
                | "rollback"
                | "setSavepoint"
                | "releaseSavepoint"
                | "rollbackToSavepoint"
                | "compact"
                | "analyze"
                | "deanalyze"
                | "reencrypt"
                | "loadSchema"
                | "getSchemaResult"
                | "clearParameters"
                | "execute"
                | "getResult"
                | "next"
                | "lookup"
                | "subscribe"
                | "unsubscribe"
                | "setRequestedUpdateInterval"
                | "wait"
                | "notify"
                | "notifyAll"
                | "tryLock"
                | "determinePreferredLocales"
                | "getAvailableLocaleIDNames"
                | "getLanguage"
                | "getRegion"
                | "getScript"
                | "getVariant"
                | "getKeysAndValues"
                | "isRightToLeft"
                | "format"
                | "formattingWithCurrencySymbolIsSafe"
                | "setCurrency"
                | "parse"
                | "useRedirectedURL"
                | "download"
                | "upload"
                | "addBootstrapPeer"
                | "addIPMulticastAddress"
                | "authorizations"
                | "encodeBootstrapPeerIDSpec"
                | "encodeIPMulticastAddressSpec"
                | "encodePostingAuthorization"
                | "encodePublishAuthorization"
                | "groupspecWithAuthorizations"
                | "groupspecWithoutAuthorizations"
                | "makeUnique"
                | "setPostingPassword"
                | "setPublishPassword"
                | "addHaveObjects"
                | "addMemberHint"
                | "addNeighbor"
                | "addWantObjects"
                | "convertPeerIDToGroupAddress"
                | "createGroupAddress"
                | "denyRequestedObject"
                | "post"
                | "removeHaveObjects"
                | "removeWantObjects"
                | "satisfyRequestedObject"
                | "writeRequestedObject"
                | "addBinaryChainBuildingCertificate"
                | "getRemote"
                | "compositionSelectionChanged"
                | "compositionAbandoned"
                | "sendToAllNeighbors"
                | "sendToNearest"
                | "sendToNeighbor"
                | "allowDomain"
                | "allowInsecureDomain"
                | "addHeader"
                | "call"
                | "appendBytes"
                | "appendBytesAction"
                | "attachAudio"
                | "onPeerConnect"
                | "play2"
                | "publish"
                | "receiveAudio"
                | "receiveVideo"
                | "receiveVideoFPS"
                | "step"
                | "listStreams"
                | "resetDRMVouchers"
                | "addToDeviceGroup"
                | "authenticate"
                | "loadPreviewVoucher"
                | "loadVoucher"
                | "removeFromDeviceGroup"
                | "setAuthenticationToken"
                | "addPage"
                | "registerCommandHandler"
                | "sendMetric"
                | "sendSpanMetric"
                | "sendSpanValue"
                | "unregisterCommandHandler"
                | "getDefinition"
                | "hasDefinition"
                | "hasMultiChannelAudio"
                | "supports32BitProcesses"
                | "supports64BitProcesses"
                | "doConversion"
                | "setCompositionString"
                | "checkPolicyFile"
                | "receive"
                | "loadPolicyFile"
                | "loadPolicyFileFromBytes"
                | "showSettings"
                | "disposeXML"
                | "gc"
                | "setClipboard"
                | "createMessageChannel"
                | "getSharedProperty"
                | "setSharedProperty"
                | "terminate"
                | "createWorker"
                | "listWorkers"
                | "local3DToGlobal"
                | "globalToLocal3D"
                | "beginShaderFill"
                | "lineShaderStyle"
                | "unloadAndStop"
                | "getLoaderInfoByDefinition"
                | "isDeviceFontCompatible"
                | "getFontMetrics"
                | "getElementAt"
                | "getElementAtCharIndex"
                | "getElementIndex"
                | "groupElements"
                | "mergeTextElements"
                | "replaceElements"
                | "setElements"
                | "splitTextElement"
                | "ungroupElements"
                | "getJustifierForLocale"
                | "createJustifierForLocale"
                | "createTextLine"
                | "recreateTextLine"
                | "findNextAtomBoundary"
                | "findNextWordBoundary"
                | "findPreviousAtomBoundary"
                | "findPreviousWordBoundary"
                | "getTextLineAtCharIndex"
                | "releaseLineCreationData"
                | "releaseLines"
                | "dump"
                | "flushAtomData"
                | "getAtomBidiLevel"
                | "getAtomBounds"
                | "getAtomCenter"
                | "getAtomGraphic"
                | "getAtomIndexAtCharIndex"
                | "getAtomIndexAtPoint"
                | "getAtomTextBlockBeginIndex"
                | "getAtomTextBlockEndIndex"
                | "getAtomTextRotation"
                | "getAtomWordBoundaryOnLeft"
                | "getBaselinePosition"
                | "getMirrorRegion"
                | "confirmComposition"
                | "getTextBounds"
                | "getTextInRange"
                | "selectRange"
                | "updateComposition"
                | "requestContext3D"
                | "requestContext3DMatchingProfiles"
                | "configureBackBuffer"
                | "createCubeTexture"
                | "createIndexBuffer"
                | "createProgram"
                | "createRectangleTexture"
                | "createTexture"
                | "createVertexBuffer"
                | "createVideoTexture"
                | "drawToBitmapData"
                | "present"
                | "setBlendFactors"
                | "setColorMask"
                | "setCulling"
                | "setDepthTest"
                | "setFillMode"
                | "setProgram"
                | "setProgramConstantsFromByteArray"
                | "setProgramConstantsFromMatrix"
                | "setProgramConstantsFromVector"
                | "setRenderToBackBuffer"
                | "setRenderToTexture"
                | "setSamplerStateAt"
                | "setScissorRectangle"
                | "setStencilActions"
                | "setStencilReferenceValue"
                | "setTextureAt"
                | "setVertexBufferAt"
                | "uploadFromByteArray"
                | "uploadFromVector"
                | "uploadCompressedTextureFromByteArray"
                | "uploadFromBitmapData"
                | "getSamples"
                | "isToolButtonDown"
                | "getCamera"
                | "getMicrophone"
                | "getEnhancedMicrophone"
                | "setMode"
                | "setQuality"
                | "setKeyFrameInterval"
                | "setLoopback"
                | "setLoopBack"
                | "setMotionLevel"
                | "setCursor"
                | "copyToByteArray"
                | "copyToVector"
                | "setSilenceLevel"
                | "setUseEchoSuppression"
                | "setProfileLevel"
                | "hideBuiltInItems"
                | "systemClearMenuItem"
                | "systemCopyLinkMenuItem"
                | "systemCopyMenuItem"
                | "systemCutMenuItem"
                | "systemOpenLinkMenuItem"
                | "systemPasteMenuItem"
                | "systemSelectAllMenuItem"
                | "cancelLoad"
                | "createRootWindow"
                | "getHistoryAt"
                | "historyBack"
                | "historyForward"
                | "historyGo"
                | "loadString"
                | "reload"
                | "createWindow"
                | "updateLocation"
                | "updateStatus"
                | "updateTitle"
                | "windowBlur"
                | "windowClose"
                | "windowFocus"
                | "loadURL"
                | "drawViewPortToBitmapData"
                | "addBitmapData"
                | "browseForImage"
                | "launch"
                | "bind"
                | "listen"
                | "findInterfaces"
                | "setLoginCredentialsForHost"
                | "uploadUnencoded"
                | "preloadEmbeddedData"
                | "setDRMAuthenticationCredentials"
                | "selectPaperSize"
                | "showPageSetupDialog"
                | "start2"
                | "addCertificate"
                | "verify"
                | "dereference"
                | "createExtensionContext"
                | "getExtensionDirectory"
                | "getDeviceAt"
                | "getControlAt"
                | "startCachingSamples"
                | "stopCachingSamples"
                | "getCachedSamples"
                | "sendEvent"
                | "accDoDefaultAction"
                | "accLocation"
                | "accSelect"
                | "getChildIDArray"
                | "get_accDefaultAction"
                | "get_accFocus"
                | "get_accName"
                | "get_accRole"
                | "get_accSelection"
                | "get_accState"
                | "get_accValue"
                | "isLabeledBy"
                | "get_selectionActiveIndex"
                | "get_selectionAnchorIndex"
                | "loadFilePromise"
                | "createVertexBufferForInstances"
                | "drawTrianglesInstanced"
                | "uploadFromBitmapDataAsync"
                | "uploadFromByteArrayAsync"
                | "writeDynamicProperty"
                | "writeDynamicProperties"
                | "getQualifiedDefinitionNames"
                | "li8"
                | "li16"
                | "li32"
                | "lf32"
                | "lf64"
                | "si8"
                | "si16"
                | "si32"
                | "sf32"
                | "sf64"
                | "sxi1"
                | "sxi8"
                | "sxi16"
                | "pauseForGCIfCollectionImminent"
                | "returnVoucher"
        )
}
