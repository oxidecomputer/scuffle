// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::HasDirectPropertyGroups;
use crate::PropertyGroup;
use crate::PropertyGroupDirect;
use crate::Scf;
use crate::error::AddPropertyGroupError;
use crate::error::DeletePropertyGroupError;
use crate::error::ErrorPath;
use crate::error::LibscfError;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;

/// Flags controlling creation of new property groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive] // leave the door open for libscf to add more flags
pub enum AddPropertyGroupFlags {
    /// Persistent property group.
    ///
    /// This is the default and typical value for most property groups.
    Persistent,

    /// Non-persistent property group.
    ///
    /// Discussion of non-persistent property groups from `man scf_pg_create`:
    ///
    /// > If `NonPersistent` is set, the property group is not included in
    /// > snapshots and will lose its contents upon system shutdown or reboot.
    /// > Non-persistent property groups are mainly used for smf-internal state.
    NonPersistent,
}

/// Result of deleting a property group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeletePropertyGroupResult {
    /// The property group was deleted.
    Deleted,

    /// The property group was not deleted because it already didn't exist.
    DoesNotExist,
}

/// Trait to add property groups to a [`Service`] or [`Instance`].
///
/// [`Instance`]: crate::Instance
/// [`Service`]: crate::Service
pub trait EditPropertyGroups: HasDirectPropertyGroups + ErrorPath {
    fn add_property_group(
        &mut self,
        name: &str,
        pg_type: &str,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupDirect>, AddPropertyGroupError>;

    fn ensure_property_group(
        &mut self,
        name: &str,
        pg_type: &str,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupDirect>, AddPropertyGroupError>
    {
        // This implementation is quite awkward to avoid borrow-checker
        // problems. First, we attempt to unconditionally add the pg as new; if
        // that succeeds, we _discard_ the `PropertyGroup<'_>` handle it
        // returns. If it fails, we check for `LibscfError::Exists`; any other
        // error bails out.
        //
        // If we get past this match, the pg exists: either we just added it, or
        // it already existed. We then do a lookup below. There's a sort of
        // TOCTOU here - someone could delete it between this match and our
        // lookup - but we can at least catch that error and notify the caller
        // explicitly that that's what happened.
        match self.add_property_group(name, pg_type, flags) {
            Ok(_)
            | Err(AddPropertyGroupError::Add {
                err: LibscfError::Exists,
                ..
            }) => (),
            Err(err) => return Err(err),
        }

        self.property_group_direct(name)
            .map_err(|err| AddPropertyGroupError::ExistenceLookup {
                parent: self.error_path(),
                name: Box::from(name),
                err,
            })?
            .ok_or_else(|| AddPropertyGroupError::DeletedDuringEnsure {
                parent: self.error_path(),
                name: Box::from(name),
            })
    }

    fn delete_property_group(
        &mut self,
        name: &str,
    ) -> Result<DeletePropertyGroupResult, DeletePropertyGroupError> {
        let pg = match self.property_group_direct(name) {
            Ok(Some(pg)) => pg,
            Ok(None) => return Ok(DeletePropertyGroupResult::DoesNotExist),
            Err(err) => {
                return Err(DeletePropertyGroupError::Lookup {
                    parent: self.error_path(),
                    name: name.to_string().into_boxed_str(),
                    err,
                });
            }
        };
        pg.delete()
    }
}

pub(crate) struct AddPropertyGroupArgs<'a> {
    pub(crate) name: Utf8CString,
    pub(crate) pg_type: Utf8CString,
    pub(crate) handle: ScfObject<'a, libscf_sys::scf_propertygroup_t>,
    pub(crate) flags: u32,
}

impl<'a> AddPropertyGroupArgs<'a> {
    pub(crate) fn validate<P: ErrorPath>(
        scf: &'a Scf<'_>,
        parent: &P,
        name: &str,
        pg_type: &str,
        flags: AddPropertyGroupFlags,
    ) -> Result<Self, AddPropertyGroupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            AddPropertyGroupError::InvalidName {
                parent: parent.error_path(),
                name: Box::from(name),
                err,
            }
        })?;

        let pg_type = Utf8CString::from_str(pg_type).map_err(|err| {
            AddPropertyGroupError::InvalidType {
                parent: parent.error_path(),
                pg_type: Box::from(pg_type),
                err,
            }
        })?;

        let handle = scf.scf_pg_create()?;

        let flags = match flags {
            AddPropertyGroupFlags::Persistent => 0,
            AddPropertyGroupFlags::NonPersistent => {
                libscf_sys::SCF_PG_FLAG_NONPERSISTENT
            }
        };

        Ok(Self { name, pg_type, handle, flags })
    }
}
