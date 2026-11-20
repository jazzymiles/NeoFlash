use super::encrypted_store::{SQL_ENC_VFS_MAGIC, encrypt_sql_payload, generate_sql_salt_and_nonce};
use super::network_security::{
    extract_all_xml_tag_blocks, extract_der_oid_string, parse_cert_metadata,
};
use super::*;

#[test]
fn air_path_resolution_urls_and_relative_paths_follow_air_contract() {
    assert_eq!(
        normalize_air_path("app:/data/levels/1.json").unwrap(),
        "/app/data/levels/1.json"
    );
    assert_eq!(
        normalize_air_path("app-storage:/prefs/../save.bin").unwrap(),
        "/app-storage/save.bin"
    );
    assert_eq!(
        normalize_air_path("documents:/Notes/todo.txt").unwrap(),
        "/user/Documents/Notes/todo.txt"
    );
    assert_eq!(
        normalize_air_path("file:///user/Desktop/my%20file.png").unwrap(),
        "/user/Desktop/my file.png"
    );
    assert_eq!(
        resolve_air_path("/app-storage/profiles", "slot1/data.sav").unwrap(),
        "/app-storage/profiles/slot1/data.sav"
    );
    assert_eq!(
        relative_air_path("/user/Documents", "/user/Documents/work/report.txt", false).unwrap(),
        Some("work/report.txt".to_owned())
    );
    assert_eq!(
        relative_air_path("/user/Documents", "/user/Desktop/icon.png", false).unwrap(),
        None
    );
    assert_eq!(
        relative_air_path("/user/Documents", "/user/Desktop/icon.png", true).unwrap(),
        Some("../Desktop/icon.png".to_owned())
    );
    assert_eq!(air_url_for_native_path("/app"), "app:/");
    assert_eq!(
        air_url_for_native_path("/app/assets/hero.png"),
        "app:/assets/hero.png"
    );
    assert_eq!(
        air_url_for_native_path("/app-storage/db/main.db"),
        "app-storage:/db/main.db"
    );
}

#[test]
fn air_virtual_filesystem_protects_app_and_supports_tree_operations() {
    let res = vec![AirResourceInit {
        path: "config/settings.txt".into(),
        data_base64: BASE64.encode(b"hello-air"),
    }];
    let mut fs = AirFileSystem::new(&res, 1_700_000_000_000.0).unwrap();

    let app_file = fs.info("/app/config/settings.txt").unwrap();
    assert!(app_file.exists);
    assert!(!app_file.is_directory);
    assert_eq!(app_file.size, 9);
    assert_eq!(app_file.extension.as_deref(), Some("txt"));
    assert!(!app_file.writable);
    assert_eq!(
        fs.read_bytes("/app/config/settings.txt").unwrap(),
        b"hello-air"
    );

    // Writing or deleting under app:/ fails with SecurityError
    assert!(matches!(
        fs.write_bytes("/app/new.txt", b"x", 1.0),
        Err(AirError::Security(_))
    ));
    assert!(matches!(
        fs.delete_file("/app/config/settings.txt"),
        Err(AirError::Security(_))
    ));

    // Copy from app:/ into app-storage:/ works and becomes writable
    fs.copy_to("/app/config", "/app-storage/backup", false, 2000.0)
        .unwrap();
    assert_eq!(
        fs.read_bytes("/app-storage/backup/settings.txt").unwrap(),
        b"hello-air"
    );
    fs.move_to(
        "/app-storage/backup/settings.txt",
        "/app-storage/backup/renamed.txt",
        false,
        3000.0,
    )
    .unwrap();
    assert_eq!(
        fs.list_directory("/app-storage/backup").unwrap(),
        vec!["/app-storage/backup/renamed.txt".to_owned()]
    );
    // Non-empty deleteDirectory(false) fails, deleteDirectory(true) succeeds
    assert!(fs.delete_directory("/app-storage/backup", false).is_err());
    fs.delete_directory("/app-storage/backup", true).unwrap();
    assert!(!fs.info("/app-storage/backup").unwrap().exists);

    // Temp file & directory creation
    let tmp_f = fs.create_temp(false, 4000.0).unwrap();
    let tmp_d = fs.create_temp(true, 4000.0).unwrap();
    assert!(fs.info(&tmp_f).unwrap().exists);
    assert!(fs.info(&tmp_d).unwrap().is_directory);
}

