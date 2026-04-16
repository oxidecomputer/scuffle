// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use scuffle::AddPropertyGroupFlags;
use scuffle::EditPropertyGroups;
use scuffle::HasComposedPropertyGroups;
use scuffle::HasDirectPropertyGroups;
use scuffle::PropertyGroupType;
use scuffle::Scf;
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
