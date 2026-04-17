// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Error types used throughout `scuffle`.

use crate::ValueKind;
use chrono::DateTime;
use chrono::Utc;
use num_traits::FromPrimitive;
use std::ffi::FromBytesWithNulError;
use std::ffi::NulError;
use std::fmt;
use std::ptr::NonNull;
use std::str::Utf8Error;

#[cfg(any(test, feature = "testing"))]
pub use crate::isolated::IsolatedConfigdRefreshError;

/// Name of a `libscf` entity.
///
/// `ScfEntity` is used in various error variants that may be emitted for
/// multiple entity types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScfEntity {
    Instance,
    Iter,
    Name,
    Scf,
    Service,
    Snapshot,
    PropertyGroup,
    PropertyGroupType,
    Property,
    Scope,
    Transaction,
    TransactionEntry,
    Value,
}

impl fmt::Display for ScfEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Instance => "instance",
            Self::Iter => "iterator",
            Self::Name => "name",
            Self::Scf => "scf",
            Self::Service => "service",
            Self::Snapshot => "snapshot",
            Self::PropertyGroup => "property group",
            Self::PropertyGroupType => "property group type",
            Self::Property => "property",
            Self::Scope => "scope",
            Self::Transaction => "transaction",
            Self::TransactionEntry => "transaction entry",
            Self::Value => "value",
        };
        s.fmt(f)
    }
}

/// Whether a property group is direct-attached or from a composed view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyGroupKind {
    /// The property group is directly attached to a service or instance.
    DirectAttached,

    /// The property group is from the composed view of an instance.
    InstanceComposed,

    /// The property group is from the composed view of a snapshot.
    SnapshotComposed { snapshot_name: Box<str> },
}

mod sealed {
    pub trait EntityDescriptionSealed {}

    impl EntityDescriptionSealed for crate::Instance<'_> {}
    impl<T> EntityDescriptionSealed for crate::Property<'_, T> {}
    impl<T> EntityDescriptionSealed for crate::PropertyGroup<'_, T> {}
    impl EntityDescriptionSealed for crate::Scope<'_> {}
    impl EntityDescriptionSealed for crate::Service<'_> {}
    impl EntityDescriptionSealed for crate::Snapshot<'_> {}

    impl EntityDescriptionSealed
        for crate::property_group::PropertyGroupParent<'_>
    {
    }
}

/// Trait for converting an SCF entity into its description.
///
/// This is intended for constructing detailed errors.
pub trait ToEntityDescription: sealed::EntityDescriptionSealed {
    /// Get the description of this entity.
    fn to_entity_description(&self) -> ScfEntityDescription;
}

/// Description of a `libscf` entity.
///
/// Each description includes the FMRI of the entity. The variants allow
/// distinguishing differences that are not part of an FMRI (e.g., whether a
/// property group is from a direct-attached instance or a composed view of an
/// instance).
///
/// Use [`ScfEntityDescription::error_display()`] to display values of this type
/// consistently with the way `scuffle` displays them in errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScfEntityDescription {
    /// The local (only) scope.
    LocalScope,

    /// A service.
    Service { fmri: Box<str> },

    /// An instance.
    Instance { fmri: Box<str> },

    /// A composed view of an instance's property groups.
    InstanceComposed { fmri: Box<str> },

    /// A snapshot of an instance.
    Snapshot { instance_fmri: Box<str>, name: Box<str> },

    /// A property group.
    PropertyGroup { fmri: Box<str>, kind: PropertyGroupKind },

    /// A property obtained from a particular kind of property group.
    Property { fmri: Box<str>, from_pg_kind: PropertyGroupKind },
}

impl ScfEntityDescription {
    /// Display `self` suitable for use in error `Display` impls.
    pub fn error_display(&self) -> ScfEntityDescriptionErrorDisplay<'_> {
        ScfEntityDescriptionErrorDisplay(self)
    }
}

/// Newtype wrapper for displaying [`ScfEntityDescription`]s.
pub struct ScfEntityDescriptionErrorDisplay<'a>(&'a ScfEntityDescription);

