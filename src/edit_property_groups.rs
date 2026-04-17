// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::HasDirectPropertyGroups;
use crate::PropertyGroup;
use crate::PropertyGroupDirect;
use crate::PropertyGroupType;
use crate::Scf;
use crate::error::LibscfError;
use crate::error::PropertyGroupAddError;
use crate::error::PropertyGroupDeleteError;
use crate::error::ToEntityDescription;
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
pub trait EditPropertyGroups:
    HasDirectPropertyGroups + ToEntityDescription
{
    /// Add a new property group.
    fn add_property_group(
        &mut self,
        name: &str,
        pg_type: PropertyGroupType,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupDirect>, PropertyGroupAddError>;

    /// Ensure a property group exists.
    ///
    /// If a property group of the given name already exists, it is returned;
    /// its type is _not_ guaranteed to match `pg_type`. If no property group of
    /// the given name exists, it will be created.
    fn ensure_property_group(
        &mut self,
        name: &str,
        pg_type: PropertyGroupType,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupDirect>, PropertyGroupAddError>
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
            | Err(PropertyGroupAddError::Add {
                err: LibscfError::Exists,
                ..
            }) => (),
            Err(err) => return Err(err),
        }

        match self.property_group_direct(name) {
            Ok(Some(property_group)) => Ok(property_group),
            Ok(None) => Err(PropertyGroupAddError::DeletedDuringEnsure {
                parent: self.to_entity_description(),
                name: Box::from(name),
            }),
            Err(err) => Err(PropertyGroupAddError::ExistenceLookup {
                parent: self.to_entity_description(),
                name: Box::from(name),
                err,
            }),
        }
    }

    /// Delete a property group by name.
    fn delete_property_group(
        &mut self,
        name: &str,
    ) -> Result<DeletePropertyGroupResult, PropertyGroupDeleteError> {
        let pg = match self.property_group_direct(name) {
            Ok(Some(pg)) => pg,
            Ok(None) => return Ok(DeletePropertyGroupResult::DoesNotExist),
            Err(err) => {
                return Err(PropertyGroupDeleteError::Lookup {
                    parent: self.to_entity_description(),
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
    pub(crate) handle: ScfObject<'a, libscf_sys::scf_propertygroup_t>,
    pub(crate) flags: u32,
}

impl<'a> AddPropertyGroupArgs<'a> {
    pub(crate) fn validate<P: ToEntityDescription>(
        scf: &'a Scf<'_>,
        parent: &P,
        name: &str,
        flags: AddPropertyGroupFlags,
    ) -> Result<Self, PropertyGroupAddError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            PropertyGroupAddError::InvalidName {
                parent: parent.to_entity_description(),
                name: Box::from(name),
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

        Ok(Self { name, handle, flags })
    }
}
