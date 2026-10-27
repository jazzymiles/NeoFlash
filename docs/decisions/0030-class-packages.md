# Source class access and package resolution

Status: accepted for the bounded source-class/package workflow, 2026-10-27.

## Declaration model

Project sources retain one class or interface per source unit. AS3 declarations carry public/private/protected/internal visibility; omitted class/member visibility is internal. Private instance slots belong to the declaring class, so a base and derived class can each own the same private spelling. Protected virtual methods share their inherited dispatch slot. Internal members and classes belong to their package. Constructors, interface signatures, override compatibility and final classes/methods are checked by the linker. A dynamic class allows additional public properties; its subclasses must explicitly declare dynamic to inherit that behavior.

Imports resolve against the project's embedded same-dialect units and supported built-ins. Wildcards expose immediate package definitions, not subpackages. Internal types are available only in their own package; unresolved or ambiguous names diagnose. Conflicting wildcard short names diagnose eagerly even when unused; an exact import takes precedence. No filesystem discovery, SWC dependency loading or user-defined namespaces are implied.

## Runtime access

Emitted member operations carry the lexical calling class without changing a mutable global access mode. Nonpublic traits use registry-owned opaque keys and never appear under their source spelling in public object properties, JSON or enumeration. Public fields and readable accessors remain available to the existing JSON/AMF bridge. Fixed declared traits do not become dynamic enumeration entries. The shared property-reference path preserves receiver/key/RHS evaluation order and covers aliases, compound writes and calls. Sealed-object checks also cover display proxies whose custom storage is separate from the JavaScript object target. Ordinary sealed instances become nonextensible after construction; display proxies retain their host descriptors and use the explicit custom-write guard. Public computed keys do not select private/protected/internal slots. Bare super values diagnose; supported super.member access remains separate from a general reflection model.

AS2 keeps a different model: private is compile-time access control that includes subclasses, while runtime members retain their ordinary public string/prototype representation. This is not AS3 runtime secrecy. Unsupported namespace collisions, native overrides, general reflection/prototype behavior and complete AVM initialization/error semantics retain explicit boundaries.

## Verification scope

Extend the existing compiler/project scenario and add one ordinary component workflow. The workflow connects base/derived private state, protected dispatch, package collaboration, public JSON, dynamic metadata, a sealed child, AS2 inheritance and Restart. One shared delivery pass follows assembly; no independent browser cycle per modifier or API is required. The shared delivery pass and corrected browser workflow passed; the nested AS2 fixture stop() correction, final source hashes and exact observed sequence are recorded in VERIFICATION.md. Native acceptance and original Flash remain separate.

Primary references: Adobe/AIR [language statements and attributes](https://airsdk.dev/reference/actionscript/3.0/statements.html) and the Adobe [AS2 reference mirror](https://open-flash.github.io/mirrors/as2-language-reference/statements.html#private). Original Flash execution is not part of this delivery's evidence.
