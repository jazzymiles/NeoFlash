# AS2 display, platform and source profiles

The AS2 runtime connects the shared Rust display, text, XML, string and event services to these partial profiles:

- [MovieClip depths and paths](AS2-DISPLAY-BATCH.md): sparse runtime depths, duplication, replacement, target paths and bounded TextField operations.
- [Platform and events](AS2-PLATFORM-BATCH.md): browser capability facts, fixed security policies, IME composition listeners, Delegate and component EventDispatcher.
- [XML and LoadVars](AS2-XML-DATA-BATCH.md): metadata, namespace lookup, bounded form/XML requests and response callbacks.
- [Legacy syntax and globals](AS2-LEGACY-BATCH.md): legacy operators, target lookup, primitive conversion and display globals.
- [Source interfaces](AS2-INTERFACES-BATCH.md): method contracts, single-parent interface inheritance and class implementation checks.

The linked profiles describe ordinary examples, unsupported behavior and the pinned sources behind adapted algorithms. Browser origin rules remain authoritative, native host diagnostics are not all catchable by authored AS2, and complete AVM1 coercion, prototype behavior and legacy lifecycle fidelity remain unfinished.

Coverage decisions and declaration counts are maintained in the [complete ActionScript audit](progress/ACTIONSCRIPT.md). These profiles describe partial support, not full Flash compatibility.
