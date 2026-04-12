// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::cell::Cell;
use std::cell::RefCell;
use std::thread::LocalKey;

/// Run a closure with access to a thread-local buffer sized to
/// `scf_limit(SCF_LIMIT_MAX_VALUE_LENGTH) + 1`.
///
/// This function is not reentrant with itself nor with the other `with_buf_*`
/// functions in this module.
pub(crate) fn with_scf_value_buf<F, T>(f: F) -> T
where
    F: FnOnce(&mut Vec<u8>) -> T,
{
    thread_local! {
        static MAX_VALUE_LEN: Cell<Option<usize>> = const { Cell::new(None) };
    }

    let len = cache_max_len_plus_1(
        &MAX_VALUE_LEN,
        libscf_sys::SCF_LIMIT_MAX_VALUE_LENGTH,
    );

    with_buf(f, len)
}

/// Run a closure with access to a thread-local buffer sized to
/// `scf_limit(SCF_LIMIT_MAX_FMRI_LENGTH) + 1`.
///
/// This function is not reentrant with itself nor with the other `with_buf_*`
/// functions in this module.
#[allow(dead_code)] // TODO remove once we write fmri() methods
pub(crate) fn with_scf_fmri_buf<F, T>(f: F) -> T
where
    F: FnOnce(&mut Vec<u8>) -> T,
{
    thread_local! {
        static MAX_FMRI_LEN: Cell<Option<usize>> = const { Cell::new(None) };
    }

    let len = cache_max_len_plus_1(
        &MAX_FMRI_LEN,
        libscf_sys::SCF_LIMIT_MAX_FMRI_LENGTH,
    );

    with_buf(f, len)
}

fn cache_max_len_plus_1(
    max_len_tlocal: &'static LocalKey<Cell<Option<usize>>>,
    limit_id: u32,
) -> usize {
    max_len_tlocal.with(|maybe_cached| {
        if let Some(len) = maybe_cached.get() {
            return len;
        }

        let sz = unsafe { libscf_sys::scf_limit(limit_id) };

        // scf_limit() is documented as only failing if we pass an unknown
        // argument; this is a private function that we only call with
        // defined constants, so it should never fail. If the constant
        // values change or are out of sync, we should catch that
        // immediately in testing.
        assert!(
            sz > 0,
            "unexpected return value from scf_limit({limit_id}): {sz}"
        );

        // Add one to account for the Nul byte (manpage for scf_limit notes it
        // returns values _without_ room for the Nul byte).
        let sz = sz as usize + 1;

        maybe_cached.set(Some(sz));

        sz
    })
}

fn with_buf<F, T>(f: F, max_len: usize) -> T
where
    F: FnOnce(&mut Vec<u8>) -> T,
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
