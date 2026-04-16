// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(target_os = "illumos")]

use assert_matches::assert_matches;
use proptest::proptest;
use scuffle::AddPropertyGroupFlags;
use scuffle::EditPropertyGroups;
use scuffle::HasDirectPropertyGroups;
use scuffle::PropertyGroupType;
use scuffle::Scf;
use scuffle::error::PropertyGroupTypeError;
use scuffle::isolated::IsolatedConfigd;
use std::cell::RefCell;
use std::process::Command;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

/// Add a property group with an arbitrary type, then read the type back
/// via `PropertyGroup::type_()` and verify it matches.
#[test]
fn property_group_type_roundtrip() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = RefCell::new(service.instance("default").unwrap().unwrap());

    let pg_counter = AtomicU32::new(0);

    proptest!(|(pg_type: PropertyGroupType)| {
        let n = pg_counter.fetch_add(1, Ordering::Relaxed);
        let pg_name = format!("pg{n}");

        // Create a property group with the given type.
        {
            let mut inst = instance.borrow_mut();
            inst.add_property_group(
                &pg_name,
                pg_type,
                AddPropertyGroupFlags::Persistent,
            )
            .expect("add property group");
        }

        // Read the type back and verify it matches.
        {
            let inst = instance.borrow();
            let pg = inst
                .property_group_direct(&pg_name)
                .expect("lookup pg")
                .expect("pg should exist");
            let readback_type = pg.type_().expect("get property group type");
            assert_eq!(readback_type, pg_type);
        }
    });
}

/// Create a property group with a non-standard type via `svccfg`, then
/// verify that `type_()` returns `PropertyGroupTypeError::UnknownType`.
#[test]
fn property_group_type_unknown() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = service.instance("default").unwrap().unwrap();

    // Use svccfg to add a property group with a non-standard type,
    // bypassing scuffle's PropertyGroupType validation.
    let output = Command::new("svccfg")
        .env("SVCCFG_DOOR", isolated.door_path().as_str())
        .args([
            "-s",
            "svc:/test-svc:default",
            "addpg",
            "custom_pg",
            "weird_type",
        ])
        .output()
        .expect("exec svccfg");
    assert!(
        output.status.success(),
        "svccfg addpg failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pg = instance
        .property_group_direct("custom_pg")
        .expect("lookup pg")
        .expect("pg should exist");
    let err = pg.type_().expect_err("type_() should fail for unknown type");
    assert_matches!(err, PropertyGroupTypeError::UnknownType { type_, .. } => {
        assert_eq!(&*type_, "weird_type");
    });
}
