// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use clap::Parser;
use scuffle::isolated::IsolatedConfigd;

#[derive(Parser)]
#[command(about = "Run an isolated svc.configd with fake services")]
struct Args {
    /// Service names to register (e.g. "site/myapp")
    #[arg(required = true)]
    services: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut services = args.services.into_iter();
    let first = services.next().expect("clap enforces at least one");
    let mut builder = IsolatedConfigd::builder(first)?;
    for service in services {
        builder = builder.add_service(service)?;
    }

    let configd = builder.build()?;
    println!(
        "isolated svc.configd running out of {} (door: {})",
        configd.path(),
        configd.door_path(),
    );

    // Sleep forever; the isolated svc.configd will be shut down when this
    // process is killed.
    std::thread::park();

    // Unreachable, but if we do get here, shut down cleanly.
    configd.shutdown()?;
    Ok(())
}
