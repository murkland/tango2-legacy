use crate::{battle, config, current_input, locales};
use egui::{ClippedMesh, Context, TexturesDelta};
use egui_wgpu_backend::{BackendError, RenderPass, ScreenDescriptor};
use fluent_templates::Loader;
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

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "NotoSans".to_owned(),
            egui::FontData::from_static(include_bytes!("fonts/NotoSans-Regular.ttf")),
        );
        fonts.font_data.insert(
            "NotoSansJP".to_owned(),
            egui::FontData::from_static(include_bytes!("fonts/NotoSansJP-Regular.otf")),
        );
        *fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap() = vec!["NotoSans".to_owned(), "NotoSansJP".to_owned()];
        ctx.set_fonts(fonts);

        let mut style = egui::Style::default();
        style.spacing.interact_size.y = 18.0;
        *style
            .text_styles
            .get_mut(&egui::TextStyle::Heading)
            .unwrap() = egui::FontId {
            size: 18.0,
            family: egui::FontFamily::Proportional,
        };
        *style.text_styles.get_mut(&egui::TextStyle::Body).unwrap() = egui::FontId {
            size: 18.0,
            family: egui::FontFamily::Proportional,
        };
        *style.text_styles.get_mut(&egui::TextStyle::Button).unwrap() = egui::FontId {
            size: 18.0,
            family: egui::FontFamily::Proportional,
        };
        *style
            .text_styles
            .get_mut(&egui::TextStyle::Monospace)
            .unwrap() = egui::FontId {
            size: 18.0,
            family: egui::FontFamily::Monospace,
        };
        *style.text_styles.get_mut(&egui::TextStyle::Small).unwrap() = egui::FontId {
            size: 16.0,
            family: egui::FontFamily::Proportional,
        };
        ctx.set_style(style);

        ctx.set_visuals(egui::Visuals::light());

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

enum ConnectDialogState {
    Cancelled,
    PendingInput(ConnectRequest),
    InputComplete(ConnectRequest),
    Negotiating {
        cancel: Box<dyn Fn()>,
        status_getter: Box<dyn Fn() -> Option<battle::NegotiationStatus>>,
    },
    None,
}

#[derive(Clone, Debug)]

pub enum DialogState<T> {
    Pending(T),
    Ok(T),
    Closed,
}

#[derive(Clone, Debug)]
pub struct ConnectRequest {
    pub code: String,
    pub replay_folder_name: std::path::PathBuf,
    pub input_delay: u32,
}

#[derive(Clone, Debug)]
pub struct ROMInfo {
    pub path: std::path::PathBuf,
    pub id: String,
}

pub struct State {
    connect_state: parking_lot::Mutex<ConnectDialogState>,
    rom_select_state: parking_lot::Mutex<DialogState<Option<usize>>>,
    show_debug: std::sync::atomic::AtomicBool,
    show_menu: std::sync::atomic::AtomicBool,
    show_keymapping_config: std::sync::atomic::AtomicBool,
    rom_list: parking_lot::Mutex<Vec<ROMInfo>>,
    debug_stats_getter: parking_lot::Mutex<Option<Box<dyn Fn() -> Option<DebugStats>>>>,
    config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
    unfiltered_current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
}

pub struct BattleDebugStats {
    pub local_player_index: u8,
    pub local_qlen: usize,
    pub remote_qlen: usize,
    pub local_delay: u32,
    pub remote_delay: u32,
    pub tps_adjustment: i32,
}

pub struct DebugStats {
    pub fps: f32,
    pub emu_tps: f32,
    pub match_state: &'static str,
    pub battle_debug_stats: Option<BattleDebugStats>,
}

fn keybinder(
    ui: &mut egui::Ui,
    current_input: &current_input::CurrentInput,
    key: &mut winit::event::VirtualKeyCode,
) -> egui::InnerResponse<bool> {
    let response = ui.add(egui::TextEdit::singleline(&mut format!("{:?}", key)).lock_focus(true));
    let mut bound = false;
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
            bound = true;
            *key = *k;
        }
    }
    egui::InnerResponse::new(bound, response)
}

pub enum ConnectStatus {
    None,
    NotReady,
    Ready(ConnectRequest),
}

impl State {
    pub fn new(
        config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
        unfiltered_current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
    ) -> Self {
        Self {
            connect_state: parking_lot::Mutex::new(ConnectDialogState::None),
            rom_select_state: parking_lot::Mutex::new(DialogState::Closed),
            show_debug: false.into(),
            show_menu: false.into(),
            show_keymapping_config: false.into(),
            rom_list: parking_lot::Mutex::new(vec![]),
            debug_stats_getter: parking_lot::Mutex::new(None),
            config,
            unfiltered_current_input,
        }
    }

