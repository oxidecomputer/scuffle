// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(target_os = "illumos")]

use assert_matches::assert_matches;
use proptest::prelude::BoxedStrategy;
use proptest::prelude::any;
use proptest::proptest;
use proptest::strategy::Strategy;
use scuffle::AddPropertyGroupFlags;
use scuffle::DeletePropertyGroupResult;
use scuffle::EditPropertyGroups;
use scuffle::HasComposedPropertyGroups;
use scuffle::HasDirectPropertyGroups;
use scuffle::PropertyGroupType;
use scuffle::PropertyGroupUpdateResult;
use scuffle::Scf;
use scuffle::TransactionCommitResult;
use scuffle::Value;
use scuffle::ValueKind;
use scuffle::ValueRef;
use scuffle::error::LookupError;
use scuffle::error::SingleValueError;
use scuffle::error::TransactionBuildError;
use scuffle::isolated::IsolatedConfigd;
use std::cell::RefCell;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

/// Write an arbitrary value via `property_new`, commit, then read it
/// back through the property iteration API and verify equality.
#[test]
fn transaction_property_roundtrip() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(val: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Write phase: need &mut Instance for add_property_group.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            assert!(tx.is_empty());
            tx.property_new("prop", val.as_value_ref())
                .expect("property_new");
            assert!(!tx.is_empty());
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "commit should succeed",
            );
        }

        // Read phase: fresh property group handle for read-back.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");

            assert_eq!(readback, vec![val]);
        }
    });
}

/// Write multiple values to a single property via
/// `property_new_multiple`, commit, then read them all back and verify
/// the count, contents, and order are preserved.
#[test]
fn transaction_multi_value_roundtrip() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    // Generate 1..=8 Count values. We already test all value types in
    // test 1; the point here is to exercise multi-value mechanics.
    let strategy =
        proptest::collection::vec(any::<u64>(), 1..=8).prop_map(|counts| {
            counts.into_iter().map(Value::Count).collect::<Vec<_>>()
        });

    proptest!(|(values in strategy)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            assert!(tx.is_empty());
            tx.property_new_multiple(
                "prop",
                ValueKind::Count,
                values.iter().map(|v| v.as_value_ref()),
            )
            .expect("property_new_multiple");
            assert!(!tx.is_empty());
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "commit should succeed",
            );
        }

        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");

            assert_eq!(readback, values);
        }
    });
}

// proptest helper: strategy that generates a pair of `Value`s of the same kind
fn strategy_same_kind_values() -> BoxedStrategy<(Value, Value)> {
    any::<Value>()
        .prop_flat_map(|v1| {
            let kind = v1.kind();
            let v2_strategy = match kind {
                ValueKind::Bool => any::<bool>().prop_map(Value::Bool).boxed(),
                ValueKind::Count => any::<u64>().prop_map(Value::Count).boxed(),
                ValueKind::Integer => {
                    any::<i64>().prop_map(Value::Integer).boxed()
                }
                _ => {
                    // For all other kinds, just generate another arbitrary
                    // Value and filter to the same kind.
                    any::<Value>()
                        .prop_filter("same kind", move |v| v.kind() == kind)
                        .boxed()
                }
            };
            v2_strategy.prop_map(move |v2| (v1.clone(), v2))
        })
        .boxed()
}

/// Use `property_ensure` to write a value, then overwrite it with a
/// different value of the same kind. Verify the second value wins.
#[test]
fn transaction_property_ensure_overwrites() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(pair in strategy_same_kind_values())| {
        let (val1, val2) = pair;
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Write both values through property_ensure, then read back.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            // First ensure: creates the property.
            {
                let tx = pg.transaction().expect("create transaction");
                let mut tx = tx.start().expect("start transaction");
                tx.property_ensure("prop", val1.as_value_ref())
                    .expect("property_ensure");
                let result = tx.commit().expect("commit");
                assert_matches!(
                    result, TransactionCommitResult::Success(_),
                    "first commit should succeed",
                );
            }

            // Update our view of the property group.
            let updated = pg.update().expect("updated property group");
            assert_eq!(updated, PropertyGroupUpdateResult::Updated);

            // Second ensure: overwrites the property.
            {
                let tx = pg.transaction().expect("create transaction");
                let mut tx = tx.start().expect("start transaction");
                tx.property_ensure("prop", val2.as_value_ref())
                    .expect("property_ensure");
                let result = tx.commit().expect("commit");
                assert_matches!(
                    result, TransactionCommitResult::Success(_),
                    "second commit should succeed",
                );
            }
        }

        // Read back and verify the second value won.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");

            assert_eq!(readback, vec![val2]);
        }
    });
}

