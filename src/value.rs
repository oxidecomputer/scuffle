// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::Property;
use crate::Scf;
use crate::buf::scf_get_string;
use crate::buf::with_scf_value_buf;
use crate::error::ErrorPath;
use crate::error::GetValueError;
use crate::error::HandleCreateError;
use crate::error::IterError;
use crate::error::IterErrorKind;
use crate::error::LibscfError;
use crate::error::ScfEntity;
use crate::error::SetValueError;
use crate::iter::ScfIter;
use crate::scf::ScfObject;
use chrono::DateTime;
use chrono::Utc;
use libscf_sys::scf_type_t;
use num_traits::FromPrimitive;
use oxnet::IpNet;
use oxnet::Ipv4Net;
use oxnet::Ipv6Net;
use std::ffi::CString;
use std::fmt;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::Ipv6Addr;

#[cfg(any(test, feature = "testing"))]
use proptest::prelude::any;
#[cfg(any(test, feature = "testing"))]
use proptest::strategy::Strategy as _;
#[cfg(any(test, feature = "testing"))]
use test_strategy::Arbitrary;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(any(test, feature = "testing"), derive(Arbitrary))]
pub enum Value {
    Bool(bool),
    Count(u64),
    Integer(i64),
    Time(
        #[cfg_attr(any(test, feature = "testing"), strategy(
            any::<std::time::SystemTime>().prop_map(From::from)
        ))]
        DateTime<Utc>,
    ),
    AString(String),
    Opaque(Vec<u8>),
    UString(String),
    Uri(
        // We want to generate URIs that libscf considers valid. Instead of
        // trying to be fancy here, just generate plain-looking alphanumeric
        // strings.
        #[cfg_attr(
            any(test, feature = "testing"),
            strategy("[[:alpha:]][[:alnum:]]*")
        )]
        String,
    ),
    Fmri(
        // We want to generate FMRIs that libscf considers valid. Instead of
        // trying to be fancy here, just generate plain-looking alphanumeric
        // strings.
        #[cfg_attr(
            any(test, feature = "testing"),
            strategy("[[:alpha:]][[:alnum:]]*")
        )]
        String,
    ),
    // Unlike URI and FMRI, libscf does essentially no validation against HOST
    // or HOSTNAME types (only that they're valid UTF8). We don't need custom
    // strategies for them.
    Host(String),
    Hostname(String),
    NetAddrV4(Ipv4Addr),
    NetV4(
        #[cfg_attr(any(test, feature = "testing"), strategy(
            any::<arb_support::ArbIpv4Net>().prop_map(From::from)
        ))]
        Ipv4Net,
    ),
    NetAddrV6(Ipv6Addr),
    NetV6(
        #[cfg_attr(any(test, feature = "testing"), strategy(
            any::<arb_support::ArbIpv6Net>().prop_map(From::from)
        ))]
        Ipv6Net,
    ),
    NetAddr(IpAddr),
    Net(
        #[cfg_attr(any(test, feature = "testing"), strategy(
            any::<arb_support::ArbIpNet>().prop_map(From::from)
        ))]
        IpNet,
    ),
}

