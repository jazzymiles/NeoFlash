# 0002 — shared vector geometry and rendering

Status: accepted, 2026-10-26.

The document stores renderer-independent path commands: move, line, quadratic/cubic Bézier and close. A path can contain multiple contours with either nonzero or even–odd fill. Fill/stroke are RGBA colors; initial strokes use round caps and joins. Native GPU objects and browser Path2D handles never enter the source file.

The Rust core validates commands, computes analytic curve bounds, bounds adaptive flattening and performs local-space winding/stroke hit tests. Authored keyframes interpolate a path only when command topology and fill rule match. Other topology remains held until its next key; this is not arbitrary Flash shape tweening.

The desktop uses [lyon tessellation](https://docs.rs/lyon_tessellation/1.0.23/lyon_tessellation/) for concave/compound fills and stroke meshes, then applies the complete nested affine transform to every vertex. The browser uses Canvas Path2D with the same commands, fill rule and stroke style. Tests cover contour holes, curve extrema, transforms, point edits and exported drawing/hit behavior. Font shaping remains a separate issue.

Pen drafts and pencil samples are editor-only gesture state. Finishing a path creates one undo transaction. Subselection edits authored points in local coordinates through the inverse object transform. Moving an anchor carries its incident control points; control handles are independently editable. Pencil simplifies samples with a bounded iterative Ramer–Douglas–Peucker pass. A sample limit reports and rejects an overlong draft rather than silently clipping the result.

Remaining work includes pressure-sensitive brush strokes, smoothing/fitting cubic splines, point insertion/deletion, tangent constraints, gradients, shape boolean operations, masks, editable registration points and tessellation caching. This decision establishes geometry shared by future importers; it does not by itself add binary FLA or XFL DOMShape decoding.
