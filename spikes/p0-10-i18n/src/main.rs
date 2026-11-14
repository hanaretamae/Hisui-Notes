//! P0-10: ICU4X・fluent-rsの統合の確認。製品コードではない。
//! 機能（cargo feature）ごとにビルドし、バイナリサイズの増加と動作を確認する。

#[allow(unused_imports)]
use std::hint::black_box;

fn main() {
    println!("baseline");

    #[cfg(feature = "fallback")]
    {
        use icu_locale::fallback::LocaleFallbacker;
        use icu_locale_core::Locale;
        let fb = LocaleFallbacker::new();
        for tag in ["ja-JP", "zh-Hant-HK", "pt-AO", "sr-Latn-RS", "en-GB"] {
            let loc: Locale = black_box(tag).parse().unwrap();
            let mut it = fb.for_config(Default::default()).fallback_for((&loc).into());
            let mut chain = vec![];
            loop {
                chain.push(it.get().to_string());
                if it.get().is_unknown() {
                    break;
                }
                it.step();
            }
            println!("fallback {tag}: {}", chain.join(" → "));
        }
    }

    #[cfg(feature = "segmenter")]
    {
        use icu_segmenter::{LineSegmenter, WordSegmenter, options::WordBreakInvariantOptions};
        let ws = WordSegmenter::new_auto(WordBreakInvariantOptions::default());
        for s in ["今日は良い天気です。", "ภาษาไทยสวัสดีครับ", "Hello, world! 123", "中文分词测试"] {
            let s = black_box(s);
            let b: Vec<usize> = ws.segment_str(s).collect();
            let words: Vec<&str> = b.windows(2).map(|w| &s[w[0]..w[1]]).collect();
            println!("word {s}: {words:?}");
        }
        let ls = LineSegmenter::new_auto(Default::default());
        let s = black_box("「こんにちは。」と言った。長い文章ですね");
        let b: Vec<usize> = ls.segment_str(s).collect();
        let parts: Vec<&str> = b.windows(2).map(|w| &s[w[0]..w[1]]).collect();
        println!("line (禁則) {s}: {parts:?}");
    }

    #[cfg(feature = "collator")]
    {
        use icu_collator::{Collator, options::CollatorOptions};
        use icu_locale_core::locale;
        for (name, loc) in [("ja", locale!("ja")), ("root", locale!("und"))] {
            let c = Collator::try_new(loc.into(), CollatorOptions::default()).unwrap();
            let mut v = vec!["カ", "か", "あ", "ア", "亜", "b", "A", "ä", "a"];
            v.sort_by(|a, b| c.compare(a, b));
            println!("collate {name}: {v:?}");
        }
    }

    #[cfg(feature = "normalizer")]
    {
        use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};
        let nfc = ComposingNormalizerBorrowed::new_nfc();
        let nfd = DecomposingNormalizerBorrowed::new_nfd();
        let s = black_box("e\u{301} が ｶﾞ");
        let c = nfc.normalize(s);
        let d = nfd.normalize(&c);
        println!("normalize: input {} bytes, NFC {} bytes, NFD {} bytes, NFD(NFC(x))==NFD(x): {}", s.len(), c.len(), d.len(), d == nfd.normalize(s));
    }

    #[cfg(feature = "plurals")]
    {
        use icu_locale_core::locale;
        use icu_plurals::PluralRules;
        for (name, loc) in [("en", locale!("en")), ("ru", locale!("ru")), ("ar", locale!("ar")), ("ja", locale!("ja"))] {
            let pr = PluralRules::try_new_cardinal(loc.into()).unwrap();
            let cats: Vec<String> = [0u32, 1, 2, 3, 5, 11, 21, 101].iter().map(|n| format!("{n}:{:?}", pr.category_for(*n))).collect();
            println!("plural {name}: {}", cats.join(" "));
        }
    }

    #[cfg(feature = "decimal")]
    {
        use icu_decimal::{DecimalFormatter, input::Decimal};
        use icu_locale_core::locale;
        for (name, loc) in [("ja", locale!("ja")), ("de", locale!("de")), ("ar-EG", locale!("ar-EG")), ("hi-IN", locale!("hi-IN"))] {
            let f = DecimalFormatter::try_new(loc.into(), Default::default()).unwrap();
            println!("decimal {name}: {}", f.format(&Decimal::from(black_box(1234567i64))));
        }
    }

    #[cfg(feature = "datetime")]
    {
        use icu_calendar::Date;
        use icu_datetime::{DateTimeFormatter, fieldsets::YMD};
        use icu_locale_core::locale;
        let d = Date::try_new_iso(2026, 11, 8).unwrap();
        for (name, loc) in [("ja", locale!("ja")), ("en-US", locale!("en-US")), ("ar-EG", locale!("ar-EG")), ("ja-u-ca-japanese", locale!("ja-u-ca-japanese"))] {
            let f = DateTimeFormatter::try_new(loc.into(), YMD::long()).unwrap();
            println!("date {name}: {}", f.format(&d));
        }
    }

    #[cfg(feature = "fluent")]
    {
        use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
        use fluent_langneg::{NegotiationStrategy, negotiate_languages};
        use unic_langid::{LanguageIdentifier, langid};

        let resources: &[(LanguageIdentifier, &str)] = &[
            (langid!("en-US"), "files = { $count ->\n    [one] {$count} file\n   *[other] {$count} files\n}\nonly-en = Only in English\n"),
            (langid!("ja"), "files = {$count} 個のファイル\n"),
            (langid!("ru"), "files = { $count ->\n    [one] {$count} файл\n    [few] {$count} файла\n   *[many] {$count} файлов\n}\n"),
        ];
        let available: Vec<LanguageIdentifier> = resources.iter().map(|r| r.0.clone()).collect();
        let default = langid!("en-US");
        for req in ["ja-JP", "ru", "fr-CA"] {
            let requested: Vec<LanguageIdentifier> = vec![req.parse().unwrap()];
            let chain = negotiate_languages(&requested, &available, Some(&default), NegotiationStrategy::Filtering);
            let bundles: Vec<FluentBundle<FluentResource>> = chain
                .iter()
                .map(|l| {
                    let src = resources.iter().find(|r| &r.0 == *l).unwrap().1;
                    let mut b = FluentBundle::new(vec![(*l).clone()]);
                    b.set_use_isolating(false);
                    b.add_resource(FluentResource::try_new(src.to_string()).unwrap()).unwrap();
                    b
                })
                .collect();
            let fmt = |id: &str, n: Option<i64>| -> String {
                for b in &bundles {
                    if let Some(m) = b.get_message(id) {
                        let mut args = FluentArgs::new();
                        if let Some(n) = n {
                            args.set("count", n);
                        }
                        let mut errs = vec![];
                        return format!("[{}] {}", b.locales[0], b.format_pattern(m.value().unwrap(), Some(&args), &mut errs));
                    }
                }
                format!("<missing {id}>")
            };
            let chain_s: Vec<String> = chain.iter().map(|l| l.to_string()).collect();
            println!(
                "fluent {req} (chain {}): {} | {} | {} | {}",
                chain_s.join(" → "),
                fmt("files", Some(1)),
                fmt("files", Some(3)),
                fmt("files", Some(5)),
                fmt("only-en", None)
            );
        }
    }
}
