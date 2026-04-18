// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::AddPropertyGroupFlags;
use crate::EditPropertyGroups;
use crate::HasComposedPropertyGroups;
use crate::HasDirectPropertyGroups;
use crate::PropertyGroup;
use crate::PropertyGroupComposed;
use crate::PropertyGroupDirect;
use crate::PropertyGroupType;
use crate::PropertyGroups;
use crate::Scf;
use crate::Service;
use crate::Snapshot;
use crate::Snapshots;
use crate::buf::scf_get_name;
use crate::edit_property_groups::AddPropertyGroupArgs;
use crate::error::InstanceFromFmriError;
use crate::error::InstanceOp;
use crate::error::InstanceOpError;
use crate::error::IterError;
use crate::error::IterErrorKind;
use crate::error::LibscfError;
use crate::error::LookupError;
use crate::error::PropertyGroupAddError;
use crate::error::ScfEntity;
use crate::error::ScfEntityDescription;
use crate::error::ToEntityDescription;
use crate::iter::ScfIter;
use crate::iter::ScfUninitializedIter;
use crate::scf::ScfObject;
use crate::utf8cstring::InstanceFmri;
use crate::utf8cstring::PropertyGroupFmri;
use crate::utf8cstring::Utf8CString;

#[cfg(not(feature = "smf-by-instance"))]
use crate::libscf_sys_priv;

/// Handle to an SMF instance.
///
/// Instances may be obtained by way of their parent service
/// ([`Service::instance()`]), by direct-FMRI lookup
/// ([`Scf::instance_from_fmri()`]), or by direct-FMRI lookup for the current
/// process assuming it is running under SMF
/// ([`Scf::self_instance_from_env()`]).
///
/// For processes that want to read their own effective configuration, the
/// recommended path is to obtain the self instance via
/// [`Scf::self_instance_from_env()`] and then obtain the running snapshot via
/// [`Instance::snapshot("running")`].
///
/// [`Instance::snapshot("running")`]: Instance::snapshot
#[derive(Debug)]
pub struct Instance<'a> {
    name: Utf8CString,
    fmri: InstanceFmri,
    handle: ScfObject<'a, libscf_sys::scf_instance_t>,
}

impl<'a> Instance<'a> {
    pub(crate) fn from_service(
        service: &'a Service<'a>,
        name: &str,
    ) -> Result<Option<Self>, LookupError> {
        let name = Utf8CString::from_str(name).map_err(|err| {
            LookupError::InvalidName {
                entity: ScfEntity::Instance,
                name: name.to_string().into_boxed_str(),
                err,
            }
        })?;

        let fmri = service.instance_fmri(&name);

        let mut handle = service.scf().scf_instance_create()?;

        let result = unsafe {
            service
                .scf_get_instance(name.as_c_str().as_ptr(), handle.as_mut_ptr())
        };

        match result {
            Ok(()) => Ok(Some(Self { name, fmri, handle })),
            Err(LibscfError::NotFound) => Ok(None),
            Err(err) => Err(LookupError::Get {
                entity: ScfEntity::Instance,
                parent: service.to_entity_description(),
                name: name.into_string().into_boxed_str(),
                err,
            }),
        }
    }

    pub(crate) fn from_fmri(
        scf: &'a Scf<'a>,
        fmri: &str,
    ) -> Result<Self, InstanceFromFmriError> {
        let fmri = Utf8CString::from_str(fmri).map_err(|err| {
            InstanceFromFmriError::InvalidFmri {
                fmri: fmri.to_string().into_boxed_str(),
                err,
            }
        })?;

        let mut handle = scf.scf_instance_create()?;
        () = unsafe {
            scf.scf_decode_fmri_exact_instance(
                fmri.as_c_str().as_ptr(),
                handle.as_mut_ptr(),
            )
        }
        .map_err(|err| InstanceFromFmriError::Get {
            fmri: fmri.to_string().into_boxed_str(),
            err,
        })?;

        // On success, we now know `fmri` is a valid instance FMRI.
        let fmri = InstanceFmri::new_unvalidated(fmri);

        // Given an `InstanceFmri`, we could attempt to parse the name of
        // the instance out ourself, but it's more straightforward to just ask
        // scf. This is very unlikely to fail given we just succeeded in looking
        // up the instance handle.
        let name = scf_get_name(|buf, buf_len| unsafe {
            libscf_sys::scf_instance_get_name(handle.as_ptr(), buf, buf_len)
        })
        .map_err(|err| InstanceFromFmriError::GetName {
            fmri: fmri.to_string().into_boxed_str(),
            err,
        })?;

        Ok(Self { name, fmri, handle })
    }

