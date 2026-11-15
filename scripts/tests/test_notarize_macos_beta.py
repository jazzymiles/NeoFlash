#!/usr/bin/env python3
"""Hermetic transactional and failure-injection tests for scripts/notarize-macos-beta.sh.

Boundary:
    This test suite validates ONLY the local staging, nested-media hash refresh,
    notarytool status/log handling, stapling/Gatekeeper gate wiring, SHA256SUMS
    regeneration, DMG mount/temp cleanup, and atomic five-artifact replacement or
    rollback of ``scripts/notarize-macos-beta.sh`` using deterministic local stubs.
    It never contacts Apple servers and does NOT establish real Apple Developer ID
    code-signing or Apple Notary Service acceptance.
"""

from __future__ import annotations

import hashlib
import json
import os
import plistlib
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import textwrap
import unittest
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
NOTARIZE_SCRIPT = ROOT / "scripts" / "notarize-macos-beta.sh"
PROVENANCE_SCRIPT = ROOT / "scripts" / "bundled-media-provenance.py"
ENTITLEMENTS_SRC = ROOT / "scripts" / "macos-entitlements.plist"
ICON_SRC = ROOT / "crates" / "neoflash-editor" / "assets" / "app-icon" / "NeoFlash.icns"

VERSION = "0.1.0-beta.1"
ZIP_NAME = f"NeoFlash-{VERSION}-macos-arm64.zip"
DMG_NAME = f"NeoFlash-{VERSION}-macos-arm64.dmg"
SOURCE_NAME = f"NeoFlash-{VERSION}-media-corresponding-source.tar.xz"
SUMS_NAME = f"NeoFlash-{VERSION}-SHA256SUMS.txt"
DEV_ID = "Developer ID Application: NeoFlash Test Harness (TESTTEAM99)"
NOTARY_PROFILE = "neoflash-hermetic-test-profile"

MACHO_ARM64_HEADER = b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01\x00\x00\x00\x00\x02\x00\x00\x00"
CODESIGN_MARKER = b"\n__NEOFLASH_CODESIGN__:"
STAPLE_DMG_MARKER = b"\n__NEOFLASH_STAPLED_TICKET__:dmg\n"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65_536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def snapshot_tree(root: Path) -> dict[str, dict[str, object]]:
    """Capture exact structure, POSIX modes, symlinks, sizes and SHA-256 digests without following links."""
    entries: dict[str, dict[str, object]] = {}
    if not root.exists() and not root.is_symlink():
        return entries

    def walk(current: Path) -> None:
        rel = "." if current == root else current.relative_to(root).as_posix()
        st = current.lstat()
        if stat.S_ISLNK(st.st_mode):
            entries[rel] = {
                "kind": "symlink",
                "target": os.readlink(current),
            }
            return
        if stat.S_ISDIR(st.st_mode):
            entries[rel] = {
                "kind": "dir",
                "mode": stat.S_IMODE(st.st_mode),
            }
            children = sorted(current.iterdir(), key=lambda item: item.name)
            for child in children:
                walk(child)
            return
        entries[rel] = {
            "kind": "file",
            "mode": stat.S_IMODE(st.st_mode),
            "size": st.st_size,
            "sha256": sha256_file(current),
        }

    walk(root)
    return entries


