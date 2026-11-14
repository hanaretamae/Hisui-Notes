//! P0-09: ropey 1.6.1 vs crop 0.4.3 on a ~10 MB mixed-script document.
use std::time::{Duration, Instant};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};

// ---- deterministic PRNG (xorshift64*) ----
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&str] = &[
    "hello ", "world ", "editor ", "rope ", "日本語", "の", "文書", "編集", "テキスト", "、", "。",
    "漢字かな交じり文", "🇯🇵", "👩‍💻", "😀", "é", "e\u{301}", "Ω", "مرحبا ", "שלום ", "한국어 ", "中文 ",
    "ไทย", "हिन्दी ",
];

fn gen_text(rng: &mut Rng, target_bytes: usize) -> String {
    let mut s = String::with_capacity(target_bytes + 1024);
    while s.len() < target_bytes {
        let n = 2 + rng.below(10);
        for _ in 0..n {
            s.push_str(PIECES[rng.below(PIECES.len())]);
        }
        s.push('\n');
    }
    s
}

fn rss_kib() -> usize {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: usize = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4
}

fn t<T>(label: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let r = f();
    let d = start.elapsed();
    println!("  {label:<44} {:>10.3} ms", d.as_secs_f64() * 1e3);
    (r, d)
}

fn string_line_start(s: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    s.match_indices('\n').nth(line - 1).map(|(i, _)| i + 1).unwrap_or(s.len())
}

// ropey grapheme helper (pattern from ropey's documentation examples)
fn ropey_next_grapheme(slice: &ropey::Rope, byte_idx: usize) -> usize {
    let (mut chunk, mut chunk_byte_idx, _, _) = slice.chunk_at_byte(byte_idx);
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match gc.next_boundary(chunk, chunk_byte_idx) {
            Ok(None) => return slice.len_bytes(),
            Ok(Some(n)) => return n,
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_byte_idx += chunk.len();
                let (a, b, _, _) = slice.chunk_at_byte(chunk_byte_idx);
                chunk = a;
                chunk_byte_idx = b;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let ctx = slice.chunk_at_byte(n - 1).0;
                gc.provide_context(ctx, n - ctx.len());
            }
            _ => unreachable!(),
        }
    }
}

