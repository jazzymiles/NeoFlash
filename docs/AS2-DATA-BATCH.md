# AS2 collections, data providers and binding

AS2 provides bounded collection, data-provider, record-set and primitive data-binding behavior. These are nonvisual runtime services.

## Implementation and reuse

The shared Rust data kernel owns row identities, ordered views, stable named-field sorting, filters, ranges, cursor movement, snapshot iteration, nested event suppression and local change coalescing. The browser adapter retains original object references and delivers callbacks and model events. Records are not JSON-cloned: the same object remains observable through a collection, provider, DataSet and iterator.

The Rust binding kernel validates endpoint paths, performs primitive conversions and guards reentrant two-way propagation. Its browser adapter resolves object references and subscribes to explicit events. Component mixins are installed on the requested instance only. Primitive field kinds are inferred from the first value, rather than pretending to provide an XML schema engine.

Array sorting contracts and flag handling were studied and adapted from the MIT-licensed Ruffle source at commit `89a7049965f3090e706265670df309a5128233ac`, `core/src/avm1/globals/array.rs`. Events reuse NeoFlash's existing Rust broadcaster and AS2 EventDispatcher adaptation; the pinned reference is `core/src/avm1/globals/as_broadcaster.rs`. Existing Ruffle notices apply. The data-view, local transaction and binding kernels are project implementations, not an embedded third-party engine. Reference source snapshots and checksums are recorded under ignored `target/verification/as2-two-hour/data-sources/`.

Public contracts were checked against Adobe's AS2 Components Reference: Collection pages 145–152, DataProvider 317–329, DataSet 357–428, Delta classes 459–481, Iterator 745–746 and binding classes 208–248. The archived primary reference is [Adobe's AS2 Components Reference PDF](https://web.archive.org/web/20130325184910id_/http://livedocs.adobe.com/flash/9.0/main/flash_as2_components_reference.pdf).

## Working partial profiles

- `mx.utils.Collection` and `Iterator` interfaces are fulfilled by concrete `CollectionImpl` and returned snapshot iterators. Collection membership preserves reference identity and rejects null additions.
- `DataProvider.initialize(array)` explicitly equips one array with provider methods. DataSet also exposes a provider backed by its shared Rust model. Edits, replacement, removal, sorting and model-change notifications work. Direct structural array mutation after initialization is diagnosed on the next provider operation.
- The nonvisual `DataSet` supports CRUD, stable IDs, cursor selection, independent iterator views, named-field sorting, filtering and ranges. `clear()` removes the current visible view; `removeAll()` removes all rows. Cancelable writes and nested event batches work. Direct property assignments on returned records remain untracked; use `editField` for a recorded change.
- `applyUpdates()` produces a local immutable delta snapshot with coalesced field changes. The snapshot exposes packet, delta and delta-item interfaces. It does not claim server acknowledgement or invoke a nonexistent resolver.
- `EndPoint`, `Binding`, `ComponentMixins`, `DataType` and `TypedValue` support ordinary object paths, constant sources, one-way and two-way bindings, explicit source events, manual refresh, primitive typed fields and validation errors that preserve the destination.

DataSet's nonvisual constructor, `CollectionImpl`, `DataProvider.initialize`, returned iterator classes and symbolic sort helpers are practical entry points. They are listed separately from the declaration catalogue rather than inflating its coverage.

## Limits and unclaimed behavior

This is partial compatibility. UI component inheritance, schema construction, remote resolvers, persistence, XML/XPath endpoint addressing, custom formatters, schema negotiation and arbitrary comparator functions remain unsupported. Unsupported paths diagnose rather than report fabricated success. Direct construction of catalogue interfaces is rejected.

The data model is bounded to 10,000 rows, 256 views, 64 named sorts, 16 fields per sort, 1,024 snapshot iterators and 100,000 reference handles. Binding paths contain at most 32 segments and explicitly reject prototype-mutation keys. Restart invalidates old model, iterator, packet and binding references.

## Example

[as2-data-batch.nflash](../examples/as2-data-batch.nflash) combines collections, sorting, data-provider updates, local transactions and binding in one visible workflow. Representative browser execution was observed; full historical component behavior remains outside the supported profile.
