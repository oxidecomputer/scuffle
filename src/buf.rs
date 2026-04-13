// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::error::LibscfError;
use crate::error::ScfStringError;
use crate::limit;
use crate::utf8cstring::Utf8CString;
use std::cell::RefCell;
use std::ffi::CStr;

pub(crate) fn scf_get_name<F>(f: F) -> Result<Utf8CString, ScfStringError>
where
    F: FnOnce(*mut libc::c_char, usize) -> libc::ssize_t,
{
    with_scf_name_buf(move |buf| scf_get_string("name", buf, f))
}

pub(crate) fn scf_get_string<F>(
    kind: &'static str,
    buf: &mut [u8],
    f: F,
) -> Result<Utf8CString, ScfStringError>
where
    F: FnOnce(*mut libc::c_char, usize) -> libc::ssize_t,
{
    let scf_len = LibscfError::from_ssize(f(
        buf.as_mut_ptr().cast::<libc::c_char>(),
        buf.len(),
    ))
    .map_err(|err| ScfStringError::Get { kind, err })?;

    // `libscf` always returns the length of the _internal_ string as `scf_len`,
    // not counting its nul terminator. If this fits in `buf`, then `scf_len +
    // 1` (+ 1 to account for nul) is at most `buf.len()`; otherwise, `buf` was
    // too small.
    if scf_len + 1 > buf.len() {
        return Err(ScfStringError::OutOfBounds {
            kind,
            scf_len,
            max_len: buf.len(),
        });
    }

    let cstr = CStr::from_bytes_with_nul(&buf[..scf_len + 1])?;
    let utf8_cstring = Utf8CString::from_c_str(cstr)?;

    Ok(utf8_cstring)
}

/// Run a closure with access to a thread-local buffer sized to
/// `scf_limit(SCF_LIMIT_MAX_VALUE_LENGTH) + 1`.
///
/// This function is not reentrant with itself nor with the other `with_buf_*`
/// functions in this module.
pub(crate) fn with_scf_value_buf<F, T>(f: F) -> T
where
    F: FnOnce(&mut [u8]) -> T,
{
    with_buf(f, *limit::SCF_LIMIT_MAX_VALUE_LENGTH + 1)
}

/// Run a closure with access to a thread-local buffer sized to
/// `scf_limit(SCF_LIMIT_MAX_NAME_LENGTH) + 1`.
///
/// This function is not reentrant with itself nor with the other `with_buf_*`
/// functions in this module.
pub(crate) fn with_scf_name_buf<F, T>(f: F) -> T
where
    F: FnOnce(&mut [u8]) -> T,
{
    with_buf(f, *limit::SCF_LIMIT_MAX_NAME_LENGTH + 1)
}

/// Run a closure with access to a thread-local buffer sized to
/// `scf_limit(SCF_LIMIT_MAX_FMRI_LENGTH) + 1`.
///
/// This function is not reentrant with itself nor with the other `with_buf_*`
/// functions in this module.
#[allow(dead_code)] // TODO remove once we write fmri() methods
pub(crate) fn with_scf_fmri_buf<F, T>(f: F) -> T
where
    F: FnOnce(&mut [u8]) -> T,
{
    with_buf(f, *limit::SCF_LIMIT_MAX_FMRI_LENGTH + 1)
}

fn with_buf<F, T>(f: F, max_len: usize) -> T
where
    F: FnOnce(&mut [u8]) -> T,
{
    thread_local! {
        static BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    BUF.with_borrow_mut(|buf| {
        buf.clear();
        buf.resize(max_len, 0);
        f(buf)
    })
}
