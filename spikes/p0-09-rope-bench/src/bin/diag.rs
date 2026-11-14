use unicode_segmentation::UnicodeSegmentation;
const PIECES: &[&str] = &[
    "hello ", "world ", "editor ", "rope ", "日本語", "の", "文書", "編集", "テキスト", "、", "。",
    "漢字かな交じり文", "🇯🇵", "👩‍💻", "😀", "é", "e\u{301}", "Ω", "مرحبا ", "שלום ", "한국어 ", "中文 ",
    "ไทย", "हिन्दी ",
];
fn main() {
    // Build many small ropes to find a minimal failing input.
    let mut seed = 1u64;
    let mut next = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
    for trial in 0..200_000 {
        let mut s = String::new();
        let n = 1 + (next() % 3000) as usize;
        while s.len() < n { s.push_str(PIECES[(next() % PIECES.len() as u64) as usize]); }
        let rope = crop::Rope::from(s.as_str());
        let a: Vec<String> = rope.graphemes().map(|g| g.to_string()).collect();
        let b: Vec<&str> = s.graphemes(true).collect();
        if a.len() != b.len() || a.iter().zip(&b).any(|(x, y)| x != y) {
            let i = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
            let show = |g: &str| g.chars().map(|c| format!("U+{:04X}", c as u32)).collect::<Vec<_>>().join(" ");
            println!("trial {trial}: len={} bytes; first mismatch at grapheme #{i}", s.len());
            println!("  crop:      {:?} [{}] then {:?}", a[i], show(&a[i]), a.get(i + 1));
            println!("  reference: {:?} [{}]", b[i], show(b[i]));
            // Check whether the cluster straddles a chunk boundary
            let off: usize = b[..i].iter().map(|g| g.len()).sum();
            let mut pos = 0; let mut straddles = false;
            for ch in rope.chunks() { let end = pos + ch.len(); if off < end && off + b[i].len() > end { straddles = true; } pos = end; }
            println!("  reference cluster at byte {off}, straddles a chunk boundary: {straddles}");
            return;
        }
    }
    println!("no mismatch found");
}
