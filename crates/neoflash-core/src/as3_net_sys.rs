//! Shared Rust core for ActionScript 3 Area 8: Globalization, Networking,
//! Streaming/FLV/NetGroup, LocalConnection, XMLSocket framing, X.509 DER/PEM
//! Certificate parsing, DRM voucher lifecycle, System/Capabilities, Workers,
//! Concurrency (Mutex/Condition), Crypto CSPRNG, Sampler and Telemetry.
//!
//! Independently implemented from Adobe Flash Platform ActionScript 3.0
//! Reference specifications, RFC 5280 (X.509), RFC 6234 (SHA-256) and BCP-47.

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// Pure-Rust RFC 6234 SHA-256 + CSPRNG whitening
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
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
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
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
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn hex_encode_upper(bytes: &[u8]) -> String {
    hex_encode(bytes).to_ascii_uppercase()
}

pub fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    let clean = text.trim();
    if !clean.len().is_multiple_of(2) {
        return Err("Hex string must contain an even number of digits".into());
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    let bytes = clean.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = (bytes[i] as char)
            .to_digit(16)
            .ok_or_else(|| "Invalid hex character".to_owned())?;
        let lo = (bytes[i + 1] as char)
            .to_digit(16)
            .ok_or_else(|| "Invalid hex character".to_owned())?;
        out.push(((hi << 4) | lo) as u8);
    }
    Ok(out)
}

/// Generates `1..=1024` cryptographically whitened random bytes per
/// `flash.crypto.generateRandomBytes(numberRandomBytes:uint)`.
pub fn generate_random_bytes(
    count: u32,
    entropy: &[u8],
    counter: &mut u64,
) -> Result<Vec<u8>, String> {
    if !(1..=1024).contains(&count) {
        return Err(
            "Error #2004: One of the parameters is invalid. generateRandomBytes requires 1..1024 bytes."
                .into(),
        );
    }
    let len = count as usize;
    let mut out = Vec::with_capacity(len);
    let mut block_idx = 0u32;
    while out.len() < len {
        *counter = counter.wrapping_add(1);
        let mut seed_material = Vec::with_capacity(entropy.len() + 20);
        seed_material.extend_from_slice(b"NeoFlash-CSPRNG-v1:");
        seed_material.extend_from_slice(&counter.to_le_bytes());
        seed_material.extend_from_slice(&block_idx.to_le_bytes());
        if entropy.len() >= (block_idx as usize) * 32 {
            let start = ((block_idx as usize) * 32).min(entropy.len());
            let end = (start + 32).min(entropy.len());
            seed_material.extend_from_slice(&entropy[start..end]);
            seed_material.extend_from_slice(entropy);
        } else {
            seed_material.extend_from_slice(entropy);
        }
        let digest = sha256(&seed_material);
        let take = (len - out.len()).min(32);
        out.extend_from_slice(&digest[..take]);
        block_idx += 1;
    }
    // Mix raw host entropy directly via XOR when exact-length entropy is supplied.
    if entropy.len() == len {
        for (dst, &src) in out.iter_mut().zip(entropy.iter()) {
            *dst ^= src;
        }
    }
    Ok(out)
}

mod capabilities_sys;
mod file_print_sampler;
mod sockets_conn;
#[cfg(test)]
mod tests;

pub use capabilities_sys::*;
pub use file_print_sampler::*;
pub use sockets_conn::*;
