// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Properties;
use crate::PropertiesError;
use crate::Property;
use crate::PropertyError;
use crate::Scf;
use crate::ScfStringError;
use crate::Service;
use crate::buf::scf_get_name;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;
use std::ffi::NulError;
use std::marker::PhantomData;

#[derive(Debug, thiserror::Error)]
pub enum PropertyGroupError {
    #[error("invalid property group name {name:?}")]
    InvalidName {
        name: String,
        #[source]
        err: NulError,
    },

    #[error(
        "error creating handle for property group `{name}` within `{parent}`"
    )]
    HandleCreate {
        parent: String,
        name: String,
        #[source]
        err: LibscfError,
    },

    #[error("error getting property group `{name}` within `{parent}`")]
    Get {
        parent: String,
        name: String,
        #[source]
        err: LibscfError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PropertyGroupsError {
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

    #[error("error creating property group while iterating over `{parent}`")]
    CreatePropertyGroup {
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error iterating property groups of `{parent}`")]
    Iterating {
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error(
        "error getting name of property group while iterating over `{parent}`"
    )]
    GetName {
        parent: String,
        #[source]
        err: ScfStringError,
    },
}

pub enum PropertyGroupEditable {}
pub enum PropertyGroupSnapshot {}

pub struct PropertyGroup<'a, St> {
    parent: PropertyGroupParent<'a>,
    name: Utf8CString,
    handle: ScfObject<'a, libscf_sys::scf_propertygroup_t>,
    _state: PhantomData<fn() -> St>,
}

// Methods available on all property groups.
impl<'a, St> PropertyGroup<'a, St> {
    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.handle.scf()
    }

    pub(crate) unsafe fn scf_get_property(
        &self,
        name: *const i8,
        property: *mut libscf_sys::scf_property_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_pg_get_property(
                self.handle.as_ptr(),
                name,
                property,
            )
        })
    }

    pub(crate) fn to_description_for_error(&self) -> String {
        match self.parent {
            PropertyGroupParent::Service(service) => {
                format!("{}/:properties/{}", service.name(), self.name())
            }
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn property(
        &self,
        name: &str,
    ) -> Result<Option<Property<'_, St>>, PropertyError> {
        Property::from_property_group(self, name)
    }

    pub fn properties(&self) -> Result<Properties<'_, St>, PropertiesError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            PropertiesError::CreateIter {
                parent: self.to_description_for_error(),
                err,
            }
        })?;
        let iter = unsafe {
            iter.init_property_group_properties(self.handle.as_ptr())
        }
        .map_err(|err| PropertiesError::InitIter {
            parent: self.to_description_for_error(),
            err,
        })?;
        Ok(Properties::new(self, iter))
    }
}

// Methods only available on editable property groups.
impl<'a> PropertyGroup<'a, PropertyGroupEditable> {
    pub(crate) fn from_service(
        service: &'a Service<'a>,
        name: &str,
    ) -> Result<Option<Self>, PropertyGroupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            PropertyGroupError::InvalidName { name: name.to_string(), err }
        })?;

        let handle = service.scf().scf_pg_create().map_err(|err| {
            PropertyGroupError::HandleCreate {
                parent: service.name().to_string(),
                name: name.to_string(),
                err,
            }
        })?;

        let result = unsafe {
            service.scf_get_pg(name.as_c_str().as_ptr(), handle.as_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self {
                parent: PropertyGroupParent::Service(service),
                name,
                handle,
                _state: PhantomData,
            })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(PropertyGroupError::Get {
                parent: service.name().to_string(),
                name: name.into_string(),
                err,
            }),
        }
    }
}

#[derive(Clone, Copy)]
enum PropertyGroupParent<'a> {
    Service(&'a Service<'a>),
}

impl<'a> PropertyGroupParent<'a> {
    fn scf(&self) -> &'a Scf<'a> {
        match self {
            PropertyGroupParent::Service(service) => service.scf(),
        }
    }

    fn to_description_for_error(self) -> String {
        match self {
            PropertyGroupParent::Service(service) => service.name().to_string(),
        }
    }
}

pub struct PropertyGroups<'a, St> {
    parent: PropertyGroupParent<'a>,
    iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    _state: PhantomData<fn() -> St>,
}

impl<'a, St> PropertyGroups<'a, St> {
    pub(crate) fn from_service(
        service: &'a Service<'a>,
        iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    ) -> Self {
        Self {
            parent: PropertyGroupParent::Service(service),
            iter,
            _state: PhantomData,
        }
    }
}

impl<'a, St> Iterator for PropertyGroups<'a, St> {
    type Item = Result<PropertyGroup<'a, St>, PropertyGroupsError>;

    fn next(&mut self) -> Option<Self::Item> {
        let handle = match self.parent.scf().scf_pg_create() {
            Ok(handle) => handle,
            Err(err) => {
                return Some(Err(PropertyGroupsError::CreatePropertyGroup {
                    parent: self.parent.to_description_for_error(),
                    err,
                }));
            }
        };

        // Fill in `handle` with next item from the internal iterator; on
        // success, also get the property group's name.
        let result = unsafe { self.iter.try_next(handle.as_ptr()) }?
            .map_err(|err| PropertyGroupsError::Iterating {
                parent: self.parent.to_description_for_error(),
                err,
            })
            .and_then(|()| {
                // `handle` has been filled in; get its name.
                scf_get_name(|out_buf, out_len| unsafe {
                    libscf_sys::scf_pg_get_name(
                        handle.as_ptr(),
                        out_buf,
                        out_len,
                    )
                })
                .map_err(|err| PropertyGroupsError::GetName {
                    parent: self.parent.to_description_for_error(),
                    err,
                })
            });

        match result {
            Ok(name) => Some(Ok(PropertyGroup {
                parent: self.parent,
                handle,
                name,
                _state: PhantomData,
            })),
            Err(err) => Some(Err(err)),
        }
    }
}
