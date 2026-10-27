# E4X references, namespace names and literal compilation

Status: accepted for the bounded profile below, 2026-10-27.

## References belong to the XML tree

An XMLList selection carries its target as well as its current nodes. Assignment to an empty, uniquely rooted selection can create the selected child. A copied JavaScript array cannot provide that contract. The XML service therefore owns selector descriptors, target resolution, staged edits, parent links and publication budgets. Ambiguous multi-parent creation fails before changing any tree or list.

Compiled property reads and method calls have separate routes. `value.length` can select a child named length; `value.length()` invokes the XML method. Child, attribute, descendant and namespace selection use one URI/local-name model. Computed receivers and selectors retain their original evaluation order. Replacement/deletion and namespace serialization share the same mutable tree as explicit XML methods.

Namespace/QName values describe XML names. Namespace matching uses URI and local name, not a particular prefix spelling. This does not implement class access-control namespaces or make a default XML namespace into global publication state. Scoped defaults, filters and wider E4X equality/addition remain separate work until their actual language rules are implemented.

## Literals need a lexical context

The parser enters XML literal mode where a primary expression is expected. Ordinary comparisons, shifts, quoted ActionScript strings and project source boundaries retain their normal lexical rules. The isolated scanner returns static markup and source-positioned expression holes; the normal ActionScript parser handles each interpolation expression, including its braces and nested syntax. Static quoted XML attributes, comments, processing instructions and CDATA remain lexical XML content.

The runtime builder parses the static skeleton with the existing bounded Rust XML parser, then inserts each hole through the XML tree service. Compiler-generated thunks preserve expression and conversion order. Scalar child values become text, attribute values stay data, and XML/XMLList child values are copied as nodes with their namespaces. Unescaped string concatenation is not the construction mechanism. The initial profile supports whole unquoted attribute-value holes and child-content holes; dynamic tag/attribute/namespace declarations remain diagnosed.

## One workflow

Extend the existing XML catalog and its existing project-runner helper. The AS3 pane edits an attribute, creates an empty target, replaces/deletes selected items, builds XML/XMLList literals containing escaped text and copied subtrees, updates qualified metadata and serializes/reloads. A child named length exercises property/method separation. An ambiguous edit leaves the complete tree unchanged. The AS2 DOM pane remains a regression, and Restart recreates both models. One shared regression pass and one browser acceptance cover the delivery.

Primary references: [ECMA-357 second edition](https://ecma-international.org/wp-content/uploads/ECMA-357_2nd_edition_december_2005.pdf), [QName](https://airsdk.dev/reference/actionscript/3.0/QName.html), [Namespace](https://airsdk.dev/reference/actionscript/3.0/Namespace.html) and [XMLList](https://airsdk.dev/reference/actionscript/3.0/XMLList.html). These guide implementation; original-player and independent declaration verification remain separate evidence.
