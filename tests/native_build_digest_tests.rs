//! Contracts for the engine's digest formatting and the content-addressed
//! prefix identity it feeds. The engine is a source module shared by the
//! build script and xtask, so these tests include it directly; dead-code
//! allowances cover the parts only the real entry points reach, and the
//! included engine carries its sha2 dependency here through the crate's
//! dev-dependencies.

#[allow(dead_code)]
#[path = "../native_build.rs"]
mod native_build;

use native_build::{digest_lowercase_hex, pinned_pair_identity};

#[test]
fn should_encode_digest_bytes_as_lowercase_hex_with_leading_zeros() {
    // SHA-256 of the empty input: every byte exercises the table, and bytes
    // like 0xb0 and 0xc4 pin the zero-nibble positions — a dropped zero
    // digit anywhere in this 64-character string would corrupt prefix
    // identities and checksum comparisons downstream.
    let empty_input_digest = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9,
        0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52,
        0xb8, 0x55,
    ];
    assert_eq!(
        digest_lowercase_hex(&empty_input_digest),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn should_keep_pinned_pair_identity_stable() {
    // The golden identity of the current pins + recipe. Any change that
    // breaks this string orphans every machine's compiled native pair —
    // updating it must therefore always be a conscious act that rides a
    // deliberate pin or recipe bump, never an accident of refactoring or a
    // dependency upgrade changing how digests render.
    assert_eq!(pinned_pair_identity(), "ebb073908e65ee10");
}
