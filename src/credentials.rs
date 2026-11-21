use std::{io, ptr, slice};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

pub fn encrypt(secret: &str) -> io::Result<Vec<u8>> {
    transform(secret.as_bytes(), true)
}

pub fn decrypt(encrypted: &[u8]) -> io::Result<String> {
    String::from_utf8(transform(encrypted, false)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

fn transform(input: &[u8], encrypting: bool) -> io::Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: input
            .len()
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Credential is too large"))?,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // DPAPI reads the input buffer and allocates the output with LocalAlloc.
    let success = unsafe {
        if encrypting {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    if output.pbData.is_null() {
        return Ok(Vec::new());
    }
    let bytes = unsafe {
        let bytes = slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        // This buffer contains plaintext after decryption.
        if !encrypting {
            for index in 0..output.cbData as usize {
                output.pbData.add(index).write_volatile(0);
            }
        }
        LocalFree(output.pbData.cast());
        bytes
    };
    Ok(bytes)
}
