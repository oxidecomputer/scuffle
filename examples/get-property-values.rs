// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::bail;
use clap::Parser;
use scuffle::HasComposedPropertyGroups;
use scuffle::HasDirectPropertyGroups;
use scuffle::Property;
use scuffle::PropertyGroup;
use scuffle::Scf;

#[derive(Parser)]
#[command(about = "Print the values of an SMF service property")]
struct Args {
    service: String,
    #[arg(long)]
    instance: Option<String>,
    #[arg(long, requires = "instance", conflicts_with = "snapshot")]
    composed: bool,
    #[arg(long, requires = "instance")]
    snapshot: Option<String>,
    property_group: Option<String>,
    #[arg(requires = "property_group")]
    property: Option<String>,
}

fn print_property_values<St>(prop: &Property<'_, St>) -> anyhow::Result<()> {
    for value in prop.values()? {
        let value = value?;
        println!("{} {}", prop.fmri(), value.display_smf());
    }
    Ok(())
}

fn print_properties<St>(pg: &PropertyGroup<'_, St>) -> anyhow::Result<()> {
    for prop in pg.properties()? {
        let prop = prop?;
        print_property_values(&prop)?;
    }
    Ok(())
}

fn run_direct(
    target: &impl HasDirectPropertyGroups,
    name: &str,
    property_group: Option<String>,
    property: Option<String>,
) -> anyhow::Result<()> {
    if let Some(property_group) = property_group {
        let Some(pg) = target.property_group_direct(&property_group)? else {
            bail!("property group `{property_group}` not found in `{name}`");
        };

        if let Some(property) = property {
            let Some(prop) = pg.property(&property)? else {
                bail!("property `{property}` not found in `{}`", pg.fmri());
            };
            print_property_values(&prop)?;
        } else {
            print_properties(&pg)?;
        }
    } else {
        for pg in target.property_groups_direct()? {
            let pg = pg?;
            println!("-- {} --", pg.fmri());
            print_properties(&pg)?;
        }
    }
    Ok(())
}

fn run_composed(
    target: &impl HasComposedPropertyGroups,
    name: &str,
    property_group: Option<String>,
    property: Option<String>,
) -> anyhow::Result<()> {
    if let Some(property_group) = property_group {
        let Some(pg) = target.property_group_composed(&property_group)? else {
            bail!("property group `{property_group}` not found in `{name}`");
        };

        if let Some(property) = property {
            let Some(prop) = pg.property(&property)? else {
                bail!("property `{property}` not found in `{}`", pg.fmri());
            };
            print_property_values(&prop)?;
        } else {
            print_properties(&pg)?;
        }
    } else {
        for pg in target.property_groups_composed()? {
            let pg = pg?;
            println!("-- {} --", pg.fmri());
            print_properties(&pg)?;
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let Args {
        service,
        instance,
        composed,
        snapshot,
        property_group,
        property,
    } = Args::parse();

    let scf = Scf::connect_global_zone()?;
    let scope = scf.scope_local()?;

    let Some(service) = scope.service(&service)? else {
        bail!("service `{}` not found", service);
    };

    if let Some(inst_name) = &instance {
        let Some(inst) = service.instance(inst_name)? else {
            bail!("instance `{inst_name}` not found in `{}`", service.fmri());
        };
        if let Some(snap_name) = &snapshot {
            let Some(snap) = inst.snapshot(snap_name)? else {
                bail!(
                    "snapshot `{snap_name}` not found in instance {}",
                    inst.fmri(),
                );
            };
            let name = format!("{} ({} snapshot)", inst.fmri(), snap.name());
            run_composed(&snap, &name, property_group, property)?;
        } else if composed {
            let name = format!("{} (composed)", inst.fmri());
            run_composed(&inst, &name, property_group, property)?;
        } else {
            run_direct(&inst, inst.fmri(), property_group, property)?;
        }
    } else {
        run_direct(&service, service.fmri(), property_group, property)?;
    }

    Ok(())
}