fn main() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let text = gen_text(&mut rng, 10 * 1024 * 1024);
    let lines = text.matches('\n').count();
    println!(
        "document: {} bytes, {} chars, {} lines, {} graphemes",
        text.len(),
        text.chars().count(),
        lines,
        text.graphemes(true).count()
    );

    // Edit script shared by both ropes: insert at line start, or delete a whole line.
    const EDITS: usize = 10_000;
    let mut script = Vec::with_capacity(EDITS);
    {
        let mut line_count = lines;
        for _ in 0..EDITS {
            if rng.below(2) == 0 {
                let piece = PIECES[rng.below(PIECES.len())];
                script.push((true, rng.below(line_count), piece));
            } else {
                script.push((false, rng.below(line_count - 1), ""));
                line_count -= 1;
            }
        }
    }

    // ---- correctness against String on a small document ----
    {
        let mut r2 = Rng(42);
        let small = gen_text(&mut r2, 200_000);
        let mut reference = small.clone();
        let mut a = ropey::Rope::from_str(&small);
        let mut b = crop::Rope::from(small.as_str());
        let mut lc = small.matches('\n').count();
        for _ in 0..2_000 {
            if r2.below(2) == 0 || lc < 3 {
                let p = PIECES[r2.below(PIECES.len())];
                let l = r2.below(lc);
                let off = string_line_start(&reference, l);
                reference.insert_str(off, p);
                a.insert(a.byte_to_char(a.line_to_byte(l)), p);
                b.insert(b.byte_of_line(l), p);
            } else {
                let l = r2.below(lc - 1);
                let (s, e) = (string_line_start(&reference, l), string_line_start(&reference, l + 1));
                reference.replace_range(s..e, "");
                let (cs, ce) = (a.line_to_char(l), a.line_to_char(l + 1));
                a.remove(cs..ce);
                b.delete(b.byte_of_line(l)..b.byte_of_line(l + 1));
                lc -= 1;
            }
        }
        assert_eq!(a.to_string(), reference, "ropey content mismatch");
        assert_eq!(b.to_string(), reference, "crop content mismatch");
        let ref_graphemes = reference.graphemes(true).count();
        let mut bi = 0;
        let mut ropey_g = 0;
        while bi < a.len_bytes() {
            bi = ropey_next_grapheme(&a, bi);
            ropey_g += 1;
        }
        assert_eq!(ropey_g, ref_graphemes, "ropey grapheme count");
        let cg = b.graphemes().count();
        if cg != ref_graphemes { println!("WARNING: crop grapheme count {cg} != reference {ref_graphemes}"); }
        let ref_u16 = reference.encode_utf16().count();
        assert_eq!(a.len_utf16_cu(), ref_u16, "ropey utf16 len");
        assert_eq!(b.utf16_len(), ref_u16, "crop utf16 len");
        // utf16 offsets at every line start agree with the reference
        for l in 0..lc {
            let off = string_line_start(&reference, l);
            let ref_u = reference[..off].encode_utf16().count();
            assert_eq!(a.char_to_utf16_cu(a.byte_to_char(off)), ref_u);
            assert_eq!(b.utf16_code_unit_of_byte(off), ref_u);
        }
        println!("correctness: OK (2000 edits, graphemes={ref_graphemes}, utf16={ref_u16})");
    }

    const Q: usize = 100_000;
    let qlines: Vec<usize> = (0..Q).map(|_| rng.below(lines - EDITS)).collect();

    let which = std::env::args().nth(1).unwrap_or_default();
    let before = rss_kib();
    if which == "ropey" {
        println!("\n== ropey 1.6.1 ==");
        let (mut r, _) = t("build from 10 MB str", || ropey::Rope::from_str(&text));
        println!("  {:<44} {:>10} KiB", "RSS delta after build", rss_kib().saturating_sub(before));
        t("10k edits (insert/delete at line start)", || {
            for &(ins, l, p) in &script {
                if ins {
                    let c = r.line_to_char(l);
                    r.insert(c, p);
                } else {
                    let (s, e) = (r.line_to_char(l), r.line_to_char(l + 1));
                    r.remove(s..e);
                }
            }
        });
        let mut worst = Duration::ZERO;
        for &l in &qlines[..1000] {
            let s = Instant::now();
            let c = r.line_to_char(l);
            r.insert(c, "あ");
            worst = worst.max(s.elapsed());
        }
        println!("  {:<44} {:>10.3} µs", "worst single insert (of 1k)", worst.as_secs_f64() * 1e6);
        t("100k line->byte + byte->line", || {
            let mut acc = 0usize;
            for &l in &qlines {
                let b = r.line_to_byte(l);
                acc = acc.wrapping_add(r.byte_to_line(b));
            }
            acc
        });
        t("100k byte->utf16 + utf16->byte", || {
            let mut acc = 0usize;
            for &l in &qlines {
                let b = r.line_to_byte(l);
                let u = r.char_to_utf16_cu(r.byte_to_char(b));
                acc = acc.wrapping_add(r.char_to_byte(r.utf16_cu_to_char(u)));
            }
            acc
        });
        t("graphemes of 10k lines (cursor walk)", || {
            let mut n = 0usize;
            for &l in &qlines[..10_000] {
                let (mut b, e) = (r.line_to_byte(l), r.line_to_byte(l + 1));
                while b < e {
                    b = ropey_next_grapheme(&r, b);
                    n += 1;
                }
            }
            n
        });
        t("1k clones + edit on each clone", || {
            let mut v = Vec::with_capacity(1000);
            for &l in &qlines[..1000] {
                let mut c = r.clone();
                c.insert(c.line_to_char(l), "x");
                v.push(c);
            }
            v.len()
        });
        println!("  {:<44} {:>10} KiB", "RSS delta at end (incl. 1k clones)", rss_kib().saturating_sub(before));
    } else {
        println!("\n== crop 0.4.3 ==");
        let (mut r, _) = t("build from 10 MB str", || crop::Rope::from(text.as_str()));
        println!("  {:<44} {:>10} KiB", "RSS delta after build", rss_kib().saturating_sub(before));
        t("10k edits (insert/delete at line start)", || {
            for &(ins, l, p) in &script {
                if ins {
                    let b = r.byte_of_line(l);
                    r.insert(b, p);
                } else {
                    let (s, e) = (r.byte_of_line(l), r.byte_of_line(l + 1));
                    r.delete(s..e);
                }
            }
        });
        let mut worst = Duration::ZERO;
        for &l in &qlines[..1000] {
            let s = Instant::now();
            let b = r.byte_of_line(l);
            r.insert(b, "あ");
            worst = worst.max(s.elapsed());
        }
        println!("  {:<44} {:>10.3} µs", "worst single insert (of 1k)", worst.as_secs_f64() * 1e6);
        t("100k line->byte + byte->line", || {
            let mut acc = 0usize;
            for &l in &qlines {
                let b = r.byte_of_line(l);
                acc = acc.wrapping_add(r.line_of_byte(b));
            }
            acc
        });
        t("100k byte->utf16 + utf16->byte", || {
            let mut acc = 0usize;
            for &l in &qlines {
                let b = r.byte_of_line(l);
                let u = r.utf16_code_unit_of_byte(b);
                acc = acc.wrapping_add(r.byte_of_utf16_code_unit(u));
            }
            acc
        });
        t("graphemes of 10k lines (iterator)", || {
            let mut n = 0usize;
            for &l in &qlines[..10_000] {
                let (s, e) = (r.byte_of_line(l), r.byte_of_line(l + 1));
                n += r.byte_slice(s..e).graphemes().count();
            }
            n
        });
        t("1k clones + edit on each clone", || {
            let mut v = Vec::with_capacity(1000);
            for &l in &qlines[..1000] {
                let mut c = r.clone();
                c.insert(c.byte_of_line(l), "x");
                v.push(c);
            }
            v.len()
        });
        println!("  {:<44} {:>10} KiB", "RSS delta at end (incl. 1k clones)", rss_kib().saturating_sub(before));
    }
}
