use egui::{ClippedMesh, Context, TexturesDelta};
use egui_wgpu_backend::{BackendError, RenderPass, ScreenDescriptor};
use pixels::{wgpu, PixelsContext};
use winit::window::Window;

pub struct Gui {
    ctx: Context,
    winit_state: egui_winit::State,
    screen_descriptor: ScreenDescriptor,
    rpass: RenderPass,
    paint_jobs: Vec<ClippedMesh>,
    textures: TexturesDelta,
    state: std::sync::Arc<State>,
}

impl Gui {
    pub fn new(width: u32, height: u32, scale_factor: f32, pixels: &pixels::Pixels) -> Self {
        let max_texture_size = pixels.device().limits().max_texture_dimension_2d as usize;

        let ctx = Context::default();
        ctx.set_visuals(egui::Visuals::light());
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
        let state = std::sync::Arc::new(State::new());

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

    pub fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        self.winit_state.on_event(&self.ctx, event)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.screen_descriptor.physical_width = width;
            self.screen_descriptor.physical_height = height;
        }
    }

    pub fn prepare(&mut self, window: &Window) {
        let raw_input = self.winit_state.take_egui_input(window);
        let output = self.ctx.run(raw_input, |ctx| {
            self.state.layout(ctx);
        });

        self.textures.append(output.textures_delta);
        self.winit_state
            .handle_platform_output(window, &self.ctx, output.platform_output);
        self.paint_jobs = self.ctx.tessellate(output.shapes);
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        context: &PixelsContext,
    ) -> Result<(), BackendError> {
        self.rpass
            .add_textures(&context.device, &context.queue, &self.textures)?;
        self.rpass.update_buffers(
            &context.device,
            &context.queue,
            &self.paint_jobs,
            &self.screen_descriptor,
        );

        self.rpass.execute(
            encoder,
            render_target,
            &self.paint_jobs,
            &self.screen_descriptor,
            None,
        )?;

        let textures = std::mem::take(&mut self.textures);
        self.rpass.remove_textures(textures)
    }

    pub fn state(&self) -> std::sync::Arc<State> {
        self.state.clone()
    }
}

pub enum DialogStatus<T> {
    Pending(T),
    Ok(T),
    Cancelled,
}

pub struct State {
    link_code_state: parking_lot::Mutex<Option<DialogStatus<String>>>,
    show_debug: std::sync::atomic::AtomicBool,
    debug_status_getter: Box<dyn Fn() -> DebugStats>,
}

pub struct BattleDebugStatus {
    pub local_player_index: u8,
    pub local_qlen: usize,
    pub remote_qlen: usize,
    pub local_delay: usize,
}

pub struct DebugStats {
    pub fps: f64,
    pub target_tps: usize,
    pub battle_debug_stats: Option<BattleDebugStatus>,
}

impl State {
    fn new() -> Self {
        Self {
            link_code_state: parking_lot::Mutex::new(None),
            show_debug: true.into(),
            debug_status_getter: Box::new(|| DebugStats {
                fps: 0.0,
                target_tps: 0,
                battle_debug_stats: None,
            }),
        }
    }

    pub fn open_link_code_dialog(&self) {
        let mut maybe_link_code_state = self.link_code_state.lock();
        if maybe_link_code_state.is_some() {
            return;
        }
        *maybe_link_code_state = Some(DialogStatus::Pending(String::new()));
    }

    pub fn close_link_code_dialog(&self) {
        let mut maybe_link_code_state = self.link_code_state.lock();
        *maybe_link_code_state = None;
    }

    pub fn lock_link_code_status(&self) -> parking_lot::MutexGuard<Option<DialogStatus<String>>> {
        self.link_code_state.lock()
    }

    fn layout(&self, ctx: &Context) {
        let mut maybe_link_code_state = self.link_code_state.lock();

        if let Some(DialogStatus::Pending(code)) = &mut *maybe_link_code_state {
            if let Some(egui::InnerResponse { inner: Some((ok, cancel)), .. }) = egui::Window::new("")
                .collapsible(false)
                .title_bar(false)
                .fixed_size(egui::vec2(300.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut true)
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("お互いに接続するために、あなたと相手が決めたリンクコードを以下に入力してください。"));
                    ui.separator();
                    let response = ui.add(egui::TextEdit::singleline( code).hint_text("リンクコード"));
                    *code = code.to_lowercase().trim().to_string();
                    let text_ok = response.lost_focus() && ui.input().key_pressed(egui::Key::Enter) && !code.is_empty();
                    response.request_focus();
                    ui.separator();
                    let (button_ok, cancel) = ui.horizontal(|ui| {
                        let ok = ui.add(egui::Button::new("接続")).clicked();
                        let cancel = ui.add(egui::Button::new("キャンセル")).clicked();
                        (ok, cancel)
                    }).inner;
                    (text_ok || button_ok, cancel)
                }) {
                    if ok {
                        *maybe_link_code_state = Some(DialogStatus::Ok(code.to_string()));
                    }

                    if cancel {
                        *maybe_link_code_state = Some(DialogStatus::Cancelled);
                    }
                }
        }

        let mut show_debug = self.show_debug.load(std::sync::atomic::Ordering::SeqCst);
        egui::Window::new("Debug")
            .open(&mut show_debug)
            .show(ctx, |ui| {
                let debug_stats = (self.debug_status_getter)();
                egui::Grid::new("debug_grid").show(ui, |ui| {
                    ui.label("draw fps");
                    ui.label(format!("{:.0}", debug_stats.fps));
                    ui.end_row();

                    ui.label("target tps");
                    ui.label(format!("{}", debug_stats.target_tps));
                    ui.end_row();

                    if let Some(battle_debug_stats) = debug_stats.battle_debug_stats {
                        ui.label("local player index");
                        ui.label(format!("{:.0}", battle_debug_stats.local_player_index));
                        ui.end_row();

                        ui.label("qlen");
                        ui.label(format!(
                            "{} (-{}) : {}",
                            battle_debug_stats.local_qlen,
                            battle_debug_stats.local_delay,
                            battle_debug_stats.remote_qlen
                        ));
                        ui.end_row();
                    }
                });
            });
        self.show_debug
            .store(show_debug, std::sync::atomic::Ordering::SeqCst)
    }
}
