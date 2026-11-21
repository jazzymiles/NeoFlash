#!/usr/bin/env python3
"""Compare a nested classic tween against licensed Adobe Animate on an authoring host.

Prepare a fresh directory. Run its author.jsfl in Animate, wait for ORACLE_DONE
in Output, then run capture.jsfl and check the same directory. Animate CC
prompts to convert the temporary AS1/AS2 source documents to AS3. The original
AVM1 SWFs are played directly inside Test Movie.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "animate-oracle"
FRAMES = 5


def editor(cli, *args):
    process = subprocess.run([str(cli), "--headless", *map(str, args)], stdout=subprocess.PIPE)
    if process.returncode:
        raise ValueError(f"Editor command {args[0]} failed: {process.stdout.decode()}")
    return json.loads(process.stdout)


def prepare(root, cli):
    root.mkdir(parents=True, exist_ok=False)
    for filename in ("author.jsfl", "capture.jsfl", "playback.as"):
        shutil.copyfile(FIXTURES / filename, root / filename)
    for version in (1, 2, 3):
        stem = f"as{version}"
        project = json.loads((FIXTURES / "nested.nflash").read_text())
        project["name"] = f"Oracle-{stem}"
        project["script_language"] = f"action_script{version}"
        for frame in project["symbols"][0]["timeline"]["scripts"]:
            script = frame["script"]
            script["language"] = project["script_language"]
            script["source"] = script["source"].replace("oracle_", f"oracle_{stem}_")
        source = root / f"{stem}.nflash"
        source.write_text(json.dumps(project, indent=2) + "\n")
        for kind, dest in (("fla", f"{stem}.fla"), ("xfl", f"{stem}.dir"), ("swf", f"{stem}.swf")):
            flags = ["--swf-target", "fp10" if version == 3 else "fp8"] if kind == "swf" else []
            editor(cli, "export", "--project", source, "--target", kind, "--out", root / dest, *flags)
    print(f"Run {root / 'author.jsfl'} in Animate. After ORACLE_DONE run {root / 'capture.jsfl'}.")


def rgba(image, ffmpeg):
    # Some authoring exports encode unused transparent pixels differently; compare the
    # actual RGBA bytes, including alpha, at the original 128 x 72 resolution.
    data = subprocess.check_output([ffmpeg, "-v", "error", "-i", str(image), "-frames:v", "1",
                                    "-pix_fmt", "rgba", "-f", "rawvideo", "-"])
    if len(data) != 128 * 72 * 4:
        raise ValueError(f"Unexpected pixel count: {image}")
    return data


def native(cli, project, temp, ffmpeg):
    frames = editor(cli, "eval", "--project", project, "--frames", "0..=4", "--scripts")["frames"]
    images = []
    for frame in range(FRAMES):
        image = temp / f"{project.stem}-{frame}.png"
        editor(cli, "export", "--target", "image", "--project", project, "--transparent",
               "--frame", frame + 1, "--out", image)
        images.append(rgba(image, ffmpeg))
    return [item for frame in frames for item in frame["traces"]], images


def checksum(rgba_bytes):
    value = 0
    for index in range(0, len(rgba_bytes), 4):
        red, green, blue, alpha = rgba_bytes[index:index + 4]
        argb = (alpha << 24) | (red << 16) | (green << 8) | blue
        value = (31 * value + argb) & 0xFFFFFFFF
    return f"{value:x}"


def require_equal(actual, expected, name):
    if actual != expected:
        raise ValueError(f"Mismatch: {name}")


def check(root, cli, temp, ffmpeg):
    rows = (root / "author.txt").read_text().splitlines()
    print(f"Adobe authoring host: {rows[0]}")
    require_equal(rows[1:], [f"as{v}\t{kind}\t3\t128\t72\t8\t5"
                             for v in (1, 2, 3) for kind in ("fla", "xfl")],
                  "Adobe authoring document dimensions, rate, length and AS3 conversion")
    output = (root / "playback.txt").read_text(encoding="utf-8-sig").splitlines()
    first = output.index("ORACLE_WRAPPER")
    last = output.index("ORACLE_DONE", first)
    movies = {}
    current = None
    for line in output[first + 1:last]:
        if line.startswith("ORACLE_LOADING "):
            current = line.split()[1]
            if current in movies:
                raise ValueError(f"Repeated input movie: {current}")
            movies[current] = {"pixels": [], "traces": []}
        elif line.startswith("ORACLE_PIXEL "):
            _, movie, value = line.split()
            require_equal(movie, current, line)
            movies[movie]["pixels"].append(value)
        elif line.strip():
            movies[current]["traces"].append(line)
    expected_movies = ["as1.swf", "as2.swf", "as3.swf", "adobe-fla.swf", "adobe-xfl.swf"]
    require_equal(list(movies), expected_movies, "Test Movie input sequence")
    for version in (1, 2, 3):
        stem = f"as{version}"
        traces, images = native(cli, root / f"{stem}.nflash", temp, ffmpeg)
        require_equal(traces, [f"oracle_{stem}_F1_7", f"oracle_{stem}_F5_7"], stem + " frame scripts")
        hashes = [checksum(image) for image in images]
        require_equal(len(set(hashes)), FRAMES, stem + " distinct frame positions")
        require_equal(movies[f"{stem}.swf"], {"traces": traces, "pixels": hashes}, stem + " Flash playback")
        for kind in ("fla", "xfl"):
            adobe = [rgba(root / f"{stem}-{kind}{i}.png", ffmpeg) for i in range(FRAMES)]
            for frame, (actual, expected) in enumerate(zip(adobe, images)):
                require_equal(actual, expected, f"Adobe authoring {stem} {kind}, frame {frame + 1}")
            if version == 3:
                movie = f"adobe-{kind}.swf"
                require_equal(movies[movie], {"traces": traces, "pixels": hashes}, movie + " Flash playback")
                for name in (movie, f"adobe-{kind}-resaved.fla"):
                    recovered = temp / (name + ".nflash")
                    editor(cli, "import", "--input", root / name, "--out", recovered)
                    other_traces, other_images = native(cli, recovered, temp, ffmpeg)
                    require_equal(other_traces, traces, name + " reimported frame scripts")
                    for frame, (actual, expected) in enumerate(zip(other_images, adobe)):
                        require_equal(actual, expected, f"{name} reimport, frame {frame + 1}")
    print("Matched 30 authored and 20 reimported RGBA frames; five SWFs each matched five "
          "Flash Player frame checksums and two script traces.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("prepare", "check"))
    parser.add_argument("directory", type=Path, help="new evidence directory for prepare, same directory for check")
    parser.add_argument("--editor", type=Path, required=True, help="path to the NeoFlash editor executable")
    parser.add_argument("--ffmpeg", default="ffmpeg", help="PNG decoder for check")
    args = parser.parse_args()
    try:
        cli = args.editor.resolve(strict=True)
        if args.operation == "prepare":
            prepare(args.directory.resolve(), cli)
        else:
            with tempfile.TemporaryDirectory() as tmp:
                check(args.directory.resolve(), cli, Path(tmp), args.ffmpeg)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))


if __name__ == "__main__":
    main()
