# BitmapData image processing and source snapshots

Status: accepted for the bounded BitmapData image pipeline, 2026-10-27.

## One pixel owner

BitmapData remains a session-owned Rust premultiplied RGBA arena. Bulk ARGB transfer, color transforms, shared filter kernels and PNG encoding operate on that arena; JavaScript facades carry references and publication lifetime. ByteArray transfer follows each operation's cursor contract. In particular, setPixels underrun preserves already written complete ARGB words and their consumed cursor, then reports EOF. Parameter/refusal checks precede mutation; an input underrun is a documented partial write rather than a transaction rollback.

Drawing captures the source before changing the destination, including aliases of the same BitmapData. Raster surfaces are temporary rendering results, never an independently editable bitmap model. The existing arena budgets, integer color conversion and disposal rules remain in force. Draw/filter staging is bounded to 4 Mi pixels; PNG output to 16 MiB. getPixels creates a big-endian ByteArray positioned at its end; other bulk transfers use the supplied buffer endian and position. PNG encode creates a new buffer at position 0 or writes/advances an existing buffer at its current position; the latter is an explicit profile rather than an original-player cursor claim.

## Reuse the scene painter

A DisplayObject source needs a read-only normalized snapshot of its current subtree, including detached runtime-created content. Its own matrix, color transform, alpha and visible flag are excluded; descendants retain local transforms, color and visibility. Constructing this snapshot must not reparent content, change visibility, advance timelines or dispatch scripts. Root scroll clipping and masks within the captured subtree remain active. Dependencies on mask objects outside the subtree are diagnosed. Display-object filters throughout the capture are bypassed following the pinned offscreen path; BitmapData.applyFilter pixels are already baked into their source and remain visible. No original Flash differential is inferred.

The normal stage renderer and offscreen capture use the same primitive/group painter. This keeps text, gradients, bitmap views and clips on one implementation path, while ordinary stage rendering continues to apply its filter groups. A destination matrix, color transform and clip rectangle are draw-call inputs; normal blending is the initial profile. The clip rectangle is in destination coordinates according to the pinned implementation reference. Unsupported quality/blend/native source options must diagnose before destination pixels change.

## One integrated workflow

One original publication connects source pixels, independent color/filter edits, subtree capture, bulk transfer, PNG bytes and the existing Loader. AS2 covers its common bitmap operations; the ByteArray/PNG API belongs to AS3. Reuse existing grouped pixel/compiler checks and one helper. A recording or injected Canvas adapter is only wiring evidence; actual source-subtree pixels have the separate browser workflow in VERIFICATION.md. Native authoring and original Flash differential remain separate acceptance.

Independent `IMAGE-PIPELINE-HTML5-20261027T0943Z` on 6f4f66a verifies the complete initial source/tint oracle, complete captured/roundtrip/loaded RGBA equality, bulk cursor/word agreement and the partial setPixels EOF contract. The initial color refusal is generic Error; this does not claim all historical error classes. AS2 evidence uses the AS3-created shared BitmapData facade. Later palette/Restart phases retain screenshots and AX, but unavailable later logs do not prove repeated complete byte streams or a zero-error run. See [the independent record](../VERIFICATION.md#independent-image-pipeline-verification-of-6f4f66a) for exact boundaries.

References: Adobe/AIR [BitmapData](https://airsdk.dev/reference/actionscript/3.0/flash/display/BitmapData.html), [PNGEncoderOptions](https://airsdk.dev/reference/actionscript/3.0/flash/display/PNGEncoderOptions.html), and the Adobe AS2 reference mirror [BitmapData](https://open-flash.github.io/mirrors/as2-language-reference/flash/display/BitmapData.html). Pinned Ruffle `fe6cfc545405b888ebb478ce1c211d63147ca49b` provides an implementation reference for [draw and offscreen coordinates](https://github.com/ruffle-rs/ruffle/blob/fe6cfc545405b888ebb478ce1c211d63147ca49b/core/src/bitmap/operations.rs#L1375). These sources do not substitute for original-player measurements.
