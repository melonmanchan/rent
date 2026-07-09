use crate::config::{self, NUMFONTSCALES};
use fontdue::{Font, FontSettings};

pub struct Renderer {
    fonts: Vec<Font>,
    sizes: [f32; NUMFONTSCALES],
}

pub struct Fit {
    pub size: f32,
    pub font_h: f32,
    pub ascent: f32,
    pub width: f32,
    pub height: f32,
}

impl Renderer {
    pub fn new() -> Result<Renderer, String> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut ids = Vec::new();
        for fam in config::FONT_FALLBACKS {
            let q = fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                ..Default::default()
            };
            if let Some(id) = db.query(&q) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        /* last resort: whatever the system calls sans-serif */
        let q = fontdb::Query {
            families: &[fontdb::Family::SansSerif],
            ..Default::default()
        };
        if let Some(id) = db.query(&q) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }

        let mut fonts = Vec::new();
        for id in ids {
            let loaded = db.with_face_data(id, |data, index| {
                Font::from_bytes(data, FontSettings {
                    collection_index: index,
                    ..FontSettings::default()
                })
            });
            if let Some(Ok(font)) = loaded {
                if font.horizontal_line_metrics(config::fontsz(0)).is_some() {
                    fonts.push(font);
                }
            }
        }
        if fonts.is_empty() {
            return Err("rent: unable to load any font".into());
        }

        let mut sizes = [0.0; NUMFONTSCALES];
        for (i, s) in sizes.iter_mut().enumerate() {
            *s = config::fontsz(i);
        }
        Ok(Renderer { fonts, sizes })
    }

    fn font_for(&self, ch: char) -> &Font {
        self.fonts
            .iter()
            .find(|f| f.lookup_glyph_index(ch) != 0)
            .unwrap_or(&self.fonts[0])
    }

    /* Xft-style font height: ascent + descent of the primary face. */
    fn line_h(&self, size: f32) -> f32 {
        let m = self.fonts[0].horizontal_line_metrics(size).unwrap();
        m.ascent - m.descent
    }

    pub fn text_width(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|ch| self.font_for(ch).metrics(ch, size).advance_width)
            .sum()
    }

    /* Mirror of sent's getfontsize(): pick the largest scale whose line
     * block fits the usable height, then shrink until the widest line
     * fits the usable width. */
    pub fn fit(&self, lines: &[String], uw: f32, uh: f32) -> Fit {
        let lfac = config::LINESPACING * lines.len().saturating_sub(1) as f32 + 1.0;
        let mut j = (0..NUMFONTSCALES)
            .rev()
            .find(|&j| self.line_h(self.sizes[j]) * lfac <= uh)
            .unwrap_or(0);
        for line in lines {
            while j > 0 && self.text_width(line, self.sizes[j]) > uw {
                j -= 1;
            }
        }
        let size = self.sizes[j];
        let width = lines
            .iter()
            .map(|l| self.text_width(l, size))
            .fold(0.0, f32::max);
        let font_h = self.line_h(size);
        let ascent = self.fonts[0].horizontal_line_metrics(size).unwrap().ascent;
        Fit { size, font_h, ascent, width, height: font_h * lfac }
    }

    pub fn draw_slide(&self, frame: &mut [u32], fw: u32, fh: u32, lines: &[String]) {
        let uw = fw as f32 * config::USABLEWIDTH;
        let uh = fh as f32 * config::USABLEHEIGHT;
        let fit = self.fit(lines, uw, uh);
        let x = (fw as f32 - fit.width) / 2.0;
        for (i, line) in lines.iter().enumerate() {
            let y = (fh as f32 - fit.height) / 2.0 + i as f32 * config::LINESPACING * fit.font_h;
            self.draw_line(frame, fw, fh, x, y + fit.ascent, line, fit.size);
        }
    }

    fn draw_line(
        &self,
        frame: &mut [u32],
        fw: u32,
        fh: u32,
        x: f32,
        baseline: f32,
        text: &str,
        size: f32,
    ) {
        let mut pen = x;
        let base = baseline.round() as i64;
        for ch in text.chars() {
            let font = self.font_for(ch);
            let (m, cov) = font.rasterize(ch, size);
            let gx = (pen + m.xmin as f32).round() as i64;
            let gtop = base - (m.ymin + m.height as i32) as i64;
            for row in 0..m.height {
                let py = gtop + row as i64;
                if py < 0 || py >= fh as i64 {
                    continue;
                }
                for col in 0..m.width {
                    let px = gx + col as i64;
                    if px < 0 || px >= fw as i64 {
                        continue;
                    }
                    let a = cov[row * m.width + col] as u32;
                    if a == 0 {
                        continue;
                    }
                    let i = py as usize * fw as usize + px as usize;
                    frame[i] = blend(frame[i], config::FOREGROUND, a);
                }
            }
            pen += m.advance_width;
        }
    }
}

#[inline]
fn blend(dst: u32, fg: u32, a: u32) -> u32 {
    let na = 255 - a;
    let r = (((fg >> 16) & 0xFF) * a + ((dst >> 16) & 0xFF) * na) / 255;
    let g = (((fg >> 8) & 0xFF) * a + ((dst >> 8) & 0xFF) * na) / 255;
    let b = ((fg & 0xFF) * a + (dst & 0xFF) * na) / 255;
    (r << 16) | (g << 8) | b
}
