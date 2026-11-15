//! Public Flash/Ruffle containers complement the independently authored wire cases.
//! Fixture origin, exact revisions and hashes are in fixtures/sol/provenance.json.
use neoflash_core::{
    AmfDecoded, AmfGraph, AmfNode, AmfValue, MAX_AMF_BYTES, amf_decode, amf_encode,
};

const FIXTURES: &[(&str, u8, &[u8])] = &[
    (
        "RuffleTest",
        0,
        include_bytes!("fixtures/sol/amf0_shared_object.sol"),
    ),
    (
        "RuffleTest",
        3,
        include_bytes!("fixtures/sol/amf3_shared_object.sol"),
    ),
    (
        "strict_array_test",
        0,
        include_bytes!("fixtures/sol/amf0_arrays.sol"),
    ),
    (
        "amf_object_payload",
        0,
        include_bytes!("fixtures/sol/amf0_typed_objects.sol"),
    ),
    (
        "RuffleTestRef",
        0,
        include_bytes!("fixtures/sol/amf0_self_reference.sol"),
    ),
];

fn node<'a>(graph: &'a AmfGraph, value: &AmfValue) -> &'a AmfNode {
    let AmfValue::Ref(id) = value else {
        panic!("expected node, got {value:?}")
    };
    &graph.nodes[*id]
}

fn entries(node: &AmfNode) -> &[(String, AmfValue)] {
    match node {
        AmfNode::Object { entries, .. } | AmfNode::Array { entries, .. } => entries,
        _ => panic!("expected object/array, got {node:?}"),
    }
}

fn property<'a>(node: &'a AmfNode, key: &str) -> &'a AmfValue {
    &entries(node)
        .iter()
        .find(|(name, _)| name == key)
        .unwrap_or_else(|| panic!("missing property {key} in {node:?}"))
        .1
}

fn root(graph: &AmfGraph) -> &AmfNode {
    node(graph, &graph.root)
}

fn text(value: &AmfValue) -> &str {
    let AmfValue::String(text) = value else {
        panic!("expected string, got {value:?}")
    };
    text
}

fn array(node: &AmfNode) -> (&[AmfValue], &[(String, AmfValue)], Option<u32>) {
    let AmfNode::Array {
        dense,
        entries,
        amf0_length,
    } = node
    else {
        panic!("expected array, got {node:?}")
    };
    (dense, entries, *amf0_length)
}

// A container envelope independent of encode_sol, for literal wire and invalid-input cases.
fn container(name: &[u8], format: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"\x00\xbf\x00\x00\x00\x00TCSO\x00\x04\x00\x00\x00\x00".to_vec();
    bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&format.to_be_bytes());
    bytes.extend_from_slice(body);
    let length = (bytes.len() - 6) as u32;
    bytes[2..6].copy_from_slice(&length.to_be_bytes());
    bytes
}