/// Use `property_change` to overwrite an existing property with a new
/// value of the same kind. Verify the new value is read back.
#[test]
fn transaction_property_change_overwrites() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(pair in strategy_same_kind_values())| {
        let (val1, val2) = pair;
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Write val1 via property_new.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new("prop", val1.as_value_ref())
                .expect("property_new");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "initial commit should succeed",
            );
        }

        // Overwrite with val2 via property_change.
        {
            let inst = instance.borrow();
            let mut pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_change("prop", val2.as_value_ref())
                .expect("property_change");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "change commit should succeed",
            );
        }

        // Read back and verify the second value won.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");

            assert_eq!(readback, vec![val2]);
        }
    });
}

/// Write a property via a transaction, then verify it is NOT visible
/// through the "running" snapshot until after `instance.refresh()`.
#[test]
fn transaction_snapshot_visibility_after_refresh() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(val: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Write phase: add a property group with a single property.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new("prop", val.as_value_ref())
                .expect("property_new");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "commit should succeed",
            );
        }

        // The new property group should NOT be visible through the
        // "running" snapshot before refresh.
        {
            let inst = instance.borrow();
            let snapshot = inst
                .snapshot("running")
                .expect("lookup snapshot")
                .expect("running snapshot should exist");
            let pg = snapshot
                .property_group_composed(&pg_name)
                .expect("lookup pg composed");
            assert!(
                pg.is_none(),
                "property group {pg_name} should not be visible \
                 in the running snapshot before refresh",
            );
        }

        // Refresh the instance so its "running" snapshot is updated.
        {
            let inst = instance.borrow();
            inst.refresh().expect("refresh");
        }

        // After refresh, the property group and its value SHOULD be
        // visible through the "running" snapshot.
        {
            let inst = instance.borrow();
            let snapshot = inst
                .snapshot("running")
                .expect("lookup snapshot")
                .expect("running snapshot should exist");
            let pg = snapshot
                .property_group_composed(&pg_name)
                .expect("lookup pg composed")
                .expect(
                    "property group should be visible in the \
                     running snapshot after refresh",
                );
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");

            assert_eq!(readback, vec![val]);
        }
    });
}

/// Set a property at the service level and verify visibility: visible
/// at the service level, not visible via instance direct-attach, but
/// visible via instance composed view and via the "running" snapshot
/// (only after refresh).
#[test]
fn service_property_composed_visibility() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = RefCell::new(scope.service("test-svc").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(val: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Write phase: add a property group at the service level.
        {
            let mut svc = service.borrow_mut();
            let mut pg = svc
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group to service");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new("prop", val.as_value_ref())
                .expect("property_new");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "commit should succeed",
            );
        }

        // The property group should be readable at the service level,
        // should NOT be visible via instance direct-attach, but SHOULD
        // be visible via instance composed view.
        {
            let svc = service.borrow();

            let pg = svc
                .property_group_direct(&pg_name)
                .expect("lookup service pg")
                .expect("pg should exist on service");
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");
            assert_eq!(readback, vec![val.clone()]);

            let inst = svc
                .instance("default")
                .expect("lookup instance")
                .expect("instance should exist");

            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup instance direct pg");
            assert!(
                pg.is_none(),
                "service-level pg {pg_name} should not be visible \
                 via instance direct-attach",
            );

            let pg = inst
                .property_group_composed(&pg_name)
                .expect("lookup instance composed pg")
                .expect(
                    "service-level pg should be visible via \
                     instance composed view",
                );
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");
            assert_eq!(readback, vec![val.clone()]);
        }

        // NOT visible via the "running" snapshot before refresh.
        {
            let svc = service.borrow();
            let inst = svc
                .instance("default")
                .expect("lookup instance")
                .expect("instance should exist");
            let snapshot = inst
                .snapshot("running")
                .expect("lookup snapshot")
                .expect("running snapshot should exist");
            let pg = snapshot
                .property_group_composed(&pg_name)
                .expect("lookup snapshot pg");
            assert!(
                pg.is_none(),
                "service-level pg {pg_name} should not be visible \
                 in the running snapshot before refresh",
            );
        }

        // Refresh the instance.
        {
            let svc = service.borrow();
            let inst = svc
                .instance("default")
                .expect("lookup instance")
                .expect("instance should exist");
            inst.refresh().expect("refresh");
        }

        // After refresh, visible via the "running" snapshot.
        {
            let svc = service.borrow();
            let inst = svc
                .instance("default")
                .expect("lookup instance")
                .expect("instance should exist");
            let snapshot = inst
                .snapshot("running")
                .expect("lookup snapshot")
                .expect("running snapshot should exist");
            let pg = snapshot
                .property_group_composed(&pg_name)
                .expect("lookup snapshot pg")
                .expect(
                    "service-level pg should be visible in the \
                     running snapshot after refresh",
                );
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");
            assert_eq!(readback, vec![val]);
        }
    });
}

