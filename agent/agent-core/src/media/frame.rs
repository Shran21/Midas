//! Converts captured BGRA frames to the NV12 layout H.264 encoders take, scaling by area
//! averaging so that downscaled text stays legible. Colours follow BT.709 in limited range, which
//! the encoder also signals to the decoder.

/// Fixed-point BT.709 limited-range coefficients, scaled by 256.
const Y: [i32; 3] = [47, 157, 16];
const U: [i32; 3] = [-26, -87, 113];
const V: [i32; 3] = [112, -102, -10];

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The source buffer is shorter than its stride and height describe.
    Truncated,
    /// NV12 needs even, non-zero output dimensions no larger than the source.
    Size,
}

/// One BGRA source: `stride` bytes per row, at least `width * 4`.
#[derive(Clone, Copy)]
pub struct Bgra<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
}

/// The NV12 buffer size for a frame: a full luma plane and a half-height interleaved chroma plane.
pub fn nv12_len(width: u32, height: u32) -> usize {
    width as usize * height as usize * 3 / 2
}

fn span(index: u32, from: u32, to: u32) -> (usize, usize) {
    let start = (u64::from(index) * u64::from(from) / u64::from(to)) as usize;
    let end = (u64::from(index + 1) * u64::from(from) / u64::from(to)) as usize;
    (start, end.max(start + 1))
}

/// Writes `source` scaled to `width` by `height` into `out` as NV12, reusing its allocation.
pub fn bgra_to_nv12(
    source: Bgra<'_>,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), FrameError> {
    if width == 0
        || height == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || width > source.width
        || height > source.height
    {
        return Err(FrameError::Size);
    }
    let row = source.width as usize * 4;
    let needed = source
        .stride
        .checked_mul(source.height as usize - 1)
        .and_then(|bytes| bytes.checked_add(row));
    if source.stride < row || needed.is_none_or(|needed| source.data.len() < needed) {
        return Err(FrameError::Truncated);
    }
    let (w, h) = (width as usize, height as usize);
    out.clear();
    out.resize(nv12_len(width, height), 0);
    let (luma, chroma) = out.split_at_mut(w * h);
    let columns: Vec<_> = (0..width).map(|x| span(x, source.width, width)).collect();
    // Two output rows of averaged RGB, so each chroma sample averages a 2x2 block.
    let mut rows = vec![[0i32; 3]; w * 2];
    for pair in 0..h / 2 {
        for half in 0..2 {
            let y = pair * 2 + half;
            let (top, bottom) = span(y as u32, source.height, height);
            for (x, &(left, right)) in columns.iter().enumerate() {
                let mut sum = [0i64; 3];
                for line in top..bottom {
                    let start = line * source.stride;
                    for pixel in source.data[start + left * 4..start + right * 4]
                        .as_chunks::<4>()
                        .0
                    {
                        sum[0] += i64::from(pixel[2]);
                        sum[1] += i64::from(pixel[1]);
                        sum[2] += i64::from(pixel[0]);
                    }
                }
                let count = ((bottom - top) * (right - left)) as i64;
                let rgb = sum.map(|channel| ((channel + count / 2) / count) as i32);
                luma[y * w + x] = (16 + (dot(Y, rgb) + 128) / 256).clamp(16, 235) as u8;
                rows[half * w + x] = rgb;
            }
        }
        for x in 0..w / 2 {
            let mut rgb = [0i32; 3];
            for (half, column) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let pixel = rows[half * w + x * 2 + column];
                for channel in 0..3 {
                    rgb[channel] += pixel[channel];
                }
            }
            let rgb = rgb.map(|channel| (channel + 2) / 4);
            let at = pair * w + x * 2;
            chroma[at] = (128 + (dot(U, rgb) + 128).div_euclid(256)).clamp(16, 240) as u8;
            chroma[at + 1] = (128 + (dot(V, rgb) + 128).div_euclid(256)).clamp(16, 240) as u8;
        }
    }
    Ok(())
}

fn dot(weights: [i32; 3], rgb: [i32; 3]) -> i32 {
    weights[0] * rgb[0] + weights[1] * rgb[1] + weights[2] * rgb[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, bgra: [u8; 4]) -> Vec<u8> {
        bgra.repeat((width * height) as usize)
    }

    fn convert(data: &[u8], size: (u32, u32), to: (u32, u32)) -> Vec<u8> {
        let mut out = Vec::new();
        bgra_to_nv12(
            Bgra {
                data,
                width: size.0,
                height: size.1,
                stride: size.0 as usize * 4,
            },
            to.0,
            to.1,
            &mut out,
        )
        .unwrap();
        out
    }

    #[test]
    fn reference_colours_match_bt709_limited_range() {
        for (bgra, expected) in [
            ([255, 255, 255, 255], [235, 128, 128]),
            ([0, 0, 0, 255], [16, 128, 128]),
            ([0, 0, 255, 255], [63, 102, 240]),
            ([0, 255, 0, 255], [173, 42, 26]),
            ([255, 0, 0, 255], [32, 240, 118]),
        ] {
            let out = convert(&solid(4, 4, bgra), (4, 4), (4, 4));
            let (luma, chroma) = out.split_at(16);
            for (got, want) in [
                (luma[5], expected[0]),
                (chroma[0], expected[1]),
                (chroma[1], expected[2]),
            ] {
                assert!(got.abs_diff(want) <= 1, "{bgra:?}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn downscaling_averages_areas_so_fine_detail_turns_grey() {
        let mut checker = Vec::new();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let value = if (x + y) % 2 == 0 { 255 } else { 0 };
                checker.extend([value, value, value, 255]);
            }
        }
        let out = convert(&checker, (8, 8), (4, 4));
        assert_eq!(out.len(), nv12_len(4, 4));
        assert!(
            out[..16].iter().all(|&y| y.abs_diff(126) <= 1),
            "{:?}",
            &out[..16]
        );
        assert!(out[16..].iter().all(|&c| c == 128));
    }

    #[test]
    fn padded_rows_and_odd_sources_convert_without_reading_padding() {
        let (width, height, stride) = (5u32, 3u32, 32usize);
        let mut data = vec![7u8; stride * height as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                data[y * stride + x * 4..y * stride + x * 4 + 4]
                    .copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let mut out = Vec::new();
        let source = Bgra {
            data: &data[..stride * 2 + width as usize * 4],
            width,
            height,
            stride,
        };
        bgra_to_nv12(source, 4, 2, &mut out).unwrap();
        assert!(out[..8].iter().all(|&y| y == 235));
    }

    #[test]
    fn invalid_sizes_and_short_buffers_are_refused() {
        let data = solid(4, 4, [0, 0, 0, 255]);
        let source = Bgra {
            data: &data,
            width: 4,
            height: 4,
            stride: 16,
        };
        let mut out = Vec::new();
        for (width, height) in [(0, 2), (3, 2), (2, 3), (6, 4), (4, 6)] {
            assert_eq!(
                bgra_to_nv12(source, width, height, &mut out),
                Err(FrameError::Size)
            );
        }
        let short = Bgra {
            data: &data[..60],
            ..source
        };
        assert_eq!(
            bgra_to_nv12(short, 4, 4, &mut out),
            Err(FrameError::Truncated)
        );
        let narrow = Bgra {
            stride: 12,
            ..source
        };
        assert_eq!(
            bgra_to_nv12(narrow, 4, 4, &mut out),
            Err(FrameError::Truncated)
        );
    }
}
