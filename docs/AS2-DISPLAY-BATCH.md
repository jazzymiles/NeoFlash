# AS2 MovieClip depths and target paths

AS2 sparse runtime depths, replacement, lookup, depth swapping, duplication and legacy display paths use the shared Rust graph. AS3 index-based display-list operations retain their own semantics.

## Delivered behavior

- `getDepth`, `getInstanceAtDepth`, `getNextHighestDepth`, `swapDepths` use logical AS2 depths rather than child counts. Runtime depths accept integers 0–1048575; authored children have read-only negative lookup depths derived from their current authored order. A missing lookup returns `undefined`.
- `createEmptyMovieClip`, `createTextField`, `attachBitmap` and `attachMovie` accept sparse depths and replace the prior runtime child at the requested depth. Native lifecycle events carry the detached object's captured routes.
- `duplicateMovieClip` copies a runtime-created MovieClip's independent drawing state, affine transform and ColorTransform, and applies ordinary enumerable initObject data properties. Arbitrary source Object fields and dynamically added child clips do not copy, matching the referenced AVM1 clone behavior. Authored, linked and externally loaded timeline duplication remains explicitly unsupported; callers can use `attachMovie` for a fresh exported symbol instance.
- MovieClip, TextField and Button share `getDepth`, `_target` and `_url`. `_framesloaded` reports the frames available in the already-loaded publication; it is not a streaming byte-progress API. `_url` is the live browser publication URL.
- Rust resolves bounded `/holder/child`, `_root.holder.child`, `_level0.holder.child`, relative names, `../child` and `_parent.child` paths. `targetPath` produces `_level0.holder.child`; `_target` uses `/holder/child`. Paths require nonempty unambiguous instance names and return a diagnostic for unsupported names. Missing lookup returns `undefined`; proxy identity remains authoritative.
- AS2 `getBounds` and `getRect` expose the legacy `{xMin,yMin,xMax,yMax}` object and accept supported string target paths. AS2 `localToGlobal` / `globalToLocal` mutate the supplied point and return no value. Existing AS3 Rectangle and Point return values remain unchanged.

TextField `replaceSel` marshals into the existing Rust selected-range replacement, and `removeTextField` uses the existing Rust dynamic-removal operation. Authored text removal remains unsupported.

The depth table is separate from child indices. Mixing AS3 index reordering and AS2 depths in one container retains the most recent AS3 order until a subsequent AS2 depth mutation explicitly sorts the runtime suffix. Authored children remain immovable. This is useful partial compatibility, not a claim of full AVM1 depth or timeline semantics.

## Reuse and ownership

Behavior was read and adapted from [Ruffle AVM1 MovieClip](https://github.com/ruffle-rs/ruffle/blob/89a7049965f3090e706265670df309a5128233ac/core/src/avm1/globals/movie_clip.rs) at pinned revision `89a7049965f3090e706265670df309a5128233ac` (v0.7.1): `get_instance_at_depth`, `get_next_highest_depth`, `swap_depths`, `duplicate_movie_clip` and `clone_sprite`. The selected MIT notice already exists in `THIRD_PARTY_NOTICES.txt` and is included in exports. The downloaded source is ignored under `target/verification/as2-runtime-hour/display-upstream/`.

The implementation is adapted to NeoFlash's lifetime-stamped ObjectRefs, native drawing budget, placement graph and event routes. It does not embed Ruffle or a second VM. New depth/path/duplication algorithms live in `crates/neoflash-core/src/runtime_graph/as2_display.rs`; `crates/neoflash-web/src/as2_display.rs` only transports requests/results. `web/display.js` resolves host proxy references, initialization values and the browser URL. The smaller bounded depth range and explicit errors replace Ruffle's SWF-version-dependent wrapping and silent failure cases.

## Example

[as2-display-batch.nflash](../examples/as2-display-batch.nflash) combines depth mutation, replacement, duplication, target paths, bounds, independent drawings and TextField operations. It also demonstrates [uniform text, snapshots and legacy input](AS2-TEXT-DISPLAY-TWO-HOUR.md).
