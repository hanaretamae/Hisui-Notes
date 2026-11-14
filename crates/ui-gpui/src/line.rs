//! 1つの論理行を折り返して描画する要素（Spec: editor-ui-m2 §4.1）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    App, AvailableSpace, Bounds, ElementId, GlobalElementId, Hsla, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, SharedString, Style, TextRun, UnderlineStyle, Window, WrappedLine,
    fill, point, prelude::*, px, size,
};

use crate::display::DisplayLine;

/// 描画した行の組版の情報。上下の移動・クリック・IME の候補ウィンドウの位置に使う。
pub struct PaintedLine {
    pub line: DisplayLine,
    /// ウィンドウ座標での左上。
    pub origin: Point<Pixels>,
    pub layout: Option<WrappedLine>,
    pub height: Pixels,
    generation: u64,
}

/// 直近のフレームで描画した行（論理行の番号 → 組版）。
#[derive(Default)]
pub struct LayoutCache {
    pub lines: HashMap<usize, PaintedLine>,
    generation: u64,
}

impl LayoutCache {
    /// フレームの終わりに呼ぶ。このフレームで描画しなかった行を取り除く。
    pub fn finish_frame(&mut self) {
        let g = self.generation;
        self.lines.retain(|_, l| l.generation == g);
        self.generation += 1;
    }
}

impl PaintedLine {
    /// 表示上の位置の、行の左上からの座標。
    pub fn position(&self, d: usize, line_height: Pixels) -> Point<Pixels> {
        self.layout
            .as_ref()
            .and_then(|l| l.position_for_index(d, line_height))
            .unwrap_or_default()
    }

    /// 行の左上からの座標に最も近い表示上の位置。
    pub fn index_at(&self, p: Point<Pixels>, line_height: Pixels) -> usize {
        match &self.layout {
            Some(l) => l
                .closest_index_for_position(p, line_height)
                .unwrap_or_else(|i| i)
                .min(self.line.text.len()),
            None => 0,
        }
    }
}

/// 表示の色と寸法。
#[derive(Clone, Copy)]
pub struct LineStyle {
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub text: Hsla,
    pub selection: Hsla,
    pub cursor: Hsla,
    pub focused: bool,
}

pub struct LineElement {
    pub index: usize,
    pub line: DisplayLine,
    pub style: LineStyle,
    pub cache: Rc<RefCell<LayoutCache>>,
}

fn shape(
    line: &DisplayLine,
    style: &LineStyle,
    width: Option<Pixels>,
    window: &mut Window,
) -> Option<WrappedLine> {
    let text: SharedString = line.text.clone().into();
    let font = window.text_style().font();
    let run = TextRun {
        len: 0,
        font,
        color: style.text,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs: Vec<TextRun> = match line.composition {
        Some((cs, len, _)) => [
            (cs, None),
            (
                len,
                Some(UnderlineStyle {
                    color: Some(style.text),
                    thickness: px(1.),
                    wavy: false,
                }),
            ),
            (text.len() - cs - len, None),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(len, underline)| TextRun {
            len,
            underline,
            ..run.clone()
        })
        .collect(),
        None => vec![TextRun {
            len: text.len(),
            ..run
        }],
    };
    window
        .text_system()
        .shape_text(text, style.font_size, &runs, width, None)
        .ok()?
        .into_iter()
        .next()
}

fn height_of(layout: Option<&WrappedLine>, style: &LineStyle) -> Pixels {
    let rows = layout.map_or(1, |l| l.wrap_boundaries().len() + 1);
    style.line_height * rows as f32
}

impl IntoElement for LineElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct LinePrepaint {
    layout: Option<WrappedLine>,
}

impl Element for LineElement {
    type RequestLayoutState = ();
    type PrepaintState = LinePrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.).into();
        if self.line.hidden {
            style.size.height = px(0.).into();
            return (window.request_layout(style, [], cx), ());
        }
        let line = self.line.clone();
        let line_style = self.style;
        let id = window.request_measured_layout(style, move |known, available, window, _cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let layout = shape(&line, &line_style, width, window);
            size(
                width.unwrap_or_default(),
                height_of(layout.as_ref(), &line_style),
            )
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> LinePrepaint {
        let layout = if self.line.hidden {
            None
        } else {
            shape(&self.line, &self.style, Some(bounds.size.width), window)
        };
        LinePrepaint { layout }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut LinePrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let lh = self.style.line_height;
        let layout = prepaint.layout.take();
        if let Some(layout) = &layout {
            let pos = |d: usize| layout.position_for_index(d, lh).unwrap_or_default();
            for (range, continues) in &self.line.selections {
                let (a, b) = (pos(range.start), pos(range.end));
                let right = bounds.size.width;
                let mut rects = Vec::new();
                if a.y == b.y {
                    let end = if *continues {
                        right.min(b.x + px(8.))
                    } else {
                        b.x
                    };
                    rects.push((a.x, a.y, end));
                } else {
                    rects.push((a.x, a.y, right));
                    let mut y = a.y + lh;
                    while y < b.y {
                        rects.push((px(0.), y, right));
                        y += lh;
                    }
                    rects.push((px(0.), b.y, if *continues { right } else { b.x }));
                }
                for (x0, y, x1) in rects {
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin + point(x0, y),
                            size((x1 - x0).max(px(0.)), lh),
                        ),
                        self.style.selection,
                    ));
                }
            }
            let _ = layout.paint(bounds.origin, lh, gpui::TextAlign::Left, None, window, cx);
            if self.style.focused {
                for &c in &self.line.cursors {
                    let p = pos(c);
                    window.paint_quad(fill(
                        Bounds::new(bounds.origin + p, size(px(2.), lh)),
                        self.style.cursor,
                    ));
                }
            }
        }
        let mut cache = self.cache.borrow_mut();
        let generation = cache.generation;
        cache.lines.insert(
            self.index,
            PaintedLine {
                line: self.line.clone(),
                origin: bounds.origin,
                layout,
                height: bounds.size.height,
                generation,
            },
        );
    }
}
