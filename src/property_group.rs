// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::DeletePropertyGroupResult;
use crate::Instance;
use crate::Properties;
use crate::Property;
use crate::Scf;
use crate::Service;
use crate::Snapshot;
use crate::Transaction;
use crate::TransactionReset;
use crate::buf::scf_get_string;
use crate::buf::with_scf_pg_type_buf;
use crate::error::DeletePropertyGroupError;
use crate::error::ErrorPath;
use crate::error::IterError;
use crate::error::IterErrorKind;
use crate::error::LibscfError;
use crate::error::LookupError;
use crate::error::PropertyGroupTypeError;
use crate::error::ScfEntity;
use crate::error::TransactionError;
use crate::error::UpdatePropertyGroupError;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::PropertyFmri;
use crate::utf8cstring::PropertyGroupFmri;
use crate::utf8cstring::Utf8CString;
use std::ffi::CStr;
use std::fmt;
use std::marker::PhantomData;

/// Property group types.
///
/// The underlying values for these variants map to the corresponding
/// `SCF_GROUP_*` constants in `libscf.h.`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(any(test, feature = "testing"), derive(test_strategy::Arbitrary))]
pub enum PropertyGroupType {
    Application,
    Framework,
    Dependency,
    Method,
    Template,
    TemplatePgPattern,
    TemplatePropPattern,
}

impl fmt::Display for PropertyGroupType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Application => "application",
            Self::Framework => "framework",
            Self::Dependency => "dependency",
            Self::Method => "method",
            Self::Template => "template",
            Self::TemplatePgPattern => "template_pg_pattern",
            Self::TemplatePropPattern => "template_prop_pattern",
        };
        s.fmt(f)
    }
}

impl PropertyGroupType {
    pub fn new(name: &str) -> Option<Self> {
        let pg_type = match name {
            "application" => Self::Application,
            "framework" => Self::Framework,
            "dependency" => Self::Dependency,
            "method" => Self::Method,
            "template" => Self::Template,
            "template_pg_pattern" => Self::TemplatePgPattern,
            "template_prop_pattern" => Self::TemplatePropPattern,
            _ => return None,
        };
        Some(pg_type)
    }

    pub(crate) fn as_c_str(&self) -> &'static CStr {
        let s = match self {
            Self::Application => b"application\0" as &[u8],
            Self::Framework => b"framework\0",
            Self::Dependency => b"dependency\0",
            Self::Method => b"method\0",
            Self::Template => b"template\0",
            Self::TemplatePgPattern => b"template_pg_pattern\0",
            Self::TemplatePropPattern => b"template_prop_pattern\0",
        };
        CStr::from_bytes_with_nul(s).expect("string constants are valid CStrs")
    }
}

/// Type-state marker for a [`PropertyGroup`] that is directly attached to an
/// [`Instance`] or [`Service`].
///
/// Directly-attached property groups may have their properties modified via
/// [`PropertyGroup::transaction()`].
#[derive(Debug)]
pub enum PropertyGroupDirect {}

/// Type-state marker for a [`PropertyGroup`] that is from a composed view.
///
/// Composed view property groups may be obtained via the
/// [`HasComposedPropertyGroups`] implementation on [`Instance`] (giving a view
/// of an instance -> service) or [`Snapshot`] (giving a view of a snapshot ->
/// instance -> service).
///
/// Composed property groups may not be modified. `libscf` does not allow
/// modification of composed property groups from snapshots, but does allow
/// modification of composed property groups from instances. `scuffle` is
/// intentionally less flexible in this; modifying property groups through a
/// composed view can be confusing at runtime, so `scuffle` requires all
/// modifications be made via directly attached properties.
///
/// [`HasComposedPropertyGroups`]: `crate::HasComposedPropertyGroups`
#[derive(Debug)]
pub enum PropertyGroupComposed {}

/// Result of updating a property group against its latest version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyGroupUpdateResult {
    /// The property group was updated.
    Updated,
    /// The property group was not updated.
    AlreadyUpToDate,
}

