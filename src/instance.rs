// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::AddPropertyGroup;
use crate::AddPropertyGroupFlags;
use crate::HasPropertyGroups;
use crate::PropertyGroup;
use crate::PropertyGroupEditable;
use crate::PropertyGroups;
use crate::Scf;
use crate::Service;
use crate::Snapshot;
use crate::Snapshots;
use crate::add_property_group::AddPropertyGroupArgs;
use crate::error::AddPropertyGroupError;
use crate::error::ErrorPath;
use crate::error::IterEntity;
use crate::error::IterError;
use crate::error::LibscfError;
use crate::error::LookupEntity;
use crate::error::LookupError;
use crate::error::format_lookup_target;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::InstanceFmri;
use crate::utf8cstring::PropertyGroupFmri;
use crate::utf8cstring::Utf8CString;

#[derive(Debug)]
pub struct Instance<'a> {
    service: &'a Service<'a>,
    fmri: InstanceFmri,
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
                name: name.to_string().into_boxed_str(),
                err,
            }
        })?;

        let fmri = service.instance_fmri(&name);

        let mut handle =
            service.scf().scf_instance_create().map_err(|err| {
                LookupError::HandleCreate {
                    entity: LookupEntity::Instance,
                    target: format_lookup_target(&fmri, None),
                    err,
                }
            })?;

        let result = unsafe {
            service
                .scf_get_instance(name.as_c_str().as_ptr(), handle.as_mut_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { service, fmri, handle })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: LookupEntity::Instance,
                target: format_lookup_target(&fmri, None),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.service.scf()
    }

    pub(crate) unsafe fn scf_get_snapshot(
        &self,
        name: *const i8,
        snapshot: *mut libscf_sys::scf_snapshot_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_get_snapshot(
                self.handle.as_ptr(),
                name,
                snapshot,
            )
        })
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

    pub(crate) unsafe fn scf_get_pg_composed(
        &self,
        snapshot: *const libscf_sys::scf_snapshot_t,
        name: *const i8,
        pg: *mut libscf_sys::scf_propertygroup_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_get_pg_composed(
                self.handle.as_ptr(),
                snapshot,
                name,
                pg,
            )
        })
    }

    pub(crate) unsafe fn scf_iter_pgs_composed(
        &self,
        snapshot: *const libscf_sys::scf_snapshot_t,
    ) -> Result<ScfIter<'_, libscf_sys::scf_propertygroup_t>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::PropertyGroup,
                parent: self.error_path().into_boxed_str(),
                err,
            }
        })?;
        unsafe {
            iter.init_instance_property_groups_composed(
                self.handle.as_ptr(),
                snapshot,
            )
        }
        .map_err(|err| IterError::InitIter {
            entity: IterEntity::PropertyGroup,
            parent: self.error_path().into_boxed_str(),
            err,
        })
    }

    pub fn fmri(&self) -> &str {
        self.fmri.as_str()
    }

    pub(crate) fn fmri_internal(&self) -> &InstanceFmri {
        &self.fmri
    }

    pub(crate) fn property_group_fmri(
        &self,
        name: &Utf8CString,
    ) -> PropertyGroupFmri {
        self.fmri.append_pg(name)
    }

    pub fn snapshot(
        &self,
        name: &str,
    ) -> Result<Option<Snapshot<'_>>, LookupError> {
        Snapshot::new(self, name)
    }

    pub fn snapshots(&self) -> Result<Snapshots<'_>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::Snapshot,
                parent: self.error_path().into_boxed_str(),
                err,
            }
        })?;
        let iter =
            unsafe { iter.init_instance_snapshots(self.handle.as_ptr()) }
                .map_err(|err| IterError::InitIter {
                    entity: IterEntity::Snapshot,
                    parent: self.error_path().into_boxed_str(),
                    err,
                })?;
        Ok(Snapshots::new(self, iter))
    }
}

impl AddPropertyGroup for Instance<'_> {
    fn add_property_group(
        &mut self,
        name: &str,
        pg_type: &str,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupEditable>, AddPropertyGroupError>
    {
        let AddPropertyGroupArgs { name, pg_type, mut handle, flags } =
            AddPropertyGroupArgs::validate(
                self.scf(),
                self,
                name,
                pg_type,
                flags,
            )?;
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_add_pg(
                self.handle.as_mut_ptr(),
                name.as_c_str().as_ptr(),
                pg_type.as_c_str().as_ptr(),
                flags,
                handle.as_mut_ptr(),
            )
        })
        .map_err(|err| AddPropertyGroupError::Add {
            parent: self.error_path().into_boxed_str(),
            name: name.to_string().into_boxed_str(),
            err,
        })?;

        Ok(PropertyGroup::from_instance_add_pg(self, name, handle))
    }
}

impl HasPropertyGroups for Instance<'_> {
    type St = PropertyGroupEditable;

    fn property_group(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, Self::St>>, LookupError> {
        PropertyGroup::from_instance(self, name)
    }

    fn property_groups(
        &self,
    ) -> Result<PropertyGroups<'_, Self::St>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::PropertyGroup,
                parent: self.error_path().into_boxed_str(),
                err,
            }
        })?;
        let iter =
            unsafe { iter.init_instance_property_groups(self.handle.as_ptr()) }
                .map_err(|err| IterError::InitIter {
                    entity: IterEntity::PropertyGroup,
                    parent: self.error_path().into_boxed_str(),
                    err,
                })?;
        Ok(PropertyGroups::from_instance(self, iter))
    }
}

impl ErrorPath for Instance<'_> {
    fn error_path(&self) -> String {
        self.fmri().to_string()
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
                    fmri: self.service.instance_fmri(&name),
                    handle,
                })
            })
    }
}
