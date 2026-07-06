//! Windows DPAPI（用户作用域）加解密小封装——给私有源密码落盘加密。
//! 密文只有同一 Windows 用户能解；换机 / 换用户无法解开（符合本地凭据预期）。

#[cfg(windows)]
pub fn encrypt(plain: &str) -> Result<Vec<u8>, String> {
    use std::ptr::null_mut;
    use winapi::um::dpapi::CryptProtectData;
    use winapi::um::winbase::LocalFree;
    use winapi::um::wincrypt::DATA_BLOB;

    let mut input = plain.as_bytes().to_vec();
    let mut in_blob = DATA_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_mut_ptr(),
    };
    let mut out_blob = DATA_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &mut in_blob,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            0,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return Err("DPAPI 加密失败".into());
    }
    let out =
        unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec() };
    unsafe {
        LocalFree(out_blob.pbData as *mut _);
    }
    Ok(out)
}

#[cfg(windows)]
pub fn decrypt(data: &[u8]) -> Result<String, String> {
    use std::ptr::null_mut;
    use winapi::um::dpapi::CryptUnprotectData;
    use winapi::um::winbase::LocalFree;
    use winapi::um::wincrypt::DATA_BLOB;

    let mut input = data.to_vec();
    let mut in_blob = DATA_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_mut_ptr(),
    };
    let mut out_blob = DATA_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &mut in_blob,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            0,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return Err("DPAPI 解密失败".into());
    }
    let out =
        unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec() };
    unsafe {
        LocalFree(out_blob.pbData as *mut _);
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn encrypt(plain: &str) -> Result<Vec<u8>, String> {
    Ok(plain.as_bytes().to_vec())
}
#[cfg(not(windows))]
pub fn decrypt(data: &[u8]) -> Result<String, String> {
    String::from_utf8(data.to_vec()).map_err(|e| e.to_string())
}
