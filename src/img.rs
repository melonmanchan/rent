use crate::config;

pub struct Img {
    w: u32,
    h: u32,
    /* 3 bytes per pixel, alpha pre-blended against BACKGROUND like ffload() */
    rgb: Vec<u8>,
    scaled: Option<Scaled>,
}

struct Scaled {
    w: u32,
    h: u32,
    pix: Vec<u32>,
}

impl Img {
    pub fn load(path: &str) -> Result<Img, String> {
        let data =
            std::fs::read(path).map_err(|e| format!("rent: unable to open '{path}': {e}"))?;
        let (w, h, rgba) = if data.starts_with(b"farbfeld") {
            decode_farbfeld(&data).map_err(|e| format!("rent: '{path}': {e}"))?
        } else {
            let im = image::load_from_memory(&data)
                .map_err(|e| format!("rent: unable to decode '{path}': {e}"))?
                .to_rgba8();
            (im.width(), im.height(), im.into_raw())
        };

        /* blend the opaque part of the image with the window background
         * color to emulate transparency */
        let (br, bg, bb) = channels(config::BACKGROUND);
        let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
        for px in rgba.chunks_exact(4) {
            let a = px[3] as u32;
            let na = 255 - a;
            rgb.push(((px[0] as u32 * a + br * na) / 255) as u8);
            rgb.push(((px[1] as u32 * a + bg * na) / 255) as u8);
            rgb.push(((px[2] as u32 * a + bb * na) / 255) as u8);
        }
        Ok(Img { w, h, rgb, scaled: None })
    }

    pub fn invalidate(&mut self) {
        self.scaled = None;
    }

    /* Scale to fit the usable area (sent's ffprepare) and blit centered
     * into the frame (sent's ffdraw). */
    pub fn draw(&mut self, frame: &mut [u32], fw: u32, fh: u32, uw: u32, uh: u32) {
        let (tw, th) = fit_rect(self.w, self.h, uw, uh);
        if tw == 0 || th == 0 {
            return;
        }
        if self.scaled.as_ref().map(|s| (s.w, s.h)) != Some((tw, th)) {
            self.scaled = Some(self.scale(tw, th));
        }
        let s = self.scaled.as_ref().unwrap();
        let xoff = ((fw - tw) / 2) as usize;
        let yoff = ((fh - th) / 2) as usize;
        for y in 0..th as usize {
            let src = &s.pix[y * tw as usize..(y + 1) * tw as usize];
            let dst = (yoff + y) * fw as usize + xoff;
            frame[dst..dst + tw as usize].copy_from_slice(src);
        }
    }

