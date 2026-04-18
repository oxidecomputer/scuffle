<!-- cargo-sync-rdme title [[ -->
# scuffle
<!-- cargo-sync-rdme ]] -->
<!-- cargo-sync-rdme rustdoc [[ -->
## scuffle

`scuffle` provides an illumos-only wrapper around [`libscf`][manlibscf],
focused on interactions with property groups and properties of existing
services, instances, and snapshots. [Understanding SMF Properties][blog] is
highly recommended background reading.

There are large swathes of `libscf` that are currently *not* available via
`scuffle` due to its focus on properties, including:

* The ability to create or delete services
* Interacting with running instances or services in any way other than
  refreshing instances
* All functionality related to `snaplevel`s
* All functionality related to templates

## Examples

Reading all properties from a specific property group in a service
instance’s `running` snapshot:

````rust,no_run
fn read_from_snapshot(
    service_name: &str,
    instance_name: &str,
    property_group_name: &str,
) -> anyhow::Result<BTreeMap<String, Vec<Value>>> {
    // Get a handle to scf and the local scope.
    let scf = Scf::connect_global_zone()?;
    let scope = scf.scope_local()?;

    // Look up the property group within our snapshot by stepping through
    // each level.
    let Some(service) = scope.service(service_name)? else {
        bail!("service {service_name} not found");
    };
    let Some(instance) = service.instance(instance_name)? else {
        bail!("instance {instance_name} not found within {}", service.fmri());
    };
    let Some(snapshot) = instance.snapshot("running")? else {
        bail!("no running snapshot found for {}", instance.fmri());
    };
    let Some(pg) = snapshot.property_group_composed(property_group_name)? else {
        bail!(
            "property group {property_group_name} not found for {}",
            instance.fmri(),
        );
    };

    let mut all_properties = BTreeMap::new();
    for property in pg.properties()? {
        let property = property?;
        let values = property.values()?.collect::<Result<_, _>>()?;
        all_properties.insert(property.name().to_string(), values);
    }
    Ok(all_properties)
}
````

Adding a new property to an existing property group of a service instance:

````rust,no_run
fn add_new_property(
    service_name: &str,
    instance_name: &str,
    property_group_name: &str,
    property_name: &str,
    value: ValueRef<'_>,
) -> anyhow::Result<()> {
    // Get a handle to scf and the local scope.
    let scf = Scf::connect_global_zone()?;
    let scope = scf.scope_local()?;

    // Look up the property group within our instance by stepping through
    // each level.
    let Some(service) = scope.service(service_name)? else {
        bail!("service {service_name} not found");
    };
    let Some(instance) = service.instance(instance_name)? else {
        bail!("instance {instance_name} not found within {}", service.fmri());
    };
    let Some(mut pg) = instance.property_group_direct(property_group_name)? else {
        bail!(
            "property group {property_group_name} not found for {}",
            instance.fmri(),
        );
    };

    // Open a transaction on the property group.
    let tx = pg.transaction()?;

    // Start the transaction. This takes a snapshot of the property group's
    // current version; if it's modified by someone else between this point
    // and our attempt to `commit()` below, we'll get an `OutOfDate` result.
    let mut tx = tx.start()?;

    // Add an entry to the transaction to create the new property.
    tx.property_new(property_name, value)?;

    // Commit the transaction.
    match tx.commit()? {
        TransactionCommitResult::Success(_committed_tx) => Ok(()),
        TransactionCommitResult::OutOfDate(_reset_tx) => {
            // We'll return an error for this example, but real code may
            // want to call `pg.update()` and try again.
            bail!("property group concurrently modified");
        }
    }
}
````

See the `examples/` directory for more complete examples.

## Error types

`scuffle`’s errors aim to provide extensive context; e.g., an error that
occurs while operating on an instance will include that instance’s FMRI.
These error types make extensive use of source errors as discussed in
[Defining Error Types and Logging Errors][error-doc]. It is critical that
printing or logging of these error types walk the entire error chain as
discussed in that document, or the underlying error(s) will not be emitted.

The examples above use `anyhow` to allow easy `?`-propagation despite the
multiple errors involved. `scuffle` does not currently provide a catch-all
error type of its own.

## Features

`scuffle` has three optional Cargo features:

* Enabling the `daft` feature adds implementations of
  [`daft::Diffable`][diffable] to [`Value`](https://docs.rs/scuffle/0.1.0/scuffle/value/enum.Value.html), [`ValueRef`](https://docs.rs/scuffle/0.1.0/scuffle/value/enum.ValueRef.html), and
  [`ValueKind`](https://docs.rs/scuffle/0.1.0/scuffle/value/enum.ValueKind.html).
* Enabling the `smf-by-instance` feature adds several `Instance::smf_*`
  methods for controlling the SMF state of an instance, but requires a
  `libscf` that includes [recently-stabilized
  APIs](https://www.illumos.org/issues/18043).
* Enabling the `testing` feature adds types to support writing tests that
  interact with SMF without needing to modify system-level SMF services /
  instances / properties; see “Testing Support” below.

## Stability

`scuffle` makes use of some non-public interfaces. Specifically:

* \[`Scf::connect_zone()`\] uses an undocumented SCF handle decoration to
  connect to `svc.configd` inside the specified zone. This matches how
  `svcadm` and `svcprop` implement their `-z zone` flags.
* If the `smf-by-instance` Cargo feature is not enabled,
  \[`Instance::smf_refresh()`\] uses a non-public function defined by
  `libscf_priv.h` (`_smf_refresh_instance_i()`).
* If the `testing` feature is enabled, it uses several other non-public
  interfaces; see “Testing Support” below.

## Testing Support

If the `testing` feature is enabled, `scuffle` exports an
[`isolated::IsolatedConfigd`](https://docs.rs/scuffle/0.1.0/scuffle/isolated/struct.IsolatedConfigd.html) type that can run an instance of `svc.configd`
inside a temporary directory. After creating an instance of this type, tests
can connect to it via \[`Scf::connect_isolated()`\], and then freely read and
write properties within that instance without touching the real system’s
`svc.configd` and without the permissions normally required to do that.

This comes with several caveats:

* `IsolatedConfigd` takes advantage of undocumented and uncommitted
  interfaces in at least `svccfg`, `svc.configd`, and `libscf` itself. These
  may break in the future.
* Refreshing instances inside an `IsolatedConfigd` via `libscf` does not
  work. `scuffle` works around this by refreshing instances via `svccfg`
  pointed at the isolated `svc.configd`, but this is a divergence from what
  production code will do for `refresh`.
* Non-persistent property groups do not work inside `IsolatedConfigd`.

`scuffle` uses `IsolatedConfigd` for its own tests, and therefore does not
have test coverage on features that interact with these restrictions.

[manlibscf]: https://smartos.org/man/3LIB/libscf
[blog]: https://www.davepacheco.net/blog/2026/smf-properties/
[error-doc]: https://github.com/oxidecomputer/omicron/blob/main/docs/error-types-and-logging.adoc
[diffable]: https://docs.rs/daft/latest/daft/trait.Diffable.html
<!-- cargo-sync-rdme ]] -->
