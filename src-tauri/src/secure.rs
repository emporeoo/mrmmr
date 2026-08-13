use windows::core::PCWSTR;
use windows::Win32::Foundation::LocalFree;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use crate::nexus::AuthError;

/// Encrypt a secret using Windows DPAPI scoped to the current user.
///
/// The returned ciphertext is only decryptable by the same Windows user on
/// the same machine, and remains valid across app restarts and reboots.
pub fn encrypt(plaintext: &str) -> Result<Vec<u8>, AuthError> {
    let input = plaintext.as_bytes();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    unsafe {
        CryptProtectData(
            &input_blob,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|e| AuthError::Storage(format!("Could not encrypt credential: {e}")))?;

    let ciphertext =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData.cast(),
        )))
    };
    Ok(ciphertext)
}

/// Decrypt a secret previously encrypted with [`encrypt`].
pub fn decrypt(ciphertext: &[u8]) -> Result<String, AuthError> {
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    unsafe {
        CryptUnprotectData(
            &input_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|e| AuthError::Storage(format!("Could not decrypt credential: {e}")))?;

    let plaintext =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData.cast(),
        )))
    };

    String::from_utf8(plaintext)
        .map_err(|_| AuthError::Storage("Decrypted credential is not valid UTF-8".into()))
}
