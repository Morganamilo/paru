# Contributing to paru

## Formatting

Please format the code using `cargo fmt`

## Building

Paru is built with cargo.

To build paru use:

```
cargo build
```

To run paru use:

```
cargo run -- <args>
```

Paru has a couple of feature flags which you may want to enable:

- backtrace: does nothing, kept around for backwards compatibility
- git: target the libalpm-git API
- generate: generate the libalpm bindings at build time (requires clang)

### Building Against a Custom libalpm

If you wish to build against a custom libalpm you can specify **ALPM_LIB_DIR** while using the generate
feature. Then running with **LD_LIBRARY_PATH** pointed at the custom libalpm.so.

## Testing

Paru's test suite can be run by running:

```
cargo test --features mock
```

### Chroot dependency isolation

Paru uses `aur-depends`' `Actions::resolved_dependencies` API. When testing paired
changes with a sibling `aur-depends` checkout, pass Cargo's local dependency
override to the build and test commands:

```sh
cargo --config 'patch.crates-io.aur-depends.path="../aur-depends"' test --features mock
cargo --config 'patch.crates-io.aur-depends.path="../aur-depends"' build --release
```

This override changes the lockfile's dependency source. Before submitting paired
changes upstream, update the dependency version and lockfile to a release
containing this API.

Run the real chroot regression test with the release binary built above:

```sh
bash tests/chroot.sh
```

This requires Arch Linux, devtools, ripgrep, and sudo. The test creates a temporary
chroot and builds independent packages, a versioned provider, a matching explicit
target, and a consumer with overridden runtime dependencies. It checks the source
preparation and build commands with check dependencies enabled and disabled.
An injected provider requires repository Git; the test verifies Git is absent
from the clean root but installed in the consumer's chroot.
It also deliberately fails a provider build, verifies that consumers can use
repository Git, and verifies that unavailable or insufficient dependency versions
still fail inside the chroot. Successful consumer archives and `.BUILDINFO` prove
that the run continues while reporting the failed targets. It installs no packages
on the host and removes the temporary chroot on exit.

Build requirements use the checkout's post-download `.SRCINFO`; archive runtime
requirements use actual package metadata. Resolver selections are per-consumer
snapshots, not proof of satisfaction in the chroot. A changed declaration may
retain a unique Build provider hint, checked against the available archive, but
cannot inherit an older external-satisfaction decision. A Build hint shared with
installed or repository choices for the same package retains that identity.
Conflicting hints defer
to native resolution rather than becoming unplanned requirements.

Resolved provider choices guide archive injection. They do not require every
planned rebuild to succeed. Only available archives satisfying the declared
requirements are injected. Ambiguous provider preferences do not force an
archive choice. Archive selection returns a list, not a dependency satisfaction
error. Pacman and makepkg in the chroot resolve requirements
without a usable archive, including their version constraints. Host installations
do not establish that those requirements are satisfied inside the chroot.

## Translating

See https://github.com/Morganamilo/paru/discussions/433 for discussion on localization.
You probably want to subscribe to this to be notified when translations need to be updated.

### New Languages

When translating to a new language try to stick to languages pacman already supports:
https://gitlab.archlinux.org/pacman/pacman/-/tree/master/src/pacman/po. For example using
`es` over `es_ES`.

To translate paru to a new language, copy the the template .pot file to the locale you
are translating to.

For example, to translate paru to Japanese you would do:

```
cp po/paru.pot po/jp.po
```

Then fill out the template file with your information and translation.

Alternatively, you can use programs like `poedit` to write the translations.

### Updating existing translations

To update existing translations against new code you must first update the .po
files.

Do this as its own commit.

```
./scripts/updpo
git commit po
```

Then fill in new strings.

### Testing Translations

To test the translations you first must build the translation then run paru
pointing it at the generated files.

```
./scripts/mkmo locale/
LOCALE_DIR=locale/ cargo run -- <args>
```
