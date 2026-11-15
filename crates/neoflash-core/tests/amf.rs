use neoflash_core::*;

#[test]
fn amf_graph_wire_and_bounded_cursor_workflow() {
    use AmfValue as V;
    xml_and_sparse_wire_values();
    let primitive = |root| AmfGraph {
        root,
        nodes: vec![],
    };
    assert_eq!(
        amf_encode(0, &primitive(V::Number("1.5".into()))).unwrap(),
        [0, 0x3f, 0xf8, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        amf_encode(3, &primitive(V::Number("-1".into()))).unwrap(),
        [4, 255, 255, 255, 255]
    );
    assert_eq!(amf_decode(0, &[1, 255]).unwrap().graph.root, V::Bool(true));
    let self_array = AmfGraph {
        root: V::Ref(0),
        nodes: vec![AmfNode::Array {
            dense: vec![V::Ref(0)],
            entries: vec![],
            amf0_length: None,
        }],
    };
    assert_eq!(amf_encode(3, &self_array).unwrap(), [9, 3, 1, 9, 0]);
    assert_eq!(amf_decode(3, &[9, 3, 1, 9, 0]).unwrap().graph, self_array);
    // Independent, literal wire fixture exercises string and trait references.
    let strings = [9, 5, 1, 6, 7, b'f', b'o', b'o', 6, 0];
    assert!(
        matches!(&amf_decode(3,&strings).unwrap().graph.nodes[0],AmfNode::Array {dense,..} if dense==&[V::String("foo".into()),V::String("foo".into())])
    );
    let sealed = [9, 5, 1, 10, 0x13, 1, 3, b'a', 4, 7, 10, 1, 4, 8];
    let sealed = amf_decode(3, &sealed).unwrap();
    assert_eq!(
        sealed.graph.nodes[1],
        AmfNode::Object {
            class_name: None,
            entries: vec![("a".into(), V::Number("7".into()))]
        }
    );
    assert_eq!(
        sealed.graph.nodes[2],
        AmfNode::Object {
            class_name: None,
            entries: vec![("a".into(), V::Number("8".into()))]
        }
    );

    let typed = AmfGraph {
        root: V::Ref(0),
        nodes: vec![
            AmfNode::Object {
                class_name: Some("nf.SaveEntry".into()),
                entries: vec![
                    ("score".into(), V::Number("42".into())),
                    ("peer".into(), V::Ref(1)),
                ],
            },
            AmfNode::Object {
                class_name: Some("nf.SaveEntry".into()),
                entries: vec![("score".into(), V::Number("99".into()))],
            },
        ],
    };
    for format in [0, 3] {
        let encoded = amf_encode(format, &typed).unwrap();
        assert_eq!(encoded[0], if format == 0 { 16 } else { 10 });
        assert_eq!(amf_decode(format, &encoded).unwrap().graph, typed);
    }

    let graph = AmfGraph {
        root: V::Ref(0),
        nodes: vec![
            AmfNode::Object {
                class_name: None,
                entries: vec![
                    ("self".into(), V::Ref(0)),
                    ("array".into(), V::Ref(1)),
                    ("record".into(), V::Ref(2)),
                    ("again".into(), V::Ref(2)),
                    ("date".into(), V::Ref(3)),
                    ("dateAgain".into(), V::Ref(3)),
                    ("bytes".into(), V::Ref(4)),
                    ("bytesAgain".into(), V::Ref(4)),
                ],
            },
            AmfNode::Array {
                dense: vec![
                    V::Undefined,
                    V::Null,
                    V::Bool(true),
                    V::Number("NaN".into()),
                    V::Number("Infinity".into()),
                    V::Number("-Infinity".into()),
                    V::Number("-0".into()),
                    V::Number("-268435456".into()),
                    V::Number("268435455".into()),
                    V::Number("268435456".into()),
                    V::String("same".into()),
                    V::String("same".into()),
                ],
                entries: vec![("label".into(), V::String("same".into()))],
                amf0_length: None,
            },
            AmfNode::Object {
                class_name: None,
                entries: vec![
                    ("owner".into(), V::Ref(0)),
                    ("word".into(), V::String("same".into())),
                ],
            },
            AmfNode::Date {
                milliseconds: "1700000000123".into(),
            },
            AmfNode::Bytes {
                base64: "AAH+/w==".into(),
            },
        ],
    };
    for format in [0, 3] {
        let bytes = amf_encode(format, &graph).unwrap();
        assert_eq!(bytes[0], if format == 0 { 3 } else { 10 });
        let mut stream = bytes.clone();
        stream.extend(amf_encode(format, &primitive(V::String("tail".into()))).unwrap());
        let decoded = amf_decode(format, &stream).unwrap();
        assert_eq!(decoded.consumed, bytes.len());
        assert_eq!(
            amf_decode(format, &stream[decoded.consumed..])
                .unwrap()
                .graph
                .root,
            V::String("tail".into())
        );
        assert_eq!(amf_encode(format, &decoded.graph).unwrap(), bytes);
        let AmfNode::Object { entries, .. } = &decoded.graph.nodes[0] else {
            panic!("root record")
        };
        assert_eq!(entries[0], ("self".into(), V::Ref(0)));
        assert_eq!(entries[2].1, entries[3].1);
        assert_eq!(entries[6].1, entries[7].1);
        if format == 3 {
            assert_eq!(entries[4].1, entries[5].1);
        } else {
            assert_ne!(entries[4].1, entries[5].1);
        } // AMF0 dates have no identity table.
        for cut in 0..bytes.len() {
            assert!(
                amf_decode(format, &bytes[..cut]).is_err(),
                "format {format}, cut {cut}"
            );
        }
    }
    for bytes in [
        &[6, 0][..],
        &[6, 3, 255],
        &[9, 3, 1, 10, 0],
        &[9, 255, 255, 255, 255],
        &[10, 7, 1],
        &[10, 11, 3, b'A'],
        &[13, 1],
        &[10, 1],
    ] {
        assert!(amf_decode(3, bytes).is_err());
    }
    assert!(amf_decode(0, &[7, 0, 0]).is_err());
    assert!(amf_decode(0, &[10, 255, 255, 255, 255]).is_err());
    assert!(
        amf_encode(
            3,
            &AmfGraph {
                root: V::Ref(7),
                nodes: vec![]
            }
        )
        .is_err()
    );
    assert!(amf_encode(3, &primitive(V::String("x".repeat(MAX_AMF_BYTES + 1)))).is_err());
    let chain = AmfGraph {
        root: V::Ref(0),
        nodes: (0..130)
            .map(|i| AmfNode::Array {
                dense: vec![if i == 129 { V::Null } else { V::Ref(i + 1) }],
                entries: vec![],
                amf0_length: None,
            })
            .collect(),
    };
    assert!(amf_encode(3, &chain).is_err());
    let mut deep = vec![];
    for _ in 0..130 {
        deep.extend([9, 3, 1]);
    }
    deep.push(1);
    assert!(amf_decode(3, &deep).is_err());
}

fn xml_and_sparse_wire_values() {
    use AmfValue as V;
    // Adobe AMF3 sections 3.9/3.13 distinguish the two XML types and put
    // both in the object reference table, independently of string references.
    for (marker, node) in [
        (
            7,
            AmfNode::XmlDocument {
                text: "<x/>".into(),
            },
        ),
        (
            11,
            AmfNode::Xml {
                text: "<x/>".into(),
            },
        ),
    ] {
        let inline = [marker, 9, b'<', b'x', b'/', b'>'];
        let graph = AmfGraph {
            root: V::Ref(0),
            nodes: vec![node.clone()],
        };
        assert_eq!(amf_decode(3, &inline).unwrap().graph, graph);
        assert_eq!(amf_encode(3, &graph).unwrap(), inline);
        let repeated = AmfGraph {
            root: V::Ref(0),
            nodes: vec![
                AmfNode::Array {
                    dense: vec![V::Ref(1), V::Ref(1)],
                    entries: vec![],
                    amf0_length: None,
                },
                node,
            ],
        };
        let wire = [9, 5, 1, marker, 9, b'<', b'x', b'/', b'>', marker, 2];
        assert_eq!(amf_encode(3, &repeated).unwrap(), wire);
        assert_eq!(amf_decode(3, &wire).unwrap().graph, repeated);
        let legacy_wire = amf_encode(0, &repeated).unwrap();
        let legacy = amf_decode(0, &legacy_wire).unwrap();
        let AmfNode::Array { dense, .. } = &legacy.graph.nodes[0] else {
            panic!("array")
        };
        assert_eq!(
            dense[0] == dense[1],
            marker == 11,
            "AMF0 XMLDocument is inline; E4X uses AMF3 references"
        );
        assert_eq!(amf_encode(0, &legacy.graph).unwrap(), legacy_wire);
    }
    let document_wire = [15, 0, 0, 0, 4, b'<', b'x', b'/', b'>'];
    let document = amf_decode(0, &document_wire).unwrap();
    assert_eq!(
        document.graph.nodes,
        vec![AmfNode::XmlDocument {
            text: "<x/>".into()
        }]
    );
    assert_eq!(amf_encode(0, &document.graph).unwrap(), document_wire);
    let e4x = AmfGraph {
        root: V::Ref(0),
        nodes: vec![AmfNode::Xml {
            text: "<x/>".into(),
        }],
    };
    assert_eq!(
        amf_encode(0, &e4x).unwrap(),
        [17, 11, 9, b'<', b'x', b'/', b'>']
    );
    assert_eq!(
        amf_decode(0, &[17, 11, 9, b'<', b'x', b'/', b'>'])
            .unwrap()
            .graph,
        e4x
    );
    for wire in [
        &[11, 3, 255][..],                               // Invalid UTF-8.
        &[7, 11, b'<', b'x', b'/', b'>'],                // Truncated payload.
        &[9, 5, 1, 7, 9, b'<', b'x', b'/', b'>', 11, 2], // Wrong XML reference type.
    ] {
        assert!(amf_decode(3, wire).is_err());
    }
    assert!(amf_decode(0, &[15, 255, 255, 255, 255]).is_err());

    // Literal sparse form from the AMF3 array grammar: three dense values,
    // numeric associative key "20", and no properties at indices 3 through 19.
    let sparse3 = [9, 7, 5, b'2', b'0', 4, 2, 1, 4, 1, 4, 2, 4, 3];
    let decoded = amf_decode(3, &sparse3).unwrap();
    assert_eq!(
        decoded.graph.nodes[0],
        AmfNode::Array {
            dense: vec![
                V::Number("1".into()),
                V::Number("2".into()),
                V::Number("3".into())
            ],
            entries: vec![("20".into(), V::Number("2".into()))],
            amf0_length: None,
        }
    );
    assert_eq!(amf_encode(3, &decoded.graph).unwrap(), sparse3);
    let sparse_cycle = [9, 1, 3, b'5', 9, 0, 1];
    let decoded = amf_decode(3, &sparse_cycle).unwrap();
    assert_eq!(
        decoded.graph.nodes[0],
        AmfNode::Array {
            dense: vec![],
            entries: vec![("5".into(), V::Ref(0))],
            amf0_length: None,
        }
    );
    assert_eq!(amf_encode(3, &decoded.graph).unwrap(), sparse_cycle);
    // AMF0 ECMA keys preserve holes and the native length hint; strict arrays
    // instead encode every gap as an explicit Undefined (AMF0 section 2.12).
    let sparse0 = [
        8, 0, 0, 0, 6, 0, 1, b'0', 6, 0, 1, b'5', 2, 0, 1, b'x', 0, 0, 9,
    ];
    let decoded = amf_decode(0, &sparse0).unwrap();
    assert_eq!(
        decoded.graph.nodes[0],
        AmfNode::Array {
            dense: vec![],
            entries: vec![
                ("0".into(), V::Undefined),
                ("5".into(), V::String("x".into()))
            ],
            amf0_length: Some(6),
        }
    );
    assert_eq!(amf_encode(0, &decoded.graph).unwrap(), sparse0);
    let strict = amf_decode(0, &[10, 0, 0, 0, 3, 6, 6, 1, 1]).unwrap();
    assert_eq!(
        strict.graph.nodes[0],
        AmfNode::Array {
            dense: vec![V::Undefined, V::Undefined, V::Bool(true)],
            entries: vec![],
            amf0_length: None,
        }
    );
    let mut mixed = AmfGraph {
        root: V::Ref(0),
        nodes: vec![AmfNode::Array {
            dense: vec![V::Null],
            entries: vec![("tag".into(), V::Bool(true))],
            amf0_length: None,
        }],
    };
    assert_eq!(
        &amf_encode(0, &mixed).unwrap()[1..5],
        &[0, 0, 0, 1],
        "named keys do not increase native ECMA length"
    );
    mixed.nodes[0] = AmfNode::Array {
        dense: vec![],
        entries: vec![("99999".into(), V::Null)],
        amf0_length: Some(100000),
    };
    let decoded = amf_decode(0, &amf_encode(0, &mixed).unwrap()).unwrap();
    assert_eq!(
        decoded.graph, mixed,
        "large sparse indices never allocate dense placeholders"
    );
    mixed.nodes[0] = AmfNode::Array {
        dense: vec![],
        entries: vec![],
        amf0_length: Some(100000),
    };
    assert_eq!(
        amf_encode(3, &mixed).unwrap(),
        [9, 1, 1],
        "AMF3 has no separate length hint or invented trailing holes"
    );
    for node in [
        AmfNode::Array {
            dense: vec![],
            entries: vec![],
            amf0_length: Some(100001),
        },
        AmfNode::Array {
            dense: vec![],
            entries: vec![("100000".into(), V::Null)],
            amf0_length: None,
        },
    ] {
        mixed.nodes[0] = node;
        assert!(amf_encode(0, &mixed).is_err());
    }
    assert!(amf_decode(0, &[8, 0, 1, 0x86, 0xa1, 0, 0, 9]).is_err());

    // Native dates retain invalid-date NaN; ByteArray uses the standard AVM+
    // switch when embedded in an AMF0 context.
    let date = amf_decode(3, &[8, 1, 0x7f, 0xf8, 0, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(
        date.graph.nodes,
        vec![AmfNode::Date {
            milliseconds: "NaN".into()
        }]
    );
    let bytes = amf_decode(0, &[17, 12, 9, 0, 1, 254, 255]).unwrap();
    assert_eq!(
        bytes.graph.nodes,
        vec![AmfNode::Bytes {
            base64: "AAH+/w==".into()
        }]
    );
}
