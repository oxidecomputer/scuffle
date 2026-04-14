// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::PropertyGroup;
use crate::PropertyGroupEditable;
use crate::Scf;
use crate::ValueKind;
use crate::ValueRef;
use crate::error::ErrorPath;
use crate::error::LibscfError;
use crate::error::TransactionError;
use crate::scf::ScfObject;
use crate::utf8cstring::Utf8CString;
use crate::value::ScfValue;
use std::marker::PhantomData;

#[derive(Debug)]
pub enum TransactionReset {}
#[derive(Debug)]
pub enum TransactionStarted {}
#[derive(Debug)]
pub enum TransactionCommitted {}

#[derive(Debug)]
pub enum TransactionCommitResult<'a, 'pg> {
    Success(Transaction<'a, 'pg, TransactionCommitted>),
    OutOfDate(Transaction<'a, 'pg, TransactionReset>),
}

#[derive(Debug)]
pub struct Transaction<'a, 'pg, St> {
    inner: TransactionInner<'a, 'pg>,
    _state: PhantomData<fn() -> St>,
}

#[derive(Debug)]
struct TransactionInner<'a, 'pg> {
    property_group: &'a mut PropertyGroup<'pg, PropertyGroupEditable>,
    handle: ScfObject<'a, libscf_sys::scf_transaction_t>,
    // We can't drop the entries until the transaction is reset; hold on to them
    // here.
    entries: Vec<TransactionEntry<'a>>,
}

impl Drop for TransactionInner<'_, '_> {
    fn drop(&mut self) {
        // reset the transaction to detach any entries before dropping (and
        // therefore destroying) the transaction itself
        self.reset();
    }
}

impl TransactionInner<'_, '_> {
    fn reset(&mut self) {
        // Reset the transaction...
        () = unsafe {
            libscf_sys::scf_transaction_reset(self.handle.as_mut_ptr())
        };

        // then drop (and destroy) all the entries that were associated with it.
        self.entries.clear();
    }
}

// Methods available on transaction in any state.
impl<'a, 'pg, St> Transaction<'a, 'pg, St> {
    pub fn reset(mut self) -> Transaction<'a, 'pg, TransactionReset> {
        self.inner.reset();
        Transaction { inner: self.inner, _state: PhantomData }
    }

    fn scf(&self) -> &'a Scf<'a> {
        self.inner.property_group.scf()
    }

    fn pg_error_path(&self) -> Box<str> {
        self.inner.property_group.error_path()
    }
}

// Methods available on Reset (also the just-created state) transactions.
impl<'a, 'pg> Transaction<'a, 'pg, TransactionReset> {
    pub(crate) fn new(
        property_group: &'a mut PropertyGroup<'pg, PropertyGroupEditable>,
    ) -> Result<Self, TransactionError> {
        let handle = property_group.scf().scf_transaction_create()?;
        Ok(Self {
            inner: TransactionInner {
                property_group,
                handle,
                entries: Vec::new(),
            },
            _state: PhantomData,
        })
    }

    pub fn start(
        mut self,
    ) -> Result<Transaction<'a, 'pg, TransactionStarted>, TransactionError>
    {
        match unsafe {
            self.inner
                .property_group
                .scf_transaction_start(self.inner.handle.as_mut_ptr())
        } {
            Ok(()) => {
                Ok(Transaction { inner: self.inner, _state: PhantomData })
            }
            Err(err) => Err(TransactionError::Start {
                property_group: self.pg_error_path(),
                err,
            }),
        }
    }
}

// Methods available on Started transactions.
impl<'a, 'pg> Transaction<'a, 'pg, TransactionStarted> {
    fn check_property_name(
        &self,
        name: &str,
    ) -> Result<Utf8CString, TransactionError> {
        Utf8CString::from_str(name).map_err(|err| {
            TransactionError::InvalidName {
                property_group: self.pg_error_path(),
                err,
            }
        })
    }

