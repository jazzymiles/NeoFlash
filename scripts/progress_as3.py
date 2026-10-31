#!/usr/bin/env python3
"""Partition the existing AS3 audit by documented platform and adaptation order.

The compact manifest records reference-derived owner defaults and exact member
exceptions. Public builds need only the manifest and normalized audit, never the
untracked research catalogue. A platform label is not an implementation claim.
"""

from __future__ import annotations

from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import subprocess

from progress_catalog import STATUSES, counter, read_bytes

MANIFEST_PATH = "docs/progress/as3-priorities.json"
BUCKETS = (
    ("player", "Flash Player reference", "AS3 language, core and Player APIs; member and runtime-profile limits still apply."),
    ("air", "Far future: AIR extensions", "Declarations documented for AIR without Player availability. Far-future backlog, with no scheduled adaptation; revisit only after an explicit priority change."),
    ("library", "Far future: libraries and platform extensions", "Flash components, Flex/Spark, TLF and other supplied libraries. Far-future backlog, with no scheduled adaptation; executing on Player does not make them built-in APIs."),
    ("unresolved", "Platform not resolved", "The reference does not establish Player or AIR availability; review before assigning an adaptation target."),
)
GROUPS = (
    ("foundation", "Important now: classic foundation", "Stages 1–6: mathematics, objects, MovieClip, display, drawing and text, in that order."),
    ("media", "Next: file audio and video", "Stage 7: ordinary audio/video playback after the classic foundation."),
    ("later", "Later: services and advanced Player APIs", "Stage 8: capture, networking, files, platform integration, storage, 3D, workers and specialized APIs."),
    *BUCKETS[1:],
)
STAGES = (
    ("math-geometry", "Mathematics and 2D geometry", "foundation", "Numbers, mathematical operations, points, rectangles, matrices and color transforms."),
    ("objects-language", "Objects, language and collections", "foundation", "Object and class semantics, functions, collections, strings, errors, dates, regular expressions, XML and binary data."),
    ("timeline", "MovieClip and timeline", "foundation", "MovieClip playback, frame labels, scenes and frame lifecycle events."),
    ("display-events", "Display, events and basic input", "foundation", "Display trees, stage behavior, event dispatch, keyboard/mouse input and timers."),
    ("graphics-bitmap", "Graphics, bitmaps and filters", "foundation", "2D drawing, bitmap pixels, image encoding and conventional visual filters."),
    ("text-fonts", "Text and fonts", "foundation", "Classic text fields, text formatting, stylesheets, font access and text metrics."),
    ("audio-video", "File audio and video playback", "media", "Sound channels and transforms, video display and the file-playback part of NetStream."),
    ("services-advanced", "Services and advanced Player APIs", "later", "Camera/microphone capture, network/files/platform/storage, loading, 3D, workers, Flash Text Engine and specialized profiles."),
)
LIBRARY_SCOPES = {"component", "framework", "platform_extension"}
BUCKET_IDS = {row[0] for row in BUCKETS}
STAGE_IDS = {row[0] for row in STAGES}
IDENTITY_FIELDS = ("catalog_id", "language", "owner", "name", "kind", "scope", "source_url")