/// Add a property group, verify it exists, delete it, verify it is
/// gone, then delete again and verify the `DoesNotExist` result.
#[test]
fn delete_property_group() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(val: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Add a property group and write a value into it.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new("prop", val.as_value_ref())
                .expect("property_new");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "commit should succeed",
            );
        }

        // Verify the property group and its property exist.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");
            assert_eq!(readback, vec![val]);
        }

        // Delete the property group.
        {
            let mut inst = instance.borrow_mut();
            let result = inst
                .delete_property_group(&pg_name)
                .expect("delete property group");
            assert_eq!(result, DeletePropertyGroupResult::Deleted);
        }

        // The property group should no longer exist.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg after delete");
            assert!(
                pg.is_none(),
                "pg {pg_name} should not exist after deletion",
            );
        }

        // Deleting again should return DoesNotExist.
        {
            let mut inst = instance.borrow_mut();
            let result = inst
                .delete_property_group(&pg_name)
                .expect("delete property group again");
            assert_eq!(result, DeletePropertyGroupResult::DoesNotExist);
        }
    });
}

/// Start a transaction, then modify the same property group through a
/// second handle before committing. The commit should return
/// `OutOfDate`.
#[test]
fn transaction_commit_out_of_date() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(val: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Create a property group with an initial value.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new("prop", val.as_value_ref())
                .expect("property_new");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "initial commit should succeed",
            );
        }

        // Start a transaction, then concurrently modify the property
        // group through a separate handle before committing. The
        // commit should return OutOfDate.
        {
            let inst = instance.borrow();
            let mut pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_ensure("prop", val.as_value_ref())
                .expect("property_ensure");

            // Modify the same property group through a second instance
            // handle obtained directly from the service. This bumps
            // the property group's version.
            {
                let inst2 = service
                    .instance("default")
                    .expect("lookup instance")
                    .expect("instance should exist");
                let mut pg2 = inst2
                    .property_group_direct(&pg_name)
                    .expect("lookup pg")
                    .expect("pg should exist");
                let tx2 = pg2.transaction().expect("create transaction");
                let mut tx2 = tx2.start().expect("start transaction");
                tx2.property_ensure("prop", val.as_value_ref())
                    .expect("property_ensure");
                let result = tx2.commit().expect("commit");
                assert_matches!(
                    result, TransactionCommitResult::Success(_),
                    "concurrent commit should succeed",
                );
            }

            // The original transaction should now be out of date.
            assert!(!tx.is_empty());
            let result = tx.commit().expect("commit");
            let stale_tx = assert_matches!(
                result, TransactionCommitResult::OutOfDate(tx) => tx,
                "commit should be out of date after concurrent \
                 modification",
            );
            // OutOfDate resets the transaction, clearing its entries.
            assert!(stale_tx.is_empty());
        }
    });
}