#[test]
fn encrypted_local_store_roundtrips_and_resets() {
    let mut els = AirEncryptedStore::default();
    els.set_item("token", &[1, 2, 3, 255]).unwrap();
    assert_eq!(els.get_item("token").unwrap(), Some(vec![1, 2, 3, 255]));
    els.remove_item("token").unwrap();
    assert_eq!(els.get_item("token").unwrap(), None);
    els.set_item("k1", b"abc").unwrap();
    els.reset();
    assert_eq!(els.get_item("k1").unwrap(), None);
}

#[test]
fn sql_engine_executes_ddl_dml_joins_aggregates_transactions_savepoints_and_persistence() {
    let mut rt = AirRuntime::new("AIR Inventory", 800, 600, &[], 1000.0).unwrap();
    assert_eq!(rt.app_id, "org.neoflash.air-inventory");
    assert!(rt.descriptor_xml.contains("<width>800</width>"));

    let cid = rt
        .sql_open(Some("app-storage:/store.db"), "create", false, 1024, 1000.0)
        .unwrap();
    rt.sql_execute_json(
        cid,
        "CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, category TEXT NOT NULL, price REAL DEFAULT 0, name TEXT COLLATE NOCASE UNIQUE)",
        "{}",
        1001.0,
    )
    .unwrap();
    rt.sql_execute_json(
        cid,
        "CREATE INDEX idx_items_cat ON items (category)",
        "{}",
        1002.0,
    )
    .unwrap();

    let p1 = serde_json::json!({
        ":cat": {"k": "Text", "v": "tools"},
        ":price": {"k": "Real", "v": 12.5},
        ":name": {"k": "Text", "v": "Hammer"}
    })
    .to_string();
    let res1: serde_json::Value = serde_json::from_str(
        &rt.sql_execute_json(
            cid,
            "INSERT INTO items (category, price, name) VALUES (:cat, :price, :name)",
            &p1,
            1003.0,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(res1["rowsAffected"], 1);
    assert_eq!(res1["lastInsertRowID"], 1);

    rt.sql_execute_json(
        cid,
        "INSERT INTO items (category, price, name) VALUES ('tools', 7.5, 'Wrench'), ('books', 20.0, 'Manual')",
        "{}",
        1004.0,
    )
    .unwrap();

    // Savepoint + rollback to savepoint inside transaction
    rt.sql_begin(cid, Some("immediate")).unwrap();
    rt.sql_set_savepoint(cid, Some("sp1")).unwrap();
    rt.sql_execute_json(
        cid,
        "DELETE FROM items WHERE category = 'books'",
        "{}",
        1005.0,
    )
    .unwrap();
    rt.sql_rollback_to_savepoint(cid, Some("sp1")).unwrap();
    rt.sql_execute_json(
        cid,
        "UPDATE items SET price = price + 2.5 WHERE name = 'hammer'",
        "{}",
        1006.0,
    )
    .unwrap();
    rt.sql_commit(cid, 1007.0).unwrap();

    // Close and reopen from virtual FS in READ mode to verify persistence!
    rt.sql_close(cid).unwrap();
    let cid2 = rt
        .sql_open(Some("/app-storage/store.db"), "read", false, 1024, 2000.0)
        .unwrap();

    let sel: serde_json::Value = serde_json::from_str(
        &rt.sql_execute_json(
            cid2,
            "SELECT category, COUNT(*) AS cnt, SUM(price) AS total FROM items GROUP BY category ORDER BY total DESC",
            "{}",
            2001.0,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        sel["columns"],
        serde_json::json!(["category", "cnt", "total"])
    );
    assert_eq!(sel["rows"].as_array().unwrap().len(), 2);
    assert_eq!(sel["rows"][0][0]["v"], "tools");
    assert_eq!(sel["rows"][0][1]["v"], 2);
    assert_eq!(sel["rows"][0][2]["v"], 22.5);

    // Schema introspection returns table + columns + index
    let schema: serde_json::Value = serde_json::from_str(
        &rt.sql_load_schema_json(cid2, None, None, "main", true)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(schema["tables"][0]["name"], "items");
    assert_eq!(schema["tables"][0]["columns"].as_array().unwrap().len(), 4);
    assert_eq!(schema["indices"][0]["name"], "idx_items_cat");

    // Writing to a READ mode connection fails with errorId 3132
    let ro_err = rt
        .sql_execute_json(cid2, "DELETE FROM items", "{}", 2002.0)
        .unwrap_err();
    assert_eq!(ro_err.error_id, 3132);
}

#[test]
fn sql_encrypted_database_roundtrips_rejects_wrong_key_and_reencrypts() {
    let mut rt = AirRuntime::new("AIR Vault", 640, 480, &[], 1000.0).unwrap();
    let key1 = b"0123456789abcdef";
    let key2 = b"fedcba9876543210";
    let bad_short = b"short";

    assert_eq!(
        rt.sql_open_with_key(
            Some("app-storage:/vault.db"),
            "create",
            false,
            1024,
            Some(bad_short),
            1000.0
        )
        .unwrap_err()
        .error_id,
        3143
    );

    let cid = rt
        .sql_open_with_key(
            Some("app-storage:/vault.db"),
            "create",
            false,
            1024,
            Some(key1),
            1000.0,
        )
        .unwrap();
    rt.sql_execute_json(
        cid,
        "CREATE TABLE secrets (id INTEGER PRIMARY KEY, code TEXT)",
        "{}",
        1001.0,
    )
    .unwrap();
    rt.sql_execute_json(
        cid,
        "INSERT INTO secrets (id, code) VALUES (1, 'alpha-7')",
        "{}",
        1002.0,
    )
    .unwrap();
    rt.sql_close(cid).unwrap();

    // Raw file bytes on VFS are encrypted and do not contain plaintext
    let raw_vfs = rt.fs.read_bytes("/app-storage/vault.db").unwrap();
    assert!(raw_vfs.starts_with(SQL_ENC_VFS_MAGIC));
    assert!(!String::from_utf8_lossy(&raw_vfs).contains("alpha-7"));

    // Opening without key or with wrong key fails with errorId 3138
    assert_eq!(
        rt.sql_open(Some("app-storage:/vault.db"), "read", false, 1024, 1010.0)
            .unwrap_err()
            .error_id,
        3138
    );
    assert_eq!(
        rt.sql_open_with_key(
            Some("app-storage:/vault.db"),
            "read",
            false,
            1024,
            Some(key2),
            1010.0
        )
        .unwrap_err()
        .error_id,
        3138
    );

    // Reopen with key1 in update mode and reencrypt to key2
    let cid_re = rt
        .sql_open_with_key(
            Some("app-storage:/vault.db"),
            "update",
            false,
            1024,
            Some(key1),
            1020.0,
        )
        .unwrap();
    rt.sql_reencrypt(cid_re, key2, 1021.0).unwrap();
    rt.sql_close(cid_re).unwrap();

    // Old key1 now fails; new key2 succeeds and reads rows
    assert_eq!(
        rt.sql_open_with_key(
            Some("app-storage:/vault.db"),
            "read",
            false,
            1024,
            Some(key1),
            1030.0
        )
        .unwrap_err()
        .error_id,
        3138
    );
    let cid_ok = rt
        .sql_open_with_key(
            Some("app-storage:/vault.db"),
            "read",
            false,
            1024,
            Some(key2),
            1031.0,
        )
        .unwrap();
    let res: serde_json::Value = serde_json::from_str(
        &rt.sql_execute_json(cid_ok, "SELECT code FROM secrets", "{}", 1032.0)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(res["rows"][0][0]["v"], "alpha-7");
}

#[test]
fn html_network_xmlsig_and_text_rasterizer_engines_follow_air_contracts() {
    assert_eq!(
        sha1_bytes(b"abc"),
        [
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
            0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
        ]
    );
    assert_eq!(sha256_bytes(b"abc")[..4], [0xba, 0x78, 0x16, 0xbf]);

    let res = vec![AirResourceInit {
        path: "docs/index.html".into(),
        data_base64: BASE64.encode(
            b"<html><head><title>App Help</title></head><body><h1 id=\"hdr\">Welcome</h1><a href=\"app:/docs/p2.html\">Next</a></body></html>",
        ),
    }];
    let mut rt = AirRuntime::new("AIR Suite", 800, 600, &res, 1000.0).unwrap();

    // HTML session & history navigation
    let hid = rt.html.create_session(400.0, 300.0);
    let st1: serde_json::Value = serde_json::from_str(
        &rt.html_load_url(hid, "app:/docs/index.html", false, None)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(st1["title"], "App Help");
    assert_eq!(st1["hasFocusableContent"], true);
    assert_eq!(st1["historyLength"], 1);

    let st2: serde_json::Value = serde_json::from_str(
        &rt.html
            .load_string(hid, "<title>Second</title><p>Body 2</p>", "about:second")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(st2["title"], "Second");
    assert_eq!(st2["historyLength"], 2);
    assert_eq!(st2["historyPosition"], 1);

    let back: serde_json::Value =
        serde_json::from_str(&rt.html.history_go(hid, -1).unwrap()).unwrap();
    assert_eq!(back["title"], "App Help");
    assert_eq!(back["historyPosition"], 0);

    // UDP DatagramSocket + TCP ServerSocket loopback
    let u1 = rt.net.udp_create();
    let u2 = rt.net.udp_create();
    let b2: serde_json::Value =
        serde_json::from_str(&rt.net.udp_bind(u2, 0, "127.0.0.1").unwrap()).unwrap();
    let port2 = b2["localPort"].as_u64().unwrap() as u32;
    rt.net.udp_receive(u2).unwrap();
    let send_res: serde_json::Value =
        serde_json::from_str(&rt.net.udp_send(u1, Some("127.0.0.1"), port2).unwrap()).unwrap();
    assert_eq!(send_res["deliveries"].as_array().unwrap().len(), 1);
    assert_eq!(send_res["deliveries"][0]["targetSockId"], u2);

    let srv = rt.net.tcp_server_create();
    let sb: serde_json::Value =
        serde_json::from_str(&rt.net.tcp_server_bind(srv, 0, "127.0.0.1").unwrap()).unwrap();
    let srv_port = sb["localPort"].as_u64().unwrap() as u32;
    rt.net.tcp_server_listen(srv, 5).unwrap();
    let conn_info: serde_json::Value =
        serde_json::from_str(&rt.net.tcp_connect_loopback("127.0.0.1", srv_port)).unwrap();
    assert_eq!(conn_info["matched"], true);
    let client_cid = conn_info["clientConnId"].as_u64().unwrap() as u32;
    let accept_cid = conn_info["acceptedConnId"].as_u64().unwrap() as u32;
    assert_eq!(rt.net.tcp_write(client_cid, b"ping").unwrap(), accept_cid);
    assert_eq!(rt.net.tcp_read(accept_cid), b"ping");

    // XMLSignatureValidator real SHA-256 + HMAC-SHA256 verification & tamper detection
    let cert = b"CN=NeoFlash Release, O=NeoFlash";
    rt.xml_sig.add_certificate(cert, true).unwrap();
    let payload = b"<order id='42'>approved</order>";
    let digest_b64 = BASE64.encode(sha256_bytes(payload));
    let signed_info = format!(
        "<SignedInfo><CanonicalizationMethod Algorithm=\"http://www.w3.org/TR/2001/REC-xml-c14n-20010315\"/><SignatureMethod Algorithm=\"http://www.w3.org/2001/04/xmldsig-more#hmac-sha256\"/><Reference URI=\"urn:order:42\"><DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha256\"/><DigestValue>{digest_b64}</DigestValue></Reference></SignedInfo>"
    );
    let canon = canonicalize_xml_fragment(&signed_info);
    let sig_val = BASE64.encode(hmac_sha256(cert, canon.as_bytes()));
    let cert_b64 = BASE64.encode(cert);
    let xml_sig = format!(
        "<Signature>{signed_info}<SignatureValue>{sig_val}</SignatureValue><KeyInfo><X509Data><X509Certificate>{cert_b64}</X509Certificate></X509Data></KeyInfo></Signature>"
    );
    let deref_ok = serde_json::json!({
        "urn:order:42": BASE64.encode(payload)
    })
    .to_string();
    let verified: serde_json::Value = serde_json::from_str(&rt.xml_sig.verify_json(
        &xml_sig,
        &deref_ok,
        "validIdentity",
        "bestEffort",
        false,
    ))
    .unwrap();
    assert_eq!(verified["validityStatus"], "valid");
    assert_eq!(verified["digestStatus"], "valid");
    assert_eq!(verified["referencesStatus"], "valid");
    assert_eq!(verified["identityStatus"], "valid");
    assert_eq!(verified["signerCN"], "NeoFlash Release");

    let deref_tampered = serde_json::json!({
        "urn:order:42": BASE64.encode(b"<order id='42'>tampered</order>")
    })
    .to_string();
    let tampered: serde_json::Value = serde_json::from_str(&rt.xml_sig.verify_json(
        &xml_sig,
        &deref_tampered,
        "validIdentity",
        "bestEffort",
        false,
    ))
    .unwrap();
    assert_eq!(tampered["referencesStatus"], "invalid");
    assert_eq!(tampered["validityStatus"], "invalid");

    // Tampering with <SignedInfo> without re-signing must fail digestStatus!
    let tampered_si_xml = xml_sig.replace("urn:order:42", "urn:order:99");
    let tampered_si: serde_json::Value = serde_json::from_str(&rt.xml_sig.verify_json(
        &tampered_si_xml,
        &deref_ok,
        "validIdentity",
        "bestEffort",
        false,
    ))
    .unwrap();
    assert_eq!(tampered_si["digestStatus"], "invalid");
    assert_eq!(tampered_si["validityStatus"], "invalid");

    // W3C Canonical XML 1.0 sorts attributes and expands self-closing tags
    assert_eq!(
        canonicalize_xml_fragment("<Ref z='2' a=\"1\"/>"),
        "<Ref a=\"1\" z=\"2\"></Ref>"
    );

    // DeviceRotation quaternion calculation produces normalized unit quaternion
    let q = device_rotation_quaternion(90.0, 30.0, -15.0);
    assert_eq!(q.len(), 7);
    let norm_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    assert!((norm_sq - 1.0).abs() < 1e-9);
    assert_eq!(q[4], -15.0);
    assert_eq!(q[5], 30.0);
    assert_eq!(q[6], 90.0);

    // Text rasterizer produces foreground glyph pixels and selection highlight
    let rgba = rasterize_text_rgba(64, 24, "AIR", 0x112233, 0xFFFFFFFF, 16, "left:sel=0,2");
    assert_eq!(rgba.len(), 64 * 24 * 4);
    assert!(rgba.as_chunks::<4>().0.contains(&[0x11, 0x22, 0x33, 0xff]));
    assert!(rgba.as_chunks::<4>().0.contains(&[0x33, 0x90, 0xff, 0xff]));
}

#[test]
fn x509_der_oid_parsing_handles_long_lengths_issuer_vs_subject_and_nested_xml_blocks() {
    // 1. False-positive OID bytes without preceding 0x06, 0x03 tag/length are ignored,
    //    and long-form DER string length (0x81, 130) is decoded properly.
    let long_cn = "A".repeat(130);
    let mut der_long = vec![
        0x04, 0x05, 0x55, 0x04, 0x03, 0x0c, 0x03, b'B', b'A', b'D', // Not an OID tag (0x04)
        0x06, 0x03, 0x55, 0x04, 0x03, // OID 2.5.4.3 (CommonName)
        0x0c, 0x81, 130, // UTF8String with 1-byte long-form length = 130
    ];
    der_long.extend_from_slice(long_cn.as_bytes());
    assert_eq!(
        extract_der_oid_string(&der_long, &[0x55, 0x04, 0x03]).as_deref(),
        Some(long_cn.as_str())
    );

    // 2. X.509 certificate with distinct Issuer RDNSequence (first) and Subject RDNSequence (second)
    let mut cert_der = Vec::new();
    // Issuer CN = "NeoFlash Root CA", O = "NeoFlash Trust"
    cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 16]);
    cert_der.extend_from_slice(b"NeoFlash Root CA");
    cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 14]);
    cert_der.extend_from_slice(b"NeoFlash Trust");
    // Subject CN = "NeoFlash App Signer", O = "NeoFlash Labs"
    cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 19]);
    cert_der.extend_from_slice(b"NeoFlash App Signer");
    cert_der.extend_from_slice(&[0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 13]);
    cert_der.extend_from_slice(b"NeoFlash Labs");

    let meta = parse_cert_metadata(&cert_der);
    assert_eq!(meta.issuer_cn, "NeoFlash Root CA");
    assert_eq!(meta.issuer_dn, "CN=NeoFlash Root CA, O=NeoFlash Trust");
    assert_eq!(meta.cn, "NeoFlash App Signer");
    assert_eq!(meta.dn, "CN=NeoFlash App Signer, O=NeoFlash Labs");

    // 3. Nested same-named <Object> tags in extract_all_xml_tag_blocks
    let nested_xml = "<Signature><Object Id=\"outer\"><Object Id=\"inner\">payload</Object><Extra/></Object><Object Id=\"second\">two</Object></Signature>";
    let blocks = extract_all_xml_tag_blocks(nested_xml, "Object");
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].0.trim(), "Id=\"outer\"");
    assert_eq!(blocks[0].1, "<Object Id=\"inner\">payload</Object><Extra/>");
    assert_eq!(blocks[1].0.trim(), "Id=\"second\"");
    assert_eq!(blocks[1].1, "two");
}

