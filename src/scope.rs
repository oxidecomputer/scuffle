// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Scf;
use crate::Service;
use crate::ServiceError;
use std::ffi::NulError;
use std::ptr::NonNull;

#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error("error creating scope handle")]
    HandleCreate(#[source] LibscfError),

    #[error("error getting local scope")]
    GetLocalScope(#[source] LibscfError),

    #[error("invalid service name {name:?}")]
    InvalidServiceName {
        name: String,
        #[source]
        err: NulError,
    },

    #[error("failed creating handle for service `{name}`")]
    CreateService {
        name: String,
        #[source]
        err: LibscfError,
    },
}

pub struct Scope<'a> {
    scf: &'a Scf<'a>,
    handle: NonNull<libscf_sys::scf_scope_t>,
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        unsafe { libscf_sys::scf_scope_destroy(self.handle.as_ptr()) };
    }
}

impl<'a> Scope<'a> {
    pub(crate) fn new_local(scf: &'a Scf) -> Result<Self, ScopeError> {
        let handle = LibscfError::from_ptr(unsafe {
            libscf_sys::scf_scope_create(scf.handle().as_ptr())
        })
        .map_err(ScopeError::HandleCreate)?;

        // Construct the Scope object immediately so we clean up on drop on any
        // error below.
        let scope = Self { scf, handle };

        LibscfError::from_ret(unsafe {
            libscf_sys::scf_handle_get_scope(
                scf.handle().as_ptr(),
                libscf_sys::SCF_SCOPE_LOCAL.as_ptr().cast::<i8>(),
                scope.handle.as_ptr(),
            )
        })
        .map_err(ScopeError::GetLocalScope)?;

        Ok(scope)
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.scf
    }

    pub(crate) fn handle(&self) -> &NonNull<libscf_sys::scf_scope_t> {
        &self.handle
    }

    pub fn service(
        &self,
        name: &str,
    ) -> Result<Option<Service<'_>>, ServiceError> {
        Service::new(self, name)
    }
}
