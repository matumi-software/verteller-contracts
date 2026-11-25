# verteller-contracts

The shared contracts of [Verteller](https://github.com/matumi-software/verteller): the types, traits and pure code that
the open app and its Pro features both build on.

The open app depends on this crate. So do the Pro features, which are built from a private source. The Pro source never
depends on the app, and the app never depends on the Pro source. What they both need to agree on lives here.

It holds no state and no I/O of its own beyond reading and writing audio files. The job queue, the event bus, accounts,
the speech models and the server stay in the app and reach the Pro features through the `Host` trait.

MIT licensed. Not published to crates.io yet: depend on it by git tag.
