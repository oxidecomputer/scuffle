#!/bin/bash
#:
#: name = "build-and-test"
#: variety = "basic"
#: target = "helios-3.0-32c256gb"
#: rust_toolchain = "stable"
#: output_rules = [
#: ]
#:

set -o errexit
set -o pipefail
set -o xtrace

cargo --version
rustc --version

pfexec mkdir -p /out
pfexec chown "$LOGNAME" /out

export RUSTFLAGS="--cfg tokio_unstable -D warnings"
export RUSTDOCFLAGS="--document-private-items -D warnings"

banner test-no-features
ptime -m cargo test --locked --verbose

banner test-individual-features
ptime -m cargo test --features daft            --locked --verbose
ptime -m cargo test --features smf-by-instance --locked --verbose
ptime -m cargo test --features testing         --locked --verbose

# we could test partial combinations of features, but that seems excessive

banner test-all-features
ptime -m cargo test --doc --all-features --locked --verbose
ptime -m cargo test --all-targets --all-features --locked --verbose
