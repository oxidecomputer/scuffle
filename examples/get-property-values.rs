// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::bail;
use clap::Parser;
use scuffle::Property;
use scuffle::PropertyGroup;
use scuffle::Scf;
use scuffle::Zone;

#[derive(Parser)]
#[command(about = "Print the values of an SMF service property")]
struct Args {
    service: String,
    property_group: Option<String>,
    #[arg(requires = "property_group")]
    property: Option<String>,
}

fn print_property_values<St>(prop: &Property<'_, St>) -> anyhow::Result<()> {
    for value in prop.values()? {
        let value = value?;
        println!("{} {}", prop.name(), value.display_smf());
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

fn main() -> anyhow::Result<()> {
    let Args { service, property_group, property } = Args::parse();

    let scf = Scf::connect(Zone::Global)?;
    let scope = scf.scope_local()?;

    let Some(service) = scope.service(&service)? else {
        bail!("service `{}` not found", service);
    };

    if let Some(property_group) = property_group {
        let Some(pg) = service.property_group(&property_group)? else {
            bail!(
                "property group `{property_group}` not found in service `{}`",
                service.name(),
            );
        };

        if let Some(property) = property {
            let Some(prop) = pg.property(&property)? else {
                bail!(
                    "property `{property}` not found in `{}/:properties/{}`",
                    service.name(),
                    pg.name(),
                );
            };
            print_property_values(&prop)?;
        } else {
            print_properties(&pg)?;
        }
    } else {
        for pg in service.property_groups()? {
            let pg = pg?;
            println!("-- property group {} --", pg.name());
            print_properties(&pg)?;
        }
    }

    Ok(())
}
