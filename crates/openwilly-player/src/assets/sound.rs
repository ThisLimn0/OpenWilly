//! Sound extraction from Director sndS/sndH chunks
//!
//! Director stores sounds as:
//! - sndH: header with sample rate, bit depth, etc.
//! - sndS: raw PCM sample data
//!
//! Output: WAV-compatible PCM data

use byteorder::{LittleEndian, WriteBytesExt};

/// Extracted sound ready for playback
#[derive(Debug, Clone)]
pub struct DecodedSound {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub pcm_data: Vec<u8>,
}

impl DecodedSound {
    /// Convert raw sndS data into a playable sound
    pub fn from_raw_pcm(data: &[u8], sample_rate: u32, bits_per_sample: u16) -> Self {
        let channels = 1u16; // Director 6 sounds are mono

        let pcm_data = if bits_per_sample == 16 {
            // 16-bit samples may need byte-swapping (Mac -> PC)
            let mut swapped = Vec::with_capacity(data.len());
            for chunk in data.chunks(2) {
                if chunk.len() == 2 {
                    swapped.push(chunk[1]);
                    swapped.push(chunk[0]);
                }
            }
            swapped
        } else {
            // 8-bit PCM: Director uses unsigned, WAV also uses unsigned for 8-bit
            data.to_vec()
        };

        DecodedSound {
            sample_rate,
            channels,
            bits_per_sample,
            pcm_data,
        }
    }

    /// Encode as WAV file bytes
    pub fn to_wav(&self) -> Vec<u8> {
        let byte_rate =
            self.sample_rate * self.channels as u32 * self.bits_per_sample as u32 / 8;
        let block_align = self.channels * self.bits_per_sample / 8;
        let data_len = self.pcm_data.len() as u32;
        let file_len = 36 + data_len;

        let mut wav = Vec::with_capacity(file_len as usize + 8);

        // RIFF header
        wav.extend_from_slice(b"RIFF");
        wav.write_u32::<LittleEndian>(file_len).unwrap();
        wav.extend_from_slice(b"WAVE");

        // fmt chunk
        wav.extend_from_slice(b"fmt ");
        wav.write_u32::<LittleEndian>(16).unwrap(); // chunk size
        wav.write_u16::<LittleEndian>(1).unwrap(); // PCM format
        wav.write_u16::<LittleEndian>(self.channels).unwrap();
        wav.write_u32::<LittleEndian>(self.sample_rate).unwrap();
        wav.write_u32::<LittleEndian>(byte_rate).unwrap();
        wav.write_u16::<LittleEndian>(block_align).unwrap();
        wav.write_u16::<LittleEndian>(self.bits_per_sample).unwrap();

        // data chunk
        wav.extend_from_slice(b"data");
        wav.write_u32::<LittleEndian>(data_len).unwrap();
        wav.extend_from_slice(&self.pcm_data);

        wav
    }
}
