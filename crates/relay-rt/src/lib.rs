//! Bounded SPSC transport for interleaved `f32` audio samples.
//!
//! Create a queue with [`audio_ring()`] on a control thread, move the
//! [`AudioProducer`] to its one producer thread and the [`AudioConsumer`] to
//! its one consumer thread, and retain [`AudioRingMetrics`] off the audio
//! callback for diagnostics. Sample order is preserved; the queue deliberately
//! does not interpret channel count, so capacities and operation counts are in
//! scalar interleaved samples rather than frames.
//!
//! # Realtime contract
//!
//! [`AudioProducer::write()`] and [`AudioConsumer::read()`] are bounded by the
//! provided slice length. They allocate no memory, acquire no locks, perform no
//! waits, retries, logging, formatting, I/O, networking, or DSP. Construction
//! allocates the fixed ring storage and shared counters. The payload is plain
//! `f32`, so consuming or discarding an item cannot run a heap-owning
//! destructor.
//!
//! A write is all-or-drop-new: if the whole input slice does not fit, none of
//! it is published and all input samples are counted as dropped. A read may be
//! partial and never initializes the unused output remainder; its caller can
//! apply its own silence/concealment policy.
//!
//! Endpoint destruction is **not** part of the callback-safe API contract.
//! Dropping the last endpoint can deallocate the preallocated ring, and dropping
//! the last metrics handle can deallocate its counters. Stop/detach the device
//! callback first, obtain the embedding audio API's stop acknowledgement, then
//! destroy endpoints and metrics on a control or worker thread. A
//! `Disconnected` observation is diagnostic, not a reclamation barrier.

#![forbid(unsafe_code)]

mod counters;
mod ring;

pub use counters::{AudioRingMetrics, AudioRingSnapshot};
pub use ring::{
    AudioConsumer, AudioProducer, ReadOutcome, ReadState, RingConfigError, WriteOutcome, audio_ring,
};
