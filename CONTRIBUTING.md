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
`cargo run -- <args>
```

Paru has a couple of feature flags which you may want to enable:

- backtrace: generate backtraces when errors occur (requires nightly)
- git: target the libalpm-git API
- generate: generate the libalpm buildings at build time (requires clang)

### Building Against a Custom libalpm

If you wish to build against a custom libalpm you can specify **ALPM_LIB_DIR** while using the generate
feature. Then running with **LD_LIBRARY_PATH** pointed at the custom libalpm.so.

## Testing

Paur's test suite can be run by running:

```
cargo test --features mock -- --test-threads=1
```

## Translating

### New Languages

To tranlate paru to a new language, copy the the template .pot file to the locale you
are translating to.

For example, to translate paru to Japanese you would do:

```
cp po/paru.pot po/jp.po
```

Then fill out the template file with your information and translation.

Alternatively, you can use programs like `poedit` to write the translations.

### Updating existing translations

To update existing translations against new code you must first update the
template file then upate the .po files.

Updating the template requires [xtr](https://github.com/woboq/tr) to be installed.

```
./scripts/mkpot
./scripts/updpo
```

Then fill in new strings.

### Testing Translations

To test the translations you first must build the translation then run paru
pointing it at the generated files.

```
./scripts/mkmo locale/
LOCALE_DIR=locale/ cargo run -- <args>
```