STUB_DRIVER_SOURCE = textwrap.dedent(
    r'''
    #!/usr/bin/env python3
    """Deterministic stateful tool-stub dispatcher for test_notarize_macos_beta.py."""

    from __future__ import annotations

    import hashlib
    import json
    import os
    import shutil
    import signal
    import subprocess
    import sys
    import tarfile
    from pathlib import Path

    CODESIGN_MARKER = b"\n__NEOFLASH_CODESIGN__:"
    STAPLE_DMG_MARKER = b"\n__NEOFLASH_STAPLED_TICKET__:dmg\n"


    def sha256_file(path: Path) -> str:
        h = hashlib.sha256()
        with path.open("rb") as f:
            for chunk in iter(lambda: f.read(65536), b""):
                h.update(chunk)
        return h.hexdigest()


    def state_dir() -> Path:
        p = Path(os.environ["NEOFLASH_TEST_STATE_DIR"])
        p.mkdir(parents=True, exist_ok=True)
        return p


    def record_event(tool: str, stage: str, args: list[str], extra: dict | None = None) -> int:
        log_path = Path(os.environ["NEOFLASH_TEST_EVENT_LOG"])
        log_path.parent.mkdir(parents=True, exist_ok=True)
        entry = {"tool": tool, "stage": stage, "args": args}
        if extra:
            entry.update(extra)
        with log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry, sort_keys=True) + "\n")
        counts_path = state_dir() / "stage_counts.json"
        counts = json.loads(counts_path.read_text(encoding="utf-8")) if counts_path.is_file() else {}
        counts[stage] = int(counts.get(stage, 0)) + 1
        counts_path.write_text(json.dumps(counts, sort_keys=True), encoding="utf-8")
        return counts[stage]


    def should_fail(stage: str, occurrence: int) -> tuple[bool, str]:
        target_stage = os.environ.get("NEOFLASH_TEST_FAIL_STAGE", "")
        if target_stage != stage:
            return False, ""
        target_occ = int(os.environ.get("NEOFLASH_TEST_FAIL_OCCURRENCE", "1"))
        if target_occ > 0 and occurrence != target_occ:
            return False, ""
        return True, os.environ.get("NEOFLASH_TEST_FAIL_MODE", "exit-nonzero")


    def trigger_failure(stage: str, mode: str) -> None:
        if mode == "sigterm-parent":
            os.kill(os.getppid(), signal.SIGTERM)
            sys.exit(0)
        sys.stderr.write(f"Injected hermetic tool failure at stage {stage} ({mode}).\n")
        if stage.startswith("spctl:"):
            sys.exit(3)
        sys.exit(1)


    def load_mounts() -> list[str]:
        p = state_dir() / "active_mounts.json"
        return json.loads(p.read_text(encoding="utf-8")) if p.is_file() else []


    def save_mounts(mounts: list[str]) -> None:
        (state_dir() / "active_mounts.json").write_text(
            json.dumps(mounts, indent=2), encoding="utf-8"
        )


    def strip_codesign_trailer(data: bytes) -> bytes:
        idx = data.find(CODESIGN_MARKER)
        return data[:idx] if idx >= 0 else data


    def sign_binary_file(path: Path, identity: str, timestamp: bool) -> None:
        if not path.is_file():
            sys.stderr.write(f"codesign stub: missing file {path}\n")
            sys.exit(1)
        raw = strip_codesign_trailer(path.read_bytes())
        ts_flag = "1" if timestamp else "0"
        signed = raw + CODESIGN_MARKER + f"id={identity}:ts={ts_flag}\n".encode("utf-8")
        mode = path.stat().st_mode & 0o777
        path.write_bytes(signed)
        path.chmod(mode)


    def verify_signed_binary_file(path: Path, require_dev_id: bool) -> None:
        data = path.read_bytes()
        idx = data.find(CODESIGN_MARKER)
        if idx < 0:
            sys.stderr.write(f"codesign verify failed: unsigned file {path}\n")
            sys.exit(1)
        trailer = data[idx:].decode("utf-8", errors="replace")
        if require_dev_id and ("id=-:" in trailer or "ts=1" not in trailer):
            sys.stderr.write(f"codesign verify failed: expected Developer ID + timestamp on {path}\n")
            sys.exit(1)


    def handle_codesign(args: list[str]) -> None:
        target = Path(args[-1])
        if "--verify" in args:
            stage = "codesign:verify-app" if target.suffix == ".app" else "codesign:verify-dmg"
            occ = record_event("codesign", stage, args)
            fail, mode = should_fail(stage, occ)
            if fail:
                trigger_failure(stage, mode)
            if target.suffix == ".app":
                sig_file = target / "Contents" / "_CodeSignature" / "CodeResources"
                if not sig_file.is_file():
                    sys.stderr.write("codesign verify: missing _CodeSignature/CodeResources\n")
                    sys.exit(1)
                manifest = json.loads(sig_file.read_text(encoding="utf-8"))
                if manifest.get("identity") == "-" or not manifest.get("timestamp"):
                    sys.stderr.write("codesign verify: app bundle is not Developer-ID signed\n")
                    sys.exit(1)
                for rel, expected_sha in manifest["files"].items():
                    file_path = target / rel
                    if not file_path.is_file() or sha256_file(file_path) != expected_sha:
                        sys.stderr.write(f"codesign verify: sealed resource modified: {rel}\n")
                        sys.exit(1)
                for nested in (
                    list((target / "Contents/Resources/media/lib").glob("*.dylib"))
                    + list((target / "Contents/Resources/media/bin").glob("*"))
                    + [target / "Contents/MacOS/neoflash-editor"]
                ):
                    if nested.is_file():
                        verify_signed_binary_file(nested, require_dev_id=True)
            else:
                verify_signed_binary_file(target, require_dev_id=True)
            return

        sign_idx = args.index("--sign")
        identity = args[sign_idx + 1]
        has_timestamp = "--timestamp" in args
        has_runtime = "--options" in args and "runtime" in args
        has_entitlements = "--entitlements" in args

        target_posix = target.as_posix()
        if target.suffix == ".dylib":
            stage = "codesign:nested-dylib"
        elif "/Contents/Resources/media/bin/" in target_posix:
            stage = "codesign:nested-helper"
        elif target_posix.endswith("/Contents/MacOS/neoflash-editor"):
            stage = "codesign:app-executable"
        elif target.suffix == ".app":
            stage = "codesign:app-bundle"
        elif target.suffix == ".dmg":
            stage = "codesign:dmg"
        else:
            stage = "codesign:other"

        occ = record_event("codesign", stage, args, {"identity": identity})
        fail, mode = should_fail(stage, occ)
        if fail and mode not in (
            "corrupt-nested-media-on-app-sign",
            "extra-untracked-media-file-on-helper-sign",
        ):
            trigger_failure(stage, mode)

        if stage in ("codesign:nested-dylib", "codesign:nested-helper", "codesign:app-executable", "codesign:app-bundle"):
            if not has_runtime or not has_timestamp:
                sys.stderr.write(f"codesign stub: {stage} requires --options runtime and --timestamp\n")
                sys.exit(1)
        if stage in ("codesign:nested-helper", "codesign:app-executable", "codesign:app-bundle"):
            if not has_entitlements:
                sys.stderr.write(f"codesign stub: {stage} requires --entitlements\n")
                sys.exit(1)
        if stage == "codesign:dmg" and not has_timestamp:
            sys.stderr.write("codesign stub: dmg signing requires --timestamp\n")
            sys.exit(1)

        if stage == "codesign:app-bundle":
            # Verify inside-out signing order before sealing the outer bundle.
            for nested in (
                list((target / "Contents/Resources/media/lib").glob("*.dylib"))
                + list((target / "Contents/Resources/media/bin").glob("*"))
                + [target / "Contents/MacOS/neoflash-editor"]
            ):
                if nested.is_file():
                    verify_signed_binary_file(nested, require_dev_id=True)
            if fail and mode == "corrupt-nested-media-on-app-sign":
                ffmpeg_bin = target / "Contents/Resources/media/bin/ffmpeg"
                ffmpeg_bin.write_bytes(ffmpeg_bin.read_bytes() + b"CORRUPTED")
            files_map: dict[str, str] = {}
            for p in sorted(target.rglob("*")):
                if p.is_symlink() or not p.is_file():
                    continue
                rel = p.relative_to(target).as_posix()
                if rel.startswith("Contents/_CodeSignature/"):
                    continue
                files_map[rel] = sha256_file(p)
            sig_dir = target / "Contents" / "_CodeSignature"
            sig_dir.mkdir(parents=True, exist_ok=True)
            sig_dir.chmod(0o755)
            sig_file = sig_dir / "CodeResources"
            sig_file.write_text(
                json.dumps(
                    {"identity": identity, "timestamp": has_timestamp, "files": files_map},
                    indent=2,
                    sort_keys=True,
                )
                + "\n",
                encoding="utf-8",
            )
            sig_file.chmod(0o644)
        else:
            sign_binary_file(target, identity, has_timestamp)
            if fail and mode == "extra-untracked-media-file-on-helper-sign":
                rogue = target.parent / "rogue-untracked-helper"
                rogue.write_bytes(b"rogue")
                rogue.chmod(0o755)


    def handle_xcrun(args: list[str]) -> None:
        subtool = args[0]
        rest = args[1:]
        if subtool == "notarytool":
            action = rest[0]
            if action == "submit":
                archive = Path(rest[1])
                stage = "notarytool:submit-app" if archive.suffix == ".zip" else "notarytool:submit-dmg"
                occ = record_event("xcrun", stage, args)
                if not archive.is_file() or archive.stat().st_size == 0:
                    sys.stderr.write(f"notarytool submit: missing or empty archive {archive}\n")
                    sys.exit(1)
                if "--wait" not in rest or "--output-format" not in rest or "json" not in rest:
                    sys.stderr.write("notarytool submit: expected --wait --output-format json\n")
                    sys.exit(1)
                fail, mode = should_fail(stage, occ)
                if fail:
                    if mode == "notary-invalid-exit0":
                        print(
                            json.dumps(
                                {
                                    "id": f"sub-invalid-{stage.split(':')[-1]}-001",
                                    "message": "Processing complete",
                                    "status": "Invalid",
                                }
                            )
                        )
                        sys.exit(0)
                    if mode == "notary-invalid-exit1":
                        print(
                            json.dumps(
                                {
                                    "id": f"sub-invalid-{stage.split(':')[-1]}-002",
                                    "message": "Processing complete",
                                    "status": "Invalid",
                                }
                            )
                        )
                        sys.exit(1)
                    if mode == "notary-malformed-json":
                        print("{not-valid-json")
                        sys.exit(0)
                    trigger_failure(stage, mode)

                notarized_file = state_dir() / "notarized.json"
                notarized = (
                    json.loads(notarized_file.read_text(encoding="utf-8"))
                    if notarized_file.is_file()
                    else []
                )
                notarized.append(stage)
                notarized_file.write_text(json.dumps(notarized), encoding="utf-8")
                print(
                    json.dumps(
                        {
                            "id": f"sub-accepted-{stage.split(':')[-1]}-{occ:03d}",
                            "message": "Processing complete",
                            "status": "Accepted",
                        }
                    )
                )
                return

            if action == "log":
                submission_id = rest[1]
                record_event("xcrun", "notarytool:log", args, {"submission_id": submission_id})
                print(
                    json.dumps(
                        {
                            "jobId": submission_id,
                            "status": "Invalid",
                            "statusSummary": "Archive contains critical validation errors",
                            "issues": [
                                {
                                    "severity": "error",
                                    "path": "NeoFlash.app/Contents/MacOS/neoflash-editor",
                                    "message": "The binary is not signed with a valid Developer ID certificate.",
                                }
                            ],
                        }
                    )
                )
                return

        if subtool == "stapler":
            action = rest[0]
            target = Path(rest[1])
            kind = "app" if target.suffix == ".app" else "dmg"
            stage = f"stapler:{action}-{kind}"
            occ = record_event("xcrun", stage, args)
            fail, mode = should_fail(stage, occ)
            if fail:
                trigger_failure(stage, mode)
            notarized_file = state_dir() / "notarized.json"
            notarized = (
                json.loads(notarized_file.read_text(encoding="utf-8"))
                if notarized_file.is_file()
                else []
            )
            required_submit = f"notarytool:submit-{kind}"
            if action == "staple":
                if required_submit not in notarized:
                    sys.stderr.write(f"stapler: {target} was not notarized before stapling\n")
                    sys.exit(1)
                if kind == "app":
                    ticket = target / "Contents" / "_CodeSignature" / "CodeTicket"
                    ticket.write_text("NEOFLASH_STAPLED_APP_TICKET_V1\n", encoding="utf-8")
                    ticket.chmod(0o644)
                else:
                    data = target.read_bytes()
                    if STAPLE_DMG_MARKER not in data:
                        target.write_bytes(data + STAPLE_DMG_MARKER)
                return
            if action == "validate":
                if kind == "app":
                    ticket = target / "Contents" / "_CodeSignature" / "CodeTicket"
                    if not ticket.is_file():
                        sys.stderr.write("stapler validate: missing app CodeTicket\n")
                        sys.exit(1)
                else:
                    if STAPLE_DMG_MARKER not in target.read_bytes():
                        sys.stderr.write("stapler validate: missing dmg ticket\n")
                        sys.exit(1)
                return

        sys.stderr.write(f"xcrun stub: unsupported invocation {args}\n")
        sys.exit(1)


    def handle_hdiutil(args: list[str]) -> None:
        verb = args[0]
        if verb == "create":
            occ = record_event("hdiutil", "hdiutil:create", args)
            fail, mode = should_fail("hdiutil:create", occ)
            if fail:
                trigger_failure("hdiutil:create", mode)
            src_idx = args.index("-srcfolder")
            src_folder = Path(args[src_idx + 1])
            out_rw = Path(args[-1])
            app_in_dmg = src_folder / "NeoFlash.app"
            app_link = src_folder / "Applications"
            notice = src_folder / "Licenses and source.txt"
            vol_icon = src_folder / ".VolumeIcon.icns"
            ticket = app_in_dmg / "Contents" / "_CodeSignature" / "CodeTicket"
            if (
                not app_in_dmg.is_dir()
                or not ticket.is_file()
                or not notice.is_file()
                or not vol_icon.is_file()
                or not app_link.is_symlink()
                or os.readlink(app_link) != "/Applications"
            ):
                sys.stderr.write("hdiutil create: incomplete or unstapled DMG staging folder\n")
                sys.exit(1)
            payload_tar = out_rw.with_suffix(".tar")
            with tarfile.open(payload_tar, "w") as tf:
                for item in sorted(src_folder.iterdir(), key=lambda p: p.name):
                    tf.add(item, arcname=item.name, recursive=True)
            out_rw.write_bytes(b"NEOFLASH_UDRW_IMAGE\n" + payload_tar.read_bytes())
            payload_tar.unlink()
            return

        if verb == "attach":
            occ = record_event("hdiutil", "hdiutil:attach", args)
            mnt_idx = args.index("-mountpoint")
            mnt = Path(args[mnt_idx + 1])
            fail, mode = should_fail("hdiutil:attach", occ)
            if fail:
                trigger_failure("hdiutil:attach", mode)
            rw_img = Path(args[1])
            if not rw_img.is_file():
                sys.stderr.write("hdiutil attach: missing UDRW image\n")
                sys.exit(1)
            sentinel = mnt / ".mounted-volume-sentinel"
            sentinel.write_text(str(rw_img), encoding="utf-8")
            mounts = load_mounts()
            if str(mnt) not in mounts:
                mounts.append(str(mnt))
            save_mounts(mounts)
            return

        if verb == "detach":
            force = "-force" in args
            mnt = Path(args[-1])
            stage = "hdiutil:detach-force" if force else "hdiutil:detach"
            occ = record_event("hdiutil", stage, args)
            if not force:
                fail_soft, mode_soft = should_fail("hdiutil:detach-soft", occ)
                if fail_soft:
                    trigger_failure("hdiutil:detach-soft", mode_soft)
            fail_all, mode_all = should_fail("hdiutil:detach-all", occ)
            if fail_all:
                trigger_failure("hdiutil:detach-all", mode_all)
            sentinel = mnt / ".mounted-volume-sentinel"
            if sentinel.exists():
                sentinel.unlink()
            mounts = [m for m in load_mounts() if m != str(mnt)]
            save_mounts(mounts)
            return

        if verb == "convert":
            occ = record_event("hdiutil", "hdiutil:convert", args)
            fail, mode = should_fail("hdiutil:convert", occ)
            if fail:
                trigger_failure("hdiutil:convert", mode)
            if load_mounts():
                sys.stderr.write("hdiutil convert: volume still mounted during convert\n")
                sys.exit(1)
            rw_img = Path(args[1])
            out_idx = args.index("-o")
            out_dmg = Path(args[out_idx + 1])
            raw = rw_img.read_bytes()
            if not raw.startswith(b"NEOFLASH_UDRW_IMAGE\n"):
                sys.stderr.write("hdiutil convert: invalid UDRW input\n")
                sys.exit(1)
            out_dmg.write_bytes(b"NEOFLASH_UDZO_IMAGE\n" + raw[len(b"NEOFLASH_UDRW_IMAGE\n") :])
            out_dmg.chmod(0o644)
            return

        if verb == "verify":
            occ = record_event("hdiutil", "hdiutil:verify", args)
            fail, mode = should_fail("hdiutil:verify", occ)
            if fail:
                trigger_failure("hdiutil:verify", mode)
            dmg = Path(args[-1])
            data = dmg.read_bytes()
            if not data.startswith(b"NEOFLASH_UDZO_IMAGE\n"):
                sys.stderr.write("hdiutil verify: invalid UDZO header\n")
                sys.exit(1)
            return

        sys.stderr.write(f"hdiutil stub: unsupported verb {verb}\n")
        sys.exit(1)


    def handle_spctl(args: list[str]) -> None:
        target = Path(args[-1])
        stage = "spctl:app" if target.suffix == ".app" else "spctl:dmg"
        occ = record_event("spctl", stage, args)
        fail, mode = should_fail(stage, occ)
        if fail:
            trigger_failure(stage, mode)
        if stage == "spctl:app":
            if "--type" not in args or "execute" not in args:
                sys.stderr.write("spctl app: expected --type execute\n")
                sys.exit(1)
            ticket = target / "Contents" / "_CodeSignature" / "CodeTicket"
            sig = target / "Contents" / "_CodeSignature" / "CodeResources"
            if not ticket.is_file() or not sig.is_file():
                sys.stderr.write("spctl app: rejected unstapled or unsigned app\n")
                sys.exit(3)
        else:
            if "--type" not in args or "open" not in args or "context:primary-signature" not in args:
                sys.stderr.write("spctl dmg: expected --type open --context context:primary-signature\n")
                sys.exit(1)
            data = target.read_bytes()
            if CODESIGN_MARKER not in data or STAPLE_DMG_MARKER not in data:
                sys.stderr.write("spctl dmg: rejected unstapled or unsigned dmg\n")
                sys.exit(3)


    def handle_ditto(args: list[str]) -> None:
        if "-c" in args:
            # 1st zip is pre-staple submission zip; 2nd zip is post-staple distributable zip.
            src_app = Path(args[-2])
            has_ticket = (src_app / "Contents" / "_CodeSignature" / "CodeTicket").is_file()
            stage = "ditto:final-zip" if has_ticket else "ditto:submit-zip"
        else:
            dst = Path(args[-1])
            stage = "ditto:dmg-stage-app" if "dmg-stage" in dst.as_posix() else "ditto:stage-app"
        occ = record_event("ditto", stage, args)
        fail, mode = should_fail(stage, occ)
        if fail:
            trigger_failure(stage, mode)
        rc = subprocess.call(["/usr/bin/ditto", *args])
        sys.exit(rc)


    def handle_mv(args: list[str]) -> None:
        dst_path = Path(args[-1])
        dst = dst_path.as_posix()
        if "/incoming/" in dst:
            stage = "mv:incoming-stage"
        elif "/backup/" in dst:
            stage = "mv:backup-step"
        else:
            stage = "mv:install-step"
        occ = record_event("mv", stage, args)
        fail, mode = should_fail(stage, occ)
        if fail and mode != "corrupt-installed-dmg-post-swap":
            trigger_failure(stage, mode)
        rc = subprocess.call(["/bin/mv", *args])
        if rc == 0 and fail and mode == "corrupt-installed-dmg-post-swap":
            for sibling in dst_path.parent.glob("*.dmg"):
                sibling.write_bytes(sibling.read_bytes() + b"POST_SWAP_BIT_ROT")
        sys.exit(rc)


    def handle_setfile(args: list[str]) -> None:
        occ = record_event("SetFile", "setfile:custom-icon", args)
        fail, mode = should_fail("setfile:custom-icon", occ)
        if fail:
            trigger_failure("setfile:custom-icon", mode)
        # Force fallback to xattr -wx com.apple.FinderInfo so both paths are testable.
        sys.exit(1)


    def handle_xattr(args: list[str]) -> None:
        stage = "xattr:finder-info" if "-wx" in args else "xattr:scrub"
        occ = record_event("xattr", stage, args)
        fail, mode = should_fail(stage, occ)
        if fail:
            trigger_failure(stage, mode)
        if stage == "xattr:finder-info":
            mnt = Path(args[-1])
            if not (mnt / ".mounted-volume-sentinel").is_file():
                sys.stderr.write("xattr finder-info: DMG is not mounted at target\n")
                sys.exit(1)
            return
        rc = subprocess.call(["/usr/bin/xattr", *args])
        sys.exit(rc)


    def handle_rust_licenses(args: list[str]) -> None:
        occ = record_event("stage-rust-licenses", "rust-licenses:stage", args)
        fail, mode = should_fail("rust-licenses:stage", occ)
        if fail:
            trigger_failure("rust-licenses:stage", mode)
        dest = Path(args[args.index("--dest") + 1])
        dest.mkdir(parents=True, exist_ok=True)
        dest.chmod(0o755)
        pkg_dir = dest / "packages" / "neoflash-editor-0.1.0-beta.1"
        pkg_dir.mkdir(parents=True, exist_ok=True)
        pkg_dir.chmod(0o755)
        (dest / "packages").chmod(0o755)
        lic = pkg_dir / "01-LICENSE-MIT"
        lic.write_text("MIT license fixture for hermetic notarization pipeline test.\n", encoding="utf-8")
        lic.chmod(0o644)
        manifest = {
            "schema_version": 1,
            "root_package": "neoflash-editor",
            "package_count": 1,
            "packages": [{"name": "neoflash-editor", "version": "0.1.0-beta.1", "spdx": "MIT OR Apache-2.0"}],
        }
        manifest_json = dest / "rust-dependency-licenses.json"
        manifest_json.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        manifest_json.chmod(0o644)
        manifest_md = dest / "rust-dependency-licenses.md"
        manifest_md.write_text("# Rust runtime dependency licenses\n", encoding="utf-8")
        manifest_md.chmod(0o644)


    def main() -> None:
        tool = sys.argv[1]
        args = sys.argv[2:]
        dispatch = {
            "codesign": handle_codesign,
            "xcrun": handle_xcrun,
            "hdiutil": handle_hdiutil,
            "spctl": handle_spctl,
            "ditto": handle_ditto,
            "mv": handle_mv,
            "SetFile": handle_setfile,
            "xattr": handle_xattr,
            "rust-licenses": handle_rust_licenses,
        }
        dispatch[tool](args)


    if __name__ == "__main__":
        main()
    '''
).lstrip()


