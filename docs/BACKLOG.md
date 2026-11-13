# Known issues and open follow-ups

This page lists concrete defects and the follow-ups that each delivered family left open. It does not repeat other
records:

- Planned and partial **product functions**: the progress registry ([summary](progress/DETAILS.md),
  `docs/progress/product.json`).
- **ActionScript** declaration status and per-declaration gaps: the [ActionScript audit](progress/ACTIONSCRIPT.md).
- **Defects found during verification** and their status: [VERIFICATION.md › Open findings](VERIFICATION.md#open-findings-and-scoped-fixes).
- Future direction: [ROADMAP.md](ROADMAP.md). Legacy FLA approximations: [IMPORTS.md](IMPORTS.md).

## Standing HTML5 correctness requirements

- Known built-in properties implement their contract or diagnose unsupported behaviour. Visibility agrees across
  pixels, scene, hit testing and semantic targets; text, dimensions and callback assignments never become silent
  custom fields.
- XFL Include Hidden Layers policy is preserved for root and nested timelines; placement lifetime stays distinct from
  XML slot order; ambiguous transfers are rejected.
- AS2 and AS3 keep separate value profiles (parseInt radix, Number.toFixed limits, typed slot writes and defaults,
  rotation normalization, public void results).
- Each feature batch keeps a representative source project, an ordinary HTML5 publish and an aggregate browser
  result; documented expectations stay separate from original Flash measurements, and tested browser engines are named.

Still open: broader owner-aware unsupported-API diagnostics (TextField mutation and callbacks outside the supported
subset), typed handler slots and defaults across aliases, complete visibility conversion, construction and lifecycle
conformance, rotated dimension setters.

## Runtime family follow-ups

Recorded when each family was delivered (October 2026). Later ActionScript conformance work may have closed some of
them; check the ActionScript audit before starting one.

- **Text and fonts:** rich character runs, HTML text, input/IME, auto-size and scrolling, fuller metrics, embedded
  fonts; mixed runs in input forms, full IME/locale/grapheme behaviour, initial keyboard traversal, Canvas occlusion of
  active DOM editing; general AS2 string target resolution.
- **Bitmaps and loading:** broader Loader formats, security and timing; SWF/GIF loading, application/security domains,
  native subclasses, legacy target paths/levels, full garbage collection; `drawWithQuality` and non-normal blends,
  external mask dependencies, other encoders.
- **Graphics, masks and filters:** bitmap fills, repeat/reflect spreads, linearRGB; animated authored mask sources,
  bitmap/text/alpha masks, fractional or deferred scroll rectangles; remaining filter families, high-density raster
  quality, filtered DOM text input, full cacheAsBitmap and bounds semantics, original-player differential. The masks
  example overlaps an explanatory label in phase two.
- **Audio and video:** streaming, linkage, historical mixing, re-synchronising streams after dropped frames; video
  timeline synchronization, codec/seek/error fidelity, RTMP/camera/publishing, richer media authoring.
- **Data and XML:** regex literals, broader PCRE parity, historical Date parsing/DST/locale, remaining native JSON
  encodings; E4X filters, scoped default namespace, dynamic names, namespace interpolation, XML/Namespace/QName
  equality and addition, wildcard target creation, class namespace declarations.
- **Storage:** typed AMF aliases, externalizable/XML/Vector/Dictionary values, `.sol` files, remote synchronization,
  cross-tab notifications.
- **Language:** complete AVM global-scope lookup, prototype and reflection parity, `arguments.caller`, runtime-built
  Function code, generalized compile-time defaults and typing, native overrides, user namespaces, full
  initialization-order conformance; native display enumeration and historical mutation/coercion behaviour.
- **Pointer and drag:** full legacy enabled/trackAsMenu behaviour, transparent-fill/glyph/filter hit fidelity,
  reparenting during drag, multi-touch/3D, cross-engine and original-player comparison.

## `0.1.0-beta.1` Known Issues & Limitations

- **Release target & manual updates:** `0.1.0-beta.1` targets **macOS 15+ (Apple Silicon)** and provides Windows x64 and Ubuntu 24.04-compatible Linux x64 Technical Previews. Intel (`x86_64`) macOS builds and a built-in automatic updater are not included in Beta 1. The unsigned Windows archive may trigger SmartScreen; its private connected debugger and physical Windows 11 desktop acceptance remain pending. Linux AppImage/deb packaging and broader-distro acceptance remain pending. To update `NeoFlash.app`, replace the bundle in `/Applications`; user preferences, custom workspaces, recent files and crash recovery under `~/Library/Application Support/NeoFlash` are preserved. Keep `.nflash` JSON projects in Git or regular file backups, and attach a minimal `.nflash` reproduction, `Output` panel diagnostics, and the build commit from **NeoFlash → About NeoFlash** when reporting bugs.
- **Bundled media helpers:** Platform packages carry `ffmpeg` and `ffprobe` in `Contents/Resources/media/bin/` (macOS) or `media/bin/` (Windows/Linux), requiring no separate media-tool installation. The Windows/Linux helpers include codec dependencies but use normal system runtime libraries. Exact Rust/media licence evidence is inside the package and a checksummed corresponding-source archive is beside it; the package gate refuses incomplete evidence. Set `NEOFLASH_FFMPEG` (and `NEOFLASH_FFPROBE`) only for a custom override.
- **Native macOS VoiceOver (`platform-accessibility`):** Exposing the custom-painted NF2 editor UI as a full native macOS NSAccessibility / VoiceOver tree is deferred to post-Beta. Keyboard-only focus navigation (`Tab` / `Shift+Tab`, `keyboardFocusActive` focus rings) and published HTML5 `#stage-a11y-overlay` ARIA DOM (`role`, `aria-label`, `tabindex`) are supported in `0.1.0-beta.1`.
- **Experimental physical graphics-tablet hardware (`T-235`):** Deterministic event-level pressure (`0.05..=1.0`), tilt-to-nib-angle mapping, fast consecutive strokes, pen-tip/eraser tool switching, single-step Undo, and `.nflash` save/reopen round-trip are verified in `0.1.0-beta.1`, while physical Wacom and Apple Sidecar hardware certification remains an external hardware check.
- **Bounded legacy FLA, SWF and ActionScript compatibility:** Legacy binary FLA (Flash 3–CS4), XFL, SWF and ActionScript 1/2/3 support operates within the documented [compatibility matrix](COMPATIBILITY.md) and [legacy import rules](IMPORTS.md).

## Native and platform acceptance still pending

- Live native Sources editing and detached Actions feedback (`D-008`), native macOS VoiceOver (`platform-accessibility`), physical Wacom / Apple Sidecar tablet certification (`T-235`), IME, multi-monitor.
- Touch devices and Safari for the HTML5 preview; audible output and balance for audio families.

## Legacy import

- The five local binary FLA documents that still refuse; per-file private-corpus tests with rendered frames and input
  scenarios; lossless FLA round-trip.
- Evaluate Ruffle as an optional SWF playback module; HTML5 publication stays independent of SWF.
