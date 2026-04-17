// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Scf;
use crate::Service;
use crate::error::LibscfError;
use crate::error::LookupError;
use crate::error::ScfEntityDescription;
use crate::error::ScopeError;
use crate::error::ToEntityDescription;
use crate::scf::ScfObject;

/// Handle to an SMF scope.
///
/// SMF currently only supports one scope: the local scope.
pub struct Scope<'a> {
    handle: ScfObject<'a, libscf_sys::scf_scope_t>,
}

impl<'a> Scope<'a> {
    pub(crate) fn new_local(scf: &'a Scf) -> Result<Self, ScopeError> {
        let mut handle = scf.scf_scope_create()?;

        unsafe { scf.scf_get_scope_local(handle.as_mut_ptr()) }
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

    /// Look up a service by name.
    pub fn service(
        &self,
        name: &str,
    ) -> Result<Option<Service<'_>>, LookupError> {
        Service::new(self, name)
    }
}

impl ToEntityDescription for Scope<'_> {
    fn to_entity_description(&self) -> ScfEntityDescription {
        ScfEntityDescription::LocalScope
    }
}
