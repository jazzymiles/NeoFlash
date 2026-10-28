# AS2 MovieClip depth and target-path batch

This bounded AS2 package adds sparse runtime depths, replacement, lookup, depth swapping, runtime MovieClip duplication and legacy display paths to the shared Rust graph. AS3 index-based display-list operations keep their existing semantics. UI, FLA import and other paused product work are unchanged.

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

## Accounting

The exact catalogue inventory is produced for the coordinator in ignored `target/verification/as2-runtime-hour/display-inventory.json`. Counts are declarations, not independent algorithms:

| Change | Methods | Properties | Total |
| --- | ---: | ---: | ---: |
| Newly exposed declarations | 8 | 7 | 15 |
| Existing APIs with broader/corrected behavior | 9 | 0 | 9 |
| Existing working declarations re-reviewed | 4 | 20 | 24 |

The eight new method declarations are MovieClip `getDepth`, `getInstanceAtDepth`, `swapDepths`, `duplicateMovieClip`, the shared TextField/Button `getDepth`, and TextField `replaceSel` / `removeTextField`. The seven properties are `_target` and `_url` on all three owners, plus MovieClip `_framesloaded`. Owner aliases are not counted as independent implementations. No constructor, constant or event is added. Global `targetPath` registration is owned and counted by the coordinator, so it is excluded from this package's totals.

The existing-method improvements are the five depth-aware creation/query APIs and four AS2 coordinate/bounds corrections. Existing `clear`, `curveTo`, `removeMovieClip` and MovieClip/Button `_width`/`_height` are re-reviewed rather than claimed as new implementations. The same applies to TextField `_name`, `_visible`, `_xmouse`, `_ymouse`, `filters`, `maxChars`, `password`, `replaceText`, `restrict`, `selectable`, `textHeight`, `textWidth`, `type`, and TextFormat `font`, `size`, `bold`, `italic`. TextFormat remains uniform whole-field formatting; metrics use the existing Rust layout with browser font measurements. Input restrictions retain the existing bounded input profile. Filters are the existing copied Rust filter state and have the existing filtered-editor limits.

## Verification

One grouped native scenario passed: `cargo test -p neoflash-core --test as2_display` (1 passed). It covers sparse depths, sibling swapping, free-depth movement, replacement events, independent drawing copies, nested target paths, reset invalidation and preservation of AS3 index semantics. No independent full suite was run by this agent. After the browser cycle exposed ObjectRef key-order loss in an untyped JSON packet, the WASM binding was changed to typed serialization. `cargo check -p neoflash-web` then passed; the direct workspace-pinned serde dependency adds no new package version.

`examples/as2-display-batch.nflash` is an ordinary authored AS2 publication with 20 checks and visible independent drawings. It exercises depth mutations, replacement, duplicate init data, target paths, publication metadata, mutable coordinate points, bounds, independent graphics, TextField depth and removal, selected/range replacement, uniform TextFormat properties, input-state round trips and layout metrics. Actual HTML5 export/browser acceptance is delegated to the coordinator's shared delivery cycle; this document does not claim a browser pass before that cycle completes. Unexercised owner aliases are supported by shared source inspection, not separate browser passes.
