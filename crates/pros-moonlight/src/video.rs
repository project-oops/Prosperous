//! Turning an encoded frame into the RTP packets a Moonlight client expects.
//!
//! **Adapted from Moonshine** (Hans Gaiser, BSD-2-Clause; see `THIRD-PARTY-LICENSES.md`), whose
//! `video/packetizer.rs` is the working reference for GameStream's video wire format. The byte
//! layout, the NV video-packet header, the `fec_info` bit-packing and the Reed-Solomon scheme
//! below follow it. Nothing here decodes a picture - it prepends a frame header, splits the bytes
//! into equal shards, adds parity, and wraps each shard in RTP.
//!
//! A packet is laid out exactly as the client reads it:
//!
//! ```text
//! [0..12]   RTP header
//! [12..16]  four zero bytes of padding
//! [16..32]  NV video packet header
//! [32..]    shard payload (frame header on the first shard, then frame bytes; zero-padded)
//! ```

use fec_rs::ReedSolomon;

/// Reed-Solomon works in GF(256), so a block holds at most this many shards, data plus parity.
const MAX_SHARDS: usize = 255;
/// The RTP header length.
const RTP_HEADER: usize = 12;
/// Four zero bytes the client skips between the RTP header and ours.
const PADDING: usize = 4;
/// The NV video packet header length.
const NV_HEADER: usize = 16;
/// Where a shard's payload begins within a packet.
const PAYLOAD_OFFSET: usize = RTP_HEADER + PADDING + NV_HEADER;
/// The per-frame header prepended to the first shard's payload.
const FRAME_HEADER: usize = 8;

/// Video packet flags, in the NV header's byte 8.
mod flag {
    /// The shard carries picture data (a data shard, not parity).
    pub(super) const PIC_DATA: u8 = 0x1;
    /// The shard is the last of the frame's data.
    pub(super) const END_OF_FRAME: u8 = 0x2;
    /// The shard is the first of the frame's data.
    pub(super) const START_OF_FRAME: u8 = 0x4;
}

/// How a frame is cut into packets.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Config {
    /// The payload bytes per shard, from the client's negotiated packet size.
    pub(crate) shard_payload: usize,
    /// The Reed-Solomon overhead, as a percentage of the data shards.
    pub(crate) fec_percentage: u8,
}

impl Default for Config {
    fn default() -> Self {
        // 1024-byte shards and 20% parity are the common GameStream defaults; the client's ANNOUNCE
        // can narrow them, but these stream on a LAN.
        Self {
            shard_payload: 1024,
            fec_percentage: 20,
        }
    }
}

/// Builds the RTP packet stream for successive frames, keeping the running packet counter.
#[derive(Debug, Default)]
pub(crate) struct Packetizer {
    /// The stream-wide packet index, in both the RTP sequence and the NV header.
    stream_index: u32,
}

impl Packetizer {
    /// A fresh packetizer.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Cut one frame into its RTP packets: the data shards, then the FEC parity shards.
    ///
    /// `frame_index` numbers the picture; `keyframe` marks an IDR. The returned packets are ready
    /// to send, in order.
    pub(crate) fn packetize(
        &mut self,
        frame: &[u8],
        keyframe: bool,
        frame_index: u32,
        config: &Config,
    ) -> Vec<Vec<u8>> {
        let shard_payload = config.shard_payload.max(1);

        // The frame header, then the frame, split into equal shards (the last zero-padded).
        let mut data = Vec::with_capacity(FRAME_HEADER + frame.len());
        let last_payload_len =
            u32::try_from((FRAME_HEADER + frame.len()) % shard_payload).unwrap_or(0);
        data.extend_from_slice(&frame_header(keyframe, last_payload_len));
        data.extend_from_slice(frame);

        let nr_data = data.len().div_ceil(shard_payload).max(1);
        let mut shards: Vec<Vec<u8>> = data
            .chunks(shard_payload)
            .map(|chunk| {
                let mut shard = vec![0_u8; shard_payload];
                shard[..chunk.len()].copy_from_slice(chunk);
                shard
            })
            .collect();
        while shards.len() < nr_data {
            shards.push(vec![0_u8; shard_payload]);
        }

        // Parity shards: a percentage of the data, capped so a block never exceeds GF(256).
        let mut nr_parity = parity_count(nr_data, config.fec_percentage);
        for _ in 0..nr_parity {
            shards.push(vec![0_u8; shard_payload]);
        }
        if nr_parity > 0 {
            // fec-rs is the same Reed-Solomon Moonshine and Moonlight use; it fills the parity
            // shards in place from the data shards. If it cannot be built (only for invalid shard
            // counts, which the caps above prevent), send the data shards alone rather than zeros
            // a client would take for parity.
            match ReedSolomon::new(nr_data, nr_parity) {
                Ok(fec) => {
                    if let Err(error) = fec.encode(&mut shards) {
                        tracing::warn!(%error, "FEC encode failed; sending data shards only");
                        shards.truncate(nr_data);
                        nr_parity = 0;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "could not build FEC; sending data shards only");
                    shards.truncate(nr_data);
                    nr_parity = 0;
                }
            }
        }

        let total = nr_data + nr_parity;
        let mut packets = Vec::with_capacity(total);
        for (index, shard) in shards.iter().enumerate() {
            let is_data = index < nr_data;
            let mut flags = 0;
            if is_data {
                flags |= flag::PIC_DATA;
                if index == 0 {
                    flags |= flag::START_OF_FRAME;
                }
                if index == nr_data - 1 {
                    flags |= flag::END_OF_FRAME;
                }
            }
            // See Moonshine: shard index, data-shard count and FEC percentage packed into one word.
            let fec_info = (u32::try_from(index).unwrap_or(0) << 12)
                | (u32::try_from(nr_data).unwrap_or(0) << 22)
                | (u32::from(config.fec_percentage) << 4);
            packets.push(self.wrap(shard, frame_index, flags, fec_info));
        }
        packets
    }

