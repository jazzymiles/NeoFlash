# 0006 — Structured ActionScript control flow and exceptions

Date: 2026-10-27. Status: accepted for the documented subset.

## Problem

Interactive handlers need state-machine branches, recoverable failures and cleanup. Accepting a keyword while losing its scope, evaluation order or abrupt completion would silently change the project. JavaScript also permits arbitrary thrown values: the previous HTML5 handler read `error.message` directly, losing strings and causing a secondary failure for null/undefined.

## Decision

Keep the Rust parser/AST/scope/emitter pipeline and its per-handler compile API. Add switch, do/while, AS3 labels and synchronous throw/try/catch/finally together with enclosing jump validation, lexical catch environments, selected typed dispatch and conservative function completion analysis. Catch bindings do not become persistent instance fields. Variables declared inside a catch retain their containing handler/function declaration owner.

Emit native JavaScript control constructs after validation. Do not wrap try bodies in a function: that would change return, break, continue and receiver behavior. Multi-catch uses one generated catch and ordered predicates, with a lexical binding for each selected clause and an unchanged rethrow when no clause matches. Return coercion occurs at the return expression, before finally executes.

AS3 throws retain their original values. AS2 explicit throws use an object with own data properties `[Symbol.for('neoflash.as2.thrown.v1')]: true` and `value: payload`. This allows separately compiled AS2 handlers to share explicit thrown values while internal JavaScript/host failures bypass AS2 catches and still execute finally. Unmatched clauses rethrow the envelope; author code sees the original payload. The symbol is a compatibility convention, not a security boundary. General cross-language exception interoperability and native Flash error codes remain separate work.

At the HTML5 failure boundary, read envelope descriptors defensively, unwrap one transport layer, retain the original payload in `Error.cause`, and include the script location. Guard stack/message access and string conversion so reporting cannot replace an author's failure with a secondary getter/conversion exception. This does not normalize values while a user catch is running.

Only the explicitly listed core error constructors/types are accepted. A closed facade is not an implementation of arbitrary constructors, custom class identity, all Flash errors or Flash stack traces. AS2 ambiguous catch-name scope/closures and unsupported catch types produce diagnostics. The authoritative accepted surface and source discrepancies belong in [COMPATIBILITY.md](../COMPATIBILITY.md).

## Consequences and evidence

Executable fixtures check evaluation order, fallthrough, labels, throw identity, catch scope, return coercion and finally precedence. Host tests execute the shipped proxy/dispatch implementation, including hostile errors. The original recovery example additionally runs through the Rust compiler and actual WASM Player, exercising independent instance state, nested clocks under a stopped scene, activation, cross-handler throws and reset.

These establish NeoFlash behavior; they are not an Adobe Player differential conformance suite. Native editor authoring, imported ActionScript records and full language compatibility retain separate acceptance criteria. Classes/packages require project source units, type resolution and linkage; they must not be added as unsupported source pasted into today's handler wrapper.
