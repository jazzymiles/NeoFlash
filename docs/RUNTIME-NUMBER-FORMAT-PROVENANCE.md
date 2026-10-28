# Rust NumberFormatter profile

28 October 2026. The AS3 `flash.globalization.NumberFormatter` and
`NumberParseResult` facades provide a bounded locale-aware decimal formatting
and parsing family. Rust owns formatter state, validation, fixed-point rendering,
group placement, digit substitution, sign placement, parsing and UTF-16 result
indices. The browser reads locale metadata using constant Intl probes; authored
numbers and input strings never go through Intl formatting or JavaScript parsing
algorithms.

## Sources and implementation

Contracts were checked against the official references for
[NumberFormatter](https://airsdk.dev/reference/actionscript/3.0/flash/globalization/NumberFormatter.html),
[NumberParseResult](https://airsdk.dev/reference/actionscript/3.0/flash/globalization/NumberParseResult.html)
and [NationalDigitsType](https://airsdk.dev/reference/actionscript/3.0/flash/globalization/NationalDigitsType.html).
This part is a new Rust implementation using the Rust standard library's bounded
fixed-precision formatter and floating-point parser. No external implementation
source was copied, no third-party engine is linked and no dependency is added.
The pinned Ruffle tree used by the adjacent filter work did not supply a
NumberFormatter implementation at the inspected path; it is not credited as a
source for algorithms that it did not provide.

The browser's Intl implementation supplies locale identifiers, separators, minus
sign, a grouping pattern and the Unicode zero digit via fixed constants only.
This is platform metadata, analogous to Flash's operating-system locale data.
It does not promise the historical operating system's exact defaults.

## Delivered scope and counts

- NumberFormatter: **13 properties** and **5 instance methods** (`formatInt`,
  `formatUint`, `formatNumber`, `parse`, `parseNumber`).
- NumberParseResult: **3 read-only properties**, including UTF-16 start/end
  offsets, and explicit construction.
- **21 new member paths**, **2 constructors** counted separately, no migrations,
  no new language constructs. These classes are AS3-only.
- Thirty-three contiguous decimal digit sets are recognized in parsing; the
  configured set controls rendering. This is support within `digitsType`, not
  33 new API names. The NationalDigitsType constant class itself is not added.

Formatting supports configurable fractional digits, leading/trailing zeroes,
up to four group sizes with optional repeat, custom separators and minus sign,
and all five documented negative-number patterns. int/uint conversion occurs in
Rust. Parsing accepts locale decimal/group separators and all supported digit
sets, returns the first numeric region or strictly checks the entire trimmed
input, and preserves UTF-16 offsets even after a supplementary-plane character.
NumberParseResult represents failure as NaN with both indices `0x7fffffff`.
Mutation validation preserves the old value and sets an error status.

## Explicit partial boundaries

- `getAvailableLocaleIDNames()` diagnoses the browser's lack of a complete locale
  enumeration API. It is not counted or silently replaced with a small list.
- Rust fixed-precision rounding is the profile. The original API permits
  operating-system-dependent rounding; exact old-platform parity is not claimed.
  Fractional digits are bounded to 0..20. Nonfinite formatting uses `NaN` and
  `Infinity`; locale-specific spelling for those values is not implemented.
- Supported group sizes are 1..9, at most four; separators/signs are bounded to
  32 UTF-8 bytes and cannot make decimal/group separators identical. Unsupported
  numbering systems such as non-contiguous ideographic digits diagnose.
- Scientific notation is not parsed. Grouping position is permissive, as the
  documented parser ignores groupingPattern/useGrouping. The whitespace profile
  follows Rust Unicode whitespace; exact historic Zs-only and all invalid-sign
  recovery behavior remain unfinished. Input is limited to 1 MiB.
- Default precision is zero, grouping is initially disabled, leading zero is
  enabled, trailing zeros are disabled and prefix minus is selected. Callers can
  explicitly configure all these properties. Locale defaults and fallback status
  use current browser metadata, not an emulated Flash platform database.
- Native subclassing, currency formatting and the remaining globalization
  classes are outside this package.

## Representative evidence

One grouped Rust test, `as_number_format::tests::number_formatter_family`, covers
repeat grouping, negative round-trip parsing, UTF-16 offsets, Arabic digits,
int/uint conversion, strict rejection and atomic settings. The ordinary project
`examples/runtime-number-format.nflash` shows English, French and Arabic outputs
and checks all supported member families in one authored program. Its Run again
and Restart browser results belong to the coordinator's delivery record; source
creation alone does not constitute browser verification.
