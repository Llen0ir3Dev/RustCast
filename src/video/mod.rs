//! Video-side helpers shared by the media engine and the preview widget.

/// A tightly packed RGBA frame ready for upload to a GPU texture.
#[derive(Clone, Debug)]
pub struct PreviewFrame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl PreviewFrame {
    pub fn new(width: usize, height: usize, rgba: Vec<u8>) -> Self {
        Self {
            width,
            height,
            rgba,
        }
    }
}

/// Copies a strided GStreamer video plane into a tightly packed RGBA buffer.
pub fn pack_rgba(plane: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 4];
    let row_bytes = width * 4;
    for y in 0..height {
        let src_start = y.saturating_mul(stride);
        let src_end = (src_start + row_bytes).min(plane.len());
        if src_start >= plane.len() {
            break;
        }
        let dst_start = y * row_bytes;
        out[dst_start..dst_start + row_bytes]
            .copy_from_slice(&plane[src_start..src_end]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_rgba_respects_stride() {
        let mut plane = vec![0u8; 6 * 4];
        for x in 0..4 {
            plane[x] = x as u8;
        }
        for y in 1..4 {
            for x in 0..4 {
                plane[y * 6 + x] = (y * 4 + x) as u8 + 10;
            }
        }
        let packed = pack_rgba(&plane, 6, 4, 4);
        assert_eq!(packed.len(), 4 * 4 * 4);
        assert_eq!(packed[0], 0);
        assert_eq!(packed[4 * 4], 14);
    }
}
