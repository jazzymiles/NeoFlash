# File video, streams and shared display ownership

Status: accepted for the bounded HTML5 file-playback profile, 2026-10-27.

## Display and decoding

Video is a display leaf in the existing Rust graph, with stable authored/dynamic identity, dimensions, transforms and clipping. AS3 can construct Video; AS2 uses a named authored Video placement and attachVideo. The editor offers an explicit placeholder instead of pretending to decode video in the side-effect-free authoring evaluator. Video is not an InteractiveObject; its containing interactive object owns pointer interaction.

The browser owns HTMLVideoElement decoding and temporary frame surfaces. Rust retains the scene graph and dimensions; decoded frames are not a second editable pixel store. The shared Canvas painter consumes video frames for normal rendering and supported subtree snapshots, including inherited transforms and existing clip/filter groups. Frame changes must invalidate any group cache. Native video codec support determines whether a file plays; the original fixture uses H.264/yuv420p with silent AAC.

The initial profile permits four NetStream objects per publication, decoder metadata up to 1920×1080, and 32 retained/derived frame surfaces with a combined 16 Mi-pixel budget. Authored viewport dimensions are finite 0–8192. Stream records remain counted until Restart. The renderer shares filter/color-transform processing with existing display objects; decoded frames are not inserted into the editable BitmapData arena.

## Transport and lifecycle

NetConnection.connect(null) establishes the local file-playback profile. NetStream accepts portable embedded media and validated HTTP(S) URLs. Embedded resources use Blob URLs; HTTP uses the native browser media element with CORS, so it can request and decode media progressively rather than waiting for URLLoader to buffer the entire file. Portable resource limits remain unchanged. Browser network/decode buffering is managed by the browser; a metadata dimension check does not claim to prevent all prior browser allocation.

Publication ownership guards asynchronous metadata, native playback/seek/ended events and callbacks. Replacing a source, closing a stream and Restart must suppress stale callbacks and release old requests, Blob URLs and retained surfaces. close resets the public time while retaining the displayed frame; Video.clear clears its visible image without silently changing the stream attachment. Native transition events, not merely a requested operation, drive status reports. SoundTransform supports the stated volume profile; unsupported pan/matrix behavior diagnoses. The existing gesture/mute controls cover video audio, and the fixture is silent.

RTMP/RTMFP, camera capture, publishing, DRM, Flash FLV codecs, cue points and historical buffer/mixing/seek precision remain separate work. Native playback, timeline synchronization and codec conformance are not inferred from the placeholder or the first file workflow.

## Verification

The first shared delivery pass and one ordinary browser workflow passed. Actual MP4 pixels, pause retention, seek, natural completion, Close retention, HTTP playback and fresh playback after Restart were observed in the Codex in-app browser. The small 8,709-byte clip does not prove playback before a large HTTP download completes, precise keyframe behavior, audio quality or other engines. Native insertion/Properties remains compile/model checked with live UI pending. See [the verification record](../VERIFICATION.md#file-video-and-local-stream-workflow).

One original four-second clip connects AS3-created and AS2-authored Video views, metadata, Play/Pause/Seek/Resume/Close, optional HTTP playback and Restart. A host adapter can verify command/lifetime wiring, but only the actual browser establishes decoding and changing pixels. Extend existing grouped core/compiler scenarios; perform one shared delivery pass and one browser workflow rather than per-method cycles. Native insertion/Properties acceptance remains distinct from automatic model/build checks.

Independent `VIDEO-HTML5-20261027T1042Z` on frozen f42ad94 adds actual browser state/metadata/status traces and the one-view clear observation. Pause precedes completion; negative AS3 seek preserves its old time beside a working positive seek. Close follows natural completion, so active/pending close is not independently accepted. Restart interrupts an actual AS2 HTTP play; a fresh play completes once per dialect. The short silent file supports neither audible gain/pan effects nor large progressive-download, exact-timing or codec-wide claims. Main helper evidence and this independent browser evidence remain separate. See [the independent record](../VERIFICATION.md#independent-video-verification-of-f42ad94).

References: Adobe/AIR [Video](https://airsdk.dev/reference/actionscript/3.0/flash/media/Video.html), [NetStream](https://airsdk.dev/reference/actionscript/3.0/flash/net/NetStream.html), [NetConnection](https://airsdk.dev/reference/actionscript/3.0/flash/net/NetConnection.html), Adobe [AS2 Video mirror](https://open-flash.github.io/mirrors/as2-language-reference/Video.html), and the HTML [media element standard](https://html.spec.whatwg.org/multipage/media.html#media-elements). These contracts and the browser implementation do not substitute for original Flash execution.
