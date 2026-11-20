use super::*;

#[test]
fn fast_memory_opcodes_and_sign_extension() {
    assert_eq!(fast_mem_sxi1(0), 0);
    assert_eq!(fast_mem_sxi1(1), -1);
    assert_eq!(fast_mem_sxi8(0x80), -128);
    assert_eq!(fast_mem_sxi8(0x7f), 127);
    assert_eq!(fast_mem_sxi16(0x8000), -32768);
    assert_eq!(fast_mem_sxi16(0x7fff), 32767);

    let mut mem = vec![0u8; MIN_DOMAIN_MEMORY_LENGTH];
    fast_mem_write(&mut mem, "si8", 0, 250.0).unwrap();
    assert_eq!(fast_mem_read(&mem, "li8", 0).unwrap(), 250.0);
    fast_mem_write(&mut mem, "si16", 4, 0xCAFE as f64).unwrap();
    assert_eq!(fast_mem_read(&mem, "li16", 4).unwrap(), 0xCAFE as f64);
    fast_mem_write(&mut mem, "si32", 8, -123456789.0).unwrap();
    assert_eq!(fast_mem_read(&mem, "li32", 8).unwrap(), -123456789.0);
    fast_mem_write(&mut mem, "sf32", 16, 3.5).unwrap();
    assert!((fast_mem_read(&mem, "lf32", 16).unwrap() - 3.5).abs() < 1e-6);
    fast_mem_write(&mut mem, "sf64", 24, std::f64::consts::PI).unwrap();
    assert!((fast_mem_read(&mem, "lf64", 24).unwrap() - std::f64::consts::PI).abs() < 1e-12);
    assert!(fast_mem_read(&mem, "li32", MIN_DOMAIN_MEMORY_LENGTH - 2).is_err());
}