/// Handle to an SMF property group.
///
/// Property groups may be associated with different kinds of parent entities:
///
/// * [`Service`]s (direct attached)
/// * [`Instance`]s (direct attached or composed)
/// * [`Snapshot`]s (composed)
///
/// and may be obtained via the [`HasDirectPropertyGroups`] or
/// [`HasComposedPropertyGroups`] implementations on each of those parent types.
///
/// [`HasComposedPropertyGroups`]: `crate::HasComposedPropertyGroups`
/// [`HasDirectPropertyGroups`]: `crate::HasDirectPropertyGroups`
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
                entity: ScfEntity::PropertyGroup,
                name: name.to_string().into_boxed_str(),
                err,
            }
        })?;

        let fmri = parent.property_group_fmri(&name);

        let mut handle = parent.scf().scf_pg_create()?;

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
                entity: ScfEntity::PropertyGroup,
                parent: parent.error_path(),
                name: name.into_string().into_boxed_str(),
                err,
            }),
        }
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.handle.scf()
    }

    pub(crate) fn parent(&self) -> PropertyGroupParent<'a> {
        self.parent
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

    /// Get the name of this property group.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get the full FMRI of this property group.
    ///
    /// Note that if this property group is from a composed view through an
    /// instance or snapshot, that information is _not_ included in the FMRI.
    pub fn fmri(&self) -> &str {
        self.fmri.as_str()
    }

    /// Get the type of this property group.
    pub fn type_(&self) -> Result<PropertyGroupType, PropertyGroupTypeError> {
        let type_string = with_scf_pg_type_buf(|buf| {
            scf_get_string(
                ScfEntity::PropertyGroupType,
                buf,
                |buf, buf_len| unsafe {
                    libscf_sys::scf_pg_get_type(
                        self.handle.as_ptr(),
                        buf,
                        buf_len,
                    )
                },
            )
        })
        .map_err(|err| PropertyGroupTypeError::GetType {
            description: self.error_path(),
            err,
        })?;

        PropertyGroupType::new(type_string.as_str()).ok_or_else(|| {
            PropertyGroupTypeError::UnknownType {
                description: self.error_path(),
                type_: type_string.into_string().into_boxed_str(),
            }
        })
    }

    pub(crate) fn property_fmri(&self, name: &Utf8CString) -> PropertyFmri {
        self.fmri.append_property(name)
    }

    /// Look up a property in this property group by name.
    pub fn property(
        &self,
        name: &str,
    ) -> Result<Option<Property<'_, St>>, LookupError> {
        Property::from_property_group(self, name)
    }

    /// Get an iterator over all [`Property`]s in this property group.
    pub fn properties(&self) -> Result<Properties<'_, St>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf())?;
        let iter = unsafe {
            iter.init_property_group_properties(self.handle.as_ptr())
        }
        .map_err(|err| IterError::Iter {
            entity: ScfEntity::Property,
            parent: self.error_path(),
            kind: IterErrorKind::Init(err),
        })?;
        Ok(Properties::new(self, iter))
    }

    /// Ensure that this property group handle is attached to the most recent
    /// version of this property group.
    pub fn update(
        &mut self,
    ) -> Result<PropertyGroupUpdateResult, UpdatePropertyGroupError> {
        match unsafe { libscf_sys::scf_pg_update(self.handle.as_mut_ptr()) } {
            0 => Ok(PropertyGroupUpdateResult::AlreadyUpToDate),
            1 => Ok(PropertyGroupUpdateResult::Updated),
            _ => Err(UpdatePropertyGroupError::Failed {
                description: self.error_path(),
                err: LibscfError::last(),
            }),
        }
    }
}

impl<St> ErrorPath for PropertyGroup<'_, St> {
    fn error_path(&self) -> Box<str> {
        match &self.parent {
            // If we are direct-attached to a service or instance, our FMRI
            // is a full description of ourself for errors.
            //
            // If we're going through a composed view, that information is not
            // included in any way in `self.fmri()`; append a note.
            PropertyGroupParent::Service(_)
            | PropertyGroupParent::Instance(_) => {
                self.fmri().to_string().into_boxed_str()
            }
            PropertyGroupParent::InstanceComposed(_) => {
                format!("{} (composed)", self.fmri()).into_boxed_str()
            }
            PropertyGroupParent::Snapshot(snapshot) => {
                format!("{} ({} snapshot)", self.fmri(), snapshot.name())
                    .into_boxed_str()
            }
        }
    }
}

