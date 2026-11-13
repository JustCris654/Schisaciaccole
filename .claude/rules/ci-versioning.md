---
paths:
  - ".github/workflows/**"
  - "Dockerfile.deb"
  - "deploy/Dockerfile"
  - "packaging/**"
  - "crates/app/Cargo.toml"
  - "Cargo.toml"
---

# CI, release and packaging

Read `.github/workflows/ci.yml`, `release.yml` and the matching README sections. CI checks formatting, build, tests, clippy and SQLx offline metadata with locked dependencies and PostgreSQL 17.

A `vX.Y.Z` tag must match root `[workspace.package].version`. Release builds publish desktop `.deb`, Arch and macOS `.app` packages (zip/dmg) and a server GHCR image tagged with version and `latest`. Manual workflow dispatch builds artifacts without cutting a release. Do not push tags, dispatch publication jobs or publish releases/images without explicit requested scope.

`SCHISACIACCOLE_BACKEND_URL` is baked into desktop builds. macOS signing requires the complete seven-secret set documented in README; never print secret values. Preserve least-privilege job permissions, signing conditions and artifact paths.

Run cargo-bundle from `crates/app`, makepkg from `packaging/arch`. Keep sound/icon/font resources and package metadata aligned. Validate changed workflows with available static tooling and shell scripts with `bash -n`; report unavailable tools. Root instructions define required Rust checks.
