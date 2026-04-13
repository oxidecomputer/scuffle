// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::PropertyGroup;
use crate::Scf;
use crate::Value;
use crate::Values;
use crate::error::ErrorPath;
use crate::error::IterEntity;
use crate::error::IterError;
use crate::error::LibscfError;
use crate::error::LookupEntity;
use crate::error::LookupError;
use crate::error::SingleValueError;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;

pub struct Property<'a, St> {
    property_group: &'a PropertyGroup<'a, St>,
    name: Utf8CString,
    handle: ScfObject<'a, libscf_sys::scf_property_t>,
}

impl<'a, St> Property<'a, St> {
    pub(crate) fn from_property_group(
        property_group: &'a PropertyGroup<'a, St>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: LookupEntity::Property,
                parent: Some(property_group.error_path()),
                name: name.to_string(),
                err,
            }
        })?;

        let mut handle =
            property_group.scf().scf_property_create().map_err(|err| {
                LookupError::HandleCreate {
                    entity: LookupEntity::Property,
                    parent: Some(property_group.error_path()),
                    name: name.to_string(),
                    err,
                }
            })?;

        let result = unsafe {
            property_group
                .scf_get_property(name.as_c_str().as_ptr(), handle.as_mut_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { property_group, name, handle })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: LookupEntity::Property,
                parent: Some(property_group.error_path()),
                name: name.into_string(),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.property_group.scf()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn values(&self) -> Result<Values<'_, St>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::Value,
                parent: self.error_path(),
                err,
            }
        })?;
        let iter = unsafe { iter.init_property_values(self.handle.as_ptr()) }
            .map_err(|err| IterError::InitIter {
            entity: IterEntity::Value,
            parent: self.error_path(),
            err,
        })?;
        Values::new(self, iter)
    }

    pub fn single_value(&self) -> Result<Value, SingleValueError> {
        let mut iter = self.values()?;

        let first_val = iter.next().ok_or_else(|| {
            SingleValueError::NoValues { description: self.error_path() }
        })??;

        match iter.next() {
            None => Ok(first_val),
            Some(Ok(_)) => Err(SingleValueError::MultipleValues {
                description: self.error_path(),
            }),
            Some(Err(err)) => Err(err.into()),
        }
    }
}

impl<St> ErrorPath for Property<'_, St> {
    fn error_path(&self) -> String {
        format!("{}/{}", self.property_group.error_path(), self.name())
    }
}

pub struct Properties<'a, St> {
    property_group: &'a PropertyGroup<'a, St>,
    iter: ScfIter<'a, libscf_sys::scf_property_t>,
}

impl<'a, St> Properties<'a, St> {
    pub(crate) fn new(
        property_group: &'a PropertyGroup<'a, St>,
        iter: ScfIter<'a, libscf_sys::scf_property_t>,
    ) -> Self {
        Self { property_group, iter }
    }
}

impl<'a, St> Iterator for Properties<'a, St> {
    type Item = Result<Property<'a, St>, IterError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next_named(self.property_group, || {
                self.property_group.scf().scf_property_create()
            })
            .map(|result| {
                result.map(|(name, handle)| Property {
                    property_group: self.property_group,
                    name,
                    handle,
                })
            })
    }
}