#[test]
fn public_flash_containers_preserve_sparse_arrays_dates_xml_and_aliases() {
    for (name, format, bytes) in FIXTURES {
        let (decoded_name, decoded_format, decoded) = AmfDecoded::decode_sol(bytes).unwrap();
        assert_eq!(decoded_name, *name);
        assert_eq!(decoded_format, *format);
        assert_eq!(decoded.consumed, bytes.len());
        assert_eq!(
            u32::from_be_bytes(bytes[2..6].try_into().unwrap()) as usize,
            bytes.len() - 6
        );
        let graph = &decoded.graph;
        let data = root(graph);

        match (*name, *format) {
            ("RuffleTest", 0) => {
                assert_eq!(property(data, "saved"), &AmfValue::Bool(true));
                assert_eq!(property(data, "num"), &AmfValue::Number("10".into()));
                assert_eq!(text(property(data, "str")), "hello");
                let sparse = node(graph, property(data, "array"));
                let (dense, values, length) = array(sparse);
                assert!(dense.is_empty());
                assert_eq!(length, Some(5));
                assert_eq!(text(property(sparse, "0")), "elem0");
                assert_eq!(text(property(sparse, "4")), "elem4");
                assert!(
                    !values
                        .iter()
                        .any(|(name, _)| ["1", "2", "3"].contains(&name.as_str()))
                );
                assert_eq!(text(property(sparse, "prop")), "property");
                assert_eq!(text(property(sparse, "-1")), "elem negative one");
                assert_eq!(
                    node(graph, property(data, "date")),
                    &AmfNode::Date {
                        milliseconds: "2147483647".into()
                    }
                );
                assert_eq!(
                    node(graph, property(data, "testxml")),
                    &AmfNode::XmlDocument {
                        text: "<test>Test</test>".into()
                    }
                );
            }
            ("RuffleTest", 3) => {
                assert_eq!(
                    entries(data).len(),
                    1,
                    "non-enumerable hidden field was not saved"
                );
                let (props, _, _) = array(node(graph, property(data, "props")));
                assert_eq!(props.len(), 10);
                assert_eq!(props[0], AmfValue::Bool(true));
                assert_eq!(text(&props[1]), "hello");
                let sparse = node(graph, &props[6]);
                let (dense, values, length) = array(sparse);
                // The original file writes the contiguous index 0 as its dense
                // prefix, then index 4 and -1 as associative properties.
                assert_eq!(dense, &[AmfValue::String("elem0".into())]);
                assert_eq!(length, None);
                assert_eq!(text(property(sparse, "4")), "elem4");
                assert_eq!(text(property(sparse, "-1")), "elem negative one");
                assert_eq!(values.len(), 2);
                let (dense, named, _) = array(node(graph, &props[7]));
                assert_eq!(
                    dense,
                    &[
                        AmfValue::Number("1".into()),
                        AmfValue::Number("2".into()),
                        AmfValue::Number("3".into())
                    ]
                );
                assert!(named.is_empty());
                assert_eq!(
                    text(property(
                        node(graph, &props[8]),
                        "aPropertyThatIsNotANumber"
                    )),
                    "strange property"
                );
                assert_eq!(
                    node(graph, &props[9]),
                    &AmfNode::Date {
                        milliseconds: "2147483647".into()
                    }
                );
            }
            ("strict_array_test", 0) => {
                for (key, length, present) in [
                    ("denseArray", 2, vec!["0", "1"]),
                    ("sparseArray", 6, vec!["0", "5"]),
                    ("mixedArray", 1, vec!["0", "custom_prop"]),
                ] {
                    let (dense, values, hint) = array(node(graph, property(data, key)));
                    assert!(dense.is_empty());
                    assert_eq!(hint, Some(length));
                    assert_eq!(
                        values
                            .iter()
                            .map(|(name, _)| name.as_str())
                            .collect::<Vec<_>>(),
                        present
                    );
                }
                for key in ["fakeArray", "protoOnlyArray"] {
                    assert!(matches!(
                        node(graph, property(data, key)),
                        AmfNode::Object { .. }
                    ));
                }
            }
            ("amf_object_payload", 0) => {
                for (key, alias, value) in [
                    ("objPlain", None, "plain"),
                    ("objUnregistered", None, "bar"),
                    ("objRegistered", Some("com.test.RegisteredClass"), "baz"),
                    ("objMismatched", Some("CustomAMFAliasString"), "qux"),
                    (
                        "objForgedConstructor",
                        Some("com.test.RegisteredClass"),
                        "forged_ctor",
                    ),
                    ("objForgedProto", None, "forged_proto"),
                    ("objDoubleRegistered", Some("SecondAlias.Class"), "double"),
                ] {
                    let object = node(graph, property(data, key));
                    let AmfNode::Object { class_name, .. } = object else {
                        panic!("expected object")
                    };
                    assert_eq!(class_name.as_deref(), alias);
                    assert_eq!(text(property(object, "foo")), value);
                }
            }
            ("RuffleTestRef", 0) => {
                assert_eq!(property(data, "saved"), &AmfValue::Bool(true));
                let foo = property(data, "foo");
                assert_eq!(property(node(graph, foo), "self"), foo);
                assert_eq!(text(property(node(graph, foo), "name")), "Foo");
                // Unlike ordinary AMF0, the preceding Boolean occupies slot 0.
                assert_eq!(graph.encode_sol(name, *format).unwrap(), *bytes);
            }
            _ => unreachable!(),
        }

        let encoded = graph.encode_sol(name, *format).unwrap();
        let (_, _, reparsed) = AmfDecoded::decode_sol(&encoded).unwrap();
        assert_eq!(
            reparsed.graph, *graph,
            "semantic roundtrip for {name}/{format}"
        );
    }
}

