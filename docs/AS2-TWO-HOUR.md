# AS2 data, text and resource profiles

These partial AS2 profiles share the Rust core and the ordinary HTML5 publisher:

- [Data and binding](AS2-DATA-BATCH.md): collections, snapshot iterators, data providers, local DataSet transactions and primitive bindings. Remote resolvers, schemas and visual component skins remain unsupported.
- [Text, snapshots and input](AS2-TEXT-DISPLAY-TWO-HOUR.md): uniform CSS/HTML, paragraph formats, static-text snapshots, selection paint and legacy input callbacks. Mixed rich-text runs and full historical layout remain unfinished.
- [Bitmap operations](AS2-BITMAP-BATCH.md): bounded pixel operations, seeded noise and BitmapData displacement. Exact original-player noise/pixel output and live display displacement are outside the profile.
- [Files](AS2-FILES-BATCH.md): browser-native selection, bounded multipart upload and download. Native save/download and active-transfer cancellation still need browser acceptance.
- [Locale](AS2-LOCALE-BATCH.md): XLIFF string tables, callbacks and replacement. Editor Strings integration and full XLIFF remain unsupported.
- [Objects and named references](AS2-OBJECT-BATCH.md): watchpoints, accessors, missing-property resolution and bounded named paths. This is not a complete AVM1 object model.
- [Sound](AS2-SOUND-BATCH.md): project-resource attachment, metadata and channel matrices. Audible balance and quality were not established by the silent example.
- [Timeline variables](AS2-VARIABLES-BATCH.md): bounded form decoding and browser GET/POST loading into MovieClip targets.

Each linked profile records implementation boundaries, examples and applicable source attribution. Necessary notices accompany exports in [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt).

Coverage decisions and declaration counts are maintained in the [complete ActionScript audit](progress/ACTIONSCRIPT.md). These profiles describe partial support, not full Flash compatibility.
