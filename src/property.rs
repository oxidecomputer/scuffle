// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::PropertyGroup;
use crate::Scf;
use crate::ScfStringError;
use crate::Value;
use crate::Values;
use crate::ValuesError;
use crate::buf::scf_get_name;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;
use std::ffi::NulError;

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

#[derive(Debug, thiserror::Error)]
pub enum PropertiesError {
    #[error("error creating iterator over `{parent}`")]
    CreateIter {
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error initializing iterator over `{parent}`")]
    InitIter {
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error creating property while iterating over `{parent}`")]
    CreateProperty {
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error iterating properties of `{parent}`")]
    Iterating {
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error getting name of property while iterating over `{parent}`")]
    GetName {
        parent: String,
        #[source]
        err: ScfStringError,
    },
}

pub struct Property<'a, St> {
    property_group: &'a PropertyGroup<'a, St>,
    name: Utf8CString,
    handle: ScfObject<'a, libscf_sys::scf_property_t>,
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

        let result = unsafe {
            property_group
                .scf_get_property(name.as_c_str().as_ptr(), handle.as_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { property_group, name, handle })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(PropertyError::Get {
                parent: property_group.to_description_for_error(),
                name: name.into_string(),
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

pub struct Properties<'a, St> {
    property_group: &'a PropertyGroup<'a, St>,
    iter: ScfIter<'a, libscf_sys::scf_property_t>,
}

impl<'a, St> Properties<'a, St> {
    pub(crate) fn new(
        property_group: &'a PropertyGroup<'a, St>,
        iter: ScfIter<'a, libscf_sys::scf_property_t>,
    ) -> Self {
        Self { property_group, iter }
    }
}

impl<'a, St> Iterator for Properties<'a, St> {
    type Item = Result<Property<'a, St>, PropertiesError>;

    fn next(&mut self) -> Option<Self::Item> {
        let handle = match self.property_group.scf().scf_property_create() {
            Ok(handle) => handle,
            Err(err) => {
                return Some(Err(PropertiesError::CreateProperty {
                    parent: self.property_group.to_description_for_error(),
                    err,
                }));
            }
        };

        // Fill in `handle` with next item from the internal iterator; on
        // success, also get the property's name.
        let result = unsafe { self.iter.try_next(handle.as_ptr()) }?
            .map_err(|err| PropertiesError::Iterating {
                parent: self.property_group.to_description_for_error(),
                err,
            })
            .and_then(|()| {
                // `handle` has been filled in; get its name.
                scf_get_name(|out_buf, out_len| unsafe {
                    libscf_sys::scf_property_get_name(
                        handle.as_ptr(),
                        out_buf,
                        out_len,
                    )
                })
                .map_err(|err| PropertiesError::GetName {
                    parent: self.property_group.to_description_for_error(),
                    err,
                })
            });

        match result {
            Ok(name) => Some(Ok(Property {
                property_group: self.property_group,
                handle,
                name,
            })),
            Err(err) => Some(Err(err)),
        }
    }
}
