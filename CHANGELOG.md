# 0.3.0

### Fixed

- the inbound stream/datagram handlers now close when there's a connection error

# 0.2.0

### Added

- `Quickie::send_datagram`: the counterpart to [quinn::Connection::send_datagram](https://docs.rs/quinn/0.8.3/quinn/struct.Connection.html#method.send_datagram)
- `Quickie::rebind`: the counterpart to [quinn::Endpoint::rebind](https://docs.rs/quinn/0.8.3/quinn/struct.Endpoint.html#method.rebind)
- `Quickie::local_addr`
- `node::Config`: allows nodes to be started in client or server mode
- `Node::new`: creates a new `Node` with the given `Config`

### Changed

- `Quickie::listen` became `::start`, and it only has a `SocketAddr` param now

### Fixed

- `quinn`'s `StreamId`s are now correctly distinguished (version `0.1` wrongly assumed their numeric indices are unique per connection)

### Removed

- `impl Default for Node`: no longer possible due to it containing the `Config` now
- `StreamIdx`: not needed anymore

# 0.1.0

### Added

- the initial implementation
