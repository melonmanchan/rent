use crate::config::{self, NUMFONTSCALES};
use std::collections::HashMap;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::{CacheKey, FontRef, GlyphId};

/* An owned font face; swash's FontRef only borrows. */
struct Font {
    data: Vec<u8>,
    offset: u32,
    key: CacheKey,
    units_per_em: f32,
    /* font units */
    ascent: f32,
    descent: f32,
}

impl Font {
    fn load(data: Vec<u8>, index: u32) -> Option<Font> {
        let fr = FontRef::from_index(&data, index as usize)?;
        let m = fr.metrics(&[]);
        if m.units_per_em == 0 {
            return None;
        }
        let (offset, key) = (fr.offset, fr.key);
        Some(Font {
            offset,
            key,
            units_per_em: m.units_per_em as f32,
            ascent: m.ascent,
            descent: m.descent,
            data,
        })
    }

    fn as_ref(&self) -> FontRef<'_> {
        FontRef { data: &self.data, offset: self.offset, key: self.key }
    }

    fn glyph(&self, ch: char) -> GlyphId {
        self.as_ref().charmap().map(ch)
    }

    fn advance(&self, glyph: GlyphId, size: f32) -> f32 {
        self.as_ref().glyph_metrics(&[]).advance_width(glyph) * size / self.units_per_em
    }
}

pub struct Renderer {
    db: fontdb::Database,
    fonts: Vec<Font>,
    /* face ids already in `fonts`, parallel to it */
    loaded: Vec<fontdb::ID>,
    char_cache: HashMap<char, usize>,
    sizes: [f32; NUMFONTSCALES],
    scale_ctx: ScaleContext,
}

pub struct Fit {
    pub size: f32,
    pub font_h: f32,
    pub ascent: f32,
    pub width: f32,
    pub height: f32,
}

/* Zero-width scaffolding of emoji sequences (ZWNJ, ZWJ, variation
 * selectors); rendering their .notdef boxes would be pure noise. */
fn ignorable(ch: char) -> bool {
    matches!(ch, '\u{200C}' | '\u{200D}' | '\u{FE00}'..='\u{FE0F}')
}

