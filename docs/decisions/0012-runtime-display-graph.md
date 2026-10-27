# Runtime display graph and Graphics

Status: Proposed, 2026-10-27. This document specifies a follow-up batch; it does not describe implemented dynamic display support.

## Context

Runtime-created components need display identity, ordering, drawing, events and nested clocks in the shared Rust/WASM scene. A separate JavaScript overlay would give rendering, hit testing and authoring preview different models.

The current runtime uses authored paths for both identity and ancestry. Proxy parents, bounds, lifetime checks and event routing depend on path prefixes. Reparenting by changing those paths would incorrectly recreate objects and lose listeners or state. Stable runtime identity is therefore the first dependency.

## Proposed contract

Introduce an opaque `ObjectRef { id, generation }`, serialized across WASM as decimal strings. A ref identifies a runtime object independently of its parent or child index. Authored paths remain reconciliation/source addresses and compatibility-wrapper inputs. A removed or replaced authored placement receives a new ref; reset invalidates all previous session refs.

The core owns the runtime arena, explicit parent/ordered-child relationships, transforms, visibility, graphics, metadata and clocks. Descriptors distinguish object existence from attachment to Stage. Rendering and semantics consume the same composed runtime snapshot.

```text
create_display(Sprite | Shape | EmptyMovieClip | Symbol(id)) -> ObjectRef
add_child(parent, child, index?) -> MutationResult
remove_child(parent, child) -> MutationResult
set_child_index(parent, child, index) -> MutationResult
swap_children(parent, first, second) -> MutationResult
graphics_apply(owner, commands[]) -> Result
runtime_snapshot() -> { objects, draws, semantics }
```

Mutations validate owner types, refs, cycles, indices and budgets before changing the graph. `MutationResult` contains bounded lifecycle notifications with stable targets and captured old/new ancestry. The host delivers supported ADDED/REMOVED and Stage attachment notifications through the existing event facade before returning to the caller. This defines an initial post-mutation schedule, not complete Flash construction or re-entrant event conformance.

Each draw carries its owning object ref separately from primitive identity. Several Graphics batches belong to one Sprite/Shape; they do not create display children or change `numChildren`. Pointer picking, bounds and semantic activation resolve the same owner and ancestry.

## First working scope

- Create Sprite/Shape values and empty or explicitly linked MovieClips. Dynamic objects can be attached to authored or dynamic containers, reordered, detached and reattached. Moving/removing authored timeline placements remains an explicit unsupported operation until timeline ownership rules are implemented.
- `removeChild` detaches without destroying object identity, custom fields, listeners or graphics. Detached objects produce no Stage rendering, pointer activation or semantic target. Preserve their clock state; the proposed first profile continues independent dynamic clocks on ordinary Player ticks. Reset invalidates the entire arena. Garbage-collection equivalence is not claimed.
- Keep Graphics state and validated paint/path batches in Rust. Start with clear, solid beginFill/endFill, bounded lineStyle, moveTo/lineTo/curveTo and rectangle/circle/ellipse helpers. Reuse existing path/primitive rendering and hit geometry. Unsupported gradient/bitmap fills, stroke modes, masks and filters diagnose.
- `attachMovie` resolves an explicit exported linkage identifier, never a guessed symbol name. The linkage manifest/schema and a bounded AS2 dynamic-depth table must be agreed before implementation. Initially unsupported depth collisions may diagnose; AS3 detachment must not silently stand in for AVM1 replacement semantics.
- Apply object, nesting, command and event-work limits, including detached objects retained until reset. Keep authored `evaluate(project, frame)` pure: runtime mutations do not enter saved projects, scrubbing or onion skin.

## Ownership and integration

1. **Core graph/WASM owner:** arena and authored reconciliation, refs, transactional mutations, clocks, Graphics storage, composed snapshots and Rust/WASM bindings. Existing path APIs remain compatibility wrappers where useful.
2. **Compiler/linkage owner:** Sprite/Shape constructor recognition, a native allocation hook that supplies the runtime proxy before a project constructor runs, AS2 creation calls and explicit symbol linkage resolution. Constructors must run once on the actual receiver.
3. **Browser facade/events owner:** one proxy per ref, stable Graphics wrapper, parent/children queries without path-prefix assumptions, event routes, bounds, input and semantics. The browser translates operations; it does not own a second display graph.

Publication stays self-contained HTML5. The native editor and WASM share the core graph; introducing runtime state does not serialize backend handles or execute authoring scripts during evaluation.

## Acceptance and limits

Use one coherent workflow: construct and draw a component, reparent it without losing listener identity, change order, detach/reattach, create a linked nested MovieClip, then Restart. Verify core/WASM graph invariants and atomic rejection alongside ordinary exported pointer/keyboard behavior in the shared batch. Do not add a separate acceptance cycle for each Graphics method.

References: [AS3 DisplayObjectContainer](https://airsdk.dev/reference/actionscript/3.0/flash/display/DisplayObjectContainer.html), [AS3 Graphics](https://airsdk.dev/reference/actionscript/3.0/flash/display/Graphics.html), [AS2 MovieClip](https://open-flash.github.io/mirrors/as2-language-reference/MovieClip.html). These references guide the bounded profile; this proposal is not measured Flash Player conformance.
