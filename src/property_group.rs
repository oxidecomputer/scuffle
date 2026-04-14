// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Instance;
use crate::Properties;
use crate::Property;
use crate::Scf;
use crate::Service;
use crate::Snapshot;
use crate::Transaction;
use crate::TransactionReset;
use crate::error::ErrorPath;
use crate::error::IterEntity;
use crate::error::IterError;
use crate::error::LibscfError;
use crate::error::LookupEntity;
use crate::error::LookupError;
use crate::error::TransactionError;
use crate::error::format_lookup_target;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::PropertyFmri;
use crate::utf8cstring::PropertyGroupFmri;
use crate::utf8cstring::Utf8CString;
use std::marker::PhantomData;

#[derive(Debug)]
pub enum PropertyGroupEditable {}
#[derive(Debug)]
pub enum PropertyGroupSnapshot {}

#[derive(Debug)]
pub struct PropertyGroup<'a, St> {
    parent: PropertyGroupParent<'a>,
    name: Utf8CString,
    fmri: PropertyGroupFmri,
    handle: ScfObject<'a, libscf_sys::scf_propertygroup_t>,
    _state: PhantomData<fn() -> St>,
}

// Methods available on all property groups.
impl<'a, St> PropertyGroup<'a, St> {
    fn from_parent(
        parent: PropertyGroupParent<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: LookupEntity::PropertyGroup,
                name: name.to_string().into_boxed_str(),
                err,
            }
        })?;

        let fmri = parent.property_group_fmri(&name);

        let mut handle = parent.scf().scf_pg_create().map_err(|err| {
            LookupError::HandleCreate {
                entity: LookupEntity::PropertyGroup,
                target: format_lookup_target(&fmri, parent.snapshot()),
                err,
            }
        })?;

        let result = unsafe {
            parent.scf_get_pg(name.as_c_str().as_ptr(), handle.as_mut_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self {
                parent,
                name,
                fmri,
                handle,
                _state: PhantomData,
            })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: LookupEntity::PropertyGroup,
                target: format_lookup_target(&fmri, parent.snapshot()),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.handle.scf()
    }

    pub(crate) fn snapshot(&self) -> Option<&'a Snapshot<'a>> {
        self.parent.snapshot()
    }

    pub(crate) unsafe fn scf_get_property(
        &self,
        name: *const i8,
        property: *mut libscf_sys::scf_property_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_pg_get_property(
                self.handle.as_ptr(),
                name,
                property,
            )
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn fmri(&self) -> &str {
        self.fmri.as_str()
    }

    pub(crate) fn property_fmri(&self, name: &Utf8CString) -> PropertyFmri {
        self.fmri.append_property(name)
    }

    pub fn property(
        &self,
        name: &str,
    ) -> Result<Option<Property<'_, St>>, LookupError> {
        Property::from_property_group(self, name)
    }

    pub fn properties(&self) -> Result<Properties<'_, St>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf()).map_err(|err| {
            IterError::CreateIter {
                entity: IterEntity::Property,
                parent: self.error_path(),
                err,
            }
        })?;
        let iter = unsafe {
            iter.init_property_group_properties(self.handle.as_ptr())
        }
        .map_err(|err| IterError::InitIter {
            entity: IterEntity::Property,
            parent: self.error_path(),
            err,
        })?;
        Ok(Properties::new(self, iter))
    }
}

impl<St> ErrorPath for PropertyGroup<'_, St> {
    fn error_path(&self) -> Box<str> {
        if let Some(snapshot) = self.snapshot() {
            format!("{} ({} snapshot)", self.fmri(), snapshot.name())
                .into_boxed_str()
        } else {
            self.fmri().to_string().into_boxed_str()
        }
    }
}

// Methods only available on editable property groups.
impl<'a> PropertyGroup<'a, PropertyGroupEditable> {
    pub(crate) fn from_service(
        service: &'a Service<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        Self::from_parent(PropertyGroupParent::Service(service), name)
    }

    pub(crate) fn from_service_add_pg(
        service: &'a Service<'a>,
        name: Utf8CString,
        handle: ScfObject<'a, libscf_sys::scf_propertygroup_t>,
    ) -> Self {
        Self {
            parent: PropertyGroupParent::Service(service),
            fmri: service.property_group_fmri(&name),
            name,
            handle,
            _state: PhantomData,
        }
    }

    pub(crate) fn from_instance(
        instance: &'a Instance<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        Self::from_parent(PropertyGroupParent::Instance(instance), name)
    }

