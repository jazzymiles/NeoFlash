#!/usr/bin/env python3
"""Regenerate NeoFlash's own small XFL/ZIP-FLA/imported-project examples.

These are synthetic XFL containers, not files saved by Adobe Flash. No external
artwork is used. Run `cargo build -p neoflash-import` first. Output is restricted
to examples/fla-corpus/authored; importer reports are build output in target/.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile
from xml.sax.saxutils import escape, quoteattr


ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "examples/fla-corpus/authored"
NS = 'xmlns="http://ns.adobe.com/xfl/2008/"'
HEADER = '<?xml version="1.0" encoding="UTF-8"?>\n'
COMMENT = '<!-- NeoFlash original synthetic XFL fixture; no Adobe-authored content. -->\n'


def matrix(x, y, scale=1):
    return f'<matrix><Matrix tx="{x}" ty="{y}" a="{scale}" d="{scale}"/></matrix>'


def text(value, x, y, size=18, color="#D8E5EF"):
    return (f'<DOMStaticText width="670" height="32">{matrix(x, y)}'
            f'<textRuns><DOMTextRun><characters>{escape(value)}</characters>'
            f'<textAttrs><DOMTextAttrs face="Arial" size="{size}" fillColor="{color}"/>'
            '</textAttrs></DOMTextRun></textRuns></DOMStaticText>')


def primitive(kind, name, x, y, w, h, color, alpha=1):
    tag = "DOMRectangleObject" if kind == "rectangle" else "DOMOvalObject"
    return (f'<{tag} name={quoteattr(name)} objectWidth="{w}" objectHeight="{h}" '
            f'x="{x}" y="{y}"><fill><SolidColor color="{color}" alpha="{alpha}"/>'
            f'</fill></{tag}>')


def frame(index, duration, elements="", label=None, script=None):
    attrs = f'index="{index}" duration="{duration}"'
    if label is not None:
        attrs += f' name={quoteattr(label)} labelType="name"'
    actions = f'<Actionscript><script><![CDATA[{script}]]></script></Actionscript>' if script else ""
    return f'<DOMFrame {attrs}>{actions}<elements>{elements}</elements></DOMFrame>'


def layer(name, frames, locked=False):
    return (f'<DOMLayer name={quoteattr(name)} locked="{str(locked).lower()}"><frames>\n'
            + "\n".join(frames) + '\n</frames></DOMLayer>')


def timeline(name, layers):
    return f'<DOMTimeline name={quoteattr(name)}><layers>\n' + "\n".join(layers) + '\n</layers></DOMTimeline>'


def document(name, layers, symbols=()):
    includes = '<symbols>' + ''.join(f'<Include href="{s}.xml"/>' for s in symbols) + '</symbols>' if symbols else ""
    return (HEADER + COMMENT + f'<DOMDocument {NS} width="720" height="420" frameRate="24" '
            'backgroundColor="#101D2B" xflVersion="2.0">\n' + includes + '\n<timelines>'
            + timeline(name, layers) + '</timelines>\n</DOMDocument>\n')


def symbol(name, layers):
    return (HEADER + COMMENT + f'<DOMSymbolItem {NS} name={quoteattr(name)} symbolType="movie clip">\n'
            + '<timeline>' + timeline(name, layers) + '</timeline>\n</DOMSymbolItem>\n')


def instance(name, target, x, y, *, mode=None, first=0, scale=1, alpha=1):
    attrs = f'name={quoteattr(name)} libraryItemName={quoteattr(target)}'
    if mode:
        attrs += f' symbolType="graphic" loop={quoteattr(mode)} firstFrame="{first}"'
    else:
        attrs += ' symbolType="movie clip"'
    return (f'<DOMSymbolInstance {attrs}>{matrix(x, y, scale)}'
            f'<color><Color alphaMultiplier="{alpha}"/></color></DOMSymbolInstance>')


def publish(version):
    return (HEADER + '<flash_profiles><flash_profile current="true"><PublishFlashProperties>'
            f'<ActionScriptVersion>{version}</ActionScriptVersion>'
            '</PublishFlashProperties></flash_profile></flash_profiles>\n')


def cases():
    title = text("PRIMITIVES / HELD FRAMES", 32, 30, 27, "#FFFFFF")
    caption = text("Two held poses, a blank interval, then return. No tweens.", 32, 75, 17)
    footer = text("0-23: left  |  24-47: right  |  48-59: blank  |  60-71: return", 32, 365, 16)
    shapes = lambda x: primitive("rectangle", "coralSquare", x, 155, 110, 110, "#FF8467") + primitive("oval", "mintCircle", x+140, 165, 90, 90, "#64DFBE", 0.8)
    geometry = document("Primitives and holds", [
        layer("Animated primitives", [frame(0,24,shapes(70),"left"),frame(24,24,shapes(370),"right"),frame(48,12,"","blank"),frame(60,12,shapes(70),"return")]),
        layer("Caption", [frame(0,72,title+caption+footer)], True),
    ])
    yield {
        "id":"primitives-and-holds", "title":"Primitives and holds", "language":None,
        "purpose":"Solid rectangle/oval primitives, translucent fill, static text, held keyframes, explicit blank frames and frame labels.",
        "files":{"DOMDocument.xml":geometry}, "root_frames":72,"symbol_count":1,"script_count":0,
        "expected_warning_codes":["text_metrics"],
        "manual_checks":["At frame 0 and 23 the shapes are on the left; at 24 and 47 they are on the right.","Both animated shapes disappear at frames 48-59 while the captions remain visible; they return at 60.","The circle retains its 80% fill alpha; both objects remain editable primitives."],
    }

    orb = symbol("Orb", [layer("Four held poses", [
        frame(0,6,primitive("oval","orb",0,0,36,36,"#64DFBE"),"northwest"),
        frame(6,6,primitive("oval","orb",54,0,36,36,"#FFCC66"),"northeast"),
        frame(12,6,primitive("oval","orb",54,54,36,36,"#FF8467"),"southeast"),
        frame(18,6,primitive("oval","orb",0,54,36,36,"#80AEFF"),"southwest"),
    ])])
    pair = symbol("Pair", [layer("Nested Orb placements", [frame(0,48,
        instance("liveOrb","Orb",0,0)+instance("frozenOrb","Orb",105,0,mode="single frame",first=12,alpha=0.55),
        script='trace("AS2: nested Pair frame 1");')])])
    nest_caption = text("NESTED TIMELINES / AS2",32,30,27,"#FFFFFF") + text("One library animation, four playback modes.",32,75,17)
    labels = text("Independent pair",32,132,17)+text("Loop",290,132,17)+text("Play once",425,132,17)+text("Single frame 12",555,132,17)
    foot = text("Pair contains another live Orb and a translucent frozen Orb.",32,340,16)+text("The AS2 frame scripts write two messages when playback starts.",32,370,16)
    placements = instance("pair","Pair",32,190)+instance("loop","Orb",290,190,mode="loop")+instance("once","Orb",425,190,mode="play once")+instance("still","Orb",555,190,mode="single frame",first=12)
    nested = document("Nested playback AS2",[
        layer("Instances",[frame(0,48,placements,"start",'trace("AS2: root frame 1");')]),
        layer("Caption",[frame(0,48,nest_caption+labels+foot)],True),
    ],["Orb","Pair"])
    yield {
        "id":"nested-playback-as2","title":"Nested playback AS2","language":"action_script2",
        "purpose":"Nested MovieClip instances plus Graphic loop, play-once and single-frame modes, placement alpha and preserved AS2 frame scripts.",
        "files":{"DOMDocument.xml":nested,"PublishSettings.xml":publish(2),"LIBRARY/Orb.xml":orb,"LIBRARY/Pair.xml":pair},
        "root_frames":48,"symbol_count":3,"script_count":2,
        "expected_warning_codes":["publish_settings","script_preserved","text_metrics"],
        "manual_checks":["Looping Orb advances through four colored poses; play-once Orb holds its last pose after 24 frames.","Single-frame Orb is frozen at zero-based frame 12. The second Orb inside Pair is also frozen and translucent.","Browser playback emits AS2 root/nested trace messages; authoring scrubbing must not execute scripts."],
    }

    heading = text("FRAME SCRIPTS / AS3",32,30,27,"#FFFFFF") + text("Typed locals, frame labels and a final stop().",32,75,17)
    bottom = text("0: START  |  12: MIDDLE  |  24: DONE (playback stops)",32,345,17)+text("Watch the browser console for the three trace messages.",32,377,16)
    pose = lambda x,color: primitive("rectangle","stageCard",x,160,140,115,color)
    scripted = document("Frame scripts AS3",[
        layer("Actions and poses",[
            frame(0,12,pose(45,"#80AEFF"),"start",'var stageName:String = "start";\ntrace("AS3: " + stageName);'),
            frame(12,12,pose(290,"#FFCC66"),"middle",'var amount:int = 2 + 3;\ntrace("AS3: middle " + amount);'),
            frame(24,12,pose(535,"#64DFBE"),"done",'trace("AS3: done");\nstop();'),
        ]),layer("Caption",[frame(0,36,heading+bottom)],True),
    ])
    yield {
        "id":"frame-scripts-as3","title":"Frame scripts AS3","language":"action_script3",
        "purpose":"Preserved AS3 typed local variables, arithmetic/string trace expressions, three labeled keyframes and stop() on the final pose.",
        "files":{"DOMDocument.xml":scripted,"PublishSettings.xml":publish(3)},
        "root_frames":36,"symbol_count":1,"script_count":3,
        "expected_warning_codes":["publish_settings","script_preserved","text_metrics"],
        "manual_checks":["Browser playback displays the blue, yellow and mint cards in sequence.","Console traces are AS3: start, AS3: middle 5 and AS3: done; playback stops at zero-based frame 24.","Scrubbing the editor changes the pose without evaluating frame scripts; frame labels remain start/middle/done."],
    }


def run_json(importer, source):
    result = subprocess.run([str(importer),"inspect",str(source)],capture_output=True,text=True,check=True)
    report = json.loads(result.stdout)
    if not report["can_import"]:
        raise RuntimeError(f"{source}: {report['diagnostics']}")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--importer",type=Path,default=ROOT/"target/debug/neoflash-import")
    args = parser.parse_args()
    importer = args.importer.resolve()
    if not importer.is_file():
        parser.error("Importer missing; run cargo build -p neoflash-import first.")
    reports = ROOT / "target/fla-corpus-research/authored"
    reports.mkdir(parents=True,exist_ok=True)
    entries=[]
    for case in cases():
        folder=DEST/case["id"]
        xfl=folder/"xfl"
        xfl.mkdir(parents=True,exist_ok=True)
        files=case.pop("files")
        files[case["id"]+".xfl"]="PROXY-CS5"
        for relative,content in files.items():
            path=xfl/relative
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_bytes(content.encode("utf-8"))
        archive=folder/(case["id"]+".fla")
        with zipfile.ZipFile(archive,"w",compression=zipfile.ZIP_STORED) as package:
            for relative,content in sorted(files.items()):
                info=zipfile.ZipInfo(relative,date_time=(2000,1,1,0,0,0))
                info.create_system=3
                info.external_attr=0o100644 << 16
                package.writestr(info,content.encode("utf-8"))
        report=run_json(importer,archive)
        directory_report=run_json(importer,xfl)
        warning_codes=sorted({d["code"] for d in report["diagnostics"]})
        if warning_codes != case["expected_warning_codes"]:
            raise RuntimeError(f"Unexpected warnings for {case['id']}: {warning_codes}")
        native=folder/(case["id"]+".nflash")
        with tempfile.TemporaryDirectory(prefix="import-",dir=reports) as tmp:
            temporary=Path(tmp)/"project.nflash"
            subprocess.run([str(importer),"import",str(archive),str(temporary)],check=True,capture_output=True,text=True)
            project=json.loads(temporary.read_text())
            if len(project["symbols"]) != case["symbol_count"]:
                raise RuntimeError("Symbol count changed")
            root=next(s for s in project["symbols"] if s["id"]==project["root"])
            if root["timeline"]["frame_count"] != case["root_frames"]:
                raise RuntimeError("Root duration changed")
            scripts=[s for symbol in project["symbols"] for s in symbol["timeline"]["scripts"]]
            if len(scripts) != case["script_count"]:
                raise RuntimeError("Script count changed")
            if any(s["script"]["language"] != case["language"] for s in scripts):
                raise RuntimeError("Script language changed")
            native.write_bytes(temporary.read_bytes())
        (reports/(case["id"]+"-archive.json")).write_text(json.dumps(report,indent=2)+"\n")
        (reports/(case["id"]+"-directory.json")).write_text(json.dumps(directory_report,indent=2)+"\n")
        records=[]
        for path in sorted(folder.rglob("*")):
            if path.is_file():
                data=path.read_bytes()
                records.append({"path":str(path.relative_to(DEST)),"sha256":hashlib.sha256(data).hexdigest(),"bytes":len(data)})
        entries.append({**case,"source_kind":"neoflash-authored-synthetic-xfl","license":"MIT OR Apache-2.0","can_import":True,"fla":str(archive.relative_to(DEST)),"xfl":str(xfl.relative_to(DEST)),"native":str(native.relative_to(DEST)),"files":records})
        print(f"{case['id']}: XFL + ZIP accepted, {case['symbol_count']} symbols, {case['root_frames']} frames, {case['script_count']} scripts")
    metadata={"schema_version":1,"generator":"scripts/generate-fla-examples.py","notice":"Original NeoFlash fixture content. FLA files are deterministic synthetic ZIP/XFL packages, not files saved by Adobe Flash or historical binary FLA.","cases":entries}
    (DEST/"metadata.json").write_text(json.dumps(metadata,indent=2)+"\n")


if __name__ == "__main__":
    main()
