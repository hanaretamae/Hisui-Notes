//! P0-03: GPUIのテキスト描画（文字体系、フォールバック、原文位置との対応）の確認。製品コードではない。
//!
//! 起動すると各文字列を表示し、同時に整形（shaping）の結果を標準出力に書く。
//! `--analyze-only` を付けると、解析結果を書いた後にウィンドウを閉じて終了する。

use gpui::{
    App, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_platform::application;
use unicode_segmentation::UnicodeSegmentation;

const SAMPLES: &[(&str, &str)] = &[
    ("ascii", "Hello, world! 0123"),
    ("japanese", "日本語の文章。かな・カナ・漢字「引用」"),
    ("chinese", "中文简体 繁體中文"),
    ("korean", "한국어 문장입니다"),
    ("emoji-zwj", "👩‍💻 👨‍👩‍👧 🏳️‍🌈"),
    ("emoji-flag", "🇯🇵🇺🇸🇫🇷"),
    ("emoji-skin", "👍🏽 ✋🏿"),
    ("combining", "e\u{301} a\u{308} n\u{303} 가\u{302}"),
    ("ivs", "葛\u{E0100} 葛"),
    ("arabic", "مرحبا بالعالم"),
    ("hebrew", "שלום עולם"),
    ("bidi-mixed", "abc مرحبا 123 def"),
    ("bidi-number", "עברית 2026 ABC"),
    ("devanagari", "क्षत्रिय हिन्दी"),
    ("tamil", "தமிழ் மொழி"),
    ("thai", "ภาษาไทยสวัสดี"),
    ("math-symbols", "∑ ∫ √ ≠ ≤ ∞"),
];

struct RenderTest;

impl Render for RenderTest {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().gap_1().p_4().bg(rgb(0xffffff)).text_color(rgb(0x000000)).size_full();
        for (label, text) in SAMPLES {
            col = col.child(
                div()
                    .flex()
                    .gap_4()
                    .child(div().w(px(130.0)).text_sm().text_color(rgb(0x666666)).child(*label))
                    .child(div().text_xl().child(*text)),
            );
        }
        col
    }
}

/// 整形結果の解析。
fn analyze(window: &mut Window) {
    let style = window.text_style();
    let ts = window.text_system().clone();
    println!(
        "{:<13} {:>5} {:>5} {:>6} {:>7} {:<22} {:<10} {:<9} fonts",
        "sample", "bytes", "graph", "glyphs", "notdef", "glyph order (visual)", "roundtrip", "x-monot."
    );
    for (label, text) in SAMPLES {
        let run = style.to_run(text.len());
        let line = ts.shape_line(SharedString::from(*text), px(20.0), &[run], None);
        let mut glyphs: Vec<(f32, usize)> = vec![];
        let mut notdef = 0;
        let mut fonts: Vec<String> = vec![];
        for r in &line.runs {
            let name = ts.get_font_for_id(r.font_id).map(|f| f.family.to_string()).unwrap_or("?".into());
            if !fonts.contains(&name) {
                fonts.push(name);
            }
            for g in &r.glyphs {
                if g.id.0 == 0 {
                    notdef += 1;
                }
                glyphs.push((f32::from(g.position.x), g.index));
            }
        }
        // 表示上の左から右への順に並べたときの原文位置の順序
        let mut by_x = glyphs.clone();
        by_x.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let idx: Vec<usize> = by_x.iter().map(|g| g.1).collect();
        let asc = idx.windows(2).all(|w| w[0] <= w[1]);
        let desc = idx.windows(2).all(|w| w[0] >= w[1]);
        let order = if asc {
            "LTR (index ascending)"
        } else if desc {
            "RTL (index descending)"
        } else {
            "mixed"
        };
        // 書記素境界ごとの x_for_index → closest_index_for_x の往復
        let bounds: Vec<usize> = text.grapheme_indices(true).map(|(i, _)| i).chain([text.len()]).collect();
        let mut ok = 0;
        let mut fails = vec![];
        let mut xs = vec![];
        for &b in &bounds {
            let x = line.x_for_index(b);
            xs.push(f32::from(x));
            let back = line.closest_index_for_x(x);
            if back == b {
                ok += 1;
            } else {
                fails.push((b, back));
            }
        }
        let x_monotone = if xs.windows(2).all(|w| w[0] <= w[1]) { "ascending" } else { "no" };
        println!(
            "{:<13} {:>5} {:>5} {:>6} {:>7} {:<22} {:<10} {:<9} {}",
            label,
            text.len(),
            bounds.len() - 1,
            glyphs.len(),
            notdef,
            order,
            format!("{ok}/{}", bounds.len()),
            x_monotone,
            fonts.join(", ")
        );
        if !fails.is_empty() {
            println!("    roundtrip failures (index -> back): {fails:?}");
        }
        if *label == "bidi-mixed" || *label == "arabic" || *label == "bidi-number" {
            println!("    visual index order: {idx:?}");
            println!("    x_for_index at grapheme bounds: {:?}", xs.iter().map(|x| x.round() as i32).collect::<Vec<_>>());
        }
    }
}

fn main() {
    let analyze_only = std::env::args().any(|a| a == "--analyze-only");
    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            move |window, cx| {
                analyze(window);
                if analyze_only {
                    cx.defer(|cx| cx.quit());
                }
                cx.new(|_| RenderTest)
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
