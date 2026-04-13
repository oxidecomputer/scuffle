// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Scope;
use crate::ValueRef;
use crate::error::LibscfError;
use crate::error::RefreshError;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone<'a> {
    Global,
    NonGlobal(&'a str),
}

// We intentionally do not impl `Send` or `Sync` for `Scf`. Errors flow out
// through the thread-local `scf_error()` function, so we don't want to mix
// use of the same handle across different threads.
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
    pub fn connect(zone: Zone<'_>) -> Result<Self, ScfError> {
        Self::connect_common(
            ConnectMode::from(zone),
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
        let handle =
            LibscfError::from_ptr(handle).map_err(ScfError::HandleCreate)?;

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
                let mut value = ScfValue::new(&scf).map_err(|err| {
                    ScfError::CreateZoneName {
                        zonename: zonename.to_owned(),
                        err,
                    }
                })?;
                value.set(ValueRef::AString(zonename)).map_err(|err| {
                    ScfError::SetZoneName { zonename: zonename.to_owned(), err }
                })?;
                unsafe {
                    value.scf_apply_as_decoration(
                        scf.handle.as_ptr(),
                        libscf_sys::decorations::ZONE.as_ptr().cast::<i8>(),
                    )
                }
                .map_err(|err| {
                    ScfError::SetDecorationZoneName {
                        zonename: zonename.to_owned(),
                        err,
                    }
                })?;
            }

            #[cfg(any(test, feature = "testing"))]
            ConnectMode::DoorPath(door_path) => {
                let mut value = ScfValue::new(&scf).map_err(|err| {
                    ScfError::CreateDoorPath {
                        door_path: door_path.to_owned(),
                        err,
                    }
                })?;
                value.set(ValueRef::AString(door_path)).map_err(|err| {
                    ScfError::SetDoorPath {
                        door_path: door_path.to_owned(),
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
                        door_path: door_path.to_owned(),
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

    pub fn refresh(&self, fmri: &str) -> Result<(), RefreshError> {
        let fmri = CString::new(fmri).map_err(|err| {
            RefreshError::InvalidFmri { fmri: fmri.to_owned(), err }
        })?;
        self.refresh_cstr(&fmri)
    }

    pub(crate) fn refresh_cstr(&self, fmri: &CStr) -> Result<(), RefreshError> {
        self.refresher.refresh(fmri)
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
}

enum ConnectMode<'a> {
    Global,
    Zone(&'a str),
    #[cfg(any(test, feature = "testing"))]
    DoorPath(&'a str),
}

impl<'a> From<Zone<'a>> for ConnectMode<'a> {
    fn from(zone: Zone<'a>) -> Self {
        match zone {
            Zone::Global => Self::Global,
            Zone::NonGlobal(z) => Self::Zone(z),
        }
    }
}

#[cfg(any(test, feature = "testing"))]
impl<'a> From<&'a IsolatedConfigd> for ConnectMode<'a> {
    fn from(configd: &'a IsolatedConfigd) -> Self {
        Self::DoorPath(configd.door_path().as_str())
    }
}

enum RefreshMechanism<'a> {
    // This variant stores a `PhantomData` so we don't get an unused lifetime
    // error in non-test builds, which only have this variant.
    Libscf(PhantomData<&'a ()>),

    #[cfg(any(test, feature = "testing"))]
    Isolated(&'a IsolatedConfigd),
}

impl RefreshMechanism<'_> {
    fn refresh(&self, fmri: &CStr) -> Result<(), RefreshError> {
        match self {
            RefreshMechanism::Libscf(_) => {
                // Per the manpage, `smf_refresh_instance()` still sets an error
                // retrievable via `scf_error()` on failure, so we can use the
                // same error handling as all our other libscf calls.
                let ret =
                    unsafe { libscf_sys::smf_refresh_instance(fmri.as_ptr()) };
                LibscfError::from_ret(ret).map_err(|err| RefreshError::Failed {
                    fmri: String::from_utf8_lossy(fmri.to_bytes()).into_owned(),
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
