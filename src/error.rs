// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Snapshot;
use crate::ValueKind;
use crate::utf8cstring::Fmri;
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
    Service,
    Snapshot,
    PropertyGroup,
    Property,
    Value,
}

impl fmt::Display for ScfEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Instance => "instance",
            Self::Service => "service",
            Self::Snapshot => "snapshot",
            Self::PropertyGroup => "property group",
            Self::Property => "property",
            Self::Value => "value",
        };
        s.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error("invalid {entity} name {name}")]
    InvalidName {
        entity: ScfEntity,
        name: Box<str>,
        #[source]
        err: NulError,
    },

    #[error("error creating handle for {entity} {target}")]
    HandleCreate {
        entity: ScfEntity,
        target: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("error getting {entity} {target}")]
    Get {
        entity: ScfEntity,
        target: Box<str>,
        #[source]
        err: LibscfError,
    },
}

pub(crate) fn format_lookup_target<T: Fmri>(
    fmri: &T,
    snapshot: Option<&Snapshot<'_>>,
) -> Box<str> {
    match snapshot {
        Some(snap) => format!("`{fmri}` ({} snapshot)", snap.name()),
        None => format!("`{fmri}`"),
    }
    .into_boxed_str()
}

#[derive(Debug, thiserror::Error)]
pub enum IterError {
    #[error("error creating {entity} iterator over `{parent}`")]
    CreateIter {
        entity: ScfEntity,
        parent: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("error initializing {entity} iterator over `{parent}`")]
    InitIter {
        entity: ScfEntity,
        parent: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("error creating {entity} while iterating over `{parent}`")]
    CreateItem {
        entity: ScfEntity,
        parent: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("error iterating {entity} of `{parent}`")]
    Iterating {
        entity: ScfEntity,
        parent: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("error getting name of {entity} while iterating over `{parent}`")]
    GetName {
        entity: ScfEntity,
        parent: Box<str>,
        #[source]
        err: ScfStringError,
    },

    #[error("error converting {entity} value while iterating `{parent}`")]
    GetValue {
        entity: ScfEntity,
        parent: Box<str>,
        #[source]
        err: GetValueError,
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
        if ret == 0 { Ok(()) } else { Err(Self::last()) }
    }

    pub(crate) fn from_ssize(ret: libc::ssize_t) -> Result<usize, Self> {
        usize::try_from(ret).map_err(|_| Self::last())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScfStringError {
    #[error(
        "libscf returned {kind} of length {scf_len} \
         (expected at most {max_len})"
    )]
    OutOfBounds { kind: &'static str, scf_len: usize, max_len: usize },

    #[error("error getting {kind} as string")]
    Get {
        kind: &'static str,
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
    #[error("error creating scope handle")]
    HandleCreate(#[source] LibscfError),

    #[error("error getting local scope")]
    GetLocalScope(#[source] LibscfError),
}

#[derive(Debug, thiserror::Error)]
pub enum ScfError {
    #[error("error creating scf handle")]
    HandleCreate(#[source] LibscfError),

    #[error("error binding scf handle")]
    HandleBind(#[source] LibscfError),

    #[error(
        "error creating zone name value for zone {zonename} during connect"
    )]
    CreateZoneName {
        zonename: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("error setting zone name to {zonename} during connect")]
    SetZoneName {
        zonename: Box<str>,
        #[source]
        err: SetValueError,
    },

    #[error("error setting decoration to attach to zone {zonename}")]
    SetDecorationZoneName {
        zonename: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("error creating door path value to {door_path} during connect")]
    CreateDoorPath {
        door_path: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("error setting door path to {door_path} during connect")]
    SetDoorPath {
        door_path: Box<str>,
        #[source]
        err: SetValueError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("error setting decoration to connect to door {door_path}")]
    SetDecorationDoorPath {
        door_path: Box<str>,
        #[source]
        err: LibscfError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("invalid fmri {fmri:?}")]
    InvalidFmri {
        fmri: Box<str>,
        #[source]
        err: NulError,
    },

    #[error("failed to refresh fmri `{fmri}`")]
    Failed {
        fmri: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("failed to refresh isolated svc.configd")]
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

    #[error("error getting value as boolean")]
    GetBool(#[source] LibscfError),

    #[error("error getting value as count")]
    GetCount(#[source] LibscfError),

    #[error("error getting value as integer")]
    GetInteger(#[source] LibscfError),

    #[error("error getting value as time")]
    GetTime(#[source] LibscfError),

    #[error("timestamp value from scf is invalid: {secs}.{nanos:09}")]
    InvalidTime { secs: i64, nanos: i32 },

    #[error("error getting value as opaque")]
    GetOpaque(#[source] LibscfError),

    #[error("error getting value as opaque: got out of bounds length {0}")]
    GetOpaqueOutOfBounds(usize),

    #[error("error getting value as string")]
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

    #[error("error getting single value")]
    IterError(#[from] IterError),
}

#[derive(Debug, thiserror::Error)]
pub enum AddPropertyGroupError {
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

    #[error("failed to create property group handle for `{parent}`")]
    HandleCreate {
        parent: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("failed to add property group `{name}` to `{parent}`")]
    Add {
        parent: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "error looking up existence of property group `{name}` on \
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
pub enum TransactionError {
    #[error("failed to create transaction on `{property_group}`")]
    HandleCreate {
        property_group: Box<str>,
        #[source]
        err: LibscfError,
    },

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

    #[error("error creating entry in transaction on `{property_group}`")]
    CreateEntry {
        property_group: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "error looking up existence of property `{name}` on \
         `{property_group}`"
    )]
    ExistenceLookup {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LookupError,
    },

    #[error(
        "error deleting property `{name}` in transaction on `{property_group}`"
    )]
    Delete {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "error creating new property `{name}` in transaction on \
         `{property_group}`"
    )]
    New {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "error changing property `{name}` in transaction on \
         `{property_group}`"
    )]
    Change {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "error changing type of property `{name}` in transaction on \
         `{property_group}`"
    )]
    ChangeType {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

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
        "error creating value for property `{name}` in transaction on \
         `{property_group}`"
    )]
    CreateValue {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error(
        "error setting value for property `{name}` in transaction on \
         `{property_group}`"
    )]
    SetValue {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: SetValueError,
    },

    #[error(
        "error adding value to property `{name}` in transaction on \
         `{property_group}`"
    )]
    AddValue {
        property_group: Box<str>,
        name: Box<str>,
        #[source]
        err: LibscfError,
    },

    #[error("failed to commit transaction on `{property_group}`")]
    Commit {
        property_group: Box<str>,
        #[source]
        err: LibscfError,
    },
}
