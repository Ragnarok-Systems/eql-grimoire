//! KEEPING A SECRET ON DISK, OR REFUSING TO.
//!
//! Two things in this app are worth protecting at rest: the Twitch refresh token, which can speak
//! in chat as the owner, and the Ragnarok account token, which signs the app in as the owner's
//! account. `settings.json` is plain JSON on purpose and everything in it is inert; a token is not,
//! so it does not go there and it does not go anywhere else in the clear.
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
//! THE ENTROPY IS NOT A KEY AND IS NOT PRETENDING TO BE. Each [`Purpose`] mixes a fixed application
//! string into the derivation, so a blob written by this app cannot be unsealed by a different
//! program that merely happens to run as the same user and guess the file, and a blob written for
//! one purpose cannot be swapped into the other's file and read back as the wrong kind of token.
//! The strings are in the source in the clear, they are meant to be, and calling them secrets
//! would be the kind of security theatre this file exists to avoid.
//!
//! EVERY OTHER PLATFORM REFUSES RATHER THAN DEGRADES. There is no DPAPI on Linux or macOS, and the
//! wrong answer there is to write the token in the clear and say nothing. [`seal`] answers `None`
//! off Windows, the caller does not persist, and the screen says the sign-in lasts until the app
//! closes. A feature that quietly becomes less safe on another platform is worse than one that is
//! honestly absent.

/// WHICH SECRET IS BEING KEPT, and so which entropy is mixed in.
///
/// AN ENUM AND NOT A `&[u8]` PARAMETER, so a caller cannot invent a third string at a call site
/// and every entropy this app has ever written lives in one match below.
#[derive(Clone, Copy, Debug)]
pub enum Purpose {
    /// The Twitch refresh token (`twitch_auth`). Its entropy is the ORIGINAL one and must never
    /// change: every sign-in saved before accounts existed was sealed with it.
    TwitchToken,
    /// The Ragnarok account app token (`account`).
    AccountToken,
}

impl Purpose {
    /// Mixed into the key derivation. See the module note: this is not a key and is not secret.
    ///
    /// Windows only, because only DPAPI reads it; everywhere else `seal` refuses, and an ungated
    /// function is dead code there that `-D warnings` rejects on the Linux CI runner.
    ///
    /// `TwitchToken`'s bytes ARE THE ONES THIS FILE SHIPPED WITH as its single `ENTROPY` constant.
    /// Changing them signs every existing Twitch sign-in out on upgrade, silently, and
    /// `a_twitch_blob_sealed_the_old_way_still_unseals` is what fails if they move.
    #[cfg(windows)]
    fn entropy(self) -> &'static [u8] {
        match self {
            Purpose::TwitchToken => b"eql-grimoire/twitch-token/v1",
            Purpose::AccountToken => b"eql-grimoire/account-token/v1",
        }
    }
}

#[cfg(windows)]
mod imp {
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

