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

#[test]
fn x509_chain_verification_cross_domain_policy_and_netgroup_replication() {
    // 1. X.509 chain verification: trusted CA, untrusted signer, mismatch, expired
    let ca_der = build_custom_x509_der(
        "NeoFlash Root CA",
        "NeoFlash Root CA",
        "20250101000000Z",
        "20350101000000Z",
    );
    let leaf_der = build_custom_x509_der(
        "*.api.neoflash.dev",
        "NeoFlash Root CA",
        "20250101000000Z",
        "20350101000000Z",
    );
    let now_2026 = 1_795_000_000_000.0;

    let (st_ok, _) = verify_x509_certificate_chain(
        &leaf_der,
        "edge.api.neoflash.dev",
        now_2026,
        &[(ca_der.clone(), true)],
        false,
    );
    assert_eq!(st_ok, "trusted");

    let (st_untrusted, _) = verify_x509_certificate_chain(
        &leaf_der,
        "edge.api.neoflash.dev",
        now_2026,
        &[(ca_der.clone(), false)],
        false,
    );
    assert_eq!(st_untrusted, "untrustedSigners");

    let (st_mismatch, _) = verify_x509_certificate_chain(
        &leaf_der,
        "wrong.domain.org",
        now_2026,
        &[(ca_der.clone(), true)],
        false,
    );
    assert_eq!(st_mismatch, "principalMismatch");

    let expired_der = build_custom_x509_der(
        "edge.api.neoflash.dev",
        "NeoFlash Root CA",
        "20200101000000Z",
        "20220101000000Z",
    );
    let (st_expired, _) = verify_x509_certificate_chain(
        &expired_der,
        "edge.api.neoflash.dev",
        now_2026,
        &[(ca_der, true)],
        false,
    );
    assert_eq!(st_expired, "expired");

    // 2. Cross-domain policy XML parsing and port/domain/secure evaluation
    let xml = r#"<?xml version="1.0"?>
        <cross-domain-policy>
            <site-control permitted-cross-domain-policies="master-only"/>
            <allow-access-from domain="*.partner.example.com" to-ports="443,8080-8090" secure="true"/>
            <allow-http-request-headers-from domain="*.partner.example.com" headers="X-Api-Key,Authorization" secure="false"/>
        </cross-domain-policy>"#;
    let policy = parse_cross_domain_policy_xml(xml).unwrap();
    assert_eq!(policy.site_control, "master-only");
    assert!(policy_allows_access(
        &policy,
        "app.partner.example.com",
        Some(8085),
        true
    ));
    assert!(!policy_allows_access(
        &policy,
        "app.partner.example.com",
        Some(8085),
        false
    ));
    assert!(!policy_allows_access(
        &policy,
        "app.partner.example.com",
        Some(9090),
        true
    ));
    assert!(!policy_allows_access(&policy, "evil.org", Some(443), true));
    assert_eq!(
        extract_policy_target_host("http://127.0.0.1:8080/crossdomain.xml"),
        "127.0.0.1"
    );
    assert_eq!(
        extract_policy_target_host("xmlsocket://api.example.com:843"),
        "api.example.com"
    );
    let exact_xml = r#"<cross-domain-policy><allow-access-from domain="example.org" to-ports="80,443" secure="false"/></cross-domain-policy>"#;
    let exact_pol = parse_cross_domain_policy_xml(exact_xml).unwrap();
    assert!(!policy_allows_access_with_settings(
        &exact_pol,
        "sub.example.org",
        None,
        false,
        true
    ));
    assert!(policy_allows_access_with_settings(
        &exact_pol,
        "sub.example.org",
        None,
        false,
        false
    ));
    assert!(!policy_allows_access_with_settings(
        &exact_pol,
        "localhost",
        None,
        false,
        false
    ));

    // 3. GroupSpecifier auth commitment + NetGroup posting auth & rarestFirst object replication
    let mut gs = GroupSpecifierState::new("replicated-room", 1).unwrap();
    gs.posting_enabled = true;
    gs.object_replication_enabled = true;
    gs.set_posting_password(Some("room-secret"), Some("s1"));
    let with_auth = gs.groupspec_with_authorizations();
    let without_auth = gs.groupspec_without_authorizations();

    let mut kernel = As3NetSysKernel::new();
    let g1: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({ "op": "netgroup_new", "groupspec": with_auth, "peerID": "peer-seed-1" })
                    .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    let g2: Value = serde_json::from_str(
        &kernel
            .execute_json(&json!({ "op": "netgroup_new", "groupspec": without_auth, "peerID": "peer-leech-2" }).to_string())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(g1["canonicalBase"], g2["canonicalBase"]);
    assert_eq!(g1["postingAuthorized"], json!(true));
    assert_eq!(g2["postingAuthorized"], json!(false));

    // Unauthorized member cannot post; authorized member posts to unauthorized subscriber!
    let denied_post: Value = serde_json::from_str(
        &kernel
            .execute_json(&json!({ "op": "netgroup_op", "id": g2["id"], "action": "post", "payloadSummary": "nope", "byteLength": 4 }).to_string())
            .unwrap(),
    )
    .unwrap();
    assert!(denied_post["messageID"].is_null());

    let ok_post: Value = serde_json::from_str(
        &kernel
            .execute_json(&json!({ "op": "netgroup_op", "id": g1["id"], "action": "post", "payloadSummary": "hello", "byteLength": 32 }).to_string())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ok_post["recipients"], json!([g2["id"]]));

    // Object replication: seed has [0..2], leech wants [1..2] -> ticket scheduled -> reply updates send & receive bytes!
    kernel
        .execute_json(&json!({ "op": "netgroup_op", "id": g1["id"], "action": "have_want", "kind": "addHave", "start": 0, "end": 2 }).to_string())
        .unwrap();
    let want_res: Value = serde_json::from_str(
        &kernel
            .execute_json(&json!({ "op": "netgroup_op", "id": g2["id"], "action": "have_want", "kind": "addWant", "start": 1, "end": 1 }).to_string())
            .unwrap(),
    )
    .unwrap();
    let req_id = want_res["fetchRequests"][0]["requestID"].as_i64().unwrap();
    assert_eq!(want_res["fetchRequests"][0]["index"], json!(1));

    let reply_res: Value = serde_json::from_str(
        &kernel
            .execute_json(&json!({ "op": "netgroup_op", "id": g1["id"], "action": "replication_reply", "requestID": req_id, "byteLength": 128, "deny": false }).to_string())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply_res["requesterId"], g2["id"]);
    assert_eq!(reply_res["index"], json!(1));

    let snap2: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({ "op": "netgroup_op", "id": g2["id"], "action": "snapshot" }).to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        snap2["info"]["objectReplicationReceiveBytesPerSecond"],
        json!(128.0)
    );
}

