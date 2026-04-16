// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::AddPropertyGroupFlags;
use crate::EditPropertyGroups;
use crate::HasDirectPropertyGroups;
use crate::Instance;
use crate::Instances;
use crate::PropertyGroup;
use crate::PropertyGroupDirect;
use crate::PropertyGroupType;
use crate::PropertyGroups;
use crate::Scf;
use crate::Scope;
use crate::edit_property_groups::AddPropertyGroupArgs;
use crate::error::AddPropertyGroupError;
use crate::error::ErrorPath;
use crate::error::IterError;
use crate::error::IterErrorKind;
use crate::error::LibscfError;
use crate::error::LookupError;
use crate::error::ScfEntity;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::InstanceFmri;
use crate::utf8cstring::PropertyGroupFmri;
use crate::utf8cstring::ServiceFmri;
use crate::utf8cstring::Utf8CString;
use std::marker::PhantomData;

#[derive(Debug)]
pub struct Service<'a> {
    // Lifetime that binds us to the parent `Scope`, ensuring we outlive it.
    _scope: PhantomData<&'a ()>,
    name: Utf8CString,
    fmri: ServiceFmri,
    handle: ScfObject<'a, libscf_sys::scf_service_t>,
}

impl<'a> Service<'a> {
    pub(crate) fn new(
        scope: &'a Scope<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: ScfEntity::Service,
                name: name.to_string().into_boxed_str(),
                err,
            }
        })?;

        let fmri = ServiceFmri::new(&name);

        let mut handle = scope.scf().scf_service_create()?;

        let result = unsafe {
            scope.scf_get_service(name.as_c_str().as_ptr(), handle.as_mut_ptr())
        };

        match result {
            Ok(()) => {
                Ok(Some(Self { _scope: PhantomData, name, handle, fmri }))
            }
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: ScfEntity::Service,
                parent: "scope".into(),
                name: name.into_string().into_boxed_str(),
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

    pub fn fmri(&self) -> &str {
        self.fmri.as_str()
    }

    pub fn instance(
        &self,
        name: &str,
    ) -> Result<Option<Instance<'_>>, LookupError> {
        Instance::from_service(self, name)
    }

    pub(crate) fn instance_fmri(&self, name: &Utf8CString) -> InstanceFmri {
        self.fmri.append_instance(name)
    }

    pub(crate) fn property_group_fmri(
        &self,
        name: &Utf8CString,
    ) -> PropertyGroupFmri {
        self.fmri.append_pg(name)
    }

    pub fn instances(&self) -> Result<Instances<'_>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf())?;
        let iter = unsafe { iter.init_service_instances(self.handle.as_ptr()) }
            .map_err(|err| IterError::Iter {
                entity: ScfEntity::Instance,
                parent: self.error_path(),
                kind: IterErrorKind::Init(err),
            })?;
        Ok(Instances::new(self, iter))
    }
}

impl EditPropertyGroups for Service<'_> {
    fn add_property_group(
        &mut self,
        name: &str,
        pg_type: PropertyGroupType,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupDirect>, AddPropertyGroupError>
    {
        let AddPropertyGroupArgs { name, mut handle, flags } =
            AddPropertyGroupArgs::validate(self.scf(), self, name, flags)?;
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_service_add_pg(
                self.handle.as_mut_ptr(),
                name.as_c_str().as_ptr(),
                pg_type.as_c_str().as_ptr(),
                flags,
                handle.as_mut_ptr(),
            )
        })
        .map_err(|err| AddPropertyGroupError::Add {
            parent: self.error_path(),
            name: name.to_string().into_boxed_str(),
            err,
        })?;

        Ok(PropertyGroup::from_service_add_pg(self, name, handle))
    }
}

impl HasDirectPropertyGroups for Service<'_> {
    fn property_group_direct(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupDirect>>, LookupError>
    {
        PropertyGroup::from_service(self, name)
    }

    fn property_groups_direct(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupDirect>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf())?;
        let iter =
            unsafe { iter.init_service_property_groups(self.handle.as_ptr()) }
                .map_err(|err| IterError::Iter {
                    entity: ScfEntity::PropertyGroup,
                    parent: self.error_path(),
                    kind: IterErrorKind::Init(err),
                })?;
        Ok(PropertyGroups::from_service(self, iter))
    }
}

impl ErrorPath for Service<'_> {
    fn error_path(&self) -> Box<str> {
        self.fmri().to_string().into_boxed_str()
    }
}
