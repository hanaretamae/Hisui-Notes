//! GPUIの PlatformWindow のAndroid向け実装（検証用）。
//! Linuxのヘッドレスのウィンドウ（gpui_linux）を基に、wgpuの描画面とコールバックを加えた。

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Bounds, Capslock, DevicePixels, DispatchEventResult, GpuSpecs, Modifiers, Pixels, PlatformAtlas, PlatformDisplay,
    PlatformInput, PlatformInputHandler, PlatformWindow, Point, PromptButton, PromptLevel, RequestFrameOptions, Scene,
    Size, WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowParams, WindowVisibility,
    point, px, size,
};
use gpui_wgpu::{GpuContext, WgpuRenderer, WgpuSurfaceConfig};
use raw_window_handle as rwh;

/// wgpu に渡す ANativeWindow のハンドル。
#[derive(Debug, Clone)]
pub struct RawWindow(pub *mut c_void);
// Safety: ANativeWindow はウィンドウの存続期間中有効で、wgpu の表面の作成にのみ使う。
unsafe impl Send for RawWindow {}
unsafe impl Sync for RawWindow {}
impl rwh::HasWindowHandle for RawWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let h = rwh::AndroidNdkWindowHandle::new(NonNull::new(self.0).ok_or(rwh::HandleError::Unavailable)?);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(h.into()) })
    }
}
impl rwh::HasDisplayHandle for RawWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(rwh::AndroidDisplayHandle::new().into()) })
    }
}

pub struct WindowState {
    pub bounds: Bounds<Pixels>,
    pub scale: f32,
    pub renderer: WgpuRenderer,
    pub input_handler: Option<PlatformInputHandler>,
    pub request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    pub input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    pub resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    pub active: Option<Box<dyn FnMut(bool)>>,
    pub mouse: Point<Pixels>,
    pub title: String,
    /// IME の候補の位置（GPUI からの通知。検証では記録のみ）
    pub ime_bounds: Option<Bounds<Pixels>>,
}

#[derive(Clone)]
pub struct AndroidWindow(pub Rc<RefCell<WindowState>>);

impl AndroidWindow {
    pub fn new(
        _params: WindowParams,
        native: RawWindow,
        width_px: i32,
        height_px: i32,
        scale: f32,
        gpu: GpuContext,
    ) -> anyhow::Result<Self> {
        let config = WgpuSurfaceConfig {
            size: size(DevicePixels(width_px), DevicePixels(height_px)),
            transparent: false,
            preferred_present_mode: Some(gpui_wgpu::wgpu::PresentMode::Mailbox),
        };
        let renderer = WgpuRenderer::new(gpu, &native, config, None)?;
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(width_px as f32 / scale), px(height_px as f32 / scale)));
        Ok(Self(Rc::new(RefCell::new(WindowState {
            bounds,
            scale,
            renderer,
            input_handler: None,
            request_frame: None,
            input: None,
            resize: None,
            active: None,
            mouse: Point::default(),
            title: String::new(),
            ime_bounds: None,
        }))))
    }

    /// イベントループから描画を要求する。
    pub fn frame(&self) {
        let cb = self.0.borrow_mut().request_frame.take();
        if let Some(mut cb) = cb {
            cb(RequestFrameOptions::default());
            self.0.borrow_mut().request_frame = Some(cb);
        }
    }

    pub fn dispatch(&self, input: PlatformInput) -> bool {
        let cb = self.0.borrow_mut().input.take();
        if let Some(mut cb) = cb {
            let r = cb(input);
            self.0.borrow_mut().input = Some(cb);
            !r.propagate
        } else {
            false
        }
    }

    /// 確定した文字列を入力ハンドラーへ渡す。
    pub fn commit_text(&self, text: &str) {
        let h = self.0.borrow_mut().input_handler.take();
        if let Some(mut h) = h {
            h.replace_text_in_range(None, text);
            self.0.borrow_mut().input_handler = Some(h);
        }
    }

    /// 変換中の文字列を入力ハンドラーへ渡す。
    pub fn set_marked_text(&self, text: &str) {
        let h = self.0.borrow_mut().input_handler.take();
        if let Some(mut h) = h {
            h.replace_and_mark_text_in_range(None, text, None);
            self.0.borrow_mut().input_handler = Some(h);
        }
    }

    pub fn resized(&self, width_px: i32, height_px: i32) {
        let (sz, scale, cb) = {
            let mut s = self.0.borrow_mut();
            s.renderer.update_drawable_size(size(DevicePixels(width_px), DevicePixels(height_px)));
            let scale = s.scale;
            s.bounds.size = size(px(width_px as f32 / scale), px(height_px as f32 / scale));
            (s.bounds.size, scale, s.resize.take())
        };
        if let Some(mut cb) = cb {
            cb(sz, scale);
            self.0.borrow_mut().resize = Some(cb);
        }
    }
}

impl rwh::HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        Err(rwh::HandleError::NotSupported)
    }
}
impl rwh::HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Err(rwh::HandleError::NotSupported)
    }
}

impl PlatformWindow for AndroidWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }
    fn is_maximized(&self) -> bool {
        true
    }
    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.bounds())
    }
    fn content_size(&self) -> Size<Pixels> {
        self.bounds().size
    }
    fn resize(&mut self, _size: Size<Pixels>) {}
    fn scale_factor(&self) -> f32 {
        self.0.borrow().scale
    }
    fn appearance(&self) -> WindowAppearance {
        WindowAppearance::Light
    }
    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        None
    }
    fn mouse_position(&self) -> Point<Pixels> {
        self.0.borrow().mouse
    }
    fn modifiers(&self) -> Modifiers {
        Modifiers::default()
    }
    fn capslock(&self) -> Capslock {
        Capslock::default()
    }
    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.borrow_mut().input_handler = Some(input_handler);
    }
    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.borrow_mut().input_handler.take()
    }
    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }
    fn activate(&self) {}
    fn is_active(&self) -> bool {
        true
    }
    fn visibility(&self) -> WindowVisibility {
        WindowVisibility::Visible
    }
    fn is_hovered(&self) -> bool {
        false
    }
    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }
    fn set_title(&mut self, title: &str) {
        self.0.borrow_mut().title = title.into();
    }
    fn get_title(&self) -> String {
        self.0.borrow().title.clone()
    }
    fn set_background_appearance(&self, _background: WindowBackgroundAppearance) {}
    fn minimize(&self) {}
    fn zoom(&self) {}
    fn toggle_fullscreen(&self) {}
    fn is_fullscreen(&self) -> bool {
        true
    }
    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.borrow_mut().request_frame = Some(callback);
    }
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.borrow_mut().input = Some(callback);
    }
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.borrow_mut().active = Some(callback);
    }
    fn on_visibility_change(&self, _callback: Box<dyn FnMut(WindowVisibility)>) {}
    fn on_hover_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.borrow_mut().resize = Some(callback);
    }
    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}
    fn on_display_changed(&self, _callback: Box<dyn FnMut()>) {}
    fn on_should_close(&self, _callback: Box<dyn FnMut() -> bool>) {}
    fn on_close(&self, _callback: Box<dyn FnOnce()>) {}
    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {}
    fn on_appearance_changed(&self, _callback: Box<dyn FnMut()>) {}
    fn draw(&self, scene: &Scene) {
        self.0.borrow_mut().renderer.draw(scene);
    }
    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.borrow().renderer.sprite_atlas().clone()
    }
    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }
    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().ime_bounds = Some(bounds);
    }
    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.0.borrow().renderer.gpu_specs()
    }
}
