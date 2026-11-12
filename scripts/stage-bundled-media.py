#!/usr/bin/env python3
"""
Stage a self-contained, dylib-relocated FFmpeg + FFprobe media toolchain bundle.

By default stages into `<workspace_root>/target/bundled-media`.
Pass `--dest <dir>` (for example `dist/NeoFlash.app/Contents/Resources/media`) to stage
directly into a packaged application bundle.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
from pathlib import Path


SYSTEM_PREFIXES = (
    "/System/Library/",
    "/usr/lib/",
)


def run_checked(cmd: list[str], env: dict[str, str] | None = None) -> str:
    res = subprocess.run(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
        check=False,
    )
    if res.returncode != 0:
        raise RuntimeError(
            f"Command failed ({res.returncode}): {' '.join(cmd)}\n{res.stderr.strip()}"
        )
    return res.stdout


def locate_source_binary(name: str, env_var: str) -> Path:
    override = os.environ.get(env_var, "").strip()
    if override:
        p = Path(override)
        if p.is_file():
            return p.resolve()
        raise FileNotFoundError(f"{env_var}={override} does not point to an existing file")

    for candidate in (
        f"/opt/homebrew/bin/{name}",
        f"/usr/local/bin/{name}",
    ):
        p = Path(candidate)
        if p.is_file():
            return p.resolve()

    found = shutil.which(name)
    if found:
        return Path(found).resolve()

    raise FileNotFoundError(
        f"Could not locate host '{name}'. Install ffmpeg or set {env_var}."
    )


def otool_id(binary_path: Path) -> str | None:
    out = run_checked(["otool", "-D", str(binary_path)]).splitlines()
    if len(out) >= 2:
        val = out[1].strip()
        return val if val else None
    return None


def otool_dependencies(binary_path: Path) -> list[str]:
    lines = run_checked(["otool", "-L", str(binary_path)]).splitlines()[1:]
    own_id = otool_id(binary_path)
    deps: list[str] = []
    for idx, raw in enumerate(lines):
        line = raw.strip()
        if not line:
            continue
        dep = line.split(" (compatibility version", 1)[0].strip()
        if idx == 0 and own_id and dep == own_id:
            continue
        deps.append(dep)
    return deps


def is_system_dep(dep: str) -> bool:
    return dep.startswith(SYSTEM_PREFIXES)


def resolve_non_system_dep(parent_real: Path, dep: str) -> Path | None:
    if is_system_dep(dep):
        return None
    if dep.startswith("@rpath/") or dep.startswith("@loader_path/") or dep.startswith("@executable_path/"):
        sub = dep.split("/", 1)[1]
        candidates = [
            parent_real.parent / sub,
            parent_real.parent.parent / "lib" / sub,
            Path("/opt/homebrew/lib") / sub,
            Path("/usr/local/lib") / sub,
        ]
        for cand in candidates:
            if cand.exists():
                return cand.resolve()
        raise FileNotFoundError(f"Unable to resolve relative/rpath dependency '{dep}' from '{parent_real}'")
    p = Path(dep)
    if not p.exists():
        raise FileNotFoundError(f"Missing dependency '{dep}' referenced by '{parent_real}'")
    return p.resolve()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def extract_cellar_origin(real_path: Path) -> tuple[str, str, Path | None]:
    parts = real_path.parts
    if "Cellar" in parts:
        idx = parts.index("Cellar")
        if idx + 2 < len(parts):
            formula = parts[idx + 1]
            version = parts[idx + 2]
            keg_root = Path(*parts[: idx + 3])
            return formula, version, keg_root
    return "system-or-custom", "unknown", None


def make_writable(path: Path) -> None:
    mode = path.stat().st_mode
    path.chmod(mode | stat.S_IWUSR | stat.S_IRUSR | stat.S_IXUSR)


def stage_bundle(dest_dir: Path, src_ffmpeg: Path, src_ffprobe: Path) -> None:
    bin_dir = dest_dir / "bin"
    lib_dir = dest_dir / "lib"
    prov_dir = dest_dir / "provenance"
    lic_dir = dest_dir / "licenses"

    if dest_dir.exists():
        shutil.rmtree(dest_dir)
    bin_dir.mkdir(parents=True, exist_ok=True)
    lib_dir.mkdir(parents=True, exist_ok=True)
    prov_dir.mkdir(parents=True, exist_ok=True)
    lic_dir.mkdir(parents=True, exist_ok=True)

    staged_ffmpeg = bin_dir / "ffmpeg"
    staged_ffprobe = bin_dir / "ffprobe"
    shutil.copy2(src_ffmpeg, staged_ffmpeg)
    shutil.copy2(src_ffprobe, staged_ffprobe)
    make_writable(staged_ffmpeg)
    make_writable(staged_ffprobe)

    # Discover all transitive non-system dylibs.
    dylib_by_name: dict[str, Path] = {}
    raw_deps_by_target: dict[Path, list[tuple[str, str]]] = {}

    queue: list[tuple[Path, Path]] = [
        (src_ffmpeg, staged_ffmpeg),
        (src_ffprobe, staged_ffprobe),
    ]
    visited_staged: set[Path] = {staged_ffmpeg, staged_ffprobe}

    while queue:
        src_real, staged_target = queue.pop(0)
        deps = otool_dependencies(src_real)
        rewrites: list[tuple[str, str]] = []
        for dep in deps:
            resolved = resolve_non_system_dep(src_real, dep)
            if resolved is None:
                continue
            dep_basename = os.path.basename(dep)
            rewrites.append((dep, dep_basename))
            if dep_basename not in dylib_by_name:
                dylib_by_name[dep_basename] = resolved
                staged_dylib = lib_dir / dep_basename
                shutil.copy2(resolved, staged_dylib)
                make_writable(staged_dylib)
                if staged_dylib not in visited_staged:
                    visited_staged.add(staged_dylib)
                    queue.append((resolved, staged_dylib))
            elif dylib_by_name[dep_basename] != resolved:
                raise RuntimeError(
                    f"Conflicting dylib basename '{dep_basename}': "
                    f"{dylib_by_name[dep_basename]} vs {resolved}"
                )
        raw_deps_by_target[staged_target] = rewrites

    # Rewrite install names in all staged dylibs and executables.
    for dep_basename in sorted(dylib_by_name):
        staged_dylib = lib_dir / dep_basename
        cmd = ["install_name_tool", "-id", f"@loader_path/{dep_basename}"]
        for old_dep, child_basename in raw_deps_by_target.get(staged_dylib, []):
            cmd.extend(["-change", old_dep, f"@loader_path/{child_basename}"])
        cmd.append(str(staged_dylib))
        run_checked(cmd)

    for staged_exec in (staged_ffmpeg, staged_ffprobe):
        cmd = ["install_name_tool"]
        for old_dep, child_basename in raw_deps_by_target.get(staged_exec, []):
            cmd.extend(["-change", old_dep, f"@executable_path/../lib/{child_basename}"])
        cmd.append(str(staged_exec))
        if len(cmd) > 2:
            run_checked(cmd)

    # Verify zero external/Homebrew/@rpath references remain.
    all_staged_binaries = [staged_ffmpeg, staged_ffprobe] + [
        lib_dir / name for name in sorted(dylib_by_name)
    ]
    for bin_path in all_staged_binaries:
        lines = run_checked(["otool", "-L", str(bin_path)]).splitlines()[1:]
        for raw in lines:
            dep = raw.strip().split(" (compatibility version", 1)[0].strip()
            if not dep:
                continue
            if (
                dep.startswith("/opt/homebrew")
                or dep.startswith("/usr/local")
                or dep.startswith("@rpath")
            ):
                raise RuntimeError(
                    f"Unrelocated dependency '{dep}' remains in '{bin_path}'"
                )

    # Ad-hoc sign inside-out: all dylibs first, then executables.
    for dep_basename in sorted(dylib_by_name):
        run_checked(["codesign", "--force", "--sign", "-", str(lib_dir / dep_basename)])
    for staged_exec in (staged_ffmpeg, staged_ffprobe):
        run_checked(["codesign", "--force", "--sign", "-", str(staged_exec)])

    # Verify clean execution with empty PATH.
    clean_env = {"PATH": "", "HOME": os.environ.get("HOME", "/tmp")}
    version_out = run_checked([str(staged_ffmpeg), "-version"], env=clean_env)
    probe_version_out = run_checked([str(staged_ffprobe), "-version"], env=clean_env)
    buildconf_out = run_checked([str(staged_ffmpeg), "-hide_banner", "-buildconf"], env=clean_env)
    license_out = run_checked([str(staged_ffmpeg), "-hide_banner", "-L"], env=clean_env)
    encoders_out = run_checked([str(staged_ffmpeg), "-hide_banner", "-encoders"], env=clean_env)

    (prov_dir / "ffmpeg-version.txt").write_text(version_out + "\n" + probe_version_out, encoding="utf-8")
    (prov_dir / "ffmpeg-buildconf.txt").write_text(buildconf_out, encoding="utf-8")
    (prov_dir / "ffmpeg-license.txt").write_text(license_out, encoding="utf-8")
    (prov_dir / "ffmpeg-encoders.txt").write_text(encoders_out, encoding="utf-8")

    # Collect licenses from kegs.
    copied_kegs: set[Path] = set()
    for real_path in [src_ffmpeg, src_ffprobe] + list(dylib_by_name.values()):
        formula, version, keg_root = extract_cellar_origin(real_path)
        if keg_root and keg_root not in copied_kegs and keg_root.is_dir():
            copied_kegs.add(keg_root)
            for candidate in keg_root.iterdir():
                if candidate.is_file() and any(
                    token in candidate.name.upper()
                    for token in ("LICENSE", "COPYING", "COPYRIGHT", "NOTICE")
                ):
                    target_lic = lic_dir / f"{formula}-{version}-{candidate.name}"
                    shutil.copy2(candidate, target_lic)

    ffmpeg_formula, ffmpeg_ver, _ = extract_cellar_origin(src_ffmpeg)
    corresponding_source_note = f"""NeoFlash Bundled Media Toolchain — Corresponding Source & Licensing Notice
