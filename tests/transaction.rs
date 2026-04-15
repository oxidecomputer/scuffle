// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use assert_matches::assert_matches;
use proptest::prelude::any;
use proptest::proptest;
use proptest::strategy::Strategy;
use scuffle::AddPropertyGroupFlags;
use scuffle::EditPropertyGroups;
use scuffle::HasDirectPropertyGroups;
use scuffle::PropertyGroupUpdateResult;
use scuffle::Scf;
use scuffle::TransactionCommitResult;
use scuffle::Value;
use scuffle::ValueKind;
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
    let instance =
        RefCell::new(service.instance("default").unwrap().unwrap());

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
                    "application",
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
    let instance =
        RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    // Generate 1..=8 Count values. We already test all value types in
    // test 1; the point here is to exercise multi-value mechanics.
    let strategy =
        proptest::collection::vec(any::<u64>(), 1..=8).prop_map(|counts| {
            counts.into_iter().map(Value::Count).collect::<Vec<_>>()
        });

    proptest!(|(values in strategy)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("mpg{n}");

        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    "application",
                    AddPropertyGroupFlags::Persistent,
                )
                .expect("add property group");

            let tx = pg.transaction().expect("create transaction");
            let mut tx = tx.start().expect("start transaction");
            tx.property_new_multiple(
                "prop",
                ValueKind::Count,
                values.iter().map(|v| v.as_value_ref()),
            )
            .expect("property_new_multiple");
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

/// Use `property_ensure` to write a value, then overwrite it with a
/// different value of the same kind. Verify the second value wins.
#[test]
fn transaction_property_ensure_overwrites() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance =
        RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    // Generate pairs of values that share the same ValueKind.
    let strategy = any::<Value>().prop_flat_map(|v1| {
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
    });

    proptest!(|(pair in strategy)| {
        let (val1, val2) = pair;
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("epg{n}");

        // Write both values through property_ensure, then read back.
        {
            let mut inst = instance.borrow_mut();
            let mut pg = inst
                .add_property_group(
                    &pg_name,
                    "application",
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
