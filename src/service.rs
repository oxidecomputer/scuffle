// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::PropertyGroup;
use crate::PropertyGroupEditable;
use crate::PropertyGroupError;
use crate::PropertyGroups;
use crate::PropertyGroupsError;
use crate::Scf;
use crate::Scope;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;
use std::ffi::NulError;
use std::marker::PhantomData;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("invalid service name {name:?}")]
    InvalidName {
        name: String,
        #[source]
        err: NulError,
    },

    #[error("error creating handle for service `{name}`")]
    HandleCreate {
        name: String,
        #[source]
        err: LibscfError,
    },

    #[error("failed getting service `{name}`")]
    GetService {
        name: String,
        #[source]
        err: LibscfError,
    },
}

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
    ) -> Result<Option<Self>, ServiceError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            ServiceError::InvalidName { name: name.to_string(), err }
        })?;

        let handle = scope.scf().scf_service_create().map_err(|err| {
            ServiceError::HandleCreate { name: name.to_string(), err }
        })?;

        let result = unsafe {
            scope.scf_get_service(name.as_c_str().as_ptr(), handle.as_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { _scope: PhantomData, handle, name })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => {
                Err(ServiceError::GetService { name: name.into_string(), err })
            }
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

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn property_group(
        &self,
        name: &str,
    ) -> Result<
        Option<PropertyGroup<'_, PropertyGroupEditable>>,
        PropertyGroupError,
    > {
        PropertyGroup::from_service(self, name)
    }

    pub fn property_groups(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupEditable>, PropertyGroupsError>
    {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            PropertyGroupsError::CreateIter {
                parent: self.name().to_string(),
                err,
            }
        })?;
        let iter =
            unsafe { iter.init_service_property_groups(self.handle.as_ptr()) }
                .map_err(|err| PropertyGroupsError::InitIter {
                    parent: self.name().to_string(),
                    err,
                })?;
        Ok(PropertyGroups::from_service(self, iter))
    }
}