// Methods only available on direct-attached property groups.
impl<'a> PropertyGroup<'a, PropertyGroupDirect> {
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

    /// Open a [`Transaction`] to make changes to the properties within this
    /// property group.
    ///
    /// This method is only available on `PropertyGroup`s in the
    /// [`PropertyGroupDirect`] type state; modifications to property groups
    /// through composed views are not supported.
    pub fn transaction(
        &mut self,
    ) -> Result<Transaction<'_, 'a, TransactionReset>, TransactionError> {
        Transaction::new(self)
    }

    pub(crate) fn delete(
        mut self,
    ) -> Result<DeletePropertyGroupResult, DeletePropertyGroupError> {
        let result = LibscfError::from_ret(unsafe {
            libscf_sys::scf_pg_delete(self.handle.as_mut_ptr())
        });
        match result {
            Ok(()) => Ok(DeletePropertyGroupResult::Deleted),
            // The fact that we have a fully-constructed `PropertyGroup` means
            // the pg _did_ exist at one point; if we get a `Deleted` here,
            // that means someone else concurrently deleted us.
            Err(LibscfError::Deleted) => {
                Ok(DeletePropertyGroupResult::DoesNotExist)
            }
            Err(err) => Err(DeletePropertyGroupError::Delete {
                description: self.error_path(),
                err,
            }),
        }
    }
}

// Methods only available on composed property groups.
impl<'a> PropertyGroup<'a, PropertyGroupComposed> {
    pub(crate) fn from_snapshot(
        snapshot: &'a Snapshot<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        Self::from_parent(PropertyGroupParent::Snapshot(snapshot), name)
    }

    pub(crate) fn from_instance_composed(
        instance: &'a Instance<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        Self::from_parent(PropertyGroupParent::InstanceComposed(instance), name)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PropertyGroupParent<'a> {
    Service(&'a Service<'a>),
    Instance(&'a Instance<'a>),
    InstanceComposed(&'a Instance<'a>),
    Snapshot(&'a Snapshot<'a>),
}

impl<'a> PropertyGroupParent<'a> {
    fn scf(&self) -> &'a Scf<'a> {
        match self {
            Self::Service(service) => service.scf(),
            Self::Instance(instance) | Self::InstanceComposed(instance) => {
                instance.scf()
            }
            Self::Snapshot(snapshot) => snapshot.scf(),
        }
    }

    fn property_group_fmri(&self, name: &Utf8CString) -> PropertyGroupFmri {
        match self {
            Self::Service(service) => service.property_group_fmri(name),
            Self::Instance(instance) | Self::InstanceComposed(instance) => {
                instance.property_group_fmri(name)
            }
            Self::Snapshot(snapshot) => snapshot.property_group_fmri(name),
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
            Self::InstanceComposed(instance) => unsafe {
                instance.scf_get_pg_composed(std::ptr::null(), name, pg)
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
            Self::InstanceComposed(instance) => {
                format!("{} (composed)", instance.error_path()).into_boxed_str()
            }
            Self::Snapshot(snapshot) => snapshot.error_path(),
        }
    }
}

/// Iterator over all [`PropertyGroup`]s in an instance, service, or composed
/// view (instance or snapshot).
///
/// Obtained via [`HasDirectPropertyGroups::property_groups_direct()`] or
/// [`HasComposedPropertyGroups::property_groups_composed()`].
///
/// [`HasDirectPropertyGroups::property_groups_direct()`]:
/// crate::HasDirectPropertyGroups::property_groups_direct
/// [`HasComposedPropertyGroups::property_groups_composed()`]:
/// crate::HasComposedPropertyGroups::property_groups_composed
pub struct PropertyGroups<'a, St> {
    parent: PropertyGroupParent<'a>,
    iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    _state: PhantomData<fn() -> St>,
}

impl<'a> PropertyGroups<'a, PropertyGroupDirect> {
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

impl<'a> PropertyGroups<'a, PropertyGroupComposed> {
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

    pub(crate) fn from_instance_composed(
        instance: &'a Instance<'a>,
        iter: ScfIter<'a, libscf_sys::scf_propertygroup_t>,
    ) -> Self {
        Self {
            parent: PropertyGroupParent::InstanceComposed(instance),
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
