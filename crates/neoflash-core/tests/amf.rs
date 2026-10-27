use neoflash_core::*;

#[test]
fn amf_graph_wire_and_bounded_cursor_workflow() {
    use AmfValue as V;
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
            entries: vec![("a".into(), V::Number("7".into()))]
        }
    );
    assert_eq!(
        sealed.graph.nodes[2],
        AmfNode::Object {
            entries: vec![("a".into(), V::Number("8".into()))]
        }
    );

    let graph = AmfGraph {
        root: V::Ref(0),
        nodes: vec![
            AmfNode::Object {
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
            },
            AmfNode::Object {
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
        let AmfNode::Object { entries } = &decoded.graph.nodes[0] else {
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
