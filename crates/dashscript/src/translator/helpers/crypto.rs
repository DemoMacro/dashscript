/// WinterTC WebCrypto helper — `__ds::crypto_random_uuid`. `crypto.randomUUID()`
/// (an RFC 4122 version-4 UUID) lowers here; `uuid::Uuid::new_v4` is the
/// reference implementation (`v4` feature, backed by `getrandom`). Pure-Rust —
/// WinterTC never degrades a Web API to the engine.
pub const CRYPTO_HELPER: &str = r#"
/// `crypto.randomUUID()` — an RFC 4122 version-4 UUID string (36 chars,
/// `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`). Each call returns a fresh UUID.
pub fn crypto_random_uuid() -> String {
    ::uuid::Uuid::new_v4().to_string()
}
/// `crypto.getRandomValues(buf)` — fill `buf` with cryptographically-strong
/// random bytes (WebCrypto `getRandomValues`, backed by `getrandom` — the same
/// source `uuid::new_v4` uses). Consumes the buffer and returns it filled (ES
/// returns the same typed array it was passed), matching the common
/// `var iv = crypto.getRandomValues(new Uint8Array(12))` shape. An in-place
/// call on an existing local (`crypto.getRandomValues(buf)`) moves the local,
/// so a later read of `buf` is a cargo-check error honestly — assign the
/// result back (`buf = crypto.getRandomValues(buf)`). ES caps the buffer at
/// 65536 bytes (a `QuotaExceededError`); that bound is unchecked here.
pub fn crypto_get_random_values(mut buf: ::std::vec::Vec<u8>) -> ::std::vec::Vec<u8> {
    ::getrandom::getrandom(&mut buf).expect("getrandom failed");
    buf
}
"#;

