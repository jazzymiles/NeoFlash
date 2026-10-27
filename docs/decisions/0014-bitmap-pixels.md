# Shared bitmap pixels and image placements

Status: accepted for the first bitmap family increment, 2026-10-27.

## Ownership

An authored Bitmap placement references an existing portable resource by ID and records its intrinsic width/height and smoothing choice. PNG/JPEG dimensions must match bounded decoded content. The document stores no native textures, browser canvases or runtime pixel references. Version 3 remains sufficient: older readers already reject the unknown Bitmap element rather than silently discarding it. Clipboard transfer remaps resource references with the existing atomic resource snapshot.

The pure authoring evaluator still emits document elements. Runtime evaluation uses a separate draw-kind wrapper, preserving the existing vector representation while adding revision-stamped pixel-buffer draws. Mutable BitmapData belongs to the Rust runtime arena; a generation-qualified pixel reference identifies each buffer. Several Bitmap display objects can share one buffer without copying the editable data. Clones own independent buffers, disposal releases the buffer, and Restart invalidates old references.

Core operations own integer coordinates, clipping, overlapping copies, premultiplied alpha and allocation limits. Browser facades supply the existing ActionScript services and diagnostics. RGBA transfer uses typed WASM bytes only when a buffer revision changes. Canvas copies are render caches, never a competing mutable model. Unsupported drawing, filters, alpha masks, encoding and legacy linkage are diagnosed rather than approximated silently.

## Native and browser rendering

Library → Resource files → Place image inserts a PNG/JPEG placement, centered and scaled to fit 80% of the stage without enlarging its intrinsic pixels. Properties exposes smoothing; regular transforms, selection, nested symbols and onion skin use the pure scene. Removing a resource still referenced by artwork is rejected before changing history. Image preview textures are shared per resource content and sampler choice, bounded to 64 MiB and invalidated by content replacement. Missing previews have an explicit crossed outline and diagnostic.

The browser uses the same runtime display graph for Bitmap identity, placement and lifetime. Pixel snapping is a per-instance choice: NEVER preserves the transform, ALWAYS rounds translation, and AUTO snaps an unrotated unit mapping. Browser snapping operates in Canvas drawing-buffer pixels; native authoring AUTO uses its physical preview-pixel mapping. CSS preview scaling and browser/device rasterization can differ; this is not a claim of historical Flash pixel parity.

## Verification budget and boundaries

One original project combines an authored PNG, shared dynamic Bitmap views, an independent copied/scrolled buffer, AS2 attachBitmap, visible pixel changes and Restart. Its exported/WASM helper joins the existing family runner. Existing authoring/resource scenarios cover preview texture reuse and Undo. One shared regression and aggregate browser pass cover the delivery. This does not establish full BitmapData conformance, arbitrary Loader decoding, animated images, media playback or native GUI acceptance.
