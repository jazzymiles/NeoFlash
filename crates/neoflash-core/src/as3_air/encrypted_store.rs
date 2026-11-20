use super::*;

// ---------------------------------------------------------------------------
// Application Descriptor & EncryptedLocalStore
// ---------------------------------------------------------------------------

pub(super) fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn synthesize_app_id(project_name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dot = false;
    for ch in project_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dot = false;
        } else if !slug.is_empty() && !prev_dot {
            slug.push('-');
            prev_dot = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "org.neoflash.application".to_owned()
    } else {
        format!("org.neoflash.{slug}")
    }
}

pub fn synthesize_app_descriptor_xml(project_name: &str, width: u32, height: u32) -> String {
    let app_id = synthesize_app_id(project_name);
    let safe_name = if project_name.trim().is_empty() {
        "NeoFlash Application"
    } else {
        project_name.trim()
    };
    let escaped = xml_escape(safe_name);
    format!(
        "<application xmlns=\"http://ns.adobe.com/air/application/32.0\"><id>{}</id><filename>{escaped}</filename><name>{escaped}</name><versionNumber>1.0.0</versionNumber><initialWindow><title>{escaped}</title><visible>true</visible><width>{width}</width><height>{height}</height></initialWindow></application>",
        xml_escape(&app_id)
    )
}

#[derive(Debug, Clone, Default)]
pub struct AirEncryptedStore {
    items: BTreeMap<String, Vec<u8>>,
    total_bytes: usize,
}

impl AirEncryptedStore {
    pub fn set_item(&mut self, name: &str, bytes: &[u8]) -> Result<(), AirError> {
        if name.is_empty() {
            return Err(AirError::Argument(
                "EncryptedLocalStore item name must not be empty.".into(),
            ));
        }
        if name.len() > MAX_AIR_PATH_BYTES {
            return Err(AirError::Range(
                "EncryptedLocalStore item name exceeds 4096 bytes.".into(),
            ));
        }
        if bytes.len() > MAX_AIR_FILE_BYTES {
            return Err(AirError::Range(
                "EncryptedLocalStore item exceeds 16 MiB.".into(),
            ));
        }
        let old_len = self.items.get(name).map_or(0, Vec::len);
        if !self.items.contains_key(name) && self.items.len() >= MAX_AIR_ELS_ITEMS {
            return Err(AirError::Range(
                "EncryptedLocalStore item count limit (1024) exceeded.".into(),
            ));
        }
        let next_total = self
            .total_bytes
            .saturating_sub(old_len)
            .saturating_add(bytes.len());
        if next_total > MAX_AIR_TOTAL_BYTES {
            return Err(AirError::Range(
                "EncryptedLocalStore total storage limit (64 MiB) exceeded.".into(),
            ));
        }
        self.total_bytes = next_total;
        self.items.insert(name.to_owned(), bytes.to_vec());
        Ok(())
    }

    pub fn get_item(&self, name: &str) -> Result<Option<Vec<u8>>, AirError> {
        if name.is_empty() {
            return Err(AirError::Argument(
                "EncryptedLocalStore item name must not be empty.".into(),
            ));
        }
        Ok(self.items.get(name).cloned())
    }

