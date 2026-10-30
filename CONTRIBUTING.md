# Contributing

## Set up a development shell

Install Rust and Cargo with [rustup](https://rustup.rs/) if they are not
already available. From the repository root, build the picker:

```sh
cargo build --manifest-path picker/Cargo.toml
```

In Bash, source the integration:

```sh
source "$PWD/script/xws.bash"
```

In Zsh, use:

```zsh
source "$PWD/script/xws.zsh"
```

## Build and test

```sh
cargo fmt --manifest-path picker/Cargo.toml -- --check
cargo test --manifest-path picker/Cargo.toml --locked
cargo build --manifest-path picker/Cargo.toml --locked
bash -n script/xws-common.sh script/xws.bash xws-install
zsh -n script/xws-common.sh script/xws.zsh
```

To install a local debug build for manual testing, run `./xws-install install`.
After building with `--release`, use `./xws-install install --release`.
Local install and artifact options are documented by `./xws-install --help`.

The root-level `xws-install` script is a distribution tool. It is not included
in release packages or installed as part of xws.

## Preparing and publishing a release

The package version in `picker/Cargo.toml` is the release version. The
`picker/Cargo.lock` entry for `xws-picker` must match it, and the Git tag must
be `v` followed by that version (for example, package version `0.1.2` uses tag
`v0.1.2`). The release workflow checks the tag/package match and publishes a
GitHub release when a `v*` tag is pushed. There is no separate changelog or
release-version constant in the installer; GitHub generates release notes
from the repository history.

1. Update `version` in `picker/Cargo.toml`. Refresh the lockfile from the
   repository root:

   ```sh
   cargo check --manifest-path picker/Cargo.toml
   ```

   Review the `xws-picker` entry in `picker/Cargo.lock`; avoid unrelated
   dependency updates.

2. Run the release checks locally:

   ```sh
   cargo fmt --all --manifest-path picker/Cargo.toml -- --check
   cargo test --locked --manifest-path picker/Cargo.toml --bin xws-picker
   cargo test --locked --manifest-path picker/Cargo.toml --test cli -- --test-threads=1
   cargo clippy --locked --manifest-path picker/Cargo.toml --all-targets -- -D warnings
   ```

3. Commit and push the version and release changes to `master`. Wait for the
   branch CI workflow to pass before tagging.

4. Create and push an annotated tag matching the package version:

   ```sh
   VERSION=0.1.2
   git tag -a "v$VERSION" -m "xws $VERSION"
   git push origin "v$VERSION"
   ```

   Use the actual version you selected. Check that the tag does not already
   exist before creating it; do not move or reuse a published release tag.

5. Monitor the **Release packaging** workflow. It validates the tag, builds
   the supported macOS and Linux targets, packages the binaries, shell
   integrations, templates, and `LICENSE`, generates `SHA256SUMS.txt`, smoke
   tests the Linux package, and publishes the GitHub release with generated
   notes. No manual upload is needed.

6. Confirm the release contains `xws-aarch64-apple-darwin.tar.gz`,
   `xws-x86_64-apple-darwin.tar.gz`,
   `xws-x86_64-unknown-linux-gnu.tar.gz`, and `SHA256SUMS.txt`. Test the
   published installer in a temporary prefix, substituting the released
   version:

   ```sh
   VERSION=0.1.2
   test_root=$(mktemp -d)
   curl -fsSL "https://raw.githubusercontent.com/pre-cursor/xws/v${VERSION}/xws-install" |
     XWS_INSTALL_PREFIX="$test_root/install" \
     XWS_RELEASE_BASE_URL="https://github.com/pre-cursor/xws/releases/download/v${VERSION}" \
     sh -s -- install --latest
   "$test_root/install/bin/xws-picker" --help
   ```

   Remove the specific temporary directory when finished. The installer used
   by the general installation instructions tracks `master`, and its default
   download URL resolves the latest release.
