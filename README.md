# quickie
[![crates.io](https://img.shields.io/crates/v/quickie)](https://crates.io/crates/quickie)
[![docs.rs](https://docs.rs/quickie/badge.svg)](https://docs.rs/quickie)
[![LOC](https://tokei.rs/b1/github/ljedrz/quickie?category=code)](https://github.com/ljedrz/quickie/tree/master/src)
[![dependencies](https://deps.rs/repo/github/ljedrz/quickie/status.svg)](https://deps.rs/repo/github/ljedrz/quickie)
[![actively developed](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg)](https://gist.github.com/cheerfulstoic/d107229326a01ff0f333a1d3476e068d)
[![issues](https://img.shields.io/github/issues-raw/ljedrz/quickie)](https://github.com/ljedrz/quickie/issues)

**quickie** is a simple, low-level, and customizable implementation of a QUIC P2P node. Its design is inspired by [pea2pea](https://github.com/ljedrz/pea2pea).

## goals
- small, simple, non-framework codebase
- ease of use: few objects and traits, no "turboeels" or generics/references that would force all parent objects to adapt
- correctness: builds with stable Rust, there is no unsafe code
- low-level oriented: while the underlying `quinn` crate does the QUIC heavy-lifting, the user should have access to most of its functionalities

## how to use it
1. define a clonable struct containing a [Node](https://docs.rs/quickie/latest/quickie/struct.Node.html) and any extra state you'd like to carry
2. implement the [`Quickie`](https://docs.rs/quickie/latest/quickie/trait.Quickie.html) trait for it
3. create that struct (or as many of them as you like)

That's it!

## examples
- simple interop with libp2p-quic

## status
- the core functionalities seem to work, but there can still be bugs
- not all the `quinn` features are exposed yet
- some tests are already in place
- the crate follows [semver](https://semver.org/), and API breakage is to be expected before `1.0`
