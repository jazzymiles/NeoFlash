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

## Native and platform acceptance still pending

- Live native Sources editing and detached Actions feedback (`D-008`), VoiceOver, IME, multi-monitor.
- Touch devices and Safari for the HTML5 preview; audible output and balance for audio families.

## Legacy import

- The five local binary FLA documents that still refuse; per-file private-corpus tests with rendered frames and input
  scenarios; lossless FLA round-trip.
- Evaluate Ruffle as an optional SWF playback module; HTML5 publication stays independent of SWF.
