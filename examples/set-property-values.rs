// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::Context;
use anyhow::bail;
use clap::Parser;
use scuffle::AddPropertyGroup;
use scuffle::AddPropertyGroupFlags;
use scuffle::Scf;
use scuffle::TransactionCommitResult;
use scuffle::Value;
use scuffle::Zone;

#[derive(Parser)]
#[command(about = "Create property groups and set SMF service property values")]
struct Args {
    service: String,
    #[arg(long)]
    instance: Option<String>,
    property_group: String,
    /// Property group type (default: "application")
    #[arg(long, default_value = "application")]
    pg_type: String,
    property: String,
    /// Value in the form type:value (e.g., "astring:hello", "count:42",
    /// "boolean:true", "integer:-1")
    value: String,
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
    target: &mut impl AddPropertyGroup,
    name: &str,
    pg_name: &str,
    pg_type: &str,
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

    let tx = pg.transaction()?;
    let mut tx = tx.start()?;
    tx.property_ensure(property, value.as_value_ref())?;
    match tx.commit()? {
        TransactionCommitResult::Success(_) => {
            println!(
                "set {name}/:properties/{pg_name}/{property} = {}",
                value.display_smf(),
            );
        }
        TransactionCommitResult::OutOfDate(_) => {
            bail!(
                "transaction on {name}/:properties/{pg_name} was out of \
                 date; retry",
            );
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let value = parse_value(&args.value)?;

    let scf = Scf::connect(Zone::Global)?;
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
            &args.pg_type,
            &args.property,
            &value,
        )?;
    } else {
        let name = service.fmri().to_string();
        run(
            &mut service,
            &name,
            &args.property_group,
            &args.pg_type,
            &args.property,
            &value,
        )?;
    }

    Ok(())
}
