// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Instance;
use crate::Scope;
use crate::ValueRef;
use crate::error::HandleCreateError;
use crate::error::InstanceFromEnvError;
use crate::error::InstanceFromFmriError;
use crate::error::LibscfError;
use crate::error::RefreshError;
use crate::error::ScfEntity;
use crate::error::ScfError;
use crate::error::ScopeError;
use crate::value::ScfValue;
use std::ffi::CStr;
use std::ffi::CString;
use std::marker::PhantomData;
use std::ptr::NonNull;

mod object;

pub(crate) use object::ScfObject;
pub(crate) use object::ScfObjectType;

#[cfg(any(test, feature = "testing"))]
use crate::isolated::IsolatedConfigd;

// We intentionally do not impl `Send` or `Sync` for `Scf`. Errors flow out
// through the thread-local `scf_error()` function, so we don't want to mix
// use of the same handle across different threads.
#[derive(Debug)]
pub struct Scf<'a> {
    did_bind_handle: bool,
    handle: NonNull<libscf_sys::scf_handle_t>,
    refresher: RefreshMechanism<'a>,
}

impl Drop for Scf<'_> {
    fn drop(&mut self) {
        // We bind the handle in `connect_common()`, but only at the end: it's
        // possible we could fail partway through `connect_common()` and drop an
        // `Scf` before we bind it. If so, don't try to unbind it. (libscf
        // guards against this, so unbinding an unbound handle doesn't cause
        // undefined behavior, but (a) it does return an error and (b) that's an
        // undocumented implementation detail.)
        if self.did_bind_handle {
            unsafe { libscf_sys::scf_handle_unbind(self.handle.as_ptr()) };
        }
        unsafe { libscf_sys::scf_handle_destroy(self.handle.as_ptr()) };
    }
}

impl Scf<'static> {
    pub fn connect_global_zone() -> Result<Self, ScfError> {
        Self::connect_common(
            ConnectMode::Global,
            RefreshMechanism::Libscf(PhantomData),
        )
    }

    pub fn connect_zone(zonename: &str) -> Result<Self, ScfError> {
        Self::connect_common(
            ConnectMode::Zone(zonename),
            RefreshMechanism::Libscf(PhantomData),
        )
    }
}

#[cfg(any(test, feature = "testing"))]
impl<'a> Scf<'a> {
    pub fn connect_isolated(
        configd: &'a IsolatedConfigd,
    ) -> Result<Self, ScfError> {
        Self::connect_common(
            ConnectMode::from(configd),
            RefreshMechanism::Isolated(configd),
        )
    }
}

impl<'a> Scf<'a> {
    fn connect_common(
        mode: ConnectMode<'_>,
        refresher: RefreshMechanism<'a>,
    ) -> Result<Self, ScfError> {
        let handle =
            unsafe { libscf_sys::scf_handle_create(libscf_sys::SCF_VERSION) };
        let handle = LibscfError::from_ptr(handle)
            .map_err(|err| HandleCreateError { entity: ScfEntity::Scf, err })?;

        // Create the Scf object immediately so we clean up on drop on any error
        // below. We don't bind it until the end, though.
        let mut scf = Self { did_bind_handle: false, handle, refresher };

        // Both the `Zone` (available in prod and tests) and `DoorPath`
        // (available only in tests) connect modes rely on undocumented and
        // uncommitted interfaces. These match the way `svccfg` implements the
        // same techniques: after creating an `scf_handle_t` but before binding
        // it, decorating it with either the "zone" (with an astring-typed value
        // specifying the name of the zone) or "door_path" decoration (with an
        // astring-typed value containing a path to the door) will cause us to
        // either connect to the svc.configd instance inside a zone or at a
        // specific door path, respectively.
        //
        // In both cases, we can destroy the value after calling
        // `scf_handle_decorate()`; we do that implicitly here by dropping them.
        match mode {
            ConnectMode::Global => {
                // Nothing special to do.
            }

            ConnectMode::Zone(zonename) => {
                let mut value = ScfValue::new(&scf)?;
                value.set(ValueRef::AString(zonename)).map_err(|err| {
                    ScfError::SetZoneName { zonename: Box::from(zonename), err }
                })?;
                unsafe {
                    value.scf_apply_as_decoration(
                        scf.handle.as_ptr(),
                        libscf_sys::decorations::ZONE.as_ptr().cast::<i8>(),
                    )
                }
                .map_err(|err| {
                    ScfError::SetDecorationZoneName {
                        zonename: Box::from(zonename),
                        err,
                    }
                })?;
            }

            #[cfg(any(test, feature = "testing"))]
            ConnectMode::DoorPath(door_path) => {
                let mut value = ScfValue::new(&scf)?;
                value.set(ValueRef::AString(door_path)).map_err(|err| {
                    ScfError::SetDoorPath {
                        door_path: Box::from(door_path),
                        err,
                    }
                })?;
                unsafe {
                    value.scf_apply_as_decoration(
                        scf.handle.as_ptr(),
                        decorations::DOOR_PATH.as_ptr().cast::<i8>(),
                    )
                }
                .map_err(|err| {
                    ScfError::SetDecorationDoorPath {
                        door_path: Box::from(door_path),
                        err,
                    }
                })?;
            }
        }

        let ret = unsafe { libscf_sys::scf_handle_bind(scf.handle.as_ptr()) };
        () = LibscfError::from_ret(ret).map_err(ScfError::HandleBind)?;
        scf.did_bind_handle = true;

        Ok(scf)
    }

