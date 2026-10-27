# Runtime image loading and owned content

Status: accepted for the bounded runtime image profile, 2026-10-27.

## Keep image decoding and display identity in Rust

AS3 Loader/LoaderInfo and AS2 MovieClipLoader reuse the portable-resource and browser HTTP transport. Encoded PNG/JPEG bytes pass through the same bounded Rust decoder as authored image resources. Pixel buffers remain in the shared arena; the browser keeps only revision-stamped rendering copies. The initial profile excludes SWF/GIF loading, application/security domains and native Loader subclasses. File paths do not grant desktop access. Encoded images are limited to 16 MiB and dimensions to 4096; decoded images share the 128 MiB/256-record pixel arena, with at most 256 tracked load targets.

Loader is a distinct display container with one managed image child. Its normal child mutation methods reject. Replacement/unload detaches the previous AS3 content rather than silently destroying references to that Bitmap or its BitmapData; retained content can be used elsewhere. Full garbage collection is separate work. Explicit disposal, publication reset and existing arena budgets bound retained state without changing aliases behind the author's back.

AS2 loadClip replaces a non-root MovieClip's original artwork and descendants, clears its runtime drawing, stops its timeline and suppresses its original authored dispatches. It preserves the target's display identity and transforms. unloadClip leaves an empty target; it does not restore the authored placeholder. Arbitrary target-path strings and numeric levels require a later legacy-loading profile.

## Share transport and asynchronous lifecycle

A host-only byte transport seam shares URLRequest validation, embedded resources, HTTP(S)/CORS, response budgets, cancellation and generation checks with URLLoader. It does not allocate an inaccessible ByteArray for every image transfer. Loader.loadBytes snapshots its ByteArray input and completes asynchronously. LoaderInfo supplies content metadata and ordinary progress/init/complete/error events; AS3 initialization precedes completion, while MovieClipLoader's complete callback precedes init.

Every operation owns its callbacks. A newer load, close, unload or Restart invalidates obsolete completion before it can change the display list. Pending loads leave the currently committed content visible until replacement succeeds; complete historical load-transition timing remains outside this profile. Callback-triggered replacement must be checked between lifecycle events. Browser/network timing and complete historical Flash event ordering remain separate conformance work.

The loader-gallery example combines embedded Loader loading, URLLoader bytes into loadBytes, an AS2 authored placeholder replaced by image content, a decode-error probe and immediate cancellation. One control swaps, unloads and restores the gallery. The shared project runner adds one representative image/HTTP/cancellation/Restart workflow; it does not add individual acceptance cycles per method.

References: [Loader](https://airsdk.dev/reference/actionscript/3.0/flash/display/Loader.html), [LoaderInfo](https://airsdk.dev/reference/actionscript/3.0/flash/display/LoaderInfo.html), and [MovieClipLoader](https://open-flash.github.io/mirrors/as2-language-reference/MovieClipLoader.html). They define intended contracts; acceptance against the original Flash player is not implied.
