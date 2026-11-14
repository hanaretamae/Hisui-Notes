//! OS の UI フォント（Spec: editor-ui-m2 §2-5）。

/// OS の UI フォントのファミリー名。
///
/// - Linux: fontconfig の `sans-serif` の解決結果（`fc-match`）。取得できない場合は `None`。
/// - macOS・Windows: `None`（GPUI の既定の `.SystemUIFont` が OS のフォントに解決される）。
pub fn system_ui_font_family() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("fc-match")
            .args(["--format=%{family[0]}", "sans-serif"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_family(&out.stdout)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// `fc-match` の出力からファミリー名を取り出す。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_family(output: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(output).ok()?.trim();
    (!s.is_empty()).then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fc_match_output() {
        assert_eq!(
            parse_family(b"Noto Sans CJK JP\n").as_deref(),
            Some("Noto Sans CJK JP")
        );
        assert_eq!(parse_family(b"  \n"), None);
        assert_eq!(parse_family(b"\xff"), None);
    }
}
