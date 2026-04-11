// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::LibscfError;
use crate::Scf;
use chrono::DateTime;
use chrono::Utc;
use libscf_sys::scf_type_t;
use num_traits::FromPrimitive;
use oxnet::IpNet;
use oxnet::Ipv4Net;
use oxnet::Ipv6Net;
use std::borrow::Cow;
use std::cell::Cell;
use std::cell::RefCell;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::FromBytesWithNulError;
use std::ffi::NulError;
use std::fmt;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;
use std::ptr::NonNull;
use std::str::Utf8Error;

#[derive(Debug, thiserror::Error)]
#[error("failed to create value")]
pub struct CreateValueError(#[source] pub LibscfError);

#[derive(Debug, thiserror::Error)]
pub enum SetValueError {
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
        "invalid subsecond nanos in timestamp {timestamp} ({seconds}.{nanos:09})"
    )]
    InvalidTimestampNanos { timestamp: DateTime<Utc>, seconds: i64, nanos: u32 },
}

#[derive(Debug, thiserror::Error)]
pub enum GetValueError {
    #[error("unexpected scf type value: {0}")]
    UnexpectedTypeValue(i32),

    #[error("value is invalid")]
    Invalid(#[source] LibscfError),

    #[error("error getting value as boolean")]
    GetBool(#[source] LibscfError),

    #[error("error getting value as count")]
    GetCount(#[source] LibscfError),

    #[error("error getting value as integer")]
    GetInteger(#[source] LibscfError),

    #[error("error getting value as time")]
    GetTime(#[source] LibscfError),

    #[error("timestamp value from scf is invalid: {secs}.{nanos:09}")]
    InvalidTime { secs: i64, nanos: i32 },

    #[error("error getting value as opaque")]
    GetOpaque(#[source] LibscfError),

    #[error("error getting value as opaque: got out of bounds length {0}")]
    GetOpaqueOutOfBounds(usize),

    #[error("error getting value as string")]
    GetAsString(#[source] LibscfError),

    #[error("error getting value as string: got out of bounds length {0}")]
    GetAsStringOutOfBounds(usize),

    #[error("error getting value as string: invalid C string")]
    GetAsStringInvalidCStr(#[from] FromBytesWithNulError),

    #[error("error getting value as string: not UTF8")]
    GetAsStringNotUtf8(#[from] Utf8Error),

    #[error("invalid net address v4 value: {0}")]
    InvalidNetAddrV4(String),

    #[error("invalid net address v6 value: {0}")]
    InvalidNetAddrV6(String),

    #[error("invalid net address value: {0}")]
    InvalidNetAddr(String),
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
    pub(crate) fn new(scf: &'scf Scf) -> Result<Self, CreateValueError> {
        let value =
            unsafe { libscf_sys::scf_value_create(scf.handle().as_ptr()) };
        let value = LibscfError::from_ptr(value).map_err(CreateValueError)?;
        Ok(Self { _scf: PhantomData, handle: value })
    }
}

impl ScfValue<'_> {
    pub(crate) fn handle(&self) -> &NonNull<libscf_sys::scf_value_t> {
        &self.handle
    }

    pub(crate) fn get(&self) -> Result<Value<'static>, GetValueError> {
        // Helper function for all the scf types for which we have to fetch
        // strings (and possibly then do additional parsing). This handles
        // extracting a Rust string from what libscf writes to `buf`.
        fn get_as_string(
            ptr: *mut libscf_sys::scf_value_t,
            buf: &mut Vec<u8>,
        ) -> Result<&str, GetValueError> {
            let sz = LibscfError::from_ssize(unsafe {
                libscf_sys::scf_value_get_as_string(
                    ptr,
                    buf.as_mut_ptr().cast::<libc::c_char>(),
                    buf.len(),
                )
            })
            .map_err(GetValueError::GetAsString)?;

            // per the manpage, `sz` is equivalent to `strlen(s)` of the
            // returned string, so we need to add 1 to pick up the Nul byte
            // before constructing a `CStr`.
            if sz + 1 > buf.len() {
                return Err(GetValueError::GetAsStringOutOfBounds(sz));
            }
            let cstr = CStr::from_bytes_with_nul(&buf[..sz + 1])?;

            Ok(cstr.to_str()?)
        }

        let ptr = self.handle.as_ptr();

        let ret = unsafe { libscf_sys::scf_value_type(ptr) };
        let scf_value_type_err = LibscfError::last();
        let Some(ty) = scf_type_t::from_i32(ret) else {
            return Err(GetValueError::UnexpectedTypeValue(ret));
        };
        match ty {
            scf_type_t::SCF_TYPE_INVALID => {
                // Per the man page, `scf_value_type()` sets
                // `LibscfError::last()` when it returns SCF_TYPE_INVALID. We
                // captured `LibscfError::last()` immediately after calling
                // `scf_value_type` above, but only now know it's meaningful.
                Err(GetValueError::Invalid(scf_value_type_err))
            }
            scf_type_t::SCF_TYPE_BOOLEAN => {
                let mut b = 0;
                LibscfError::from_ret(unsafe {
                    libscf_sys::scf_value_get_boolean(ptr, &mut b)
                })
                .map_err(GetValueError::GetBool)?;
                Ok(Value::Bool(b != 0))
            }
            scf_type_t::SCF_TYPE_COUNT => {
                let mut n = 0;
                LibscfError::from_ret(unsafe {
                    libscf_sys::scf_value_get_count(ptr, &mut n)
                })
                .map_err(GetValueError::GetCount)?;
                Ok(Value::Count(n))
            }
            scf_type_t::SCF_TYPE_INTEGER => {
                let mut i = 0;
                LibscfError::from_ret(unsafe {
                    libscf_sys::scf_value_get_integer(ptr, &mut i)
                })
                .map_err(GetValueError::GetInteger)?;
                Ok(Value::Integer(i))
            }
            scf_type_t::SCF_TYPE_TIME => {
                let mut secs = 0;
                let mut nanos = 0;
                LibscfError::from_ret(unsafe {
                    libscf_sys::scf_value_get_time(ptr, &mut secs, &mut nanos)
                })
                .map_err(GetValueError::GetTime)?;
                let nanos_u32 = u32::try_from(nanos)
                    .map_err(|_| GetValueError::InvalidTime { secs, nanos })?;
                let ts = DateTime::from_timestamp(secs, nanos_u32)
                    .ok_or(GetValueError::InvalidTime { secs, nanos })?;
                Ok(Value::Time(ts))
            }
            scf_type_t::SCF_TYPE_OPAQUE => with_scf_value_buf(|buf| {
                let sz = LibscfError::from_ssize(unsafe {
                    libscf_sys::scf_value_get_opaque(
                        ptr,
                        buf.as_mut_ptr().cast::<libc::c_void>(),
                        buf.len(),
                    )
                })
                .map_err(GetValueError::GetOpaque)?;
                if sz > buf.len() {
                    Err(GetValueError::GetOpaqueOutOfBounds(sz))
                } else {
                    Ok(Value::Opaque(buf[..sz].to_vec().into()))
                }
            }),
            scf_type_t::SCF_TYPE_ASTRING => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                Ok(Value::AString(s.to_owned().into()))
            }),
            scf_type_t::SCF_TYPE_USTRING => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                Ok(Value::UString(s.to_owned().into()))
            }),
            scf_type_t::SCF_TYPE_URI => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                Ok(Value::Uri(s.to_owned().into()))
            }),
            scf_type_t::SCF_TYPE_FMRI => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                Ok(Value::Fmri(s.to_owned().into()))
            }),
            scf_type_t::SCF_TYPE_HOST => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                Ok(Value::Host(s.to_owned().into()))
            }),
            scf_type_t::SCF_TYPE_HOSTNAME => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                Ok(Value::Hostname(s.to_owned().into()))
            }),
            scf_type_t::SCF_TYPE_NET_ADDR_V4 => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                // libscf allows bare IP addresses or IP addresses with a
                // /prefix; try bare IPs first then fall back to ipnets.
                if let Ok(ip) = s.parse() {
                    Ok(Value::NetAddrV4(ip))
                } else if let Ok(ipnet) = s.parse() {
                    Ok(Value::NetV4(ipnet))
                } else {
                    Err(GetValueError::InvalidNetAddrV4(s.to_owned()))
                }
            }),
            scf_type_t::SCF_TYPE_NET_ADDR_V6 => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                if let Ok(ip) = s.parse() {
                    Ok(Value::NetAddrV6(ip))
                } else if let Ok(ipnet) = s.parse() {
                    Ok(Value::NetV6(ipnet))
                } else {
                    Err(GetValueError::InvalidNetAddrV6(s.to_owned()))
                }
            }),
            scf_type_t::SCF_TYPE_NET_ADDR => with_scf_value_buf(|buf| {
                let s = get_as_string(ptr, buf)?;
                if let Ok(ip) = s.parse() {
                    Ok(Value::NetAddr(ip))
                } else if let Ok(ipnet) = s.parse() {
                    Ok(Value::Net(ipnet))
                } else {
                    Err(GetValueError::InvalidNetAddr(s.to_owned()))
                }
            }),
        }
    }

    pub(crate) fn set(
        &mut self,
        value: &Value<'_>,
    ) -> Result<(), SetValueError> {
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
        ) -> Result<Result<(), LibscfError>, SetValueError> {
            let s = CString::new(s).map_err(|err| {
                SetValueError::InvalidString { value: s.to_owned(), err }
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
                        SetValueError::InvalidTimestampNanos {
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
        result.map_err(|err| SetValueError::Set {
            value: value.to_static_value(),
            err,
        })
    }
}

fn with_scf_value_buf<F, T>(f: F) -> T
where
    F: FnOnce(&mut Vec<u8>) -> T,
{
    thread_local! {
        static BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static MAX_LEN: Cell<Option<usize>> = const { Cell::new(None) };
    }

    let len = MAX_LEN.with(|cell| {
        if let Some(len) = cell.get() {
            return len;
        }
        let sz = unsafe {
            libscf_sys::scf_limit(libscf_sys::SCF_LIMIT_MAX_VALUE_LENGTH)
        };
        // scf_limit() is documented as only failing if we pass an unknown
        // argument; `SCF_LIMIT_MAX_VALUE_LENGTH` is not unknown, so it should
        // never fail. If the constant value changes or is out of sync, we
        // should catch that immediately in our unit tests.
        assert!(sz > 0, "unexpected return value from scf_limit(): {sz}");
        let sz = sz as usize;
        cell.set(Some(sz));
        sz
    });

    BUF.with_borrow_mut(|buf| {
        buf.clear();
        buf.resize(len, 0);
        f(buf)
    })
}