/// Verify that `property_new` with a NUL-containing property name
/// returns `TransactionError::InvalidName`. Also covers
/// `property_delete`, `property_ensure`, `property_change`, and
/// `property_change_type` which share the same name validation.
#[test]
fn transaction_invalid_property_name() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    let mut pg = instance
        .add_property_group(
            "pg",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add property group");

    let tx = pg.transaction().expect("create transaction");
    let mut tx = tx.start().expect("start transaction");
    assert!(tx.is_empty());

    let err = tx
        .property_new("prop\0bad", ValueRef::Bool(true))
        .expect_err("should fail with InvalidName");
    assert_matches!(err, TransactionBuildError::InvalidName { .. });

    let err = tx
        .property_delete("del\0bad")
        .expect_err("should fail with InvalidName");
    assert_matches!(err, TransactionBuildError::InvalidName { .. });

    let err = tx
        .property_ensure("ens\0bad", ValueRef::Bool(true))
        .expect_err("should fail with InvalidName");
    assert_matches!(
        err,
        // property_ensure() tries to look up the existing property first, so we
        // get an inner invalid name from that lookup instead of a top-level
        // `TransactionError::InvalidName`.
        TransactionBuildError::ExistenceLookup {
            err: LookupError::InvalidName { .. },
            ..
        }
    );

    let err = tx
        .property_change("chg\0bad", ValueRef::Bool(true))
        .expect_err("should fail with InvalidName");
    assert_matches!(err, TransactionBuildError::InvalidName { .. });

    let err = tx
        .property_change_type("ct\0bad", ValueRef::Bool(true))
        .expect_err("should fail with InvalidName");
    assert_matches!(err, TransactionBuildError::InvalidName { .. });

    // All operations failed before pushing entries; transaction is still empty.
    assert!(tx.is_empty());
}

/// Verify that `property_new_multiple` with mismatched value kinds
/// returns `TransactionError::TypeMismatch`.
#[test]
fn transaction_type_mismatch() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    let mut pg = instance
        .add_property_group(
            "pg",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add property group");

    let tx = pg.transaction().expect("create transaction");
    let mut tx = tx.start().expect("start transaction");
    assert!(tx.is_empty());

    let err = tx
        .property_new_multiple(
            "prop",
            ValueKind::Count,
            std::iter::once(ValueRef::Bool(true)),
        )
        .expect_err("should fail with TypeMismatch");
    assert_matches!(
        err,
        TransactionBuildError::TypeMismatch {
            property_type: ValueKind::Count,
            value_type: ValueKind::Bool,
            ..
        }
    );

    // Failed operation did not push an entry; transaction is still empty.
    assert!(tx.is_empty());
}

/// Verify that `pg.update()` returns `AlreadyUpToDate` when the
/// property group has not been modified and `Updated` after a
/// concurrent modification.
#[test]
fn property_group_update_already_up_to_date() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    let mut pg = instance
        .add_property_group(
            "pg",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add property group");

    // Immediately after creation, the handle should be up to date.
    let result = pg.update().expect("update");
    assert_eq!(result, PropertyGroupUpdateResult::AlreadyUpToDate);

    // Modify the property group through a second handle.
    {
        let inst2 = service.instance("default").unwrap().unwrap();
        let mut pg2 = inst2
            .property_group_direct("pg")
            .expect("lookup pg")
            .expect("pg should exist");
        let tx = pg2.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        tx.property_new("prop", ValueRef::Bool(true)).expect("property_new");
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }

    // The original handle should now see an update.
    let result = pg.update().expect("update");
    assert_eq!(result, PropertyGroupUpdateResult::Updated);

    // Calling update again with no further changes should be
    // AlreadyUpToDate.
    let result = pg.update().expect("update");
    assert_eq!(result, PropertyGroupUpdateResult::AlreadyUpToDate);
}

/// Write an arbitrary value via `property_new`, commit, then delete the
/// property in a new transaction. Verify the property is gone but the
/// property group still exists.
#[test]
fn transaction_property_delete() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(val: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Create a property group and write a value.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            assert!(tx.is_empty());
            tx.property_new("prop", val.as_value_ref())
                .expect("property_new");
            assert!(!tx.is_empty());
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "initial commit should succeed",
            );
        }

        // Delete the property in a new transaction.
        {
            let inst = instance.borrow();
            let mut pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            assert!(tx.is_empty());
            tx.property_delete("prop").expect("property_delete");
            assert!(!tx.is_empty());
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "delete commit should succeed",
            );
        }

        // The property group should still exist, but the property
        // should be gone.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should still exist after property deletion");
            let prop = pg.property("prop").expect("lookup property");
            assert!(
                prop.is_none(),
                "property should not exist after deletion",
            );
        }
    });
}

/// Use `property_change_type` to overwrite an existing property with a
/// value of a potentially different kind. Verify the new value is read
/// back.
#[test]
fn transaction_property_change_type() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(val1: Value, val2: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Write val1 via property_new.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new("prop", val1.as_value_ref())
                .expect("property_new");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "initial commit should succeed",
            );
        }

        // Overwrite with val2 via property_change_type (may change
        // the property's type).
        {
            let inst = instance.borrow();
            let mut pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_change_type("prop", val2.as_value_ref())
                .expect("property_change_type");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "change_type commit should succeed",
            );
        }

        // Read back and verify the second value won.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback: Vec<Value> = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .values()
                .expect("get values")
                .collect::<Result<Vec<_>, _>>()
                .expect("iterate values");

            assert_eq!(readback, vec![val2]);
        }
    });
}

