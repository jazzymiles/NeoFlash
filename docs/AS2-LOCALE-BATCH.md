# AS2 Locale batch

`mx.lang.Locale` now has a nonvisual partial profile for translation tables and asynchronously loaded Flash XLIFF resources. Rust owns the tables, language selection, status, bounded loading tokens and XML extraction. The browser bridge connects the existing URLLoader and preserves authored callback references. It adds no editor Strings panel, FLA handling or AS3 behavior.

## Supported behavior

The batch adds 10 methods (`addXMLPath`, `checkXMLStatus`, `getDefaultLang`, `initialize`, `loadLanguageXML`, `loadString`, `loadStringEx`, `setDefaultLang`, `setLoadCallback`, `setString`), two snapshot list properties (`languageCodeArray`, `stringIDArray`) and the static class entry. The catalogue gain is **12 method/property rows plus one class**, all previously missing. There is no constructor claim and no stale-audit promotion.

`initialize()` chooses a configured exact browser language, then its base language, then the configured default. The initial default is `en`; authored code can set it. Language keys are normalized to lowercase with hyphens. `loadStringEx(id, language)` only looks up an existing table and never performs network IO. Missing strings return `undefined`; empty translations remain empty strings. There is no undocumented interpolation API.

Each explicit load uses an existing URLLoader operation. A new load cancels the earlier one. A custom callback replaces the registered default callback for that load. Unconfigured paths, transport failures and malformed XML complete with `false`, leaving previous translations intact. Successful parsing atomically merges the language's strings, selects that language and sets XML status to true. Reset cancels pending transport, clears callbacks and replaces the Rust state.

## XLIFF scope and limits

The parser reuses the shared Rust AS2 XML kernel. It accepts the conventional XLIFF external DOCTYPE as inert document metadata and never fetches that DTD. The Flash Strings-panel profile reads `file/body/trans-unit`, uses `resname` as the string ID and reads the unit's `source` value. Plain text, XML entities, Unicode, CDATA and empty source values work. Duplicate names, language mismatches, grouped units, inline element markup and `target` selection produce a failed load rather than silently dropping content.

The model supports at most 128 languages, 10,000 distinct string IDs and 4 MiB of translation table data; XML retains the shared 1 MiB source bound. This is a Flash XLIFF 1.x plain-text subset, not a complete XLIFF processor.

This original batch covered manual replacement only. The subsequent [project includes and Locale batch](AS2-INCLUDES-LOCALE-BATCH.md) adds partial `autoReplace=true` and `addDelayedInstance` support for live plain TextFields after successful XLIFF loads; richer text and editor Strings-panel integration remain unsupported.

## Provenance and validation

Contracts were checked against the [Adobe AS2 Locale reference](https://open-flash.github.io/mirrors/as2-language-reference/mx/lang/Locale.html) and Adobe's [Flash CS3 User Guide, exported XML sample, page 287](https://www.adobe.com/support/documentation/archived_content/en/flash/cs3/flash_cs3_help.pdf). Reference snapshots and SHA-256 identifiers are under ignored `target/verification/as2-two-hour/locale-sources/`. The Locale table code is a project implementation. It reuses the existing AS2 XML adaptation whose Ruffle reference is pinned to `07d3e430631333f80f754df01a968bdcd80d98c5`, `core/src/avm1/globals/xml.rs`; existing MIT notices remain applicable. No new foreign-language implementation or complete engine is embedded.

One grouped native test, `as2_locale::tests::as2_locale_family`, passed. An initial command without `--lib` also compiled unrelated integration fixtures and stopped at a concurrent TextFormatPatch fixture mismatch; the focused library test then ran successfully. No additional per-method tests were added.

The ordinary example `examples/as2-locale-batch.nflash` contains three embedded XML resources and six grouped checks covering tables, list snapshots, asynchronous loading, custom/default callbacks, malformed XML retention, initialization and unconfigured paths. The coordinator observed `AS2_LOCALE_RESULT 6/6` in the real browser during the shared delivery. Exact catalogue IDs are in `target/verification/as2-two-hour/locale-inventory.json`.
