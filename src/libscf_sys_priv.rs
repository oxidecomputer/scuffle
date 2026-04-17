// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![allow(unused_variables)]

#[cfg(target_os = "illumos")]
mod private {
    #[link(name = "scf")]
    unsafe extern "C" {
        pub fn _smf_refresh_instance_i(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> libc::c_int;
    }
}

#[cfg(not(target_os = "illumos"))]
mod private {
    pub unsafe fn _smf_refresh_instance_i(
        instance: *mut libscf_sys::scf_instance_t,
    ) -> libc::c_int {
        unimplemented!()
    }
}

pub(crate) use private::*;
