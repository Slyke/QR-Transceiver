#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p offline
cargo vendor --locked offline/vendor > offline/cargo-config.toml
npm --prefix desktop ci --cache "$PWD/offline/npm-cache"
printf '%s\n' 'Prepared crates and npm cache. Copy the repository and offline directory plus target-OS packages.' 'On the offline build machine use the source replacement in offline/cargo-config.toml and npm --prefix desktop ci --offline --cache "$PWD/offline/npm-cache".'