    pub fn set_rom_list(&self, rom_filenames: Vec<ROMInfo>) {
        let mut rom_list = self.rom_list.lock();
        *rom_list = rom_filenames;
    }

    pub fn request_connect(
        &self,
        cancel: Box<dyn Fn()>,
        status_getter: Box<dyn Fn() -> Option<battle::NegotiationStatus>>,
    ) -> ConnectStatus {
        let mut connect_state = self.connect_state.lock();
        if let ConnectDialogState::Cancelled = &*connect_state {
            *connect_state = ConnectDialogState::None;
            return ConnectStatus::None;
        }

        if let ConnectDialogState::None = &*connect_state {
            *connect_state = ConnectDialogState::PendingInput(ConnectRequest {
                code: "".to_owned(),
                replay_folder_name: std::path::PathBuf::new(),
                input_delay: 3,
            });
        }
        let status = match &*connect_state {
            ConnectDialogState::Cancelled => {
                unreachable!()
            }
            ConnectDialogState::Negotiating { .. } | ConnectDialogState::None => {
                ConnectStatus::None
            }
            ConnectDialogState::PendingInput(_) => ConnectStatus::NotReady,
            ConnectDialogState::InputComplete(s) => ConnectStatus::Ready(s.clone()),
        };

        match status {
            ConnectStatus::None => {}
            ConnectStatus::NotReady => {}
            ConnectStatus::Ready(_) => {
                *connect_state = ConnectDialogState::Negotiating {
                    cancel,
                    status_getter,
                };
            }
        }

        status
    }

    pub fn connect_dialog_is_open(&self) -> bool {
        !matches!(&*self.connect_state.lock(), ConnectDialogState::None)
    }

    pub fn request_rom(&self) -> DialogState<Option<usize>> {
        let mut rom_select_state = self.rom_select_state.lock();
        if let DialogState::Closed = &*rom_select_state {
            *rom_select_state = DialogState::Pending(None);
        }
        let state = rom_select_state.clone();
        match *rom_select_state {
            DialogState::Pending(_) | DialogState::Closed => {}
            DialogState::Ok(_) => {
                *rom_select_state = DialogState::Closed;
            }
        }
        state
    }

    pub fn set_debug_stats_getter(&self, getter: Option<Box<dyn Fn() -> Option<DebugStats>>>) {
        *self.debug_stats_getter.lock() = getter;
    }