#[test]
fn rfc8439_chacha20_vector_and_cross_runtime_unique_sql_encryption_nonces() {
    // RFC 8439 §2.4.2 test vector
    let mut rfc_key = [0u8; 32];
    for (i, b) in rfc_key.iter_mut().enumerate() {
        *b = i as u8;
    }
    let rfc_nonce: [u8; 12] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00,
    ];
    let ladies = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let ct = chacha20_xor(&rfc_key, &rfc_nonce, 1, ladies);
    assert_eq!(
        &ct[..16],
        &[
            0x6e, 0x2e, 0x35, 0x9a, 0x25, 0x68, 0xf9, 0x80, 0x41, 0xba, 0x07, 0x28, 0xdd, 0x0d,
            0x69, 0x81,
        ]
    );
    let rt_plain = chacha20_xor(&rfc_key, &rfc_nonce, 1, &ct);
    assert_eq!(rt_plain.as_slice(), ladies);

    // Two separate AirRuntime instances encrypting identical payloads with the same key
    // and same initial nonce_seed must produce distinct salts, nonces, and ciphertexts.
    let key: [u8; 16] = *b"0123456789abcdef";
    let payload = b"{\"tables\":{\"secrets\":true}}";
    let enc_a = encrypt_sql_payload(&key, 0, payload);
    let enc_b = encrypt_sql_payload(&key, 0, payload);
    assert_ne!(enc_a, enc_b);
    let body_a = enc_a.strip_prefix(SQL_ENC_VFS_MAGIC).unwrap();
    let body_b = enc_b.strip_prefix(SQL_ENC_VFS_MAGIC).unwrap();
    assert!(body_a.starts_with(SQL_ENC_V2_HEADER));
    assert!(body_b.starts_with(SQL_ENC_V2_HEADER));
    // Salt (16B) + Nonce (12B) right after "CV2\0" must differ
    assert_ne!(&body_a[4..32], &body_b[4..32]);
    assert_eq!(decrypt_sql_payload(&key, body_a, "open").unwrap(), payload);
    assert_eq!(decrypt_sql_payload(&key, body_b, "open").unwrap(), payload);
}

