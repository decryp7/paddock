//! VNC authentication (RFB security type 2): DES-encrypt the 16-byte challenge with the password as key.

use des::Des;
use des::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use zeroize::Zeroizing;

/// The response to a VNC authentication challenge. The password is truncated to 8 bytes (the protocol's limit, and
/// VMware's) and each key byte is bit-reversed, a quirk of the original VNC implementation. The password is never logged.
pub fn vnc_auth_response(password: &[u8], challenge: &[u8; 16]) -> [u8; 16] {
    // The key is derived from the password: it is wiped when it goes out of scope (and the cipher's own key schedule is
    // wiped on drop by des's `zeroize` feature).
    let mut key = Zeroizing::new([0u8; 8]);
    for (k, b) in key.iter_mut().zip(password) {
        *k = b.reverse_bits();
    }
    let cipher = Des::new(GenericArray::from_slice(&key[..]));
    let mut out = *challenge;
    for chunk in out.chunks_mut(8) {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        // Reference value from OpenSSL: DES-ECB of bytes 0..16 under the bit-reversed key of "password" (0e86ceceeef64e26).
        let challenge: [u8; 16] = core::array::from_fn(|i| i as u8);
        let r = vnc_auth_response(b"password", &challenge);
        let want = [
            0xb8, 0x66, 0x92, 0x41, 0x25, 0xc8, 0xee, 0xbb, 0x9d, 0xeb, 0xc1, 0xdb, 0x61, 0xc5, 0x38, 0xe2,
        ];
        assert_eq!(r, want);
        assert_eq!(r, vnc_auth_response(b"password-and-more", &challenge), "only 8 bytes count");
        assert_ne!(r, vnc_auth_response(b"Password", &challenge));
        assert_ne!(&r[..], &challenge[..]);
        // An empty password is a valid (all-zero) key, not a panic.
        let _ = vnc_auth_response(b"", &challenge);
    }
}