    pub(crate) fn from_instance_add_pg(
        instance: &'a Instance<'a>,
        name: Utf8CString,
        handle: ScfObject<'a, libscf_sys::scf_propertygroup_t>,
    ) -> Self {
        Self {
            parent: PropertyGroupParent::Instance(instance),
            fmri: instance.property_group_fmri(&name),
            name,
            handle,
            _state: PhantomData,
        }
    }

    pub(crate) unsafe fn scf_transaction_start(
        &mut self,
        tx: *mut libscf_sys::scf_transaction_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_transaction_start(tx, self.handle.as_mut_ptr())
        })
    }

    pub fn transaction(
        &mut self,
    ) -> Result<Transaction<'_, 'a, TransactionReset>, TransactionError> {
        Transaction::new(self)
    }
}

// Methods only available on snapshot property groups.
impl<'a> PropertyGroup<'a, PropertyGroupSnapshot> {
    pub(crate) fn from_snapshot(
        snapshot: &'a Snapshot<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        Self::from_parent(PropertyGroupParent::Snapshot(snapshot), name)
    }
}

#[derive(Debug, Clone, Copy)]
enum PropertyGroupParent<'a> {
    Service(&'a Service<'a>),
    Instance(&'a Instance<'a>),
    Snapshot(&'a Snapshot<'a>),
}

impl<'a> PropertyGroupParent<'a> {
    fn scf(&self) -> &'a Scf<'a> {
        match self {
            Self::Service(service) => service.scf(),
            Self::Instance(instance) => instance.scf(),
            Self::Snapshot(snapshot) => snapshot.scf(),
        }
    }

    fn property_group_fmri(&self, name: &Utf8CString) -> PropertyGroupFmri {
        match self {
            PropertyGroupParent::Service(service) => {
                service.property_group_fmri(name)
            }
            PropertyGroupParent::Instance(instance) => {
                instance.property_group_fmri(name)
            }
            PropertyGroupParent::Snapshot(snapshot) => {
                snapshot.property_group_fmri(name)
            }
        }
    }

    fn snapshot(&self) -> Option<&'a Snapshot<'a>> {
        match self {
            PropertyGroupParent::Service(_)
            | PropertyGroupParent::Instance(_) => None,
            PropertyGroupParent::Snapshot(snapshot) => Some(snapshot),
        }
    }

    unsafe fn scf_get_pg(
        &self,
        name: *const libc::c_char,
        pg: *mut libscf_sys::scf_propertygroup_t,
    ) -> Result<(), LibscfError> {
        match self {
            Self::Service(service) => unsafe { service.scf_get_pg(name, pg) },
            Self::Instance(instance) => unsafe {
                instance.scf_get_pg(name, pg)
            },
            Self::Snapshot(snapshot) => unsafe {
                snapshot.scf_get_pg(name, pg)
            },
        }
    }
}

impl ErrorPath for PropertyGroupParent<'_> {
    fn error_path(&self) -> Box<str> {
        match self {
            Self::Service(service) => service.error_path(),
            Self::Instance(instance) => instance.error_path(),
            Self::Snapshot(snapshot) => snapshot.error_path(),
        }
    }
}

pub struct PropertyGroups<'a, St> {
    parent: PropertyGroupParent<'a>,
    iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    _state: PhantomData<fn() -> St>,
}

impl<'a> PropertyGroups<'a, PropertyGroupEditable> {
    pub(crate) fn from_service(
        service: &'a Service<'a>,
        iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    ) -> Self {
        Self {
            parent: PropertyGroupParent::Service(service),
            iter,
            _state: PhantomData,
        }
    }

    pub(crate) fn from_instance(
        instance: &'a Instance<'a>,
        iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    ) -> Self {
        Self {
            parent: PropertyGroupParent::Instance(instance),
            iter,
            _state: PhantomData,
        }
    }
}

impl<'a> PropertyGroups<'a, PropertyGroupSnapshot> {
    pub(crate) fn from_snapshot(
        snapshot: &'a Snapshot<'a>,
        iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    ) -> Self {
        Self {
            parent: PropertyGroupParent::Snapshot(snapshot),
            iter,
            _state: PhantomData,
        }
    }
}

impl<'a, St> Iterator for PropertyGroups<'a, St> {
    type Item = Result<PropertyGroup<'a, St>, IterError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next_named(&self.parent, || self.parent.scf().scf_pg_create())
            .map(|result| {
                result.map(|(name, handle)| PropertyGroup {
                    parent: self.parent,
                    fmri: self.parent.property_group_fmri(&name),
                    name,
                    handle,
                    _state: PhantomData,
                })
            })
    }
}
