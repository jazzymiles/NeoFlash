# ADR 0035: AS2 subsystem kernels and bounded browser facilities

Status: accepted for the authorized AS2 two-hour delivery, 28 October 2026.

## Context

Per-member implementation and acceptance cycles limit useful breadth. The AS2
catalogue also includes declarations that share behavior, interface surfaces,
events, and existing implementation with stale audit status. These must not be
counted as independent new algorithms or full compatibility.

## Decision

Implement coherent AS2 families through owned Rust kernels: collection/view and
delta state, binding validation, locale tables, text styles and snapshots,
pixel operations, sound metadata/matrices, variable decoding, dynamic-property
state and file payload assembly. Browser adapters retain live references and
connect browser facilities through the existing transport, display, audio and
event services. Preserve dependency order during reset and disposal.

Adapt individual compatible upstream algorithms with pinned provenance and
notices. Do not embed an external ActionScript engine or add foreign-language
runtime libraries. The existing Rust frontend continues to emit authored
program JavaScript; browser adapters do not duplicate the new Rust algorithms.

Validate families through ordinary authored HTML5 projects, one shared workflow
pass and focused reruns for concrete failures. Catalogue reviews identify new,
extended and already-existing declarations separately and retain bounded
profiles. Unsupported platform behavior produces explicit diagnostics.

## Consequences

Related declarations can gain useful partial behavior together without a new
engine or a separate browser test for every method. Integration, memory bounds
and restart lifetimes remain shared responsibilities. The catalogue still
contains unsupported behavior; partial is not a full AS2 conformance claim.
AS3 feature work, editor UI and FLA remain outside this delivery.
