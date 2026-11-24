//! Random values from the operating system: per-launch secrets, step tokens and session ids.

/// Fills `buf` with random bytes from the operating system.
pub fn fill(buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(buf)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{
            BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
        };
        let len = u32::try_from(buf.len()).map_err(std::io::Error::other)?;
        // SAFETY: `buf` is valid for `len` bytes; a null algorithm handle is allowed with
        // BCRYPT_USE_SYSTEM_PREFERRED_RNG.
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                len,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "BCryptGenRandom failed: {status:#x}"
            )))
        }
    }
}

/// `n` random bytes as lowercase hex.
pub fn hex(n: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; n];
    fill(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// A random (version 4) UUID, as agents take for a session id.
pub fn uuid_v4() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    fill(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        h[0..4].concat(),
        h[4..6].concat(),
        h[6..8].concat(),
        h[8..10].concat(),
        h[10..16].concat()
    ))
}

/// Compares two strings in constant time for equal lengths (for secrets).
pub fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_have_the_v4_shape_and_differ() {
        let u = uuid_v4().unwrap();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(u, uuid_v4().unwrap());
    }

    #[test]
    fn secrets_compare() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
        assert_eq!(hex(32).unwrap().len(), 64);
    }
}