    /* nearest neighbor, like sent's ffscale() */
    fn scale(&self, tw: u32, th: u32) -> Scaled {
        let mut pix = Vec::with_capacity(tw as usize * th as usize);
        for y in 0..th as u64 {
            let sy = (y * self.h as u64 / th as u64) as usize;
            let row = &self.rgb[sy * self.w as usize * 3..];
            for x in 0..tw as u64 {
                let sx = (x * self.w as u64 / tw as u64) as usize;
                let p = &row[sx * 3..sx * 3 + 3];
                pix.push(((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32);
            }
        }
        Scaled { w: tw, h: th, pix }
    }
}

/* Largest w x h rectangle with the aspect ratio of (bw, bh) that fits
 * into (uw, uh). Same arithmetic as sent's ffprepare(). */
pub fn fit_rect(bw: u32, bh: u32, uw: u32, uh: u32) -> (u32, u32) {
    if bw == 0 || bh == 0 || uw == 0 || uh == 0 {
        return (0, 0);
    }
    if uw as u64 * bh as u64 > uh as u64 * bw as u64 {
        ((bw as u64 * uh as u64 / bh as u64) as u32, uh)
    } else {
        (uw, (bh as u64 * uw as u64 / bw as u64) as u32)
    }
}

/* farbfeld: "farbfeld" magic, u32 BE width and height, rows of
 * 16-bit BE RGBA. Returns 8-bit RGBA. */
pub fn decode_farbfeld(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if data.len() < 16 || &data[..8] != b"farbfeld" {
        return Err("no valid farbfeld header".into());
    }
    let w = u32::from_be_bytes(data[8..12].try_into().unwrap());
    let h = u32::from_be_bytes(data[12..16].try_into().unwrap());
    let npx = w as usize * h as usize;
    let need = 16 + npx * 8;
    if data.len() < need {
        return Err("truncated farbfeld data".into());
    }
    let mut rgba = Vec::with_capacity(npx * 4);
    for c in data[16..need].chunks_exact(2) {
        rgba.push((u16::from_be_bytes([c[0], c[1]]) / 257) as u8);
    }
    Ok((w, h, rgba))
}

fn channels(c: u32) -> (u32, u32, u32) {
    ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    /* build a farbfeld buffer: every pixel gets the same 16-bit RGBA */
    fn farbfeld(w: u32, h: u32, rgba16: [u16; 4]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"farbfeld");
        buf.extend_from_slice(&w.to_be_bytes());
        buf.extend_from_slice(&h.to_be_bytes());
        for _ in 0..(w as usize * h as usize) {
            for c in rgba16 {
                buf.extend_from_slice(&c.to_be_bytes());
            }
        }
        buf
    }

    #[test]
    fn decode_valid_buffer_dims_and_channel_mapping() {
        let (w, h, rgba) = decode_farbfeld(&farbfeld(3, 2, [0xFFFF, 0x0000, 0x8000, 0x0101]))
            .expect("valid farbfeld must decode");
        assert_eq!((w, h), (3, 2));
        assert_eq!(rgba.len(), 3 * 2 * 4);
        for px in rgba.chunks_exact(4) {
            assert_eq!(px[0], 255); /* 0xFFFF -> 255 */
            assert_eq!(px[1], 0); /* 0x0000 -> 0 */
            assert_eq!(px[2], (0x8000u16 / 257) as u8); /* mid value -> v/257 = 127 */
            assert_eq!(px[3], 1); /* 0x0101 / 257 == 1 */
        }
    }

    #[test]
    fn decode_bad_magic_is_error() {
        let mut buf = farbfeld(1, 1, [0, 0, 0, 0]);
        buf[0] = b'F';
        assert!(decode_farbfeld(&buf).is_err());
    }

    #[test]
    fn decode_truncated_pixel_data_is_error() {
        let mut buf = farbfeld(2, 2, [0xFFFF; 4]);
        buf.truncate(buf.len() - 1);
        assert!(decode_farbfeld(&buf).is_err());
        /* header only, pixels missing entirely */
        assert!(decode_farbfeld(&farbfeld(2, 2, [0; 4])[..16]).is_err());
    }

    #[test]
    fn decode_short_header_is_error() {
        assert!(decode_farbfeld(b"").is_err());
        assert!(decode_farbfeld(b"farbfeld").is_err());
        assert!(decode_farbfeld(&b"farbfeld\x00\x00\x00\x01"[..]).is_err());
    }

    #[test]
    fn fit_wide_image_limited_by_width() {
        /* 2:1 image into a square: width is the binding constraint */
        assert_eq!(fit_rect(200, 100, 100, 100), (100, 50));
    }

    #[test]
    fn fit_tall_image_limited_by_height() {
        /* 1:2 image into a square: height is the binding constraint */
        assert_eq!(fit_rect(100, 200, 100, 100), (50, 100));
    }

    #[test]
    fn fit_exact_passthrough() {
        assert_eq!(fit_rect(123, 45, 123, 45), (123, 45));
        /* same aspect, integer upscale */
        assert_eq!(fit_rect(100, 50, 200, 100), (200, 100));
    }

    #[test]
    fn fit_zero_dimension_gives_zero() {
        assert_eq!(fit_rect(0, 100, 50, 50), (0, 0));
        assert_eq!(fit_rect(100, 0, 50, 50), (0, 0));
        assert_eq!(fit_rect(100, 100, 0, 50), (0, 0));
        assert_eq!(fit_rect(100, 100, 50, 0), (0, 0));
    }

    #[test]
    fn fit_result_bounded_and_aspect_preserved() {
        let cases: &[(u32, u32, u32, u32)] = &[
            (1920, 1080, 800, 600),
            (1080, 1920, 800, 600),
            (1, 1000, 500, 500),
            (1000, 1, 500, 500),
            (640, 480, 640, 480),
            (3, 7, 1000, 1000),
            (7, 3, 13, 17),
        ];
        for &(bw, bh, uw, uh) in cases {
            let (w, h) = fit_rect(bw, bh, uw, uh);
            assert!(w <= uw && h <= uh, "({bw},{bh}) into ({uw},{uh}) gave ({w},{h})");
            /* one side always fills the usable area exactly */
            assert!(w == uw || h == uh, "({bw},{bh}) into ({uw},{uh}) gave ({w},{h})");
            /* aspect preserved within integer truncation: the scaled side is
             * floor(exact), so w*bh <= h*bw + bw and h*bw <= w*bh + bh */
            let (wbh, hbw) = (w as u64 * bh as u64, h as u64 * bw as u64);
            assert!(
                wbh <= hbw + bw as u64 && hbw <= wbh + bh as u64,
                "aspect drift: ({bw},{bh}) into ({uw},{uh}) gave ({w},{h})"
            );
        }
    }
}
