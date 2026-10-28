# Rust String and geometry profiles

NeoFlash uses individual licensed algorithms in its shared Rust core for AS2 strings and AS3 geometry. The existing ActionScript frontend compiles authored programs to JavaScript; browser adapters connect those programs to Rust services without embedding a third-party ActionScript engine.

AS2 provides UTF-16 String operations, historical case tables, escape/unescape and legacy character, length and substring globals. AS3 provides bounded Vector3D, Matrix3D and Utils3D operations, including composition, inversion, rotation, scalar/bulk transforms and projection. Projection produces coordinates for the existing 2D scene; it does not provide a 3D display renderer.

The ordinary examples [reuse-as2.nflash](../examples/reuse-as2.nflash) and [reuse-as3.nflash](../examples/reuse-as3.nflash) demonstrate text processing and visible geometry updates. Original-player equivalence, complete coercion/prototype behavior and full 3D rendering are outside these profiles.

[AS2 provenance and limits](AS2-REUSE-PROVENANCE.md), [AS3 provenance and limits](AS3-REUSE-PROVENANCE.md) and the [geometry member inventory](../scripts/reuse-as3-inventory.json) retain pinned revisions, adapted source paths and modifications. Ruffle and OpenFL attribution and selected MIT notices remain in [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt) and accompany HTML5 exports.

Coverage decisions and declaration counts are maintained in the [complete ActionScript audit](progress/ACTIONSCRIPT.md). These profiles describe partial support, not full Flash compatibility.
