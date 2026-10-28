# AS2 platform and component events

This AS2-only batch adds a browser publication profile for `System.capabilities`, `System`, `System.security`, IME composition listeners, lock-key queries, `mx.utils.Delegate`, `mx.events.EventDispatcher`, `getVersion`, `updateAfterEvent` and `stopAllSounds`. It does not modify AS3 classes, the native editor or FLA import. The profile reports NeoFlash's facilities and browser facts; it does not identify itself as an Adobe or Macromedia player.

## Implementation and reuse

`crates/neoflash-core/src/as2_platform.rs` owns capability policy, locale normalization, URL query serialization and lock-key state. `crates/neoflash-web/src/as2_platform.rs` exposes that state to WASM. `web/as2-platform.js` gathers browser facts and binds object references, callbacks and browser composition events. Existing `ScriptBroadcaster` Rust state supplies ordered listener identity, duplicate handling, removal and bounds for both IME and AS2 component events. No additional event engine, timer loop or foreign-language runtime is introduced.

The separation of player policy from host facts and the capability server-query contract were adapted after reading Ruffle's MIT-licensed [`core/src/avm1/globals/system_capabilities.rs`](https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/avm1/globals/system_capabilities.rs) and [`core/src/system_properties.rs`](https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/system_properties.rs), pinned at `89a7049965f3090e706265670df309a5128233ac`. NeoFlash uses its existing serde types and bounded owned state; Ruffle's VM, bitflags, GC objects and player impersonation are not imported. Query values are encoded once, and the query agrees with the public capability snapshot. The existing Ruffle MIT notice in `THIRD_PARTY_NOTICES.txt` applies and is shipped by the standard HTML5 exporter. Downloaded reference files and hashes stay under ignored `target/verification/as2-runtime-hour/platform-sources/`.

