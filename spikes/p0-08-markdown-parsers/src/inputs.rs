//! 計測用の文書と入力。

/// 決定的な乱数（xorshift）。
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[(self.next() % v.len() as u64) as usize]
    }
}

const WORDS: &[&str] = &[
    "Markdown",
    "note",
    "editor",
    "vault",
    "文書",
    "編集",
    "見出し",
    "リンク",
    "テキスト",
    "バッファ",
    "한국어",
    "中文",
    "ไทย",
    "हिन्दी",
    "العربية",
    "עברית",
    "👩‍💻",
    "🇯🇵",
    "e\u{301}",
    "parser",
    "表示",
    "保存",
    "検索",
];

/// Obsidianのノートを模した、標準Markdown・GFM・Obsidian拡張が混在する文書を `target` バイト以上生成する。
pub fn document(target: usize) -> String {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut s = String::from("---\ntitle: bench\ntags: [a, b]\naliases: [ベンチ]\n---\n\n");
    let mut n = 0;
    while s.len() < target {
        n += 1;
        s.push_str(&format!("## 見出し {n} {}\n\n", r.pick(WORDS)));
        for _ in 0..3 {
            for i in 0..12 {
                let w = r.pick(WORDS);
                match r.next() % 12 {
                    0 => s.push_str(&format!("**{w}**")),
                    1 => s.push_str(&format!("*{w}*")),
                    2 => s.push_str(&format!("`{w}`")),
                    3 => s.push_str(&format!("[{w}](notes/{n}.md#見出し-{i})")),
                    4 => s.push_str(&format!("[[ノート{n}#見出し|{w}]]")),
                    5 => s.push_str(&format!("#tag/{i}")),
                    6 => s.push_str(&format!("=={w}==")),
                    7 => s.push_str(&format!("$x_{i}$")),
                    _ => s.push_str(w),
                }
                s.push(' ');
            }
            s.push_str(&format!("^b{n}\n\n"));
        }
        match n % 5 {
            0 => s.push_str("- [ ] task\n- [x] done\n  - nested *item*\n\n"),
            1 => s.push_str("```rust\nfn main() { println!(\"[[not a link]]\"); }\n```\n\n"),
            2 => s.push_str("| a | b |\n|---|---|\n| 1 | [[x\\|y]] |\n| 2 | **z** |\n\n"),
            3 => s.push_str("> [!note] 題\n> 本文 *強調*\n> - 項目\n\n"),
            _ => s.push_str("1. one\n2. two\n\n> quote\n\n%% comment %%\n\n"),
        }
    }
    s
}

pub const POSITION_SAMPLE: &str = "# 題 👩‍💻\n\n本文 [日本語のリンク](ノート/a.md#見出し) と **強調**。\n\n## 見出し 🇯🇵 e\u{301}\n\n- 項目 [b](b.md)\n";

/// Obsidian構文ごとの入力（requirements/parser.md §3 と §2）。
pub const OBSIDIAN_PROBES: &[(&str, &str)] = &[
    ("obsidian.wikilink", "[[ノート#見出し|表示]] と [[a#^blk]]"),
    (
        "obsidian.embed",
        "![[画像.png|100x145]] ![[文書.pdf#page=3]]",
    ),
    ("obsidian.image-size", "![alt|100](a.png)"),
    ("obsidian.block-id", "段落 ^blk-1"),
    (
        "obsidian.callout",
        "> [!warning]- 題\n> 本文\n> > [!note]\n> > 入れ子",
    ),
    ("obsidian.tag", "#tag と #入れ子/タグ と url#frag"),
    ("obsidian.highlight", "==強調表示== です"),
    ("obsidian.comment", "前 %%隠す%% 後\n\n%%\nブロック\n%%"),
    ("obsidian.inline-footnote", "本文^[インライン脚注]"),
    ("obsidian.task-status", "- [?] 疑問\n- [/] 途中"),
    ("frontmatter.yaml", "---\ntags: [a]\n---\n本文"),
    ("math", "$x^2$ と\n\n$$\n\\int f\n$$"),
    ("footnote", "本文[^1]\n\n[^1]: 脚注"),
    // CJKの句読点に隣接する強調（Obsidianでの確認用の入力と同じ）
    ("cjk-emphasis-1", "**この文は重要です。**但这句话"),
    ("cjk-emphasis-2", "**「重要」**という語"),
    ("cjk-emphasis-3", "*強調。*次の文"),
    ("cjk-emphasis-4", "__この文は重要です。__次の文"),
    ("cjk-emphasis-5", "これは**重要**です"),
    ("cjk-emphasis-6", "**重要です。** 次の文"),
];

pub const WIKILINK_EXT_CASES: &[&str] = &[
    "前 [[ノート#見出し|表示]] 後",
    "[[x*y*z]] と `[[コード]]`",
    "**強調 [[a]]** と [[b_c_d]]",
];

/// cmark の pathological_tests.py を参考にした入力。
pub fn pathological() -> Vec<(&'static str, String)> {
    let n = 30_000;
    vec![
        (
            "nested strong emph",
            format!("{}a{}", "*a **a ".repeat(n), " a** a*".repeat(n)),
        ),
        ("many emph closers", format!("a{}", "a*".repeat(n))),
        ("many emph openers", format!("{}a", "*a".repeat(n))),
        ("many link closers", "a]".repeat(n)),
        ("many link openers", "[a".repeat(n)),
        ("mismatched openers", "*a_ ".repeat(n)),
        (
            "nested brackets",
            format!("{}a{}", "[".repeat(n), "]".repeat(n)),
        ),
        ("nested block quotes", format!("{}a", "> ".repeat(n))),
        (
            "backticks",
            (1..2000).map(|i| format!("e{}", "`".repeat(i))).collect(),
        ),
        ("unclosed links", "[a](<b".repeat(n)),
        ("wikilink openers", "[[a".repeat(n)),
        (
            "nested lists",
            (0..2000)
                .map(|i| format!("{}- a\n", "  ".repeat(i)))
                .collect(),
        ),
    ]
}
