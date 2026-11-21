// Run the generated copy beside the input movies in Animate.
(function () {
    var base = fl.scriptURI.replace(/[^/]+$/, "");
    var log = base + "author.txt";
    if (FLfile.exists(log)) {
        fl.trace("Authoring evidence already exists. Prepare a fresh oracle directory.");
        return;
    }
    FLfile.write(log, fl.version + "\n");
    for (var version = 1; version <= 3; version++) {
        var stem = "as" + version;
        for (var index = 0; index < 2; index++) {
            var format = index === 0 ? "fla" : "xfl";
            var input = index === 0 ? stem + ".fla" : stem + ".dir/" + stem + ".xfl";
            var doc = fl.openDocument(base + input);
            if (!doc) throw new Error("Could not open " + input);
            try {
                var timeline = doc.getTimeline();
                FLfile.write(log, [stem, format, doc.asVersion, doc.width, doc.height,
                    doc.frameRate, timeline.frameCount].join("\t") + "\n", "append");
                for (var frame = 0; frame < timeline.frameCount; frame++) {
                    timeline.currentFrame = frame;
                    var image = base + stem + "-" + format + frame + ".png";
                    if (!doc.exportPNG(image, true, true)) throw new Error("PNG export failed: " + image);
                }
                if (version === 3) {
                    var output = base + "adobe-" + format;
                    if (!doc.exportSWF(output + ".swf", true) ||
                        !fl.saveDocument(doc, output + "-resaved.fla")) {
                        throw new Error("Adobe movie or source export failed: " + input);
                    }
                }
            } finally {
                fl.closeDocument(doc, false);
            }
        }
    }
    var runner = fl.createDocument("timeline");
    runner.getTimeline().layers[0].frames[0].actionScript = FLfile.read(base + "playback.as");
    fl.saveDocument(runner, base + "oracle-playback.fla");
    runner.testMovie();
})();