impl fmt::Display for ScfEntityDescriptionErrorDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ScfEntityDescription::LocalScope => write!(f, "local scope"),
            ScfEntityDescription::Service { fmri }
            | ScfEntityDescription::Instance { fmri } => write!(f, "`{fmri}`"),
            ScfEntityDescription::InstanceComposed { fmri } => {
                write!(f, "`{fmri}` (composed view)")
            }
            ScfEntityDescription::Snapshot { instance_fmri, name } => {
                write!(f, "`{instance_fmri}` (`{name}` snapshot)")
            }
            ScfEntityDescription::PropertyGroup { fmri, kind }
            | ScfEntityDescription::Property { fmri, from_pg_kind: kind } => {
                match kind {
                    PropertyGroupKind::DirectAttached => write!(f, "`{fmri}`"),
                    PropertyGroupKind::InstanceComposed => {
                        write!(f, "`{fmri}` (composed view)")
                    }
                    PropertyGroupKind::SnapshotComposed { snapshot_name } => {
                        write!(f, "`{fmri}` (`{snapshot_name}` snapshot)")
                    }
                }
            }
        }
    }
}

/// Error creating a new entity handle.
#[derive(Debug, thiserror::Error)]
#[error("failed to create {entity} handle")]
pub struct HandleCreateError {
    pub entity: ScfEntity,
    #[source]
    pub err: LibscfError,
}

/// Error looking up an entity within a parent.
#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("invalid {entity} name {name:?}")]
    InvalidName {
        entity: ScfEntity,
        name: Box<str>,
        #[source]
        err: NulError,
    },

    #[error(
        "failed to get {entity} `{name}` within {}",
        .parent.error_display(),
    )]
    Get {
        entity: ScfEntity,
        parent: ScfEntityDescription,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },
}

/// Error getting an instance handle via its FMRI.
#[derive(Debug, thiserror::Error)]
pub enum InstanceFromFmriError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("invalid FMRI {fmri:?}")]
    InvalidFmri {
        fmri: Box<str>,
        #[source]
        err: NulError,
    },

    #[error("failed to get instance `{fmri}`")]
    Get {
        fmri: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("failed to get name of instance `{fmri}` from libscf")]
    GetName {
        fmri: Box<str>,
        #[source]
        err: ScfStringError,
    },
}

/// Error getting a handle to the instance of the currently-running service
/// instance.
#[derive(Debug, thiserror::Error)]
pub enum InstanceFromEnvError {
    #[error(
        "failed to look up fmri from env var `{env_var}` \
         (is this process running under SMF?)"
    )]
    EnvLookup {
        env_var: &'static str,
        #[source]
        err: std::env::VarError,
    },

    #[error(transparent)]
    InstanceFromFmri(#[from] InstanceFromFmriError),
}

