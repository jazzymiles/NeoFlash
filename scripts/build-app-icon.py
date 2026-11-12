#!/usr/bin/env python3
"""Generate the NeoFlash application icon set from the NF2 logo geometry.

The mark is the NF2 `logo` component (crates/neoflash-ui/src/components/display.rs): on a 28-unit plate, a ring
of radius 8 with a 2-unit stroke and a filled dot of radius 4. Colours come from the NF2 tokens
(interface/design-system/tokens/tokens.css):

* light: plate `accent/default` #3c6bf0 (gradient towards `accent/hover` #2f59d6), mark `text/on-accent` #ffffff;
* dark: plate `bg/hover` #1e2330 -> `bg/app` #0d1016, mark `accent/hover` #86a5ff -> `accent/default` #6c93ff.

Outputs (committed, regenerate with `python3 scripts/build-app-icon.py`):

* `{light,dark}/icon_<N>.png` for N = 16, 32, 64, 128, 256, 512, 1024 (macOS template: 824/1024 plate,
  continuous-looking corners, drop shadow);
* `NeoFlash.icns` (light) and `NeoFlash-dark.icns` via `iconutil` (macOS only);
* `NeoFlash.icon/` Icon Composer document with a dark-appearance specialization, compiled by `actool` into
  `Assets.car` at packaging time so macOS 26+ shows the dark/tinted variants;
* `windows/NeoFlash.ico` (16-256) and MSIX logos `windows/{Square44x44Logo,Square150x150Logo,StoreLogo}.png`;
* `window-icon.png` (256, light) used by the editor window on Windows/Linux and for unbundled macOS runs.

Dependency-free: rasterises analytically (signed distances, one coverage sample per pixel) and writes PNG with zlib.
"""

from __future__ import annotations

import json
import math
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "neoflash-editor" / "assets" / "app-icon"


def hex_rgb(value: str) -> tuple[float, float, float]:
    value = value.lstrip("#")
    return tuple(int(value[i : i + 2], 16) / 255.0 for i in (0, 2, 4))  # type: ignore[return-value]


def mix(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


WHITE = (1.0, 1.0, 1.0)
LIGHT = {
    "plate_top": mix(hex_rgb("#3c6bf0"), WHITE, 0.14),
    "plate_bottom": hex_rgb("#2f59d6"),
    "mark_top": WHITE,
    "mark_bottom": WHITE,
    "rim": (WHITE, 0.14),
}
DARK = {
    "plate_top": hex_rgb("#1e2330"),
    "plate_bottom": hex_rgb("#0d1016"),
    "mark_top": hex_rgb("#86a5ff"),
    "mark_bottom": hex_rgb("#6c93ff"),
    "rim": (WHITE, 0.10),
}

# Mark proportions on the 28-unit NF2 logo plate.
RING_RADIUS = 8.0 / 28.0
RING_STROKE = 2.0 / 28.0
DOT_RADIUS = 4.0 / 28.0


def clamp01(v: float) -> float:
    return 0.0 if v < 0.0 else 1.0 if v > 1.0 else v


def round_rect_sd(px: float, py: float, half: float, radius: float) -> float:
    qx = abs(px) - half + radius
    qy = abs(py) - half + radius
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    return outside + min(max(qx, qy), 0.0) - radius


def render(size: int, palette: dict, style: str) -> bytes:
    """Return straight-alpha RGBA bytes for one icon size."""
    s = float(size)
    if style == "macos":
        side = s * 824.0 / 1024.0
        radius = s * 185.4 / 1024.0
        shadow = (s * 10.0 / 1024.0, max(s * 12.0 / 1024.0, 0.6), 0.30)  # y offset, sigma, alpha
    else:  # windows: near full-bleed plate, no shadow
        side = s * 30.0 / 32.0
        radius = side * 0.21
        shadow = None
    cx = cy = s / 2.0
    half = side / 2.0
    top = cy - half
    ring_r = RING_RADIUS * side
    ring_w = RING_STROKE * side
    dot_r = DOT_RADIUS * side
    rim_w = max(1.0, s / 512.0)
    rim_color, rim_alpha = palette["rim"]
    sqrt2 = math.sqrt(2.0)
    out = bytearray(size * size * 4)
    i = 0
    for y in range(size):
        py = y + 0.5
        t = clamp01((py - top) / side)
        plate = mix(palette["plate_top"], palette["plate_bottom"], t)
        mark = mix(palette["mark_top"], palette["mark_bottom"], t)
        dy = py - cy
        for x in range(size):
            dx = x + 0.5 - cx
            # premultiplied accumulation, back to front
            r = g = b = a = 0.0
            d_plate = round_rect_sd(dx, dy, half, radius)
            if shadow is not None:
                off, sigma, s_alpha = shadow
                d_shadow = round_rect_sd(dx, dy - off, half, radius)
                sa = s_alpha * 0.5 * math.erfc(d_shadow / (sigma * sqrt2))
                a = sa  # black
            cov = clamp01(0.5 - d_plate)
            if cov > 0.0:
                # plate over shadow
                r = plate[0] * cov + r * (1.0 - cov)
                g = plate[1] * cov + g * (1.0 - cov)
                b = plate[2] * cov + b * (1.0 - cov)
                a = cov + a * (1.0 - cov)
                # inner rim
                d_rim = abs(d_plate + rim_w / 2.0) - rim_w / 2.0
                rc = clamp01(0.5 - d_rim) * rim_alpha * cov
                if rc > 0.0:
                    r = rim_color[0] * rc + r * (1.0 - rc)
                    g = rim_color[1] * rc + g * (1.0 - rc)
                    b = rim_color[2] * rc + b * (1.0 - rc)
                # mark: ring + dot
                dist = math.hypot(dx, dy)
                mc = max(clamp01(0.5 - (abs(dist - ring_r) - ring_w / 2.0)), clamp01(0.5 - (dist - dot_r))) * cov
                if mc > 0.0:
                    r = mark[0] * mc + r * (1.0 - mc)
                    g = mark[1] * mc + g * (1.0 - mc)
                    b = mark[2] * mc + b * (1.0 - mc)
            if a > 0.0:
                inv = 1.0 / a
                out[i] = round(clamp01(r * inv) * 255)
                out[i + 1] = round(clamp01(g * inv) * 255)
                out[i + 2] = round(clamp01(b * inv) * 255)
                out[i + 3] = round(clamp01(a) * 255)
            i += 4
    return bytes(out)


def png_bytes(size: int, rgba: bytes) -> bytes:
    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    stride = size * 4
    raw = b"".join(b"\x00" + rgba[y * stride : (y + 1) * stride] for y in range(size))
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    srgb = b"\x00"  # perceptual
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"sRGB", srgb)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico_bytes(images: list[tuple[int, bytes]]) -> bytes:
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    data = b""
    for size, png in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    return header + entries + data