    pub fn scope_local(&self) -> Result<Scope<'_>, ScopeError> {
        Scope::new_local(self)
    }

    pub fn refresh_instance(&self, fmri: &str) -> Result<(), RefreshError> {
        let fmri = CString::new(fmri).map_err(|err| {
            RefreshError::InvalidFmri { fmri: Box::from(fmri), err }
        })?;
        self.refresh_instance_cstr(&fmri)
    }

    pub fn instance_from_fmri(
        &self,
        fmri: &str,
    ) -> Result<Instance<'_>, InstanceFromFmriError> {
        Instance::from_fmri(self, fmri)
    }

    pub fn self_instance_from_env(
        &self,
    ) -> Result<Instance<'_>, InstanceFromEnvError> {
        // From `man smf_method`:
        //
        // > Environment Variables
        // >
        // > The restarter provides four environment variables to the method
        // > that determine the context in which the method is invoked.
        // >
        // > SMF_FMRI
        // >
        // >     The service fault management resource identifier (FMRI) of the
        // >     instance for which the method is invoked.
        //
        // If this process was started under SMF, it can look up its own
        // instance FMRI via that env var.
        const SELF_FMRI_ENV_VAR: &str = "SMF_FMRI";

        let fmri = std::env::var(SELF_FMRI_ENV_VAR).map_err(|err| {
            InstanceFromEnvError::EnvLookup { env_var: SELF_FMRI_ENV_VAR, err }
        })?;
        Ok(self.instance_from_fmri(&fmri)?)
    }

    pub(crate) fn refresh_instance_cstr(
        &self,
        fmri: &CStr,
    ) -> Result<(), RefreshError> {
        self.refresher.refresh_instance(fmri)
    }

    pub(crate) unsafe fn scf_get_scope_local(
        &self,
        scope: *mut libscf_sys::scf_scope_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_handle_get_scope(
                self.handle.as_ptr(),
                libscf_sys::SCF_SCOPE_LOCAL.as_ptr().cast::<i8>(),
                scope,
            )
        })
    }

    pub(crate) unsafe fn scf_decode_fmri_exact_instance(
        &self,
        fmri: *const libc::c_char,
        instance: *mut libscf_sys::scf_instance_t,
    ) -> Result<(), LibscfError> {
        // Require `fmri` to describe exactly an instance.
        let flags = libscf_sys::SCF_DECODE_FMRI_REQUIRE_INSTANCE
            | libscf_sys::SCF_DECODE_FMRI_EXACT;

        LibscfError::from_ret(unsafe {
            libscf_sys::scf_handle_decode_fmri(
                self.handle.as_ptr(),
                fmri,
                std::ptr::null_mut(), // scope
                std::ptr::null_mut(), // service
                instance,
                std::ptr::null_mut(), // property group
                std::ptr::null_mut(), // property
                flags,
            )
        })
    }
}

enum ConnectMode<'a> {
    Global,
    Zone(&'a str),
    #[cfg(any(test, feature = "testing"))]
    DoorPath(&'a str),
}

#[cfg(any(test, feature = "testing"))]
impl<'a> From<&'a IsolatedConfigd> for ConnectMode<'a> {
    fn from(configd: &'a IsolatedConfigd) -> Self {
        Self::DoorPath(configd.door_path().as_str())
    }
}

#[derive(Debug)]
enum RefreshMechanism<'a> {
    // This variant stores a `PhantomData` so we don't get an unused lifetime
    // error in non-test builds, which only have this variant.
    Libscf(PhantomData<&'a ()>),

    #[cfg(any(test, feature = "testing"))]
    Isolated(&'a IsolatedConfigd),
}

impl RefreshMechanism<'_> {
    fn refresh_instance(&self, fmri: &CStr) -> Result<(), RefreshError> {
        match self {
            RefreshMechanism::Libscf(_) => {
                // Per the manpage, `smf_refresh_instance()` still sets an error
                // retrievable via `scf_error()` on failure, so we can use the
                // same error handling as all our other libscf calls.
                let ret =
                    unsafe { libscf_sys::smf_refresh_instance(fmri.as_ptr()) };
                LibscfError::from_ret(ret).map_err(|err| RefreshError::Failed {
                    fmri: String::from_utf8_lossy(fmri.to_bytes())
                        .into_owned()
                        .into_boxed_str(),
                    err,
                })
            }

            #[cfg(any(test, feature = "testing"))]
            RefreshMechanism::Isolated(configd) => {
                use std::ffi::OsStr;
                use std::os::unix::ffi::OsStrExt;

                let fmri = OsStr::from_bytes(fmri.to_bytes());
                configd.refresh(fmri).map_err(From::from)
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod decorations {
    pub const DOOR_PATH: &[u8] = b"door_path\0";
}
