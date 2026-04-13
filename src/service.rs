// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::HasPropertyGroups;
use crate::Instance;
use crate::Instances;
use crate::PropertyGroup;
use crate::PropertyGroupEditable;
use crate::PropertyGroups;
use crate::Scf;
use crate::Scope;
use crate::error::ErrorPath;
use crate::error::IterEntity;
use crate::error::IterError;
use crate::error::LibscfError;
use crate::error::LookupEntity;
use crate::error::LookupError;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;
use std::marker::PhantomData;

pub struct Service<'a> {
    // Lifetime that binds us to the parent `Scope`, ensuring we outlive it.
    _scope: PhantomData<&'a ()>,
    name: Utf8CString,
    handle: ScfObject<'a, libscf_sys::scf_service_t>,
}

impl<'a> Service<'a> {
    pub(crate) fn new(
        scope: &'a Scope<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: LookupEntity::Service,
                parent: None,
                name: name.to_string(),
                err,
            }
        })?;

        let handle = scope.scf().scf_service_create().map_err(|err| {
            LookupError::HandleCreate {
                entity: LookupEntity::Service,
                parent: None,
                name: name.to_string(),
                err,
            }
        })?;

        let result = unsafe {
            scope.scf_get_service(name.as_c_str().as_ptr(), handle.as_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { _scope: PhantomData, handle, name })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: LookupEntity::Service,
                parent: None,
                name: name.into_string(),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.handle.scf()
    }

    pub(crate) unsafe fn scf_get_pg(
        &self,
        name: *const i8,
        pg: *mut libscf_sys::scf_propertygroup_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_service_get_pg(self.handle.as_ptr(), name, pg)
        })
    }

    pub(crate) unsafe fn scf_get_instance(
        &self,
        name: *const i8,
        instance: *mut libscf_sys::scf_instance_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_service_get_instance(
                self.handle.as_ptr(),
                name,
                instance,
            )
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn instance(
        &self,
        name: &str,
    ) -> Result<Option<Instance<'_>>, LookupError> {
        Instance::new(self, name)
    }

    pub fn instances(&self) -> Result<Instances<'_>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::Instance,
                parent: self.error_path(),
                err,
            }
        })?;
        let iter = unsafe { iter.init_service_instances(self.handle.as_ptr()) }
            .map_err(|err| IterError::InitIter {
                entity: IterEntity::Instance,
                parent: self.error_path(),
                err,
            })?;
        Ok(Instances::new(self, iter))
    }
}

impl HasPropertyGroups for Service<'_> {
    type St = PropertyGroupEditable;

    fn property_group(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, Self::St>>, LookupError> {
        PropertyGroup::from_service(self, name)
    }

    fn property_groups(
        &self,
    ) -> Result<PropertyGroups<'_, Self::St>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::PropertyGroup,
                parent: self.error_path(),
                err,
            }
        })?;
        let iter =
            unsafe { iter.init_service_property_groups(self.handle.as_ptr()) }
                .map_err(|err| IterError::InitIter {
                    entity: IterEntity::PropertyGroup,
                    parent: self.error_path(),
                    err,
                })?;
        Ok(PropertyGroups::from_service(self, iter))
    }
}

impl ErrorPath for Service<'_> {
    fn error_path(&self) -> String {
        self.name().to_string()
    }
}
