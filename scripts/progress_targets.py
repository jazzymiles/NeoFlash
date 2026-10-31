#!/usr/bin/env python3
"""Functional target view of the ActionScript catalogue (methodology v3).

Both languages are counted against the same eight Flash Player areas. The
target is what NeoFlash adapts: AS3 Flash Player declarations (routed by the AS3
priority manifest) and AS2 language/core/runtime declarations (routed below).
AIR, Flex, components and other libraries are reported once, outside the target,
and never enter a percentage. Statuses are the catalogue's; this module only
partitions them.
"""

from __future__ import annotations

from progress_as3 import STAGES, classified_rows
from progress_catalog import STATUSES

# Public name of each stored status. `implemented_subset` is the stored key for Working.
PUBLIC = {"implemented_subset": "working", "partial": "partial", "missing": "missing", "needs_review": "needs_review"}
PUBLIC_STATUSES = tuple(PUBLIC.values())
NOW_STAGES = 6  # Stages 1–6 are the current priority: the classic foundation.
AS2_TARGET_SCOPES = {"language", "core", "runtime"}
AS2_OUTSIDE = (
    ("component", "AS2 UI components", "Version 2 component framework (Button, List, DataGrid, …). Not built in; far-future backlog."),
    ("other", "Other libraries and extensions", "Framework and platform-extension declarations shipped outside the Player."),
)
AS3_OUTSIDE = (
    ("air", "AIR-only APIs", "Documented for AIR without Flash Player availability. Far-future backlog."),
    ("library", "Flex, components and libraries", "Flex/Spark, Flash components, TLF and other supplied libraries. Not built in; far-future backlog."),
    ("unresolved", "Platform not resolved", "The reference does not state Player or AIR availability; classify before scheduling."),
)

# AS2 classes by Flash Player area. Unknown classes are an error, so a catalogue
# change always forces an explicit decision instead of a silent default.
AS2_CLASSES = {
    "math-geometry": {"Math", "Number", "flash.geom.Point", "flash.geom.Rectangle", "flash.geom.Matrix", "flash.geom.ColorTransform", "flash.geom.Transform"},
    "objects-language": {"Array", "Boolean", "Date", "Error", "Function", "Object", "String", "XML", "XMLNode", "arguments", "AsBroadcaster"},
    "timeline": {"MovieClip"},
    "display-events": {"Button", "Key", "Mouse", "Stage", "Selection", "ContextMenu", "ContextMenuItem"},
    "graphics-bitmap": {"Color", "flash.display.BitmapData", "flash.filters.BevelFilter", "flash.filters.BitmapFilter", "flash.filters.BlurFilter",
                        "flash.filters.ColorMatrixFilter", "flash.filters.ConvolutionFilter", "flash.filters.DisplacementMapFilter",
                        "flash.filters.DropShadowFilter", "flash.filters.GlowFilter", "flash.filters.GradientBevelFilter", "flash.filters.GradientGlowFilter"},
    "text-fonts": {"TextField", "TextField.StyleSheet", "TextFormat", "TextSnapshot", "flash.text.TextRenderer"},
    "audio-video": {"Sound", "Video", "NetStream", "NetConnection"},
    "services-advanced": {"Camera", "Microphone", "LoadVars", "LocalConnection", "SharedObject", "XMLSocket", "MovieClipLoader", "PrintJob",
                          "System", "System.IME", "System.capabilities", "System.security", "Accessibility",
                          "flash.external.ExternalInterface", "flash.net.FileReference", "flash.net.FileReferenceList"},
}
# Global functions and properties that belong to a specific area; everything else
# without an owner (operators, statements, directives, constants) is language.
AS2_GLOBALS = {
    "math-geometry": {"int", "isFinite", "isNaN", "parseFloat", "parseInt", "random", "Infinity", "-Infinity", "NaN"},
    "timeline": {"gotoAndPlay", "gotoAndStop", "play", "stop", "nextFrame", "prevFrame", "nextScene", "prevScene", "ifFrameLoaded", "tellTarget",
                 "call", "duplicateMovieClip", "removeMovieClip", "setProperty", "getProperty", "targetPath", "onClipEvent", "_root", "_parent", "_level"},
    "display-events": {"on", "setInterval", "clearInterval", "setTimeout", "clearTimeout", "getTimer", "startDrag", "stopDrag", "updateAfterEvent", "_focusrect"},
    "graphics-bitmap": {"_quality", "_highquality", "toggleHighQuality", "showRedrawRegions"},
    "text-fonts": {"scroll", "maxscroll"},
    "audio-video": {"stopAllSounds", "_soundbuftime"},
    "services-advanced": {"getURL", "loadMovie", "loadMovieNum", "unloadMovie", "unloadMovieNum", "loadVariables", "loadVariablesNum", "fscommand",
                          "MMExecute", "asfunction", "getVersion", "print", "printNum", "printAsBitmap", "printAsBitmapNum", "_accProps"},
}
AS2_CLASS_STAGE = {name: stage for stage, names in AS2_CLASSES.items() for name in names}
AS2_GLOBAL_STAGE = {name: stage for stage, names in AS2_GLOBALS.items() for name in names}


