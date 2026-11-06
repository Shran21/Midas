use openh264::{
    OpenH264API,
    encoder::{BitRate, Complexity, Encoder, EncoderConfig, FrameRate, RateControlMode, UsageType},
    formats::{BgraSliceU8, YUVBuffer},
};

use crate::{adapt::Params, capture::Frame};

/// Nearest-neighbour downscale to even dimensions, as required by 4:2:0 encoding.
pub fn scale(frame: &Frame, factor: f64, out: &mut Vec<u8>) -> (usize, usize) {
    let width = (((frame.width as f64 * factor) as usize) & !1).max(16);
    let height = (((frame.height as f64 * factor) as usize) & !1).max(16);
    out.resize(width * height * 4, 0);
    for y in 0..height {
        let sy = y * frame.height / height;
        let row = &frame.bgra[sy * frame.width * 4..(sy + 1) * frame.width * 4];
        let target = &mut out[y * width * 4..(y + 1) * width * 4];
        for x in 0..width {
            let sx = x * frame.width / width;
            target[x * 4..x * 4 + 4].copy_from_slice(&row[sx * 4..sx * 4 + 4]);
        }
    }
    (width, height)
}

pub struct H264 {
    encoder: Encoder,
    params: Params,
    dimensions: (usize, usize),
    scaled: Vec<u8>,
}

fn build(params: Params) -> Result<Encoder, String> {
    let config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(params.bitrate_bps as u32))
        .max_frame_rate(FrameRate::from_hz(params.fps as f32))
        .usage_type(UsageType::ScreenContentRealTime)
        .rate_control_mode(RateControlMode::Bitrate)
        .skip_frames(true)
        .complexity(Complexity::Low)
        .num_threads(4);
    Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| e.to_string())
}

impl H264 {
    pub fn new(params: Params) -> Result<Self, String> {
        Ok(Self {
            encoder: build(params)?,
            params,
            dimensions: (0, 0),
            scaled: Vec::new(),
        })
    }

    /// Applies new targets; a new encoder session starts with a keyframe.
    pub fn reconfigure(&mut self, params: Params) -> Result<(), String> {
        if params != self.params {
            self.encoder = build(params)?;
            self.params = params;
            self.dimensions = (0, 0);
        }
        Ok(())
    }

    pub fn keyframe(&mut self) {
        self.encoder.force_intra_frame();
    }

    /// Returns Annex B access unit bytes and the encoded dimensions; empty when the rate control skipped the frame.
    pub fn encode(&mut self, frame: &Frame) -> Result<(Vec<u8>, (usize, usize)), String> {
        let (width, height) = scale(frame, self.params.scale, &mut self.scaled);
        if self.dimensions != (width, height) {
            if self.dimensions != (0, 0) {
                self.encoder = build(self.params)?;
            }
            self.dimensions = (width, height);
        }
        let yuv = YUVBuffer::from_rgb_source(BgraSliceU8::new(&self.scaled, (width, height)));
        let bitstream = self.encoder.encode(&yuv).map_err(|e| e.to_string())?;
        Ok((bitstream.to_vec(), (width, height)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{Source, Synthetic};

    #[test]
    fn encodes_a_keyframe_and_scales_on_request() {
        let mut source = Synthetic::new(640, 360);
        let mut encoder = H264::new(Params {
            bitrate_bps: 1_000_000,
            scale: 1.0,
            fps: 30,
        })
        .unwrap();
        let (first, size) = encoder.encode(source.capture().unwrap()).unwrap();
        assert_eq!(size, (640, 360));
        assert!(
            first
                .windows(5)
                .any(|w| w == [0, 0, 0, 1, 0x67] || w == [0, 0, 1, 0x67, 0x42])
        );
        encoder
            .reconfigure(Params {
                bitrate_bps: 300_000,
                scale: 0.5,
                fps: 15,
            })
            .unwrap();
        let (_, size) = encoder.encode(source.capture().unwrap()).unwrap();
        assert_eq!(size, (320, 180));
    }
}