/// Test `Property::single_value()`: happy path via proptest,
/// `MultipleValues` error for multi-valued properties, and
/// `NoValues` error if achievable through the API.
#[test]
fn single_value_and_errors() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    // (a) Happy path: write a single arbitrary value, read back via
    // single_value().
    let pg_counter = AtomicU32::new(0);

    proptest!(|(val: Value)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    PropertyGroupType::Application,
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new("prop", val.as_value_ref())
                .expect("property_new");
            let result = tx.commit().expect("commit");
            assert_matches!(
                result, TransactionCommitResult::Success(_),
                "commit should succeed",
            );
        }

        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback = pg
                .property("prop")
                .expect("lookup property")
                .expect("property should exist")
                .single_value()
                .expect("single_value");
            assert_eq!(readback, val);
        }
    });

    // (b) MultipleValues error: write two values, then call
    // single_value().
    {
        let mut inst = instance.borrow_mut();
        let mut pg = inst
            .add_property_group(
                "pg_multi",
                PropertyGroupType::Application,
                AddPropertyGroupFlags::Persistent,
            )
            .expect("add property group");

        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        tx.property_new_multiple(
            "prop",
            ValueKind::Count,
            [ValueRef::Count(1), ValueRef::Count(2)],
        )
        .expect("property_new_multiple");
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }
    {
        let inst = instance.borrow();
        let pg = inst
            .property_group_direct("pg_multi")
            .expect("lookup pg")
            .expect("pg should exist");
        let err = pg
            .property("prop")
            .expect("lookup property")
            .expect("property should exist")
            .single_value()
            .expect_err("should fail with MultipleValues");
        assert_matches!(err, SingleValueError::MultipleValues { .. });
    }

    // (c) NoValues error: create a property with zero values, then call
    // single_value().
    {
        let mut inst = instance.borrow_mut();
        let mut pg = inst
            .add_property_group(
                "pg_empty",
                PropertyGroupType::Application,
                AddPropertyGroupFlags::Persistent,
            )
            .expect("add property group");

        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        tx.property_new_multiple("prop", ValueKind::Count, std::iter::empty())
            .expect("property_new_multiple with empty values");
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }
    {
        let inst = instance.borrow();
        let pg = inst
            .property_group_direct("pg_empty")
            .expect("lookup pg")
            .expect("pg should exist");
        let prop =
            pg.property("prop").expect("lookup property").expect("prop exists");
        let err = prop.single_value().expect_err("should fail with NoValues");
        assert_matches!(err, SingleValueError::NoValues { .. });

        // values() should work and give us an empty iterator
        let mut values = prop.values().expect("create iterator");
        assert_matches!(values.next(), None);
    }
}

