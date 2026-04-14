// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every object type exposed by `libscf` follows the same pattern:
//!
//! * Create an instance of the object by calling an `scf_*_create()` function
//!   that takes the enclosing [`Scf`]
//! * ... use the object ...
//! * Destroy the object by calling an `scf_*_destroy()` function
//!
//! This module handles creation (by exposing methods on [`Scf`]) and
//! destruction (via the `Drop` impl on [`ScfObject`]). This allows the rest of
//! the crate to operate on objects, including initializing them, without
//! worrying about remembering to destroy the object if there's an error at any
//! point.

use super::Scf;
use crate::error::HandleCreateError;
use crate::error::LibscfError;
use crate::error::ScfEntity;
use std::fmt;
use std::ptr::NonNull;

mod sealed {
    pub(crate) trait Sealed {}
}

pub(crate) trait ScfObjectType: sealed::Sealed {
    unsafe fn create(handle: *mut libscf_sys::scf_handle_t) -> *mut Self;
    unsafe fn destroy(ptr: *mut Self);
}

macro_rules! impl_scf_type {
    ($entity:ident, $type:ident, $create:ident, $destroy:ident) => {
        impl sealed::Sealed for libscf_sys::$type {}

        impl ScfObjectType for libscf_sys::$type {
            unsafe fn create(
                handle: *mut libscf_sys::scf_handle_t,
            ) -> *mut Self {
                unsafe { libscf_sys::$create(handle) }
            }

            unsafe fn destroy(ptr: *mut Self) {
                unsafe { libscf_sys::$destroy(ptr) }
            }
        }

        impl Scf<'_> {
            pub(crate) fn $create(
                &self,
            ) -> Result<ScfObject<'_, libscf_sys::$type>, HandleCreateError>
            {
                match LibscfError::from_ptr(unsafe {
                    <libscf_sys::$type as ScfObjectType>::create(
                        self.handle.as_ptr(),
                    )
                }) {
                    Ok(handle) => Ok(ScfObject { scf: self, handle }),
                    Err(err) => Err(HandleCreateError {
                        entity: ScfEntity::$entity,
                        err,
                    }),
                }
            }
        }
    };
}

impl_scf_type!(
    Instance,
    scf_instance_t,
    scf_instance_create,
    scf_instance_destroy
);
impl_scf_type!(Iter, scf_iter_t, scf_iter_create, scf_iter_destroy);
impl_scf_type!(Scope, scf_scope_t, scf_scope_create, scf_scope_destroy);
impl_scf_type!(Service, scf_service_t, scf_service_create, scf_service_destroy);
impl_scf_type!(
    Snapshot,
    scf_snapshot_t,
    scf_snapshot_create,
    scf_snapshot_destroy
);
impl_scf_type!(
    Property,
    scf_property_t,
    scf_property_create,
    scf_property_destroy
);
impl_scf_type!(
    PropertyGroup,
    scf_propertygroup_t,
    scf_pg_create,
    scf_pg_destroy
);
impl_scf_type!(
    Transaction,
    scf_transaction_t,
    scf_transaction_create,
    scf_transaction_destroy
);
impl_scf_type!(
    TransactionEntry,
    scf_transaction_entry_t,
    scf_entry_create,
    scf_entry_destroy
);
impl_scf_type!(Value, scf_value_t, scf_value_create, scf_value_destroy);

pub(crate) struct ScfObject<'scf, T: ScfObjectType> {
    scf: &'scf Scf<'scf>,
    handle: NonNull<T>,
}

impl<T: ScfObjectType> fmt::Debug for ScfObject<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScfObject")
            .field("scf", &self.scf)
            .field("handle", &self.handle.as_ptr())
            .finish()
    }
}

impl<T: ScfObjectType> Drop for ScfObject<'_, T> {
    fn drop(&mut self) {
        unsafe { <T as ScfObjectType>::destroy(self.handle.as_ptr()) };
    }
}

impl<'a, T: ScfObjectType> ScfObject<'a, T> {
    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.scf
    }

    pub(crate) fn as_ptr(&self) -> *const T {
        self.handle.as_ptr()
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut T {
        self.handle.as_ptr()
    }
}
