# 0005 — clipboard data and recipient ownership

Status: accepted and implemented for object transfer, 2026-10-26. Native clipboard and shortcut limitations are recorded in CLIPBOARD.md and VERIFICATION.md.

The authoring clipboard belongs in the shared Rust core as a bounded versioned payload, separate from the saved Project. It contains ordered sampled placements, their complete reachable library closure, a placement anchor and a live source-document token. The payload carries scripts as data; parsing never executes scripts or resolves filesystem paths. A recognizable text prefix permits native clipboard transport without a proprietary native runtime.

Within a live document session, existing symbol IDs denote current library definitions, even after edits made since Copy. Missing captured definitions can be recovered if their IDs are free. A different session imports the captured closure with fresh IDs and consistent mappings for symbols, layers and repeated placement identities across keys. Shared references remain shared. Fresh top-level placement IDs distinguish runtime lifetimes. Symbol names alone never establish identity, and script source is not rewritten heuristically.

Paste constructs a complete candidate Project, checks the destination layer/frame, materializes the sampled key, appends the ordered placements and validates the whole graph before returning. The editor commits the result and its library additions as one history transaction. Invalid input cannot partially modify the destination. Clipboard limits supplement the model's validation rather than bypassing it.

Object paste targets the active layer. Normal paste centers the copied bounds in the visible stage; Paste in Place applies zero translation. Copy retains an anchor measured with the editor's existing geometry and font model, with placement positions as a fallback for empty instances. Frame-range and layer-preserving clipboard operations require their own explicit data model.

Text and object recipients must remain distinct. The editor captures a focused text widget's document/symbol/frame/selection context before Edit opens. Its command is delivered on the next pass after restoring focus; intervening user input or changed scope cancels it. Playback stops before text delivery. Native keyboard Copy/Cut/Paste events remain available to the focused TextEdit.

Menu Paste uses the pinned arboard 3.7.0 native text read on the UI thread; this dependency was already present through eframe. An immediate result distinguishes empty/error outcomes and avoids attaching a stale mode or target to an unrelated asynchronous paste event. Writes use egui's normal CopyText output. Tests inject the menu read function and never read or overwrite the user's system clipboard.

Verification covers structure, shared runtime behavior after transfer, serialization, atomic errors, grouped history, real egui menu/input routing and live platform checks as separate evidence. Untagged native paste events cannot prove the original Shift modifier after every batched key-release order; the menu command is deterministic and a native key-press adapter remains planned.