impl Value {
    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Bool(_) => ValueKind::Bool,
            Self::Count(_) => ValueKind::Count,
            Self::Integer(_) => ValueKind::Integer,
            Self::Time(_) => ValueKind::Time,
            Self::AString(_) => ValueKind::AString,
            Self::Opaque(_) => ValueKind::Opaque,
            Self::UString(_) => ValueKind::UString,
            Self::Uri(_) => ValueKind::Uri,
            Self::Fmri(_) => ValueKind::Fmri,
            Self::Host(_) => ValueKind::Host,
            Self::Hostname(_) => ValueKind::Hostname,
            Self::NetAddrV4(_) | Self::NetV4(_) => ValueKind::NetAddrV4,
            Self::NetAddrV6(_) | Self::NetV6(_) => ValueKind::NetAddrV6,
            Self::NetAddr(_) | Self::Net(_) => ValueKind::NetAddr,
        }
    }

    pub fn as_value_ref(&self) -> ValueRef<'_> {
        match self {
            Self::Bool(b) => ValueRef::Bool(*b),
            Self::Count(c) => ValueRef::Count(*c),
            Self::Integer(i) => ValueRef::Integer(*i),
            Self::Time(ts) => ValueRef::Time(*ts),
            Self::AString(s) => ValueRef::AString(s),
            Self::Opaque(data) => ValueRef::Opaque(data),
            Self::UString(s) => ValueRef::UString(s),
            Self::Uri(s) => ValueRef::Uri(s),
            Self::Fmri(s) => ValueRef::Fmri(s),
            Self::Host(s) => ValueRef::Host(s),
            Self::Hostname(s) => ValueRef::Hostname(s),
            Self::NetAddrV4(ip) => ValueRef::NetAddrV4(*ip),
            Self::NetV4(ip) => ValueRef::NetV4(*ip),
            Self::NetAddrV6(ip) => ValueRef::NetAddrV6(*ip),
            Self::NetV6(ip) => ValueRef::NetV6(*ip),
            Self::NetAddr(ip) => ValueRef::NetAddr(*ip),
            Self::Net(ip) => ValueRef::Net(*ip),
        }
    }

    /// Returns a displayable type that formats values consistently with how SMF
    /// would display them as strings (e.g., `Opaque` values are hex-encoded;
    /// `Time` values are `{seconds}.{nanoseconds}`).
    pub fn display_smf(&self) -> ValueDisplaySmf<'_> {
        ValueDisplaySmf(self.as_value_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueKind {
    Bool,
    Count,
    Integer,
    Time,
    AString,
    Opaque,
    UString,
    Uri,
    Fmri,
    Host,
    Hostname,
    NetAddrV4,
    NetAddrV6,
    NetAddr,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ValueKind::Bool => "boolean",
            ValueKind::Count => "count",
            ValueKind::Integer => "integer",
            ValueKind::Time => "time",
            ValueKind::AString => "astring",
            ValueKind::Opaque => "opaque",
            ValueKind::UString => "ustring",
            ValueKind::Uri => "uri",
            ValueKind::Fmri => "fmri",
            ValueKind::Host => "host",
            ValueKind::Hostname => "hostname",
            ValueKind::NetAddrV4 => "net_address_v4",
            ValueKind::NetAddrV6 => "net_address_v6",
            ValueKind::NetAddr => "net_address",
        };
        s.fmt(f)
    }
}

