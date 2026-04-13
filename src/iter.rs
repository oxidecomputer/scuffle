// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Scf;
use crate::buf::scf_get_name;
use crate::error::ErrorPath;
use crate::error::IterEntity;
use crate::error::IterError;
use crate::error::LibscfError;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;
use std::marker::PhantomData;

mod sealed {
    pub(crate) trait ScfIterable: crate::scf::ScfObjectType {
        const ENTITY: crate::error::IterEntity;

        unsafe fn try_next(
            iter: *mut libscf_sys::scf_iter_t,
            uninitialized_item: *mut Self,
        ) -> libc::c_int;
    }

    pub(crate) trait ScfNamedIterable: ScfIterable {
        unsafe fn get_name(
            item: *const Self,
            buf: *mut libc::c_char,
            buf_len: usize,
        ) -> libc::ssize_t;
    }
}

impl sealed::ScfIterable for libscf_sys::scf_value_t {
    const ENTITY: IterEntity = IterEntity::Value;

    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_value(iter, uninitialized_item) }
    }
}

impl sealed::ScfIterable for libscf_sys::scf_propertygroup_t {
    const ENTITY: IterEntity = IterEntity::PropertyGroup;

    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_pg(iter, uninitialized_item) }
    }
}

impl sealed::ScfNamedIterable for libscf_sys::scf_propertygroup_t {
    unsafe fn get_name(
        item: *const Self,
        buf: *mut libc::c_char,
        buf_len: usize,
    ) -> libc::ssize_t {
        unsafe { libscf_sys::scf_pg_get_name(item, buf, buf_len) }
    }
}

impl sealed::ScfIterable for libscf_sys::scf_property_t {
    const ENTITY: IterEntity = IterEntity::Property;

    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_property(iter, uninitialized_item) }
    }
}

impl sealed::ScfNamedIterable for libscf_sys::scf_property_t {
    unsafe fn get_name(
        item: *const Self,
        buf: *mut libc::c_char,
        buf_len: usize,
    ) -> libc::ssize_t {
        unsafe { libscf_sys::scf_property_get_name(item, buf, buf_len) }
    }
}

impl sealed::ScfIterable for libscf_sys::scf_instance_t {
    const ENTITY: IterEntity = IterEntity::Instance;

    unsafe fn try_next(
        iter: *mut libscf_sys::scf_iter_t,
        uninitialized_item: *mut Self,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_next_instance(iter, uninitialized_item) }
    }
}

impl sealed::ScfNamedIterable for libscf_sys::scf_instance_t {
    unsafe fn get_name(
        item: *const Self,
        buf: *mut libc::c_char,
        buf_len: usize,
    ) -> libc::ssize_t {
        unsafe { libscf_sys::scf_instance_get_name(item, buf, buf_len) }
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

    pub(crate) unsafe fn init_service_instances(
        self,
        service: *const libscf_sys::scf_service_t,
    ) -> Result<ScfIter<'a, libscf_sys::scf_instance_t>, LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_iter_service_instances(
                self.handle.as_ptr(),
                service,
            )
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

    pub(crate) unsafe fn init_instance_property_groups(
        self,
        instance: *const libscf_sys::scf_instance_t,
    ) -> Result<ScfIter<'a, libscf_sys::scf_propertygroup_t>, LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_iter_instance_pgs(self.handle.as_ptr(), instance)
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
    pub(crate) fn next_with_handle<P>(
        &mut self,
        parent: &P,
        handle: &ScfObject<'a, T>,
    ) -> Option<Result<(), IterError>>
    where
        P: ErrorPath,
    {
        match unsafe { T::try_next(self.handle.as_ptr(), handle.as_ptr()) } {
            0 => None,
            1 => Some(Ok(())),
            _ => Some(Err(IterError::Iterating {
                entity: T::ENTITY,
                parent: parent.error_path(),
                err: LibscfError::last(),
            })),
        }
    }
}

impl<'a, T: sealed::ScfNamedIterable> ScfIter<'a, T> {
    pub(crate) fn next_named<F, P>(
        &mut self,
        parent: &P,
        make_handle: F,
    ) -> Option<Result<(Utf8CString, ScfObject<'a, T>), IterError>>
    where
        P: ErrorPath,
        F: FnOnce() -> Result<ScfObject<'a, T>, LibscfError>,
    {
        let handle = match make_handle() {
            Ok(handle) => handle,
            Err(err) => {
                return Some(Err(IterError::CreateItem {
                    entity: T::ENTITY,
                    parent: parent.error_path(),
                    err,
                }));
            }
        };

        match self.next_with_handle(parent, &handle)? {
            Ok(()) => (),
            Err(err) => return Some(Err(err)),
        }

        let name = match scf_get_name(|buf, buf_len| unsafe {
            T::get_name(handle.as_ptr(), buf, buf_len)
        }) {
            Ok(name) => name,
            Err(err) => {
                return Some(Err(IterError::GetName {
                    entity: T::ENTITY,
                    parent: parent.error_path(),
                    err,
                }));
            }
        };

        Some(Ok((name, handle)))
    }
}