impl Renderer {
    pub fn new() -> Result<Renderer, String> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut ids = Vec::new();
        let mut push = |id: Option<fontdb::ID>| {
            if let Some(id) = id {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        };
        for fam in config::FONT_FALLBACKS {
            push(db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                ..Default::default()
            }));
        }
        /* generic sans-serif before the emoji faces so text symbols
         * prefer the text look */
        push(db.query(&fontdb::Query {
            families: &[fontdb::Family::SansSerif],
            ..Default::default()
        }));
        for fam in config::EMOJI_FONTS {
            push(db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                ..Default::default()
            }));
        }

        let mut fonts = Vec::new();
        let mut loaded = Vec::new();
        for id in ids {
            let font = db
                .with_face_data(id, |data, index| Font::load(data.to_vec(), index))
                .flatten();
            if let Some(font) = font {
                fonts.push(font);
                loaded.push(id);
            }
        }
        if fonts.is_empty() {
            return Err("rent: unable to load any font".into());
        }

        let mut sizes = [0.0; NUMFONTSCALES];
        for (i, s) in sizes.iter_mut().enumerate() {
            *s = config::fontsz(i);
        }
        Ok(Renderer {
            db,
            fonts,
            loaded,
            char_cache: HashMap::new(),
            sizes,
            scale_ctx: ScaleContext::new(),
        })
    }

    /* Index of the first configured font covering ch; on a miss, hunt the
     * whole system database for any covering face, like sent does per
     * codepoint through XftFontMatch. 0 (.notdef) if nothing covers it. */
    fn font_idx(&mut self, ch: char) -> usize {
        if let Some(&i) = self.char_cache.get(&ch) {
            return i;
        }
        let idx = self
            .fonts
            .iter()
            .position(|f| f.glyph(ch) != 0)
            .or_else(|| self.search_db(ch))
            .unwrap_or(0);
        self.char_cache.insert(ch, idx);
        idx
    }

    fn search_db(&mut self, ch: char) -> Option<usize> {
        /* prefer regular text faces: non-emoji, upright, near-400 weight,
         * proportional */
        let mut cands: Vec<((bool, bool, u16, bool), fontdb::ID)> = self
            .db
            .faces()
            .filter(|f| !self.loaded.contains(&f.id))
            .map(|f| {
                let emoji = f
                    .families
                    .iter()
                    .any(|(n, _)| n.to_ascii_lowercase().contains("emoji"));
                let rank = (
                    emoji,
                    f.style != fontdb::Style::Normal,
                    f.weight.0.abs_diff(400),
                    f.monospaced,
                );
                (rank, f.id)
            })
            .collect();
        cands.sort_unstable();

        for (_, id) in cands {
            let covers = self
                .db
                .with_face_data(id, |data, index| {
                    FontRef::from_index(data, index as usize)
                        .is_some_and(|fr| fr.charmap().map(ch) != 0)
                })
                .unwrap_or(false);
            if !covers {
                continue;
            }
            let font = self
                .db
                .with_face_data(id, |data, index| Font::load(data.to_vec(), index))
                .flatten();
            if let Some(font) = font {
                self.fonts.push(font);
                self.loaded.push(id);
                return Some(self.fonts.len() - 1);
            }
        }
        None
    }

    /* Xft-style font height: ascent + descent of the primary face. */
    fn line_h(&self, size: f32) -> f32 {
        let f = &self.fonts[0];
        (f.ascent + f.descent) * size / f.units_per_em
    }

    fn ascent(&self, size: f32) -> f32 {
        let f = &self.fonts[0];
        f.ascent * size / f.units_per_em
    }

    pub fn text_width(&mut self, text: &str, size: f32) -> f32 {
        text.chars()
            .filter(|&ch| !ignorable(ch))
            .map(|ch| {
                let i = self.font_idx(ch);
                let f = &self.fonts[i];
                f.advance(f.glyph(ch), size)
            })
            .sum()
    }

    /* Mirror of sent's getfontsize(): pick the largest scale whose line
     * block fits the usable height, then shrink until the widest line
     * fits the usable width. */
    pub fn fit(&mut self, lines: &[String], uw: f32, uh: f32) -> Fit {
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
            .fold(0.0f32, f32::max);
        let font_h = self.line_h(size);
        Fit { size, font_h, ascent: self.ascent(size), width, height: font_h * lfac }
    }

    pub fn draw_slide(&mut self, frame: &mut [u32], fw: u32, fh: u32, lines: &[String]) {
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
        &mut self,
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
            if ignorable(ch) {
                continue;
            }
            let fi = self.font_idx(ch);
            let font = &self.fonts[fi];
            let glyph = font.glyph(ch);
            let mut scaler = self
                .scale_ctx
                .builder(font.as_ref())
                .size(size)
                .hint(false)
                .build();
            let img = Render::new(&[
                Source::ColorOutline(0),
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::Outline,
            ])
            .render(&mut scaler, glyph);
            if let Some(img) = img {
                let gx = pen.round() as i64 + img.placement.left as i64;
                let gtop = base - img.placement.top as i64;
                let (gw, gh) = (img.placement.width as usize, img.placement.height as usize);
                for row in 0..gh {
                    let py = gtop + row as i64;
                    if py < 0 || py >= fh as i64 {
                        continue;
                    }
                    for col in 0..gw {
                        let px = gx + col as i64;
                        if px < 0 || px >= fw as i64 {
                            continue;
                        }
                        let i = py as usize * fw as usize + px as usize;
                        match img.content {
                            Content::Mask => {
                                let a = img.data[row * gw + col] as u32;
                                if a > 0 {
                                    frame[i] = blend(frame[i], config::FOREGROUND, a);
                                }
                            }
                            Content::Color | Content::SubpixelMask => {
                                let p = &img.data[(row * gw + col) * 4..][..4];
                                let a = p[3] as u32;
                                if a > 0 {
                                    let src = ((p[0] as u32) << 16)
                                        | ((p[1] as u32) << 8)
                                        | p[2] as u32;
                                    frame[i] = blend(frame[i], src, a);
                                }
                            }
                        }
                    }
                }
            }
            pen += font.advance(glyph, size);
        }
    }
}

