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
use scuffle::error::InstanceFromEnvError;
use scuffle::error::InstanceFromFmriError;
use scuffle::error::LibscfError;
use scuffle::error::LookupError;
use scuffle::error::PropertyGroupAddError;
use scuffle::error::ScfEntity;
use scuffle::isolated::IsolatedConfigd;

#[test]
fn instance_from_fmri_success() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let instance = scf
        .instance_from_fmri("svc:/test-svc:default")
        .expect("instance_from_fmri");
    assert_eq!(instance.name(), "default");
    assert_eq!(instance.fmri(), "svc:/test-svc:default");
}

#[test]
fn instance_from_fmri_nonexistent_service() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let err = scf
        .instance_from_fmri("svc:/no-such-service:default")
        .expect_err("should fail for nonexistent service");
    assert_matches!(
        err,
        InstanceFromFmriError::Get { fmri, err: LibscfError::NotFound } => {
            assert_eq!(&*fmri, "svc:/no-such-service:default");
        }
    );
}

#[test]
fn instance_from_fmri_nonexistent_instance() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let err = scf
        .instance_from_fmri("svc:/test-svc:nonexistent")
        .expect_err("should fail for nonexistent instance");
    assert_matches!(
        err,
        InstanceFromFmriError::Get { fmri, err: LibscfError::NotFound } => {
            assert_eq!(&*fmri, "svc:/test-svc:nonexistent");
        }
    );
}

#[test]
fn instance_from_fmri_service_only() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let err = scf
        .instance_from_fmri("svc:/test-svc")
        .expect_err("should fail for service-only FMRI");
    assert_matches!(
        err,
        InstanceFromFmriError::Get { fmri, .. } => {
            assert_eq!(&*fmri, "svc:/test-svc");
        }
    );
}

#[test]
fn instance_from_fmri_invalid_nul() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let err = scf
        .instance_from_fmri("svc:/test\0-svc:default")
        .expect_err("should fail for FMRI with embedded NUL");
    assert_matches!(
        err,
        InstanceFromFmriError::InvalidFmri { fmri, .. } => {
            assert_eq!(&*fmri, "svc:/test\0-svc:default");
        }
    );
}

#[test]
fn instance_from_fmri_empty() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let err =
        scf.instance_from_fmri("").expect_err("should fail for empty FMRI");
    assert_matches!(
        err,
        InstanceFromFmriError::Get { fmri, .. } => {
            assert_eq!(&*fmri, "");
        }
    );
}

// All self_instance_from_env cases are in a single test because they mutate the
// process-wide SMF_FMRI environment variable and cannot safely run concurrently
// with each other.
#[test]
fn self_instance_from_env() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();

    // Success: SMF_FMRI points to a valid instance.
    unsafe { std::env::set_var("SMF_FMRI", "svc:/test-svc:default") };
    let result = scf.self_instance_from_env();
    unsafe { std::env::remove_var("SMF_FMRI") };

    let instance = result.expect("self_instance_from_env");
    assert_eq!(instance.name(), "default");
    assert_eq!(instance.fmri(), "svc:/test-svc:default");
    drop(instance);

    // Missing env var: SMF_FMRI is not set.
    unsafe { std::env::remove_var("SMF_FMRI") };
    let err =
        scf.self_instance_from_env().expect_err("should fail without SMF_FMRI");
    assert_matches!(
        err,
        InstanceFromEnvError::EnvLookup { env_var, .. } => {
            assert_eq!(env_var, "SMF_FMRI");
        }
    );

    // Invalid FMRI: SMF_FMRI points to a nonexistent service.
    unsafe { std::env::set_var("SMF_FMRI", "svc:/bogus:default") };
    let result = scf.self_instance_from_env();
    unsafe { std::env::remove_var("SMF_FMRI") };

    let err = result.expect_err("should fail for nonexistent FMRI");
    assert_matches!(
        err, InstanceFromEnvError::InstanceFromFmri(
            InstanceFromFmriError::Get { fmri, err: LibscfError::NotFound }
        ) => {
            assert_eq!(&*fmri, "svc:/bogus:default");
        }
    );
}

