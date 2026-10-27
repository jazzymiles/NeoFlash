# Date, regular expressions and JSON data processing

Status: accepted first partial profile, 2026-10-27.

## Captured language families

The Rust ActionScript frontend resolves Date in both dialects, and RegExp/JSON in AS3, to lexically captured adapters. They are embedded with the existing Array/String helpers in ordinary handlers and project methods. JavaScript/TypeScript globals and prototypes are unchanged. Local bindings and user classes retain resolution precedence; unsupported native inheritance is explicit. Values must survive transfer between separately compiled handlers and project methods, so Date uses its native internal brand and RegExp uses a shared portable identity strategy rather than incompatible per-handler WeakMaps.

Date delegates clock storage and calendar calculations to the browser Date backend. Numeric/current/component construction, UTC/local fields and mutations support ordinary web workflows. System timezones and browser date parsing/formatting are explicitly a modern backend profile, not a historical Flash locale/DST oracle. AS3 Date JSON conversion uses its string form instead of the JavaScript ISO default.

RegExp starts with constructor patterns and the AS3 exec/test and String matching/replacement/splitting surface. Regex literals need a separate context-aware slash lexer and remain diagnosed. The browser engine is a documented partial backend; supported flags and syntax are checked by the adapter. String operations preserve the earlier primitive-pattern path. Size/output/iteration limits do not establish a hard execution-time bound for native backtracking. No separate regular-expression engine is introduced in this increment.

## JSON and class metadata

JSON.parse uses the captured native parser after an input/depth/size scan, then walks reviver callbacks with original holders. JSON.stringify has its own bounded traversal so project-class public fields and getters are included without changing their ordinary enumerability. The program registry supplies readable public keys; dynamic own properties remain visible. Getters and callbacks run in their ordinary receiver context, toJSON precedes the replacer, cyclic ancestry fails explicitly, and repeated acyclic references remain allowed. Unknown native objects need their own supported toJSON or a replacer.

The profile bounds text to 1,000,000 UTF-16 units, nesting to 128 containers and traversal to 100,000 values. Reviver deletion preserves array holes; object writes use data descriptors so a JSON __proto__ key cannot invoke a prototype setter. The serializer streams bounded chunks and applies the same limits after callbacks. Non-String/Number/null indentation values remain unsupported: published reference text and the inspected avmplus implementation disagree on their conversion. No full error-message, property-order, locale or numeric-format conformance is inferred.

## Combined evidence

One original embedded-data publication loads JSON records, extracts codes with RegExp, computes UTC due dates, renders text, serializes a class including its getter, and advances an independent AS2 Date. Restart repeats the pipeline. Broader callback/type/identity branches extend existing compiler/project scenarios. One shared delivery pass plus that browser workflow verifies the family; no per-method test matrix is added.

Primary references: [AS3 Date](https://airsdk.dev/reference/actionscript/3.0/Date.html), [AS2 Date](https://open-flash.github.io/mirrors/as2-language-reference/Date.html), [RegExp](https://airsdk.dev/reference/actionscript/3.0/RegExp.html), [JSON](https://airsdk.dev/reference/actionscript/3.0/JSON.html), and the [avmplus JSON implementation](https://github.com/adobe/avmplus/blob/master/core/JSON.as). These are documented expectations; original Flash execution is a separate evidence requirement.
