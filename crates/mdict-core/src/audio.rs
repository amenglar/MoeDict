use oxideav_speex::{SpeexHeader, SpeexStreamDecoder};

use crate::util::mime_type;

/// Return the bytes in a format the webview can actually play. Ogg/Speex is
/// decoded to PCM WAV; everything else is passed through untouched.
pub(crate) fn playable(bytes: Vec<u8>, path: &str) -> (Vec<u8>, &'static str) {
    let mime = mime_type(path);
    if path.to_ascii_lowercase().ends_with(".spx") || mime == "audio/ogg" {
        if let Some(wav) = spx_to_wav(&bytes) {
            return (wav, "audio/wav");
        }
    }
    (bytes, mime)
}

/// Convert an Ogg/Speex (`.spx`) payload to 16-bit PCM WAV.
///
/// Webviews do not decode Speex, so dictionary pronunciation audio stored in
/// Ogg/Speex is remuxed on demand into a universally playable WAV. Returns
/// `None` for anything that is not a valid Speex stream.
pub fn spx_to_wav(bytes: &[u8]) -> Option<Vec<u8>> {
    let packets = ogg_packets(bytes)?;
    let (header_packet, audio) = packets.split_first()?;
    let header = SpeexHeader::parse(header_packet).ok()?;
    if !header.is_known_mode() {
        return None;
    }
    let mut decoder = SpeexStreamDecoder::for_header(&header).ok()?;
    let mut pcm = Vec::new();
    for packet in audio.iter().skip(header.extra_headers as usize) {
        if let Ok(samples) = decoder.decode_packet_pcm_i16(packet) {
            pcm.extend_from_slice(&samples);
        }
    }
    if pcm.is_empty() {
        return None;
    }
    Some(encode_wav(&pcm, decoder.output_rate_hz()))
}

fn ogg_packets(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    if !bytes.starts_with(b"OggS") {
        return None;
    }
    let mut pos = 0usize;
    let mut packets = Vec::new();
    let mut current = Vec::new();
    while pos + 27 <= bytes.len() {
        if &bytes[pos..pos + 4] != b"OggS" {
            break;
        }
        let segments = bytes[pos + 26] as usize;
        let table = pos + 27;
        if table + segments > bytes.len() {
            return None;
        }
        let lengths = &bytes[table..table + segments];
        let body_len: usize = lengths.iter().map(|&n| n as usize).sum();
        let body = table + segments;
        if body + body_len > bytes.len() {
            return None;
        }
        let mut offset = body;
        for &length in lengths {
            let end = offset + length as usize;
            current.extend_from_slice(&bytes[offset..end]);
            offset = end;
            if length < 255 {
                if !current.is_empty() {
                    packets.push(std::mem::take(&mut current));
                }
            }
        }
        pos = offset;
    }
    if packets.is_empty() {
        None
    } else {
        Some(packets)
    }
}

fn encode_wav(pcm: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &sample in pcm {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_ogg_payloads() {
        assert!(spx_to_wav(b"ID3\x04\0\0\0\0\0").is_none());
        assert!(spx_to_wav(b"").is_none());
    }

    #[test]
    fn decodes_ogg_speex_into_wav() {
        use oxideav_speex::SpeexHeader;

        let header = SpeexHeader {
            speex_string: *b"Speex   ",
            speex_version: [0; 20],
            speex_version_id: 1,
            header_size: 80,
            rate: 8000,
            mode: 0,
            mode_bitstream_version: 4,
            nb_channels: 1,
            bitrate: 0,
            frame_size: 160,
            vbr: 0,
            frames_per_packet: 1,
            extra_headers: 0,
            reserved1: 0,
            reserved2: 0,
        };
        let mut stream = Vec::new();
        for (sequence, payload) in [header.write_bytes().to_vec(), vec![0u8; 2]]
            .iter()
            .enumerate()
        {
            stream.extend_from_slice(b"OggS");
            stream.push(0);
            stream.push(0);
            stream.extend_from_slice(&0u64.to_le_bytes());
            stream.extend_from_slice(&1u32.to_le_bytes());
            stream.extend_from_slice(&(sequence as u32).to_le_bytes());
            stream.extend_from_slice(&0u32.to_le_bytes());
            stream.push(1);
            stream.push(payload.len() as u8);
            stream.extend_from_slice(payload);
        }

        let wav = spx_to_wav(&stream).expect("decode minimal speex stream");
        assert!(wav.starts_with(b"RIFF"));
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 8000);
    }
}
