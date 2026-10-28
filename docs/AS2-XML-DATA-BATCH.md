# AS2 XML metadata, namespaces and loading

AS2 XML metadata, namespace lookup and request/response support use the existing shared Rust XML parser.

## Supported operations

The partial profile includes XML `addRequestHeader`, `contentType`, `docTypeDecl`, `idMap`, `send`, `sendAndLoad` and XMLNode `getNamespaceForPrefix` / `getPrefixForNamespace`.

XML `xmlDecl` is writable. LoadVars exposes its default `onData` as a callable method with lifecycle guards; `addRequestHeader` replaces an earlier header of the same name case-insensitively.

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

## Example

[as2-xml-data-batch.nflash](../examples/as2-xml-data-batch.nflash) edits an XML tree, round-trips metadata, loads bundled XML/forms and calls the default LoadVars callback. Its **Send XML to local test endpoint** control posts XML to the optional same-origin `/__as2_xml_echo` backend and displays status/data/load results. A static file server alone does not provide that endpoint.

## Provenance

Behavioral references: Adobe's [AS2 XML reference](https://open-flash.github.io/mirrors/as2-language-reference/XML.html), [XMLNode reference](https://open-flash.github.io/mirrors/as2-language-reference/XMLNode.html), and [LoadVars reference](https://open-flash.github.io/mirrors/as2-language-reference/LoadVars.html).

Ruffle source was inspected at immutable revision [`07d3e430631333f80f754df01a968bdcd80d98c5`](https://github.com/ruffle-rs/ruffle/tree/07d3e430631333f80f754df01a968bdcd80d98c5): `core/src/avm1/globals/xml.rs` and `core/src/avm1/globals/load_vars.rs`, licensed MIT OR Apache-2.0. This informed the document metadata, duplicate-ID and default-callback contracts. No implementation was copied. In particular, an upstream stub is not treated as evidence of working behavior. Downloaded source and exploratory material remain under ignored `target/references/as2-runtime-hour/`.