    pub(crate) fn scf(&self) -> &'a Scf<'a> {
        self.handle.scf()
    }

    pub(crate) unsafe fn scf_get_snapshot(
        &self,
        name: *const i8,
        snapshot: *mut libscf_sys::scf_snapshot_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_get_snapshot(
                self.handle.as_ptr(),
                name,
                snapshot,
            )
        })
    }

    pub(crate) unsafe fn scf_get_pg(
        &self,
        name: *const i8,
        pg: *mut libscf_sys::scf_propertygroup_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_get_pg(self.handle.as_ptr(), name, pg)
        })
    }

    pub(crate) unsafe fn scf_get_pg_composed(
        &self,
        snapshot: *const libscf_sys::scf_snapshot_t,
        name: *const i8,
        pg: *mut libscf_sys::scf_propertygroup_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_get_pg_composed(
                self.handle.as_ptr(),
                snapshot,
                name,
                pg,
            )
        })
    }

    pub(crate) unsafe fn scf_iter_pgs_composed(
        &self,
        snapshot: *const libscf_sys::scf_snapshot_t,
    ) -> Result<ScfIter<'_, libscf_sys::scf_propertygroup_t>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf())?;
        unsafe {
            iter.init_instance_property_groups_composed(
                self.handle.as_ptr(),
                snapshot,
            )
        }
        .map_err(|err| IterError::Iter {
            entity: ScfEntity::PropertyGroup,
            parent: self.to_entity_description(),
            kind: IterErrorKind::Init(err),
        })
    }

    /// The name of this instance.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The full FMRI of this instance.
    pub fn fmri(&self) -> &str {
        self.fmri.as_str()
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn fmri_c_str(&self) -> &std::ffi::CStr {
        self.fmri.as_c_str()
    }

    /// Refresh this instance.
    ///
    /// This is equivalent to running `svcadm refresh THIS_INSTANCE`; i.e.,
    /// it will both update the `"running"` snapshot to match any property
    /// changes made since the last time the instance was refreshed and will
    /// invoke the instance's SMF `refresh` method.
    pub fn smf_refresh(&mut self) -> Result<(), InstanceOpError> {
        self.scf().refresh_instance(self)
    }

    // Without the new functions gated by `smf-by-instance`, we refresh via a
    // private API that is also used by `svccfg`.
    #[cfg(not(feature = "smf-by-instance"))]
    pub(crate) fn scf_refresh(&mut self) -> Result<(), InstanceOpError> {
        LibscfError::from_ret(unsafe {
            libscf_sys_priv::_smf_refresh_instance_i(self.handle.as_mut_ptr())
        })
        .map_err(|err| InstanceOpError::Failed {
            op: InstanceOp::Refresh,
            fmri: self.fmri().to_string().into_boxed_str(),
            err,
        })
    }

    pub(crate) fn property_group_fmri(
        &self,
        name: &Utf8CString,
    ) -> PropertyGroupFmri {
        self.fmri.append_pg(name)
    }

    /// Look up a snapshot in this instance by name.
    pub fn snapshot(
        &self,
        name: &str,
    ) -> Result<Option<Snapshot<'_>>, LookupError> {
        Snapshot::new(self, name)
    }

    /// Get an iterator over all [`Snapshot`]s in this instance.
    pub fn snapshots(&self) -> Result<Snapshots<'_>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf())?;
        let iter =
            unsafe { iter.init_instance_snapshots(self.handle.as_ptr()) }
                .map_err(|err| IterError::Iter {
                    entity: ScfEntity::Snapshot,
                    parent: self.to_entity_description(),
                    kind: IterErrorKind::Init(err),
                })?;
        Ok(Snapshots::new(self, iter))
    }
}

