// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Scf;
use crate::Service;
use crate::ServiceError;
use crate::scf::ScfObject;

#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error("error creating scope handle")]
    HandleCreate(#[source] LibscfError),

    #[error("error getting local scope")]
    GetLocalScope(#[source] LibscfError),
}

pub struct Scope<'a> {
    handle: ScfObject<'a, libscf_sys::scf_scope_t>,
}

impl<'a> Scope<'a> {
    pub(crate) fn new_local(scf: &'a Scf) -> Result<Self, ScopeError> {
        let handle =
            scf.scf_scope_create().map_err(ScopeError::HandleCreate)?;

        unsafe { scf.scf_get_scope_local(handle.as_ptr()) }
            .map_err(ScopeError::GetLocalScope)?;

        Ok(Scope { handle })
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.handle.scf()
    }

    pub(crate) unsafe fn scf_get_service(
        &self,
        name: *const i8,
        service: *mut libscf_sys::scf_service_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_scope_get_service(
                self.handle.as_ptr(),
                name,
                service,
            )
        })
    }

    pub fn service(
        &self,
        name: &str,
    ) -> Result<Option<Service<'_>>, ServiceError> {
        Service::new(self, name)
    }
}