/// After a commit returns `OutOfDate`, drop the stale transaction,
/// update the property group, and retry with a new transaction. The
/// retry should succeed.
#[test]
fn transaction_reset_and_retry_after_out_of_date() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    // Create a PG with an initial value.
    let mut pg = instance
        .add_property_group(
            "retrypg",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add property group");

    {
        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        assert!(tx.is_empty());
        tx.property_new("prop", ValueRef::Count(1)).expect("property_new");
        assert!(!tx.is_empty());
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }

    pg.update().expect("update");

    // Start a transaction to change the value, then cause a concurrent
    // modification so our commit returns OutOfDate.
    {
        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        assert!(tx.is_empty());
        tx.property_ensure("prop", ValueRef::Count(42))
            .expect("property_ensure");
        assert!(!tx.is_empty());

        // Concurrently modify the PG through a second handle.
        {
            let inst2 = service
                .instance("default")
                .expect("lookup instance")
                .expect("instance should exist");
            let mut pg2 = inst2
                .property_group_direct("retrypg")
                .expect("lookup pg")
                .expect("pg should exist");
            let tx2 = pg2.transaction().expect("create transaction");
            let mut tx2 = tx2.start().expect("start transaction");
            tx2.property_ensure("prop", ValueRef::Count(99))
                .expect("property_ensure");
            let result = tx2.commit().expect("commit");
            assert_matches!(result, TransactionCommitResult::Success(_));
        }

        let result = tx.commit().expect("commit");
        let stale_tx = assert_matches!(
            result,
            TransactionCommitResult::OutOfDate(tx) => tx,
            "commit should be out of date",
        );
        // OutOfDate resets the transaction, clearing its entries.
        assert!(stale_tx.is_empty());
        // Drop the stale transaction (releases &mut pg).
    }

    // Update the PG to pick up the concurrent change, then retry with a
    // new transaction.
    pg.update().expect("update after out of date");

    {
        let tx = pg.transaction().expect("create retry transaction");
        let mut tx = tx.start().expect("start retry transaction");
        assert!(tx.is_empty());
        tx.property_ensure("prop", ValueRef::Count(42))
            .expect("property_ensure on retry");
        assert!(!tx.is_empty());
        let result = tx.commit().expect("retry commit");
        assert_matches!(
            result,
            TransactionCommitResult::Success(_),
            "retry commit should succeed",
        );
    }

    // Read back and verify the retried value won.
    {
        let inst = service
            .instance("default")
            .expect("lookup instance")
            .expect("instance should exist");
        let pg = inst
            .property_group_direct("retrypg")
            .expect("lookup pg")
            .expect("pg should exist");
        let readback = pg
            .property("prop")
            .expect("lookup property")
            .expect("property should exist")
            .single_value()
            .expect("single_value");
        assert_eq!(readback, Value::Count(42));
    }
}

/// Exercise `property_ensure_multiple` and `property_change_multiple`
/// with fixed multi-value Count properties.
#[test]
fn transaction_multi_value_ensure_and_change() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    let mut pg = instance
        .add_property_group(
            "mvpg",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add property group");

    // (a) ensure_multiple creates the property with [1, 2, 3].
    {
        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        assert!(tx.is_empty());
        tx.property_ensure_multiple(
            "prop",
            ValueKind::Count,
            [ValueRef::Count(1), ValueRef::Count(2), ValueRef::Count(3)],
        )
        .expect("property_ensure_multiple");
        assert!(!tx.is_empty());
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }

    {
        let inst = service.instance("default").unwrap().unwrap();
        let pg_read = inst
            .property_group_direct("mvpg")
            .expect("lookup pg")
            .expect("pg should exist");
        let readback: Vec<Value> = pg_read
            .property("prop")
            .expect("lookup property")
            .expect("property should exist")
            .values()
            .expect("get values")
            .collect::<Result<Vec<_>, _>>()
            .expect("iterate values");
        assert_eq!(
            readback,
            vec![Value::Count(1), Value::Count(2), Value::Count(3)],
        );
    }

    // (b) ensure_multiple overwrites with [4, 5].
    pg.update().expect("update");
    {
        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        assert!(tx.is_empty());
        tx.property_ensure_multiple(
            "prop",
            ValueKind::Count,
            [ValueRef::Count(4), ValueRef::Count(5)],
        )
        .expect("property_ensure_multiple overwrite");
        assert!(!tx.is_empty());
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }

    {
        let inst = service.instance("default").unwrap().unwrap();
        let pg_read = inst
            .property_group_direct("mvpg")
            .expect("lookup pg")
            .expect("pg should exist");
        let readback: Vec<Value> = pg_read
            .property("prop")
            .expect("lookup property")
            .expect("property should exist")
            .values()
            .expect("get values")
            .collect::<Result<Vec<_>, _>>()
            .expect("iterate values");
        assert_eq!(readback, vec![Value::Count(4), Value::Count(5)]);
    }

    // (c) change_multiple replaces with [6].
    pg.update().expect("update");
    {
        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        assert!(tx.is_empty());
        tx.property_change_multiple(
            "prop",
            ValueKind::Count,
            [ValueRef::Count(6)],
        )
        .expect("property_change_multiple");
        assert!(!tx.is_empty());
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }

    {
        let inst = service.instance("default").unwrap().unwrap();
        let pg_read = inst
            .property_group_direct("mvpg")
            .expect("lookup pg")
            .expect("pg should exist");
        let readback: Vec<Value> = pg_read
            .property("prop")
            .expect("lookup property")
            .expect("property should exist")
            .values()
            .expect("get values")
            .collect::<Result<Vec<_>, _>>()
            .expect("iterate values");
        assert_eq!(readback, vec![Value::Count(6)]);
    }
}
