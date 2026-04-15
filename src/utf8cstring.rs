// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::fmt;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::NulError;
use std::str::Utf8Error;

#[derive(Debug)]
pub(crate) struct Utf8CString(CString);

impl Utf8CString {
    fn from_string(s: String) -> Result<Self, NulError> {
        CString::new(s).map(Self)
    }

    pub(crate) fn from_str(s: &str) -> Result<Self, NulError> {
        CString::new(s).map(Self)
    }

    pub(crate) fn from_c_str(s: &CStr) -> Result<Self, Utf8Error> {
        // Ensure `s` contains a legal UTF8 string.
        let _s: &str = s.to_str()?;
        Ok(Self(s.to_owned()))
    }

    pub(crate) fn as_c_str(&self) -> &CStr {
        &self.0
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.to_str().expect("Utf8CString always contains valid UTF8")
    }

    pub(crate) fn into_string(self) -> String {
        self.0.into_string().expect("Utf8CString always contains valid UTF8")
    }
}

impl fmt::Display for Utf8CString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

// These are taken from `libscf_priv.h`, but we omit everything related to
// scoped (this crate only supports the local scope).
const SVC_PREFIX: &str = "svc:/";
const INSTANCE_PREFIX: &str = ":";
const PG_PREFIX: &str = "/:properties/";
const PROP_PREFIX: &str = "/";

#[derive(Debug)]
pub(crate) struct ServiceFmri(Utf8CString);

#[derive(Debug)]
pub(crate) struct InstanceFmri(Utf8CString);

#[derive(Debug)]
pub(crate) struct PropertyGroupFmri(Utf8CString);

#[derive(Debug)]
pub(crate) struct PropertyFmri(Utf8CString);

impl ServiceFmri {
    pub(crate) fn new(name: &Utf8CString) -> Self {
        let fmri = format!("{SVC_PREFIX}{name}");
        Self(Utf8CString::from_string(fmri).expect("string is still valid"))
    }

    pub(crate) fn append_instance(&self, name: &Utf8CString) -> InstanceFmri {
        let fmri = format!("{self}{INSTANCE_PREFIX}{name}");
        InstanceFmri(
            Utf8CString::from_string(fmri).expect("string is still valid"),
        )
    }

    pub(crate) fn append_pg(&self, name: &Utf8CString) -> PropertyGroupFmri {
        let fmri = format!("{self}{PG_PREFIX}{name}");
        PropertyGroupFmri(
            Utf8CString::from_string(fmri).expect("string is still valid"),
        )
    }
}

impl InstanceFmri {
    pub(crate) fn append_pg(&self, name: &Utf8CString) -> PropertyGroupFmri {
        let fmri = format!("{self}{PG_PREFIX}{name}");
        PropertyGroupFmri(
            Utf8CString::from_string(fmri).expect("string is still valid"),
        )
    }
}

impl PropertyGroupFmri {
    pub(crate) fn append_property(&self, name: &Utf8CString) -> PropertyFmri {
        let fmri = format!("{self}{PROP_PREFIX}{name}");
        PropertyFmri(
            Utf8CString::from_string(fmri).expect("string is still valid"),
        )
    }
}

macro_rules! impl_wrapper {
    ($type:ident) => {
        impl std::ops::Deref for $type {
            type Target = Utf8CString;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

impl_wrapper!(ServiceFmri);
impl_wrapper!(InstanceFmri);
impl_wrapper!(PropertyGroupFmri);
impl_wrapper!(PropertyFmri);
