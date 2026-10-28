# AS2 XML and data batch

This bounded AS2-only delivery adds XML metadata, namespace lookup and request/response support. AS3 continues to use the unchanged shared XML parser. UI and FLA work remain paused.

## Implementation and accounting

Eight previously absent declarations receive working partial implementations: XML `addRequestHeader`, `contentType`, `docTypeDecl`, `idMap`, `send`, `sendAndLoad`, and XMLNode `getNamespaceForPrefix` / `getPrefixForNamespace` (five methods and three properties).

Three existing declarations are extended: XML `xmlDecl` becomes writable; LoadVars exposes its default `onData` as a callable method, including lifecycle guards; LoadVars `addRequestHeader` replaces an earlier header of the same name case-insensitively. These are improvements to existing behavior, not three entirely new APIs.

Seventeen stale catalogue entries are independently identified as existing code, not credited as new implementation: XML `createElement`, `createTextNode`, `parseXML`, `onData`, `onHTTPStatus`; LoadVars `contentType`, `onHTTPStatus`, `send`, `toString`; XMLNode constructor, `hasChildNodes`, `insertBefore`, `localName`, `namespaceURI`, `prefix`, `nextSibling`, `previousSibling`. The exact IDs, prior status, kind and attribution are recorded in the delivery inventory and transferred into the main catalogue by the coordinator.

## Rust and browser responsibilities

`crates/neoflash-core/src/as2_xml.rs` adds an AS2 wrapper around the existing Rust XML parser. It preserves a single inert DOCTYPE in the prolog, builds the parsed ID index with the last duplicate ID winning, and resolves namespace prefixes using the current ancestor declarations. `quick-xml` remains the existing pinned parser dependency; no new parser or complete third-party engine is embedded.

The browser host marshals the Rust node indices back to existing XMLNode wrappers, preserves declarations during serialization, and connects XML requests to the existing URLLoader/fetch transport. AS2 XML `sendAndLoad` sends an actual XML body, permitted custom headers and the selected content type, then dispatches HTTP status, raw data and load callbacks on the receiving XML object. Header marshalling and callback wiring remain browser adapter work.

## Explicit partial boundaries

- Input remains bounded UTF-8 XML 1.0: 1 MiB input, shared depth/node/string budgets. Malformed XML still diagnoses or sets the existing AS2 parse status. A failed `parseXML` keeps the prior tree, an intentional existing deviation from historical Flash clearing behavior.
- DOCTYPE is retained as text only. DTD validation, external subsets, external entities and custom entity expansion are not implemented or fetched. Duplicate/late DOCTYPE declarations diagnose. Assigning `xmlDecl` or `docTypeDecl` stores bounded text; it does not execute or validate a DTD.
- `idMap` is a parse snapshot, not a live index of subsequent attribute edits or DOM moves. The last parsed element with an identical `id` wins.
- XMLNode construction/name mutation supports unqualified element/text names. Namespace lookups honor current ancestry and prefix shadowing, including the built-in `xml` prefix. Namespace mutation and every historical SWF-version quirk are outside this package.
- XML `sendAndLoad` uses browser HTTP(S), ordinary CORS, GET/POST and the existing response budgets. It does not emulate Flash crossdomain.xml permissions or forbidden browser headers. A target must be an XML document.
- XML `send` provides GET browser navigation. Raw XML POST navigation cannot be represented by the existing HTML form navigator and raises an actionable diagnostic; use `sendAndLoad` for XML POST/replies. This API is partial, not a silent fire-and-forget replacement.
- LoadVars preserves the existing bounded primitive form encoding and repeated-key arrays. Its custom `onData` override still controls whether parsing and `onLoad` occur. Existing missing-target `send` behavior is retained.

## Verification budget and ordinary HTML5 project

One grouped native test, `as2_xml_document_metadata_batch`, covers declarations, inert DTDs, ID selection, namespace shadowing and unchanged AS3 DOCTYPE rejection. It passed once: 1 passed, 14 filtered out. JavaScript syntax checks passed for `web/xml.js` and `web/net.js`.

`examples/as2-xml-data-batch.nflash` is a normal AS2 authoring project. One workflow edits an XML tree, round-trips metadata, loads bundled XML/forms, calls the default LoadVars callback and reports its result on the stage. A visible **Send XML to local test endpoint** control invokes `postXML('/__as2_xml_echo')` for the coordinator's same-origin fixture server. It sends `text/xml; charset=UTF-8`, `X-NeoFlash-Batch: xml-data`, `X-NeoFlash-Language: AS2`, and an XML request body. HTTP status/data/load results and the parsed response are visible on the stage.

The HTML5 export and browser acceptance belong to the single shared delivery pass. This document does not claim that acceptance before the coordinator records it. No new standalone JavaScript harness or per-method acceptance loop is added.

## Provenance

Behavioral references: Adobe's [AS2 XML reference](https://open-flash.github.io/mirrors/as2-language-reference/XML.html), [XMLNode reference](https://open-flash.github.io/mirrors/as2-language-reference/XMLNode.html), and [LoadVars reference](https://open-flash.github.io/mirrors/as2-language-reference/LoadVars.html).

Ruffle source was inspected at immutable revision [`07d3e430631333f80f754df01a968bdcd80d98c5`](https://github.com/ruffle-rs/ruffle/tree/07d3e430631333f80f754df01a968bdcd80d98c5): `core/src/avm1/globals/xml.rs` and `core/src/avm1/globals/load_vars.rs`, licensed MIT OR Apache-2.0. This informed the document metadata, duplicate-ID and default-callback contracts. No implementation was copied. In particular, an upstream stub is not treated as evidence of working behavior. Downloaded source and exploratory material remain under ignored `target/references/as2-runtime-hour/`.

## Additional existing-object review

A bounded review found six further stale declarations with existing implementations: Object constructor, `hasOwnProperty`, `valueOf`, `isPrototypeOf`, `prototype`, and Array `CASEINSENSITIVE`. The existing compiler adapters in `intrinsics.js`, `containers.js` and `collections.js` support a plain-record pipeline and case-insensitive `sortOn`. Two compact checks extend the same authored XML/data workflow; no separate test harness was created. These six entries are catalogue corrections, not newly written methods.

The total reviewed-existing count for this package is therefore **23**: ten methods, seven properties, three events, two constructors and one constant. Exact machine counts in the delivery inventory are authoritative. Function `call` and `apply` were reviewed but already partial and receive no status upgrade.

Object `watch`, `unwatch`, `addProperty`, `registerClass`, `__resolve` and the AS2 spelling `isPropertyEnumerable` lack corresponding implementation. `__proto__` remains inconclusive: native JavaScript reachability alone does not prove historical AS2 behavior. `constructor` is also excluded because a plain record currently exposes native Object rather than the AS2 facade. Primitive boxing and prototype/version quirks are outside the existing partial profile.

The ordinary export succeeded after correcting the example's required keyframe schema fields (`label` and `tween`). This is an exported project, with browser acceptance still owned by the coordinator's shared pass.