    pub fn toggle_menu(&self) {
        self.show_menu
            .fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn layout(&self, ctx: &Context) {
        if self.show_menu.load(std::sync::atomic::Ordering::Relaxed) {
            egui::TopBottomPanel::top("menu-bar").show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    if ui
                        .button(locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping"))
                        .clicked()
                    {
                        self.show_keymapping_config
                            .fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
                    };
                    if ui
                        .button(locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "debug"))
                        .clicked()
                    {
                        self.show_debug
                            .fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
            });
        }

        {
            let mut maybe_rom_select_state = self.rom_select_state.lock();

            let mut open = matches!(&*maybe_rom_select_state, DialogState::Pending(_));

            egui::Window::new(locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "select-game"))
                .id(egui::Id::new("select-game-window"))
                .collapsible(false)
                .title_bar(false)
                .fixed_size(egui::vec2(300.0, 200.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut open)
                .show(ctx, |ui| {
                    let rom_filenames = self.rom_list.lock();

                    let selected_index = if let DialogState::Pending(selected_index) =
                        &mut *maybe_rom_select_state
                    {
                        selected_index
                    } else {
                        unreachable!();
                    };

                    ui.label(egui::RichText::new(
                        locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "select-game.description"),
                    ));
                    egui::Frame::none()
                        .stroke(egui::Stroke::new(1.0, egui::Color32::DARK_GRAY))
                        .rounding(egui::Rounding::same(2.0))
                        .margin(egui::style::Margin::same(2.0))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for (i, rom_info) in rom_filenames.iter().enumerate() {
                                        let mut response = ui.selectable_label(
                                            *selected_index == Some(i),
                                            format!(
                                                "{}: {}",
                                                rom_info.path.to_string_lossy(),
                                                rom_info.id
                                            ),
                                        );
                                        if response.clicked() {
                                            *selected_index = Some(i);
                                            response.mark_changed();
                                        }
                                    }
                                });
                        });

                    if selected_index.is_some() {
                        *maybe_rom_select_state = DialogState::Ok(*selected_index);
                    }
                });
        }

        {
            let mut maybe_connect_state = self.connect_state.lock();

            let mut open = !matches!(&*maybe_connect_state, ConnectDialogState::None);

            egui::Window::new(locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "connect"))
                .id(egui::Id::new("connect-window"))
                .collapsible(false)
                .title_bar(false)
                .fixed_size(egui::vec2(300.0, 0.0))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut open)
                .show(ctx, |ui| match &mut *maybe_connect_state {
                    ConnectDialogState::PendingInput(s) => {
                        ui.label(
                            locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "connect.description"),
                        );

                        let response = ui.add(
                            egui::TextEdit::singleline(&mut s.code).hint_text(
                                locales::LOCALES
                                    .lookup(&locales::SYSTEM_LOCALE, "connect.input-link-code"),
                            ),
                        );
                        s.code = s.code.to_lowercase().trim().to_string();
                        if s.code.is_empty() {
                            response.request_focus();
                        }

                        ui.add(
                            egui::Slider::new(&mut s.input_delay, 3..=10).text(
                                locales::LOCALES
                                    .lookup(&locales::SYSTEM_LOCALE, "connect.input-input-delay"),
                            ),
                        );

                        let mut replay_folder_name =
                            s.replay_folder_name.to_string_lossy().to_string();
                        ui.add(
                            egui::TextEdit::singleline(&mut replay_folder_name).hint_text(
                                locales::LOCALES.lookup(
                                    &locales::SYSTEM_LOCALE,
                                    "connect.input-replay-folder-name",
                                ),
                            ),
                        );
                        s.replay_folder_name = std::path::PathBuf::from(replay_folder_name);
                        let text_ok = ui.input().key_pressed(egui::Key::Enter)
                            && !s.code.is_empty()
                            && !s.replay_folder_name.as_os_str().is_empty();

                        ui.separator();
                        let (button_ok, cancel) = ui
                            .horizontal(|ui| {
                                let ok = ui
                                    .add(egui::Button::new(
                                        locales::LOCALES
                                            .lookup(&locales::SYSTEM_LOCALE, "connect.confirm"),
                                    ))
                                    .clicked();
                                let cancel = ui
                                    .add(egui::Button::new(
                                        locales::LOCALES
                                            .lookup(&locales::SYSTEM_LOCALE, "connect.cancel"),
                                    ))
                                    .clicked();
                                (ok, cancel)
                            })
                            .inner;

                        if text_ok || button_ok {
                            *maybe_connect_state = ConnectDialogState::InputComplete(s.clone());
                        }

                        if cancel {
                            *maybe_connect_state = ConnectDialogState::Cancelled;
                        }
                    }
                    ConnectDialogState::Negotiating {
                        cancel,
                        status_getter,
                    } => match status_getter() {
                        None => {
                            *maybe_connect_state = ConnectDialogState::None;
                        }
                        Some(status) => match status {
                            battle::NegotiationStatus::Ready => {
                                *maybe_connect_state = ConnectDialogState::None;
                            }
                            battle::NegotiationStatus::NotReady(progress) => {
                                ui.horizontal(|ui| {
                                    ui.add(egui::widgets::Spinner::new());

                                    ui.label(match progress {
                                        battle::NegotiationProgress::NotStarted => locales::LOCALES
                                            .lookup(
                                                &locales::SYSTEM_LOCALE,
                                                "connect.description-not-started",
                                            ),
                                        battle::NegotiationProgress::Signalling => locales::LOCALES
                                            .lookup(
                                                &locales::SYSTEM_LOCALE,
                                                "connect.description-signalling",
                                            ),
                                        battle::NegotiationProgress::Handshaking => {
                                            locales::LOCALES.lookup(
                                                &locales::SYSTEM_LOCALE,
                                                "connect.description-handshaking",
                                            )
                                        }
                                    });
                                });

                                if ui
                                    .add(egui::Button::new(
                                        locales::LOCALES
                                            .lookup(&locales::SYSTEM_LOCALE, "connect.cancel"),
                                    ))
                                    .clicked()
                                {
                                    cancel();
                                }
                            }
                            battle::NegotiationStatus::Failed(failure) => {
                                ui.label(match failure {
                                    battle::NegotiationFailure::IncompatibleGames => {
                                        locales::LOCALES.lookup(
                                            &locales::SYSTEM_LOCALE,
                                            "connect.description-error-incompatible-games",
                                        )
                                    }
                                    battle::NegotiationFailure::MatchTypeMismatch => {
                                        locales::LOCALES.lookup(
                                            &locales::SYSTEM_LOCALE,
                                            "connect.description-error-match-type-mismatch",
                                        )
                                    }
                                    battle::NegotiationFailure::ProtocolVersionMismatch => {
                                        locales::LOCALES.lookup(
                                            &locales::SYSTEM_LOCALE,
                                            "connect.description-error-protocol-version-mismatch",
                                        )
                                    }
                                    battle::NegotiationFailure::Unknown => locales::LOCALES.lookup(
                                        &locales::SYSTEM_LOCALE,
                                        "connect.description-error-unknown",
                                    ),
                                });

                                if ui
                                    .add(egui::Button::new(
                                        locales::LOCALES.lookup(
                                            &locales::SYSTEM_LOCALE,
                                            "connect.confirm-error",
                                        ),
                                    ))
                                    .clicked()
                                {
                                    *maybe_connect_state = ConnectDialogState::None;
                                }
                            }
                        },
                    },
                    _ => {
                        return;
                    }
                });
        }

        {
            let unfiltered_current_input = self.unfiltered_current_input.clone();
            let unfiltered_current_input = unfiltered_current_input.borrow();
            let mut show_keymapping_config = self
                .show_keymapping_config
                .load(std::sync::atomic::Ordering::Relaxed);
            let mut config = self.config.lock();
            let mut bound = false;
            egui::Window::new(locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping"))
                .id(egui::Id::new("keymapping-window"))
                .open(&mut show_keymapping_config)
                .fixed_size(egui::vec2(150.0, 0.0))
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::Grid::new("keymapping-grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label(
                                locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping.up"),
                            );
                            if keybinder(ui, &*unfiltered_current_input, &mut config.keymapping.up)
                                .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping.down"),
                            );
                            if keybinder(
                                ui,
                                &*unfiltered_current_input,
                                &mut config.keymapping.down,
                            )
                            .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping.left"),
                            );
                            if keybinder(
                                ui,
                                &*unfiltered_current_input,
                                &mut config.keymapping.left,
                            )
                            .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES
                                    .lookup(&locales::SYSTEM_LOCALE, "keymapping.right"),
                            );
                            if keybinder(
                                ui,
                                &*unfiltered_current_input,
                                &mut config.keymapping.right,
                            )
                            .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping.a"),
                            );
                            if keybinder(ui, &*unfiltered_current_input, &mut config.keymapping.a)
                                .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping.b"),
                            );
                            if keybinder(ui, &*unfiltered_current_input, &mut config.keymapping.b)
                                .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping.l"),
                            );
                            if keybinder(ui, &*unfiltered_current_input, &mut config.keymapping.l)
                                .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "keymapping.r"),
                            );
                            if keybinder(ui, &*unfiltered_current_input, &mut config.keymapping.r)
                                .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES
                                    .lookup(&locales::SYSTEM_LOCALE, "keymapping.start"),
                            );
                            if keybinder(
                                ui,
                                &*unfiltered_current_input,
                                &mut config.keymapping.start,
                            )
                            .inner
                            {
                                bound = true;
                            }
                            ui.end_row();

                            ui.label(
                                locales::LOCALES
                                    .lookup(&locales::SYSTEM_LOCALE, "keymapping.select"),
                            );
                            if keybinder(
                                ui,
                                &*unfiltered_current_input,
                                &mut config.keymapping.select,
                            )
                            .inner
                            {
                                bound = true;
                            }
                            ui.end_row();
                        });
                });
            if bound {
                if let Err(e) = config::save(&*config) {
                    log::warn!("failed to save config: {}", e);
                }
            }
            self.show_keymapping_config
                .store(show_keymapping_config, std::sync::atomic::Ordering::Relaxed);
        }

        let mut show_debug = self.show_debug.load(std::sync::atomic::Ordering::Relaxed);
        egui::Window::new(locales::LOCALES.lookup(&locales::SYSTEM_LOCALE, "debug"))
            .id(egui::Id::new("debug-window"))
            .open(&mut show_debug)
            .auto_sized()
            .collapsible(false)
            .show(ctx, |ui| {
                if let Some(debug_stats_getter) = &*self.debug_stats_getter.lock() {
                    if let Some(debug_stats) = debug_stats_getter() {
                        egui::Grid::new("debug-grid").num_columns(2).show(ui, |ui| {
                            ui.label("FPS");
                            ui.label(format!("{:.0}", debug_stats.fps));
                            ui.end_row();

                            ui.label("TPS");
                            ui.label(format!("{:.0}", debug_stats.emu_tps));
                            ui.end_row();

                            ui.label("Match state");
                            ui.label(debug_stats.match_state);
                            ui.end_row();

                            if let Some(battle_debug_stats) = debug_stats.battle_debug_stats {
                                ui.label("Player index");
                                ui.label(format!("{:.0}", battle_debug_stats.local_player_index));
                                ui.end_row();

                                ui.label("TPS adjustment");
                                ui.label(format!("{:}", battle_debug_stats.tps_adjustment));
                                ui.end_row();

                                ui.label("Queue length");
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
