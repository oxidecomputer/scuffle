// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Scf;
use chrono::DateTime;
use chrono::Utc;
use libscf_sys::scf_type_t;
use oxnet::IpNet;
use oxnet::Ipv4Net;
use oxnet::Ipv6Net;
use std::borrow::Cow;
use std::ffi::CString;
use std::ffi::NulError;
use std::fmt;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::ptr::NonNull;

#[derive(Debug, thiserror::Error)]
pub enum ValueError {
    #[error("failed to create value")]
    Create(#[source] LibscfError),

    #[error("failed to set value {value:?} on internal libscf value")]
    Set {
        value: Value<'static>,
        #[source]
        err: LibscfError,
    },

    #[error("invalid string value {value:?}")]
    InvalidString {
        value: String,
        #[source]
        err: NulError,
    },

    #[error(
        "invalid subsecond nanos in timestamp {timestamp} ({seconds}.{nanos})"
    )]
    InvalidTimestampNanos { timestamp: DateTime<Utc>, seconds: i64, nanos: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value<'a> {
    Bool(bool),
    Count(u64),
    Integer(i64),
    Time(DateTime<Utc>),
    AString(Cow<'a, str>),
    Opaque(Cow<'a, [u8]>),
    UString(Cow<'a, str>),
    Uri(Cow<'a, str>),
    Fmri(Cow<'a, str>),
    Host(Cow<'a, str>),
    Hostname(Cow<'a, str>),
    NetAddrV4(Ipv4Addr),
    NetV4(Ipv4Net),
    NetAddrV6(Ipv6Addr),
    NetV6(Ipv6Net),
    NetAddr(IpAddr),
    Net(IpNet),
}

impl<'a> Value<'a> {
    pub fn to_static_value(&self) -> Value<'static> {
        match self {
            Value::Bool(b) => Value::Bool(*b),
            Value::Count(c) => Value::Count(*c),
            Value::Integer(i) => Value::Integer(*i),
            Value::Time(ts) => Value::Time(*ts),
            Value::AString(cow) => {
                Value::AString(Cow::Owned(cow.clone().into_owned()))
            }
            Value::Opaque(cow) => {
                Value::Opaque(Cow::Owned(cow.clone().into_owned()))
            }
            Value::UString(cow) => {
                Value::UString(Cow::Owned(cow.clone().into_owned()))
            }
            Value::Uri(cow) => Value::Uri(Cow::Owned(cow.clone().into_owned())),
            Value::Fmri(cow) => {
                Value::Fmri(Cow::Owned(cow.clone().into_owned()))
            }
            Value::Host(cow) => {
                Value::Host(Cow::Owned(cow.clone().into_owned()))
            }
            Value::Hostname(cow) => {
                Value::Hostname(Cow::Owned(cow.clone().into_owned()))
            }
            Value::NetAddrV4(ip) => Value::NetAddrV4(*ip),
            Value::NetV4(ipnet) => Value::NetV4(*ipnet),
            Value::NetAddrV6(ip) => Value::NetAddrV6(*ip),
            Value::NetV6(ipnet) => Value::NetV6(*ipnet),
            Value::NetAddr(ip) => Value::NetAddr(*ip),
            Value::Net(ipnet) => Value::Net(*ipnet),
        }
    }
}

impl Value<'_> {
    /// Returns a displayable type that formats values consistently with how SMF
    /// would display them as strings (e.g., `Opaque` values are hex-encoded;
    /// `Time` values are `{seconds}.{nanoseconds}`).
    pub fn display_smf(&self) -> ValueDisplaySmf<'_> {
        ValueDisplaySmf(self)
    }
}

pub struct ValueDisplaySmf<'a>(&'a Value<'a>);

impl fmt::Display for ValueDisplaySmf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Value::Bool(b) => b.fmt(f),
            Value::Count(n) => n.fmt(f),
            Value::Integer(i) => i.fmt(f),
            Value::Time(ts) => {
                let secs = ts.timestamp();
                let nanosecs = ts.timestamp_subsec_nanos();
                if nanosecs == 0 {
                    secs.fmt(f)
                } else {
                    write!(f, "{secs}.{nanosecs:09}")
                }
            }
            Value::Opaque(cow) => {
                for b in cow.as_ref() {
                    write!(f, "{b:02x}")?;
                }
                Ok(())
            }
            Value::AString(cow)
            | Value::UString(cow)
            | Value::Uri(cow)
            | Value::Fmri(cow)
            | Value::Host(cow)
            | Value::Hostname(cow) => cow.fmt(f),
            Value::NetAddrV4(ip) => ip.fmt(f),
            Value::NetV4(ip) => ip.fmt(f),
            Value::NetAddrV6(ip) => ip.fmt(f),
            Value::NetV6(ip) => ip.fmt(f),
            Value::NetAddr(ip) => ip.fmt(f),
            Value::Net(ip) => ip.fmt(f),
        }
    }
}

pub(crate) struct ScfValue<'scf> {
    // Phantom data referring to the `Scf` handle within which we were
    // created; this ensures we can't be dropped before that instance.
    _scf: PhantomData<&'scf ()>,
    handle: NonNull<libscf_sys::scf_value_t>,
}

