// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::bail;
use clap::Parser;
use scuffle::Scf;
use scuffle::Zone;

#[derive(Parser)]
#[command(about = "Print the values of an SMF service property")]
struct Args {
    service: String,
    property_group: String,
    property: String,
}

fn main() -> anyhow::Result<()> {
    let Args { service, property_group, property } = Args::parse();

    let scf = Scf::connect(Zone::Global)?;
    let scope = scf.scope_local()?;

    let Some(service) = scope.service(&service)? else {
        bail!("service `{}` not found", service);
    };

    let Some(pg) = service.property_group(&property_group)? else {
        bail!(
            "property group `{property_group}` not found in service `{}`",
            service.name(),
        );
    };

    let Some(prop) = pg.property(&property)? else {
        bail!(
            "property `{property}` not found in `{}/:properties/{}`",
            service.name(),
            pg.name(),
        );
    };

    for value in prop.values()? {
        let value = value?;
        println!("{}", value.display_smf());
    }

    Ok(())
}
