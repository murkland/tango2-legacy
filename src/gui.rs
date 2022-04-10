use egui::{ClippedMesh, Context, TexturesDelta};
use egui_wgpu_backend::{BackendError, RenderPass, ScreenDescriptor};
use pixels::{wgpu, PixelsContext};
use winit::window::Window;

/// Manages all state required for rendering egui over `Pixels`.
pub struct Gui {
    // State for egui.
    ctx: Context,
    winit_state: egui_winit::State,
    screen_descriptor: ScreenDescriptor,
    rpass: RenderPass,
    paint_jobs: Vec<ClippedMesh>,
    textures: TexturesDelta,

    // State for the GUI
    state: State,
}

impl Gui {
    /// Create egui.
    pub fn new(width: u32, height: u32, scale_factor: f32, pixels: &pixels::Pixels) -> Self {
        let max_texture_size = pixels.device().limits().max_texture_dimension_2d as usize;

        let ctx = Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "NotoSans".to_owned(),
            egui::FontData::from_static(include_bytes!("fonts/NotoSans-Regular.ttf")).tweak(
                egui::FontTweak {
                    scale: 1.5,
                    ..egui::FontTweak::default()
                },
            ),
        );
        fonts.font_data.insert(
            "NotoSansJP".to_owned(),
            egui::FontData::from_static(include_bytes!("fonts/NotoSansJP-Regular.otf")).tweak(
                egui::FontTweak {
                    scale: 1.5,
                    ..egui::FontTweak::default()
                },
            ),
        );
        *fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap() = vec!["NotoSans".to_owned(), "NotoSansJP".to_owned()];
        ctx.set_fonts(fonts);

        let winit_state = egui_winit::State::from_pixels_per_point(max_texture_size, scale_factor);
        let screen_descriptor = ScreenDescriptor {
            physical_width: width,
            physical_height: height,
            scale_factor,
        };
        let rpass = RenderPass::new(pixels.device(), pixels.render_texture_format(), 1);
        let textures = TexturesDelta::default();
        let state = State::new();

        Self {
            ctx,
            winit_state,
            screen_descriptor,
            rpass,
            paint_jobs: Vec::new(),
            textures,
            state,
        }
    }

    /// Handle input events from the window manager.
    pub fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        self.winit_state.on_event(&self.ctx, event)
    }

    /// Resize egui.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.screen_descriptor.physical_width = width;
            self.screen_descriptor.physical_height = height;
        }
    }

    /// Update scaling factor.
    pub fn scale_factor(&mut self, scale_factor: f64) {
        self.screen_descriptor.scale_factor = scale_factor as f32;
    }

    /// Prepare egui.
    pub fn prepare(&mut self, window: &Window) {
        // Run the egui frame and create all paint jobs to prepare for rendering.
        let raw_input = self.winit_state.take_egui_input(window);
        let output = self.ctx.run(raw_input, |ctx| {
            // Draw the demo application.
            self.state.layout(ctx);
        });

        self.textures.append(output.textures_delta);
        self.winit_state
            .handle_platform_output(window, &self.ctx, output.platform_output);
        self.paint_jobs = self.ctx.tessellate(output.shapes);
    }

    /// Render egui.
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        context: &PixelsContext,
    ) -> Result<(), BackendError> {
        // Upload all resources to the GPU.
        self.rpass
            .add_textures(&context.device, &context.queue, &self.textures)?;
        self.rpass.update_buffers(
            &context.device,
            &context.queue,
            &self.paint_jobs,
            &self.screen_descriptor,
        );

        // Record all render passes.
        self.rpass.execute(
            encoder,
            render_target,
            &self.paint_jobs,
            &self.screen_descriptor,
            None,
        )?;

        // Cleanup
        let textures = std::mem::take(&mut self.textures);
        self.rpass.remove_textures(textures)
    }
}

struct State {}

impl State {
    fn new() -> Self {
        Self {}
    }

    fn layout(&mut self, ctx: &Context) {
        egui::Window::new("")
            .open(&mut true)
            .collapsible(false)
            .title_bar(false)
            .fixed_size(egui::vec2(300.0, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("お互いに接続するために、あなたと相手が決めたリンクコードを以下に入力してください。"));
                ui.separator();
                let mut code = String::new();
                ui.add(egui::TextEdit::singleline(&mut code).hint_text("リンクコード"));
                let mut delay = 3u32;
                ui.add(egui::Slider::new(&mut delay, 3..=10).text("入力遅延").clamp_to_range(true).suffix("f"));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(egui::Button::new("接続"));
                    ui.add(egui::Button::new("キャンセル"));
                })
            });
    }
}