impl Drop for ScfValue<'_> {
    fn drop(&mut self) {
        unsafe { libscf_sys::scf_value_destroy(self.handle.as_ptr()) };
    }
}

impl<'scf> ScfValue<'scf> {
    pub(crate) fn new(scf: &'scf Scf) -> Result<Self, ValueError> {
        let value =
            unsafe { libscf_sys::scf_value_create(scf.handle().as_ptr()) };
        let value = LibscfError::from_ptr(value).map_err(ValueError::Create)?;
        Ok(Self { _scf: PhantomData, handle: value })
    }
}

impl ScfValue<'_> {
    pub(crate) fn handle(&self) -> &NonNull<libscf_sys::scf_value_t> {
        &self.handle
    }

    pub(crate) fn set(&mut self, value: &Value<'_>) -> Result<(), ValueError> {
        // Wrapper around `libscf_sys::scf_value_set_from_string()` used by many
        // of the variants of `Value` in the match below.
        //
        // The return type is slightly awkward to be consistent with the other
        // arms of that match - we'll `?` our way out of the outer Result inside
        // the match arm, then handle the inner Result just before returning.
        fn set_from_string(
            ptr: *mut libscf_sys::scf_value_t,
            ty: scf_type_t,
            s: &str,
        ) -> Result<Result<(), LibscfError>, ValueError> {
            let s = CString::new(s).map_err(|err| {
                ValueError::InvalidString { value: s.to_owned(), err }
            })?;
            let ret = unsafe {
                libscf_sys::scf_value_set_from_string(ptr, ty, s.as_ptr())
            };
            Ok(LibscfError::from_ret(ret))
        }

        // Store `value` inside `self.value` via the correct libscf function.
        // Some of these are infallible; for those, we assign the result to `()`
        // (statically guaranteeing it's actually infallible).
        let result = {
            use scf_type_t::*;
            let ptr = self.handle.as_ptr();
            match value {
                Value::Bool(b) => {
                    () = unsafe {
                        libscf_sys::scf_value_set_boolean(ptr, u8::from(*b))
                    };
                    Ok(())
                }
                Value::Count(n) => {
                    () = unsafe { libscf_sys::scf_value_set_count(ptr, *n) };
                    Ok(())
                }
                Value::Integer(i) => {
                    () = unsafe { libscf_sys::scf_value_set_integer(ptr, *i) };
                    Ok(())
                }
                Value::Time(ts) => {
                    let seconds = ts.timestamp();
                    let nanos = ts.timestamp_subsec_nanos();
                    let nanos = i32::try_from(nanos).map_err(|_| {
                        ValueError::InvalidTimestampNanos {
                            timestamp: *ts,
                            seconds,
                            nanos,
                        }
                    })?;
                    LibscfError::from_ret(unsafe {
                        libscf_sys::scf_value_set_time(ptr, seconds, nanos)
                    })
                }
                Value::Opaque(cow) => LibscfError::from_ret(unsafe {
                    libscf_sys::scf_value_set_opaque(
                        ptr,
                        cow.as_ptr().cast::<libc::c_void>(),
                        cow.len(),
                    )
                }),
                // TODO-correctness There are explicit functions for setting
                // AString and UString values, respectively, but the libscf_sys
                // bindings are incorrect, so instead we to through "set from
                // string" for them like the remainder of the fancy types.
                // See <https://github.com/illumos/libscf-sys/issues/1>.
                Value::AString(cow) => {
                    set_from_string(ptr, SCF_TYPE_ASTRING, cow)?
                }
                Value::UString(cow) => {
                    set_from_string(ptr, SCF_TYPE_USTRING, cow)?
                }
                Value::Uri(cow) => set_from_string(ptr, SCF_TYPE_URI, cow)?,
                Value::Fmri(cow) => set_from_string(ptr, SCF_TYPE_FMRI, cow)?,
                Value::Host(cow) => set_from_string(ptr, SCF_TYPE_HOST, cow)?,
                Value::Hostname(cow) => {
                    set_from_string(ptr, SCF_TYPE_HOSTNAME, cow)?
                }
                Value::NetAddrV4(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V4, &ip.to_string())?
                }
                Value::NetV4(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V4, &ip.to_string())?
                }
                Value::NetAddrV6(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V6, &ip.to_string())?
                }
                Value::NetV6(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V6, &ip.to_string())?
                }
                Value::NetAddr(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR, &ip.to_string())?
                }
                Value::Net(ipnet) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR, &ipnet.to_string())?
                }
            }
        };
        result.map_err(|err| ValueError::Set {
            value: value.to_static_value(),
            err,
        })
    }
}
