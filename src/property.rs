// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::GetValueError;
use crate::LibscfError;
use crate::PropertyGroup;
use crate::Scf;
use crate::Value;
use crate::iter::ScfIter;
use crate::iter::ScfIterKind;
use crate::utf8cstring::Utf8CString;
use crate::value::ScfValue;
use std::ffi::NulError;
use std::ptr::NonNull;

#[derive(Debug, thiserror::Error)]
pub enum PropertyError {
    #[error("invalid property name {name:?}")]
    InvalidName {
        name: String,
        #[source]
        err: NulError,
    },

    #[error("error creating handle for property `{name}` within `{parent}`")]
    HandleCreate {
        parent: String,
        name: String,
        #[source]
        err: LibscfError,
    },

    #[error("error getting property `{name}` within `{parent}`")]
    Get {
        parent: String,
        name: String,
        #[source]
        err: LibscfError,
    },

    #[error("error creating iterator over `{parent}/{name}`")]
    CreateIter {
        parent: String,
        name: String,
        #[source]
        err: LibscfError,
    },
}

pub struct Property<'a, St> {
    property_group: &'a PropertyGroup<'a, St>,
    name: Utf8CString,
    handle: NonNull<libscf_sys::scf_property_t>,
}

impl<St> Drop for Property<'_, St> {
    fn drop(&mut self) {
        unsafe { libscf_sys::scf_property_destroy(self.handle.as_ptr()) };
    }
}

impl<'a, St> Property<'a, St> {
    pub(crate) fn from_property_group(
        property_group: &'a PropertyGroup<'a, St>,
        name: &str,
    ) -> Result<Option<Self>, PropertyError> {
        let name = Utf8CString::new(name).map_err(|err| {
            PropertyError::InvalidName { name: name.to_string(), err }
        })?;

        let handle =
            property_group.scf().scf_property_create().map_err(|err| {
                PropertyError::HandleCreate {
                    parent: property_group.to_description_for_error(),
                    name: name.to_string(),
                    err,
                }
            })?;
        let prop = Self { property_group, name, handle };

        let result = unsafe {
            property_group.scf_get_property(
                prop.name.as_c_str().as_ptr(),
                prop.handle.as_ptr(),
            )
        };

        match result {
            Ok(()) => Ok(Some(prop)),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(PropertyError::Get {
                parent: property_group.to_description_for_error(),
                name: prop.name.to_string(),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.property_group.scf()
    }

    fn to_description_for_error(&self) -> String {
        format!(
            "{}/{}",
            self.property_group.to_description_for_error(),
            self.name()
        )
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn values(&self) -> Result<Values<'_, St>, PropertyError> {
        let iter = unsafe { ScfIter::new(self.scf(), self.handle.as_ptr()) }
            .map_err(|err| PropertyError::CreateIter {
                parent: self.property_group.to_description_for_error(),
                name: self.name.to_string(),
                err,
            })?;
        Ok(Values { parent: self, iter })
    }
}

enum ScfIterKindPropertyValues {}

impl ScfIterKind for ScfIterKindPropertyValues {
    type Parent = libscf_sys::scf_property_t;
    type Item<'a> = ScfValue<'a>;

    unsafe fn init(
        iter: *mut libscf_sys::scf_iter_t,
        parent: *const Self::Parent,
    ) -> libc::c_int {
        unsafe { libscf_sys::scf_iter_property_values(iter, parent) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ValuesError {
    #[error("error iterating values of `{parent}`")]
    Iterating {
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error converting value while iterating `{parent}`")]
    GetValue {
        parent: String,
        #[source]
        err: GetValueError,
    },
}

pub struct Values<'a, St> {
    parent: &'a Property<'a, St>,
    iter: ScfIter<'a, ScfIterKindPropertyValues>,
}

impl<'a, St> Iterator for Values<'a, St> {
    type Item = Result<Value, ValuesError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next()? {
            Ok(scf_val) => {
                Some(scf_val.get().map_err(|err| ValuesError::GetValue {
                    parent: self.parent.to_description_for_error(),
                    err,
                }))
            }
            Err(err) => Some(Err(ValuesError::Iterating {
                parent: self.parent.to_description_for_error(),
                err,
            })),
        }
    }
}