#[test]
fn legacy_sql_payload_with_v2_like_salt_still_decrypts() {
    let key: [u8; 16] = *b"0123456789abcdef";
    let plaintext = b"legacy collision";
    let mut salt = [0u8; 16];
    salt[..4].copy_from_slice(SQL_ENC_V2_HEADER);
    salt[4..].copy_from_slice(b"legacy-salt!");

    let mut ciphertext = Vec::with_capacity(plaintext.len());
    for (block_idx, chunk) in plaintext.chunks(32).enumerate() {
        let mut ctr_msg = [0u8; 28];
        ctr_msg[..4].copy_from_slice(b"enc\0");
        ctr_msg[4..20].copy_from_slice(&salt);
        ctr_msg[20..28].copy_from_slice(&(block_idx as u64).to_be_bytes());
        let stream = hmac_sha256(&key, &ctr_msg);
        ciphertext.extend(chunk.iter().enumerate().map(|(i, byte)| byte ^ stream[i]));
    }
    let mut mac_msg = Vec::new();
    mac_msg.extend_from_slice(b"mac\0");
    mac_msg.extend_from_slice(&salt);
    mac_msg.extend_from_slice(&ciphertext);
    let tag = hmac_sha256(&key, &mac_msg);
    let mut body = salt.to_vec();
    body.extend_from_slice(&ciphertext);
    body.extend_from_slice(&tag);

    assert!(body.starts_with(SQL_ENC_V2_HEADER));
    assert_eq!(decrypt_sql_payload(&key, &body, "open").unwrap(), plaintext);
}