    fn collect_values<'b, I: IntoIterator<Item = ValueRef<'b>>>(
        &self,
        name: &Utf8CString,
        expected_kind: ValueKind,
        values: I,
    ) -> Result<Vec<ScfValue<'a>>, TransactionError> {
        let mut collected = Vec::new();
        for val in values {
            if val.kind() != expected_kind {
                return Err(TransactionError::TypeMismatch {
                    property_group: self.pg_error_path(),
                    name: name.to_string().into_boxed_str(),
                    property_type: expected_kind,
                    value_type: val.kind(),
                });
            }

            let mut scf_val = ScfValue::new(self.scf())?;
            scf_val.set(val).map_err(|err| TransactionError::SetValue {
                property_group: self.pg_error_path(),
                name: name.to_string().into_boxed_str(),
                err,
            })?;

            collected.push(scf_val);
        }
        Ok(collected)
    }

    pub fn property_delete(
        &mut self,
        name: &str,
    ) -> Result<(), TransactionError> {
        let name = self.check_property_name(name)?;
        let entry = TransactionEntry::new_delete(self, &name)?;
        self.inner.entries.push(entry);
        Ok(())
    }

    pub fn property_new(
        &mut self,
        name: &str,
        value: ValueRef<'_>,
    ) -> Result<(), TransactionError> {
        self.property_new_multiple(name, value.kind(), std::iter::once(value))
    }

    pub fn property_new_multiple<'b, I>(
        &mut self,
        name: &str,
        value_kind: ValueKind,
        values: I,
    ) -> Result<(), TransactionError>
    where
        I: IntoIterator<Item = ValueRef<'b>>,
    {
        let name = self.check_property_name(name)?;
        let values = self.collect_values(&name, value_kind, values)?;
        let entry = TransactionEntry::new_new(self, &name, value_kind, values)?;
        self.inner.entries.push(entry);
        Ok(())
    }

    pub fn property_change(
        &mut self,
        name: &str,
        value: ValueRef<'_>,
    ) -> Result<(), TransactionError> {
        self.property_change_multiple(
            name,
            value.kind(),
            std::iter::once(value),
        )
    }

    pub fn property_change_multiple<'b, I>(
        &mut self,
        name: &str,
        value_kind: ValueKind,
        values: I,
    ) -> Result<(), TransactionError>
    where
        I: IntoIterator<Item = ValueRef<'b>>,
    {
        let name = self.check_property_name(name)?;
        let values = self.collect_values(&name, value_kind, values)?;
        let entry =
            TransactionEntry::new_change(self, &name, value_kind, values)?;
        self.inner.entries.push(entry);
        Ok(())
    }

    pub fn property_change_type(
        &mut self,
        name: &str,
        value: ValueRef<'_>,
    ) -> Result<(), TransactionError> {
        self.property_change_type_multiple(
            name,
            value.kind(),
            std::iter::once(value),
        )
    }

    pub fn property_change_type_multiple<'b, I>(
        &mut self,
        name: &str,
        value_kind: ValueKind,
        values: I,
    ) -> Result<(), TransactionError>
    where
        I: IntoIterator<Item = ValueRef<'b>>,
    {
        let name = self.check_property_name(name)?;
        let values = self.collect_values(&name, value_kind, values)?;
        let entry =
            TransactionEntry::new_change_type(self, &name, value_kind, values)?;
        self.inner.entries.push(entry);
        Ok(())
    }

    pub fn property_ensure(
        &mut self,
        name: &str,
        value: ValueRef<'_>,
    ) -> Result<(), TransactionError> {
        self.property_ensure_multiple(
            name,
            value.kind(),
            std::iter::once(value),
        )
    }

    pub fn property_ensure_multiple<'b, I>(
        &mut self,
        name: &str,
        value_kind: ValueKind,
        values: I,
    ) -> Result<(), TransactionError>
    where
        I: IntoIterator<Item = ValueRef<'b>>,
    {
        let already_exists = self
            .inner
            .property_group
            .property(name)
            .map_err(|err| TransactionError::ExistenceLookup {
                property_group: self.pg_error_path(),
                name: name.to_string().into_boxed_str(),
                err,
            })?
            .is_some();

        if already_exists {
            self.property_change_type_multiple(name, value_kind, values)
        } else {
            self.property_new_multiple(name, value_kind, values)
        }
    }

    pub fn commit(
        mut self,
    ) -> Result<TransactionCommitResult<'a, 'pg>, TransactionError> {
        match unsafe {
            libscf_sys::scf_transaction_commit(self.inner.handle.as_mut_ptr())
        } {
            0 => Ok(TransactionCommitResult::OutOfDate(self.reset())),
            1 => Ok(TransactionCommitResult::Success(Transaction {
                inner: self.inner,
                _state: PhantomData,
            })),
            _ => {
                let err = LibscfError::last();
                Err(TransactionError::Commit {
                    property_group: self.pg_error_path(),
                    err,
                })
            }
        }
    }
}

#[derive(Debug)]
struct TransactionEntry<'a> {
    handle: ScfObject<'a, libscf_sys::scf_transaction_entry_t>,
    // We never use these, but have to keep them from being destroyed as long as
    // they're associated with `handle`.
    _values: Vec<ScfValue<'a>>,
}