impl ValueKind {
    pub(crate) fn to_scf_type(self) -> libscf_sys::scf_type_t {
        use libscf_sys::scf_type_t::*;

        match self {
            ValueKind::Bool => SCF_TYPE_BOOLEAN,
            ValueKind::Count => SCF_TYPE_COUNT,
            ValueKind::Integer => SCF_TYPE_INTEGER,
            ValueKind::Time => SCF_TYPE_TIME,
            ValueKind::AString => SCF_TYPE_ASTRING,
            ValueKind::Opaque => SCF_TYPE_OPAQUE,
            ValueKind::UString => SCF_TYPE_USTRING,
            ValueKind::Uri => SCF_TYPE_URI,
            ValueKind::Fmri => SCF_TYPE_FMRI,
            ValueKind::Host => SCF_TYPE_HOST,
            ValueKind::Hostname => SCF_TYPE_HOSTNAME,
            ValueKind::NetAddrV4 => SCF_TYPE_NET_ADDR_V4,
            ValueKind::NetAddrV6 => SCF_TYPE_NET_ADDR_V6,
            ValueKind::NetAddr => SCF_TYPE_NET_ADDR,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueRef<'a> {
    Bool(bool),
    Count(u64),
    Integer(i64),
    Time(DateTime<Utc>),
    AString(&'a str),
    Opaque(&'a [u8]),
    UString(&'a str),
    Uri(&'a str),
    Fmri(&'a str),
    Host(&'a str),
    Hostname(&'a str),
    NetAddrV4(Ipv4Addr),
    NetV4(Ipv4Net),
    NetAddrV6(Ipv6Addr),
    NetV6(Ipv6Net),
    NetAddr(IpAddr),
    Net(IpNet),
}

impl<'a> ValueRef<'a> {
    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Bool(_) => ValueKind::Bool,
            Self::Count(_) => ValueKind::Count,
            Self::Integer(_) => ValueKind::Integer,
            Self::Time(_) => ValueKind::Time,
            Self::AString(_) => ValueKind::AString,
            Self::Opaque(_) => ValueKind::Opaque,
            Self::UString(_) => ValueKind::UString,
            Self::Uri(_) => ValueKind::Uri,
            Self::Fmri(_) => ValueKind::Fmri,
            Self::Host(_) => ValueKind::Host,
            Self::Hostname(_) => ValueKind::Hostname,
            Self::NetAddrV4(_) | Self::NetV4(_) => ValueKind::NetAddrV4,
            Self::NetAddrV6(_) | Self::NetV6(_) => ValueKind::NetAddrV6,
            Self::NetAddr(_) | Self::Net(_) => ValueKind::NetAddr,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::Bool(b) => Value::Bool(*b),
            Self::Count(c) => Value::Count(*c),
            Self::Integer(i) => Value::Integer(*i),
            Self::Time(ts) => Value::Time(*ts),
            Self::AString(s) => Value::AString(s.to_string()),
            Self::Opaque(v) => Value::Opaque(v.to_vec()),
            Self::UString(s) => Value::UString(s.to_string()),
            Self::Uri(u) => Value::Uri(u.to_string()),
            Self::Fmri(f) => Value::Fmri(f.to_string()),
            Self::Host(h) => Value::Host(h.to_string()),
            Self::Hostname(h) => Value::Hostname(h.to_string()),
            Self::NetAddrV4(ip) => Value::NetAddrV4(*ip),
            Self::NetV4(ipnet) => Value::NetV4(*ipnet),
            Self::NetAddrV6(ip) => Value::NetAddrV6(*ip),
            Self::NetV6(ipnet) => Value::NetV6(*ipnet),
            Self::NetAddr(ip) => Value::NetAddr(*ip),
            Self::Net(ipnet) => Value::Net(*ipnet),
        }
    }

    /// Returns a displayable type that formats values consistently with how SMF
    /// would display them as strings (e.g., `Opaque` values are hex-encoded;
    /// `Time` values are `{seconds}.{nanoseconds}`).
    pub fn display_smf(&self) -> ValueDisplaySmf<'a> {
        ValueDisplaySmf(*self)
    }
}

pub struct ValueDisplaySmf<'a>(ValueRef<'a>);

impl fmt::Display for ValueDisplaySmf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ValueRef::Bool(b) => b.fmt(f),
            ValueRef::Count(n) => n.fmt(f),
            ValueRef::Integer(i) => i.fmt(f),
            ValueRef::Time(ts) => {
                let secs = ts.timestamp();
                let nanosecs = ts.timestamp_subsec_nanos();
                if nanosecs == 0 {
                    secs.fmt(f)
                } else {
                    write!(f, "{secs}.{nanosecs:09}")
                }
            }
            ValueRef::Opaque(data) => {
                for b in data {
                    write!(f, "{b:02x}")?;
                }
                Ok(())
            }
            ValueRef::AString(s)
            | ValueRef::UString(s)
            | ValueRef::Uri(s)
            | ValueRef::Fmri(s)
            | ValueRef::Host(s)
            | ValueRef::Hostname(s) => s.fmt(f),
            ValueRef::NetAddrV4(ip) => ip.fmt(f),
            ValueRef::NetV4(ip) => ip.fmt(f),
            ValueRef::NetAddrV6(ip) => ip.fmt(f),
            ValueRef::NetV6(ip) => ip.fmt(f),
            ValueRef::NetAddr(ip) => ip.fmt(f),
            ValueRef::Net(ip) => ip.fmt(f),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ScfValue<'a> {
    handle: ScfObject<'a, libscf_sys::scf_value_t>,
}

impl<'scf> ScfValue<'scf> {
    pub(crate) fn new(scf: &'scf Scf<'scf>) -> Result<Self, HandleCreateError> {
        let handle = scf.scf_value_create()?;
        Ok(Self { handle })
    }
}

impl ScfValue<'_> {
    pub(crate) unsafe fn scf_apply_as_decoration(
        &mut self,
        scf: *mut libscf_sys::scf_handle_t,
        decoration: *const i8,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_handle_decorate(
                scf,
                decoration,
                // TODO-correctness Could this take a const pointer instead?
                // Header takes non-const but I think it's read-only.
                self.handle.as_mut_ptr(),
            )
        })
    }

    pub(crate) unsafe fn scf_add_to_transaction_entry(
        &mut self,
        entry: *mut libscf_sys::scf_transaction_entry_t,
    ) -> Result<(), LibscfError> {
        LibscfError::from_ret(unsafe {
            libscf_sys::scf_entry_add_value(entry, self.handle.as_mut_ptr())
        })
    }

    pub(crate) fn get(&self) -> Result<Value, GetValueError> {
        // Helper function for all the scf types for which we have to fetch
        // strings. Uses the libscf-to-Rust-string support provided by
        // `crate::buf::*`.
        fn get_as_string(
            ptr: *const libscf_sys::scf_value_t,
        ) -> Result<String, GetValueError> {
            with_scf_value_buf(|buf| {
                scf_get_string(ScfEntity::Value, buf, |buf, buf_len| unsafe {
                    libscf_sys::scf_value_get_as_string(ptr, buf, buf_len)
                })
            })
            .map(|s| s.into_string())
            .map_err(From::from)
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
                    Ok(Value::Opaque(buf[..sz].to_vec()))
                }
            }),
            scf_type_t::SCF_TYPE_ASTRING => {
                Ok(Value::AString(get_as_string(ptr)?))
            }
            scf_type_t::SCF_TYPE_USTRING => {
                Ok(Value::UString(get_as_string(ptr)?))
            }
            scf_type_t::SCF_TYPE_URI => Ok(Value::Uri(get_as_string(ptr)?)),
            scf_type_t::SCF_TYPE_FMRI => Ok(Value::Fmri(get_as_string(ptr)?)),
            scf_type_t::SCF_TYPE_HOST => Ok(Value::Host(get_as_string(ptr)?)),
            scf_type_t::SCF_TYPE_HOSTNAME => {
                Ok(Value::Hostname(get_as_string(ptr)?))
            }
            scf_type_t::SCF_TYPE_NET_ADDR_V4 => {
                let s = get_as_string(ptr)?;
                // libscf allows bare IP addresses or IP addresses with a
                // /prefix; try bare IPs first then fall back to ipnets.
                if let Ok(ip) = s.parse() {
                    Ok(Value::NetAddrV4(ip))
                } else if let Ok(ipnet) = s.parse() {
                    Ok(Value::NetV4(ipnet))
                } else {
                    Err(GetValueError::InvalidNetAddrV4(s.into_boxed_str()))
                }
            }
            scf_type_t::SCF_TYPE_NET_ADDR_V6 => {
                let s = get_as_string(ptr)?;
                if let Ok(ip) = s.parse() {
                    Ok(Value::NetAddrV6(ip))
                } else if let Ok(ipnet) = s.parse() {
                    Ok(Value::NetV6(ipnet))
                } else {
                    Err(GetValueError::InvalidNetAddrV6(s.into_boxed_str()))
                }
            }
            scf_type_t::SCF_TYPE_NET_ADDR => {
                let s = get_as_string(ptr)?;
                if let Ok(ip) = s.parse() {
                    Ok(Value::NetAddr(ip))
                } else if let Ok(ipnet) = s.parse() {
                    Ok(Value::Net(ipnet))
                } else {
                    Err(GetValueError::InvalidNetAddr(s.into_boxed_str()))
                }
            }
        }
    }

    pub(crate) fn set(
        &mut self,
        value: ValueRef<'_>,
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
                SetValueError::InvalidString { value: Box::from(s), err }
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
            let ptr = self.handle.as_mut_ptr();
            match value {
                ValueRef::Bool(b) => {
                    () = unsafe {
                        libscf_sys::scf_value_set_boolean(ptr, u8::from(b))
                    };
                    Ok(())
                }
                ValueRef::Count(n) => {
                    () = unsafe { libscf_sys::scf_value_set_count(ptr, n) };
                    Ok(())
                }
                ValueRef::Integer(i) => {
                    () = unsafe { libscf_sys::scf_value_set_integer(ptr, i) };
                    Ok(())
                }
                ValueRef::Time(timestamp) => {
                    let seconds = timestamp.timestamp();
                    let nanos = timestamp.timestamp_subsec_nanos();
                    let nanos = i32::try_from(nanos).map_err(|_| {
                        SetValueError::InvalidTimestampNanos {
                            timestamp,
                            seconds,
                            nanos,
                        }
                    })?;
                    LibscfError::from_ret(unsafe {
                        libscf_sys::scf_value_set_time(ptr, seconds, nanos)
                    })
                }
                ValueRef::Opaque(data) => LibscfError::from_ret(unsafe {
                    libscf_sys::scf_value_set_opaque(
                        ptr,
                        data.as_ptr().cast::<libc::c_void>(),
                        data.len(),
                    )
                }),
                // TODO-correctness There are explicit functions for setting
                // AString and UString values, respectively, but the libscf_sys
                // bindings are incorrect, so instead we to through "set from
                // string" for them like the remainder of the fancy types.
                // See <https://github.com/illumos/libscf-sys/issues/1>.
                ValueRef::AString(s) => {
                    set_from_string(ptr, SCF_TYPE_ASTRING, s)?
                }
                ValueRef::UString(s) => {
                    set_from_string(ptr, SCF_TYPE_USTRING, s)?
                }
                ValueRef::Uri(s) => set_from_string(ptr, SCF_TYPE_URI, s)?,
                ValueRef::Fmri(s) => set_from_string(ptr, SCF_TYPE_FMRI, s)?,
                ValueRef::Host(s) => set_from_string(ptr, SCF_TYPE_HOST, s)?,
                ValueRef::Hostname(s) => {
                    set_from_string(ptr, SCF_TYPE_HOSTNAME, s)?
                }
                ValueRef::NetAddrV4(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V4, &ip.to_string())?
                }
                ValueRef::NetV4(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V4, &ip.to_string())?
                }
                ValueRef::NetAddrV6(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V6, &ip.to_string())?
                }
                ValueRef::NetV6(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR_V6, &ip.to_string())?
                }
                ValueRef::NetAddr(ip) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR, &ip.to_string())?
                }
                ValueRef::Net(ipnet) => {
                    set_from_string(ptr, SCF_TYPE_NET_ADDR, &ipnet.to_string())?
                }
            }
        };
        result
            .map_err(|err| SetValueError::Set { value: value.to_value(), err })
    }
}

