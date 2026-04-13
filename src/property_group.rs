// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Properties;
use crate::Property;
use crate::Scf;
use crate::Service;
use crate::buf::scf_get_name;
use crate::error::ErrorPath;
use crate::error::IterEntity;
use crate::error::IterError;
use crate::error::LibscfError;
use crate::error::LookupEntity;
use crate::error::LookupError;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;
use std::marker::PhantomData;

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

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn property(
        &self,
        name: &str,
    ) -> Result<Option<Property<'_, St>>, LookupError> {
        Property::from_property_group(self, name)
    }

    pub fn properties(&self) -> Result<Properties<'_, St>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::Property,
                parent: self.error_path(),
                err,
            }
        })?;
        let iter = unsafe {
            iter.init_property_group_properties(self.handle.as_ptr())
        }
        .map_err(|err| IterError::InitIter {
            entity: IterEntity::Property,
            parent: self.error_path(),
            err,
        })?;
        Ok(Properties::new(self, iter))
    }
}

impl<St> ErrorPath for PropertyGroup<'_, St> {
    fn error_path(&self) -> String {
        format!("{}/:properties/{}", self.parent.error_path(), self.name())
    }
}

// Methods only available on editable property groups.
impl<'a> PropertyGroup<'a, PropertyGroupEditable> {
    pub(crate) fn from_service(
        service: &'a Service<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: LookupEntity::PropertyGroup,
                parent: Some(service.error_path()),
                name: name.to_string(),
                err,
            }
        })?;

        let handle = service.scf().scf_pg_create().map_err(|err| {
            LookupError::HandleCreate {
                entity: LookupEntity::PropertyGroup,
                parent: Some(service.error_path()),
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
            Err(err) => Err(LookupError::Get {
                entity: LookupEntity::PropertyGroup,
                parent: Some(service.error_path()),
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
}

impl ErrorPath for PropertyGroupParent<'_> {
    fn error_path(&self) -> String {
        match self {
            PropertyGroupParent::Service(service) => service.error_path(),
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
    type Item = Result<PropertyGroup<'a, St>, IterError>;

    fn next(&mut self) -> Option<Self::Item> {
        let handle = match self.parent.scf().scf_pg_create() {
            Ok(handle) => handle,
            Err(err) => {
                return Some(Err(IterError::CreateItem {
                    entity: IterEntity::PropertyGroup,
                    parent: self.parent.error_path(),
                    err,
                }));
            }
        };

        // Fill in `handle` with next item from the internal iterator; on
        // success, also get the property group's name.
        let result = unsafe { self.iter.try_next(handle.as_ptr()) }?
            .map_err(|err| IterError::Iterating {
                entity: IterEntity::PropertyGroup,
                parent: self.parent.error_path(),
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
                .map_err(|err| IterError::GetName {
                    entity: IterEntity::PropertyGroup,
                    parent: self.parent.error_path(),
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
