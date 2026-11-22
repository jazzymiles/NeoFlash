// Run this prepared command on copies of the FLA/XFL sources in inputs.tsv.
(function () {
    function captured(dir, token) {
        if (!token || FLfile.read(dir + "complete.txt") !== token || !FLfile.exists(dir + "scenes.tsv")) return false;
        var records = FLfile.read(dir + "scenes.tsv").split(/\r?\n/);
        for (var n = 0; n < records.length; n++) {
            var last = parseInt(records[n].split("\t")[0], 10) - 1;
            var samples = [0, Math.floor(last / 2), last];
            for (var k = 0; k < samples.length; k++)
                if (!FLfile.exists(dir + "adobe-s" + n + "-f" + samples[k] + ".png")) return false;
        }
        return FLfile.exists(dir + "stage.tsv");
    }
    var base = fl.scriptURI.replace(/[^/]+$/, "");
    var queue = FLfile.read(base + "inputs.tsv").split(/\r?\n/);
    FLfile.write(base + "host.txt", fl.version);
    for (var i = 0; i < queue.length; i++) {
        if (!queue[i]) continue;
        var fields = queue[i].split("\t");
        var dir = base + fields[0] + "/";
        var token = fields[2];
        if (!token) throw new Error("Regenerate the input list with compare-fla-corpus.py prepare");
        if (captured(dir, token)) continue;
        FLfile.write(dir + "complete.txt", "in progress");
        var doc = null;
        FLfile.write(base + "current.txt", fields[0]);
        try {
            // These are disposable copies. Animate may upgrade old script settings or
            // map unavailable fonts at open time; capture uses its host defaults.
            var savedAlerts = fl.suppressAlerts;
            try {
                fl.suppressAlerts = true;
                doc = fl.openDocument(dir + fields[1]);
            } finally {
                fl.suppressAlerts = savedAlerts;
            }
            if (!doc) throw new Error("Document did not open");
            var scenes = [];
            for (var scene = 0; scene < doc.timelines.length; scene++) {
                doc.editScene(scene);
                var t = doc.getTimeline();
                var keyCount = 0, labelCount = 0, scriptCount = 0;
                for (var l = 0; l < t.layers.length; l++) {
                    var keys = t.layers[l].frames;
                    for (var k = 0; keys && k < keys.length;) {
                        var key = keys[k];
                        if (!key) break;
                        keyCount++;
                        if (key.name) labelCount++;
                        if (key.actionScript && /\S/.test(key.actionScript)) scriptCount++;
                        k += Math.max(1, key.duration);
                    }
                }
                scenes.push([t.frameCount, t.layers.length, keyCount, labelCount, scriptCount].join("\t"));
                var last = t.frameCount - 1;
                var frames = [0, Math.floor(last / 2), last];
                for (var j = 0; j < frames.length; j++) {
                    var f = frames[j];
                    if (j && f === frames[j - 1]) continue;
                    t.currentFrame = f;
                    if (!doc.exportPNG(dir + "adobe-s" + scene + "-f" + f + ".png", true, true))
                        throw new Error("PNG export failed on scene " + scene + ", frame " + f);
                }
            }
            FLfile.write(dir + "stage.tsv", [doc.width, doc.height, doc.frameRate, doc.backgroundColor].join("\t"));
            FLfile.write(dir + "scenes.tsv", scenes.join("\n"));
            FLfile.write(dir + "host.txt", fl.version);
            FLfile.write(dir + "complete.txt", token);
        } catch (e) {
            FLfile.write(dir + "complete.txt", "error: " + e);
        } finally {
            if (doc) fl.closeDocument(doc, false);
        }
    }
    FLfile.write(base + "finished.txt", "complete");
})();
