//! KEEPING A SECRET ON DISK, OR REFUSING TO.
//!
//! One thing in this app is worth protecting at rest: the Twitch access token, which can speak in
//! chat as the owner. `settings.json` is plain JSON on purpose and everything in it is inert; a
//! token is not, so it does not go there and it does not go anywhere else in the clear.
//!
//! WINDOWS DATA PROTECTION API, AND WHAT IT ACTUALLY BUYS. `CryptProtectData` encrypts with a key
//! derived from the logged-in user's credentials, held by the OS. The ciphertext is bound to THIS
//! USER on THIS MACHINE: a file copied to another machine, or opened by another account on this
//! one, cannot be decrypted, and no key material lives in this binary for anyone to extract.
//!
//! WHAT IT DOES NOT BUY, SAID PLAINLY SO NOBODY BUILDS ON A FALSE FLOOR. It is not protection from
//! the owner's own account. Any program running as this user can call `CryptUnprotectData` on the
//! same blob with the same entropy and read the token, and malware already running as you has far
//! easier ways to hurt you. What this defeats is the realistic case: a token sitting in readable
//! plaintext in a roaming profile, in a backup, in a synced folder, or in a support bundle
//! somebody zips up and emails.
//!
//! THE ENTROPY IS NOT A KEY AND IS NOT PRETENDING TO BE. `ENTROPY` is a fixed application string
//! mixed into the derivation, so a blob written by this app cannot be unsealed by a different
//! program that merely happens to run as the same user and guess the file. It is in the source in
//! the clear, it is meant to be, and calling it a secret would be the kind of security theatre
//! this file exists to avoid.
//!
//! EVERY OTHER PLATFORM REFUSES RATHER THAN DEGRADES. There is no DPAPI on Linux or macOS, and the
//! wrong answer there is to write the token in the clear and say nothing. [`seal`] answers `None`
//! off Windows, the caller does not persist, and the screen says the sign-in lasts until the app
//! closes. A feature that quietly becomes less safe on another platform is worse than one that is
//! honestly absent.

/// Mixed into the key derivation. See the module note: this is not a key and is not secret.
const ENTROPY: &[u8] = b"eql-grimoire/twitch-token/v1";

#[cfg(windows)]
mod imp {
    use super::ENTROPY;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    /// A blob pointing at a slice. The API takes a length and a pointer and writes neither.
    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        }
    }

    /// Copy what the API allocated, then hand its memory back.
    ///
    /// `LocalFree` IS NOT OPTIONAL. Both calls allocate the output with `LocalAlloc` and the
    /// caller owns it. A sign-in refreshes every few hours for as long as the app is open, so a
    /// leak here would grow for the length of a stream rather than being a one off.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        let _ = unsafe { LocalFree(Some(HLOCAL(out.pbData as *mut _))) };
        v
    }

    pub fn seal(plain: &[u8]) -> Option<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        let r = unsafe {
            CryptProtectData(
                &blob(plain),
                None,
                Some(&blob(ENTROPY)),
                None,
                None,
                0,
                &mut out,
            )
        };
        match r {
            Ok(()) => Some(unsafe { take(out) }),
            Err(e) => {
                log::warn!("the token could not be encrypted, so it was not saved: {e}");
                None
            }
        }
    }

    pub fn unseal(sealed: &[u8]) -> Option<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        let r = unsafe {
            CryptUnprotectData(
                &blob(sealed),
                None,
                Some(&blob(ENTROPY)),
                None,
                None,
                0,
                &mut out,
            )
        };
        match r {
            Ok(()) => Some(unsafe { take(out) }),
            /* NOT AN ERROR WORTH SHOUTING ABOUT. This is what a blob written by another user, on
             * another machine, or by an older version with different entropy looks like, and the
             * right answer to all three is the same: sign in again. */
            Err(_) => None,
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// No DPAPI here, so nothing is persisted. See the module note on why this refuses rather
    /// than falling back to plaintext.
    pub fn seal(_plain: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unseal(_sealed: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// WHETHER A SECRET CAN BE KEPT ON THIS MACHINE AT ALL.
///
/// The Chat screen has to tell the owner whether signing in lasts past this run, and the honest
/// answer differs by platform: Windows has DPAPI, nothing else here does, and the fallback is to
/// persist nothing rather than to write a bearer token in the clear. A screen that promised
/// "saved for next time" on a machine where nothing is saved would be worse than one that said
/// nothing, so the words come from this.
pub fn persists() -> bool {
    cfg!(windows)
}

/// Encrypt for this user on this machine, or `None` if that cannot be done here.
pub fn seal(plain: &[u8]) -> Option<Vec<u8>> {
    imp::seal(plain)
}

/// Decrypt something [`seal`] produced, or `None` if it was not, or was not for us.
pub fn unseal(sealed: &[u8]) -> Option<Vec<u8>> {
    imp::unseal(sealed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHAT GOES IN COMES OUT, AND WHAT GOES ON DISK IS NOT THE PLAINTEXT.
    ///
    /// The second half is the one that matters and the one a careless implementation gets wrong:
    /// a `seal` that returned its input would satisfy a round trip test perfectly while writing
    /// the token to disk in the clear. So the ciphertext is checked for the plaintext bytes.
    #[cfg(windows)]
    #[test]
    fn a_sealed_secret_round_trips_and_does_not_contain_itself() {
        let plain = b"oauth:THIS-IS-THE-TOKEN-THAT-SPEAKS-AS-ME";
        let sealed = seal(plain).expect("DPAPI is present on Windows");
        assert_ne!(sealed.as_slice(), plain.as_slice());
        assert!(
            !sealed.windows(plain.len()).any(|w| w == plain.as_slice()),
            "the plaintext is sitting inside the ciphertext, so nothing was encrypted"
        );
        assert!(
            sealed.len() > plain.len(),
            "a real DPAPI blob carries a header and is longer than its input"
        );
        assert_eq!(unseal(&sealed).as_deref(), Some(plain.as_slice()));
    }

    /// RUBBISH DOES NOT UNSEAL, AND DOES NOT PANIC.
    ///
    /// The file on disk is attacker writable in the sense that anything running as this user can
    /// replace it, and a truncated one is what a power cut during a write leaves behind. Both must
    /// answer "sign in again" rather than taking the process down.
    #[cfg(windows)]
    #[test]
    fn a_corrupt_blob_is_refused_rather_than_trusted_or_fatal() {
        assert_eq!(unseal(b""), None);
        assert_eq!(unseal(b"not a dpapi blob at all"), None);
        let mut sealed = seal(b"something").expect("DPAPI is present on Windows");
        let n = sealed.len();
        sealed.truncate(n / 2);
        assert_eq!(unseal(&sealed), None, "a torn write must not decrypt");
        /* AND A FLIPPED BIT IS CAUGHT. DPAPI authenticates its blob, so this is not merely
         * "probably fails"; a tampered token must never be handed back as a good one. */
        let mut sealed = seal(b"something").expect("DPAPI is present on Windows");
        let last = sealed.len() - 1;
        sealed[last] ^= 0xFF;
        assert_eq!(unseal(&sealed), None, "a tampered blob decrypted");
    }

    /// OFF WINDOWS NOTHING IS PERSISTED, WHICH IS THE POINT.
    #[cfg(not(windows))]
    #[test]
    fn without_dpapi_nothing_is_written_in_the_clear() {
        assert_eq!(seal(b"token"), None);
        assert_eq!(unseal(b"token"), None);
    }
}
