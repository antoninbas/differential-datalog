#!/bin/bash
# This script is configured to be run by git pre-push

set -e

(cd rust/template/ &&
cargo fmt --all -- --check &&
cargo clippy --all -- -D warnings)
(cd lib && rustfmt *.rs --check)