def public_counts(rows):
    counts = dict.fromkeys(PUBLIC_STATUSES, 0)
    for row in rows:
        counts[PUBLIC[row["status"]]] += 1
    counts["total"] = len(rows)
    return counts


def as2_stage(entry):
    if entry["owner"] is not None:
        key = entry["owner"]
    elif entry["kind"] == "class":
        key = entry["name"]
    else:
        return AS2_GLOBAL_STAGE.get(entry["name"], "objects-language")
    if key not in AS2_CLASS_STAGE:
        raise ValueError(f"No AS2 area decision for class {key}; add it to scripts/progress_targets.py")
    return AS2_CLASS_STAGE[key]


def language_view(rows, outside_rows, outside_defs, title):
    stages = []
    for order, (identifier, stage_title, _group, purpose) in enumerate(STAGES, 1):
        members = [row for row in rows if row["stage"] == identifier]
        stages.append({"id": identifier, "order": order, "title": stage_title, "purpose": purpose,
                       "now": order <= NOW_STAGES, "counts": public_counts(members)})
    now = [row for row in rows if row["stage"] in {stage["id"] for stage in stages if stage["now"]}]
    outside = [{"id": identifier, "title": label, "purpose": purpose, "counts": public_counts(outside_rows.get(identifier, []))}
               for identifier, label, purpose in outside_defs]
    return {"title": title, "stages": stages, "now": public_counts(now), "target": public_counts(rows),
            "outside": outside, "total": public_counts(rows + [row for group in outside_rows.values() for row in group])}


def load_targets(root, catalog, revision=None):
    """Return {AS2, AS3} target views; AS3 is absent before its manifest existed."""
    result = {}
    as2 = [row for row in catalog["entries"] if row["language"] == "AS2"]
    target, outside = [], {"component": [], "other": []}
    for row in as2:
        if row["scope"] in AS2_TARGET_SCOPES:
            target.append({**row, "stage": as2_stage(row)})
        else:
            outside["component" if row["scope"] == "component" else "other"].append(row)
    result["AS2"] = language_view(target, outside, AS2_OUTSIDE, "ActionScript 2 — Flash Player")
    classified = classified_rows(root, catalog, revision)
    if classified:
        rows = classified[2]
        outside = {identifier: [row for row in rows if row["bucket"] == identifier] for identifier, _, _ in AS3_OUTSIDE}
        result["AS3"] = language_view([row for row in rows if row["bucket"] == "player"], outside, AS3_OUTSIDE, "ActionScript 3 — Flash Player")
    for language, view in result.items():
        if view["total"]["total"] != sum(1 for row in catalog["entries"] if row["language"] == language):
            raise ValueError(f"{language} target view does not cover every catalogue entry")
    return result


def share(counts, *statuses):
    total = counts["total"]
    return 100 * sum(counts[status] for status in statuses) / total if total else 0.0


assert set(PUBLIC) == set(STATUSES)
