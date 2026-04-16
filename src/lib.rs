// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! TODO crate-level overview
//!
//! # Testing Support
//!
//! If the `testing` feature is enabled, `scuffle` exports an
//! `isolated::IsolatedConfigd` type that can run an instance of `svc.configd`
//! inside a temporary directory. After creating an instance of this type, tests
//! can connect to it via `Scf::connect_isolated()`, and then freely read and
//! write properties within that instance without touching the real system's
//! `svc.configd` and without the permissions normally required to do that.
//!
//! This comes with several caveats:
//!
//! * `IsolatedConfigd` takes advantage of undocumented and uncommitted
//!   interfaces in at least `svccfg`, `svc.configd`, and `libscf` itself. These
//!   may break in the future.
//! * Refreshing instances inside an `IsolatedConfigd` via `libscf` does not
//!   work. `scuffle` works around this by refreshing instances via `svccfg`
//!   pointed at the isolated `svc.configd`, but this is a divergence from what
//!   production code will do for `refresh`.
//! * Non-persistent property groups do not work inside `IsolatedConfigd`.
//!
//! `scuffle` uses `IsolatedConfigd` for its own tests, and therefore does not
//! have test coverage on features that interact with these restrictions.

mod buf;
mod edit_property_groups;
mod has_property_groups;
mod instance;
mod iter;
mod limit;
mod property;
mod property_group;
mod scf;
mod scope;
mod service;
mod snapshot;
mod transaction;
mod utf8cstring;
mod value;

pub mod error;

#[cfg(any(test, feature = "testing"))]
pub mod isolated;

pub use edit_property_groups::AddPropertyGroupFlags;
pub use edit_property_groups::DeletePropertyGroupResult;
pub use edit_property_groups::EditPropertyGroups;
pub use has_property_groups::HasComposedPropertyGroups;
pub use has_property_groups::HasDirectPropertyGroups;
pub use instance::Instance;
pub use instance::Instances;
pub use property::Properties;
pub use property::Property;
pub use property_group::PropertyGroup;
pub use property_group::PropertyGroupComposed;
pub use property_group::PropertyGroupDirect;
pub use property_group::PropertyGroupType;
pub use property_group::PropertyGroupUpdateResult;
pub use property_group::PropertyGroups;
pub use scf::Scf;
pub use scope::Scope;
pub use service::Service;
pub use snapshot::Snapshot;
pub use snapshot::Snapshots;
pub use transaction::Transaction;
pub use transaction::TransactionCommitResult;
pub use transaction::TransactionCommitted;
pub use transaction::TransactionReset;
pub use transaction::TransactionStarted;
pub use value::Value;
pub use value::ValueDisplaySmf;
pub use value::ValueKind;
pub use value::ValueRef;
pub use value::Values;
