// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::PropertyGroup;
use crate::Scf;
use crate::Value;
use crate::Values;
use crate::ValuesError;
use crate::iter::ScfUninitializedIter;
use crate::utf8cstring::Utf8CString;
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
}

#[derive(Debug, thiserror::Error)]
pub enum SingleValueError {
    #[error("property `{description}` has no values")]
    NoValues { description: String },

    #[error("property `{description}` has more than one value")]
    MultipleValues { description: String },

    #[error("error getting single value")]
    ValuesError(#[from] ValuesError),
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
        let name = Utf8CString::from_str(name).map_err(|err| {
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

    pub(crate) fn to_description_for_error(&self) -> String {
        format!(
            "{}/{}",
            self.property_group.to_description_for_error(),
            self.name()
        )
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn values(&self) -> Result<Values<'_, St>, ValuesError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            ValuesError::CreateIter {
                parent: self.to_description_for_error(),
                err,
            }
        })?;
        let iter = unsafe { iter.init_property_values(self.handle.as_ptr()) }
            .map_err(|err| ValuesError::InitIter {
            parent: self.to_description_for_error(),
            err,
        })?;
        Values::new(self, iter)
    }

    pub fn single_value(&self) -> Result<Value, SingleValueError> {
        let mut iter = self.values()?;

        let first_val =
            iter.next().ok_or_else(|| SingleValueError::NoValues {
                description: self.to_description_for_error(),
            })??;

        match iter.next() {
            None => Ok(first_val),
            Some(Ok(_)) => Err(SingleValueError::MultipleValues {
                description: self.to_description_for_error(),
            }),
            Some(Err(err)) => Err(err.into()),
        }
    }
}