/// Specific ways `libscf` iteration can fail.
#[derive(Debug, thiserror::Error)]
pub enum IterErrorKind {
    #[error("failed to initialize iterator")]
    Init(#[source] LibscfError),

    #[error("failed to get next item")]
    GetNext(#[source] LibscfError),

    #[error("failed to get item name")]
    GetName(#[source] ScfStringError),

    #[error("failed to get item value")]
    GetValue(#[source] ValueGetError),
}

/// Error iterating entities.
#[derive(Debug, thiserror::Error)]
pub enum IterError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("failed to iterate {entity} over {}", .parent.error_display())]
    Iter {
        entity: ScfEntity,
        parent: ScfEntityDescription,
        #[source]
        kind: IterErrorKind,
    },
}

/// Mapping of `libscf` error codes.
///
/// The variants of this enum correspond to [`libscf_sys::scf_error_t`] values.
#[derive(Debug, thiserror::Error)]
pub enum LibscfError {
    #[error("no error")]
    None,
    #[error("handle not bound")]
    NotBound,
    #[error("cannot use unset argument")]
    NotSet,
    #[error("nothing of that name found")]
    NotFound,
    #[error("type does not match value")]
    TypeMismatch,
    #[error("cannot modify while in-use")]
    InUse,
    #[error("repository connection gone")]
    ConnectionBroken,
    #[error("bad argument")]
    InvalidArgument,
    #[error("no memory available")]
    NoMemory,
    #[error("required constraint not met")]
    ConstraintViolated,
    #[error("object already exists")]
    Exists,
    #[error("repository server unavailable")]
    NoServer,
    #[error("server has insufficient resources")]
    NoResources,
    #[error("insufficient privileges for action")]
    PermissionDenied,
    #[error("backend refused access")]
    BackendAccess,
    #[error("mismatched SCF handles")]
    HandleMismatch,
    #[error("object bound to destroyed handle")]
    HandleDestroyed,
    #[error("incompatible SCF version")]
    VersionMismatch,
    #[error("backend is read-only")]
    BackendReadonly,
    #[error("object has been deleted")]
    Deleted,
    #[error("template data is invalid")]
    TemplateInvalid,
    #[error("user callback function failed")]
    CallbackFailed,
    #[error("internal error")]
    Internal,
    #[error("unknown error ({0})")]
    Unknown(u32),
}

impl From<u32> for LibscfError {
    fn from(error: u32) -> Self {
        use LibscfError::*;
        use libscf_sys::scf_error_t;

        match scf_error_t::from_u32(error) {
            Some(scf_error_t::SCF_ERROR_NONE) => Self::None,
            Some(scf_error_t::SCF_ERROR_NOT_BOUND) => NotBound,
            Some(scf_error_t::SCF_ERROR_NOT_SET) => NotSet,
            Some(scf_error_t::SCF_ERROR_NOT_FOUND) => NotFound,
            Some(scf_error_t::SCF_ERROR_TYPE_MISMATCH) => TypeMismatch,
            Some(scf_error_t::SCF_ERROR_IN_USE) => InUse,
            Some(scf_error_t::SCF_ERROR_CONNECTION_BROKEN) => ConnectionBroken,
            Some(scf_error_t::SCF_ERROR_INVALID_ARGUMENT) => InvalidArgument,
            Some(scf_error_t::SCF_ERROR_NO_MEMORY) => NoMemory,
            Some(scf_error_t::SCF_ERROR_CONSTRAINT_VIOLATED) => {
                ConstraintViolated
            }
            Some(scf_error_t::SCF_ERROR_EXISTS) => Exists,
            Some(scf_error_t::SCF_ERROR_NO_SERVER) => NoServer,
            Some(scf_error_t::SCF_ERROR_NO_RESOURCES) => NoResources,
            Some(scf_error_t::SCF_ERROR_PERMISSION_DENIED) => PermissionDenied,
            Some(scf_error_t::SCF_ERROR_BACKEND_ACCESS) => BackendAccess,
            Some(scf_error_t::SCF_ERROR_HANDLE_MISMATCH) => HandleMismatch,
            Some(scf_error_t::SCF_ERROR_HANDLE_DESTROYED) => HandleDestroyed,
            Some(scf_error_t::SCF_ERROR_VERSION_MISMATCH) => VersionMismatch,
            Some(scf_error_t::SCF_ERROR_BACKEND_READONLY) => BackendReadonly,
            Some(scf_error_t::SCF_ERROR_DELETED) => Deleted,
            Some(scf_error_t::SCF_ERROR_TEMPLATE_INVALID) => TemplateInvalid,
            Some(scf_error_t::SCF_ERROR_CALLBACK_FAILED) => CallbackFailed,
            Some(scf_error_t::SCF_ERROR_INTERNAL) => Internal,
            Option::None => Unknown(error),
        }
    }
}

impl LibscfError {
    pub(crate) fn last() -> Self {
        LibscfError::from(unsafe { libscf_sys::scf_error() })
    }

    pub(crate) fn from_ptr<T>(ptr: *mut T) -> Result<NonNull<T>, Self> {
        match NonNull::new(ptr) {
            Some(ptr) => Ok(ptr),
            None => Err(Self::last()),
        }
    }

    pub(crate) fn from_ret(ret: libc::c_int) -> Result<(), Self> {
        if ret == 0 {
            Ok(())
        } else {
            // Some libscf functions return 1 to indicate special results (e.g.,
            // `scf_iter_next_*()` returns 0 for "success, no items left" and 1
            // for "success, got next item"). Assert that _this_ function is not
            // called with those special values. This is `debug_assert!()`
            // because we don't want to crash our caller and should catch any
            // misuse in tests.
            debug_assert!(
                ret < 0,
                "LibscfError::from_ret() called with non-zero, \
                 non-negative value {ret}"
            );
            Err(Self::last())
        }
    }

    pub(crate) fn from_ssize(ret: libc::ssize_t) -> Result<usize, Self> {
        usize::try_from(ret).map_err(|_| Self::last())
    }
}

/// Error getting string values from `libscf`.
#[derive(Debug, thiserror::Error)]
pub enum ScfStringError {
    #[error(
        "libscf returned {entity} of length {scf_len} \
         (expected at most {max_len})"
    )]
    OutOfBounds { entity: ScfEntity, scf_len: usize, max_len: usize },

    #[error("failed to get {entity} as string")]
    Get {
        entity: ScfEntity,
        #[source]
        err: LibscfError,
    },