#[test]
fn drm_binary_headers_policy_return_and_fips_aes128_ctr_media_decryption() {
    use super::file_print_sampler::aes128_ctr_xor;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;

    // 1. Verify NIST FIPS-197 Appendix B / C.1 exact AES-128 block cipher vector
    let nist_key: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];
    let nist_ctr0: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];
    let zeros = [0u8; 16];
    let ks = aes128_ctr_xor(&nist_key, &nist_ctr0, &zeros);
    assert_eq!(
        ks,
        vec![
            0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
            0xc5, 0x5a
        ]
    );

    let mut kernel = As3NetSysKernel::new();

    // 2. Parse Flash Access binary TLV metadata ("FADRM\x01") with deviceGroup
    let mut fadrm = Vec::from(&b"FADRM\x01"[..]);
    for (tag, val) in [
        (1u8, "https://lic.example.org/drm"),
        (2u8, "lic-fadrm-42"),
        (3u8, "cinema-dom"),
        (4u8, "usernameAndPassword"),
        (5u8, "pol-hd"),
        (6u8, "HD Rental"),
        (7u8, "living-room-group"),
    ] {
        fadrm.push(tag);
        fadrm.extend_from_slice(&(val.len() as u16).to_be_bytes());
        fadrm.extend_from_slice(val.as_bytes());
    }
    let parsed_fadrm: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({ "op": "drm_parse_content", "base64": STANDARD.encode(&fadrm) })
                    .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(parsed_fadrm["licenseID"], "lic-fadrm-42");
    assert_eq!(parsed_fadrm["authenticationMethod"], "usernameAndPassword");
    assert_eq!(
        parsed_fadrm["accessInfo"][0]["deviceGroup"]["name"],
        "living-room-group"
    );

    // 3. Parse ISO-BMFF CENC v1 'pssh' box with KID
    let mut pssh_body = Vec::new();
    pssh_body.extend_from_slice(&[1, 0, 0, 0]); // version = 1
    pssh_body.extend_from_slice(&[0xAA; 16]); // system_id
    pssh_body.extend_from_slice(&1u32.to_be_bytes()); // kid_count = 1
    pssh_body.extend_from_slice(&[0x12; 16]); // KID
    pssh_body.extend_from_slice(&0u32.to_be_bytes()); // data_len = 0
    let mut pssh = Vec::new();
    pssh.extend_from_slice(&((8 + pssh_body.len()) as u32).to_be_bytes());
    pssh.extend_from_slice(b"pssh");
    pssh.extend_from_slice(&pssh_body);
    let parsed_pssh: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({ "op": "drm_parse_content", "base64": STANDARD.encode(&pssh) }).to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        parsed_pssh["licenseID"],
        "kid-12121212121212121212121212121212"
    );

    // 4. Encrypt media, verify refusal before authentication/voucher, authenticate, load voucher, decrypt, and revoke via return_voucher(policyID)
    let cleartext = b"FLV\x01\x05\x00\x00\x00\x09decryption-verified-payload!";
    let enc_res: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({
                    "op": "drm_op",
                    "action": "encrypt_media",
                    "metadataBase64": STANDARD.encode(&fadrm),
                    "cleartextBase64": STANDARD.encode(cleartext),
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    let enc_b64 = enc_res["encryptedBase64"].as_str().unwrap().to_owned();

    let denied: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({ "op": "drm_op", "action": "decrypt_media", "base64": enc_b64, "nowMs": 1_762_000_100_000.0 })
                    .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["needsVoucher"], true);

    kernel
        .execute_json(
            &json!({
                "op": "drm_op",
                "action": "authenticate",
                "serverURL": "https://lic.example.org/drm",
                "domain": "cinema-dom",
                "username": "alice",
                "password": "secret",
            })
            .to_string(),
        )
        .unwrap();
    kernel
        .execute_json(
            &json!({
                "op": "drm_op",
                "action": "load_voucher",
                "serverURL": "https://lic.example.org/drm",
                "licenseID": "lic-fadrm-42",
                "policyID": "pol-hd",
                "domain": "cinema-dom",
                "authenticationMethod": "usernameAndPassword",
                "setting": "allowServer",
                "nowMs": 1_762_000_000_000.0,
            })
            .to_string(),
        )
        .unwrap();

    let decrypted: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({ "op": "drm_op", "action": "decrypt_media", "base64": enc_b64, "nowMs": 1_762_000_100_000.0 })
                    .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(decrypted["ok"], true);
    assert_eq!(
        STANDARD
            .decode(decrypted["cleartextBase64"].as_str().unwrap())
            .unwrap(),
        cleartext
    );

    // Non-matching policyID does NOT remove voucher; matching policyID removes it!
    let ret_miss: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({
                    "op": "drm_op",
                    "action": "return_voucher",
                    "serverURL": "https://lic.example.org/drm",
                    "policyID": "pol-sd",
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ret_miss["removed"], 0);

    let ret_hit: Value = serde_json::from_str(
        &kernel
            .execute_json(
                &json!({
                    "op": "drm_op",
                    "action": "return_voucher",
                    "serverURL": "https://lic.example.org/drm",
                    "policyID": "pol-hd",
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(ret_hit["removed"], 1);
}
