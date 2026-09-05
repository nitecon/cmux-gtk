# Configuration and packaging metadata

Keep YAML, TOML and JSON declarative. Use one version source and generate package metadata from it. Document non-obvious configuration semantics beside their owning implementation. Validate untrusted values before interpolation into shell commands. GitHub Actions permissions should match job requirements; preserve pinned actions and dependency version constraints. [Workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax).

Homebrew Ruby is a packaging DSL, not a second application implementation. Keep Cask changes limited to package identity, version, checksum, URL and installation metadata. Follow [Homebrew Cask documentation](https://docs.brew.sh/Cask-Cookbook). Keep DEB, RPM, Cask and archives aligned; changes to release machinery need executable package checks in CI.
