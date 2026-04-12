// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Scf;
use std::marker::PhantomData;
use std::ptr::NonNull;

pub(crate) trait FromScfIter<'a>: Sized {
    fn create_uninitialized(scf: &'a Scf<'a>) -> Result<Self, LibscfError>;

    unsafe fn try_init_from_iter(
        &self,
        iter: *mut libscf_sys::scf_iter_t,
    ) -> libc::c_int;
}

pub(crate) trait ScfIterKind {
    type Parent;
    type Item<'a>: FromScfIter<'a>;

    unsafe fn init(
        iter: *mut libscf_sys::scf_iter_t,
        parent: *const Self::Parent,
    ) -> libc::c_int;
}

pub(crate) struct ScfIter<'a, T> {
    scf: &'a Scf<'a>,
    handle: NonNull<libscf_sys::scf_iter_t>,
    _inner: PhantomData<fn() -> T>,
}

impl<T> Drop for ScfIter<'_, T> {
    fn drop(&mut self) {
        unsafe { libscf_sys::scf_iter_destroy(self.handle.as_ptr()) };
    }
}

impl<'a, T: ScfIterKind> ScfIter<'a, T> {
    pub(crate) unsafe fn new(
        scf: &'a Scf<'a>,
        parent: *const T::Parent,
    ) -> Result<Self, LibscfError> {
        let handle = scf.scf_iter_create()?;
        let iter = Self { scf, handle, _inner: PhantomData };

        LibscfError::from_ret(unsafe {
            T::init(iter.handle.as_ptr(), parent)
        })?;

        Ok(iter)
    }

    pub(crate) fn next(&mut self) -> Option<Result<T::Item<'a>, LibscfError>> {
        let item = match T::Item::create_uninitialized(self.scf) {
            Ok(item) => item,
            Err(err) => return Some(Err(err)),
        };
        let ret = unsafe { item.try_init_from_iter(self.handle.as_ptr()) };

        match ret {
            0 => None,
            1 => Some(Ok(item)),
            _ => Some(Err(LibscfError::last())),
        }
    }
}
