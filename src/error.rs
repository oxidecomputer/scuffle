// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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

mod sealed {
    pub trait ErrorPath {
        /// String describing an entity in the SMF tree for the purposes of error
        /// reporting.
        ///
        /// Most types implement this as something FMRI-like; e.g., a property
        /// within a property group within a service would return
        /// `{service_name}/:properties/{property_group_name}/{property_name}`.
        fn error_path(&self) -> Box<str>;
    }
}
pub(crate) use sealed::ErrorPath;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScfEntity {
    Instance,
    Iter,
    Name,
    Scf,
    Service,
    Snapshot,
    PropertyGroup,
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
            Self::Property => "property",
            Self::Scope => "scope",
            Self::Transaction => "transaction",
            Self::TransactionEntry => "transaction entry",
            Self::Value => "value",
        };
        s.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("failed to create {entity} handle")]
pub struct HandleCreateError {
    pub entity: ScfEntity,
    #[source]
    pub err: LibscfError,
}

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

    #[error("failed to get {entity} `{name}` within `{parent}`")]
    Get {
        entity: ScfEntity,
        parent: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },
}

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

#[derive(Debug, thiserror::Error)]
pub enum IterErrorKind {
    #[error("failed to initialize iterator")]
    Init(#[source] LibscfError),

    #[error("failed to get next item")]
    GetNext(#[source] LibscfError),

    #[error("failed to get item name")]
    GetName(#[source] ScfStringError),

    #[error("failed to get item value")]
    GetValue(#[source] GetValueError),
}

#[derive(Debug, thiserror::Error)]
pub enum IterError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("failed to iterate {entity} over `{parent}`")]
    Iter {
        entity: ScfEntity,
        parent: Box<str>,
        #[source]
        kind: IterErrorKind,
    },
}

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

#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("failed to get local scope")]
    GetLocalScope(#[source] LibscfError),
}

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
        err: SetValueError,
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
        err: SetValueError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("failed to set decoration to connect to door {door_path}")]
    SetDecorationDoorPath {
        door_path: Box<str>,
        #[source]
        err: LibscfError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
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

#[derive(Debug, thiserror::Error)]
pub enum SetValueError {
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

#[derive(Debug, thiserror::Error)]
pub enum GetValueError {
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

#[derive(Debug, thiserror::Error)]
pub enum SingleValueError {
    #[error("property `{description}` has no values")]
    NoValues { description: Box<str> },

    #[error("property `{description}` has more than one value")]
    MultipleValues { description: Box<str> },

    #[error("failed to get single value")]
    IterError(#[from] IterError),
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePropertyGroupError {
    #[error("failed to update property group `{description}`")]
    Failed {
        description: Box<str>,
        #[source]
        err: LibscfError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum AddPropertyGroupError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("invalid property group name {name:?} in `{parent}`")]
    InvalidName {
        parent: Box<str>,
        name: Box<str>,
        #[source]
        err: NulError,
    },

    #[error("invalid property group type {pg_type:?} in `{parent}`")]
    InvalidType {
        parent: Box<str>,
        pg_type: Box<str>,
        #[source]
        err: NulError,
    },

    #[error("failed to add property group `{name}` to `{parent}`")]
    Add {
        parent: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "failed to look up existence of property group `{name}` on \
         `{parent}`"
    )]
    ExistenceLookup {
        parent: Box<str>,
        name: Box<str>,
        #[source]
        err: LookupError,
    },

    #[error(
        "property group `{name}` on `{parent}` was deleted concurrently \
         with ensure attempt"
    )]
    DeletedDuringEnsure { parent: Box<str>, name: Box<str> },
}

#[derive(Debug, thiserror::Error)]
pub enum DeletePropertyGroupError {
    #[error(
        "failed to look up property group `{name}` for deletion on \
         `{parent}`"
    )]
    Lookup {
        parent: Box<str>,
        name: Box<str>,
        #[source]
        err: LookupError,
    },

    #[error("failed to delete property group `{description}`")]
    Delete {
        description: Box<str>,
        #[source]
        err: LibscfError,
    },
}

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

#[derive(Debug, thiserror::Error)]
#[error(
    "failed to {} property `{name}` in transaction on `{property_group}`",
    format_transaction_op(.op),
)]
pub struct TransactionPropertyError {
    pub property_group: Box<str>,
    pub name: Box<str>,
    pub op: TransactionOp,
    #[source]
    pub err: LibscfError,
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionError {
    #[error(transparent)]
    HandleCreate(#[from] HandleCreateError),

    #[error("failed to start transaction on `{property_group}`")]
    Start {
        property_group: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("invalid property name in transaction on `{property_group}`")]
    InvalidName {
        property_group: Box<str>,
        #[source]
        err: NulError,
    },

    #[error(
        "failed to look up existence of property `{name}` in transaction on \
         `{property_group}`"
    )]
    ExistenceLookup {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LookupError,
    },

    #[error(transparent)]
    Property(#[from] TransactionPropertyError),

    #[error(
        "type mismatch on property `{name}` in transaction on \
         `{property_group}`: property has type {property_type} but value \
         has type {value_type}"
    )]
    TypeMismatch {
        property_group: Box<str>,
        name: Box<str>,
        property_type: ValueKind,
        value_type: ValueKind,
    },

    #[error(
        "failed to set value for property `{name}` in transaction on \
         `{property_group}`"
    )]
    SetValue {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: SetValueError,
    },

    #[error("failed to commit transaction on `{property_group}`")]
    Commit {
        property_group: Box<str>,
        #[source]
        err: LibscfError,
    },
}
