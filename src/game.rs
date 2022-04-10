use std::sync::Arc;

use parking_lot::Mutex;

use crate::{audio, battle::Match, bn6, fastforwarder, gui, input, mgba};

const EXPECTED_FPS: u32 = 60;
enum MatchState {
    NoMatch,
    Aborted,
    Match(Match),
}

pub struct Game {
    rt: tokio::runtime::Runtime,
    main_core: Arc<Mutex<mgba::core::Core>>,
    match_state: Arc<tokio::sync::Mutex<MatchState>>,
    _trapper: mgba::trapper::Trapper,
    event_loop: Option<winit::event_loop::EventLoop<()>>,
    input: winit_input_helper::WinitInputHelper,
    vbuf: Arc<Vec<u8>>,
    vbuf2: Arc<Mutex<Vec<u8>>>,
    window: winit::window::Window,
    pixels: pixels::Pixels,
    thread: mgba::thread::Thread,
    _stream: rodio::OutputStream,
    gui: gui::Gui,
}

impl Game {
    pub fn new() -> Result<Game, Box<dyn std::error::Error>> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        let handle = rt.handle().clone();

        let rom_path = std::path::Path::new("bn6f.gba");
        let save_path = rom_path.with_extension("sav");

        let main_core = Arc::new(Mutex::new({
            let mut core = mgba::core::Core::new_gba("tango")?;
            core.as_mut().set_audio_buffer_size(1024);

            let rom_vf = mgba::vfile::VFile::open(rom_path, mgba::vfile::flags::O_RDONLY)?;
            core.as_mut().load_rom(rom_vf)?;

            let save_vf = mgba::vfile::VFile::open(
                &save_path,
                mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
            )?;
            core.as_mut().load_save(save_vf)?;

            log::info!("loaded game: {}", core.as_ref().game_title());
            core
        }));

        let event_loop = Some(winit::event_loop::EventLoop::new());

        let (width, height, vbuf, bn6) = {
            let core = main_core.clone();
            let mut core = core.lock();
            let (width, height) = core.as_ref().desired_video_dimensions();
            let mut vbuf = vec![0u8; (width * height * 4) as usize];
            let bn6 = bn6::BN6::new(&core.as_ref().game_title());
            core.as_mut().set_video_buffer(&mut vbuf, width.into());
            (width, height, Arc::new(vbuf), bn6.unwrap())
        };

        let input = winit_input_helper::WinitInputHelper::new();

        let window = {
            let size = winit::dpi::LogicalSize::new(width * 3, height * 3);
            winit::window::WindowBuilder::new()
                .with_title("tango")
                .with_inner_size(size)
                .with_min_inner_size(size)
                .build(event_loop.as_ref().unwrap())?
        };

        let vbuf2 = Arc::new(Mutex::new(vec![0u8; (width * height * 4) as usize]));

