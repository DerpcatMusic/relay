//! Bounded, deterministic composition primitives for the RELAY audio path.
//!
//! This crate owns validation and caller-driven orchestration, while focused
//! crates own codec and resampling algorithms. It provides typed RTP-like
//! timelines, fixed-inline packets, a deterministic TX worker, and a fake
//! network. Construction may allocate fixed storage; steady-state operations do
//! not grow it. There are no hidden threads, async tasks, sockets, wall clocks,
//! waits, or device callbacks.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod accumulator;
mod config;
mod network;
mod packet;
mod playback;
mod rx;
mod timeline;
mod tx;

pub use accumulator::{AccumulatorError, Interleaved48kAccumulator};
pub use config::{AudioPipelineConfig, AudioPipelineConfigInput, ConfigError};
pub use network::{
    AdvanceError, AdvanceReport, DeterministicNetwork, DrainReport, DueBatch, DueBatchError,
    NetworkAction, NetworkConfigError, NetworkMetrics, NetworkTime, ScheduleOutcome,
    ScheduleRejection, ScheduleStatus,
};
pub use packet::{MAX_PACKET_BYTES, MediaPacket, PacketError, PayloadType, PayloadTypeError};
pub use playback::{
    FinitePlaybackEnd, FinitePlaybackInput, PlaybackBuildError, PlaybackConfig,
    PlaybackControlFault, PlaybackFinishError, PlaybackFinishErrorCause, PlaybackFinishReport,
    PlaybackFinishStatus, PlaybackMetrics, PlaybackProcessError, PlaybackProcessReport,
    PlaybackPublication, PlaybackRenderer, PlaybackResetError, PlaybackWorker, PlaybackWorkerState,
    RenderReport, RenderState, playback_pair,
};
pub use relay_clock::ClockRecoveryConfig;
pub use relay_opus::{Bitrate, EncoderPolicyV1, FrameDuration, InbandFec, PacketLossPercent};
pub use relay_resample::{AdaptiveClockConfig, FrameRequirements};
pub use rx::{
    FrameOutcome, FrameSource, FrameStatus, IngressMismatch, IngressOutcome, IngressStatus,
    PcmFrame, RxBuildError, RxMetrics, RxStreamConfig, RxWorker,
};
pub use timeline::{
    ExtendedSequence, ExtendedTimestamp, ExtensionError, RtpTimestamp, SequenceNumber, Ssrc,
    extend_sequence, extend_timestamp,
};

pub use tx::{
    CaptureInput, FinalFramePolicy, FiniteTxError, FiniteTxReport, FiniteTxWorker,
    LiveDisconnectReport, PacketBatch, PacketBatchError, TxBuildError, TxError, TxProcessFailure,
    TxProcessOutcome, TxProcessReport, TxStreamConfig, TxTimeline, TxWorker,
};
