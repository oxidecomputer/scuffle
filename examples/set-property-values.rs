// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::Context;
use anyhow::anyhow;
use anyhow::bail;
use clap::Parser;
use scuffle::AddPropertyGroupFlags;
use scuffle::EditPropertyGroups;
use scuffle::PropertyGroupType;
use scuffle::Scf;
use scuffle::TransactionCommitResult;
use scuffle::Value;

#[derive(Parser)]
#[command(about = "Create property groups and set SMF service property values")]
struct Args {
    #[arg(long, short)]
    zone: Option<String>,

    /// Instance; if omitted, changes the service level
    #[arg(long)]
    instance: Option<String>,

    #[arg(long, requires = "instance")]
    refresh: bool,

    /// Property group type
    #[arg(
        long,
        default_value_t = PropertyGroupType::Application,
        value_parser = parse_pg_type,
    )]
    pg_type: PropertyGroupType,

    service: String,
    property_group: String,
    property: String,

    /// Value in the form type:value (e.g., "astring:hello", "count:42",
    /// "boolean:true", "integer:-1")
    value: String,
}

fn parse_pg_type(s: &str) -> anyhow::Result<PropertyGroupType> {
    PropertyGroupType::new(s)
        .ok_or_else(|| anyhow!("invalid property group type: {s}"))
}

fn parse_value(s: &str) -> anyhow::Result<Value> {
    let (ty, val) =
        s.split_once(':').context("value must be in the form type:value")?;
    match ty {
        "boolean" => Ok(Value::Bool(val.parse().context("invalid boolean")?)),
        "count" => Ok(Value::Count(val.parse().context("invalid count")?)),
        "integer" => {
            Ok(Value::Integer(val.parse().context("invalid integer")?))
        }
        "astring" => Ok(Value::AString(val.to_string())),
        "ustring" => Ok(Value::UString(val.to_string())),
        _ => bail!(
            "unsupported value type `{ty}`; \
             expected one of: boolean, count, integer, astring, ustring"
        ),
    }
}

fn run(
    target: &mut impl EditPropertyGroups,
    name: &str,
    pg_name: &str,
    pg_type: PropertyGroupType,
    property: &str,
    value: &Value,
) -> anyhow::Result<()> {
    let mut pg = target
        .ensure_property_group(
            pg_name,
            pg_type,
            AddPropertyGroupFlags::Persistent,
        )
        .with_context(|| {
            format!("ensuring property group `{pg_name}` on `{name}`")
        })?;

    let pg_fmri = pg.fmri().to_string();
    let tx = pg.transaction()?;
    let mut tx = tx.start()?;
    tx.property_ensure(property, value.as_value_ref())?;
    match tx.commit()? {
        TransactionCommitResult::Success(_) => {
            println!("set {pg_fmri}/{property} = {}", value.display_smf());
        }
        TransactionCommitResult::OutOfDate(_) => {
            bail!("transaction on {pg_fmri} was out of date; retry");
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let value = parse_value(&args.value)?;

    let scf = match args.zone.as_deref() {
        Some(z) => Scf::connect_zone(z),
        None => Scf::connect_global_zone(),
    }?;
    let scope = scf.scope_local()?;

    let Some(mut service) = scope.service(&args.service)? else {
        bail!("service `{}` not found", args.service);
    };

    if let Some(inst_name) = &args.instance {
        let Some(mut inst) = service.instance(inst_name)? else {
            bail!(
                "instance `{inst_name}` not found in service `{}`",
                service.fmri(),
            );
        };
        let name = inst.fmri().to_string();
        run(
            &mut inst,
            &name,
            &args.property_group,
            args.pg_type,
            &args.property,
            &value,
        )?;

        if args.refresh {
            inst.refresh()?;
        }
    } else {
        let name = service.fmri().to_string();
        run(
            &mut service,
            &name,
            &args.property_group,
            args.pg_type,
            &args.property,
            &value,
        )?;
    }

    Ok(())
}