// Helpers to generate `Arbitrary` proptest values for oxnet types
#[cfg(any(test, feature = "testing"))]
mod arb_support {
    use super::*;

    #[derive(Debug, Clone, Copy, Arbitrary)]
    pub(super) struct ArbIpv4Net(Ipv4Addr, #[strategy(0_u8..=32)] u8);

    impl From<ArbIpv4Net> for Ipv4Net {
        fn from(value: ArbIpv4Net) -> Self {
            Self::new(value.0, value.1).unwrap()
        }
    }

    #[derive(Debug, Clone, Copy, Arbitrary)]
    pub(super) struct ArbIpv6Net(Ipv6Addr, #[strategy(0_u8..=128)] u8);

    impl From<ArbIpv6Net> for Ipv6Net {
        fn from(value: ArbIpv6Net) -> Self {
            Self::new(value.0, value.1).unwrap()
        }
    }

    #[derive(Debug, Clone, Copy, Arbitrary)]
    pub(super) enum ArbIpNet {
        V4(ArbIpv4Net),
        V6(ArbIpv6Net),
    }

    impl From<ArbIpNet> for IpNet {
        fn from(value: ArbIpNet) -> Self {
            match value {
                ArbIpNet::V4(ip) => Self::V4(ip.into()),
                ArbIpNet::V6(ip) => Self::V6(ip.into()),
            }
        }
    }
}

pub struct Values<'a, St> {
    parent: &'a Property<'a, St>,
    value: ScfValue<'a>,
    iter: ScfIter<'a, libscf_sys::scf_value_t>,
}

impl<'a, St> Values<'a, St> {
    pub(crate) fn new(
        parent: &'a Property<'a, St>,
        iter: ScfIter<'a, libscf_sys::scf_value_t>,
    ) -> Result<Self, IterError> {
        let value = ScfValue::new(parent.scf())?;
        Ok(Self { parent, value, iter })
    }
}

impl<'a, St> Iterator for Values<'a, St> {
    type Item = Result<Value, IterError>;

