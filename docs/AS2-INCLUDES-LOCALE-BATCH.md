# AS2 project includes and automatic Locale text

The Rust compiler supports project-contained source includes, and the Locale service supports delayed plain-text replacement. Arbitrary filesystem access and an editor Strings panel are not part of this profile.

## Project-contained source includes

AS2 frame/object scripts can use `#include "scripts/file.as"` on a line of its own. UTF-8 `.as` files come from the project's existing Resource files and travel with its serialized snapshot. Nested relative paths, including `../` within the resource root, expand in Rust before imports and ordinary parsing. Included declarations share the calling handler's scope. Errors inside included content identify the resource and its original line.

Includes are limited to 32 levels and 1 MiB of expanded source. Missing files, cycles, root escape, absolute/network paths, backslash paths and invalid UTF-8 diagnose. This profile requires quoted paths without escapes, trailing comments or semicolons. It does not add includes to AS2 class units, global include directories, arbitrary disk access, initclip ordering or intrinsic native declarations. [Adobe's directive contract](https://open-flash.github.io/mirrors/as2-language-reference/directives.html) is the reference; no external implementation was copied.

## Locale automatic replacement

`mx.lang.Locale.addDelayedInstance(field, stringID)` records a live plain TextField. `autoReplace=true` makes a successful XLIFF load replace registered fields before the authored load callback. Two fields can share a string ID; repeated registration rebinds the field. A missing translation leaves its text unchanged. Removed fields are discarded when replacement runs, and Restart clears registrations and pending callbacks.

Rust owns the bounded field-to-ID table, selected-language lookup and replacement plan; the host retains live TextField references and performs existing text writes. Automatic replacement defaults to false in the existing manually configured NeoFlash profile, since no Strings-panel setting exists. Setting the flag or registering a field alone does not trigger a new download or immediate replacement; the next successful load applies it. HTML/rich-text replacement, arbitrary objects and authoring-time replacement remain unsupported. The existing `setString` operation updates tables, without synthesizing a language-load event. [Adobe's Locale reference](https://open-flash.github.io/mirrors/as2-language-reference/mx/lang/Locale.html) supplies the contract.

## Evidence

[as2-runtime-language-batch.nflash](../examples/as2-runtime-language-batch.nflash) combines nested includes, supplied arguments, XLIFF resources, automatic/manual replacement, two visible fields and a French reload control. Representative browser observations covered resource includes, actual XLIFF loading and visible replacement.
