// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Scf;
use crate::error::LibscfError;
use crate::scf::ScfObject;
use std::marker::PhantomData;

mod sealed {
    pub(crate) trait ScfIterable {
        unsafe fn try_next(
            iter: *mut libscf_sys::scf_iter_t,
            uninitialized_item: *mut Self,
        ) -> libc::c_int;
    }
}

impl sealed::ScfIterable for libscf_sys::scf_value_t {
    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_value(iter, uninitialized_item) }
    }
}

impl sealed::ScfIterable for libscf_sys::scf_propertygroup_t {
    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_pg(iter, uninitialized_item) }
    }
}

impl sealed::ScfIterable for libscf_sys::scf_property_t {
    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_property(iter, uninitialized_item) }
    }
}

pub(crate) struct ScfUninitializedIter<'a> {
    handle: ScfObject<'a, libscf_sys::scf_iter_t>,
}

impl<'a> ScfUninitializedIter<'a> {
    pub(crate) fn new(scf: &'a Scf<'a>) -> Result<Self, LibscfError> {
        Ok(Self { handle: scf.scf_iter_create()? })
    }

    pub(crate) unsafe fn init_property_values(
        self,
        property: *const libscf_sys::scf_property_t,
    ) -> Result<ScfIter<'a, libscf_sys::scf_value_t>, LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_iter_property_values(self.handle.as_ptr(), property)
        })?;
        Ok(ScfIter { handle: self.handle, _inner: PhantomData })
    }

    pub(crate) unsafe fn init_service_property_groups(
        self,
        service: *const libscf_sys::scf_service_t,
    ) -> Result<ScfIter<'a, libscf_sys::scf_propertygroup_t>, LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_iter_service_pgs(self.handle.as_ptr(), service)
        })?;
        Ok(ScfIter { handle: self.handle, _inner: PhantomData })
    }

    pub(crate) unsafe fn init_property_group_properties(
        self,
        pg: *const libscf_sys::scf_propertygroup_t,
    ) -> Result<ScfIter<'a, libscf_sys::scf_property_t>, LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_iter_pg_properties(self.handle.as_ptr(), pg)
        })?;
        Ok(ScfIter { handle: self.handle, _inner: PhantomData })
    }
}

pub(crate) struct ScfIter<'a, T> {
    handle: ScfObject<'a, libscf_sys::scf_iter_t>,
    _inner: PhantomData<fn() -> T>,
}

impl<'a, T: sealed::ScfIterable> ScfIter<'a, T> {
    pub(crate) unsafe fn try_next(
        &mut self,
        out: *mut T,
    ) -> Option<Result<(), LibscfError>> {
        match unsafe { T::try_next(self.handle.as_ptr(), out) } {
            0 => None,
            1 => Some(Ok(())),
            _ => Some(Err(LibscfError::last())),
        }
    }
}
