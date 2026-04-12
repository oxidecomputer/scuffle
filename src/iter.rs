// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Scf;
use std::marker::PhantomData;
use std::ptr::NonNull;

pub(crate) trait ScfIterable {
    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int;
}

impl ScfIterable for libscf_sys::scf_value_t {
    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_value(iter, uninitialized_item) }
    }
}

impl ScfIterable for libscf_sys::scf_propertygroup_t {
    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_pg(iter, uninitialized_item) }
    }
}

struct ScfIterHandle<'scf> {
    // Phantom data referring to the `Scf` handle within which we were
    // created; this ensures we won't outlive our enclosing handle.
    _scf: PhantomData<&'scf ()>,
    handle: NonNull<libscf_sys::scf_iter_t>,
}

impl Drop for ScfIterHandle<'_> {
    fn drop(&mut self) {
        unsafe { libscf_sys::scf_iter_destroy(self.handle.as_ptr()) };
    }
}

impl<'a> ScfIterHandle<'a> {
    fn new(scf: &'a Scf<'a>) -> Result<Self, LibscfError> {
        let handle = scf.scf_iter_create()?;
        Ok(Self { _scf: PhantomData, handle })
    }

    fn as_ptr(&self) -> *mut libscf_sys::scf_iter_t {
        self.handle.as_ptr()
    }
}

pub(crate) struct ScfUninitializedIter<'a> {
    handle: ScfIterHandle<'a>,
}

impl<'a> ScfUninitializedIter<'a> {
    pub(crate) fn new(scf: &'a Scf<'a>) -> Result<Self, LibscfError> {
        Ok(Self { handle: ScfIterHandle::new(scf)? })
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
}

pub(crate) struct ScfIter<'a, T> {
    handle: ScfIterHandle<'a>,
    _inner: PhantomData<fn() -> T>,
}

impl<'a, T: ScfIterable> ScfIter<'a, T> {
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
