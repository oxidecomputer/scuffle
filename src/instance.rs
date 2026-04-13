// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::PropertyGroup;
use crate::PropertyGroupEditable;
use crate::PropertyGroups;
use crate::Scf;
use crate::Service;
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

pub struct Instance<'a> {
    service: &'a Service<'a>,
    name: Utf8CString,
    handle: ScfObject<'a, libscf_sys::scf_instance_t>,
}

impl<'a> Instance<'a> {
    pub(crate) fn new(
        service: &'a Service<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: LookupEntity::Instance,
                parent: Some(service.error_path()),
                name: name.to_string(),
                err,
            }
        })?;

        let handle = service.scf().scf_instance_create().map_err(|err| {
            LookupError::HandleCreate {
                entity: LookupEntity::Instance,
                parent: Some(service.error_path()),
                name: name.to_string(),
                err,
            }
        })?;

        let result = unsafe {
            service.scf_get_instance(name.as_c_str().as_ptr(), handle.as_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { service, name, handle })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: LookupEntity::Instance,
                parent: Some(service.error_path()),
                name: name.into_string(),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.service.scf()
    }

    pub(crate) unsafe fn scf_get_pg(
        &self,
        name: *const i8,
        pg: *mut libscf_sys::scf_propertygroup_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_get_pg(self.handle.as_ptr(), name, pg)
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn property_group(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupEditable>>, LookupError>
    {
        PropertyGroup::from_instance(self, name)
    }

    pub fn property_groups(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupEditable>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::PropertyGroup,
                parent: self.error_path(),
                err,
            }
        })?;
        let iter =
            unsafe { iter.init_instance_property_groups(self.handle.as_ptr()) }
                .map_err(|err| IterError::InitIter {
                    entity: IterEntity::PropertyGroup,
                    parent: self.error_path(),
                    err,
                })?;
        Ok(PropertyGroups::from_instance(self, iter))
    }
}

impl ErrorPath for Instance<'_> {
    fn error_path(&self) -> String {
        format!("{}:{}", self.service.error_path(), self.name())
    }
}

pub struct Instances<'a> {
    service: &'a Service<'a>,
    iter: ScfIter<'a, libscf_sys::scf_instance_t>,
}

impl<'a> Instances<'a> {
    pub(crate) fn new(
        service: &'a Service<'a>,
        iter: ScfIter<'a, libscf_sys::scf_instance_t>,
    ) -> Self {
        Self { service, iter }
    }
}

impl<'a> Iterator for Instances<'a> {
    type Item = Result<Instance<'a>, IterError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next_named(self.service, || {
                self.service.scf().scf_instance_create()
            })
            .map(|result| {
                result.map(|(name, handle)| Instance {
                    service: self.service,
                    name,
                    handle,
                })
            })
    }
}
