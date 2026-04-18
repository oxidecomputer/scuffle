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

    #[cfg(feature = "smf-by-instance")]
    #[link(name = "scf")]
    unsafe extern "C" {
        pub fn smf_refresh_all_instances(
            service: *mut libscf_sys::scf_service_t,
        ) -> libc::c_int;
        pub fn smf_enable_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
            comment: *const libc::c_char,
        ) -> libc::c_int;
        pub fn smf_disable_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
            comment: *const libc::c_char,
        ) -> libc::c_int;
        pub fn smf_refresh_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> libc::c_int;
        pub fn smf_restart_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> libc::c_int;
        pub fn smf_maintain_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
        ) -> libc::c_int;
        pub fn smf_degrade_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
        ) -> libc::c_int;
        pub fn smf_restore_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> libc::c_int;
        pub fn smf_get_state_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> *mut libc::c_char;
    }

    #[cfg(feature = "smf-by-instance")]
    pub const SCF_COMMENT_MAX_LENGTH: usize = 1024;
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

    #[cfg(feature = "smf-by-instance")]
    mod by_instance {
        pub const SCF_COMMENT_MAX_LENGTH: usize = 1024;

        pub unsafe fn smf_refresh_all_instances(
            service: *mut libscf_sys::scf_service_t,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_enable_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
            comment: *const libc::c_char,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_disable_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
            comment: *const libc::c_char,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_refresh_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_restart_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_maintain_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_degrade_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
            flags: libc::c_int,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_restore_instance_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> libc::c_int {
            unimplemented!()
        }
        pub unsafe fn smf_get_state_by_instance(
            instance: *mut libscf_sys::scf_instance_t,
        ) -> *mut libc::c_char {
            unimplemented!()
        }
    }

    #[cfg(feature = "smf-by-instance")]
    pub use by_instance::*;
}

pub(crate) use supplemental::*;
