# Project programs and browser API families

Status: accepted, 2026-10-27.

## Context

Reusable Flash-style components need source files, class linkage and independent placement state. Frame snippets alone cannot represent that workflow. Compatibility also needs useful groups of APIs without a separate acceptance cycle for every method.

## Decision

- Store bounded logical source units and document/symbol class linkage in the native project. Source paths resolve inside that snapshot, never by reading arbitrary paths during compilation. Version 1 documents without a program remain readable; programs use version 2.
- Compile project classes in Rust and emit a publication-owned class registry. Share that registry between linked classes and frame/object handlers. Restart recreates class/static state; each placement lifetime receives its own linked instance.
- Keep AS2 and AS3 class profiles distinct. The bounded AS2 implementation uses shared prototypes, unbound methods and erased annotations. AS3 uses typed slots and bound methods. Unsupported syntax and mixed inheritance diagnose. AS2 root document linkage is explicitly a NeoFlash extension.
- Keep authored evaluation independent of executing scripts. The shared Rust/WASM placement graph remains authoritative for runtime display identity, geometry, clocks and content. Browser facades translate supported script operations onto that graph and browser services.
- Group browser APIs by responsibility: display, events/Stage, legacy input, geometry and timers. Return the same public geometry constructors from display queries that project imports use. Session services cancel callbacks and discard listeners before program reconstruction.
- Publish the compiler output, runtime and browser modules as a self-contained HTML5 folder. No SWF or external CDN is required for new projects.
- Verify coherent workflows through ordinary exported projects and existing integration harnesses. Run the shared regression suite once per delivery batch; add focused checks only for distinct contracts or concrete failures. Keep unsupported boundaries and pending browser/native observations explicit.

## Consequences

This supports reusable authored components and broader working API families while preserving an honest partial compatibility profile. It does not provide an AVM1/AVM2 virtual machine, full native-class semantics, general JS/TS project modules or complete Flash lifecycle conformance.

Clipboard operations must transfer the linked source closure and reject conflicting logical files atomically. Future resource documents and dynamic display content must preserve the same identity, lifecycle and undo boundaries instead of introducing an unrelated browser-only scene.

The declaration catalogue remains the coverage denominator. Adding a family module does not promote every declaration in that family; individual audit decisions retain their evidence and source revision.
