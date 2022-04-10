use pixels::{wgpu, PixelsContext};
use std::time::Instant;

/// Manages all state required for rendering Dear ImGui over `Pixels`.
pub struct Gui {
    imgui: imgui::Context,
    platform: imgui_winit_support::WinitPlatform,
    renderer: imgui_wgpu::Renderer,
    last_frame: Instant,
    last_cursor: Option<imgui::MouseCursor>,
    state: State,
}

pub struct State {}

impl State {
    fn new() -> Self {
        State {}
    }

    fn layout(&mut self, ui: &imgui::Ui) {
        imgui::Window::new("link code")
            .position(
                [ui.io().display_size[0] / 2.0, ui.io().display_size[1] / 2.0],
                imgui::Condition::Always,
            )
            .position_pivot([0.5, 0.5])
            .size([300.0, 0.0], imgui::Condition::Always)
            .no_decoration()
            .build(ui, || {
                let mut buf = String::new();
                ui.text_wrapped("お互いに接続するために、あなたと相手が決めたリンクコードを以下に入力してください。");
                ui.input_text("コード", &mut buf).enter_returns_true(true).chars_noblank(true).build();
                let mut input_delay = 3u32;
                imgui::Slider::new("入力遅延", 3u32, 10u32).display_format("%df").build(ui, &mut input_delay);
                ui.button("接続");
                ui.same_line();
                ui.button("キャンセル");
            });
    }
}

impl Gui {
    /// Create Dear ImGui.
    pub fn new(window: &winit::window::Window, pixels: &pixels::Pixels) -> Self {
        // Create Dear ImGui context
        let mut imgui = imgui::Context::create();
        imgui.set_ini_filename(None);

        // Initialize winit platform support
        let mut platform = imgui_winit_support::WinitPlatform::init(&mut imgui);
        platform.attach_window(
            imgui.io_mut(),
            window,
            imgui_winit_support::HiDpiMode::Default,
        );

        // Configure Dear ImGui fonts
        let hidpi_factor = window.scale_factor();
        let font_size = (20.0 * hidpi_factor) as f32;
        imgui.io_mut().font_global_scale = (1.0 / hidpi_factor) as f32;
        imgui.fonts().add_font(&[
            imgui::FontSource::TtfData {
                data: include_bytes!("fonts/NotoSans-Regular.ttf"),
                size_pixels: font_size,
                config: Some(imgui::FontConfig {
                    oversample_h: 4,
                    oversample_v: 4,
                    ..imgui::FontConfig::default()
                }),
            },
            imgui::FontSource::TtfData {
                data: include_bytes!("fonts/NotoSansJP-Regular.otf"),
                size_pixels: font_size,
                config: Some(imgui::FontConfig {
                    oversample_h: 4,
                    oversample_v: 4,
                    glyph_ranges: imgui::FontGlyphRanges::japanese(),
                    ..imgui::FontConfig::default()
                }),
            },
        ]);

        // Create Dear ImGui WGPU renderer
        let device = pixels.device();
        let queue = pixels.queue();
        let config = imgui_wgpu::RendererConfig {
            texture_format: pixels.render_texture_format(),
            ..Default::default()
        };
        let renderer = imgui_wgpu::Renderer::new(&mut imgui, device, queue, config);

        // Return GUI context
        Self {
            imgui,
            platform,
            renderer,
            last_frame: Instant::now(),
            last_cursor: None,
            state: State::new(),
        }
    }

    /// Prepare Dear ImGui.
    pub fn prepare(
        &mut self,
        window: &winit::window::Window,
    ) -> Result<(), winit::error::ExternalError> {
        // Prepare Dear ImGui
        let now = Instant::now();
        self.imgui.io_mut().update_delta_time(now - self.last_frame);
        self.last_frame = now;
        self.platform.prepare_frame(self.imgui.io_mut(), window)
    }

    /// Render Dear ImGui.
    pub fn render(
        &mut self,
        window: &winit::window::Window,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        context: &PixelsContext,
    ) -> imgui_wgpu::RendererResult<()> {
        // Start a new Dear ImGui frame and update the cursor
        let mut ui = self.imgui.frame();

        let mouse_cursor = ui.mouse_cursor();
        if self.last_cursor != mouse_cursor {
            self.last_cursor = mouse_cursor;
            self.platform.prepare_render(&ui, window);
        }

        self.state.layout(&mut ui);

        // Render Dear ImGui with WGPU
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("imgui"),
            color_attachments: &[wgpu::RenderPassColorAttachment {
                view: render_target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        });

        self.renderer
            .render(ui.render(), &context.queue, &context.device, &mut rpass)
    }

    /// Handle any outstanding events.
    pub fn handle_event(
        &mut self,
        window: &winit::window::Window,
        event: &winit::event::Event<()>,
    ) {
        self.platform
            .handle_event(self.imgui.io_mut(), window, event);
    }
}