    #[error("received invalid C string from libscf")]
    InvalidCString(#[from] FromBytesWithNulError),

    #[error("received non-UTF8 string from libscf")]
    NonUtf8String(#[from] Utf8Error),
}

/// Error looking up the local scope.
#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("failed to get local scope")]
    GetLocalScope(#[source] LibscfError),
}

/// Error constructing the top-level `libscf` handle.
#[derive(Debug, thiserror::Error)]
pub enum ScfError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("failed to bind scf handle")]
    HandleBind(#[source] LibscfError),

    #[error("failed to set zone name to {zonename} during connect")]
    SetZoneName {
        zonename: Box<str>,
        #[source]
        err: ValueSetError,
    },

    #[error("failed to set decoration to attach to zone {zonename}")]
    SetDecorationZoneName {
        zonename: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("failed to set door path to {door_path} during connect")]
    SetDoorPath {
        door_path: Box<str>,
        #[source]
        err: ValueSetError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("failed to set decoration to connect to door {door_path}")]
    SetDecorationDoorPath {
        door_path: Box<str>,
        #[source]
        err: LibscfError,
    },
}

/// Error refreshing an instance.
#[derive(Debug, thiserror::Error)]
pub enum InstanceRefreshError {
    #[error("invalid instance FMRI {fmri:?}")]
    InvalidFmri {
        fmri: Box<str>,
        #[source]
        err: NulError,
    },

    #[error("failed to refresh instance `{fmri}`")]
    Failed {
        fmri: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("failed to refresh via isolated svc.configd")]
    Isolated(#[from] IsolatedConfigdRefreshError),
}

/// Error setting a `libscf` value.
#[derive(Debug, thiserror::Error)]
pub enum ValueSetError {
    #[error(
        "failed to set value `{}` on internal libscf value",
        value.display_smf(),
    )]
    Set {
        value: crate::Value,
        #[source]
        err: LibscfError,
    },

    #[error("invalid string value {value:?}")]
    InvalidString {
        value: Box<str>,
        #[source]
        err: NulError,
    },

    #[error(
        "invalid subsecond nanos in timestamp {timestamp} ({seconds}.{nanos:09})"
    )]
    InvalidTimestampNanos { timestamp: DateTime<Utc>, seconds: i64, nanos: u32 },
}

/// Error getting a `libscf` value.
#[derive(Debug, thiserror::Error)]
pub enum ValueGetError {
    #[error("unexpected scf type value: {0}")]
    UnexpectedTypeValue(i32),

    #[error("value is invalid")]
    Invalid(#[source] LibscfError),

    #[error("failed to get value as boolean")]
    GetBool(#[source] LibscfError),

    #[error("failed to get value as count")]
    GetCount(#[source] LibscfError),

    #[error("failed to get value as integer")]
    GetInteger(#[source] LibscfError),

    #[error("failed to get value as time")]
    GetTime(#[source] LibscfError),

    #[error("timestamp value from scf is invalid: {secs}.{nanos:09}")]
    InvalidTime { secs: i64, nanos: i32 },

    #[error("failed to get value as opaque")]
    GetOpaque(#[source] LibscfError),

    #[error("failed to get value as opaque: got out of bounds length {0}")]
    GetOpaqueOutOfBounds(usize),

    #[error("failed to get value from libscf")]
    GetAsString(#[from] ScfStringError),

    #[error("invalid net address v4 value: {0}")]
    InvalidNetAddrV4(Box<str>),

    #[error("invalid net address v6 value: {0}")]
    InvalidNetAddrV6(Box<str>),

    #[error("invalid net address value: {0}")]
    InvalidNetAddr(Box<str>),
}

/// Error getting the sole value of a property.
#[derive(Debug, thiserror::Error)]
pub enum SingleValueError {
    #[error("property {} has no values", .description.error_display())]
    NoValues { description: ScfEntityDescription },

    #[error(
        "property {} has more than one value",
        .description.error_display(),
    )]
    MultipleValues { description: ScfEntityDescription },

    #[error("failed to get single value")]
    IterError(#[from] IterError),
}

/// Error from [`PropertyGroup::update()`].
///
/// [`PropertyGroup::update()`]: crate::PropertyGroup::update
#[derive(Debug, thiserror::Error)]
pub enum PropertyGroupUpdateError {
    #[error("failed to update property group {}", .description.error_display())]
    Failed {
        description: ScfEntityDescription,
        #[source]
        err: LibscfError,
    },
}

/// Error getting a property group's type from `libscf`.
#[derive(Debug, thiserror::Error)]
pub enum PropertyGroupTypeError {
    #[error(
        "failed to get type of property group {} from libscf",
        .description.error_display(),
    )]
    GetType {
        description: ScfEntityDescription,
        #[source]
        err: ScfStringError,
    },

    #[error(
        "unknown type for property group {}: `{type_}`",
        .description.error_display(),
    )]
    UnknownType { description: ScfEntityDescription, type_: Box<str> },
}

/// Error adding a property group to a service or instance.
#[derive(Debug, thiserror::Error)]
pub enum PropertyGroupAddError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error(
        "invalid property group name {name:?} in {}",
        .parent.error_display(),
    )]
    InvalidName {
        parent: ScfEntityDescription,
        name: Box<str>,
        #[source]
        err: NulError,
    },

