use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::config;
use crate::draw::Renderer;
use crate::img::Img;
use crate::slide::Slide;

/* Export every slide as a rasterized PDF page: each page embeds a
 * flate-compressed RGB bitmap rendered by the exact same code path as
 * the window, at EXPORTWIDTH x EXPORTHEIGHT. Pages are scaled to a
 * 720pt-high media box with the same aspect ratio. */
pub fn export(
    path: &str,
    renderer: &mut Renderer,
    slides: &[Slide],
    images: &mut [Option<Img>],
) -> Result<(), String> {
    let (w, h) = (config::EXPORTWIDTH, config::EXPORTHEIGHT);
    let ph: f32 = 720.0;
    let pw: f32 = (w as f32 * ph / h as f32).round();
    let n = slides.len();

    /* object layout: 1 catalog, 2 pages, then (page, contents, image)
     * triples per slide */
    let mut objs: Vec<Vec<u8>> = Vec::with_capacity(2 + 3 * n);
    let kids: String = (0..n).map(|i| format!("{} 0 R ", 3 + 3 * i)).collect();
    objs.push("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".into());
    objs.push(
        format!("2 0 obj\n<< /Type /Pages /Kids [ {kids}] /Count {n} >>\nendobj\n").into_bytes(),
    );

    for i in 0..n {
        let page_id = 3 + 3 * i;
        let content_id = page_id + 1;
        let image_id = page_id + 2;

        objs.push(
            format!(
                "{page_id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] \
                 /Resources << /XObject << /I {image_id} 0 R >> >> \
                 /Contents {content_id} 0 R >>\nendobj\n"
            )
            .into_bytes(),
        );

        let cs = format!("q {pw} 0 0 {ph} 0 0 cm /I Do Q");
        objs.push(
            format!(
                "{content_id} 0 obj\n<< /Length {} >>\nstream\n{cs}\nendstream\nendobj\n",
                cs.len()
            )
            .into_bytes(),
        );

        /* render the slide exactly like the window would */
        let mut frame = vec![config::BACKGROUND; w as usize * h as usize];
        if let Some(im) = &mut images[i] {
            let uw = (w as f32 * config::USABLEWIDTH) as u32;
            let uh = (h as f32 * config::USABLEHEIGHT) as u32;
            im.draw(&mut frame, w, h, uw, uh);
        } else {
            renderer.draw_slide(&mut frame, w, h, &slides[i].lines);
        }
        let mut rgb = Vec::with_capacity(frame.len() * 3);
        for &p in &frame {
            rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&rgb)
            .map_err(|e| format!("rent: deflate: {e}"))?;
        let data = enc.finish().map_err(|e| format!("rent: deflate: {e}"))?;

        let mut img_obj = format!(
            "{image_id} 0 obj\n<< /Type /XObject /Subtype /Image /Width {w} /Height {h} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode \
             /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        img_obj.extend_from_slice(&data);
        img_obj.extend_from_slice(b"\nendstream\nendobj\n");
        objs.push(img_obj);
    }

    let mut pdf: Vec<u8> = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xC2\xB5\xC2\xB6\n");
    let mut offsets = Vec::with_capacity(objs.len());
    for o in &objs {
        offsets.push(pdf.len());
        pdf.extend_from_slice(o);
    }
    let xref_off = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_off}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );

    std::fs::write(path, &pdf).map_err(|e| format!("rent: unable to write '{path}': {e}"))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use flate2::read::ZlibDecoder;

    use super::*;

    fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
        hay[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|p| p + from)
    }

    fn count(hay: &[u8], needle: &[u8]) -> usize {
        hay.windows(needle.len()).filter(|w| *w == needle).count()
    }

    fn parse_usize_at(bytes: &[u8], mut p: usize) -> usize {
        let start = p;
        while p < bytes.len() && bytes[p].is_ascii_digit() {
            p += 1;
        }
        std::str::from_utf8(&bytes[start..p]).unwrap().parse().unwrap()
    }

    fn text_slide(s: &str) -> Slide {
        Slide { lines: vec![s.into()], embed: None }
    }

    /* export ["hello"], ["world"] to a unique temp file, return its bytes */
    fn export_two_slides(name: &str) -> Vec<u8> {
        let mut renderer = Renderer::new().expect("renderer");
        let slides = [text_slide("hello"), text_slide("world")];
        let mut images: [Option<Img>; 2] = [None, None];
        let path = std::env::temp_dir()
            .join(format!("rent-pdf-test-{}-{name}.pdf", std::process::id()));
        export(path.to_str().unwrap(), &mut renderer, &slides, &mut images)
            .expect("export");
        let bytes = std::fs::read(&path).expect("read exported pdf");
        std::fs::remove_file(&path).expect("remove exported pdf");
        bytes
    }

    #[test]
    fn export_writes_valid_pdf_skeleton() {
        let bytes = export_two_slides("skeleton");
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(count(&bytes, b"/Count 2") > 0);
        assert_eq!(count(&bytes, b"/Subtype /Image"), 2);
        assert!(count(&bytes, b"startxref") > 0);
        assert!(bytes.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn image_stream_decodes_to_frame_size() {
        let bytes = export_two_slides("stream");
        let dict = find(&bytes, b"/Subtype /Image", 0).expect("image dict");
        let lpos = find(&bytes, b"/Length ", dict).expect("/Length in image dict");
        let len = parse_usize_at(&bytes, lpos + b"/Length ".len());
        let s = find(&bytes, b"stream\n", dict).expect("stream after image dict");
        let data = &bytes[s + b"stream\n".len()..s + b"stream\n".len() + len];

        let mut out = Vec::new();
        ZlibDecoder::new(data).read_to_end(&mut out).expect("zlib decode");
        let expect = (config::EXPORTWIDTH * config::EXPORTHEIGHT * 3) as usize;
        assert_eq!(out.len(), expect);

        let white = out.iter().filter(|&&b| b == 0xFF).count();
        assert!(
            white * 2 > out.len(),
            "expected mostly white background: {white}/{}",
            out.len()
        );
        assert!(
            out.iter().any(|&b| b < 0x40),
            "expected dark ink pixels from 'hello'"
        );
    }

    #[test]
    fn export_errors_on_unwritable_path() {
        let mut renderer = Renderer::new().expect("renderer");
        let slides = [text_slide("x")];
        let mut images: [Option<Img>; 1] = [None];
        let err = export(
            "/nonexistent-dir-rent-test/out.pdf",
            &mut renderer,
            &slides,
            &mut images,
        )
        .unwrap_err();
        assert!(err.contains("unable to write"), "got: {err}");
    }

    #[test]
    fn xref_offsets_point_at_objects() {
        let bytes = export_two_slides("xref");

        /* follow startxref to the table instead of scanning for "xref" */
        let sx = find(&bytes, b"startxref\n", 0).expect("startxref");
        let xref_off = parse_usize_at(&bytes, sx + b"startxref\n".len());
        assert!(bytes[xref_off..].starts_with(b"xref\n"));

        /* subsection header "0 N\n", then N fixed 20-byte entries */
        let mut p = xref_off + b"xref\n".len();
        assert_eq!(bytes[p], b'0');
        p += 2;
        let total = parse_usize_at(&bytes, p);
        while bytes[p] != b'\n' {
            p += 1;
        }
        p += 1;

        let mut in_use = 0;
        for _ in 0..total {
            let entry = &bytes[p..p + 20];
            if entry[17] == b'n' {
                let off = parse_usize_at(entry, 0);
                let mut q = off;
                assert!(
                    bytes[q].is_ascii_digit(),
                    "offset {off} does not start an object"
                );
                while bytes[q].is_ascii_digit() {
                    q += 1;
                }
                assert_eq!(&bytes[q..q + 6], b" 0 obj", "offset {off} mislabeled");
                in_use += 1;
            }
            p += 20;
        }
        /* catalog + pages + 3 objects per slide */
        assert_eq!(in_use, 8);
    }
}