        let pixels = {
            let window_size = window.inner_size();
            let surface_texture =
                pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
            pixels::PixelsBuilder::new(width, height, surface_texture)
                .request_adapter_options(pixels::wgpu::RequestAdapterOptions {
                    power_preference: pixels::wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .build()?
        };

        let mut thread = {
            let core = main_core.clone();
            mgba::thread::Thread::new(core)
        };
        thread.start();

        let match_state = Arc::new(tokio::sync::Mutex::new(MatchState::NoMatch));

        let (stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
        let audio_source = {
            let core = main_core.clone();
            audio::MGBAAudioSource::new(core, 48000)
        };
        stream_handle.play_raw(audio_source)?;

        {
            let core = main_core.clone();
            let mut core = core.lock();
            core.as_mut()
                .gba_mut()
                .sync_mut()
                .as_mut()
                .unwrap()
                .set_fps_target(60.0);
        }

        let trapper = {
            let core = main_core.clone();
            let mut core = core.lock();
            let bn6 = bn6.clone();
            let handle = handle.clone();
            mgba::trapper::Trapper::new(
                &mut core,
                vec![
                    {
                        let match_state = match_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_init_call_battle_copy_input_data,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };

                                    core.gba_mut().cpu_mut().set_gpr(0, 0);
                                    let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                    m.lock_battle_state().await.battle.as_ref().expect("attempted to get p2 battle information while no battle was active!");
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_init_marshal_ret,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let mut match_state = match_state.lock().await;
                                    'abort: loop {
                                        let m = if let MatchState::Match(m) = &*match_state {
                                            m
                                        } else {
                                            return;
                                        };

                                        let mut battle_state = m.lock_battle_state().await;
                                        let battle_number = battle_state.number;
                                        let battle = battle_state.battle.as_mut().expect("attempted to get p2 battle information while no battle was active!");
                                        let local_init = bn6.local_marshaled_battle_state(core);
                                        m.send_init(battle_number, battle.local_delay(), &local_init).await.unwrap();
                                        log::info!("sent local init");
                                        bn6.set_player_marshaled_battle_state(core, battle.local_player_index() as u32, &local_init);

                                        let remote_init = match m.receive_remote_init().await {
                                            Some(remote_init) => remote_init,
                                            None => {
                                                core.gba_mut().sync_mut().unwrap().set_fps_target(EXPECTED_FPS as f32);
                                                break 'abort;
                                            }
                                        };
                                        log::info!("received remote init: {:?}", remote_init);
                                        bn6.set_player_marshaled_battle_state(core, battle.remote_player_index() as u32, &remote_init.marshaled.as_slice().try_into().unwrap());

                                        battle.set_remote_delay(remote_init.input_delay);
                                        return;
                                    }
                                    *match_state = MatchState::Aborted;
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_turn_marshal_ret,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };

                                    let mut battle_state = m.lock_battle_state().await;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get p2 battle information while no battle was active!");

                                    log::info!("turn data marshaled on {}", bn6.in_battle_time(core));

                                    let local_turn = bn6.local_marshaled_battle_state(core);
                                    battle.add_local_pending_turn(local_turn);
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        let fastforwarder = parking_lot::Mutex::new(
                            fastforwarder::Fastforwarder::new(rom_path, bn6.clone())?,
                        );
                        (
                            bn6.offsets.rom.main_read_joyflags,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let mut match_state = match_state.lock().await;
                                    'abort: loop {
                                        let m = if let MatchState::Match(m) = &*match_state {
                                            m
                                        } else {
                                            return;
                                        };

                                        let battle_state = &mut m.lock_battle_state().await;
                                        let battle_number = battle_state.number;
                                        let battle = if let Some(battle) = &mut battle_state.battle {
                                            battle
                                        } else {
                                            return;
                                        };

                                        if !battle.is_accepting_input() {
                                            return;
                                        }

                                        let in_battle_time = bn6.in_battle_time(core);
                                        if let None = battle.committed_state() {
                                            for i in 0..battle.local_delay() {
                                                battle
                                                    .add_input(
                                                        battle.local_player_index(),
                                                        input::Input {
                                                            local_tick: in_battle_time + i,
                                                            remote_tick: in_battle_time + i,
                                                            joyflags: 0xfc00,
                                                            custom_screen_state: 0,
                                                            turn: None,
                                                        },
                                                    )
                                                    .await;
                                            }
                                            for i in 0..battle.remote_delay() {
                                                battle
                                                    .add_input(
                                                        battle.remote_player_index(),
                                                        input::Input {
                                                            local_tick: in_battle_time + i,
                                                            remote_tick: in_battle_time + i,
                                                            joyflags: 0xfc00,
                                                            custom_screen_state: 0,
                                                            turn: None,
                                                        },
                                                    )
                                                    .await;
                                            }
                                            let committed_state = core.save_state().unwrap();
                                            battle.set_committed_state(committed_state);

                                            log::info!("battle state committed");
                                        }

                                        let joyflags: u16 = battle.local_joyflags();
                                        let local_tick = in_battle_time + battle.local_delay();
                                        let last_committed_remote_input =
                                            battle.last_committed_remote_input();
                                        let remote_tick = last_committed_remote_input.local_tick;
                                        let custom_screen_state = bn6.local_custom_screen_state(core);
                                        let turn = battle.take_local_pending_turn();

                                        const TIMEOUT: std::time::Duration =
                                            std::time::Duration::from_secs(5);
                                        if let Err(_) = tokio::time::timeout(
                                            TIMEOUT,
                                            battle.add_input(
                                                battle.local_player_index(),
                                                input::Input {
                                                    local_tick,
                                                    remote_tick,
                                                    joyflags,
                                                    custom_screen_state,
                                                    turn,
                                                },
                                            ),
                                        )
                                        .await
                                        {
                                            log::error!("could not queue local input within {:?}, dropping connection", TIMEOUT);
                                            core.gba_mut().sync_mut().unwrap().set_fps_target(EXPECTED_FPS as f32);
                                            break 'abort;
                                        }

                                        m.send_input(battle_number, local_tick, remote_tick, joyflags, custom_screen_state, &turn).await.unwrap();

                                        let (input_pairs, left) = battle.consume_and_peek_local().await;
                                        let mut fastforwarder = fastforwarder.lock();
                                        let (committed_state, dirty_state, last_input) = fastforwarder.fastforward(battle.committed_state().as_ref().unwrap(), battle.local_player_index(), &input_pairs, battle.last_committed_remote_input(), &left).unwrap();
                                        battle.set_committed_state(committed_state);
                                        let last_joyflags = last_input[battle.local_player_index() as usize].joyflags;
                                        battle.set_last_input(last_input);

                                        let tps = EXPECTED_FPS as i32 + (remote_tick as i32 - local_tick as i32 - battle.local_delay() as i32) - (last_committed_remote_input.remote_tick as i32 - last_committed_remote_input.local_tick as i32 - battle.remote_delay() as i32);
                                        core.gba_mut().sync_mut().unwrap().set_fps_target(tps as f32);

                                        let new_in_battle_time = bn6.in_battle_time(core);
                                        if new_in_battle_time != in_battle_time {
                                            panic!("fastforwarder moved battle time: expected {}, got {}", in_battle_time, new_in_battle_time);
                                        }

                                        core.load_state(&dirty_state).unwrap();
                                        core.gba_mut().cpu_mut().set_gpr(4, last_joyflags as i32);
                                        return;
                                    }
                                    *match_state = MatchState::Aborted;
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_update_call_battle_copy_input_data,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };

                                    core.gba_mut().cpu_mut().set_gpr(0, 0);
                                    let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                    let battle_state = &mut m.lock_battle_state().await;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get battle p2 information while no battle was active!");

                                    if !battle.is_accepting_input() {
                                        battle.start_accepting_input();
                                        log::info!("battle is now accepting input");
                                        return;
                                    }

                                    let ip = battle.take_last_input().unwrap();

                                    bn6.set_player_input_state(
                                        core,
                                        0,
                                        ip[0].joyflags as u16,
                                        ip[0].custom_screen_state as u8,
                                    );
                                    if let Some(turn) = ip[0].turn {
                                        bn6.set_player_marshaled_battle_state(core, 0, &turn);
                                    }
                                    bn6.set_player_input_state(
                                        core,
                                        1,
                                        ip[1].joyflags as u16,
                                        ip[1].custom_screen_state as u8,
                                    );
                                    if let Some(turn) = ip[1].turn {
                                        bn6.set_player_marshaled_battle_state(core, 1, &turn);
                                    }
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_run_unpaused_step_cmp_retval,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };


                                    let battle_state = &mut m.lock_battle_state().await;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get battle p2 information while no battle was active!");
                                    match core.as_ref().gba().cpu().gpr(0) {
                                        0 => m.set_won_last_battle(true).await,
                                        1 => m.set_won_last_battle(false).await,
                                        _ => {}
                                    }
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_start_ret,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };
                                    m.start_battle().await;
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_ending_ret,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };
                                    m.end_battle().await;
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_is_p2_tst,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };

                                    let battle_state = m.lock_battle_state().await;
                                    let battle = battle_state.battle.as_ref().expect("attempted to get battle p2 information while no battle was active!");
                                    core.gba_mut()
                                        .cpu_mut()
                                        .set_gpr(0, battle.local_player_index() as i32);
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.link_is_p2_ret,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };

                                    let battle_state = m.lock_battle_state().await;
                                    let battle = battle_state.battle.as_ref().expect("attempted to get battle p2 information while no battle was active!");
                                    core.gba_mut()
                                        .cpu_mut()
                                        .set_gpr(0, battle.local_player_index() as i32);
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.get_copy_data_input_state_ret,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let mut r0 = core.as_ref().gba().cpu().gpr(0);
                                    if r0 != 2 {
                                        log::warn!("expected r0 to be 2 but got {}", r0);
                                    }

                                    let match_state = match_state.lock().await;
                                    if let MatchState::Aborted = *match_state {
                                        r0 = 4;
                                    }

                                    core.gba_mut().cpu_mut().set_gpr(0, r0);
                                });
                            }),
                        )
                    },
                    {
                        (
                            bn6.offsets.rom.comm_menu_handle_link_cable_input_entry,
                            Box::new(move |mut core| {
                                log::warn!("unhandled call to commMenu_handleLinkCableInput at 0x{:0x}: uh oh!", core.as_ref().gba().cpu().gpr(15)-4);
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets
                                .rom
                                .comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input,
                            Box::new(move |mut core| {
                                let handle2 = handle.clone();
                                handle.block_on(async {
                                    let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                    let mut match_state = match_state.lock().await;
                                    match &*match_state {
                                        MatchState::Aborted => {
                                            todo!()
                                        }
                                        MatchState::NoMatch => {
                                            let m = Match::new(
                                                "test".to_string(),
                                                bn6.match_type(core),
                                                core.as_ref().game_title(),
                                                core.as_ref().crc32(),
                                            );
                                            *match_state = MatchState::Match(m);
                                            match &*match_state {
                                                MatchState::Match(m) => m.start(handle2),
                                                _ => unreachable!(),
                                            }
                                        }
                                        MatchState::Match(m) => match m.poll_for_ready().await {
                                            Ok(true) => {
                                                bn6.start_battle_from_comm_menu(core);
                                                log::info!("match started");
                                            }
                                            Ok(false) => {}
                                            Err(err) => {
                                                // TODO: return the correct error.
                                                bn6.drop_matchmaking_from_comm_menu(core, 0);
                                            }
                                        },
                                    };
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.comm_menu_init_battle_entry,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    // TODO: get appropriate link settings and background
                                    bn6.set_link_battle_settings_and_background(core, 0);
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.comm_menu_wait_for_friend_ret_cancel,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    log::info!("match canceled by user");
                                    let mut match_state = match_state.lock().await;
                                    *match_state = MatchState::NoMatch;
                                    let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);
                                });
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.comm_menu_end_battle_entry,
                            Box::new(move |mut core| {
                                handle.block_on(async {
                                    let mut match_state = match_state.lock().await;
                                    *match_state = MatchState::NoMatch;
                                    log::info!("match ended");
                                });
                            }),
                        )
                    },
                    {
                        (
                            bn6.offsets
                                .rom
                                .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                            Box::new(move |mut core| {
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);
                            }),
                        )
                    },
                ],
            )
        };

        let gui = gui::Gui::new(&window, &pixels);

        let mut game = Game {
            rt,
            main_core,
            match_state,
            _trapper: trapper,
            event_loop,
            input,
            window,
            pixels,
            vbuf,
            vbuf2,
            thread,
            _stream: stream,
            gui,
        };

        {
            let vbuf = Arc::clone(&game.vbuf);
            let vbuf2 = Arc::clone(&game.vbuf2);
            game.thread.set_frame_callback(Some(Box::new(move || {
                let mut vbuf2 = vbuf2.lock();
                vbuf2.copy_from_slice(&vbuf);
                for i in (0..vbuf2.len()).step_by(4) {
                    vbuf2[i + 3] = 0xff;
                }
            })));
        }

        Ok(game)
    }

    pub fn run(mut self: Self) {
        let handle = self.rt.handle().clone();
        let match_state = self.match_state.clone();

        self.event_loop
            .take()
            .unwrap()
            .run(move |event, _, control_flow| {
                *control_flow = winit::event_loop::ControlFlow::Poll;

                if let winit::event::Event::RedrawRequested(_) = event {
                    {
                        let vbuf2 = self.vbuf2.lock().clone();
                        self.pixels.get_frame().copy_from_slice(&vbuf2);
                    }

                    self.gui
                        .prepare(&self.window)
                        .expect("gui.prepare() failed");
                    self.pixels
                        .render_with(|encoder, render_target, context| {
                            context.scaling_renderer.render(encoder, render_target);
                            self.gui
                                .render(&self.window, encoder, render_target, context)?;
                            Ok(())
                        })
                        .unwrap();
                }

                self.gui.handle_event(&self.window, &event);
                if self.input.update(&event) {
                    if self.input.quit() {
                        *control_flow = winit::event_loop::ControlFlow::Exit;
                        return;
                    }

                    if let Some(size) = self.input.window_resized() {
                        self.pixels.resize_surface(size.width, size.height);
                    }

                    let mut core = self.main_core.lock();

                    let mut keys = 0u32;
                    if self.input.key_held(winit::event::VirtualKeyCode::Left) {
                        keys |= mgba::input::keys::LEFT;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Right) {
                        keys |= mgba::input::keys::RIGHT;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Up) {
                        keys |= mgba::input::keys::UP;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Down) {
                        keys |= mgba::input::keys::DOWN;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Z) {
                        keys |= mgba::input::keys::A;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::X) {
                        keys |= mgba::input::keys::B;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::A) {
                        keys |= mgba::input::keys::L;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::S) {
                        keys |= mgba::input::keys::R;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Return) {
                        keys |= mgba::input::keys::START;
                    }
                    if self.input.key_held(winit::event::VirtualKeyCode::Back) {
                        keys |= mgba::input::keys::SELECT;
                    }

                    handle.block_on(async {
                        match &*self.match_state.lock().await {
                            MatchState::Match(m) => {
                                let mut battle_state = m.lock_battle_state().await;
                                if let Some(b) = &mut battle_state.battle {
                                    b.set_local_joyflags(keys as u16 | 0xfc00);
                                } else {
                                    core.as_mut().set_keys(keys);
                                }
                            }
                            _ => {
                                core.as_mut().set_keys(keys);
                            }
                        }
                    });

                    self.window.request_redraw();
                }
            });
    }
}