class NotarizeMacosBetaTransactionTests(unittest.TestCase):
    """Transaction-safety and failure-injection tests for scripts/notarize-macos-beta.sh."""

    @classmethod
    def setUpClass(cls) -> None:
        cls._template_tmp = tempfile.TemporaryDirectory(prefix="neoflash-notarize-tpl-")
        tpl_root = Path(cls._template_tmp.name)
        cls.stubs_dir = tpl_root / "stubs"
        cls.stubs_dir.mkdir(parents=True)
        cls._install_stubs(cls.stubs_dir)

        cls.template_env_root = tpl_root / "template"
        cls.template_env_root.mkdir()
        cls._build_template_fixture(cls.template_env_root)

    @classmethod
    def tearDownClass(cls) -> None:
        cls._template_tmp.cleanup()

    @classmethod
    def _install_stubs(cls, stubs_dir: Path) -> None:
        driver = stubs_dir / "stub_driver.py"
        driver.write_text(STUB_DRIVER_SOURCE, encoding="utf-8")
        driver.chmod(0o755)
        python_exe = sys.executable
        for tool in (
            "codesign",
            "xcrun",
            "hdiutil",
            "spctl",
            "ditto",
            "mv",
            "SetFile",
            "xattr",
        ):
            wrapper = stubs_dir / tool
            wrapper.write_text(
                f'#!/bin/sh\nexec "{python_exe}" "{driver}" "{tool}" "$@"\n',
                encoding="utf-8",
            )
            wrapper.chmod(0o755)

        rust_lic_py = stubs_dir / "rust-licenses.py"
        rust_lic_py.write_text(
            "#!/usr/bin/env python3\n"
            "import runpy, sys\n"
            f"sys.argv = [sys.argv[0], 'rust-licenses', *sys.argv[1:]]\n"
            f"runpy.run_path({str(driver)!r}, run_name='__main__')\n",
            encoding="utf-8",
        )
        rust_lic_py.chmod(0o755)

    @classmethod
    def _build_template_fixture(cls, base: Path) -> None:
        """Build a realistic clean Git repo, Cellar provenance fixture, and 5-artifact dist/ set."""
        repo = base / "repo"
        repo.mkdir(parents=True)
        (repo / ".gitignore").write_text("/dist/\n/target/\n", encoding="utf-8")

        entitlements_dst = repo / "scripts" / "macos-entitlements.plist"
        entitlements_dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ENTITLEMENTS_SRC, entitlements_dst)

        icon_dst = repo / "crates" / "neoflash-editor" / "assets" / "app-icon" / "NeoFlash.icns"
        icon_dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ICON_SRC, icon_dst)

        subprocess.run(["git", "init", "--quiet", str(repo)], check=True)
        subprocess.run(["git", "-C", str(repo), "config", "user.email", "test@neoflash.invalid"], check=True)
        subprocess.run(["git", "-C", str(repo), "config", "user.name", "NeoFlash Test"], check=True)
        subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
        subprocess.run(["git", "-C", str(repo), "commit", "--quiet", "-m", "fixture head"], check=True)

        # Synthetic upstream archives + Homebrew Cellar for real bundled-media-provenance.py execution.
        upstream_root = base / "upstream"
        cellar_root = base / "Cellar"
        media_cache = base / "media-cache"
        media_cache.mkdir(parents=True)

        formula_hashes: dict[str, tuple[Path, str]] = {}
        for formula, ver, lic_name, lic_body in (
            ("ffmpeg", "7.1", "COPYING.GPLv3", "GNU GENERAL PUBLIC LICENSE Version 3 fixture\n"),
            ("dav1d", "1.5", "COPYING", "BSD 2-Clause License fixture\n"),
        ):
            src_tree = upstream_root / f"{formula}-{ver}"
            src_tree.mkdir(parents=True)
            (src_tree / lic_name).write_text(lic_body, encoding="utf-8")
            (src_tree / f"{formula}.c").write_text("int neoflash_media_stub(void) { return 0; }\n", encoding="utf-8")
            tar_path = upstream_root / f"{formula}-{ver}.tar.gz"
            with tarfile.open(tar_path, "w:gz") as tf:
                tf.add(src_tree, arcname=f"{formula}-{ver}")
            src_sha = sha256_file(tar_path)
            formula_hashes[formula] = (tar_path, src_sha)

            keg = cellar_root / formula / ver
            (keg / ".brew").mkdir(parents=True)
            (keg / lic_name).write_text(lic_body, encoding="utf-8")
            receipt = {
                "homebrew_version": "4.4.0-test",
                "arch": "arm64",
                "built_as_bottle": True,
                "poured_from_bottle": True,
                "source": {
                    "versions": {"stable": ver},
                    "path": "/opt/homebrew/Library/Taps/packages.arm64_sequoia.jws.json",
                },
            }
            (keg / "INSTALL_RECEIPT.json").write_text(json.dumps(receipt), encoding="utf-8")
            spdx = {
                "packages": [
                    {
                        "SPDXID": f"SPDXRef-Archive-{formula}-src",
                        "name": formula,
                        "versionInfo": ver,
                        "licenseConcluded": "GPL-3.0-or-later" if formula == "ffmpeg" else "BSD-2-Clause",
                        "downloadLocation": tar_path.as_uri(),
                        "checksums": [{"algorithm": "SHA256", "checksumValue": src_sha}],
                    },
                    {
                        "SPDXID": f"SPDXRef-Bottle-{formula}",
                        "name": formula,
                        "versionInfo": ver,
                        "licenseConcluded": "GPL-3.0-or-later" if formula == "ffmpeg" else "BSD-2-Clause",
                        "downloadLocation": f"https://example.invalid/{formula}-{ver}.arm64_sequoia.bottle.tar.gz",
                        "checksums": [{"algorithm": "SHA256", "checksumValue": ("a" if formula == "ffmpeg" else "b") * 64}],
                    },
                ]
            }
            (keg / "sbom.spdx.json").write_text(json.dumps(spdx), encoding="utf-8")
            (keg / ".brew" / f"{formula}.rb").write_text(
                f'class {formula.capitalize()} < Formula\n'
                f'  url "{tar_path.as_uri()}"\n'
                f'  sha256 "{src_sha}"\n'
                f'  license "GPL-3.0-or-later"\n'
                f"end\n",
                encoding="utf-8",
            )

        # Build realistic 5-artifact dist/ directory.
        dist = repo / "dist"
        app = dist / "NeoFlash.app"
        macos_dir = app / "Contents" / "MacOS"
        res_dir = app / "Contents" / "Resources"
        runtime_dir = res_dir / "runtime"
        lic_nf_dir = res_dir / "licenses" / "neoflash"
        lic_rust_dir = res_dir / "licenses" / "rust"
        media_bin = res_dir / "media" / "bin"
        media_lib = res_dir / "media" / "lib"
        media_lic = res_dir / "media" / "licenses"
        media_prov = res_dir / "media" / "provenance"

        for d in (
            macos_dir,
            runtime_dir,
            lic_nf_dir,
            lic_rust_dir,
            media_bin,
            media_lib,
            media_lic,
            media_prov,
        ):
            d.mkdir(parents=True, exist_ok=True)

        def adhoc_macho(label: str) -> bytes:
            return MACHO_ARM64_HEADER + f"neoflash-{label}-arm64-payload".encode("utf-8") + CODESIGN_MARKER + b"id=-:ts=0\n"

        editor_bin = macos_dir / "neoflash-editor"
        editor_bin.write_bytes(adhoc_macho("editor"))
        editor_bin.chmod(0o755)

        ffmpeg_bin = media_bin / "ffmpeg"
        ffmpeg_bin.write_bytes(adhoc_macho("ffmpeg"))
        ffmpeg_bin.chmod(0o755)

        ffprobe_bin = media_bin / "ffprobe"
        ffprobe_bin.write_bytes(adhoc_macho("ffprobe"))
        ffprobe_bin.chmod(0o755)

        avcodec_dylib = media_lib / "libavcodec.61.dylib"
        avcodec_dylib.write_bytes(adhoc_macho("libavcodec"))
        avcodec_dylib.chmod(0o755)

        dav1d_dylib = media_lib / "libdav1d.7.dylib"
        dav1d_dylib.write_bytes(adhoc_macho("libdav1d"))
        dav1d_dylib.chmod(0o755)

        (runtime_dir / "neoflash_web.js").write_text("export function boot() { return true; }\n", encoding="utf-8")
        (runtime_dir / "neoflash_web_bg.wasm").write_bytes(b"\x00asm\x01\x00\x00\x00")
        shutil.copy2(ICON_SRC, res_dir / "NeoFlash.icns")
        shutil.copy2(ICON_SRC, res_dir / "AppIcon.icns")
        (res_dir / "THIRD_PARTY_NOTICES.txt").write_text("Third-party notices fixture.\n", encoding="utf-8")
        (res_dir / "LICENSES-AND-SOURCE.txt").write_text(
            f"NeoFlash {VERSION} — Licenses and bundled-media source\n  {SOURCE_NAME}\n  {SUMS_NAME}\n",
            encoding="utf-8",
        )
        (lic_nf_dir / "LICENSE-MIT").write_text("MIT license fixture.\n", encoding="utf-8")
        (lic_nf_dir / "LICENSE-APACHE").write_text("Apache-2.0 license fixture.\n", encoding="utf-8")
        # Internal relative symlink inside NeoFlash.app to prove symlink preservation on rollback & swap.
        os.symlink("neoflash/LICENSE-MIT", res_dir / "licenses" / "PRIMARY-LICENSE")
        (lic_rust_dir / "rust-dependency-licenses.json").write_text('{"initial": true}\n', encoding="utf-8")
        (media_lic / "FFmpeg-COPYING.GPLv3.txt").write_text("GPLv3 fixture.\n", encoding="utf-8")

        ffmpeg_tar, _ = formula_hashes["ffmpeg"]
        media_sbom = {
            "schema_version": 1,
            "executables": [
                {
                    "name": "ffmpeg",
                    "relative_path": "bin/ffmpeg",
                    "version": "7.1",
                    "sha256": sha256_file(ffmpeg_bin),
                    "license": "GPL-3.0-or-later",
                    "source_url": ffmpeg_tar.as_uri(),
                },
                {
                    "name": "ffprobe",
                    "relative_path": "bin/ffprobe",
                    "version": "7.1",
                    "sha256": sha256_file(ffprobe_bin),
                    "license": "GPL-3.0-or-later",
                    "source_url": ffmpeg_tar.as_uri(),
                },
            ],
            "libraries": [
                {
                    "name": "libavcodec.61.dylib",
                    "relative_path": "lib/libavcodec.61.dylib",
                    "source_formula": "ffmpeg",
                    "version": "7.1",
                    "sha256": sha256_file(avcodec_dylib),
                },
                {
                    "name": "libdav1d.7.dylib",
                    "relative_path": "lib/libdav1d.7.dylib",
                    "source_formula": "dav1d",
                    "version": "1.5",
                    "sha256": sha256_file(dav1d_dylib),
                },
            ],
        }
        (media_prov / "media-sbom.json").write_text(
            json.dumps(media_sbom, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        (media_prov / "SHA256SUMS.txt").write_text(
            "\n".join(
                sorted(
                    [
                        f"{sha256_file(ffmpeg_bin)}  bin/ffmpeg",
                        f"{sha256_file(ffprobe_bin)}  bin/ffprobe",
                        f"{sha256_file(avcodec_dylib)}  lib/libavcodec.61.dylib",
                        f"{sha256_file(dav1d_dylib)}  lib/libdav1d.7.dylib",
                    ]
                )
            )
            + "\n",
            encoding="utf-8",
        )

        info_plist = {
            "CFBundleName": "NeoFlash",
            "CFBundleDisplayName": "NeoFlash",
            "CFBundleIdentifier": "org.neoflash.editor",
            "CFBundleExecutable": "neoflash-editor",
            "CFBundleIconFile": "NeoFlash",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": VERSION,
            "CFBundleVersion": "394",
            "NeoFlashGitCommit": "83ca3e88f8a7bd0475d1c4ab4dcb8ff29184b856",
            "NeoFlashBuildProfile": "release",
            "LSMinimumSystemVersion": "15.0",
            "NSHighResolutionCapable": True,
            "NSPrincipalClass": "NSApplication",
        }
        plist_path = app / "Contents" / "Info.plist"
        plist_path.write_bytes(plistlib.dumps(info_plist))
        subprocess.run(["plutil", "-lint", str(plist_path)], check=True, stdout=subprocess.DEVNULL)

        # Normalize modes just like package-macos.sh.
        subprocess.run(["xattr", "-cr", str(app)], check=True)
        for root_dir, dirnames, filenames in os.walk(app):
            rp = Path(root_dir)
            rp.chmod(0o755)
            for dn in dirnames:
                dp = rp / dn
                if not dp.is_symlink():
                    dp.chmod(0o755)
            for fn in filenames:
                fp = rp / fn
                if fp.is_symlink():
                    continue
                if fp.parent in (macos_dir, media_bin, media_lib):
                    fp.chmod(0o755)
                else:
                    fp.chmod(0o644)

        # Build initial corresponding-source .tar.xz via the real bundled-media-provenance.py.
        init_src_dir = base / "init-source-bundle"
        source_archive = dist / SOURCE_NAME
        subprocess.run(
            [
                sys.executable,
                str(PROVENANCE_SCRIPT),
                "--media-root",
                str(res_dir / "media"),
                "--cellar",
                str(cellar_root),
                "--cache-dir",
                str(media_cache),
                "--output-dir",
                str(init_src_dir),
                "--archive",
                str(source_archive),
                "--download",
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        shutil.rmtree(init_src_dir)
        source_archive.chmod(0o644)

        # Build initial ad-hoc .zip and .dmg and SHA256SUMS.txt.
        zip_path = dist / ZIP_NAME
        subprocess.run(
            ["/usr/bin/ditto", "-c", "-k", "--norsrc", "--keepParent", str(app), str(zip_path)],
            check=True,
        )
        zip_path.chmod(0o644)

        dmg_path = dist / DMG_NAME
        dmg_path.write_bytes(b"NEOFLASH_UDZO_IMAGE\ninitial-adhoc-dmg-payload" + CODESIGN_MARKER + b"id=-:ts=0\n")
        dmg_path.chmod(0o644)

        sums_out = subprocess.check_output(
            ["shasum", "-a", "256", ZIP_NAME, DMG_NAME, SOURCE_NAME],
            cwd=dist,
            text=True,
        )
        sums_path = dist / SUMS_NAME
        sums_path.write_text(sums_out, encoding="utf-8")
        sums_path.chmod(0o644)
        subprocess.run(
            ["shasum", "-a", "256", "-c", SUMS_NAME],
            cwd=dist,
            check=True,
            stdout=subprocess.DEVNULL,
        )

    def setUp(self) -> None:
        self._case_tmp = tempfile.TemporaryDirectory(prefix="neoflash-notarize-case-")
        self.addCleanup(self._case_tmp.cleanup)
        self.case_root = Path(self._case_tmp.name)
        self.fixture_root = self.case_root / "env"
        shutil.copytree(self.template_env_root, self.fixture_root, symlinks=True)

        self.repo_dir = self.fixture_root / "repo"
        self.dist_dir = self.repo_dir / "dist"
        self.cellar_dir = self.fixture_root / "Cellar"
        self.media_cache_dir = self.fixture_root / "media-cache"
        self.work_parent = self.case_root / "work-parent"
        self.mount_parent = self.case_root / "mount-parent"
        self.state_dir = self.case_root / "stub-state"
        self.event_log = self.case_root / "events.jsonl"

    def base_env(self, **overrides: str | None) -> dict[str, str]:
        env = os.environ.copy()
        env.update(
            {
                "NEOFLASH_APPLE_DEVELOPER_ID": DEV_ID,
                "NEOFLASH_NOTARY_PROFILE": NOTARY_PROFILE,
                "NEOFLASH_NOTARIZE_PROJECT_ROOT": str(self.repo_dir),
                "NEOFLASH_DIST_DIR": str(self.dist_dir),
                "NEOFLASH_NOTARIZE_WORK_PARENT": str(self.work_parent),
                "NEOFLASH_NOTARIZE_MOUNT_PARENT": str(self.mount_parent),
                "NEOFLASH_CODESIGN_BIN": str(self.stubs_dir / "codesign"),
                "NEOFLASH_XCRUN_BIN": str(self.stubs_dir / "xcrun"),
                "NEOFLASH_HDIUTIL_BIN": str(self.stubs_dir / "hdiutil"),
                "NEOFLASH_SPCTL_BIN": str(self.stubs_dir / "spctl"),
                "NEOFLASH_DITTO_BIN": str(self.stubs_dir / "ditto"),
                "NEOFLASH_MV_BIN": str(self.stubs_dir / "mv"),
                "NEOFLASH_SETFILE_BIN": str(self.stubs_dir / "SetFile"),
                "NEOFLASH_XATTR_BIN": str(self.stubs_dir / "xattr"),
                "NEOFLASH_PYTHON3_BIN": sys.executable,
                "NEOFLASH_STAGE_RUST_LICENSES_SCRIPT": str(self.stubs_dir / "rust-licenses.py"),
                "NEOFLASH_BUNDLED_MEDIA_PROVENANCE_SCRIPT": str(PROVENANCE_SCRIPT),
                "NEOFLASH_MEDIA_SOURCE_CACHE": str(self.media_cache_dir),
                "NEOFLASH_HOMEBREW_CELLAR": str(self.cellar_dir),
                "NEOFLASH_TEST_STATE_DIR": str(self.state_dir),
                "NEOFLASH_TEST_EVENT_LOG": str(self.event_log),
            }
        )
        for key in ("NEOFLASH_TEST_FAIL_STAGE", "NEOFLASH_TEST_FAIL_OCCURRENCE", "NEOFLASH_TEST_FAIL_MODE"):
            env.pop(key, None)
        for key, value in overrides.items():
            if value is None:
                env.pop(key, None)
            else:
                env[key] = value
        return env

    def run_pipeline(self, args: list[str] | None = None, **env_overrides: str | None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["/bin/bash", str(NOTARIZE_SCRIPT), *(args or [])],
            env=self.base_env(**env_overrides),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )

    def read_events(self) -> list[dict[str, object]]:
        if not self.event_log.is_file():
            return []
        return [
            json.loads(line)
            for line in self.event_log.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]

    def assert_clean_mounts_and_temps(self) -> None:
        mounts_file = self.state_dir / "active_mounts.json"
        if mounts_file.is_file():
            self.assertEqual(
                json.loads(mounts_file.read_text(encoding="utf-8")),
                [],
                "Expected all attached DMG mounts to be detached on exit",
            )
        if self.mount_parent.exists():
            self.assertEqual(
                list(self.mount_parent.iterdir()),
                [],
                "Expected zero leftover DMG mount directories",
            )
        if self.work_parent.exists():
            self.assertEqual(
                list(self.work_parent.iterdir()),
                [],
                "Expected zero leftover notarization staging directories",
            )

    def assert_dist_exact_five_entries(self) -> None:
        self.assertEqual(
            sorted(p.name for p in self.dist_dir.iterdir()),
            sorted(["NeoFlash.app", ZIP_NAME, DMG_NAME, SOURCE_NAME, SUMS_NAME]),
        )

    def test_hermetic_end_to_end_success_replaces_all_five_artifacts_atomically(self) -> None:
        before_app_bin = sha256_file(self.dist_dir / "NeoFlash.app/Contents/MacOS/neoflash-editor")
        before_ffmpeg = sha256_file(self.dist_dir / "NeoFlash.app/Contents/Resources/media/bin/ffmpeg")
        before_zip = sha256_file(self.dist_dir / ZIP_NAME)
        before_dmg = sha256_file(self.dist_dir / DMG_NAME)
        before_sums = (self.dist_dir / SUMS_NAME).read_text(encoding="utf-8")

        result = self.run_pipeline()
        self.assertEqual(result.returncode, 0, f"Pipeline failed:\nSTDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}")
        self.assertIn(
            "Simulated notarization pipeline complete (tool overrides active; not real Apple Notary acceptance)",
            result.stdout,
        )
        self.assertNotIn("Notarized and verified complete release set:", result.stdout)

        self.assert_dist_exact_five_entries()
        self.assert_clean_mounts_and_temps()

        # 1. Verify NeoFlash.app signatures, stapled ticket, refreshed media SBOM, licenses, and internal symlink.
        app_dir = self.dist_dir / "NeoFlash.app"
        self.assertTrue((app_dir / "Contents/_CodeSignature/CodeResources").is_file())
        self.assertTrue((app_dir / "Contents/_CodeSignature/CodeTicket").is_file())
        self.assertEqual(
            os.readlink(app_dir / "Contents/Resources/licenses/PRIMARY-LICENSE"),
            "neoflash/LICENSE-MIT",
        )
        rust_manifest = json.loads(
            (app_dir / "Contents/Resources/licenses/rust/rust-dependency-licenses.json").read_text(encoding="utf-8")
        )
        self.assertEqual(rust_manifest["root_package"], "neoflash-editor")

        after_app_bin = sha256_file(app_dir / "Contents/MacOS/neoflash-editor")
        after_ffmpeg = sha256_file(app_dir / "Contents/Resources/media/bin/ffmpeg")
        self.assertNotEqual(before_app_bin, after_app_bin)
        self.assertNotEqual(before_ffmpeg, after_ffmpeg)

        media_sbom = json.loads(
            (app_dir / "Contents/Resources/media/provenance/media-sbom.json").read_text(encoding="utf-8")
        )
        sbom_hashes = {item["relative_path"]: item["sha256"] for item in media_sbom["executables"]}
        self.assertEqual(sbom_hashes["bin/ffmpeg"], after_ffmpeg)

        # 2. Verify distributable .zip extracts cleanly with stapled ticket and zero __MACOSX forks.
        after_zip = sha256_file(self.dist_dir / ZIP_NAME)
        self.assertNotEqual(before_zip, after_zip)
        with zipfile.ZipFile(self.dist_dir / ZIP_NAME) as zf:
            names = zf.namelist()
            self.assertFalse(any(name.startswith("__MACOSX") for name in names))
            self.assertIn("NeoFlash.app/Contents/_CodeSignature/CodeTicket", names)
            self.assertIn("NeoFlash.app/Contents/_CodeSignature/CodeResources", names)

        # 3. Verify .dmg carries UDZO payload, Developer ID signature, and stapled ticket.
        dmg_bytes = (self.dist_dir / DMG_NAME).read_bytes()
        after_dmg = sha256_bytes(dmg_bytes)
        self.assertNotEqual(before_dmg, after_dmg)
        self.assertTrue(dmg_bytes.startswith(b"NEOFLASH_UDZO_IMAGE\n"))
        self.assertIn(CODESIGN_MARKER + f"id={DEV_ID}:ts=1\n".encode("utf-8"), dmg_bytes)
        self.assertIn(STAPLE_DMG_MARKER, dmg_bytes)

        # 4. Verify corresponding-source .tar.xz preserves upstream sources & records signed nested media hashes.
        source_path = self.dist_dir / SOURCE_NAME
        self.assertTrue(tarfile.is_tarfile(source_path))
        with tarfile.open(source_path, "r:xz") as tf:
            member_names = tf.getnames()
            self.assertTrue(any(name.endswith("/manifest.json") for name in member_names))
            self.assertTrue(any("components/ffmpeg/7.1/sources/" in name for name in member_names))
            self.assertTrue(any("components/dav1d/1.5/sources/" in name for name in member_names))
            manifest_member = next(m for m in tf.getmembers() if m.name.endswith("/manifest.json"))
            src_manifest = json.loads(tf.extractfile(manifest_member).read().decode("utf-8"))
            ffmpeg_comp = next(c for c in src_manifest["components"] if c["formula"] == "ffmpeg")
            staged_map = {item["relative_path"]: item["sha256"] for item in ffmpeg_comp["staged_files"]}
            self.assertEqual(staged_map["bin/ffmpeg"], after_ffmpeg)

        # 5. Verify regenerated SHA256SUMS.txt passes `shasum -a 256 -c` and matches all 3 archives.
        after_sums = (self.dist_dir / SUMS_NAME).read_text(encoding="utf-8")
        self.assertNotEqual(before_sums, after_sums)
        subprocess.run(
            ["shasum", "-a", "256", "-c", SUMS_NAME],
            cwd=self.dist_dir,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        expected_sums = subprocess.check_output(
            ["shasum", "-a", "256", ZIP_NAME, DMG_NAME, SOURCE_NAME],
            cwd=self.dist_dir,
            text=True,
        )
        self.assertEqual(after_sums, expected_sums)

        # 6. Verify tool execution order included both notarytool submissions, both staples, and both spctl checks.
        stages = [event["stage"] for event in self.read_events()]
        for required_stage in (
            "codesign:nested-dylib",
            "codesign:nested-helper",
            "codesign:app-executable",
            "codesign:app-bundle",
            "codesign:verify-app",
            "notarytool:submit-app",
            "stapler:staple-app",
            "stapler:validate-app",
            "hdiutil:create",
            "hdiutil:attach",
            "hdiutil:detach",
            "hdiutil:convert",
            "codesign:dmg",
            "codesign:verify-dmg",
            "notarytool:submit-dmg",
            "stapler:staple-dmg",
            "stapler:validate-dmg",
            "hdiutil:verify",
            "spctl:app",
            "spctl:dmg",
        ):
            self.assertIn(required_stage, stages)

    def test_parameterized_tool_stage_failures_leave_dist_byte_identical_and_clean_temps(self) -> None:
        failure_matrix: list[tuple[str, dict[str, str]]] = [
            ("rust-licenses-audit", {"NEOFLASH_TEST_FAIL_STAGE": "rust-licenses:stage"}),
            ("ditto-stage-app", {"NEOFLASH_TEST_FAIL_STAGE": "ditto:stage-app"}),
            ("codesign-nested-dylib-1", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:nested-dylib", "NEOFLASH_TEST_FAIL_OCCURRENCE": "1"}),
            ("codesign-nested-dylib-2", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:nested-dylib", "NEOFLASH_TEST_FAIL_OCCURRENCE": "2"}),
            ("codesign-nested-helper-ffmpeg", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:nested-helper", "NEOFLASH_TEST_FAIL_OCCURRENCE": "1"}),
            ("codesign-nested-helper-ffprobe", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:nested-helper", "NEOFLASH_TEST_FAIL_OCCURRENCE": "2"}),
            (
                "provenance-refresh-staged-hashes-inventory-drift",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "codesign:nested-helper",
                    "NEOFLASH_TEST_FAIL_OCCURRENCE": "2",
                    "NEOFLASH_TEST_FAIL_MODE": "extra-untracked-media-file-on-helper-sign",
                },
            ),
            ("codesign-app-executable", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:app-executable"}),
            ("codesign-app-bundle", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:app-bundle"}),
            ("codesign-verify-app", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:verify-app"}),
            (
                "outer-sign-corrupts-nested-media",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "codesign:app-bundle",
                    "NEOFLASH_TEST_FAIL_MODE": "corrupt-nested-media-on-app-sign",
                },
            ),
            ("ditto-submit-zip", {"NEOFLASH_TEST_FAIL_STAGE": "ditto:submit-zip"}),
            ("notarytool-app-exit-nonzero", {"NEOFLASH_TEST_FAIL_STAGE": "notarytool:submit-app"}),
            (
                "notarytool-app-invalid-status-exit0",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "notarytool:submit-app",
                    "NEOFLASH_TEST_FAIL_MODE": "notary-invalid-exit0",
                },
            ),
            (
                "notarytool-app-invalid-status-exit1",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "notarytool:submit-app",
                    "NEOFLASH_TEST_FAIL_MODE": "notary-invalid-exit1",
                },
            ),
            (
                "notarytool-app-malformed-json",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "notarytool:submit-app",
                    "NEOFLASH_TEST_FAIL_MODE": "notary-malformed-json",
                },
            ),
            ("stapler-staple-app", {"NEOFLASH_TEST_FAIL_STAGE": "stapler:staple-app"}),
            ("stapler-validate-app", {"NEOFLASH_TEST_FAIL_STAGE": "stapler:validate-app"}),
            ("ditto-final-zip", {"NEOFLASH_TEST_FAIL_STAGE": "ditto:final-zip"}),
            ("ditto-dmg-stage-app", {"NEOFLASH_TEST_FAIL_STAGE": "ditto:dmg-stage-app"}),
            ("hdiutil-create", {"NEOFLASH_TEST_FAIL_STAGE": "hdiutil:create"}),
            ("hdiutil-attach", {"NEOFLASH_TEST_FAIL_STAGE": "hdiutil:attach"}),
            ("dmg-mounted-finder-info-failure", {"NEOFLASH_TEST_FAIL_STAGE": "xattr:finder-info"}),
            (
                "dmg-mounted-sigterm",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "xattr:finder-info",
                    "NEOFLASH_TEST_FAIL_MODE": "sigterm-parent",
                },
            ),
            (
                "hdiutil-detach-transient-recovered-in-cleanup",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "hdiutil:detach-all",
                    "NEOFLASH_TEST_FAIL_OCCURRENCE": "1",
                },
            ),
            (
                "hdiutil-detach-permanent-refusal-never-rm-rf",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "hdiutil:detach-all",
                    "NEOFLASH_TEST_FAIL_OCCURRENCE": "0",
                },
            ),
            ("hdiutil-convert", {"NEOFLASH_TEST_FAIL_STAGE": "hdiutil:convert"}),
            ("codesign-dmg", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:dmg"}),
            ("codesign-verify-dmg", {"NEOFLASH_TEST_FAIL_STAGE": "codesign:verify-dmg"}),
            ("notarytool-dmg-exit-nonzero", {"NEOFLASH_TEST_FAIL_STAGE": "notarytool:submit-dmg"}),
            (
                "notarytool-dmg-invalid-status-exit0",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "notarytool:submit-dmg",
                    "NEOFLASH_TEST_FAIL_MODE": "notary-invalid-exit0",
                },
            ),
            (
                "notarytool-dmg-invalid-status-exit1",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "notarytool:submit-dmg",
                    "NEOFLASH_TEST_FAIL_MODE": "notary-invalid-exit1",
                },
            ),
            ("stapler-staple-dmg", {"NEOFLASH_TEST_FAIL_STAGE": "stapler:staple-dmg"}),
            ("stapler-validate-dmg", {"NEOFLASH_TEST_FAIL_STAGE": "stapler:validate-dmg"}),
            ("hdiutil-verify", {"NEOFLASH_TEST_FAIL_STAGE": "hdiutil:verify"}),
            ("spctl-app", {"NEOFLASH_TEST_FAIL_STAGE": "spctl:app"}),
            ("spctl-dmg", {"NEOFLASH_TEST_FAIL_STAGE": "spctl:dmg"}),
            (
                "swap-incoming-stage-midway",
                {"NEOFLASH_TEST_FAIL_STAGE": "mv:incoming-stage", "NEOFLASH_TEST_FAIL_OCCURRENCE": "3"},
            ),
            (
                "swap-backup-phase-midway",
                {"NEOFLASH_TEST_FAIL_STAGE": "mv:backup-step", "NEOFLASH_TEST_FAIL_OCCURRENCE": "3"},
            ),
            (
                "swap-install-phase-midway",
                {"NEOFLASH_TEST_FAIL_STAGE": "mv:install-step", "NEOFLASH_TEST_FAIL_OCCURRENCE": "3"},
            ),
            (
                "swap-install-phase-last-file",
                {"NEOFLASH_TEST_FAIL_STAGE": "mv:install-step", "NEOFLASH_TEST_FAIL_OCCURRENCE": "5"},
            ),
            (
                "swap-post-install-checksum-failure",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "mv:install-step",
                    "NEOFLASH_TEST_FAIL_OCCURRENCE": "5",
                    "NEOFLASH_TEST_FAIL_MODE": "corrupt-installed-dmg-post-swap",
                },
            ),
            (
                "swap-sigterm-midway",
                {
                    "NEOFLASH_TEST_FAIL_STAGE": "mv:install-step",
                    "NEOFLASH_TEST_FAIL_OCCURRENCE": "2",
                    "NEOFLASH_TEST_FAIL_MODE": "sigterm-parent",
                },
            ),
        ]

        before_snapshot = snapshot_tree(self.dist_dir)
        for label, overrides in failure_matrix:
            with self.subTest(stage=label):
                if self.state_dir.exists():
                    shutil.rmtree(self.state_dir)
                if self.event_log.exists():
                    self.event_log.unlink()

                result = self.run_pipeline(**overrides)
                self.assertNotEqual(result.returncode, 0, f"Expected non-zero exit for {label}")
                if overrides.get("NEOFLASH_TEST_FAIL_MODE") == "sigterm-parent":
                    self.assertEqual(result.returncode, 143)

                after_snapshot = snapshot_tree(self.dist_dir)
                self.assertEqual(
                    after_snapshot,
                    before_snapshot,
                    f"dist/ changed or left stray files after {label} failure",
                )
                self.assert_dist_exact_five_entries()

                if label != "hdiutil-detach-permanent-refusal-never-rm-rf":
                    self.assert_clean_mounts_and_temps()
                else:
                    # Even if kernel detach itself refuses every normal and -force attempt, work_root is
                    # cleaned and the non-empty mountpoint is never wiped with rm -rf.
                    if self.work_parent.exists():
                        self.assertEqual(list(self.work_parent.iterdir()), [])
                    sentinel = next(self.mount_parent.rglob(".mounted-volume-sentinel"), None)
                    self.assertIsNotNone(sentinel)
                    shutil.rmtree(self.mount_parent)

                if "invalid-status" in label:
                    events = self.read_events()
                    log_calls = [e for e in events if e["stage"] == "notarytool:log"]
                    self.assertEqual(
                        len(log_calls),
                        1,
                        f"Expected xcrun notarytool log fetch on Invalid status ({label})",
                    )
                    self.assertIn("notarization failed", result.stderr)
                    self.assertIn("Invalid", result.stderr)

    def test_soft_detach_failure_falls_back_to_force_detach_and_succeeds(self) -> None:
        result = self.run_pipeline(NEOFLASH_TEST_FAIL_STAGE="hdiutil:detach-soft")
        self.assertEqual(result.returncode, 0, result.stderr)
        stages = [e["stage"] for e in self.read_events()]
        self.assertIn("hdiutil:detach", stages)
        self.assertIn("hdiutil:detach-force", stages)
        self.assert_clean_mounts_and_temps()
        self.assert_dist_exact_five_entries()

    def test_preflight_credentials_args_git_and_checksum_failures_preserve_dist(self) -> None:
        before = snapshot_tree(self.dist_dir)

        # 1. Unexpected positional argument.
        res_arg = self.run_pipeline(args=["--unexpected"])
        self.assertEqual(res_arg.returncode, 2)
        self.assertEqual(snapshot_tree(self.dist_dir), before)
        self.assertEqual(self.read_events(), [])

        # 2. Missing Developer ID / Notary profile / whitespace / ad-hoc "-" identity.
        for label, env_patch in (
            ("missing-dev-id", {"NEOFLASH_APPLE_DEVELOPER_ID": None}),
            ("missing-profile", {"NEOFLASH_NOTARY_PROFILE": None}),
            ("whitespace-dev-id", {"NEOFLASH_APPLE_DEVELOPER_ID": "   "}),
            ("adhoc-dash-id", {"NEOFLASH_APPLE_DEVELOPER_ID": "-"}),
        ):
            with self.subTest(case=label):
                res = self.run_pipeline(**env_patch)
                self.assertEqual(res.returncode, 1)
                self.assertEqual(snapshot_tree(self.dist_dir), before)
                self.assertEqual(self.read_events(), [])

        # 3. Dirty Git checkout refused before any tool runs.
        untracked = self.repo_dir / "untracked-source.txt"
        untracked.write_text("dirty\n", encoding="utf-8")
        try:
            res_dirty = self.run_pipeline()
            self.assertEqual(res_dirty.returncode, 1)
            self.assertIn("Refusing to notarize source files", res_dirty.stderr)
            self.assertEqual(snapshot_tree(self.dist_dir), before)
            self.assertEqual(self.read_events(), [])
        finally:
            untracked.unlink()

        # 4. Preflight SHA256SUMS mismatch (corrupted digest & incomplete manifest) refused before staging.
        sums_file = self.dist_dir / SUMS_NAME
        original_sums = sums_file.read_bytes()
        try:
            sums_file.write_text(
                f"{'0' * 64}  {ZIP_NAME}\n" + "\n".join(original_sums.decode().splitlines()[1:]) + "\n",
                encoding="utf-8",
            )
            bad_snap = snapshot_tree(self.dist_dir)
            res_bad_sum = self.run_pipeline()
            self.assertNotEqual(res_bad_sum.returncode, 0)
            self.assertEqual(snapshot_tree(self.dist_dir), bad_snap)
            self.assertEqual(self.read_events(), [])

            # Truncated manifest with only 1 valid line must also fail closed.
            sums_file.write_text(original_sums.decode().splitlines()[0] + "\n", encoding="utf-8")
            trunc_snap = snapshot_tree(self.dist_dir)
            res_trunc = self.run_pipeline()
            self.assertNotEqual(res_trunc.returncode, 0)
            self.assertIn("must record exact release archives", res_trunc.stderr)
            self.assertEqual(snapshot_tree(self.dist_dir), trunc_snap)
            self.assertEqual(self.read_events(), [])
        finally:
            sums_file.write_bytes(original_sums)
        self.assertEqual(snapshot_tree(self.dist_dir), before)

    def test_preflight_tampered_nested_media_binary_fails_closed_via_real_provenance_gate(self) -> None:
        ffmpeg_path = self.dist_dir / "NeoFlash.app/Contents/Resources/media/bin/ffmpeg"
        ffmpeg_path.write_bytes(ffmpeg_path.read_bytes() + b"tampered-before-notarize")
        tampered_snapshot = snapshot_tree(self.dist_dir)

        result = self.run_pipeline()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("staged-file-hash-mismatch", result.stderr)
        self.assertEqual(snapshot_tree(self.dist_dir), tampered_snapshot)
        self.assert_clean_mounts_and_temps()

    def test_missing_any_single_release_artifact_fails_closed_without_touching_remaining_files(self) -> None:
        for artifact_name in ("NeoFlash.app", ZIP_NAME, DMG_NAME, SOURCE_NAME, SUMS_NAME):
            with self.subTest(missing=artifact_name):
                target = self.dist_dir / artifact_name
                backup = self.case_root / f"saved-{artifact_name}"
                shutil.move(str(target), str(backup))
                try:
                    partial_before = snapshot_tree(self.dist_dir)
                    res = self.run_pipeline()
                    self.assertEqual(res.returncode, 1)
                    self.assertIn("missing", res.stderr)
                    self.assertEqual(snapshot_tree(self.dist_dir), partial_before)
                    self.assert_clean_mounts_and_temps()
                finally:
                    shutil.move(str(backup), str(target))

    def test_sibling_portable_archives_in_dist_remain_untouched_on_failure_and_success(self) -> None:
        win_zip = self.dist_dir / f"NeoFlash-{VERSION}-windows-x64.zip"
        portable_src = self.dist_dir / f"NeoFlash-{VERSION}-portable-media-corresponding-source.tar.xz"
        win_zip.write_bytes(b"windows-portable-preview-bytes")
        portable_src.write_bytes(b"portable-corresponding-source-bytes")
        win_hash = sha256_file(win_zip)
        portable_hash = sha256_file(portable_src)

        before = snapshot_tree(self.dist_dir)
        fail_res = self.run_pipeline(
            NEOFLASH_TEST_FAIL_STAGE="mv:install-step",
            NEOFLASH_TEST_FAIL_OCCURRENCE="4",
        )
        self.assertNotEqual(fail_res.returncode, 0)
        self.assertEqual(snapshot_tree(self.dist_dir), before)

        ok_res = self.run_pipeline()
        self.assertEqual(ok_res.returncode, 0, ok_res.stderr)
        self.assertEqual(sha256_file(win_zip), win_hash)
        self.assertEqual(sha256_file(portable_src), portable_hash)
        self.assert_clean_mounts_and_temps()


if __name__ == "__main__":
    unittest.main()