impl EditPropertyGroups for Instance<'_> {
    fn add_property_group(
        &mut self,
        name: &str,
        pg_type: PropertyGroupType,
        flags: AddPropertyGroupFlags,
    ) -> Result<PropertyGroup<'_, PropertyGroupDirect>, PropertyGroupAddError>
    {
        let AddPropertyGroupArgs { name, mut handle, flags } =
            AddPropertyGroupArgs::validate(self.scf(), self, name, flags)?;
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_instance_add_pg(
                self.handle.as_mut_ptr(),
                name.as_c_str().as_ptr(),
                pg_type.as_c_str().as_ptr(),
                flags,
                handle.as_mut_ptr(),
            )
        })
        .map_err(|err| PropertyGroupAddError::Add {
            parent: self.to_entity_description(),
            name: name.to_string().into_boxed_str(),
            err,
        })?;

        Ok(PropertyGroup::from_instance_add_pg(self, name, handle))
    }
}

impl HasDirectPropertyGroups for Instance<'_> {
    fn property_group_direct(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupDirect>>, LookupError>
    {
        PropertyGroup::from_instance(self, name)
    }

    fn property_groups_direct(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupDirect>, IterError> {
        let iter = ScfUninitializedIter::new(self.scf())?;
        let iter =
            unsafe { iter.init_instance_property_groups(self.handle.as_ptr()) }
                .map_err(|err| IterError::Iter {
                    entity: ScfEntity::PropertyGroup,
                    parent: self.to_entity_description(),
                    kind: IterErrorKind::Init(err),
                })?;
        Ok(PropertyGroups::from_instance(self, iter))
    }
}

impl HasComposedPropertyGroups for Instance<'_> {
    fn property_group_composed(
        &self,
        name: &str,
    ) -> Result<Option<PropertyGroup<'_, PropertyGroupComposed>>, LookupError>
    {
        PropertyGroup::from_instance_composed(self, name)
    }

    fn property_groups_composed(
        &self,
    ) -> Result<PropertyGroups<'_, PropertyGroupComposed>, IterError> {
        let iter = unsafe {
            self.scf_iter_pgs_composed(
                // no snapshot; compose instance -> service only
                std::ptr::null(),
            )
        }?;
        Ok(PropertyGroups::from_instance_composed(self, iter))
    }
}

impl ToEntityDescription for Instance<'_> {
    fn to_entity_description(&self) -> ScfEntityDescription {
        ScfEntityDescription::Instance {
            fmri: self.fmri().to_string().into_boxed_str(),
        }
    }
}

/// Iterator over all [`Instance`]s in a [`Service`].
///
/// Obtained via [`Service::instances()`].
pub struct Instances<'a> {
    service: &'a Service<'a>,
    iter: ScfIter<'a, libscf_sys::scf_instance_t>,
}

impl<'a> Instances<'a> {
    pub(crate) fn new(
        service: &'a Service<'a>,
        iter: ScfIter<'a, libscf_sys::scf_instance_t>,
    ) -> Self {
        Self { service, iter }
    }
}

impl<'a> Iterator for Instances<'a> {
    type Item = Result<Instance<'a>, IterError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next_named(self.service, || {
                self.service.scf().scf_instance_create()
            })
            .map(|result| {
                result.map(|(name, handle)| Instance {
                    fmri: self.service.instance_fmri(&name),
                    name,
                    handle,
                })
            })
    }
}

#[cfg(feature = "smf-by-instance")]
pub use smf_by_instance::*;

#[cfg(feature = "smf-by-instance")]
mod smf_by_instance {
    use super::*;
    use crate::error::InstanceSmfStateError;
    use crate::libscf_sys_supplemental;
    use bitflags::bitflags;
    use std::ffi::CStr;
    use std::ptr::NonNull;