Public contracts also follow the [Adobe AS2 capability reference](https://open-flash.github.io/mirrors/as2-language-reference/System/capabilities.html), [Key reference](https://open-flash.github.io/mirrors/as2-language-reference/Key.html) and [IME reference](https://open-flash.github.io/mirrors/as2-language-reference/System/IME.html). Delegate's receiver binding and EventDispatcher's plain-object event bridge are authored host bindings over existing shared runtime objects; no ActionScript library implementation is embedded.

## Delivered profile

The 26 read-only capability properties form a publication snapshot. Audio/MP3/video/printing/composition flags reflect browser facilities available to this export; unavailable encoders, embedded SWF video, screen codecs and AS2 accessibility are reported false. `avHardwareDisable` and `localFileReadDisable` reflect restrictions of this AS2 host, not OS administrator settings. `hasIME` reports browser composition-event support, not detection of an installed OS IME. Screen size and DPI use CSS coordinates and the CSS 96-DPI convention, not physical monitor measurements. Language comes from the browser locale; OS comes from its platform hint. Manufacturer is `NeoFlash`, version begins `NEO`, and player type is the explicit extension `NeoFlashHTML5`. Authored code that requires historical Flash player identifiers needs adaptation.

`System.exactSettings` supports the fixed exact-origin profile (`true`), and `System.useCodepage` supports Unicode (`false`). Assigning the supported value is accepted; changing the policy throws an explanatory error. `System.security.sandboxType` reports `remote` for a web publication or `localWithNetwork` for a file publication; actual authority always remains the browser's origin and permission model. Domain-policy bypass, Flash policy files, player settings panels, synchronous clipboard writes and OS codepage switching are unavailable and explicitly diagnosed. These unsupported methods are **not counted** as partial support.

The ordinary success example checks the supported property values and assignments. Unsupported setters were reviewed in source and are deliberately excluded from that scenario: the current AS2 catch profile handles authored throws, while unsupported native-operation diagnostics terminate the script instead of entering an authored catch block. No exception-runtime change is part of this package.

`System.IME.addListener/removeListener` binds listener objects to browser `compositionend` delivery and passes the committed text to `onIMEComposition`. Eight documented IME constant bindings are available. The browser cannot inspect or control OS conversion mode, enable state or composition replacement, so those six control/query methods throw actionable diagnostics and remain missing. Up to 1,024 listener objects are supported. Restart removes all prior registrations.

`Key._listeners` returns a frozen snapshot. `Key.isAccessible()` is true within this single publication; cross-SWF domain access is not modeled. `Key.isToggled` reads Rust state for CapsLock/NumLock supplied by observed browser keyboard modifier flags; it returns false before the first observation and for other key codes. The capture observer does not dispatch additional keyboard events or alter AS3 KeyboardEvent behavior.

The AS2 global `getVersion()` returns the same honest NeoFlash version as the Rust capability profile. `updateAfterEvent()` calls the publication's existing render-request facility; it does not add a scheduling loop or change timeline position. The profile retains the existing browser render coalescing and scheduling behavior.

The AS2 global `stopAllSounds()` cancels every active or pending channel owned by this publication's AS2 Sound instances using the existing channel stop path. Its owner references are attached to existing sound records; no second mixer, channel registry or scheduling loop is introduced. Loaded buffers remain reusable, and later authored `start()` calls can play them again. This bounded profile does not stop AS3 Sound instances, NetStream audio or unimplemented timeline/library sounds. Sound matrices, ID3 parsing and Flash policy-file handling were inspected and remain unclaimed: the existing volume/pan adapter has no channel-matrix renderer, and fabricating ID3 fields would not implement metadata extraction.

`Delegate.create(receiver, method)` retains the receiver and function reference, forwards invocation arguments and returns the callback result. This profile requires an object/function receiver and a function method. Old delegates fail explicitly after publication Restart. Construction of the static helper itself is unsupported.

`EventDispatcher.initialize(target)` installs `addEventListener`, `removeEventListener` and `dispatchEvent` on an ordinary object. Events require a string `type`; missing `target` is set to the dispatcher. Listeners can be functions or objects whose method matches the event type. Duplicate registration is idempotent. Method lookup occurs at dispatch time, and Rust controls the ordered listener IDs. Function listeners use the dispatcher as receiver; object listeners retain their own receiver. Reinitialization clears registrations. Limits are 1,024 initialized targets, 128 event types per target, 1,024 listeners per type, and event names from 1 through 256 characters. This manual component dispatcher does not implement UI components, bubbling/capture, historical prototype flags, cloning or native subclass construction.

## Counts and evidence

| Category | New | Already implemented, stale catalogue |
| --- | ---: | ---: |
| Methods in the declaration catalogue | 8 | 6 |
| Global functions | 3 | 1 |
| Properties in the declaration catalogue | 30 | 0 |
| Constants | 8 | 15 |
| Event names | 1 | 4 |
| Static namespace/class entries | 6 | 0 |
| Constructors | 0 | 0 |

The new method/function/property subtotal is **41**. `EventDispatcher.initialize` is one additional working method absent from this catalogue and is reported separately. The 26 existing entries are four removal/Stage-registration methods, primitive Number/Boolean `valueOf` methods, the `getTimer` global, 15 Key constants and four Mouse/Stage event names found in the baseline implementation. They are catalogue corrections, not newly written functionality. Primitive `valueOf` uses the pre-existing host primitive dispatch, with boxed constructors still unsupported. The six class entries describe accessible static namespaces, not six constructible classes. No language syntax is added or existing AS3 implementation converted.

`examples/as2-platform-batch.nflash` is one ordinary AS2 source project with eight grouped assertions, a repeat button, keyboard status and visible committed IME text. It reads every capability property and covers settings boundaries, global utilities, primitive values, Delegate binding, component event registration/late lookup/removal/reinitialization, IME registration and singleton input lifecycle. The single new Rust test is `as2_platform::tests::as2_platform_family`; it passed once. JavaScript syntax checks passed. The coordinating delivery report records the ordinary HTML5 export, actual browser execution and shared regression outcome; authored assertions alone are not proof that browser acceptance passed.

The same source scenario invokes individual/global stop on two unloaded, zero-volume Sound objects without creating audio playback. Active-channel cancellation uses the pre-existing tested channel-stop implementation, but this package does not claim a new audible browser playback check.

The machine-readable entry inventory, previous statuses and pinned source hashes are kept under ignored `target/verification/as2-runtime-hour/platform-inventory.json` for the coordinating audit. Source files, this document and the authored example are the intended committed package.