#[test]
fn t267_os_csprng_salt_nonce_uniqueness_distribution_and_custom_provider_injection() {
    let key: [u8; 16] = *b"0123456789abcdef";

    // 1. Rapid calls to OS CSPRNG must produce 64 distinct (salt, nonce) pairs with healthy bit distribution.
    set_csprng_provider(None);
    let mut seen = std::collections::HashSet::new();
    let mut ones_count = 0u32;
    let mut total_bits = 0u32;
    for _ in 0..64 {
        let (salt, nonce) = generate_sql_salt_and_nonce(&key, 0, 128);
        assert!(seen.insert((salt, nonce)));
        for b in salt.iter().chain(nonce.iter()) {
            ones_count += b.count_ones();
            total_bits += 8;
        }
    }
    // Across 64 * 28 * 8 = 14,336 bits of CSPRNG output, 1-bits should be well within [35%, 65%].
    assert!(
        ones_count > total_bits * 35 / 100 && ones_count < total_bits * 65 / 100,
        "unexpected bit balance: {ones_count} / {total_bits}"
    );

    // 2. Custom CSPRNG provider injection via `set_csprng_provider`.
    static INJECT_SEQ: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);
    fn test_provider(dest: &mut [u8]) -> bool {
        let seq = INJECT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (i, b) in dest.iter_mut().enumerate() {
            *b = 0xA0u8.wrapping_add(seq).wrapping_add(i as u8);
        }
        true
    }

    struct ResetProviderGuard;
    impl Drop for ResetProviderGuard {
        fn drop(&mut self) {
            set_csprng_provider(None);
        }
    }

    let _guard = ResetProviderGuard;
    INJECT_SEQ.store(10, std::sync::atomic::Ordering::Relaxed);
    set_csprng_provider(Some(test_provider));
    let (salt_inj, nonce_inj) = generate_sql_salt_and_nonce(&key, 42, 64);
    // Other parallel AIR tests may legitimately request entropy while the process-wide host
    // provider is installed. Assert this call's provider pattern instead of assuming that it
    // must consume the first sequence number after `store(10)`.
    let injected_start = salt_inj[0];
    for (index, byte) in salt_inj.iter().chain(nonce_inj.iter()).enumerate() {
        assert_eq!(*byte, injected_start.wrapping_add(index as u8));
    }

    // 3. Fallback path when custom provider returns `false` still produces distinct ChaCha20-backed nonces.
    fn failing_provider(_dest: &mut [u8]) -> bool {
        false
    }
    set_csprng_provider(Some(failing_provider));
    let (s1, n1) = generate_sql_salt_and_nonce(&key, 0, 64);
    let (s2, n2) = generate_sql_salt_and_nonce(&key, 0, 64);
    assert_ne!((s1, n1), (s2, n2));
}
