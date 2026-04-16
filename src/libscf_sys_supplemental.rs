// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![allow(unused_variables)]

#[cfg(target_os = "illumos")]
mod supplemental {
    #[link(name = "scf")]
    unsafe extern "C" {
        pub fn scf_service_get_pg(
            service: *const libscf_sys::scf_service_t,
            name: *const libc::c_char,
            out: *mut libscf_sys::scf_propertygroup_t,
        ) -> libc::c_int;

        pub fn scf_service_add_pg(
            service: *mut libscf_sys::scf_service_t,
            name: *const libc::c_char,
            pgtype: *const libc::c_char,
            flags: u32,
            out: *mut libscf_sys::scf_propertygroup_t,
        ) -> libc::c_int;
    }
}

#[cfg(not(target_os = "illumos"))]
mod supplemental {
    pub unsafe fn scf_service_get_pg(
        service: *const libscf_sys::scf_service_t,
        name: *const libc::c_char,
        out: *mut libscf_sys::scf_propertygroup_t,
    ) -> libc::c_int {
        unimplemented!()
    }
    pub unsafe fn scf_service_add_pg(
        service: *mut libscf_sys::scf_service_t,
        name: *const libc::c_char,
        pgtype: *const libc::c_char,
        flags: u32,
        out: *mut libscf_sys::scf_propertygroup_t,
    ) -> libc::c_int {
        unimplemented!()
    }
}

pub(crate) use supplemental::*;
