use std::io::BufRead;

#[derive(Debug, PartialEq, Eq)]
pub struct Slide {
    pub lines: Vec<String>,
    /* filename after a leading '@' on the first line; empty string means
     * the '@' stood alone and the slide renders as text (sent semantics) */
    pub embed: Option<String>,
}

/* Faithful port of sent's load():
 * - a paragraph (lines separated by blank lines) is one slide
 * - lines starting with '#' are ignored, even inside a paragraph
 * - '@' at the start of a slide's first line marks an image slide
 * - a leading '\' is stripped and escapes '@' and '#'
 * - zero slides is an error */
pub fn load<R: BufRead>(reader: R) -> Result<Vec<Slide>, String> {
    let mut slides = Vec::new();
    let mut it = reader.lines();

    'outer: loop {
        /* eat consecutive empty lines and comments between slides */
        let mut line = loop {
            match it.next() {
                None => break 'outer,
                Some(Err(e)) => return Err(format!("rent: read error: {e}")),
                Some(Ok(l)) => {
                    let l = strip_cr(l);
                    if !l.is_empty() && !l.starts_with('#') {
                        break l;
                    }
                }
            }
        };

        /* read one slide */
        let mut slide = Slide { lines: Vec::new(), embed: None };
        let eof = loop {
            if !line.starts_with('#') {
                if slide.lines.is_empty() && line.starts_with('@') {
                    slide.embed = Some(line[1..].to_string());
                }
                match line.strip_prefix('\\') {
                    Some(rest) => slide.lines.push(rest.to_string()),
                    None => slide.lines.push(line),
                }
            }
            match it.next() {
                None => break true,
                Some(Err(e)) => return Err(format!("rent: read error: {e}")),
                Some(Ok(next)) => {
                    let next = strip_cr(next);
                    if next.is_empty() {
                        break false;
                    }
                    line = next;
                }
            }
        };
        slides.push(slide);
        if eof {
            break;
        }
    }

    if slides.is_empty() {
        return Err("rent: no slides in file".into());
    }
    Ok(slides)
}

fn strip_cr(mut l: String) -> String {
    if l.ends_with('\r') {
        l.pop();
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(input: &str) -> Result<Vec<Slide>, String> {
        load(Cursor::new(input))
    }

    fn slides(input: &str) -> Vec<Slide> {
        parse(input).expect("input should parse into slides")
    }

    fn text_slide(lines: &[&str]) -> Slide {
        Slide { lines: lines.iter().map(|s| s.to_string()).collect(), embed: None }
    }

    #[test]
    fn one_paragraph_per_slide() {
        let got = slides("first slide\nstill first\n\nsecond slide\n");
        assert_eq!(
            got,
            vec![
                text_slide(&["first slide", "still first"]),
                text_slide(&["second slide"]),
            ]
        );
    }

    #[test]
    fn multiple_blank_lines_collapse_without_empty_slides() {
        let got = slides("a\n\n\n\n\nb\n\n\n");
        assert_eq!(got, vec![text_slide(&["a"]), text_slide(&["b"])]);
    }

    #[test]
    fn comments_between_paragraphs_are_ignored() {
        let got = slides("# a comment\na\n\n# another\n# more\n\nb\n");
        assert_eq!(got, vec![text_slide(&["a"]), text_slide(&["b"])]);
    }

    #[test]
    fn comment_inside_paragraph_does_not_split_slide() {
        let got = slides("one\n# hidden\ntwo\n");
        assert_eq!(got, vec![text_slide(&["one", "two"])]);
    }

    #[test]
    fn at_on_first_line_sets_embed_and_keeps_line() {
        let got = slides("@image.ff\ncaption\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].embed, Some("image.ff".to_string()));
        assert_eq!(got[0].lines, vec!["@image.ff", "caption"]);
    }

    #[test]
    fn at_on_non_first_line_does_not_set_embed() {
        let got = slides("title\n@not-an-image\n");
        assert_eq!(got, vec![text_slide(&["title", "@not-an-image"])]);
    }

    #[test]
    fn comment_before_at_line_still_embeds() {
        /* a comment line is skipped entirely, so the '@' line is still
         * the first stored line of the paragraph */
        let got = slides("# note\n@pic.ff\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].embed, Some("pic.ff".to_string()));
        assert_eq!(got[0].lines, vec!["@pic.ff"]);
    }

    #[test]
    fn backslash_escapes_at_and_is_stripped() {
        let got = slides("\\@foo\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].embed, None);
        assert_eq!(got[0].lines, vec!["@foo"]);
    }

    #[test]
    fn backslash_escapes_hash_and_is_stripped() {
        let got = slides("a\n\\#bar\nb\n");
        assert_eq!(got, vec![text_slide(&["a", "#bar", "b"])]);
    }

    #[test]
    fn lone_backslash_yields_empty_line_slide() {
        let got = slides("\\\n");
        assert_eq!(got, vec![text_slide(&[""])]);
    }

    #[test]
    fn bare_at_sets_empty_embed() {
        let got = slides("@\n");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].embed, Some(String::new()));
        assert_eq!(got[0].lines, vec!["@"]);
    }

    #[test]
    fn empty_input_is_error() {
        assert!(parse("").is_err());
    }

    #[test]
    fn comments_and_blanks_only_is_error() {
        assert!(parse("# only a comment\n\n\n# another\n").is_err());
        assert!(parse("\n\n\n").is_err());
    }

    #[test]
    fn crlf_line_endings_are_stripped() {
        let got = slides("a\r\nb\r\n\r\n@img.ff\r\n");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], text_slide(&["a", "b"]));
        assert_eq!(got[1].embed, Some("img.ff".to_string()));
        assert_eq!(got[1].lines, vec!["@img.ff"]);
    }

    #[test]
    fn final_paragraph_without_trailing_newline() {
        let got = slides("a\n\nlast line");
        assert_eq!(got, vec![text_slide(&["a"]), text_slide(&["last line"])]);
    }
}
