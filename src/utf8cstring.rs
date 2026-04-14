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