#[test]
fn entry_stream_shares_object_string_and_trait_tables_without_registering_the_data_root() {
    let bodies: &[(u8, &[u8])] = &[
        (0, b"\x00\x01a\x10\x00\x05Point\x00\x04self\x07\x00\x00\x00\x05label\x02\x00\x05Point\x00\x00\x09\x00\x00\x01b\x07\x00\x00\x00\x00\x01c\x10\x00\x05Point\x00\x05label\x02\x00\x05Point\x00\x00\x09\x00"),
        (3, b"\x03a\x0a\x0b\x0bPoint\x09self\x0a\x00\x0blabel\x06\x02\x01\x00\x03b\x0a\x00\x00\x03c\x0a\x01\x06\x06\x02\x01\x00"),
    ];
    for (format, body) in bodies {
        let bytes = container(b"refs", u32::from(*format), body);
        let (_, _, decoded) = AmfDecoded::decode_sol(&bytes).unwrap();
        let graph = &decoded.graph;
        let data = root(graph);
        let a = property(data, "a");
        assert_ne!(a, &graph.root, "the SOL envelope is not a wire object");
        assert_eq!(a, property(data, "b"));
        assert_eq!(a, property(node(graph, a), "self"));
        assert_ne!(a, property(data, "c"));
        for key in ["a", "c"] {
            let object = node(graph, property(data, key));
            let AmfNode::Object { class_name, .. } = object else {
                panic!("expected typed object")
            };
            assert_eq!(class_name.as_deref(), Some("Point"));
            assert_eq!(text(property(object, "label")), "Point");
        }
        let encoded = graph.encode_sol("refs", *format).unwrap();
        assert_eq!(AmfDecoded::decode_sol(&encoded).unwrap().2.graph, *graph);
    }

    // Native SOL inline slots: saved=0, Date=1, XMLDocument=2, items=3,
    // its Number=4 and nested Object=5; reference markers do not advance it.
    // The subsequent Object therefore occupies slot 6. Property names do not
    // enter this table. This follows the pinned flash-lso writer, independent
    // of its decoder's inconsistent treatment of reference-marker slots.
    let body = b"\x00\x05saved\x01\x01\x00\x00\x04date\x0b\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03xml\x0f\x00\x00\x00\x04<x/>\x00\x00\x05items\x0a\x00\x00\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03\x00\x04self\x07\x00\x05\x00\x00\x09\x00\x00\x05again\x07\x00\x03\x00\x00\x05later\x03\x00\x04self\x07\x00\x06\x00\x00\x09\x00\x00\x0alaterAgain\x07\x00\x06\x00";
    let (_, _, decoded) = AmfDecoded::decode_sol(&container(b"slots", 0, body)).unwrap();
    let graph = &decoded.graph;
    let data = root(graph);
    let items = property(data, "items");
    assert_eq!(items, property(data, "again"));
    let (dense, _, _) = array(node(graph, items));
    assert_eq!(dense.len(), 2);
    assert_eq!(property(node(graph, &dense[1]), "self"), &dense[1]);
    let later = property(data, "later");
    assert_eq!(later, property(data, "laterAgain"));
    assert_eq!(later, property(node(graph, later), "self"));
    assert_eq!(
        graph.encode_sol("slots", 0).unwrap(),
        container(b"slots", 0, body)
    );
    // The same graph is valid ordinary AMF0 with its distinct object-only
    // numbering (including its explicitly encoded root), not SOL slot numbers.
    assert_eq!(
        amf_decode(0, &amf_encode(0, graph).unwrap()).unwrap().graph,
        *graph
    );

    // Each AVMPlus wrapper occupies one native AMF0 slot even when the value
    // inside it is an AMF3 back-reference. The AMF3 table remains independent.
    let body = b"\x00\x05bytes\x11\x0c\x03\xff\x00\x00\x04same\x11\x0c\x00\x00\x00\x03obj\x03\x00\x04self\x07\x00\x02\x00\x00\x09\x00";
    let bytes = container(b"avmplus", 0, body);
    let (_, _, decoded) = AmfDecoded::decode_sol(&bytes).unwrap();
    let graph = &decoded.graph;
    let data = root(graph);
    assert_eq!(property(data, "bytes"), property(data, "same"));
    let object = property(data, "obj");
    assert_eq!(property(node(graph, object), "self"), object);
    assert_eq!(graph.encode_sol("avmplus", 0).unwrap(), bytes);

    for (format, body) in [
        (0, &b"\x00\x01a\x07\x00\x00\x00"[..]),
        (0, &b"\x00\x01a\x01\x01\x00\x00\x01b\x07\x00\x00\x00"[..]),
        (3, &b"\x03a\x0a\x00\x00"[..]),
    ] {
        let bytes = container(b"refs", format, body);
        assert!(
            AmfDecoded::decode_sol(&bytes)
                .unwrap_err()
                .message
                .contains("reference")
        );
    }

    // A native slot index must fit the u16 wire reference even when most slots
    // contain inline scalars, which never enter the complex-object lookup map.
    let mut graph = AmfGraph {
        root: AmfValue::Ref(0),
        nodes: vec![
            AmfNode::Object {
                class_name: None,
                entries: vec![
                    ("prefix".into(), AmfValue::Ref(1)),
                    ("after".into(), AmfValue::Ref(2)),
                ],
            },
            AmfNode::Array {
                dense: vec![AmfValue::Null; 65_534],
                entries: vec![],
                amf0_length: None,
            },
            AmfNode::Object {
                class_name: None,
                entries: vec![("self".into(), AmfValue::Ref(2))],
            },
        ],
    };
    let bytes = graph.encode_sol("maximum", 0).unwrap();
    assert_eq!(AmfDecoded::decode_sol(&bytes).unwrap().2.graph, graph);
    let AmfNode::Array { dense, .. } = &mut graph.nodes[1] else {
        unreachable!()
    };
    dense.push(AmfValue::Null);
    assert!(
        graph
            .encode_sol("overflow", 0)
            .unwrap_err()
            .message
            .contains("reference index")
    );
    assert!(
        amf_encode(0, &graph).is_ok(),
        "ordinary AMF0 still has only three complex objects"
    );
}