    /// Wrap one shard payload in the RTP, padding and NV headers.
    fn wrap(&mut self, payload: &[u8], frame_index: u32, flags: u8, fec_info: u32) -> Vec<u8> {
        let mut packet = vec![0_u8; PAYLOAD_OFFSET + payload.len()];
        // RTP header: version 2, payload type 0, big-endian sequence and timestamp, zero SSRC.
        packet[0] = 0x90;
        packet[1] = 0;
        packet[2..4].copy_from_slice(
            &u16::try_from(self.stream_index & 0xffff)
                .unwrap_or(0)
                .to_be_bytes(),
        );
        packet[4..8].copy_from_slice(&frame_index.to_be_bytes());
        // packet[8..12] SSRC stays zero.
        // NV video packet header (after the four padding bytes), all little-endian.
        let nv = RTP_HEADER + PADDING;
        packet[nv..nv + 4].copy_from_slice(&self.stream_index.to_le_bytes());
        packet[nv + 4..nv + 8].copy_from_slice(&frame_index.to_le_bytes());
        packet[nv + 8] = flags;
        packet[nv + 10] = 0x10; // multi_fec_flags
        packet[nv + 12..nv + 16].copy_from_slice(&fec_info.to_le_bytes());
        packet[PAYLOAD_OFFSET..].copy_from_slice(payload);
        self.stream_index = self.stream_index.wrapping_add(1);
        packet
    }
}

/// The 8-byte per-frame header on the first shard's payload.
fn frame_header(keyframe: bool, last_payload_len: u32) -> [u8; FRAME_HEADER] {
    let mut header = [0_u8; FRAME_HEADER];
    header[0] = 0x01; // header type
    // bytes 1..3 frame processing latency: zero, we do not measure it.
    header[3] = if keyframe { 2 } else { 1 };
    header[4..8].copy_from_slice(&last_payload_len.to_le_bytes());
    header
}

/// How many parity shards a block of `nr_data` data shards gets at `fec_percentage`.
///
/// At least one when any FEC is asked for, and never so many that the block exceeds [`MAX_SHARDS`].
fn parity_count(nr_data: usize, fec_percentage: u8) -> usize {
    if fec_percentage == 0 {
        return 0;
    }
    let wanted = (nr_data * usize::from(fec_percentage)).div_ceil(100).max(1);
    wanted.min(MAX_SHARDS - nr_data)
}

#[cfg(test)]
mod tests {
    use super::{Config, PAYLOAD_OFFSET, Packetizer, flag, parity_count};

    /// The NV header flags byte of a packet.
    fn flags_of(packet: &[u8]) -> u8 {
        packet[super::RTP_HEADER + super::PADDING + 8]
    }

    #[test]
    fn a_small_frame_is_one_data_shard_plus_parity_marked_start_and_end() {
        let mut packetizer = Packetizer::new();
        let config = Config {
            shard_payload: 1024,
            fec_percentage: 20,
        };
        let packets = packetizer.packetize(b"a tiny keyframe", true, 0, &config);
        // One data shard (the frame is far under 1024) and, at 20%, one parity shard.
        assert_eq!(packets.len(), 2);
        // The single data shard is both the start and the end of the frame, and carries pic data.
        assert_eq!(
            flags_of(&packets[0]),
            flag::PIC_DATA | flag::START_OF_FRAME | flag::END_OF_FRAME
        );
        // The parity shard carries no picture-data flag.
        assert_eq!(flags_of(&packets[1]) & flag::PIC_DATA, 0);
        // Every packet is at least a full set of headers long.
        assert!(packets.iter().all(|p| p.len() >= PAYLOAD_OFFSET));
    }

    #[test]
    fn a_larger_frame_splits_into_several_data_shards() {
        let mut packetizer = Packetizer::new();
        let config = Config {
            shard_payload: 64,
            fec_percentage: 0,
        };
        let frame = vec![7_u8; 500];
        let packets = packetizer.packetize(&frame, false, 3, &config);
        // (8-byte header + 500) / 64 = 8 data shards, no parity at 0%.
        assert_eq!(packets.len(), (8 + 500_usize).div_ceil(64));
        assert_eq!(
            flags_of(&packets[0]) & flag::START_OF_FRAME,
            flag::START_OF_FRAME
        );
        assert_eq!(
            flags_of(packets.last().unwrap()) & flag::END_OF_FRAME,
            flag::END_OF_FRAME
        );
    }

    #[test]
    fn the_rtp_sequence_advances_across_frames() {
        let mut packetizer = Packetizer::new();
        let config = Config {
            shard_payload: 1024,
            fec_percentage: 0,
        };
        let first = packetizer.packetize(b"one", true, 0, &config);
        let second = packetizer.packetize(b"two", false, 1, &config);
        let seq = |p: &[u8]| u16::from_be_bytes([p[2], p[3]]);
        assert_eq!(seq(&first[0]), 0);
        assert_eq!(
            seq(&second[0]),
            1,
            "the sequence continues, it does not reset per frame"
        );
    }

    #[test]
    fn parity_is_capped_and_optional() {
        assert_eq!(parity_count(10, 0), 0);
        assert_eq!(parity_count(10, 20), 2);
        assert_eq!(
            parity_count(1, 1),
            1,
            "any FEC means at least one parity shard"
        );
        assert_eq!(parity_count(250, 100), super::MAX_SHARDS - 250);
    }
}
