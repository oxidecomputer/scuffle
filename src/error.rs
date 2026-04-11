// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::ptr::NonNull;

use num_traits::FromPrimitive;

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
