# AS2 source interfaces

AS2 supports bounded source `interface` and `implements` declarations, with `extends` for interface inheritance. The Rust compiler reuses the class linker, canonical method signatures and nominal contract registry.

An AS2 source interface can declare public method signatures with optional explicit `public`, use a qualified name matching its `.as` path, and extend one project interface. Classes can implement multiple interfaces, satisfy a contract with an inherited public method, and combine a class base with interface contracts. The existing linker checks method names, parameter/return signatures, missing methods, inherited conflicts, unresolved types and dependency cycles. Interface names are available in source annotations and imports.

AS2 interface fields, constructors, method bodies, static/private signatures, get/set accessors, optional/default/rest parameters and multiple interface parents remain rejected. AS2 `instanceof` has a separate bounded [object/type profile](AS2-OBJECT-BATCH.md); class/interface function-cast syntax remains unsupported. Signature verification here means compiler/linker contract checking, not complete historical AS2 coercion or type-check behavior.

`examples/as2-interfaces-batch.nflash` contains five ordinary AS2 source units and a frame script. A concrete class inherits its label method, implements a derived counter interface and an independent reset interface, and is accessed through typed parameters/variables. The visible **Advance typed counter** control invokes the interface method and updates the stage. It starts at value 3 and increments to 4 on the first click. There is no AS3 `is` syntax or hidden test-only runtime.

Behavioral contract: Adobe's [AS2 statements reference](https://open-flash.github.io/mirrors/as2-language-reference/statements.html#interface) and [Learning ActionScript 2.0](https://open-flash.github.io/mirrors/learning-as2.pdf), interface inheritance examples. This package adapts existing owned code; it copies no external implementation.