==========================================================================

1. Aggregation Architecture
   NeoFlash is licensed under MIT OR Apache-2.0.
   The bundled `ffmpeg` and `ffprobe` helper executables under `bin/` and their
   private relocated shared libraries under `lib/` are separate standalone
   programs invoked solely as child processes via standard command-line pipes
   and temporary files. NeoFlash does not link statically or dynamically against
   FFmpeg or its codec libraries.

2. License of the Bundled Media Helpers
   The standard macOS Beta 1 media helper bundle includes `ffmpeg` ({ffmpeg_ver})
   configured with `--enable-gpl --enable-version3` and software codec libraries
   including `libx264` (GPLv2+), `libx265` (GPLv2+), `libvpx` (BSD-3-Clause),
   `libopus` (BSD-3-Clause), `libmp3lame` (LGPLv2+), and `dav1d` (BSD-2-Clause).
   Consequently, the `bin/ffmpeg` and `bin/ffprobe` executables themselves are
   governed by the GNU General Public License version 3 or later (GPL-3.0-or-later).
   See `licenses/` for the complete license texts of FFmpeg and every bundled library.

3. Corresponding Source & Reproducible Build Scripts
   - Standard GPL build script (matching the shipped capabilities):
     `scripts/build-bundled-ffmpeg.sh`
   - Optional LGPL-only build script (without `libx264`/`libx265` or `--enable-gpl`):
     `scripts/build-bundled-ffmpeg-lgpl.sh`
   - Upstream source archives:
     - FFmpeg {ffmpeg_ver}: https://ffmpeg.org/releases/ffmpeg-{ffmpeg_ver}.tar.xz
     - x264: https://code.videolan.org/videolan/x264.git
     - x265: https://bitbucket.org/multicoreware/x265_git
     - libvpx: https://chromium.googlesource.com/webm/libvpx
     - opus: https://downloads.xiph.org/releases/opus/
     - lame: https://sourceforge.net/projects/lame/files/lame/

