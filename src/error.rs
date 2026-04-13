// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ffi::FromBytesWithNulError;
use std::ffi::NulError;
use std::fmt;
use std::ptr::NonNull;
use std::str::Utf8Error;

use chrono::DateTime;
use chrono::Utc;
use num_traits::FromPrimitive;

#[cfg(any(test, feature = "testing"))]
use crate::isolated::IsolatedConfigdRefreshError;

pub(crate) trait ErrorPath {
    /// String describing an entity in the SMF tree for the purposes of error
    /// reporting.
    ///
    /// Most types implement this as something FMRI-like; e.g., a property
    /// within a property group within a service would return
    /// `{service_name}/:properties/{property_group_name}/{property_name}`.
    fn error_path(&self) -> String;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupEntity {
    Instance,
    Service,
    Snapshot,
    PropertyGroup,
    Property,
}

impl fmt::Display for LookupEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instance => f.write_str("instance"),
            Self::Service => f.write_str("service"),
            Self::Snapshot => f.write_str("snapshot"),
            Self::PropertyGroup => f.write_str("property group"),
            Self::Property => f.write_str("property"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error("invalid {entity} name {name:?}{}", format_parent(.parent))]
    InvalidName {
        entity: LookupEntity,
        parent: Option<String>,
        name: String,
        #[source]
        err: NulError,
    },

    #[error("error creating handle for {entity} `{name}`{}", format_parent(.parent))]
    HandleCreate {
        entity: LookupEntity,
        parent: Option<String>,
        name: String,
        #[source]
        err: LibscfError,
    },

    #[error("error getting {entity} `{name}`{}", format_parent(.parent))]
    Get {
        entity: LookupEntity,
        parent: Option<String>,
        name: String,
        #[source]
        err: LibscfError,
    },
}

fn format_parent(parent: &Option<String>) -> String {
    match parent {
        Some(p) => format!(" within `{p}`"),
        None => String::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterEntity {
    Instance,
    PropertyGroup,
    Property,
    Snapshot,
    Value,
}

impl fmt::Display for IterEntity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instance => f.write_str("instance"),
            Self::PropertyGroup => f.write_str("property group"),
            Self::Property => f.write_str("property"),
            Self::Snapshot => f.write_str("snapshot"),
            Self::Value => f.write_str("value"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IterError {
    #[error("error creating {entity} iterator over `{parent}`")]
    CreateIter {
        entity: IterEntity,
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error initializing {entity} iterator over `{parent}`")]
    InitIter {
        entity: IterEntity,
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error creating {entity} while iterating over `{parent}`")]
    CreateItem {
        entity: IterEntity,
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error iterating {entity} of `{parent}`")]
    Iterating {
        entity: IterEntity,
        parent: String,
        #[source]
        err: LibscfError,
    },

    #[error("error getting name of {entity} while iterating over `{parent}`")]
    GetName {
        entity: IterEntity,
        parent: String,
        #[source]
        err: ScfStringError,
    },

    #[error("error converting {entity} value while iterating `{parent}`")]
    GetValue {
        entity: IterEntity,
        parent: String,
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
        zonename: String,
        #[source]
        err: LibscfError,
    },

    #[error("error setting zone name to {zonename} during connect")]
    SetZoneName {
        zonename: String,
        #[source]
        err: SetValueError,
    },

    #[error("error setting decoration to attach to zone {zonename}")]
    SetDecorationZoneName {
        zonename: String,
        #[source]
        err: LibscfError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("error creating door path value to {door_path} during connect")]
    CreateDoorPath {
        door_path: String,
        #[source]
        err: LibscfError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("error setting door path to {door_path} during connect")]
    SetDoorPath {
        door_path: String,
        #[source]
        err: SetValueError,
    },

    #[cfg(any(test, feature = "testing"))]
    #[error("error setting decoration to connect to door {door_path}")]
    SetDecorationDoorPath {
        door_path: String,
        #[source]
        err: LibscfError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("invalid fmri {fmri:?}")]
    InvalidFmri {
        fmri: String,
        #[source]
        err: NulError,
    },

    #[error("failed to refresh fmri `{fmri}`")]
    Failed {
        fmri: String,
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
        value: String,
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
    InvalidNetAddrV4(String),

    #[error("invalid net address v6 value: {0}")]
    InvalidNetAddrV6(String),

    #[error("invalid net address value: {0}")]
    InvalidNetAddr(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SingleValueError {
    #[error("property `{description}` has no values")]
    NoValues { description: String },

    #[error("property `{description}` has more than one value")]
    MultipleValues { description: String },

    #[error("error getting single value")]
    IterError(#[from] IterError),
}