    fn next(&mut self) -> Option<Self::Item> {
        // Reset the ScfValue we're using as the destination for the next
        // iterator item. We always return an owned `Value` or error, so don't
        // need to maintain the contents of `self.value` any longer than this
        // function. `scf_value_reset` is infallible.
        () = unsafe {
            libscf_sys::scf_value_reset(self.value.handle.as_mut_ptr())
        };

        match self.iter.next_with_handle(self.parent, &mut self.value.handle)? {
            Ok(()) => Some(self.value.get().map_err(|err| IterError::Iter {
                entity: ScfEntity::Value,
                parent: self.parent.error_path(),
                kind: IterErrorKind::GetValue(err),
            })),
            Err(err) => Some(Err(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isolated::IsolatedConfigd;
    use proptest::proptest;

    fn scf_value_as_string(v: &ScfValue) -> String {
        with_scf_value_buf(|buf| {
            let ret = unsafe {
                libscf_sys::scf_value_get_as_string(
                    v.handle.as_ptr(),
                    buf.as_mut_ptr().cast::<i8>(),
                    buf.len(),
                )
            };
            assert!(ret >= 0);
            let ret = ret as usize;
            assert!(ret <= buf.len());
            String::from_utf8_lossy(&buf[..ret]).to_string()
        })
    }

    #[test]
    fn value_get_set_roundtrip() {
        let isolated =
            IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
        let scf = Scf::connect_isolated(&isolated).unwrap();

        proptest!(|(val: Value)| {
            let mut sval = ScfValue::new(&scf).unwrap();
            sval.set(val.as_value_ref()).expect("set value");
            let roundtrip = sval.get().expect("got value");
            assert_eq!(roundtrip, val);
        });
    }

    #[test]
    fn value_display_smf_matches() {
        let isolated =
            IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
        let scf = Scf::connect_isolated(&isolated).unwrap();

        proptest!(|(val: Value)| {
            let displayed = val.display_smf().to_string();

            let mut sval = ScfValue::new(&scf).unwrap();
            sval.set(val.as_value_ref()).expect("set value");
            let expected = scf_value_as_string(&sval);

            assert_eq!(displayed, expected);
        });
    }
}