4. Replacing the Bundled Helpers
   Users and downstream redistributors may replace `bin/ffmpeg` and `bin/ffprobe`
   at any time or point `NEOFLASH_FFMPEG` and `NEOFLASH_FFPROBE` to an alternate
   compatible build.
"""
    (lic_dir / "README-CORRESPONDING-SOURCE.txt").write_text(
        corresponding_source_note, encoding="utf-8"
    )

    # Generate SHA256SUMS.txt and media-sbom.json.
    sha_lines: list[str] = []
    for rel_path in sorted(all_staged_binaries, key=lambda p: p.relative_to(dest_dir).as_posix()):
        rel_str = rel_path.relative_to(dest_dir).as_posix()
        digest = sha256_file(rel_path)
        sha_lines.append(f"{digest}  {rel_str}")
    (prov_dir / "SHA256SUMS.txt").write_text("\n".join(sha_lines) + "\n", encoding="utf-8")

    libraries_sbom = []
    for dep_basename in sorted(dylib_by_name):
        real_src = dylib_by_name[dep_basename]
        staged_dylib = lib_dir / dep_basename
        formula, ver, _ = extract_cellar_origin(real_src)
        libraries_sbom.append(
            {
                "name": dep_basename,
                "relative_path": f"lib/{dep_basename}",
                "sha256": sha256_file(staged_dylib),
                "source_formula": formula,
                "version": ver,
            }
        )

    sbom = {
        "schema_version": 1,
        "component": "neoflash-bundled-media",
        "neoflash_version": "0.1.0-beta.1",
        "neoflash_license": "MIT OR Apache-2.0",
        "helper_license": "GPL-3.0-or-later",
        "aggregation_boundary": "Separate subprocess executables invoked via Command::new; no static or dynamic linking with NeoFlash binaries.",
        "cve_and_security_owner": "NeoFlash Release Engineering (rebuild via scripts/build-bundled-ffmpeg.sh and restage via scripts/stage-bundled-media.py)",
        "build_scripts": [
            "scripts/build-bundled-ffmpeg.sh",
            "scripts/build-bundled-ffmpeg-lgpl.sh",
            "scripts/stage-bundled-media.py",
        ],
        "executables": [
            {
                "name": "ffmpeg",
                "relative_path": "bin/ffmpeg",
                "version": ffmpeg_ver,
                "sha256": sha256_file(staged_ffmpeg),
                "license": "GPL-3.0-or-later",
                "source_url": f"https://ffmpeg.org/releases/ffmpeg-{ffmpeg_ver}.tar.xz",
            },
            {
                "name": "ffprobe",
                "relative_path": "bin/ffprobe",
                "version": ffmpeg_ver,
                "sha256": sha256_file(staged_ffprobe),
                "license": "GPL-3.0-or-later",
                "source_url": f"https://ffmpeg.org/releases/ffmpeg-{ffmpeg_ver}.tar.xz",
            },
        ],
        "libraries": libraries_sbom,
    }
    (prov_dir / "media-sbom.json").write_text(
        json.dumps(sbom, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Stage self-contained FFmpeg/FFprobe media helpers for NeoFlash."
    )
    project_root = Path(__file__).resolve().parent.parent
    parser.add_argument(
        "--dest",
        type=Path,
        default=project_root / "target" / "bundled-media",
        help="Destination directory for the bundled media payload",
    )
    args = parser.parse_args()

    src_ffmpeg = locate_source_binary("ffmpeg", "NEOFLASH_SOURCE_FFMPEG")
    src_ffprobe = locate_source_binary("ffprobe", "NEOFLASH_SOURCE_FFPROBE")
    dest_dir = args.dest.resolve()
    stage_bundle(dest_dir, src_ffmpeg, src_ffprobe)
    print(f"Staged self-contained media toolchain at {dest_dir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
