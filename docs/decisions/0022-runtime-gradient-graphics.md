# Runtime gradient Graphics and shared path geometry

Status: accepted for the bounded profile below, 2026-10-27.

## Store drawing intent in Rust

Runtime Graphics owns fill and stroke styles, path runs and copied state. Gradient paints extend runtime snapshots with copied color stops and an affine gradient matrix; they do not change the authored document format. Solid paths retain their existing representation. Parent ColorTransform values apply to gradient stops through the same Rust scene pipeline as solid paint.

Linear and radial gradients use Flash's canonical gradient space. The first profile supports pad spreading and RGB interpolation, with explicit finite-coordinate, stop-count and focal-point limits. Repeat/reflect spreading, linearRGB interpolation and legacy matrix-object formats require their own implementation; constants alone do not implement those modes.

Round rectangles, cubic curves and decoded drawPath commands become the existing shared path commands. The per-corner drawRoundRectComplex convenience is a NeoFlash extension, not an Adobe Flash Graphics declaration. Their bounds, Canvas drawing and hit geometry therefore use the same curves. Graphics.copyFrom takes an independent snapshot of drawing state; later redraws cannot mutate its source or destination. Whole operation batches validate and reserve bounded storage before mutation.

## Keep renderer details outside authoring data

The browser resolves gradients using the stored gradient matrix and existing Path2D geometry. Paint visibility and pointer picking account for gradient alpha. Accessibility bounds remain conservative geometric bounds for partially visible paths, rather than claiming a pixel-exact silhouette. Gradient strokes use one reusable offscreen mask capped at 16,777,216 pixels; the mask is released on Restart, failure or page exit, and oversized coverage diagnoses before allocation. Gradient fill uses a clipped brush under the stored paint transform.

The editor continues to preserve its authored scene while scripts run in the publication. Native gradient tools, masks, filters, bitmap fills and complete Flash rasterization rules are separate work.

## Verify one composition

Extend dynamic-display.nflash: a live card has a linear fill, gradient stroke and radial badge; its copied Graphics stays unchanged when the card changes palette. The underlying shape has unequal round corners, another panel draws a Vector path and cubic curve, and AS2 draws a gradient triangle with percentage alpha. Reparenting, removal/return and Restart retain their existing roles. One ordinary-export/WASM workflow and one browser acceptance cover the family; existing regression counts remain controlled.

Primary API reference: [AIR Graphics](https://airsdk.dev/reference/actionscript/3.0/flash/display/Graphics.html). Historical player rasterization and independent per-declaration acceptance are separate evidence.