    #[error(
        "failed to add property group `{name}` to {}",
        .parent.error_display(),
    )]
    Add {
        parent: ScfEntityDescription,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "failed to look up existence of property group `{name}` on {}",
        .parent.error_display(),
    )]
    ExistenceLookup {
        parent: ScfEntityDescription,
        name: Box<str>,
        #[source]
        err: LookupError,
    },

    #[error(
        "property group `{name}` on {} was deleted concurrently \
         with ensure attempt",
        .parent.error_display(),
    )]
    DeletedDuringEnsure { parent: ScfEntityDescription, name: Box<str> },
}

/// Error deleting a property group from a service or instance.
#[derive(Debug, thiserror::Error)]
pub enum PropertyGroupDeleteError {
    #[error(
        "failed to look up property group `{name}` for deletion on {}",
        .parent.error_display(),
    )]
    Lookup {
        parent: ScfEntityDescription,
        name: Box<str>,
        #[source]
        err: LookupError,
    },

    #[error("failed to delete property group {}", .description.error_display())]
    Delete {
        description: ScfEntityDescription,
        #[source]
        err: LibscfError,
    },
}

/// Kind of transaction operation to modify a property group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionOp {
    Delete,
    New,
    Change,
    ChangeType,
    AddValue,
}

// Helper method for use in the error string of `TransactionPropertyError`
fn format_transaction_op(op: &TransactionOp) -> &'static str {
    match op {
        TransactionOp::Delete => "delete",
        TransactionOp::New => "create new",
        TransactionOp::Change => "change",
        TransactionOp::ChangeType => "change type of",
        TransactionOp::AddValue => "add value to",
    }
}

/// Error performing an operation on a property within a property group
/// transaction.
#[derive(Debug, thiserror::Error)]
#[error(
    "failed to {} property `{name}` in transaction on {}",
    format_transaction_op(.op),
    .property_group.error_display(),
)]
pub struct TransactionPropertyError {
    pub property_group: ScfEntityDescription,
    pub name: Box<str>,
    pub op: TransactionOp,
    #[source]
    pub err: LibscfError,
}

/// Error building a property group transaction.
#[derive(Debug, thiserror::Error)]
pub enum TransactionBuildError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error(
        "failed to start transaction on {}",
        .property_group.error_display(),
    )]
    Start {
        property_group: ScfEntityDescription,
        #[source]
        err: LibscfError,
    },

    #[error(
        "invalid property name in transaction on {}",
        .property_group.error_display(),
    )]
    InvalidName {
        property_group: ScfEntityDescription,
        #[source]
        err: NulError,
    },

    #[error(
        "failed to look up existence of property `{name}` in transaction on {}",
         .property_group.error_display(),
    )]
    ExistenceLookup {
        property_group: ScfEntityDescription,
        name: Box<str>,
        #[source]
        err: LookupError,
    },

    #[error(transparent)]
    Property(#[from] TransactionPropertyError),

    #[error(
        "type mismatch on property `{name}` in transaction on {}: \
         property has type {property_type} but value has type {value_type}",
        .property_group.error_display(),
    )]
    TypeMismatch {
        property_group: ScfEntityDescription,
        name: Box<str>,
        property_type: ValueKind,
        value_type: ValueKind,
    },

    #[error(
        "failed to set value for property `{name}` in transaction on {}",
        .property_group.error_display(),
    )]
    SetValue {
        property_group: ScfEntityDescription,
        name: Box<str>,
        #[source]
        err: ValueSetError,
    },
}

/// Error committing a property group transaction.
#[derive(Debug, thiserror::Error)]
#[error("failed to commit transaction on {}", .property_group.error_display())]
pub struct TransactionCommitError {
    pub property_group: ScfEntityDescription,
    #[source]
    pub err: LibscfError,
}