    bitflags! {
        /// Optional flags for putting an SMF instance into maintenance.
        ///
        /// These flags can be combined via bitwise-or (`|`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct SmfMaintainFlags: libc::c_int {
            const Immediate = libscf_sys::SMF_IMMEDIATE;
            const Temporary = libscf_sys::SMF_TEMPORARY;
        }
    }

    /// Optional flags for enabling or disabling SMF instances.
    ///
    /// These flags are mutually exclusive.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[non_exhaustive] // leave the door open for libscf to add more flags
    pub enum SmfEnableDisableFlags {
        Temporary,
        AtNextBoot,
    }

    impl SmfEnableDisableFlags {
        // These aren't bits in the `bitflags`-sense, because they're mutually
        // exclusive, but we reuse that name so we can pass any of these flag
        // types into the `smf_operation!` macro below.
        fn bits(self) -> libc::c_int {
            match self {
                Self::Temporary => libscf_sys::SMF_TEMPORARY,
                Self::AtNextBoot => libscf_sys::SMF_AT_NEXT_BOOT,
            }
        }
    }

    /// Optional flags for putting an SMF instance into the degraded state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[non_exhaustive] // leave the door open for libscf to add more flags
    pub enum SmfDegradeFlags {
        Immediate,
    }

    impl SmfDegradeFlags {
        // These aren't bits in the `bitflags`-sense, because there's only one
        // option here, but we reuse that name so we can pass any of these flag
        // types into the `smf_operation!` macro below.
        fn bits(self) -> libc::c_int {
            match self {
                Self::Immediate => libscf_sys::SMF_IMMEDIATE,
            }
        }
    }

    macro_rules! smf_operation {
        ($comment:literal, $op:ident, $our_name:ident, $scf_name:ident) => {
            #[doc = $comment]
            #[doc = "\n\n"]
            #[doc = "# Errors\n\n"]
            #[doc = "Fails if called under an `IsolatedConfigd` testing setup."]
            pub fn $our_name(&mut self) -> Result<(), InstanceOpError> {
                self.scf().fail_instance_op_if_isolated_configd()?;
                LibscfError::from_ret(unsafe {
                    libscf_sys_supplemental::$scf_name(self.handle.as_mut_ptr())
                })
                .map_err(|err| InstanceOpError::Failed {
                    op: InstanceOp::$op,
                    fmri: self.fmri().to_string().into_boxed_str(),
                    err,
                })
            }
        };

        (
            $comment:literal,
            $op:ident,
            $our_name:ident,
            $scf_name:ident,
            $flags:ty,
        ) => {
            #[doc = $comment]
            #[doc = "\n\n"]
            #[doc = "# Errors\n\n"]
            #[doc = "Fails if called under an `IsolatedConfigd` testing setup."]
            pub fn $our_name(
                &mut self,
                flags: Option<$flags>,
            ) -> Result<(), InstanceOpError> {
                self.scf().fail_instance_op_if_isolated_configd()?;
                let flags = flags.map_or(0, |f| f.bits());
                LibscfError::from_ret(unsafe {
                    libscf_sys_supplemental::$scf_name(
                        self.handle.as_mut_ptr(),
                        flags,
                    )
                })
                .map_err(|err| InstanceOpError::Failed {
                    op: InstanceOp::$op,
                    fmri: self.fmri().to_string().into_boxed_str(),
                    err,
                })
            }
        };

        (
            $comment:literal,
            $op:ident,
            $our_name:ident,
            $scf_name:ident,
            $flags:ty,
            TAKES_COMMENT
        ) => {
            #[doc = $comment]
            #[doc = "\n\n"]
            #[doc = "# Errors\n\n"]
            #[doc = "Fails if called under an `IsolatedConfigd` testing setup."]
            pub fn $our_name(
                &mut self,
                flags: Option<$flags>,
                comment: Option<&str>,
            ) -> Result<(), InstanceOpError> {
                self.scf().fail_instance_op_if_isolated_configd()?;
                let comment = match comment {
                    Some(s) => {
                        let s = Utf8CString::from_str(s).map_err(|err| {
                            InstanceOpError::InvalidComment {
                                comment: s.to_string().into_boxed_str(),
                                err,
                            }
                        })?;
                        let c_str_len = s.as_c_str().to_bytes_with_nul().len();
                        if c_str_len
                            > libscf_sys_supplemental::SCF_COMMENT_MAX_LENGTH
                        {
                            return Err(InstanceOpError::CommentTooLong {
                                c_str_len,
                                max_len: libscf_sys_supplemental::SCF_COMMENT_MAX_LENGTH,
                                comment: s.into_string().into_boxed_str(),
                            });
                        }
                        Some(s)
                    }
                    None => None,
                };

                let flags = flags.map_or(0, |f| f.bits());
                let result = LibscfError::from_ret(unsafe {
                    libscf_sys_supplemental::$scf_name(
                        self.handle.as_mut_ptr(),
                        flags,
                        comment.as_ref().map_or(
                            std::ptr::null(),
                            |s| s.as_c_str().as_ptr(),
                        ),
                    )
                })
                .map_err(|err| InstanceOpError::Failed {
                    op: InstanceOp::$op,
                    fmri: self.fmri().to_string().into_boxed_str(),
                    err,
                });

                // Keep `comment` alive across the FFI call above.
                std::mem::drop(comment);

                result
            }
        };
    }

    impl Instance<'_> {
        pub(crate) fn scf_refresh(&mut self) -> Result<(), InstanceOpError> {
            LibscfError::from_ret(unsafe {
                libscf_sys_supplemental::smf_refresh_instance_by_instance(
                    self.handle.as_mut_ptr(),
                )
            })
            .map_err(|err| InstanceOpError::Failed {
                op: InstanceOp::Refresh,
                fmri: self.fmri().to_string().into_boxed_str(),
                err,
            })
        }

        /// Get the current SMF state of this instance.
        pub fn smf_state(&mut self) -> Result<String, InstanceSmfStateError> {
            // `smf_get_state_by_instance()` returns a malloc'd string we're
            // responsible for freeing; immediately wrap the pointer in a type
            // that will free on drop.
            struct FreeOnDrop(NonNull<libc::c_char>);
            impl Drop for FreeOnDrop {
                fn drop(&mut self) {
                    unsafe {
                        libc::free(self.0.as_ptr().cast::<libc::c_void>())
                    };
                }
            }

            let state = match NonNull::new(unsafe {
                libscf_sys_supplemental::smf_get_state_by_instance(
                    self.handle.as_mut_ptr(),
                )
            }) {
                Some(state) => FreeOnDrop(state),
                None => {
                    return Err(InstanceSmfStateError::GetState {
                        fmri: self.fmri().to_string().into_boxed_str(),
                        err: LibscfError::last(),
                    });
                }
            };

            let state_c_str = unsafe { CStr::from_ptr(state.0.as_ptr()) };
            let state_str = state_c_str.to_str().map_err(|err| {
                InstanceSmfStateError::NonUtf8State {
                    fmri: self.fmri().to_string().into_boxed_str(),
                    state: Box::from(state_c_str),
                    err,
                }
            })?;

            Ok(state_str.to_owned())
        }

        smf_operation!(
            "Put this instance into the degraded state.",
            Degrade,
            smf_degrade,
            smf_degrade_instance_by_instance,
            SmfDegradeFlags,
        );
        smf_operation!(
            "Put this instance into the disabled state.",
            Disable,
            smf_disable,
            smf_disable_instance_by_instance,
            SmfEnableDisableFlags,
            TAKES_COMMENT
        );
        smf_operation!(
            "Put this instance into the enabled state.",
            Enable,
            smf_enable,
            smf_enable_instance_by_instance,
            SmfEnableDisableFlags,
            TAKES_COMMENT
        );
        smf_operation!(
            "Put this instance into the maintenance state.",
            Maintain,
            smf_maintain,
            smf_maintain_instance_by_instance,
            SmfMaintainFlags,
        );
        smf_operation!(
            "Restart this instance.",
            Restart,
            smf_restart,
            smf_restart_instance_by_instance
        );
        smf_operation!(
            "Restore this instance.",
            Restore,
            smf_restore,
            smf_restore_instance_by_instance
        );
    }
}
