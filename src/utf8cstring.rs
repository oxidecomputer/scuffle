// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::fmt;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::NulError;

pub(crate) struct Utf8CString(CString);

impl Utf8CString {
    pub(crate) fn new(s: &str) -> Result<Self, NulError> {
        CString::new(s).map(Self)
    }

    pub(crate) fn as_c_str(&self) -> &CStr {
        &self.0
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.to_str().expect("CString created from &str can go back to &str")
    }
}

impl fmt::Display for Utf8CString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}
