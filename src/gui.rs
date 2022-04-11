use crate::{config, current_input};
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
    pub fn new(
        config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
        current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
        width: u32,
        height: u32,
        scale_factor: f32,
        pixels: &pixels::Pixels,
    ) -> Self {
        let max_texture_size = pixels.device().limits().max_texture_dimension_2d as usize;

        let ctx = Context::default();
        ctx.set_visuals(egui::Visuals::light());
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "NotoSans".to_owned(),
            egui::FontData::from_static(include_bytes!("fonts/NotoSans-Regular.ttf")).tweak(
                egui::FontTweak {
                    scale: 1.25,
                    ..egui::FontTweak::default()
                },
            ),
        );
        fonts.font_data.insert(
            "NotoSansJP".to_owned(),
            egui::FontData::from_static(include_bytes!("fonts/NotoSansJP-Regular.otf")).tweak(
                egui::FontTweak {
                    scale: 1.25,
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

        Self {
            ctx,
            winit_state,
            screen_descriptor,
            rpass,
            paint_jobs: Vec::new(),
            textures,
            state: std::sync::Arc::new(State::new(config, current_input)),
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
    show_menu: std::sync::atomic::AtomicBool,
    show_keymapping_config: std::sync::atomic::AtomicBool,
    debug_stats_getter: parking_lot::Mutex<Option<Box<dyn Fn() -> Option<DebugStats>>>>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
    current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
}

pub struct BattleDebugStats {
    pub local_player_index: u8,
    pub local_qlen: usize,
    pub remote_qlen: usize,
    pub local_delay: u32,
    pub remote_delay: u32,
}

pub struct DebugStats {
    pub fps: f32,
    pub emu_tps: f32,
    pub target_tps: f32,
    pub battle_debug_stats: Option<BattleDebugStats>,
}

fn keybinder(
    ui: &mut egui::Ui,
    current_input: &current_input::CurrentInput,
    key: &mut winit::event::VirtualKeyCode,
) -> egui::Response {
    let response = ui.add(egui::TextEdit::singleline(&mut format!("{:?}", key)).lock_focus(true));
    if response.has_focus() {
        if let Some(k) = current_input
            .key_actions
            .iter()
            .flat_map(|action| {
                if let current_input::KeyAction::Pressed(k) = action {
                    vec![k]
                } else {
                    vec![]
                }
            })
            .next()
        {
            *key = *k;
        }
    }
    response
}

impl State {
    pub fn new(
        config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
        current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
    ) -> Self {
        Self {
            link_code_state: parking_lot::Mutex::new(None),
            show_debug: false.into(),
            show_menu: false.into(),
            show_keymapping_config: true.into(),
            debug_stats_getter: parking_lot::Mutex::new(None),
            config,
            current_input,
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

    pub fn open_keymapping_config_dialog(&self) {
        self.show_keymapping_config
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn close_keymapping_config_dialog(&self) {
        self.show_keymapping_config
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn lock_link_code_status(&self) -> parking_lot::MutexGuard<Option<DialogStatus<String>>> {
        self.link_code_state.lock()
    }

    pub fn set_debug_stats_getter(&self, getter: Option<Box<dyn Fn() -> Option<DebugStats>>>) {
        *self.debug_stats_getter.lock() = getter;
    }

    pub fn toggle_debug(&self) {
        self.show_debug
            .fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn toggle_menu(&self) {
        self.show_menu
            .fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn layout(&self, ctx: &Context) {
        if self.show_menu.load(std::sync::atomic::Ordering::Relaxed) {}

        {
            let mut maybe_link_code_state = self.link_code_state.lock();

            let mut open = if let Some(DialogStatus::Pending(_)) = &*maybe_link_code_state {
                true
            } else {
                false
            };

            egui::Window::new("Link Code")
                .collapsible(false)
                .title_bar(false)
                .fixed_size(egui::vec2(300.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut open)
                .show(ctx, |ui| {
                    let code = if let Some(DialogStatus::Pending(code)) = &mut *maybe_link_code_state { code } else { unreachable!(); };
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

                    if text_ok || button_ok {
                        *maybe_link_code_state = Some(DialogStatus::Ok(code.to_string()));
                    }

                    if cancel {
                        *maybe_link_code_state = Some(DialogStatus::Cancelled);
                    }
                });
        }

        {
            let current_input = self.current_input.clone();
            let current_input = current_input.borrow();
            let mut show_keymapping_config = self
                .show_keymapping_config
                .load(std::sync::atomic::Ordering::Relaxed);
            let mut config = self.config.lock();
            egui::Window::new("Keymapping")
                .open(&mut show_keymapping_config)
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::Grid::new("debug_grid").num_columns(2).show(ui, |ui| {
                        ui.label("up");
                        keybinder(ui, &*current_input, &mut config.keymapping.up);
                        ui.end_row();

                        ui.label("down");
                        keybinder(ui, &*current_input, &mut config.keymapping.down);
                        ui.end_row();

                        ui.label("left");
                        keybinder(ui, &*current_input, &mut config.keymapping.left);
                        ui.end_row();

                        ui.label("right");
                        keybinder(ui, &*current_input, &mut config.keymapping.right);
                        ui.end_row();

                        ui.label("A");
                        keybinder(ui, &*current_input, &mut config.keymapping.a);
                        ui.end_row();

                        ui.label("B");
                        keybinder(ui, &*current_input, &mut config.keymapping.b);
                        ui.end_row();

                        ui.label("L");
                        keybinder(ui, &*current_input, &mut config.keymapping.l);
                        ui.end_row();

                        ui.label("R");
                        keybinder(ui, &*current_input, &mut config.keymapping.r);
                        ui.end_row();

                        ui.label("start");
                        keybinder(ui, &*current_input, &mut config.keymapping.start);
                        ui.end_row();

                        ui.label("select");
                        keybinder(ui, &*current_input, &mut config.keymapping.select);
                        ui.end_row();
                    });
                });
            self.show_keymapping_config
                .store(show_keymapping_config, std::sync::atomic::Ordering::Relaxed);
        }

        let mut show_debug = self.show_debug.load(std::sync::atomic::Ordering::Relaxed);
        egui::Window::new("Debug")
            .open(&mut show_debug)
            .title_bar(false)
            .auto_sized()
            .show(ctx, |ui| {
                if let Some(debug_stats_getter) = &*self.debug_stats_getter.lock() {
                    if let Some(debug_stats) = debug_stats_getter() {
                        egui::Grid::new("debug_grid").num_columns(2).show(ui, |ui| {
                            ui.label("draw fps");
                            ui.label(format!("{:.0}", debug_stats.fps));
                            ui.end_row();

                            ui.label("tps");
                            ui.label(format!(
                                "{:.0} (target = {:.0})",
                                debug_stats.emu_tps, debug_stats.target_tps
                            ));
                            ui.end_row();

                            if let Some(battle_debug_stats) = debug_stats.battle_debug_stats {
                                ui.label("local player index");
                                ui.label(format!("{:.0}", battle_debug_stats.local_player_index));
                                ui.end_row();

                                ui.label("qlen");
                                ui.label(format!(
                                    "{} (-{}) vs {} (-{})",
                                    battle_debug_stats.local_qlen,
                                    battle_debug_stats.local_delay,
                                    battle_debug_stats.remote_qlen,
                                    battle_debug_stats.remote_delay,
                                ));
                                ui.end_row();
                            }
                        });
                    }
                }
            });
        self.show_debug
            .store(show_debug, std::sync::atomic::Ordering::Relaxed);
    }
}
