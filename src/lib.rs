// tracker is the underlying bipartite-buffer offset tracking.
// It has no data and no I/O.
pub mod tracker;

// buffer is the data buffer itself. It relies on the tracker
// for safety.
// It has data but no I/O.
pub mod buffer;

// sink has logic to spawn a dedicated thread to continuously and eagerly
// drain a buffer into an underlying provided std::io::Write sink.
// It has both data and I/O.
pub mod sink;
