# 0.4.0

### Added

- `quinn::StreamId` is re-exported

### Changed

- `Quickie::disconnect` is now `async`
- updated `quinn` and `quinn-proto` to `0.9`

# 0.3.0

### Added

- `Quickie::num_connections`: returns the number of live connections
- `Quickie::close_stream`: closes the specified stream
- `Quickie::get_stream_ids`: returns the list of stream IDs associated with a connection
- `Quickie::get_stream_stats`: returns the list of active streams associated with a connection
- `StreamStats`: a set of simple statistics related to a stream

### Changed

- `Quickie::open_bi` now only returns a single stream ID
- `Quickie::disconnect` is no longer `async` (it wasn't used)
- renamed `Quickie::unicast` to `::send_msg` (to be more aligned with `::send_datagram`)

### Fixed

- the inbound stream/datagram handlers now close when there's a connection error
- the bidirectional stream's send task's handle is no longer accidentally overwritten

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
