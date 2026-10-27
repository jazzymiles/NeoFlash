# Shared XML parsing with separate language facades

Status: accepted for the bounded implementation, 2026-10-27.

## Boundary

XML data belongs to a script publication, independently of the authored document model and XFL importer. A pure Rust parser produces a bounded flat transfer tree. The HTML5 service owns the mutable tree, canonical wrappers, language facades, loader callbacks and publication lifetime. This avoids a live browser DOM dependency and per-method WASM calls while keeping validation shared with the Rust application.

AS2 XML/XMLNode expose the legacy DOM model. AS3 XML/XMLList expose a distinct value and collection model. Sharing one public API would lose their different insertion, lookup and conversion rules. AS2 append/insert moves a node; the initial AS3 insertion profile explicitly copies XML input. XML strings and text insertions remain distinct operations.

The parser accepts fragments so the facade can enforce XML or XMLList cardinality. Its flat nodes preserve ordered children, expanded names, namespace declarations, attributes, comments, processing instructions and declaration metadata. CDATA becomes text. Predefined and valid numeric references decode; DOCTYPE/custom entities and non-UTF-8 declarations diagnose. There is no external entity, schema or filesystem/network resolution in the parser. Size, depth, attribute and aggregate string budgets bound work before a partial tree escapes.

## Publication service

One hidden `__nfXML` binding connects handlers and project methods to the same service and static XML settings. Original-key container routing recognizes XML/XMLList before ordinary objects, providing list indices and bounded enumeration. Implicit E4X child writes, additions and equality must not silently become JavaScript expando/identity behavior. Explicit String conversion and explicit tree methods provide a useful data workflow while the wider E4X frontend remains unfinished.

Namespace declarations and expanded names survive serialization and reparsing. String selectors use the documented bounded empty-namespace/wildcard profile; namespace mutation, QName/Namespace values and default XML namespace syntax require a later coherent increment. Logical content preservation does not imply preservation of original quote style, entity spelling or formatting.

Legacy XML.load uses the existing publication-owned URLLoader transport. Callback ownership, cancellation, previous-session invalidation and default parsing stay in the XML facade. AS3 authors use URLLoader directly. Parser failures, unsupported constructs and resource limits remain distinguishable. Strict AS2 status/recovery is an explicit partial profile, not a complete historical parser model.

## Verification cost

One grouped Rust parser scenario covers the transfer boundary. Existing compiler cases cover syntax/dispatch changes. One ordinary catalog publication connects both dialects, resource loading, copied versus moved branches, serialization, typed handler/program transfer, shared settings and Restart. The existing shared suite runs once for the delivery; no per-member browser pages or growing test matrix are required.

Primary contracts: [AS3 XML](https://airsdk.dev/reference/actionscript/3.0/XML.html), [XMLList](https://airsdk.dev/reference/actionscript/3.0/XMLList.html), [AS2 XML reference](https://open-flash.github.io/mirrors/as2-language-reference/XML.html), and [quick-xml 0.41 reader configuration](https://docs.rs/quick-xml/0.41.0/quick_xml/reader/struct.Config.html). These are reference evidence; original Flash comparison remains separate.

Delivery: the shared suite and ordinary HTML5 catalog workflow passed. See [VERIFICATION.md](../VERIFICATION.md#xml-parser-dom-and-xmllist-batch) for source/artifact fingerprints, visible interactions and limits.