    pub fn seal(entropy: &[u8], plain: &[u8]) -> Option<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        let r = unsafe {
            CryptProtectData(
                &blob(plain),
                None,
                Some(&blob(entropy)),
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

    pub fn unseal(entropy: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        let r = unsafe {
            CryptUnprotectData(
                &blob(sealed),
                None,
                Some(&blob(entropy)),
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

/// Encrypt the Twitch token for this user on this machine, or `None` if that cannot be done here.
///
/// THE ORIGINAL SIGNATURE, KEPT. `twitch_auth` calls this and nothing about what it writes has
/// changed; it is [`seal_for`] with [`Purpose::TwitchToken`].
pub fn seal(plain: &[u8]) -> Option<Vec<u8>> {
    seal_for(Purpose::TwitchToken, plain)
}

/// Decrypt something [`seal`] produced, or `None` if it was not, or was not for us.
pub fn unseal(sealed: &[u8]) -> Option<Vec<u8>> {
    unseal_for(Purpose::TwitchToken, sealed)
}

/// Encrypt for this user on this machine under `purpose`'s entropy, or `None` off Windows.
pub fn seal_for(purpose: Purpose, plain: &[u8]) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        imp::seal(purpose.entropy(), plain)
    }
    /* No DPAPI here, so nothing is persisted. See the module note on why this refuses rather than
     * falling back to plaintext. */
    #[cfg(not(windows))]
    {
        let _ = (purpose, plain);
        None
    }
}

/// Decrypt something [`seal_for`] produced under the SAME purpose, or `None`.
pub fn unseal_for(purpose: Purpose, sealed: &[u8]) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        imp::unseal(purpose.entropy(), sealed)
    }
    #[cfg(not(windows))]
    {
        let _ = (purpose, sealed);
        None
    }
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

    /// A TWITCH SIGN-IN SAVED BEFORE ACCOUNTS EXISTED STILL COMES BACK.
    ///
    /// THE BLOB IS SEALED WITH THE LITERAL BYTES, NOT WITH `Purpose::entropy`. Before this file
    /// grew purposes it had one constant, `ENTROPY = b"eql-grimoire/twitch-token/v1"`, and every
    /// `twitch-token.bin` in the field was sealed with exactly that. Sealing the fixture through
    /// the enum would prove only that the enum agrees with itself; sealing it with the old bytes,
    /// spelled out here, is what fails the day somebody "tidies" the Twitch string.
    ///
    /// WHAT MUTATION MAKES THIS RED: change any byte of `Purpose::TwitchToken`'s entropy, or route
    /// `unseal` through `Purpose::AccountToken`.
    #[cfg(windows)]
    #[test]
    fn a_twitch_blob_sealed_the_old_way_still_unseals() {
        let old = imp::seal(b"eql-grimoire/twitch-token/v1", b"REFRESH-FROM-LAST-MONTH")
            .expect("DPAPI is present on Windows");
        assert_eq!(
            unseal(&old).as_deref(),
            Some(b"REFRESH-FROM-LAST-MONTH".as_slice()),
            "a Twitch token sealed before accounts existed no longer unseals, so every owner is \
             signed out of chat by the upgrade"
        );
        assert_eq!(
            unseal_for(Purpose::TwitchToken, &old).as_deref(),
            Some(b"REFRESH-FROM-LAST-MONTH".as_slice())
        );
    }

    /// THE TWO PURPOSES CANNOT READ EACH OTHER'S BLOBS.
    ///
    /// The account token file and the Twitch token file sit side by side. A blob copied from one
    /// into the other must not unseal as the wrong kind of token, which is what distinct entropy
    /// buys and what a shared one would silently lose.
    #[cfg(windows)]
    #[test]
    fn an_account_blob_and_a_twitch_blob_are_not_interchangeable() {
        let account = seal_for(Purpose::AccountToken, b"rsa_ACCOUNT").expect("DPAPI");
        let twitch = seal_for(Purpose::TwitchToken, b"TWITCH").expect("DPAPI");
        assert_eq!(
            unseal_for(Purpose::AccountToken, &account).as_deref(),
            Some(b"rsa_ACCOUNT".as_slice())
        );
        assert_eq!(unseal_for(Purpose::TwitchToken, &account), None);
        assert_eq!(unseal(&account), None);
        assert_eq!(unseal_for(Purpose::AccountToken, &twitch), None);
    }

    /// OFF WINDOWS NOTHING IS PERSISTED, WHICH IS THE POINT.
    #[cfg(not(windows))]
    #[test]
    fn without_dpapi_nothing_is_written_in_the_clear() {
        assert_eq!(seal(b"token"), None);
        assert_eq!(unseal(b"token"), None);
        assert_eq!(seal_for(Purpose::AccountToken, b"token"), None);
        assert_eq!(unseal_for(Purpose::AccountToken, b"token"), None);
    }
}