/// Verify that `ensure_property_group` is idempotent: calling it when the
/// property group already exists returns a valid handle, and calling it on
/// a new name creates the property group.
#[test]
fn ensure_property_group_idempotent() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    // Create a property group via add_property_group.
    instance
        .add_property_group(
            "epg",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add property group");

    // ensure_property_group on the same name should succeed (not error
    // with Exists).
    {
        let pg = instance
            .ensure_property_group(
                "epg",
                PropertyGroupType::Application,
                AddPropertyGroupFlags::Persistent,
            )
            .expect("ensure existing property group");
        assert_eq!(pg.name(), "epg");
    }

    // Verify it's still there via direct lookup.
    {
        let pg = instance
            .property_group_direct("epg")
            .expect("lookup epg")
            .expect("epg should exist");
        assert_eq!(pg.name(), "epg");
    }

    // ensure_property_group on a name that does NOT yet exist should
    // create it.
    {
        let pg = instance
            .ensure_property_group(
                "epg_new",
                PropertyGroupType::Application,
                AddPropertyGroupFlags::Persistent,
            )
            .expect("ensure new property group");
        assert_eq!(pg.name(), "epg_new");
    }

    // Verify the newly-created PG exists.
    {
        let pg = instance
            .property_group_direct("epg_new")
            .expect("lookup epg_new")
            .expect("epg_new should exist");
        assert_eq!(pg.name(), "epg_new");
    }
}

/// Verify that `add_property_group` with a NUL-containing name returns
/// `PropertyGroupAddError::InvalidName`.
#[test]
fn add_property_group_invalid_name() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    let err = instance
        .add_property_group(
            "bad\0name",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect_err("should fail with InvalidName");
    assert_matches!(err, PropertyGroupAddError::InvalidName { .. });
}

/// Verify that calling `add_property_group` twice with the same name
/// returns `PropertyGroupAddError::Add` with `LibscfError::Exists`.
#[test]
fn add_property_group_already_exists() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    instance
        .add_property_group(
            "dup",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("first add should succeed");

    let err = instance
        .add_property_group(
            "dup",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect_err("second add should fail with Exists");
    assert_matches!(
        err,
        PropertyGroupAddError::Add { err: LibscfError::Exists, .. }
    );
}

/// Verify that lookup methods return `Ok(None)` for nonexistent entity
/// names (as opposed to erroring).
#[test]
fn lookup_nonexistent_returns_none() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let instance = service.instance("default").unwrap().unwrap();

    // Scope::service
    assert!(
        scope.service("no-such-svc").unwrap().is_none(),
        "nonexistent service should be None",
    );

    // Service::instance
    assert!(
        service.instance("no-such-inst").unwrap().is_none(),
        "nonexistent instance should be None",
    );

    // Instance::snapshot
    assert!(
        instance.snapshot("no-such-snap").unwrap().is_none(),
        "nonexistent snapshot should be None",
    );

    // Instance::property_group_direct
    assert!(
        instance.property_group_direct("no-such-pg").unwrap().is_none(),
        "nonexistent direct property group should be None",
    );

    // Instance::property_group_composed
    assert!(
        instance.property_group_composed("no-such-pg").unwrap().is_none(),
        "nonexistent composed property group should be None",
    );
}

/// Verify that lookup methods return `LookupError::InvalidName` when
/// given names containing embedded NUL bytes.
#[test]
fn lookup_invalid_name() {
    let isolated =
        IsolatedConfigd::builder("test-svc").unwrap().build().unwrap();
    let scf = Scf::connect_isolated(&isolated).unwrap();
    let scope = scf.scope_local().unwrap();
    let service = scope.service("test-svc").unwrap().unwrap();
    let mut instance = service.instance("default").unwrap().unwrap();

    // Scope::service with NUL
    let err =
        scope.service("svc\0bad").expect_err("should fail with InvalidName");
    assert_matches!(
        err,
        LookupError::InvalidName { entity: ScfEntity::Service, .. }
    );

    // Service::instance with NUL
    let err = service
        .instance("inst\0bad")
        .expect_err("should fail with InvalidName");
    assert_matches!(
        err,
        LookupError::InvalidName { entity: ScfEntity::Instance, .. }
    );

    // Instance::snapshot with NUL
    let err = instance
        .snapshot("snap\0bad")
        .expect_err("should fail with InvalidName");
    assert_matches!(
        err,
        LookupError::InvalidName { entity: ScfEntity::Snapshot, .. }
    );

    // Instance::property_group_direct with NUL
    let err = instance
        .property_group_direct("pg\0bad")
        .expect_err("should fail with InvalidName");
    assert_matches!(
        err,
        LookupError::InvalidName { entity: ScfEntity::PropertyGroup, .. }
    );

    // Instance::property_group_composed with NUL
    let err = instance
        .property_group_composed("pg\0bad")
        .expect_err("should fail with InvalidName");
    assert_matches!(
        err,
        LookupError::InvalidName { entity: ScfEntity::PropertyGroup, .. }
    );

    // Property lookup with NUL (need a real PG first)
    let pg = instance
        .add_property_group(
            "pg",
            PropertyGroupType::Application,
            AddPropertyGroupFlags::Persistent,
        )
        .expect("add property group");
    match pg.property("prop\0bad") {
        Err(err) => assert_matches!(
            err,
            LookupError::InvalidName { entity: ScfEntity::Property, .. }
        ),
        Ok(_) => panic!("should fail with InvalidName"),
    }
}