MAC_SIZES = (16, 32, 64, 128, 256, 512, 1024)
ICONSET = (
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
)


def srgb_string(rgb) -> str:
    return "srgb:" + ",".join(f"{c:.5f}" for c in (*rgb, 1.0))


def write_icon_composer(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    (path / "Assets").mkdir(parents=True)
    c = 512.0
    ring_r = RING_RADIUS * 1024.0
    ring_w = RING_STROKE * 1024.0
    dot_r = DOT_RADIUS * 1024.0
    def circle(r: float) -> str:
        return f"M{c - r:.3f} {c:g}a{r:.3f} {r:.3f} 0 1 0 {2 * r:.3f} 0a{r:.3f} {r:.3f} 0 1 0 {-2 * r:.3f} 0Z"

    # Filled even-odd geometry only: Icon Composer recolours layer fills and ignores SVG strokes.
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
        f'<path fill="#ffffff" fill-rule="evenodd" d="{circle(ring_r + ring_w / 2)}{circle(ring_r - ring_w / 2)}'
        f'{circle(dot_r)}"/>'
        "</svg>\n"
    )
    (path / "Assets" / "mark.svg").write_text(svg)
    doc = {
        "fill-specializations": [
            {"value": {"linear-gradient": [srgb_string(LIGHT["plate_top"]), srgb_string(LIGHT["plate_bottom"])]}},
            {
                "appearance": "dark",
                "value": {"linear-gradient": [srgb_string(DARK["plate_top"]), srgb_string(DARK["plate_bottom"])]},
            },
        ],
        "groups": [
            {
                "layers": [
                    {
                        "name": "mark",
                        "image-name": "mark.svg",
                        "glass": True,
                        "fill-specializations": [
                            {"value": {"solid": srgb_string(WHITE)}},
                            {"appearance": "dark", "value": {"solid": srgb_string(DARK["mark_bottom"])}},
                        ],
                    }
                ],
                "shadow": {"kind": "neutral", "opacity": 0.5},
                "translucency": {"enabled": True, "value": 0.4},
                "specular": True,
            }
        ],
        "supported-platforms": {"squares": ["macOS"]},
    }
    (path / "icon.json").write_text(json.dumps(doc, indent=2) + "\n")


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    rendered: dict[tuple[str, int], bytes] = {}
    for name, palette in (("light", LIGHT), ("dark", DARK)):
        folder = OUT / name
        if folder.exists():
            shutil.rmtree(folder)
        folder.mkdir()
        for size in MAC_SIZES:
            png = png_bytes(size, render(size, palette, "macos"))
            rendered[(name, size)] = png
            (folder / f"icon_{size}.png").write_bytes(png)
        print(f"{name}: {', '.join(str(s) for s in MAC_SIZES)}")

    (OUT / "window-icon.png").write_bytes(rendered[("light", 256)])

    win = OUT / "windows"
    if win.exists():
        shutil.rmtree(win)
    win.mkdir()
    ico = [(s, png_bytes(s, render(s, LIGHT, "windows"))) for s in (16, 24, 32, 48, 64, 128, 256)]
    (win / "NeoFlash.ico").write_bytes(ico_bytes(ico))
    for file_name, size in (("Square44x44Logo.png", 44), ("Square150x150Logo.png", 150), ("StoreLogo.png", 50)):
        (win / file_name).write_bytes(png_bytes(size, render(size, LIGHT, "windows")))
    print("windows: NeoFlash.ico + MSIX logos")

    write_icon_composer(OUT / "NeoFlash.icon")
    print("NeoFlash.icon (Icon Composer, light + dark)")

    if shutil.which("iconutil"):
        for name, icns in (("light", "NeoFlash.icns"), ("dark", "NeoFlash-dark.icns")):
            with tempfile.TemporaryDirectory() as tmp:
                iconset = Path(tmp) / "NeoFlash.iconset"
                iconset.mkdir()
                for file_name, size in ICONSET:
                    (iconset / file_name).write_bytes(rendered[(name, size)])
                subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(OUT / icns)], check=True)
        print("NeoFlash.icns, NeoFlash-dark.icns")
    else:
        print("iconutil not found: .icns files were not regenerated", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