    pub fn remove_item(&mut self, name: &str) -> Result<(), AirError> {
        if name.is_empty() {
            return Err(AirError::Argument(
                "EncryptedLocalStore item name must not be empty.".into(),
            ));
        }
        if let Some(old) = self.items.remove(name) {
            self.total_bytes = self.total_bytes.saturating_sub(old.len());
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.items.clear();
        self.total_bytes = 0;
    }
}

// ---------------------------------------------------------------------------
// Bounded Relational SQL Engine (`flash.data.*`)
// ---------------------------------------------------------------------------

pub(super) const SQL_VFS_MAGIC: &[u8] = b"NFAIRSDB1\n";
pub(super) const SQL_ENC_VFS_MAGIC: &[u8] = b"NFAIRSLE1\n";

pub(super) const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, &word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn sha1_bytes(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xefcdab89;
    let mut h2: u32 = 0x98badcfe;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xc3d2e1f0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, &word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdcu32),
                _ => (b ^ c ^ d, 0xca62c1d6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = sha256_bytes(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + data.len());
    for &b in &k_block {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(data);
    let inner_hash = sha256_bytes(&inner);

    let mut outer = Vec::with_capacity(64 + 32);
    for &b in &k_block {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    sha256_bytes(&outer)
}

static SQL_CRYPTO_GLOBAL_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

#[inline]
pub(super) fn chacha20_quarter_round(
    state: &mut [u32; 16],
    a: usize,
    b: usize,
    c: usize,
    d: usize,
) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(16);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(12);

    state[a] = state[a].wrapping_add(state[b]);
    state[d] ^= state[a];
    state[d] = state[d].rotate_left(8);

    state[c] = state[c].wrapping_add(state[d]);
    state[b] ^= state[c];
    state[b] = state[b].rotate_left(7);
}

/// Standard RFC 8439 §2.3 ChaCha20 block function (20 rounds).
pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0] = 0x6170_7865; // "expa"
    state[1] = 0x3320_646e; // "nd 3"
    state[2] = 0x7962_2d32; // "2-by"
    state[3] = 0x6b20_6574; // "te k"
    for i in 0..8 {
        state[4 + i] =
            u32::from_le_bytes([key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]]);
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4],
            nonce[i * 4 + 1],
            nonce[i * 4 + 2],
            nonce[i * 4 + 3],
        ]);
    }

    let mut working = state;
    for _ in 0..10 {
        // Column rounds
        chacha20_quarter_round(&mut working, 0, 4, 8, 12);
        chacha20_quarter_round(&mut working, 1, 5, 9, 13);
        chacha20_quarter_round(&mut working, 2, 6, 10, 14);
        chacha20_quarter_round(&mut working, 3, 7, 11, 15);
        // Diagonal rounds
        chacha20_quarter_round(&mut working, 0, 5, 10, 15);
        chacha20_quarter_round(&mut working, 1, 6, 11, 12);
        chacha20_quarter_round(&mut working, 2, 7, 8, 13);
        chacha20_quarter_round(&mut working, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        let word = working[i].wrapping_add(state[i]);
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// Standard RFC 8439 §2.4 ChaCha20 stream cipher encryption/decryption.
pub fn chacha20_xor(
    key: &[u8; 32],
    nonce: &[u8; 12],
    initial_counter: u32,
    input: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for (block_idx, chunk) in input.chunks(64).enumerate() {
        let counter = initial_counter.wrapping_add(block_idx as u32);
        let ks = chacha20_block(key, counter, nonce);
        for (i, &b) in chunk.iter().enumerate() {
            out.push(b ^ ks[i]);
        }
    }
    out
}

/// RFC 5869 HKDF-SHA256 key derivation for encryption and authentication subkeys.
pub(super) fn derive_sql_subkeys(master_key: &[u8; 16], salt: &[u8; 16]) -> ([u8; 32], [u8; 32]) {
    let prk = hmac_sha256(salt, master_key);
    let enc_key = hmac_sha256(&prk, b"neoflash-air-sql-chacha20-v2-enc\x01");
    let mac_key = hmac_sha256(&prk, b"neoflash-air-sql-chacha20-v2-mac\x01");
    (enc_key, mac_key)
}

/// Host-supplied CSPRNG byte filler callback (`T-267`).
pub type CsprngFillFn = fn(&mut [u8]) -> bool;

static CUSTOM_CSPRNG_PROVIDER: std::sync::OnceLock<std::sync::RwLock<Option<CsprngFillFn>>> =
    std::sync::OnceLock::new();

/// Register or clear a custom CSPRNG provider (used by embedded/WASM hosts and tests).
pub fn set_csprng_provider(provider: Option<CsprngFillFn>) {
    let lock = CUSTOM_CSPRNG_PROVIDER.get_or_init(|| std::sync::RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = provider;
    }
}

/// Fill `dest` with genuine OS cryptographic entropy (`T-267`) without `std::fs` I/O:
/// - Custom host provider registered via [`set_csprng_provider`] if present.
/// - Darwin / macOS / iOS / BSD: `arc4random_buf` in system `libSystem.B.dylib` / `libc`
///   (backed directly by the XNU kernel CSPRNG `ccrng`).
/// - Linux / Android: `getrandom(2)` in `libc`.
/// - Windows: `BCryptGenRandom` with `BCRYPT_USE_SYSTEM_PREFERRED_RNG`.
pub fn fill_os_csprng_bytes(dest: &mut [u8]) -> bool {
    if dest.is_empty() {
        return true;
    }
    if let Some(lock) = CUSTOM_CSPRNG_PROVIDER.get()
        && let Ok(guard) = lock.read()
        && let Some(provider) = *guard
    {
        return provider(dest);
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        unsafe extern "C" {
            fn arc4random_buf(buf: *mut core::ffi::c_void, nbytes: usize);
        }
        // Safety: `dest` is a valid mutable slice of `dest.len()` bytes; Darwin/BSD
        // `arc4random_buf` is thread-safe, never fails, and fills `nbytes` from the kernel CSPRNG.
        unsafe {
            arc4random_buf(dest.as_mut_ptr().cast::<core::ffi::c_void>(), dest.len());
        }
        true
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        unsafe extern "C" {
            fn getrandom(buf: *mut core::ffi::c_void, buflen: usize, flags: u32) -> isize;
        }
        let mut filled = 0usize;
        while filled < dest.len() {
            let rem = &mut dest[filled..];
            // Safety: `rem` is a valid mutable byte slice of length `rem.len()`.
            let n =
                unsafe { getrandom(rem.as_mut_ptr().cast::<core::ffi::c_void>(), rem.len(), 0) };
            if n > 0 {
                filled += n as usize;
            } else {
                break;
            }
        }
        filled == dest.len()
    }

    #[cfg(target_os = "windows")]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                h_algorithm: *mut core::ffi::c_void,
                pb_buffer: *mut u8,
                cb_buffer: u32,
                dw_flags: u32,
            ) -> i32;
        }
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
        if let Ok(len_u32) = u32::try_from(dest.len()) {
            // Safety: `dest` is a valid mutable buffer of `len_u32` bytes.
            let status = unsafe {
                BCryptGenRandom(
                    core::ptr::null_mut(),
                    dest.as_mut_ptr(),
                    len_u32,
                    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            };
            status == 0
        } else {
            false
        }
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "linux",
        target_os = "android",
        target_os = "windows"
    )))]
    {
        false
    }
}