impl Drop for TransactionEntry<'_> {
    fn drop(&mut self) {
        // Before dropping the handle and kind, which will destroy both the
        // entry and any associated values, detach the values from the entry.
        unsafe { libscf_sys::scf_entry_reset(self.handle.as_mut_ptr()) };
    }
}

impl<'a> TransactionEntry<'a> {
    fn new_common<F>(
        tx: &mut Transaction<'a, '_, TransactionStarted>,
        name: &Utf8CString,
        mut values: Vec<ScfValue<'a>>,
        f: F,
    ) -> Result<Self, TransactionError>
    where
        F: FnOnce(
            &mut Transaction<'a, '_, TransactionStarted>,
            &Utf8CString,
            &mut ScfObject<'a, libscf_sys::scf_transaction_entry_t>,
        ) -> Result<(), TransactionError>,
    {
        let mut handle = tx.scf().scf_entry_create()?;

        f(tx, name, &mut handle)?;

        for val in &mut values {
            unsafe { val.scf_add_to_transaction_entry(handle.as_mut_ptr()) }
                .map_err(|err| TransactionError::AddValue {
                    property_group: tx.pg_error_path(),
                    name: name.to_string().into_boxed_str(),
                    err,
                })?;
        }

        Ok(Self { handle, _values: values })
    }

    fn new_delete(
        tx: &mut Transaction<'a, '_, TransactionStarted>,
        name: &Utf8CString,
    ) -> Result<Self, TransactionError> {
        let values = Vec::new(); // delete has no attached values

        Self::new_common(tx, name, values, |tx, name, handle| {
            LibscfError::from_ret(unsafe {
                libscf_sys::scf_transaction_property_delete(
                    tx.inner.handle.as_mut_ptr(),
                    handle.as_mut_ptr(),
                    name.as_c_str().as_ptr(),
                )
            })
            .map_err(|err| TransactionError::Delete {
                property_group: tx.pg_error_path(),
                name: name.to_string().into_boxed_str(),
                err,
            })
        })
    }

    fn new_new(
        tx: &mut Transaction<'a, '_, TransactionStarted>,
        name: &Utf8CString,
        value_kind: ValueKind,
        values: Vec<ScfValue<'a>>,
    ) -> Result<Self, TransactionError> {
        Self::new_common(tx, name, values, |tx, name, handle| {
            LibscfError::from_ret(unsafe {
                libscf_sys::scf_transaction_property_new(
                    tx.inner.handle.as_mut_ptr(),
                    handle.as_mut_ptr(),
                    name.as_c_str().as_ptr(),
                    value_kind.to_scf_type(),
                )
            })
            .map_err(|err| TransactionError::New {
                property_group: tx.pg_error_path(),
                name: name.to_string().into_boxed_str(),
                err,
            })
        })
    }

    fn new_change(
        tx: &mut Transaction<'a, '_, TransactionStarted>,
        name: &Utf8CString,
        value_kind: ValueKind,
        values: Vec<ScfValue<'a>>,
    ) -> Result<Self, TransactionError> {
        Self::new_common(tx, name, values, |tx, name, handle| {
            LibscfError::from_ret(unsafe {
                libscf_sys::scf_transaction_property_change(
                    tx.inner.handle.as_mut_ptr(),
                    handle.as_mut_ptr(),
                    name.as_c_str().as_ptr(),
                    value_kind.to_scf_type(),
                )
            })
            .map_err(|err| TransactionError::Change {
                property_group: tx.pg_error_path(),
                name: name.to_string().into_boxed_str(),
                err,
            })
        })
    }

    fn new_change_type(
        tx: &mut Transaction<'a, '_, TransactionStarted>,
        name: &Utf8CString,
        value_kind: ValueKind,
        values: Vec<ScfValue<'a>>,
    ) -> Result<Self, TransactionError> {
        Self::new_common(tx, name, values, |tx, name, handle| {
            LibscfError::from_ret(unsafe {
                libscf_sys::scf_transaction_property_change_type(
                    tx.inner.handle.as_mut_ptr(),
                    handle.as_mut_ptr(),
                    name.as_c_str().as_ptr(),
                    value_kind.to_scf_type(),
                )
            })
            .map_err(|err| TransactionError::ChangeType {
                property_group: tx.pg_error_path(),
                name: name.to_string().into_boxed_str(),
                err,
            })
        })
    }
}
