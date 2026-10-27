# Runtime display graph and Graphics

Status: accepted for the dynamic graph and explicit linked MovieClip increments, 2026-10-27. Historical depth replacement remains future work.

## Context

Runtime-created components need display identity, ordering, drawing, events and nested clocks in the shared Rust/WASM scene. A separate JavaScript overlay would give rendering, hit testing and authoring preview different models.

The preceding runtime used authored paths for both identity and ancestry. Proxy parents, bounds, lifetime checks and event routing depend on path prefixes. Reparenting by changing those paths would incorrectly recreate objects and lose listeners or state. Stable runtime identity is therefore the first dependency.

## Contract

Introduce an opaque `ObjectRef { id, generation }`, serialized across WASM as decimal strings. A ref identifies a runtime object independently of its parent or child index. Authored paths remain reconciliation/source addresses and compatibility-wrapper inputs. A removed or replaced authored placement receives a new ref; reset invalidates all previous session refs.

The core owns the runtime arena, explicit parent/ordered-child relationships, transforms, visibility, graphics, metadata and clocks. Descriptors distinguish object existence from attachment to Stage. Rendering and semantics consume the same composed runtime snapshot.

```text
create_display(Sprite | Shape | EmptyMovieClip) -> ObjectRef
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

- Create Sprite/Shape values and empty MovieClips. Dynamic objects can be attached to authored or dynamic containers, reordered, detached and reattached. Moving/removing authored timeline placements remains an explicit unsupported operation until timeline ownership rules are implemented.
- `removeChild` detaches without destroying object identity, custom fields, listeners or graphics. Detached objects produce no Stage rendering, pointer activation or semantic target. Empty dynamic MovieClips retain their one-frame playback state; linked multi-frame dynamic clocks are deferred. Reset invalidates the entire arena. Garbage-collection equivalence is not claimed.
- Keep Graphics state and validated paint/path batches in Rust. Start with clear, solid beginFill/endFill, bounded lineStyle, moveTo/lineTo/curveTo and rectangle/circle/ellipse helpers. Reuse existing path/primitive rendering and hit geometry. Unsupported gradient/bitmap fills, stroke modes, masks and filters diagnose.
- Authored children form a stable ordered prefix; dynamic children occupy the ordered suffix. Crossing that prefix through mutation diagnoses. Retiring an authored parent detaches its dynamic subtrees without assigning them to a replacement. AS2 createEmptyMovieClip supports append depth only; attachMovie and replacement-depth semantics remain unsupported until explicit linkage and namespaced clocks are implemented.
- Apply object, nesting, command and event-work limits, including detached objects retained until reset. Keep authored `evaluate(project, frame)` pure: runtime mutations do not enter saved projects, scrubbing or onion skin.

## Ownership and integration

1. **Core graph/WASM owner:** arena and authored reconciliation, refs, transactional mutations, clocks, Graphics storage, composed snapshots and Rust/WASM bindings. Existing path APIs remain compatibility wrappers where useful.
2. **Compiler/linkage owner:** Sprite/Shape constructor recognition, a native allocation hook that supplies the runtime proxy before a project constructor runs, AS2 creation calls and explicit symbol linkage resolution. Constructors must run once on the actual receiver.
3. **Browser facade/events owner:** one proxy per ref, stable Graphics wrapper, parent/children queries without path-prefix assumptions, event routes, bounds, input and semantics. The browser translates operations; it does not own a second display graph.

Publication stays self-contained HTML5. The native editor and WASM share the core graph; introducing runtime state does not serialize backend handles or execute authoring scripts during evaluation.

## Acceptance and limits

Use one coherent workflow: construct and draw a component, reparent it without losing listener identity, change order, detach/reattach, exercise AS2 drawing and a typed buffer, then Restart. Verify core/WASM graph invariants and atomic rejection alongside ordinary exported pointer/keyboard behavior in the shared batch. Do not add a separate acceptance cycle for each Graphics method.

References: [AS3 DisplayObjectContainer](https://airsdk.dev/reference/actionscript/3.0/flash/display/DisplayObjectContainer.html), [AS3 Graphics](https://airsdk.dev/reference/actionscript/3.0/flash/display/Graphics.html), [AS2 MovieClip](https://open-flash.github.io/mirrors/as2-language-reference/MovieClip.html). These references guide the bounded profile; this bounded implementation is not measured Flash Player conformance.

## Explicit linked MovieClip increment

A saved non-root MovieClip may opt into runtime creation through `Symbol.linkage_id`, a unique, exact UTF-8 export identifier. This is separate from its library display name and optional project `class_name`. Version 2 stores the field; older documents remain readable. The Actions source panel edits the identifier in the existing undo transaction. Validation rejects root/non-MovieClip exports, invalid/duplicate identifiers and ambiguous exported class names. Cross-document clipboard transfer preserves linkage and source files, including every explicitly exported symbol and its dependency closure when transferring a program snapshot. Repeated paste reuses a matching symbol closure independently of generated IDs; conflicting content fails before changing the destination.

`create_linked(identifier)` allocates a stable root ref and a scoped timeline player over the existing immutable project. It does not clone the project. Descendant source addresses are `(namespace ObjectRef, relative authored path)`, so two instances of one symbol never share script targets, display refs or clocks. Each namespace uses the existing timeline evaluator, labels, nested clocks and authored replacement rules. The outer graph maps these placements into its ordinary refs, draw order, bounds, input and semantic targets. `target_ref` carries script dispatch identity across WASM; path-only addressing remains the compatibility route for the original scene.

AS3 `new ExportedClass()` uses the native allocation hook before the class constructor. AS2 `attachMovie` resolves the exact export identifier, accepts append depth, applies bounded enumerable data properties from `initObject` before class construction and attaches to the requested parent. A linked class can use authored children in its constructor. Construction during project static initialization is explicitly unsupported. No implicit export by display name, depth replacement or full historical construction schedule is inferred.

Each clip has independent playback controls, frame scripts and nested state. Detachment preserves the instance, fields, listeners and running clocks, while removing its rendered pixels and semantic targets from Stage. Authored replacements receive new refs; their dynamic child subtrees detach rather than becoming children of the replacement. Reset invalidates the full graph and program session.

Allocation reserves worst-case expanded symbol nodes and nesting across keyframes before modifying the graph: 100,000 linked source nodes in total, depth 64 and the existing 4,096 dynamic roots/objects. Scoped and scene script work shares the bounded queue. Reservation includes detached namespaces until reset; automatic collection is not claimed.

Acceptance extends the existing core and exported display-family scenarios. The public `linked-movieclips.nflash` combines two AS3 instances, an AS2 attachment, frame-label controls, nested animation, pointer/semantic activation, detachment and Restart in one workflow. Native editor acceptance remains separately recorded; core model tests do not establish that UI observation.
