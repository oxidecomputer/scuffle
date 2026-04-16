// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(target_os = "illumos")]

use assert_matches::assert_matches;
use scuffle::AddPropertyGroupFlags;
use scuffle::EditPropertyGroups;
use scuffle::HasComposedPropertyGroups;
use scuffle::HasDirectPropertyGroups;
use scuffle::PropertyGroupType;
use scuffle::Scf;
use scuffle::TransactionCommitResult;
use scuffle::ValueRef;
use scuffle::isolated::IsolatedConfigd;
use std::collections::BTreeSet;

/// Exercise the iterator types: `Service::instances()`,
/// `Instance::snapshots()`, `Instance::property_groups_direct()`,
/// `Instance::property_groups_composed()`, and
/// `Snapshot::property_groups_composed()`.
#[test]
fn iterators() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    // Service::instances() should contain "default".
    {
        let instances: Vec<_> = service
            .instances()
            .expect("iterate instances")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect instances");
        let names: Vec<&str> = instances.iter().map(|i| i.name()).collect();
        assert_eq!(names, vec!["default"]);
    }

    // Create two property groups for iteration tests below.
    instance
        .add_property_group(
            "iterpg1",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add iterpg1");
    instance
        .add_property_group(
            "iterpg2",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add iterpg2");

    // Instance::property_groups_direct() should contain both PGs.
    {
        let pgs: Vec<_> = instance
            .property_groups_direct()
            .expect("iterate direct pgs")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect direct pgs");
        let names: BTreeSet<&str> = pgs.iter().map(|pg| pg.name()).collect();
        assert!(
            names.is_superset(&BTreeSet::from(["iterpg1", "iterpg2"])),
            "unexpected names: {names:?}"
        );
    }

    // Instance::property_groups_composed() should also contain both.
    {
        let pgs: Vec<_> = instance
            .property_groups_composed()
            .expect("iterate composed pgs")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect composed pgs");
        let names: BTreeSet<&str> = pgs.iter().map(|pg| pg.name()).collect();
        assert!(
            names.is_superset(&BTreeSet::from(["iterpg1", "iterpg2"])),
            "unexpected names: {names:?}"
        );
    }

    // Refresh the instance so snapshots pick up the new PGs.
    instance.refresh().expect("refresh");

    // Instance::snapshots() should contain "running" after refresh.
    {
        let snapshots: Vec<_> = instance
            .snapshots()
            .expect("iterate snapshots")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect snapshots");
        let names: Vec<&str> = snapshots.iter().map(|s| s.name()).collect();
        assert!(
            names.contains(&"running"),
            "expected 'running' snapshot, got {names:?}",
        );
    }

    // Snapshot accessors and property_groups_composed().
    {
        let snapshot = instance
            .snapshot("running")
            .expect("lookup snapshot")
            .expect("running snapshot should exist");

        // Verify Snapshot::name() and Snapshot::instance_fmri().
        assert_eq!(snapshot.name(), "running");
        assert_eq!(snapshot.instance_fmri(), "svc:/test-svc:default");

        let pgs: Vec<_> = snapshot
            .property_groups_composed()
            .expect("iterate snapshot composed pgs")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect snapshot composed pgs");
        let names: BTreeSet<&str> = pgs.iter().map(|pg| pg.name()).collect();
        assert!(
            names.is_superset(&BTreeSet::from(["iterpg1", "iterpg2"])),
            "unexpected names: {names:?}"
        );
    }
}

/// Exercise the `PropertyGroup::properties()` iterator and verify
/// `Property::name()` and `Property::fmri()` on each yielded item.
#[test]
fn property_iterator() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    // Create a property group with three properties.
    {
        let mut pg = instance
            .add_property_group(
                "propiterpg",
                PropertyGroupType::Application,
                AddPropertyGroupFlags::Persistent,
            )
            .expect("add property group");

        let tx = pg.transaction().expect("create transaction");
        let mut tx = tx.start().expect("start transaction");
        tx.property_new("p1", ValueRef::Bool(true)).expect("property_new p1");
        tx.property_new("p2", ValueRef::Count(42)).expect("property_new p2");
        tx.property_new("p3", ValueRef::Integer(-1)).expect("property_new p3");
        let result = tx.commit().expect("commit");
        assert_matches!(result, TransactionCommitResult::Success(_));
    }

    // Re-fetch the property group and iterate its properties.
    let pg = instance
        .property_group_direct("propiterpg")
        .expect("lookup pg")
        .expect("pg should exist");

    let props: Vec<_> = pg
        .properties()
        .expect("iterate properties")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect properties");

    let names: BTreeSet<&str> = props.iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        BTreeSet::from(["p1", "p2", "p3"]),
        "unexpected property names: {names:?}",
    );

    // Verify Property::fmri() includes the property group and property name.
    for prop in &props {
        match prop.name() {
            "p1" => assert_eq!(
                prop.fmri(),
                "svc:/test-svc:default/:properties/propiterpg/p1"
            ),
            "p2" => assert_eq!(
                prop.fmri(),
                "svc:/test-svc:default/:properties/propiterpg/p2"
            ),
            "p3" => assert_eq!(
                prop.fmri(),
                "svc:/test-svc:default/:properties/propiterpg/p3"
            ),
            other => panic!("unexpected property name {other}"),
        }
    }
}