def identity_hash(entries):
    identities = [[row[field] for field in IDENTITY_FIELDS] for row in entries if row["language"] == "AS3"]
    identities.sort(key=lambda row: row[0])
    return hashlib.sha256(json.dumps(identities, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()


def reference_bucket(entry):
    """Read declaration availability, without inheriting the class's runtime."""
    if entry["scope"] in LIBRARY_SCOPES:
        return "library"
    if entry["scope"] == "language" and entry["kind"] in {"operator", "statement", "special_type"}:
        return "player"  # Explicit AS3 language declarations, not package-name inference.
    availability = entry.get("detail_availability") or entry.get("availability", [])
    runtimes = {item["runtime"] for item in availability if "unsupported" not in item["version"].lower()}
    if "Flash" in runtimes:
        return "player"
    if "AIR" in runtimes:
        return "air"
    return "unresolved"


def adaptation_stage(entry):
    """Authored breadth-first policy, applied only after Player classification."""
    owner, name = entry["owner"] or "", entry["name"]
    if owner in {"Math", "Number", "int", "uint", "flash.geom.Point", "flash.geom.Rectangle", "flash.geom.Matrix", "flash.geom.ColorTransform"}:
        return "math-geometry"
    if owner == "<global>" and name in {"isFinite", "isNaN", "parseFloat", "parseInt", "NaN", "Infinity"}:
        return "math-geometry"
    if owner == "flash.geom.Transform":
        return "services-advanced" if name in {"matrix3D", "perspectiveProjection", "getRelativeMatrix3D"} else "math-geometry"
    if owner in {"flash.display.MovieClip", "flash.display.FrameLabel", "flash.display.Scene"}:
        return "timeline"
    if owner == "flash.events.Event" and name in {"ENTER_FRAME", "EXIT_FRAME", "FRAME_CONSTRUCTED", "FRAME_LABEL"}:
        return "timeline"
    if owner == "flash.display.DisplayObject" and name in {"enterFrame", "exitFrame", "frameConstructed"}:
        return "timeline"
    if owner == "flash.events.Event" and name in {"ID3", "SOUND_COMPLETE", "VIDEO_FRAME"}:
        return "audio-video"
    if owner in {"flash.utils.Timer", "flash.events.TimerEvent"} or (owner == "flash.utils" and name in {"getTimer", "setInterval", "clearInterval", "setTimeout", "clearTimeout"}):
        return "display-events"
    if entry["scope"] in {"language", "core"}:
        return "objects-language"
    if owner.startswith(("flash.errors.", "flash.xml.")) or owner in {"flash.utils", "flash.utils.ByteArray", "flash.utils.CompressionAlgorithm", "flash.utils.Dictionary", "flash.utils.Endian", "flash.utils.IDataInput", "flash.utils.IDataOutput", "flash.net.ObjectEncoding"}:
        return "services-advanced" if owner == "flash.utils.ByteArray" and name in {"shareable", "atomicCompareAndSwapIntAt", "atomicCompareAndSwapLength"} else "objects-language"
    if owner == "flash.net" and name in {"getClassByAlias", "registerClassAlias"}:
        return "objects-language"
    if owner in {"flash.display.DisplayObject", "flash.display.DisplayObjectContainer", "flash.display.InteractiveObject", "flash.display.Sprite", "flash.display.Shape", "flash.display.SimpleButton", "flash.display.Stage", "flash.display.StageAlign", "flash.display.StageScaleMode", "flash.display.StageDisplayState", "flash.display.StageQuality"}:
        advanced = {"z", "rotationX", "rotationY", "rotationZ", "scaleZ", "stage3Ds", "stageVideos", "transformGesture", "softKeyboardRect", "requestSoftKeyboard", "needsSoftKeyboard", "softKeyboardInputAreaOfInterest", "globalToLocal3D", "local3DToGlobal", "blendShader"}
        return "services-advanced" if name in advanced else "display-events"
    if owner in {"flash.events.Event", "flash.events.ErrorEvent", "flash.events.EventDispatcher", "flash.events.IEventDispatcher", "flash.events.EventPhase", "flash.events.MouseEvent", "flash.events.KeyboardEvent", "flash.events.FocusEvent", "flash.events.FullScreenEvent", "flash.ui.Keyboard", "flash.ui.KeyboardType", "flash.ui.KeyLocation", "flash.ui.Mouse", "flash.ui.MouseCursor", "flash.ui.MouseCursorData"}:
        return "services-advanced" if owner == "flash.events.Event" and name in {"CHANNEL_MESSAGE", "CHANNEL_STATE", "WORKER_STATE", "CONTEXT3D_CREATE", "TEXTURE_READY"} else "display-events"
    if owner.startswith("flash.filters.") and owner != "flash.filters.ShaderFilter":
        return "graphics-bitmap"
    if owner.startswith("flash.display.Graphics") or owner in {"flash.display.Bitmap", "flash.display.BitmapData", "flash.display.BitmapDataChannel", "flash.display.BitmapEncodingColorSpace", "flash.display.BlendMode", "flash.display.CapsStyle", "flash.display.GradientType", "flash.display.InterpolationMethod", "flash.display.JPEGEncoderOptions", "flash.display.JPEGXREncoderOptions", "flash.display.JointStyle", "flash.display.LineScaleMode", "flash.display.PNGEncoderOptions", "flash.display.PixelSnapping", "flash.display.SpreadMethod", "flash.display.TriangleCulling", "flash.display.IBitmapDrawable", "flash.display.IGraphicsData", "flash.display.IGraphicsFill", "flash.display.IGraphicsPath", "flash.display.IGraphicsStroke"}:
        return "services-advanced" if "Shader" in owner or name in {"beginShaderFill", "lineShaderStyle"} else "graphics-bitmap"
    if owner.startswith("flash.text.") and not owner.startswith(("flash.text.engine.", "flash.text.ime.", "flash.text.StageText")):
        return "text-fonts"
    if owner == "flash.events.TextEvent":
        return "text-fonts"
    if owner in {"flash.media.Sound", "flash.media.SoundChannel", "flash.media.SoundMixer", "flash.media.SoundTransform", "flash.media.SoundLoaderContext", "flash.media.ID3Info", "flash.media.Video", "flash.media.VideoStatus"}:
        return "services-advanced" if name in {"attachCamera", "areSoundsInaccessible"} else "audio-video"
    if owner == "flash.net.NetStream" and name in {"NetStream", "play", "pause", "resume", "togglePause", "seek", "close", "dispose", "client", "time", "bytesLoaded", "bytesTotal", "bufferLength", "bufferTime", "checkPolicyFile", "soundTransform", "netStatus", "ioError", "asyncError", "onMetaData", "onCuePoint", "onPlayStatus"}:
        return "audio-video"
    return "services-advanced"


def build_manifest(reference, catalog, reference_sha256):
    """Build explicit routes; callers must verify source bytes against the audit."""
    entries = [row for row in catalog["entries"] if row["language"] == "AS3"]
    originals = {row["id"]: row for row in reference["entries"]}
    if len(originals) != len(reference["entries"]) or set(originals) != {row["catalog_id"] for row in entries}:
        raise ValueError("AS3 priority source IDs do not match the normalized catalogue")
    by_owner, routes = defaultdict(list), {}
    for row in entries:
        original = originals[row["catalog_id"]]
        for field in IDENTITY_FIELDS[1:]:
            if original.get(field) != row[field]:
                raise ValueError(f"AS3 reference identity differs: {row['catalog_id']} ({field})")
        bucket = reference_bucket(original)
        if bucket == "library":
            continue
        route = [bucket, adaptation_stage(original) if bucket == "player" else None]
        by_owner[row["owner"] or ""].append(tuple(route))
        routes[row["catalog_id"]] = route
    defaults = {owner: list(sorted(Counter(values).items(), key=lambda item: (-item[1], str(item[0])))[0][0]) for owner, values in sorted(by_owner.items())}
    exceptions = {row["catalog_id"]: routes[row["catalog_id"]] for row in entries if row["catalog_id"] in routes and routes[row["catalog_id"]] != defaults[row["owner"] or ""]}
    return {
        "schemaVersion": 1,
        "title": "AS3 adaptation priorities",
        "description": "Planning partitions of the complete declaration audit. Existing subset and partial evidence is retained; only missing behavior is an adaptation gap.",
        "source": {"referenceFile": "as3.json", "referenceSha256": reference_sha256,
                   "referenceGeneration": reference.get("source_baseline", {}).get("reference_generation_date"),
                   "entryCount": len(entries), "identitySha256": identity_hash(entries),
                   "url": "https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/index.html"},
        "classificationPolicy": "Library scopes are separated first. Explicit AS3 operators, statements and special types belong to the Player language foundation. For other declarations use detail_availability, then index availability; Flash means Player reference, AIR without Flash means AIR-documented, and no label stays unresolved. Owner defaults are compressed per-entry decisions, not class availability inherited by members. Exceptions preserve every differing member decision.",
        "buckets": [{"id": identifier, "title": title, "purpose": purpose} for identifier, title, purpose in BUCKETS],
        "groups": [{"id": identifier, "title": title, "purpose": purpose} for identifier, title, purpose in GROUPS],
        "stages": [{"id": identifier, "order": order, "title": title, "group": group, "purpose": purpose} for order, (identifier, title, group, purpose) in enumerate(STAGES, 1)],
        "libraryScopes": sorted(LIBRARY_SCOPES),
        "ownerDefaults": defaults,
        "entryExceptions": dict(sorted(exceptions.items())),
        "profileRestrictions": {
            "AS3:constructor:flash/desktop/Clipboard.html#Clipboard()": {
                "note": "The constructor is listed for Player but throws there; only generalClipboard is usable. This is a profile restriction, not a missing useful constructor.",
                "sourceUrl": "https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/flash/desktop/Clipboard.html#Clipboard()"
            }
        } if "AS3:constructor:flash/desktop/Clipboard.html#Clipboard()" in originals else {},
        "limitations": [
            "Counts cover declarations of all kinds, not only functions, and are not a Flash compatibility percentage.",
            "Player labels retain version, sandbox and profile limits; the list is not a promise of unrestricted HTML5 behavior.",
            "AIR-documented without a Player label is a separate documentation bucket, not proof that every unlabeled historical runtime is incompatible.",
            "Foundation priority is not an implementation status. implemented_subset and partial remain bounded; needs_review is neither implemented nor missing.",
            "The audit is a dated evidence snapshot. This classification does not run tests or promote current source conformance.",
            "A stage includes only its declared members: advanced members of shared classes may be deferred to stage 8.",
        ],
    }


def classify_entries(manifest, catalog):
    """Validate and expand one route per AS3 declaration, preserving audit rows."""
    if manifest.get("schemaVersion") != 1:
        raise ValueError("Unsupported AS3 priority manifest schema")
    entries = [row for row in catalog["entries"] if row["language"] == "AS3"]
    if len({row["catalog_id"] for row in entries}) != len(entries):
        raise ValueError("Duplicate AS3 catalogue IDs")
    source = manifest["source"]
    if source["entryCount"] != len(entries) or source["identitySha256"] != identity_hash(entries):
        raise ValueError("AS3 priority manifest does not match catalogue identities")
    audited = next((item for item in catalog["metadata"]["sourceFiles"] if item["file"] == "as3.json"), None)
    if not audited or source["referenceSha256"] != audited["sha256"]:
        raise ValueError("AS3 priority reference hash differs from audit provenance")
    if set(manifest["libraryScopes"]) != LIBRARY_SCOPES:
        raise ValueError("AS3 priority library scope policy differs")
    definitions = {}
    for name in ("buckets", "groups", "stages"):
        definitions[name] = {item["id"]: item for item in manifest[name]}
        if len(definitions[name]) != len(manifest[name]) or not all(item.get("title") and item.get("purpose") for item in manifest[name]):
            raise ValueError(f"Invalid or duplicate AS3 priority {name}")
    if set(definitions["buckets"]) != BUCKET_IDS or not {"air", "library", "unresolved"} <= definitions["groups"].keys():
        raise ValueError("AS3 priority platform groups are incomplete")
    orders = [item["order"] for item in manifest["stages"]]
    if len(set(orders)) != len(orders) or not all(type(order) is int and order > 0 for order in orders):
        raise ValueError("AS3 priority stage order must be unique and positive")
    for stage in manifest["stages"]:
        if stage["group"] not in definitions["groups"] or stage["group"] in {"air", "library", "unresolved"}:
            raise ValueError("AS3 priority stage has an invalid planning group")
    ids = {row["catalog_id"] for row in entries}
    if set(manifest["entryExceptions"]) - ids or set(manifest["profileRestrictions"]) - ids:
        raise ValueError("AS3 priority exceptions contain unknown catalogue IDs")
    used_owners = set()
    classified = []
    for row in entries:
        if row["status"] not in STATUSES:
            raise ValueError("Unknown AS3 implementation status")
        if row["scope"] in LIBRARY_SCOPES:
            if row["catalog_id"] in manifest["entryExceptions"]:
                raise ValueError("Library entries cannot override their platform bucket")
            bucket, stage = "library", None
        else:
            owner = row["owner"] or ""
            used_owners.add(owner)
            if owner not in manifest["ownerDefaults"]:
                raise ValueError(f"No explicit AS3 platform decision for owner {owner}")
            bucket, stage = manifest["entryExceptions"].get(row["catalog_id"], manifest["ownerDefaults"][owner])
        if bucket not in definitions["buckets"] or (bucket == "player" and stage not in definitions["stages"]) or (bucket != "player" and stage is not None):
            raise ValueError(f"Invalid AS3 priority route for {row['catalog_id']}")
        classified.append({**row, "bucket": bucket, "stage": stage})
    if used_owners != set(manifest["ownerDefaults"]):
        raise ValueError("AS3 priority owner defaults contain unused owners")
    return classified


def classified_rows(root, catalog, revision=None):
    """Return (manifest, raw bytes, routed AS3 rows); None when no manifest exists."""
    root = Path(root)
    if revision is None:
        if not (root / MANIFEST_PATH).exists():
            return None
    else:
        subprocess.check_output(["git", "-C", str(root), "rev-parse", "--verify", f"{revision}^{{commit}}"], stderr=subprocess.DEVNULL)
        exists = subprocess.run(["git", "-C", str(root), "cat-file", "-e", f"{revision}:{MANIFEST_PATH}"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if exists.returncode:
            return None
    raw = read_bytes(root, MANIFEST_PATH, revision)
    manifest = json.loads(raw)
    rows = classify_entries(manifest, catalog)
    stage_groups = {stage["id"]: stage["group"] for stage in manifest["stages"]}
    for row in rows:
        row["group"] = stage_groups[row["stage"]] if row["bucket"] == "player" else row["bucket"]
    return manifest, raw, rows


def load_priorities(root, catalog, revision=None):
    """Read current or immutable historical classification; absent means unknown."""
    classified = classified_rows(root, catalog, revision)
    if classified is None:
        return None
    manifest, raw, rows = classified
    return {
        "schemaVersion": 1, "title": manifest["title"], "description": manifest["description"],
        "source": {**manifest["source"], "classificationSha256": hashlib.sha256(raw).hexdigest(), "manifestPath": MANIFEST_PATH},
        "total": counter(rows),
        "buckets": [{**definition, "counts": counter([row for row in rows if row["bucket"] == definition["id"]])} for definition in manifest["buckets"]],
        "groups": [{**definition, "counts": counter([row for row in rows if row["group"] == definition["id"]])} for definition in manifest["groups"]],
        "stages": [{**definition, "counts": counter([row for row in rows if row["stage"] == definition["id"]])} for definition in sorted(manifest["stages"], key=lambda item: item["order"])],
        "limitations": manifest["limitations"], "currentSourceVerified": False,
    }
