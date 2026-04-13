// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::sync::LazyLock;

pub(crate) static SCF_LIMIT_MAX_NAME_LENGTH: LazyLock<usize> =
    LazyLock::new(|| scf_limit(libscf_sys::SCF_LIMIT_MAX_NAME_LENGTH));

pub(crate) static SCF_LIMIT_MAX_FMRI_LENGTH: LazyLock<usize> =
    LazyLock::new(|| scf_limit(libscf_sys::SCF_LIMIT_MAX_FMRI_LENGTH));

pub(crate) static SCF_LIMIT_MAX_VALUE_LENGTH: LazyLock<usize> =
    LazyLock::new(|| scf_limit(libscf_sys::SCF_LIMIT_MAX_VALUE_LENGTH));

fn scf_limit(limit_id: u32) -> usize {
    let sz = unsafe { libscf_sys::scf_limit(limit_id) };

    // scf_limit() is documented as only failing if we pass an unknown
    // argument; this is a private function that we only call with
    // defined constants, so it should never fail. If the constant
    // values change or are out of sync, we should catch that
    // immediately in testing.
    assert!(sz > 0, "unexpected return value from scf_limit({limit_id}): {sz}");

    sz as usize
}
