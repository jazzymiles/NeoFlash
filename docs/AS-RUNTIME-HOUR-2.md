# Events, filters and locale formatting

These runtime services connect the shared Rust core to the existing HTML5 display and event system:

- [Event payloads](RUNTIME-EVENTS-PROVENANCE.md) support concrete event classes, copied fields, clone/formatting and manual EventDispatcher delivery. Payload classes do not supply automatic gesture, sensor, native IME or AIR event producers.
- [Convolution, Bevel, GradientGlow and GradientBevel](RUNTIME-FILTERS-PROVENANCE.md) render vector/text/bitmap subtrees and support BitmapData filtering. Their bounded Rust pixel profile does not reproduce exact Flash rasterization.
- [Date and DateTimeFormatter](RUNTIME-DATE-PROVENANCE.md) use Rust calendar arithmetic and pattern/state handling. The browser supplies timezone resolution, legacy parsing and locale names/templates; full historical parsing and locale fidelity remain open.
- [NumberFormatter and NumberParseResult](RUNTIME-NUMBER-FORMAT-PROVENANCE.md) use Rust formatting, parsing, grouping and digit substitution. Browser Intl supplies locale metadata; locale enumeration diagnoses as unsupported.

The ordinary examples [date](../examples/runtime-date.nflash), [events](../examples/runtime-event-families.nflash), [filters](../examples/runtime-filter-extensions.nflash) and [numbers](../examples/runtime-number-format.nflash) expose visible controls and Restart behavior. They do not establish a complete browser-engine or original-player conformance matrix.

Adapted event contracts and filter algebra retain Ruffle attribution; calendar arithmetic retains Howard Hinnant's MIT attribution. Exact sources, modifications and licenses are in the linked provenance records and [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt).

Coverage decisions and declaration counts are maintained in the [complete ActionScript audit](progress/ACTIONSCRIPT.md). These profiles describe partial support, not full Flash compatibility.
