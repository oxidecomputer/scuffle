// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::PropertyGroup;
use crate::Scf;
use crate::Value;
use crate::Values;
use crate::error::ErrorPath;
use crate::error::IterError;
use crate::error::IterErrorKind;
use crate::error::LibscfError;
use crate::error::LookupError;
use crate::error::ScfEntity;
use crate::error::SingleValueError;
use crate::error::format_lookup_target;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::PropertyFmri;
use crate::utf8cstring::Utf8CString;

pub struct Property<'a, St> {
    property_group: &'a PropertyGroup<'a, St>,
    name: Utf8CString,
    fmri: PropertyFmri,
    handle: ScfObject<'a, libscf_sys::scf_property_t>,
}

impl<'a, St> Property<'a, St> {
    pub(crate) fn from_property_group(
        property_group: &'a PropertyGroup<'a, St>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: ScfEntity::Property,
                name: name.to_string().into_boxed_str(),
                err,
            }
        })?;

        let fmri = property_group.property_fmri(&name);

        let mut handle = property_group.scf().scf_property_create()?;

        let result = unsafe {
            property_group
                .scf_get_property(name.as_c_str().as_ptr(), handle.as_mut_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { property_group, name, fmri, handle })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: ScfEntity::Property,
                target: format_lookup_target(&fmri, property_group.snapshot()),
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

    pub fn fmri(&self) -> &str {
        self.fmri.as_str()
    }

    pub fn values(&self) -> Result<Values<'_, St>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf())?;
        let iter = unsafe { iter.init_property_values(self.handle.as_ptr()) }
            .map_err(|err| IterError::Iter {
            entity: ScfEntity::Value,
            parent: self.error_path(),
            kind: IterErrorKind::Init(err),
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
    fn error_path(&self) -> Box<str> {
        if let Some(snapshot) = self.property_group.snapshot() {
            format!("{} ({} snapshot)", self.fmri(), snapshot.name())
                .into_boxed_str()
        } else {
            self.fmri().to_string().into_boxed_str()
        }
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
                    fmri: self.property_group.property_fmri(&name),
                    name,
                    handle,
                })
            })
    }
}