/// WebCrypto `SubtleCrypto.digest` helper — `__ds::crypto_subtle_digest`
/// (WinterTC Web API, W3C WebCrypto). `crypto.subtle.digest(algo, data)` is the
/// one-shot hash: `algo` is the ES algorithm name (`"SHA-1"`/`"SHA-256"`/
/// `"SHA-384"`/`"SHA-512"`), `data` is the `BufferSource` (a `Vec<u8>`), and the
/// result is the digest bytes. Backed by the RustCrypto `sha1`/`sha2` crates
/// (pure Rust — WinterTC never degrades a Web API). `async` because ES
/// `digest` returns a `Promise<ArrayBuffer>`; the `await` at the call site
/// drives the future (the async-main gate flips `fn main` to `#[tokio::main]`).
/// An unknown algorithm panics the `TypeError` ES throws (the WPT verdict reads
/// the prefix). The key-bearing methods are mapped alongside: `importKey`
/// (raw format → `DsCryptoKey`) / `exportKey` (raw format ← `DsCryptoKey`),
/// `sign`/`verify` (HMAC, RustCrypto `hmac`), `encrypt`/`decrypt` (AES-GCM
/// `aes-gcm` / AES-CBC `aes`+`cbc`), `generateKey` (a fresh random `DsCryptoKey`
/// for AES-GCM/AES-CBC/HMAC), `deriveBits`/`deriveKey` (the PBKDF2/HKDF
/// key-derivation paths, reusing `hmac`). The remaining `SubtleCrypto` methods
/// (`wrapKey`) land later; `digest` is the no-key one-shot, the bulk of the WPT
/// `WebCryptoAPI/digest` fixtures.
pub const SUBTLE_HELPER: &str = r#"
/// `crypto.subtle.digest(algo, data)` — the one-shot hash. `algo` is matched
/// case-sensitively against the ES algorithm names (`"SHA-1"`/`"SHA-256"`/
/// `"SHA-384"`/`"SHA-512"`); any other value panics the `TypeError` ES throws
/// (the WPT verdict reads the prefix). `data` is hashed as raw bytes. Returns
/// the digest bytes (20/32/48/64 for SHA-1/256/384/512).
pub async fn crypto_subtle_digest(
    algo: ::std::string::String,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    match algo.as_str() {
        "SHA-1" => {
            use ::sha1::{Digest, Sha1};
            Sha1::digest(&data).to_vec()
        }
        "SHA-256" => {
            use ::sha2::{Digest, Sha256};
            Sha256::digest(&data).to_vec()
        }
        "SHA-384" => {
            use ::sha2::{Digest, Sha384};
            Sha384::digest(&data).to_vec()
        }
        "SHA-512" => {
            use ::sha2::{Digest, Sha512};
            Sha512::digest(&data).to_vec()
        }
        _ => ::core::panic!(
            "TypeError: crypto.subtle.digest: unknown or unsupported algorithm"
        ),
    }
}
/// A WebCrypto `CryptoKey` — the value `crypto.subtle.importKey(…)` returns and
/// `sign`/`verify` take (the HMAC subset of WinterTC WebCrypto). It carries the
/// `algorithm` name (`"HMAC"`), the paired `hash` (`"SHA-256"`/…), the raw `key`
/// bytes, and its `extractable`/`usages` (stored, not enforced by the static
/// path — ES enforces them at runtime; the common server shape never trips
/// them). `#[derive(Clone)]` so a key passed to `sign`/`verify` by reference
/// may copy. The marker `__ds::DsCryptoKey` pulls `SubtleCrypto` (so `sha1`/
/// `sha2`/`hmac` are flagged) via the dep derivation.
#[derive(Clone)]
pub struct DsCryptoKey {
    pub algorithm: ::std::string::String,
    pub hash: ::std::string::String,
    pub key: ::std::vec::Vec<u8>,
    pub extractable: bool,
    pub usages: ::std::vec::Vec<::std::string::String>,
}
impl DsCryptoKey {
    /// The translator-emitted constructor (the importKey lowering builds the
    /// `(algorithm, hash, key, extractable, usages)` quadruple from the ES
    /// `algorithm` object + the raw key bytes).
    pub fn new(
        algorithm: ::std::string::String,
        hash: ::std::string::String,
        key: ::std::vec::Vec<u8>,
        extractable: bool,
        usages: ::std::vec::Vec<::std::string::String>,
    ) -> Self {
        Self {
            algorithm,
            hash,
            key,
            extractable,
            usages,
        }
    }
}
/// `crypto.subtle.importKey(format, keyData, algorithm, extractable, usages)`
/// — the HMAC subset. `format` is `"raw"` (the only form lowered — pkcs8/spki
/// are not statically modeled), `keyData` the raw key bytes, `algorithm` the
/// `{name, hash}` the translator extracted. Returns a `DsCryptoKey`. `async`
/// because ES `importKey` returns `Promise<CryptoKey>`; the call site's `await`
/// drives the future.
pub async fn crypto_subtle_import_key(
    algorithm: ::std::string::String,
    hash: ::std::string::String,
    key: ::std::vec::Vec<u8>,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    DsCryptoKey::new(algorithm, hash, key, extractable, usages)
}
/// `crypto.subtle.exportKey(format, key)` — the symmetric-key raw export (the
/// inverse of `importKey`). For `format` `"raw"` the raw key bytes (`key.key`)
/// are returned; `"jwk"`/`"pkcs8"`/`"spki"` are not statically modeled (panic
/// honestly — the WinterTC server shape uses `"raw"` for HMAC/AES keys). `async`
/// because ES `exportKey` returns `Promise<ArrayBuffer>`; the call site's `await`
/// drives the future, and `callee_return_path` records the `Vec<u8>` return (like
/// `sign`/`encrypt`).
pub async fn crypto_subtle_export_key(
    format: ::std::string::String,
    key: &DsCryptoKey,
) -> ::std::vec::Vec<u8> {
    match format.as_str() {
        "raw" => key.key.clone(),
        _ => ::core::panic!("TypeError: crypto.subtle.exportKey: unsupported format"),
    }
}
/// `crypto.subtle.generateKey(algorithm, extractable, usages)` — the factory for
/// a fresh `DsCryptoKey` (the WinterTC WebCrypto subset). For AES-GCM/AES-CBC the
/// algorithm object's `length` field (128/192/256) is the key length in bits
/// (`length / 8` bytes); for HMAC, `length` (if present) is the key length in
/// bits, else the default is the named hash's block size (64 for SHA-1/SHA-256,
/// 128 for SHA-384/SHA-512). The key bytes are cryptographically random
/// (`getrandom`, the same source `crypto.getRandomValues` uses). `async` because
/// ES `generateKey` returns `Promise<CryptoKey>`; the call site's `await` drives
/// the future, and `callee_return_path` records the `DsCryptoKey` return
/// (mirroring `importKey`, so a later `sign`/`encrypt` passes the key through).
pub async fn crypto_subtle_generate_key(
    name: ::std::string::String,
    hash: ::std::string::String,
    length: usize,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    let bytes = match name.as_str() {
        "AES-GCM" | "AES-CBC" => length / 8,
        "HMAC" => {
            if length == 0 {
                match hash.as_str() {
                    "SHA-384" | "SHA-512" => 128,
                    _ => 64,
                }
            } else {
                length / 8
            }
        }
        _ => ::core::panic!("TypeError: crypto.subtle.generateKey: unsupported algorithm"),
    };
    let mut key = ::std::vec![0u8; bytes];
    ::getrandom::getrandom(&mut key).expect("getrandom failed");
    DsCryptoKey::new(name, hash, key, extractable, usages)
}
/// `crypto.subtle.deriveBits(algorithm, baseKey, length)` — the WinterTC WebCrypto
/// key-derivation path. For PBKDF2 the `baseKey` carries the password bytes
/// (`key.key`), and the ES `algorithm` object's `salt`/`iterations`/`hash` drive
/// the derivation (RFC 2898); `info` is unused. For HKDF the `baseKey` is the
/// input key material, `salt`/`info`/`hash` drive the extract+expand (RFC 5869);
/// `iterations` is unused. `length` is the output length in bits (`length / 8`
/// bytes). The hash selects the HMAC PRF (SHA-1/256/384/512), reusing the `hmac`
/// crate that backs `sign` — no separate PBKDF2/HKDF dep (the `pbkdf2` 0.13 crate
/// pulls `digest` 0.11, incompatible with the `sha1`/`sha2` 0.10 DashScript uses;
/// both KDFs are short loops over HMAC anyway). `async` because ES `deriveBits`
/// returns `Promise<ArrayBuffer>`; the call site's `await` drives the future, and
/// `callee_return_path` records the `Vec<u8>` return (like `sign`/`encrypt`).
pub async fn crypto_subtle_derive_bits(
    name: ::std::string::String,
    hash: ::std::string::String,
    salt: ::std::vec::Vec<u8>,
    info: ::std::vec::Vec<u8>,
    iterations: u32,
    key: &DsCryptoKey,
    length: usize,
) -> ::std::vec::Vec<u8> {
    let password = &key.key;
    let mut out = ::std::vec![0u8; length / 8];
    match name.as_str() {
        "PBKDF2" => match hash.as_str() {
            "SHA-1" => pbkdf2_into::<::hmac::Hmac<::sha1::Sha1>>(
                password, &salt, iterations, &mut out, 20,
            ),
            "SHA-256" => pbkdf2_into::<::hmac::Hmac<::sha2::Sha256>>(
                password, &salt, iterations, &mut out, 32,
            ),
            "SHA-384" => pbkdf2_into::<::hmac::Hmac<::sha2::Sha384>>(
                password, &salt, iterations, &mut out, 48,
            ),
            "SHA-512" => pbkdf2_into::<::hmac::Hmac<::sha2::Sha512>>(
                password, &salt, iterations, &mut out, 64,
            ),
            _ => ::core::panic!(
                "TypeError: crypto.subtle.deriveBits: unsupported PBKDF2 hash"
            ),
        },
        "HKDF" => match hash.as_str() {
            "SHA-1" => hkdf_into::<::hmac::Hmac<::sha1::Sha1>>(
                &salt, password, &info, &mut out, 20,
            ),
            "SHA-256" => hkdf_into::<::hmac::Hmac<::sha2::Sha256>>(
                &salt, password, &info, &mut out, 32,
            ),
            "SHA-384" => hkdf_into::<::hmac::Hmac<::sha2::Sha384>>(
                &salt, password, &info, &mut out, 48,
            ),
            "SHA-512" => hkdf_into::<::hmac::Hmac<::sha2::Sha512>>(
                &salt, password, &info, &mut out, 64,
            ),
            _ => ::core::panic!(
                "TypeError: crypto.subtle.deriveBits: unsupported HKDF hash"
            ),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.deriveBits: unsupported algorithm"),
    }
    out
}
/// `crypto.subtle.deriveKey(algorithm, baseKey, derivedKeyType, extractable,
/// usages)` — the PBKDF2/HKDF subset, an orchestrator over `deriveBits` + the
/// key ctor (the WinterTC WebCrypto key-derivation-and-import path). The
/// derivation algorithm (PBKDF2's `name`/`hash`/`salt`/`iterations` or HKDF's
/// `name`/`hash`/`salt`/`info`) and the password/input-key-material
/// `baseKey` feed `crypto_subtle_derive_bits`; the `derivedKeyType` object's
/// `name`/`hash`/`length` decide the output key length in bits (AES-GCM/AES-CBC:
/// `length`; HMAC: `length`, else the named hash's block size — 512 for
/// SHA-1/SHA-256, 1024 for SHA-384/SHA-512, mirroring `generateKey`). The
/// derived bytes become the `DsCryptoKey`'s raw key. `async` because ES
/// `deriveKey` returns `Promise<CryptoKey>`; the call site's `await` drives the
/// future, and `callee_return_path` records the `DsCryptoKey` return (like
/// `importKey`/`generateKey`).
pub async fn crypto_subtle_derive_key(
    name: ::std::string::String,
    hash: ::std::string::String,
    salt: ::std::vec::Vec<u8>,
    info: ::std::vec::Vec<u8>,
    iterations: u32,
    base_key: &DsCryptoKey,
    derived_name: ::std::string::String,
    derived_hash: ::std::string::String,
    derived_length: usize,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    let length_bits = match derived_name.as_str() {
        "AES-GCM" | "AES-CBC" => derived_length,
        "HMAC" => {
            if derived_length == 0 {
                match derived_hash.as_str() {
                    "SHA-384" | "SHA-512" => 1024,
                    _ => 512,
                }
            } else {
                derived_length
            }
        }
        _ => ::core::panic!("TypeError: crypto.subtle.deriveKey: unsupported derived key type"),
    };
    let bits =
        crypto_subtle_derive_bits(name, hash, salt, info, iterations, base_key, length_bits).await;
    DsCryptoKey::new(derived_name, derived_hash, bits, extractable, usages)
}
/// The PBKDF2 (RFC 2898) core — `T_i = U_1 ^ … ^ U_c`, where
/// `U_1 = HMAC(P, S || INT_32_BE(i))` and `U_j = HMAC(P, U_{j-1})` — generic over
/// the HMAC instance (`H: hmac::Mac`), so each SHA hash arms with a concrete
/// `Hmac<D>` (no `digest` trait bounds to state). `h_len` is the hash output
/// length (the F-block size); `res` is filled block by block, the trailing
/// partial block truncated.
fn pbkdf2_into<H>(password: &[u8], salt: &[u8], rounds: u32, res: &mut [u8], h_len: usize)
where
    H: ::hmac::Mac + ::hmac::digest::KeyInit,
{
    use ::hmac::Mac;
    for (idx, chunk) in res.chunks_mut(h_len).enumerate() {
        let mut block = ::std::vec![0u8; salt.len() + 4];
        block[..salt.len()].copy_from_slice(salt);
        block[salt.len()..].copy_from_slice(&((idx as u32) + 1).to_be_bytes());
        let mut mac = <H as ::hmac::digest::KeyInit>::new_from_slice(password)
            .expect("HMAC key length");
        mac.update(&block);
        let mut u: ::std::vec::Vec<u8> = mac.finalize().into_bytes().to_vec();
        let mut t = u.clone();
        for _ in 1..rounds {
            let mut mac = <H as ::hmac::digest::KeyInit>::new_from_slice(password)
            .expect("HMAC key length");
            mac.update(&u);
            u = mac.finalize().into_bytes().to_vec();
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        let n = chunk.len().min(t.len());
        chunk[..n].copy_from_slice(&t[..n]);
    }
}
/// The HKDF (RFC 5869) core — extract-then-expand, generic over the HMAC
/// instance (`H: hmac::Mac`). Extract: `PRK = HMAC-Hash(salt, IKM)` (an empty
/// `salt` is HMAC's normal zero-key path, not RFC 5869's HashLen-zeros default,
/// but HMAC of an empty key is the same as HMAC of HashLen zero bytes for these
/// hashes — the test-vector round-trip confirms it). Expand:
/// `T(0) = ""`, `T(i) = HMAC(PRK, T(i-1) || info || octet(i))`, `OKM = T(1) ||
/// T(2) || …` truncated to `res.len()`. `h_len` is the hash output length.
fn hkdf_into<H>(salt: &[u8], ikm: &[u8], info: &[u8], res: &mut [u8], h_len: usize)
where
    H: ::hmac::Mac + ::hmac::digest::KeyInit,
{
    use ::hmac::Mac;
    // Extract — PRK = HMAC(salt, IKM); reuse as the expand key.
    let mut prk_mac = <H as ::hmac::digest::KeyInit>::new_from_slice(salt).expect("HMAC key length");
    prk_mac.update(ikm);
    let prk = prk_mac.finalize().into_bytes();
    // Expand — fill `res` block by block (each HMAC output is one block).
    let mut t: ::std::vec::Vec<u8> = ::std::vec![];
    let mut idx: u8 = 1;
    for chunk in res.chunks_mut(h_len) {
        let mut mac = <H as ::hmac::digest::KeyInit>::new_from_slice(&prk).expect("HMAC key length");
        mac.update(&t);
        mac.update(info);
        mac.update(&[idx]);
        t = mac.finalize().into_bytes().to_vec();
        let n = chunk.len().min(t.len());
        chunk[..n].copy_from_slice(&t[..n]);
        idx = idx.wrapping_add(1);
    }
}
/// `crypto.subtle.sign(algo, key, data)` — the HMAC subset. The hash comes from
/// the key; the ES `algo` arg is carried by `key.algorithm` (verified to be
/// `"HMAC"`). Returns the HMAC tag bytes. `async` because ES `sign` returns
/// `Promise<ArrayBuffer>`.
pub async fn crypto_subtle_sign(
    key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    match (key.algorithm.as_str(), key.hash.as_str()) {
        ("HMAC", "SHA-1") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha1 = Hmac<::sha1::Sha1>;
            let mut mac = HmacSha1::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        ("HMAC", "SHA-256") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha256 = Hmac<::sha2::Sha256>;
            let mut mac = HmacSha256::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        ("HMAC", "SHA-384") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha384 = Hmac<::sha2::Sha384>;
            let mut mac = HmacSha384::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        ("HMAC", "SHA-512") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha512 = Hmac<::sha2::Sha512>;
            let mut mac = HmacSha512::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        _ => ::core::panic!("TypeError: crypto.subtle.sign: unsupported algorithm"),
    }
}
/// `crypto.subtle.verify(algo, key, signature, data)` — the HMAC subset. Returns
/// `true` iff `signature` recomputes from `key`+`data`. The compare folds XOR so
/// it is constant-time-ish (HMAC verification is not secret-dependent in
/// practice, but the fold avoids an early-exit timing leak). `async` because ES
/// `verify` returns `Promise<boolean>`.
pub async fn crypto_subtle_verify(
    key: &DsCryptoKey,
    signature: ::std::vec::Vec<u8>,
    data: ::std::vec::Vec<u8>,
) -> bool {
    let computed = crypto_subtle_sign(key, data).await;
    if computed.len() != signature.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in computed.iter().zip(signature.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}
/// Seal with an AES-GCM cipher built at the call site (keyed by length:
/// `Aes128Gcm`/`Aes256Gcm`). `aead::Aead::encrypt` returns
/// `ciphertext || tag` — the WebCrypto AES-GCM output format — so the result is
/// byte-compatible with a browser `crypto.subtle.encrypt`. A nonce/length error
/// is impossible for a translator-built key+iv, so the `expect` is a panic on a
/// genuinely malformed input (the engine path's `TypeError` analogue).
fn aes_gcm_seal<C>(cipher: &C, iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    C: ::aead::Aead,
{
    use ::aead::Aead;
    cipher
        .encrypt(::aead::Nonce::<C>::from_slice(iv), data)
        .expect("AES-GCM encrypt")
        .to_vec()
}
/// The decrypt twin of [`aes_gcm_seal`] — `aead::Aead::decrypt` expects
/// `ciphertext || tag` and authenticates before returning the plaintext; a tag
/// mismatch (a tampered ciphertext) panics (ES rejects the promise with an
/// `OperationError`).
fn aes_gcm_open<C>(cipher: &C, iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    C: ::aead::Aead,
{
    use ::aead::Aead;
    cipher
        .decrypt(::aead::Nonce::<C>::from_slice(iv), data)
        .expect("AES-GCM decrypt")
        .to_vec()
}
/// Seal with an AES-CBC cipher built at the call site (keyed by length:
/// `Aes128`/`Aes256`), PKCS7-padded (the only padding WebCrypto uses). `cbc`
/// re-exports the `cipher` traits, so `cbc::cipher::*` resolves. The block size
/// and key size are inferred from `A` via `KeyIvInit::new`'s `&GenericArray`
/// params (`from_slice` lets a `&[u8]` stand in for the sized array).
fn aes_cbc_seal<A>(key: &[u8], iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockEncrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{
        BlockEncryptMut, KeyIvInit, block_padding::Pkcs7, generic_array::GenericArray,
    };
    ::cbc::Encryptor::<A>::new(GenericArray::from_slice(key), GenericArray::from_slice(iv))
        .encrypt_padded_vec_mut::<Pkcs7>(data)
}
/// The decrypt twin of [`aes_cbc_seal`] — `decrypt_padded_vec_mut` returns
/// `None` on a padding mismatch (a tampered ciphertext), panicked as the ES
/// `OperationError` analogue.
fn aes_cbc_open<A>(key: &[u8], iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockDecrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{
        BlockDecryptMut, KeyIvInit, block_padding::Pkcs7, generic_array::GenericArray,
    };
    ::cbc::Decryptor::<A>::new(GenericArray::from_slice(key), GenericArray::from_slice(iv))
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .expect("AES-CBC decrypt")
}
/// `crypto.subtle.encrypt(algo, key, data)` — the AES-GCM/AES-CBC subset. The ES
/// `algo` object's `name` (`"AES-GCM"`/`"AES-CBC"`) and `iv` (the nonce/IV, a
/// `Uint8Array`) are extracted at translate time; the key length selects
/// `Aes128`/`Aes256`. AES-GCM returns `ciphertext || tag` (the WebCrypto
/// AES-GCM output format); AES-CBC returns the PKCS7-padded ciphertext. `async`
/// because ES `encrypt` returns `Promise<ArrayBuffer>`. The `iv` is taken by
/// reference so the standard encrypt→decrypt round-trip can reuse one IV
/// binding. The `additionalData`/`tagLength` AES-GCM fields are not yet mapped.
pub async fn crypto_subtle_encrypt(
    name: ::std::string::String,
    iv: &[u8],
    key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    use ::aead::KeyInit;
    match name.as_str() {
        "AES-GCM" => match key.key.len() {
            16 => aes_gcm_seal(
                &::aes_gcm::Aes128Gcm::new(::aead::Key::<::aes_gcm::Aes128Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            32 => aes_gcm_seal(
                &::aes_gcm::Aes256Gcm::new(::aead::Key::<::aes_gcm::Aes256Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            _ => ::core::panic!("TypeError: AES-GCM key length must be 128/256 bits"),
        },
        "AES-CBC" => match key.key.len() {
            16 => aes_cbc_seal::<::aes::Aes128>(&key.key, iv, &data),
            32 => aes_cbc_seal::<::aes::Aes256>(&key.key, iv, &data),
            _ => ::core::panic!("TypeError: AES-CBC key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.encrypt: unsupported algorithm"),
    }
}
/// `crypto.subtle.decrypt(algo, key, data)` — the AES-GCM/AES-CBC subset. The
/// inverse of `encrypt`: AES-GCM `data` is `ciphertext || tag`, authenticated
/// then returned as plaintext; AES-CBC `data` is PKCS7-padded ciphertext, the
/// padding stripped (a mismatch panics — ES `OperationError`). `async` because
/// ES `decrypt` returns `Promise<ArrayBuffer>`. As with `encrypt`, `iv` is by
/// reference (the same IV binds a sealed pair).
pub async fn crypto_subtle_decrypt(
    name: ::std::string::String,
    iv: &[u8],
    key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    use ::aead::KeyInit;
    match name.as_str() {
        "AES-GCM" => match key.key.len() {
            16 => aes_gcm_open(
                &::aes_gcm::Aes128Gcm::new(::aead::Key::<::aes_gcm::Aes128Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            32 => aes_gcm_open(
                &::aes_gcm::Aes256Gcm::new(::aead::Key::<::aes_gcm::Aes256Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            _ => ::core::panic!("TypeError: AES-GCM key length must be 128/256 bits"),
        },
        "AES-CBC" => match key.key.len() {
            16 => aes_cbc_open::<::aes::Aes128>(&key.key, iv, &data),
            32 => aes_cbc_open::<::aes::Aes256>(&key.key, iv, &data),
            _ => ::core::panic!("TypeError: AES-CBC key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.decrypt: unsupported algorithm"),
    }
}
/// RFC 3394 AES Key Wrap — the only `wrapKey`/`unwrapKey` algorithm WinterTC
/// servers use (the AES-KW "authentic" wrap, no padding, no IV; AES-GCM/AES-CBC
/// wraps go through `encrypt`/`decrypt`). `cbc` re-exports `cipher`, so
/// `cbc::cipher::*` resolves (cipher 0.4, the same version `aes` 0.8 and `cbc`
/// 0.1 share). The wrap operates on 64-bit blocks: the key (a non-zero multiple
/// of 8 bytes, `n` blocks) is wrapped under the KEK into `n+1` blocks, the
/// leading `A` block carrying the `0xA6A6…` integrity check, with the `t = i +
/// n·j` counter XOR'd in (RFC 3394 §2.2.1).
fn aes_kw_wrap<A>(kek: &[u8], key: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockEncrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
    assert!(
        key.len() % 8 == 0 && !key.is_empty(),
        "AES-KW: key length must be a non-zero multiple of 8 bytes"
    );
    let cipher = <A as KeyInit>::new(GenericArray::from_slice(kek));
    let n = key.len() / 8;
    let mut a = [0xA6u8; 8];
    let mut r: ::std::vec::Vec<[u8; 8]> = (0..n)
        .map(|i| {
            let mut blk = [0u8; 8];
            blk.copy_from_slice(&key[i * 8..(i + 1) * 8]);
            blk
        })
        .collect();
    let mut block = [0u8; 16];
    for j in 0..6u64 {
        for i in 1..=n as u64 {
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&r[(i - 1) as usize]);
            cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));
            a.copy_from_slice(&block[..8]);
            let t = (n as u64 * j + i).to_be_bytes();
            for k in 0..8 {
                a[k] ^= t[k];
            }
            r[(i - 1) as usize].copy_from_slice(&block[8..]);
        }
    }
    let mut out = ::std::vec::Vec::with_capacity(8 * (n + 1));
    out.extend_from_slice(&a);
    for blk in &r {
        out.extend_from_slice(blk);
    }
    out
}
/// The decrypt twin of [`aes_kw_wrap`] (RFC 3394 §2.2.2, the reverse loop in
/// `j`/`i`). The post-loop `A == 0xA6A6…` check is the integrity check — a
/// mismatch means a tampered ciphertext or wrong KEK, panicked as ES
/// `OperationError`.
fn aes_kw_unwrap<A>(kek: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockDecrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
    assert!(
        data.len() % 8 == 0 && data.len() >= 16,
        "AES-KW: wrapped data must be a multiple of 8 bytes and at least 16 bytes"
    );
    let cipher = <A as KeyInit>::new(GenericArray::from_slice(kek));
    let n = data.len() / 8 - 1;
    let mut a = [0u8; 8];
    a.copy_from_slice(&data[..8]);
    let mut r: ::std::vec::Vec<[u8; 8]> = (0..n)
        .map(|i| {
            let mut blk = [0u8; 8];
            blk.copy_from_slice(&data[(i + 1) * 8..(i + 2) * 8]);
            blk
        })
        .collect();
    let mut block = [0u8; 16];
    for j in (0..6u64).rev() {
        for i in (1..=n as u64).rev() {
            let t = (n as u64 * j + i).to_be_bytes();
            for k in 0..8 {
                a[k] ^= t[k];
            }
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&r[(i - 1) as usize]);
            cipher.decrypt_block(GenericArray::from_mut_slice(&mut block));
            a.copy_from_slice(&block[..8]);
            r[(i - 1) as usize].copy_from_slice(&block[8..]);
        }
    }
    assert!(
        a == [0xA6u8; 8],
        "OperationError: AES-KW integrity check failed"
    );
    let mut out = ::std::vec::Vec::with_capacity(8 * n);
    for blk in &r {
        out.extend_from_slice(blk);
    }
    out
}
/// `crypto.subtle.wrapKey(format, key, wrappingKey, wrapAlgorithm)` — the AES-KW
/// subset. The `wrapAlgorithm` carries no IV (AES-KW is IV-less), so the call is
/// `aes_kw_wrap(wrappingKey.key, key.key)`; the KEK length selects
/// `Aes128`/`Aes256`. `format` is `"raw"` (the only export form lowered — the
/// raw key bytes are wrapped directly; jwk/pkcs8/spki are not statically
/// modeled). `async` because ES `wrapKey` returns `Promise<ArrayBuffer>`; the
/// call site's `await` drives the future, and `callee_return_path` records the
/// `Vec<u8>` return (like `encrypt`).
pub async fn crypto_subtle_wrap_key(
    name: ::std::string::String,
    wrapping_key: &DsCryptoKey,
    key: &DsCryptoKey,
) -> ::std::vec::Vec<u8> {
    match name.as_str() {
        "AES-KW" => match wrapping_key.key.len() {
            16 => aes_kw_wrap::<::aes::Aes128>(&wrapping_key.key, &key.key),
            32 => aes_kw_wrap::<::aes::Aes256>(&wrapping_key.key, &key.key),
            _ => ::core::panic!("TypeError: AES-KW wrapping key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.wrapKey: unsupported algorithm"),
    }
}
/// `crypto.subtle.unwrapKey(format, wrappedKey, wrappingKey, wrapAlgorithm,
/// wrappedKeyAlgorithm, extractable, usages)` — the AES-KW subset (the inverse
/// of `wrapKey`). The wrapped `data` is unwrapped under the KEK, then a
/// `DsCryptoKey` is rebuilt from the unwrapped raw bytes + the
/// `wrappedKeyAlgorithm`'s `name`/`hash` (AES keys carry no hash; HMAC keys
/// carry the named hash). `async` because ES `unwrapKey` returns
/// `Promise<CryptoKey>`; the call site's `await` drives the future, and
/// `callee_return_path` records the `DsCryptoKey` return (like `importKey`).
pub async fn crypto_subtle_unwrap_key(
    name: ::std::string::String,
    wrapping_key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
    wrapped_name: ::std::string::String,
    wrapped_hash: ::std::string::String,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    let raw = match name.as_str() {
        "AES-KW" => match wrapping_key.key.len() {
            16 => aes_kw_unwrap::<::aes::Aes128>(&wrapping_key.key, &data),
            32 => aes_kw_unwrap::<::aes::Aes256>(&wrapping_key.key, &data),
            _ => ::core::panic!("TypeError: AES-KW wrapping key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.unwrapKey: unsupported algorithm"),
    };
    DsCryptoKey::new(wrapped_name, wrapped_hash, raw, extractable, usages)
}
"#;
