// Run after ORACLE_DONE appears in Test Movie's Output panel.
(function () {
    var base = fl.scriptURI.replace(/[^/]+$/, "");
    var output = base + "playback.txt";
    fl.outputPanel.save(output);
    if ((FLfile.read(output) || "").indexOf("ORACLE_DONE") < 0) {
        fl.trace("Playback has not finished. Run capture again after ORACLE_DONE.");
        return;
    }
    for (var i = fl.documents.length - 1; i >= 0; i--) {
        var doc = fl.documents[i];
        if (doc.pathURI === base + "oracle-playback.fla") fl.closeDocument(doc, false);
    }
})();
