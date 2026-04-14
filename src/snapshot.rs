// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::HasPropertyGroups;
use crate::Instance;
use crate::PropertyGroup;
use crate::PropertyGroupSnapshot;
use crate::PropertyGroups;
use crate::Scf;
use crate::error::ErrorPath;
use crate::error::IterError;
use crate::error::LibscfError;
use crate::error::LookupEntity;
use crate::error::LookupError;
use crate::error::format_lookup_target;
use crate::iter::ScfIter;
use crate::scf::ScfObject;
use crate::utf8cstring::PropertyGroupFmri;
use crate::utf8cstring::Utf8CString;

#[derive(Debug)]
pub struct Snapshot<'a> {
    instance: &'a Instance<'a>,
    name: Utf8CString,
    handle: ScfObject<'a, libscf_sys::scf_snapshot_t>,
}

impl<'a> Snapshot<'a> {
    pub(crate) fn new(
        instance: &'a Instance<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: LookupEntity::Snapshot,
                name: name.to_string().into_boxed_str(),
                err,
            }
        })?;

        let mut handle =
            instance.scf().scf_snapshot_create().map_err(|err| {
                LookupError::HandleCreate {
                    entity: LookupEntity::Snapshot,
                    target: format_lookup_target(
                        instance.fmri_internal(),
                        None,
                    ),
                    err,
                }
            })?;

        let result = unsafe {
            instance
                .scf_get_snapshot(name.as_c_str().as_ptr(), handle.as_mut_ptr())
        };

        // Construct the snapshot so we can either return it (on success) or
        // pass it to `format_lookup_target()` (on failure).
        let snap = Self { instance, name, handle };

        match result {
            Ok(()) => Ok(Some(snap)),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: LookupEntity::Snapshot,
                target: format_lookup_target(
                    instance.fmri_internal(),
                    Some(&snap),
                ),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.instance.scf()
    }

    pub(crate) unsafe fn scf_get_pg(
        &self,
        name: *const i8,
        pg: *mut libscf_sys::scf_propertygroup_t,
    ) -> Result<(), LibscfError> {
        unsafe {
            self.instance.scf_get_pg_composed(self.handle.as_ptr(), name, pg)
        }
    }

    pub fn instance_fmri(&self) -> &str {
        self.instance.fmri()
    }

    pub(crate) fn property_group_fmri(
        &self,
        name: &Utf8CString,
    ) -> PropertyGroupFmri {
        self.instance.property_group_fmri(name)
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}

impl HasPropertyGroups for Snapshot<'_> {
    type St = PropertyGroupSnapshot;

    fn property_group(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, Self::St>>, LookupError> {
        PropertyGroup::from_snapshot(self, name)
    }

    fn property_groups(
        &self,
    ) -> Result<PropertyGroups<'_, Self::St>, IterError> {
        let iter = unsafe {
            self.instance.scf_iter_pgs_composed(self.handle.as_ptr())
        }?;
        Ok(PropertyGroups::from_snapshot(self, iter))
    }
}

impl ErrorPath for Snapshot<'_> {
    fn error_path(&self) -> String {
        // There is no syntax for including a snapshot in an FMRI.
        format!("{} ({} snapshot)", self.instance_fmri(), self.name())
    }
}

pub struct Snapshots<'a> {
    instance: &'a Instance<'a>,
    iter: ScfIter<'a, libscf_sys::scf_snapshot_t>,
}

impl<'a> Snapshots<'a> {
    pub(crate) fn new(
        instance: &'a Instance<'a>,
        iter: ScfIter<'a, libscf_sys::scf_snapshot_t>,
    ) -> Self {
        Self { instance, iter }
    }
}

impl<'a> Iterator for Snapshots<'a> {
    type Item = Result<Snapshot<'a>, IterError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next_named(self.instance, || {
                self.instance.scf().scf_snapshot_create()
            })
            .map(|result| {
                result.map(|(name, handle)| Snapshot {
                    instance: self.instance,
                    name,
                    handle,
                })
            })
    }
}