#[inline]
fn blend(dst: u32, src: u32, a: u32) -> u32 {
    let na = 255 - a;
    let r = (((src >> 16) & 0xFF) * a + ((dst >> 16) & 0xFF) * na) / 255;
    let g = (((src >> 8) & 0xFF) * a + ((dst >> 8) & 0xFF) * na) / 255;
    let b = ((src & 0xFF) * a + (dst & 0xFF) * na) / 255;
    (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    const FW: u32 = 800;
    const FH: u32 = 600;
    const WHITE: u32 = 0xFFFFFF;

    fn renderer() -> Renderer {
        Renderer::new().expect("system fonts should load")
    }

    fn white_frame() -> Vec<u32> {
        vec![WHITE; (FW * FH) as usize]
    }

    fn channels(px: u32) -> (u32, u32, u32) {
        ((px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
    }

    /* bounding box of all non-white pixels: (x0, y0, x1, y1) inclusive */
    fn ink_bbox(frame: &[u32]) -> Option<(u32, u32, u32, u32)> {
        let mut bb: Option<(u32, u32, u32, u32)> = None;
        for y in 0..FH {
            for x in 0..FW {
                if frame[(y * FW + x) as usize] != WHITE {
                    bb = Some(match bb {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => {
                            (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
                        }
                    });
                }
            }
        }
        bb
    }

    fn covers(r: &Renderer, ch: char) -> bool {
        r.fonts.iter().any(|f| f.glyph(ch) != 0)
    }

    #[test]
    fn ascii_slide_draws_dark_centered_ink() {
        let mut r = renderer();
        let mut frame = white_frame();
        r.draw_slide(&mut frame, FW, FH, &["hello".to_string()]);

        let dark = frame.iter().any(|&px| {
            let (cr, cg, cb) = channels(px);
            cr < 0x40 && cg < 0x40 && cb < 0x40
        });
        assert!(dark, "expected near-black glyph pixels on a white frame");

        let (x0, y0, x1, y1) = ink_bbox(&frame).expect("slide should leave ink");
        let cx = (x0 + x1) as f32 / 2.0;
        let cy = (y0 + y1) as f32 / 2.0;
        assert!(
            (cx - FW as f32 / 2.0).abs() <= FW as f32 * 0.15,
            "ink center x = {cx}, frame center x = {}",
            FW as f32 / 2.0
        );
        assert!(
            (cy - FH as f32 / 2.0).abs() <= FH as f32 * 0.15,
            "ink center y = {cy}, frame center y = {}",
            FH as f32 / 2.0
        );
    }

    #[test]
    fn fit_shrinks_longer_lines() {
        let mut r = renderer();
        let (uw, uh) = (FW as f32 * 0.75, FH as f32 * 0.75);
        let short = r.fit(&["hi".to_string()], uw, uh);
        let long = r.fit(
            &["the quick brown fox jumps over the lazy dog again and again".to_string()],
            uw,
            uh,
        );
        assert!(
            long.size < short.size,
            "long line size {} should be below short line size {}",
            long.size,
            short.size
        );
        assert!(short.width <= uw, "short width {} > uw {uw}", short.width);
        assert!(long.width <= uw, "long width {} > uw {uw}", long.width);
    }

    #[test]
    fn empty_line_leaves_frame_white() {
        let mut r = renderer();
        let mut frame = white_frame();
        r.draw_slide(&mut frame, FW, FH, &[String::new()]);
        assert!(
            frame.iter().all(|&px| px == WHITE),
            "empty slide must not touch the frame"
        );
    }

    #[test]
    fn emoji_slide_draws_color() {
        let mut r = renderer();
        if !covers(&r, '\u{1F600}') {
            eprintln!("skipping emoji_slide_draws_color: no font covers U+1F600");
            return;
        }
        let mut frame = white_frame();
        r.draw_slide(&mut frame, FW, FH, &["\u{1F600}".to_string()]);
        /* grayscale text has r == g == b; only the color-glyph path can
         * produce a chromatic pixel */
        let colored = frame.iter().any(|&px| {
            let (cr, cg, cb) = channels(px);
            cr.abs_diff(cg).max(cg.abs_diff(cb)).max(cr.abs_diff(cb)) > 24
        });
        assert!(colored, "expected chromatic pixels from the color emoji path");
    }

    #[test]
    fn ignorables_have_zero_width() {
        let mut r = renderer();
        assert!(ignorable('\u{200D}'), "ZWJ must be ignorable");
        assert!(ignorable('\u{FE0F}'), "VS16 must be ignorable");
        assert_eq!(r.text_width("\u{200D}", 100.0), 0.0);
        /* FE0F is filtered before any font lookup, so this equality must
         * hold whether or not an emoji font is installed */
        let bare = r.text_width("\u{1F44D}", 100.0);
        let with_vs16 = r.text_width("\u{1F44D}\u{FE0F}", 100.0);
        assert_eq!(with_vs16, bare, "VS16 must not change the advance");
        if covers(&r, '\u{1F44D}') {
            assert!(bare > 0.0, "covered emoji should have a positive advance");
        } else {
            eprintln!("no font covers U+1F44D; skipping positive-advance check");
        }
    }

    #[test]
    fn two_line_block_height_uses_linespacing() {
        let mut r = renderer();
        let fit = r.fit(
            &["one".to_string(), "two".to_string()],
            FW as f32 * 0.75,
            FH as f32 * 0.75,
        );
        let expected = fit.font_h * (config::LINESPACING + 1.0);
        assert!(
            (fit.height - expected).abs() < 0.01,
            "height {} != font_h * 2.4 = {expected}",
            fit.height
        );
    }

    #[test]
    fn dynamic_fallback_finds_system_font_for_symbols() {
        let mut r = renderer();
        /* U+25B8 BLACK RIGHT-POINTING SMALL TRIANGLE: absent from common
         * text faces (e.g. Helvetica Neue) but covered by system fonts
         * like Menlo/Apple Symbols, so it exercises the fontdb search */
        let ch = '\u{25B8}';
        let pre = covers(&r, ch);

        let idx = r.font_idx(ch);
        if !pre && idx == 0 && r.fonts[0].glyph(ch) == 0 {
            eprintln!("no system font covers U+25B8; skipping fallback check");
            return;
        }
        assert_ne!(
            r.fonts[idx].glyph(ch),
            0,
            "font_idx picked font {idx}, which does not cover U+25B8"
        );

        /* char -> index cache must hand back the same face on repeat */
        assert_eq!(
            r.font_idx(ch),
            idx,
            "second lookup returned a different font index"
        );

        assert!(
            r.text_width("\u{25B8}", 100.0) > 0.0,
            "covered symbol should have a positive advance"
        );
    }
}