/// Generate fresh 16-byte salt and 12-byte RFC 8439 nonce material from the
/// OS kernel CSPRNG (`fill_os_csprng_bytes`, backed by `arc4random_buf` on macOS),
/// falling back on non-OS targets without an injected provider to an RFC 8439
/// ChaCha20 20-round stream block keyed via HKDF/HMAC-SHA256 from OS-seeded
/// `RandomState` instances and monotonic counters (`T-267`).
pub(super) fn generate_sql_salt_and_nonce(
    key: &[u8; 16],
    instance_seed: u64,
    payload_len: usize,
) -> ([u8; 16], [u8; 12]) {
    let mut csprng_bytes = [0u8; 32];
    if fill_os_csprng_bytes(&mut csprng_bytes) && csprng_bytes.iter().any(|&b| b != 0) {
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 12];
        salt.copy_from_slice(&csprng_bytes[..16]);
        nonce.copy_from_slice(&csprng_bytes[16..28]);
        return (salt, nonce);
    }

    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let global_seq = SQL_CRYPTO_GLOBAL_COUNTER
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .wrapping_add(1);
    let stack_anchor = global_seq ^ instance_seed;
    let stack_addr = &stack_anchor as *const u64 as usize as u64;

    let mut seed_material = Vec::with_capacity(64);
    seed_material.extend_from_slice(&global_seq.to_le_bytes());
    seed_material.extend_from_slice(&instance_seed.to_le_bytes());
    seed_material.extend_from_slice(&stack_addr.to_le_bytes());
    seed_material.extend_from_slice(&(payload_len as u64).to_le_bytes());
    for round in 0..4u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(global_seq.wrapping_add(round));
        hasher.write_u64(instance_seed.rotate_left((round as u32) * 13));
        hasher.write_u64(stack_addr);
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(dur) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            hasher.write_u128(dur.as_nanos());
        }
        seed_material.extend_from_slice(&hasher.finish().to_le_bytes());
    }

    let chacha_key = hmac_sha256(key, &seed_material);
    let nonce_digest = hmac_sha256(&chacha_key, b"neoflash-air-sql-fallback-nonce\x01");
    let mut chacha_nonce = [0u8; 12];
    chacha_nonce.copy_from_slice(&nonce_digest[..12]);
    let stream_block = chacha20_block(&chacha_key, global_seq as u32, &chacha_nonce);

    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    salt.copy_from_slice(&stream_block[..16]);
    nonce.copy_from_slice(&stream_block[16..28]);
    (salt, nonce)
}

pub(super) const SQL_ENC_V2_HEADER: &[u8; 4] = b"CV2\0";