#[test]
fn malformed_containers_fail_atomically_and_names_remain_logical_identifiers() {
    for (name, _, bytes) in FIXTURES {
        for cut in 0..bytes.len() {
            assert!(
                AmfDecoded::decode_sol(&bytes[..cut]).is_err(),
                "accepted truncated {name} at {cut}"
            );
        }
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(
            AmfDecoded::decode_sol(&trailing)
                .unwrap_err()
                .message
                .contains("trailing")
        );
        let mut bad_trailer = bytes.to_vec();
        *bad_trailer.last_mut().unwrap() = 1;
        assert!(
            AmfDecoded::decode_sol(&bad_trailer)
                .unwrap_err()
                .message
                .contains("trailer")
        );
        // Failure returns no partial graph and cannot poison the next independent read.
        assert!(AmfDecoded::decode_sol(bytes).is_ok());
    }
    for bytes in [
        container(b"save", 1, &[]),
        container(&[0xff], 0, &[]),
        container(b"save", 0, b"\x00\x01a\x05\x00\x00\x01a\x05\x00"),
        container(b"save", 3, b"\x03a\x01\x00\x00\x01\x00"),
        container(b"save", 0, b"\x00\x00\x05\x00"),
        container(b"save", 3, b"\x01\x01\x00"),
    ] {
        assert!(AmfDecoded::decode_sol(&bytes).is_err());
    }
    let valid = container(b"save", 3, &[]);
    for (at, byte) in [(0, 1), (6, b'X'), (11, 3)] {
        let mut invalid = valid.clone();
        invalid[at] = byte;
        assert!(AmfDecoded::decode_sol(&invalid).is_err());
    }
    for length in [
        0,
        (valid.len() - 7) as u32,
        (valid.len() - 5) as u32,
        u32::MAX,
    ] {
        let mut invalid = valid.clone();
        invalid[2..6].copy_from_slice(&length.to_be_bytes());
        assert!(AmfDecoded::decode_sol(&invalid).is_err());
    }

    let empty = AmfGraph {
        root: AmfValue::Null,
        nodes: vec![],
    };
    for name in [
        "arcade/save",
        "保存/данные",
        ".hidden/save",
        &"😀".repeat(512),
    ] {
        for format in [0, 3] {
            let bytes = empty.encode_sol(name, format).unwrap();
            assert_eq!(AmfDecoded::decode_sol(&bytes).unwrap().0, name);
        }
    }
    for name in [
        "",
        "/save",
        "save/",
        "save//slot",
        ".",
        "..",
        "arcade/../save",
        "arcade/./save",
        "save name",
        "save\nname",
        "save\0name",
        "save\u{feff}name",
        "save\u{a0}name",
        "save\\name",
        "save%20name",
        "save?name",
        &"😀".repeat(513),
    ] {
        assert!(empty.encode_sol(name, 0).is_err(), "encoded {name:?}");
        assert!(
            AmfDecoded::decode_sol(&container(name.as_bytes(), 0, &[])).is_err(),
            "decoded {name:?}"
        );
    }
}

#[test]
fn complete_container_size_includes_the_header_at_the_exact_limit() {
    // AMF0: 23-byte envelope, one-byte property name (3 bytes), long-string
    // marker/length (5 bytes), and one trailer byte: payload + 32 in total.
    let mut graph = AmfGraph {
        root: AmfValue::Ref(0),
        nodes: vec![AmfNode::Object {
            class_name: None,
            entries: vec![("a".into(), AmfValue::String("x".repeat(MAX_AMF_BYTES - 32)))],
        }],
    };
    let mut bytes = graph.encode_sol("s", 0).unwrap();
    assert_eq!(bytes.len(), MAX_AMF_BYTES);
    let decoded = AmfDecoded::decode_sol(&bytes).unwrap().2;
    assert_eq!(decoded.consumed, MAX_AMF_BYTES);
    assert_eq!(decoded.graph, graph);
    drop(decoded);
    bytes.push(0);
    assert!(
        AmfDecoded::decode_sol(&bytes)
            .unwrap_err()
            .message
            .contains("16 MiB")
    );
    let AmfNode::Object { entries, .. } = &mut graph.nodes[0] else {
        unreachable!()
    };
    let AmfValue::String(value) = &mut entries[0].1 else {
        unreachable!()
    };
    value.push('x');
    assert!(
        graph
            .encode_sol("s", 0)
            .unwrap_err()
            .message
            .contains("16 MiB")
    );
}
