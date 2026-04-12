// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Property;
use crate::PropertyError;
use crate::Scf;
use crate::Service;
use crate::utf8cstring::Utf8CString;
use std::ffi::NulError;
use std::marker::PhantomData;
use std::ptr::NonNull;

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

pub enum PropertyGroupEditable {}
pub enum PropertyGroupSnapshot {}

pub struct PropertyGroup<'a, St> {
    parent: PropertyGroupParent<'a>,
    name: Utf8CString,
    handle: NonNull<libscf_sys::scf_propertygroup_t>,
    _state: PhantomData<fn() -> St>,
}

impl<St> Drop for PropertyGroup<'_, St> {
    fn drop(&mut self) {
        unsafe { libscf_sys::scf_pg_destroy(self.handle.as_ptr()) };
    }
}

// Methods available on all property groups.
impl<'a, St> PropertyGroup<'a, St> {
    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        match &self.parent {
            PropertyGroupParent::Service(service) => service.scf(),
        }
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
        match &self.parent {
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
}

// Methods only available on editable property groups.
impl<'a> PropertyGroup<'a, PropertyGroupEditable> {
    pub(crate) fn from_service(
        service: &'a Service<'a>,
        name: &str,
    ) -> Result<Option<Self>, PropertyGroupError> {
        let name = Utf8CString::new(name).map_err(|err| {
            PropertyGroupError::InvalidName { name: name.to_string(), err }
        })?;

        let handle =
            service.scf().scf_property_group_create().map_err(|err| {
                PropertyGroupError::HandleCreate {
                    parent: service.name().to_string(),
                    name: name.to_string(),
                    err,
                }
            })?;

        let pg = Self {
            parent: PropertyGroupParent::Service(service),
            name,
            handle,
            _state: PhantomData,
        };

        let result = unsafe {
            service.scf_get_pg(pg.name.as_c_str().as_ptr(), pg.handle.as_ptr())
        };

        match result {
            Ok(()) => Ok(Some(pg)),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(PropertyGroupError::Get {
                parent: service.name().to_string(),
                name: pg.name.to_string(),
                err,
            }),
        }
    }
}

enum PropertyGroupParent<'a> {
    Service(&'a Service<'a>),
}
