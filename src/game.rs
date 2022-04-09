use std::sync::Arc;

use parking_lot::Mutex;

use crate::{audio, battle::Match, bn6, fastforwarder, gui, input, mgba};

const EXPECTED_FPS: u32 = 60;

struct GameState {
    r#match: Option<Match>,
}

pub struct Game {
    _rt: tokio::runtime::Runtime,
    main_core: Arc<Mutex<mgba::core::Core>>,
    trapper: mgba::trapper::Trapper,
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

        let rom_path = "bn6f.gba";

        let main_core = Arc::new(Mutex::new({
            let mut core = mgba::core::Core::new_gba("tango")?;
            core.set_audio_buffer_size(1024);

            let rom_vf = mgba::vfile::VFile::open(rom_path, mgba::vfile::flags::O_RDONLY)?;
            core.load_rom(rom_vf)?;

            let save_vf = mgba::vfile::VFile::open(
                "bn6f.sav",
                mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
            )?;
            core.load_save(save_vf)?;

            log::info!("loaded game: {}", core.game_title());
            core
        }));

        let event_loop = Some(winit::event_loop::EventLoop::new());

        let (width, height, vbuf, bn6) = {
            let core = main_core.clone();
            let mut core = core.lock();
            let (width, height) = core.desired_video_dimensions();
            let mut vbuf = vec![0u8; (width * height * 4) as usize];
            let bn6 = bn6::BN6::new(&core.game_title());
            core.set_video_buffer(&mut vbuf, width.into());
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

        let game_state = Arc::new(tokio::sync::Mutex::new(GameState { r#match: None }));

        let (stream, stream_handle) = rodio::OutputStream::try_default().unwrap();
        let audio_source = {
            let core = main_core.clone();
            audio::MGBAAudioSource::new(core, 48000)
        };
        stream_handle.play_raw(audio_source)?;

        {
            let core = main_core.clone();
            let mut core = core.lock();
            core.gba_mut()
                .sync_mut()
                .as_mut()
                .unwrap()
                .set_fps_target(60.0);
        }

        let trapper = {
            let core = main_core.clone();
            let bn6 = bn6.clone();
            let handle = handle.clone();
            let mut core = core.lock();
            mgba::trapper::Trapper::new(
                &mut core,
                vec![
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_init_call_battle_copy_input_data,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    match &game_state.r#match {
                                        None => {
                                            return;
                                        }
                                        Some(m) => {
                                            let _ = m.lock_battle_state().await.battle.as_ref().expect("attempted to get p2 battle information while no battle was active!");
                                        }
                                    };
                                    let mut core = core.lock();
                                    let r15 = core.gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_init_marshal_ret,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    let mut battle_state = r#match.lock_battle_state().await;
                                    let battle_number = battle_state.number;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get p2 battle information while no battle was active!");

                                    let mut core = core.lock();
                                    let local_init = bn6.local_marshaled_battle_state(&core);
                                    r#match.send_init(battle_number, battle.local_delay(), &local_init).await.unwrap();
                                    bn6.set_player_marshaled_battle_state(&mut core, battle.local_player_index() as u32, &local_init);

                                    let remote_init = match r#match.receive_remote_init().await {
                                        Some(remote_init) => remote_init,
                                        None => {
                                            core.gba_mut().sync_mut().unwrap().set_fps_target(EXPECTED_FPS as f32);
                                            r#match.abort();
                                            return;
                                        }
                                    };
                                    bn6.set_player_marshaled_battle_state(&mut core, battle.remote_player_index() as u32, &remote_init.marshaled.as_slice().try_into().unwrap());

                                    battle.set_remote_delay(remote_init.input_delay);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_turn_marshal_ret,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    let mut battle_state = r#match.lock_battle_state().await;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get p2 battle information while no battle was active!");

                                    let core = core.lock();
                                    let local_turn = bn6.local_marshaled_battle_state(&core);
                                    battle.add_local_pending_turn(local_turn);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        let fastforwarder = parking_lot::Mutex::new(
                            fastforwarder::Fastforwarder::new(rom_path, bn6.clone())?,
                        );
                        (
                            bn6.offsets.rom.main_read_joyflags,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    if r#match.aborted() {
                                        return;
                                    }

                                    let battle_state = &mut r#match.lock_battle_state().await;
                                    let battle_number = battle_state.number;
                                    let battle = if let Some(battle) = &mut battle_state.battle {
                                        battle
                                    } else {
                                        return;
                                    };

                                    if !battle.is_accepting_input() {
                                        return;
                                    }

                                    let mut core = core.lock();

                                    let in_battle_time = bn6.in_battle_time(&core);
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

                                    let joyflags: u16 = 0x0000; // TODO: get joyflags
                                    let local_tick = in_battle_time + battle.local_delay();
                                    let last_committed_remote_input =
                                        battle.last_committed_remote_input();
                                    let remote_tick = last_committed_remote_input.local_tick;
                                    let custom_screen_state = bn6.local_custom_screen_state(&core);
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
                                        r#match.abort();
                                        return;
                                    }

                                    r#match.send_input(battle_number, local_tick, remote_tick, joyflags, custom_screen_state, &turn).await.unwrap();

                                    let (input_pairs, left) = battle.consume_and_peek_local().await;
                                    let mut fastforwarder = fastforwarder.lock();
                                    let (committed_state, dirty_state, last_input) = fastforwarder.fastforward(battle.committed_state().as_ref().unwrap(), battle.local_player_index(), &input_pairs, battle.last_committed_remote_input(), &left).unwrap();
                                    battle.set_committed_state(committed_state);
                                    let last_joyflags = last_input[battle.local_player_index() as usize].joyflags;
                                    battle.set_last_input(last_input);

                                    let tps = EXPECTED_FPS + (remote_tick - local_tick - battle.local_delay()) - (last_committed_remote_input.remote_tick - last_committed_remote_input.local_tick - battle.remote_delay());
                                    core.gba_mut().sync_mut().unwrap().set_fps_target(tps as f32);

                                    let new_in_battle_time = bn6.in_battle_time(&core);
                                    if new_in_battle_time != in_battle_time {
                                        panic!("fastforwarder moved battle time: expected {}, got {}", in_battle_time, new_in_battle_time);
                                    }

                                    core.load_state(&dirty_state).unwrap();
                                    core.gba_mut().cpu_mut().set_gpr(4, last_joyflags as i32);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_update_call_battle_copy_input_data,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    let battle_state = &mut r#match.lock_battle_state().await;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get battle p2 information while no battle was active!");
                                    let mut core = core.lock();
                                    let r15 = core.gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                    battle.start_accepting_input();

                                    let ip = battle.take_last_input().unwrap();

                                    bn6.set_player_input_state(
                                        &mut core,
                                        0,
                                        ip[0].joyflags as u16,
                                        ip[0].custom_screen_state as u8,
                                    );
                                    if let Some(turn) = ip[0].turn {
                                        bn6.set_player_marshaled_battle_state(&mut core, 0, &turn);
                                    }
                                    bn6.set_player_input_state(
                                        &mut core,
                                        1,
                                        ip[1].joyflags as u16,
                                        ip[1].custom_screen_state as u8,
                                    );
                                    if let Some(turn) = ip[1].turn {
                                        bn6.set_player_marshaled_battle_state(&mut core, 1, &turn);
                                    }
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_run_unpaused_step_cmp_retval,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    let core = core.lock();

                                    let battle_state = &mut r#match.lock_battle_state().await;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get battle p2 information while no battle was active!");
                                    match core.gba().cpu().gpr(0) {
                                        0 => r#match.set_won_last_battle(true).await,
                                        1 => r#match.set_won_last_battle(false).await,
                                        _ => {}
                                    }
                                });
                            }),
                        )
                    },
                    {
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_start_ret,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    // TODO: call start_battle
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_ending_ret,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    // TODO: call end_battle
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.battle_is_p2_tst,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    let battle_state = r#match.lock_battle_state().await;
                                    let battle = battle_state.battle.as_ref().expect("attempted to get battle p2 information while no battle was active!");
                                    core.lock()
                                        .gba_mut()
                                        .cpu_mut()
                                        .set_gpr(0, battle.local_player_index() as i32);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.link_is_p2_ret,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    let battle_state = r#match.lock_battle_state().await;
                                    let battle = battle_state.battle.as_ref().expect("attempted to get battle p2 information while no battle was active!");
                                    core.lock()
                                        .gba_mut()
                                        .cpu_mut()
                                        .set_gpr(0, battle.local_player_index() as i32);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.get_copy_data_input_state_ret,
                            Box::new(move || {
                                handle.block_on(async {
                                    let game_state = game_state.lock().await;
                                    let r#match = if let Some(r#match) = &game_state.r#match {
                                        r#match
                                    } else {
                                        return;
                                    };

                                    let mut core = core.lock();
                                    let mut r0 = core.gba().cpu().gpr(0);
                                    if r0 != 2 {
                                        log::warn!("expected r0 to be 2 but got {}", r0);
                                        r0 = 2;
                                    }

                                    if r#match.aborted() {
                                        r0 = 4;
                                    }
                                    core.gba_mut().cpu_mut().set_gpr(0, r0);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        (
                            bn6.offsets.rom.comm_menu_handle_link_cable_input_entry,
                            Box::new(move || {
                                let core = core.lock();
                                log::warn!("unhandled call to commMenu_handleLinkCableInput at 0x{:0x}: uh oh!", core.gba().cpu().gpr(15)-4);
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets
                                .rom
                                .comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input,
                            Box::new(move || {
                                handle.block_on(async {
                                    let mut core = core.lock();
                                    let r15 = core.gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                    let game_state2 = game_state.clone();
                                    let mut game_state = game_state.lock().await;
                                    match &game_state.r#match {
                                        None => {
                                            let m = Match::new(
                                                "test".to_string(),
                                                bn6.match_type(&core),
                                                core.game_title(),
                                                core.crc32(),
                                            );
                                            game_state.r#match = Some(m);
                                            handle.spawn(async move {
                                                if let Err(e) = game_state2
                                                    .lock()
                                                    .await
                                                    .r#match
                                                    .as_ref()
                                                    .unwrap()
                                                    .run()
                                                    .await
                                                {
                                                    log::info!("match ended with {}", e);
                                                } else {
                                                    log::info!("match ended with ok");
                                                }
                                            });
                                        }
                                        Some(r#match) => match r#match.poll_for_ready().await {
                                            Ok(true) => {
                                                bn6.start_battle_from_comm_menu(&mut core);
                                                log::info!("match started");
                                            }
                                            Ok(false) => {}
                                            Err(err) => {
                                                // TODO: return the correct error.
                                                bn6.drop_matchmaking_from_comm_menu(&mut core, 0);
                                            }
                                        },
                                    };
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.comm_menu_init_battle_entry,
                            Box::new(move || {
                                handle.block_on(async {
                                    let mut core = core.lock();
                                    // TODO: get appropriate link settings and background
                                    bn6.set_link_battle_settings_and_background(&mut core, 0);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.comm_menu_wait_for_friend_ret_cancel,
                            Box::new(move || {
                                handle.block_on(async {
                                    log::info!("match canceled by user");
                                    let mut game_state = game_state.lock().await;
                                    game_state.r#match = None;
                                    let mut core = core.lock();
                                    let r15 = core.gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        let game_state = game_state.clone();
                        let handle = handle.clone();
                        (
                            bn6.offsets.rom.comm_menu_end_battle_entry,
                            Box::new(move || {
                                handle.block_on(async {
                                    // TODO: call endmatch

                                    let mut core = core.lock();
                                    let r15 = core.gba().cpu().gpr(15) as u32;
                                    core.gba_mut().cpu_mut().set_pc(r15 + 4);
                                });
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        (
                            bn6.offsets.rom.comm_menu_handle_link_cable_input_entry,
                            Box::new(move || {
                                let mut core = core.lock();
                                let r15 = core.gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);
                            }),
                        )
                    },
                    {
                        let core = main_core.clone();
                        (
                            bn6.offsets
                                .rom
                                .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                            Box::new(move || {
                                let mut core = core.lock();
                                let r15 = core.gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);
                            }),
                        )
                    },
                ],
            )
        };

        let gui = gui::Gui::new(&window, &pixels);

        let mut game = Game {
            _rt: rt,
            main_core,
            trapper,
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

                    core.set_keys(keys);

                    self.window.request_redraw();
                }
            });
    }
}