#[test]
fn sha256_and_csprng_bounds() {
    // NIST test vector for "abc"
    assert_eq!(
        hex_encode(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let mut counter = 1u64;
    assert!(generate_random_bytes(0, &[], &mut counter).is_err());
    assert!(generate_random_bytes(1025, &[], &mut counter).is_err());
    let b1 = generate_random_bytes(32, &[7u8; 32], &mut counter).unwrap();
    let b2 = generate_random_bytes(32, &[7u8; 32], &mut counter).unwrap();
    assert_eq!(b1.len(), 32);
    assert_ne!(b1, b2);
    let b1024 = generate_random_bytes(1024, &[0x5au8; 64], &mut counter).unwrap();
    assert_eq!(b1024.len(), 1024);
}

#[test]
fn locale_id_currency_collator_and_string_tools() {
    let loc = parse_locale_id("zh_Hans_CN@collation=pinyin;currency=CNY", "en-US");
    assert_eq!(loc.language, "zh");
    assert_eq!(loc.script, "Hans");
    assert_eq!(loc.region, "CN");
    assert_eq!(
        loc.keys.get("collation").map(String::as_str),
        Some("pinyin")
    );
    assert!(!loc.rtl);

    let ar = parse_locale_id("ar-EG", "en-US");
    assert!(ar.rtl);
    assert_eq!(ar.script, "Arab");

    let pref = determine_preferred_locales(
        &["fr-CA".into(), "de-AT".into()],
        &["en-US".into(), "de-DE".into(), "fr-FR".into()],
        "userinterface",
    );
    assert_eq!(pref, vec!["fr-FR".to_owned(), "de-DE".to_owned()]);

    // Osmanya U+104A0 national digits in CurrencyFormatter
    let mut cf = CurrencyFormatterState::new("en-US", "en-US");
    assert!(cf.is_symbol_safe("USD").unwrap());
    assert!(!cf.is_symbol_safe("EUR").unwrap());
    assert_eq!(cf.format(1234.5, true).unwrap(), "$1,234.50");
    assert_eq!(cf.format(-1234.5, false).unwrap(), "(USD1,234.50)");
    let parsed = cf.parse("($1,234.50)").unwrap();
    assert_eq!(parsed["value"], json!(-1234.5));
    assert_eq!(parsed["currencyString"], json!("$"));
    cf.set_property("digitsType", json!(0x104A0)).unwrap();
    let osm = cf.format(42.0, true).unwrap();
    assert!(osm.contains('\u{104A4}'));
    assert_eq!(cf.parse(&osm).unwrap()["value"], json!(42.0));

    // Collator matching vs sorting + numeric + kana + width
    let mut match_col = CollatorState::new("en-US", "matching", "en-US").unwrap();
    assert!(match_col.equals("Café", "cafe"));
    assert!(match_col.equals("ｶﾀｶﾅ", "かたかな"));
    assert!(match_col.equals("ＡＢＣ", "abc"));

    let mut sort_col = CollatorState::new("en-US", "sorting", "en-US").unwrap();
    assert_eq!(sort_col.compare("file10", "file2"), -1);
    sort_col.set_bool("numericComparison", true).unwrap();
    assert_eq!(sort_col.compare("file2", "file10"), -1);

    // StringTools Turkish dotted/dotless I & Greek final sigma
    let mut tr = StringToolsState::new("tr-TR", "en-US");
    assert_eq!(tr.to_lower("Iİ"), "ıi");
    assert_eq!(tr.to_upper("ıi"), "Iİ");
    let mut el = StringToolsState::new("el-GR", "en-US");
    assert_eq!(el.to_lower("ΟΔΥΣΣΕΥΣ"), "οδυσσευς");
}

#[test]
fn groupspec_flv_x509_and_kernel_workflows() {
    let mut gs = GroupSpecifierState::new("room-alpha", 1).unwrap();
    gs.posting_enabled = true;
    gs.routing_enabled = true;
    let base1 = gs.groupspec_without_authorizations();
    gs.set_posting_password(Some("secret"), Some("salt1"));
    assert_ne!(
        gs.groupspec_with_authorizations(),
        gs.groupspec_without_authorizations()
    );
    assert!(!gs.authorizations().is_empty());
    assert_ne!(gs.groupspec_without_authorizations(), base1);

    // Build minimal FLV with header + script tag ("onMetaData", {"duration": 2.5})
    let mut flv = Vec::from(&b"FLV\x01\x05\x00\x00\x00\x09\x00\x00\x00\x00"[..]);
    let mut script_payload = Vec::new();
    script_payload.extend_from_slice(&[0x02, 0x00, 0x0a]);
    script_payload.extend_from_slice(b"onMetaData");
    script_payload.push(0x03);
    script_payload.extend_from_slice(&[0x00, 0x08]);
    script_payload.extend_from_slice(b"duration");
    script_payload.push(0x00);
    script_payload.extend_from_slice(&2.5f64.to_be_bytes());
    script_payload.extend_from_slice(&[0x00, 0x00, 0x09]);
    let dsize = script_payload.len();
    flv.push(18);
    flv.extend_from_slice(&[(dsize >> 16) as u8, (dsize >> 8) as u8, dsize as u8]);
    flv.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]);
    flv.extend_from_slice(&script_payload);
    flv.extend_from_slice(&((11 + dsize) as u32).to_be_bytes());

    let mut flv_state = FlvStreamState::default();
    let res = flv_state.append_bytes(&flv);
    assert_eq!(res["callbacks"][0]["handler"], json!("onMetaData"));
    assert_eq!(res["callbacks"][0]["info"]["duration"], json!(2.5));

    // Loopback X.509 DER generation + roundtrip ASN.1 parse
    let der = build_loopback_x509_der("secure.example.test");
    let cert = parse_x509_certificate(&der).unwrap();
    assert_eq!(cert.version, 3);
    assert_eq!(
        cert.subject.common_name.as_deref(),
        Some("secure.example.test")
    );
    assert_eq!(cert.issuer.country_name.as_deref(), Some("US"));
    assert!(cert.valid_not_after_ms > cert.valid_not_before_ms);
}
