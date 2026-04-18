// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use clap::Parser;
use clap::Subcommand;
use scuffle::Scf;
use scuffle::SmfDegradeFlags;
use scuffle::SmfEnableDisableFlags;
use scuffle::SmfMaintainFlags;

#[derive(Parser)]
#[command(about = "Control the SMF state of an instance")]
struct Args {
    #[arg(long, short)]
    zone: Option<String>,

    /// Full instance FMRI (e.g., svc:/system/cron:default)
    fmri: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enable the instance.
    Enable(EnableDisableArgs),
    /// Disable the instance.
    Disable(EnableDisableArgs),
    /// Refresh the instance.
    Refresh,
    /// Restart the instance.
    Restart,
    /// Put the instance into the maintenance state.
    Maintain(MaintainArgs),
    /// Put the instance into the degraded state.
    Degrade(DegradeArgs),
    /// Clear maintenance or degraded state on the instance.
    Restore,
    /// Print the current SMF state of the instance.
    State,
}

#[derive(clap::Args)]
struct EnableDisableArgs {
    /// Take effect only for this boot session.
    #[arg(short = 't', long, conflicts_with = "at_next_boot")]
    temporary: bool,

    /// Take effect at next boot only.
    #[arg(short = 'n', long)]
    at_next_boot: bool,

    /// Audit-log comment.
    #[arg(short = 'c', long)]
    comment: Option<String>,
}

impl EnableDisableArgs {
    fn to_flags(&self) -> Option<SmfEnableDisableFlags> {
        if self.temporary {
            Some(SmfEnableDisableFlags::Temporary)
        } else if self.at_next_boot {
            Some(SmfEnableDisableFlags::AtNextBoot)
        } else {
            None
        }
    }
}

#[derive(clap::Args)]
struct MaintainArgs {
    /// Do not wait for the service's stop method to complete.
    #[arg(short = 'i', long)]
    immediate: bool,

    /// Do not persist the state across reboot.
    #[arg(short = 't', long)]
    temporary: bool,
}

impl MaintainArgs {
    fn to_flags(&self) -> Option<SmfMaintainFlags> {
        let mut flags = SmfMaintainFlags::empty();
        if self.immediate {
            flags |= SmfMaintainFlags::Immediate;
        }
        if self.temporary {
            flags |= SmfMaintainFlags::Temporary;
        }
        (!flags.is_empty()).then_some(flags)
    }
}

#[derive(clap::Args)]
struct DegradeArgs {
    /// Do not wait for the service's stop method to complete.
    #[arg(short = 'i', long)]
    immediate: bool,
}

impl DegradeArgs {
    fn to_flags(&self) -> Option<SmfDegradeFlags> {
        self.immediate.then_some(SmfDegradeFlags::Immediate)
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let scf = match args.zone.as_deref() {
        Some(z) => Scf::connect_zone(z),
        None => Scf::connect_global_zone(),
    }?;

    let mut instance = scf.instance_from_fmri(&args.fmri)?;
    let fmri = instance.fmri().to_string();

    match args.command {
        Command::Enable(opts) => {
            instance.smf_enable(opts.to_flags(), opts.comment.as_deref())?;
            println!("enabled {fmri}");
        }
        Command::Disable(opts) => {
            instance.smf_disable(opts.to_flags(), opts.comment.as_deref())?;
            println!("disabled {fmri}");
        }
        Command::Refresh => {
            instance.smf_refresh()?;
            println!("refreshed {fmri}");
        }
        Command::Restart => {
            instance.smf_restart()?;
            println!("restarted {fmri}");
        }
        Command::Maintain(opts) => {
            instance.smf_maintain(opts.to_flags())?;
            println!("marked {fmri} as maintenance");
        }
        Command::Degrade(opts) => {
            instance.smf_degrade(opts.to_flags())?;
            println!("marked {fmri} as degraded");
        }
        Command::Restore => {
            instance.smf_restore()?;
            println!("restored {fmri}");
        }
        Command::State => {
            println!("{}", instance.smf_state()?);
        }
    }

    Ok(())
}