pub(super) fn encrypt_sql_payload(key: &[u8; 16], nonce_seed: u64, plaintext: &[u8]) -> Vec<u8> {
    let (salt, nonce) = generate_sql_salt_and_nonce(key, nonce_seed, plaintext.len());
    let (enc_key, mac_key) = derive_sql_subkeys(key, &salt);
    let ciphertext = chacha20_xor(&enc_key, &nonce, 1, plaintext);

    let mut mac_msg = Vec::with_capacity(4 + 16 + 12 + ciphertext.len());
    mac_msg.extend_from_slice(SQL_ENC_V2_HEADER);
    mac_msg.extend_from_slice(&salt);
    mac_msg.extend_from_slice(&nonce);
    mac_msg.extend_from_slice(&ciphertext);
    let tag = hmac_sha256(&mac_key, &mac_msg);

    let mut out = Vec::with_capacity(SQL_ENC_VFS_MAGIC.len() + 4 + 16 + 12 + ciphertext.len() + 32);
    out.extend_from_slice(SQL_ENC_VFS_MAGIC);
    out.extend_from_slice(SQL_ENC_V2_HEADER);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out.extend_from_slice(&tag);
    out
}

pub(super) fn decrypt_sql_payload(
    key: &[u8; 16],
    encrypted_body: &[u8],
    operation: &str,
) -> Result<Vec<u8>, AirSqlError> {
    if let Some(v2_body) = encrypted_body.strip_prefix(SQL_ENC_V2_HEADER)
        && v2_body.len() >= 16 + 12 + 32
    {
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&v2_body[..16]);
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&v2_body[16..28]);
        let tag_start = v2_body.len() - 32;
        let ciphertext = &v2_body[28..tag_start];
        let tag = &v2_body[tag_start..];

        let (enc_key, mac_key) = derive_sql_subkeys(key, &salt);
        let mut mac_msg = Vec::with_capacity(4 + 16 + 12 + ciphertext.len());
        mac_msg.extend_from_slice(SQL_ENC_V2_HEADER);
        mac_msg.extend_from_slice(&salt);
        mac_msg.extend_from_slice(&nonce);
        mac_msg.extend_from_slice(ciphertext);
        let expected_tag = hmac_sha256(&mac_key, &mac_msg);
        let mut diff = 0u8;
        for (&a, &b) in tag.iter().zip(expected_tag.iter()) {
            diff |= a ^ b;
        }
        if diff == 0 {
            return Ok(chacha20_xor(&enc_key, &nonce, 1, ciphertext));
        }
        // A legacy payload has no explicit version byte: its 16-byte salt can begin with
        // `CV2\0` by chance. Only accept the v2 interpretation after its MAC verifies, then
        // fall through to the authenticated legacy decoder for compatibility.
    }

    if encrypted_body.len() < 48 {
        return Err(AirSqlError::new(
            operation,
            "Encrypted database header is truncated or corrupt.",
            3138,
        ));
    }
    let salt = &encrypted_body[..16];
    let tag_start = encrypted_body.len() - 32;
    let ciphertext = &encrypted_body[16..tag_start];
    let tag = &encrypted_body[tag_start..];

    let mut mac_msg = Vec::with_capacity(4 + 16 + ciphertext.len());
    mac_msg.extend_from_slice(b"mac\0");
    mac_msg.extend_from_slice(salt);
    mac_msg.extend_from_slice(ciphertext);
    let expected_tag = hmac_sha256(key, &mac_msg);
    let mut diff = 0u8;
    for (&a, &b) in tag.iter().zip(expected_tag.iter()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return Err(AirSqlError::new(
            operation,
            "File is encrypted or is not a database.",
            3138,
        ));
    }

    let mut plaintext = Vec::with_capacity(ciphertext.len());
    for (block_idx, chunk) in ciphertext.chunks(32).enumerate() {
        let mut ctr_msg = [0u8; 28];
        ctr_msg[..4].copy_from_slice(b"enc\0");
        ctr_msg[4..20].copy_from_slice(salt);
        ctr_msg[20..28].copy_from_slice(&(block_idx as u64).to_be_bytes());
        let ks = hmac_sha256(key, &ctr_msg);
        for (i, &b) in chunk.iter().enumerate() {
            plaintext.push(b ^ ks[i]);
        }
    }
    Ok(plaintext)
}

pub(super) fn parse_sql_encryption_key(
    key_opt: Option<&[u8]>,
    operation: &str,
) -> Result<Option<[u8; 16]>, AirSqlError> {
    let Some(raw) = key_opt else {
        return Ok(None);
    };
    if raw.len() != 16 {
        return Err(AirSqlError::new(
            operation,
            "SQL encryptionKey must be a 16-byte ByteArray.",
            3143,
        ));
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(raw);
    Ok(Some(arr))
}
