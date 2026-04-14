// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::HasPropertyGroups;
use crate::PropertyGroup;
use crate::PropertyGroupEditable;
use crate::Scf;
use crate::error::AddPropertyGroupError;
use crate::error::ErrorPath;
use crate::error::LibscfError;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum AddPropertyGroupFlags {
    Persistent,
    NonPersistent,
}

pub trait AddPropertyGroup:
    HasPropertyGroups<St = PropertyGroupEditable> + ErrorPath
{
    fn add_property_group(
        &mut self,
        name: &str,
        pg_type: &str,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupEditable>, AddPropertyGroupError>;

    fn ensure_property_group(
        &mut self,
        name: &str,
        pg_type: &str,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupEditable>, AddPropertyGroupError>
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

        self.property_group(name)
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

        let handle = scf.scf_pg_create().map_err(|err| {
            AddPropertyGroupError::HandleCreate {
                parent: parent.error_path(),
                err,
            }
        })?;

        let flags = match flags {
            AddPropertyGroupFlags::Persistent => 0,
            AddPropertyGroupFlags::NonPersistent => {
                libscf_sys::SCF_PG_FLAG_NONPERSISTENT
            }
        };

        Ok(Self { name, pg_type, handle, flags })
    }
}
