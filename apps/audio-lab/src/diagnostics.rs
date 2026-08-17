// SPDX-License-Identifier: MPL-2.0

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub input_frames: u64,
    pub rendered_frames: u64,
    pub encoded_packets: u64,
    pub emitted_frames: u64,
    pub drained_lookahead_frames: u64,
    pub ingress_accepted_packets: u64,
    pub network_drops: u64,
    pub network_duplicate_requests: u64,
    pub network_duplicate_copies_scheduled: u64,
    pub rx_duplicate_rejections: u64,
    pub fec_or_plc_frames: u64,
    pub plc_frames: u64,
    pub ring_dropped_frames: u64,
    pub ring_underrun_frames: u64,
    pub ring_high_water_frames: u64,
    pub playback_error_frames: i64,
    pub configured_capture_rate_hz: u64,
    pub configured_playback_rate_hz: u64,
    pub rendered_checksum: u64,
    pub published_chunks: u64,
}

impl Diagnostics {
    pub fn human(&self) -> String {
        format!(
            "audio-lab diagnostics\ninput frames: {}\nrendered frames: {}\nencoded packets: {}\nemitted frames: {}\ndrained lookahead frames: {}\ningress accepted packets: {}\nnetwork drops: {}\nnetwork duplicate requests: {}\nnetwork duplicate copies scheduled: {}\nRX duplicate rejections: {}\nFEC-or-PLC frames: {}\nPLC frames: {}\nring dropped frames: {}\nring underrun frames: {}\nring high-water frames: {}\nplayback error frames: {}\nconfigured nominal rates: capture={} playback={}\nrendered checksum: {}\npublished chunks: {}",
            self.input_frames,
            self.rendered_frames,
            self.encoded_packets,
            self.emitted_frames,
            self.drained_lookahead_frames,
            self.ingress_accepted_packets,
            self.network_drops,
            self.network_duplicate_requests,
            self.network_duplicate_copies_scheduled,
            self.rx_duplicate_rejections,
            self.fec_or_plc_frames,
            self.plc_frames,
            self.ring_dropped_frames,
            self.ring_underrun_frames,
            self.ring_high_water_frames,
            self.playback_error_frames,
            self.configured_capture_rate_hz,
            self.configured_playback_rate_hz,
            self.rendered_checksum,
            self.published_chunks,
        )
    }

    pub fn json(&self) -> String {
        format!(
            "{{\"input_frames\":{},\"rendered_frames\":{},\"encoded_packets\":{},\"emitted_frames\":{},\"drained_lookahead_frames\":{},\"ingress_accepted_packets\":{},\"network_drops\":{},\"network_duplicate_requests\":{},\"network_duplicate_copies_scheduled\":{},\"rx_duplicate_rejections\":{},\"fec_or_plc_frames\":{},\"plc_frames\":{},\"ring_dropped_frames\":{},\"ring_underrun_frames\":{},\"ring_high_water_frames\":{},\"playback_error_frames\":{},\"configured_capture_rate_hz\":{},\"configured_playback_rate_hz\":{},\"rendered_checksum\":{},\"published_chunks\":{}}}",
            self.input_frames,
            self.rendered_frames,
            self.encoded_packets,
            self.emitted_frames,
            self.drained_lookahead_frames,
            self.ingress_accepted_packets,
            self.network_drops,
            self.network_duplicate_requests,
            self.network_duplicate_copies_scheduled,
            self.rx_duplicate_rejections,
            self.fec_or_plc_frames,
            self.plc_frames,
            self.ring_dropped_frames,
            self.ring_underrun_frames,
            self.ring_high_water_frames,
            self.playback_error_frames,
            self.configured_capture_rate_hz,
            self.configured_playback_rate_hz,
            self.rendered_checksum,
            self.published_chunks,
        )
    }
}
