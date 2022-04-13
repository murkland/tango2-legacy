use crate::{audio, battle, bn6, config, current_input, fastforwarder, gui, input, mgba, tps};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use std::sync::Arc;

const EXPECTED_FPS: u32 = 60;
enum MatchState {
    NoMatch,
    Aborted,
    Match(battle::Match),
}

struct GameState {
    main_core: Arc<Mutex<mgba::core::Core>>,
    match_state: Arc<tokio::sync::Mutex<MatchState>>,
    _trapper: mgba::trapper::Trapper,
    _thread: mgba::thread::Thread,
    _stream: cpal::Stream,
}

pub struct Game {
    rt: tokio::runtime::Runtime,
    fps_counter: Arc<Mutex<tps::Counter>>,
    event_loop: Option<winit::event_loop::EventLoop<()>>,
    audio_device: cpal::Device,
    window: winit::window::Window,
    pixels: pixels::Pixels,
    gui: gui::Gui,
    config: Arc<Mutex<config::Config>>,
    vbuf: Arc<Mutex<Vec<u8>>>,
    current_input: std::rc::Rc<std::cell::RefCell<current_input::CurrentInput>>,
    game_state: Arc<Mutex<Option<GameState>>>,
    emu_tps_counter: Arc<Mutex<tps::Counter>>,
}

impl GameState {
    fn new(
        rom_filename: &std::path::Path,
        save_filename: &std::path::Path,
        handle: tokio::runtime::Handle,
        audio_device: &cpal::Device,
        config: Arc<Mutex<config::Config>>,
        gui_state: std::sync::Weak<gui::State>,
        vbuf: std::sync::Weak<Mutex<Vec<u8>>>,
        emu_tps_counter: std::sync::Weak<Mutex<tps::Counter>>,
    ) -> Result<Self, anyhow::Error> {
        let roms_path = std::path::Path::new("roms");
        let saves_path = std::path::Path::new("saves");

        let rom_path = roms_path.join(&rom_filename);
        let save_path = saves_path.join(&save_filename);

        let main_core = Arc::new(Mutex::new({
            let mut core = mgba::core::Core::new_gba("tango")?;
            core.enable_video_buffer();

            let rom_vf = mgba::vfile::VFile::open(&rom_path, mgba::vfile::flags::O_RDONLY)?;
            core.as_mut().load_rom(rom_vf)?;

            let save_vf = mgba::vfile::VFile::open(
                &save_path,
                mgba::vfile::flags::O_CREAT | mgba::vfile::flags::O_RDWR,
            )?;
            core.as_mut().load_save(save_vf)?;

            log::info!("loaded game: {}", core.as_ref().game_title());
            core
        }));

        let bn6 = {
            let core = main_core.clone();
            let core = core.lock();
            bn6::BN6::new(&core.as_ref().game_title()).unwrap()
        };

        let match_state = Arc::new(tokio::sync::Mutex::new(MatchState::NoMatch));

        let mut thread = {
            let core = main_core.clone();
            mgba::thread::Thread::new(core)
        };
        thread.start();

        let stream = {
            let core = main_core.clone();
            audio::open_mgba_audio_stream(core, audio_device)?
        };
        stream.play()?;

        {
            let core = main_core.clone();
            let mut core = core.lock();
            core.as_mut()
                .gba_mut()
                .sync_mut()
                .as_mut()
                .expect("sync")
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

                                        let replay_writer = battle.replay_writer().upgrade().expect("upgrade");
                                        let mut replay_writer = replay_writer.lock();

                                        let local_init = bn6.local_marshaled_battle_state(core);
                                        m.send_init(battle_number, battle.local_delay(), &local_init).await.expect("send init");
                                        log::info!("sent local init");
                                        bn6.set_player_marshaled_battle_state(core, battle.local_player_index() as u32, local_init.as_slice());
                                        replay_writer.write_init(battle.local_player_index(), local_init.as_slice()).expect("write local init");

                                        let remote_init = match m.receive_remote_init().await {
                                            Some(remote_init) => remote_init,
                                            None => {
                                                core.gba_mut().sync_mut().expect("sync").set_fps_target(EXPECTED_FPS as f32);
                                                break 'abort;
                                            }
                                        };
                                        log::info!("received remote init: {:?}", remote_init);
                                        bn6.set_player_marshaled_battle_state(core, battle.remote_player_index() as u32, remote_init.marshaled.as_slice());
                                        replay_writer.write_init(battle.remote_player_index(), remote_init.marshaled.as_slice()).expect("write remote init");

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
                            Box::new(move |core| {
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
                            fastforwarder::Fastforwarder::new(&rom_path, bn6.clone())?,
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
                                                    .add_local_input(
                                                        input::Input {
                                                            local_tick: in_battle_time + i,
                                                            remote_tick: in_battle_time + i,
                                                            joyflags: 0xfc00,
                                                            custom_screen_state: 0,
                                                            turn: vec![],
                                                        },
                                                    )
                                                    .await;
                                            }
                                            for i in 0..battle.remote_delay() {
                                                battle
                                                    .add_remote_input(
                                                        input::Input {
                                                            local_tick: in_battle_time + i,
                                                            remote_tick: in_battle_time + i,
                                                            joyflags: 0xfc00,
                                                            custom_screen_state: 0,
                                                            turn: vec![],
                                                        },
                                                    )
                                                    .await;
                                            }
                                            let committed_state = core.save_state().expect("save committed state");

                                            let replay_writer = battle.replay_writer().upgrade().expect("upgrade");
                                            let mut replay_writer = replay_writer.lock();
                                            replay_writer.write_state(&committed_state).expect("write state");

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
                                            battle.add_local_input(
                                                input::Input {
                                                    local_tick,
                                                    remote_tick,
                                                    joyflags,
                                                    custom_screen_state,
                                                    turn: turn.clone(),
                                                },
                                            ),
                                        )
                                        .await
                                        {
                                            log::error!("could not queue local input within {:?}, dropping connection", TIMEOUT);
                                            core.gba_mut().sync_mut().expect("sync").set_fps_target(EXPECTED_FPS as f32);
                                            break 'abort;
                                        }

                                        m.send_input(battle_number, local_tick, remote_tick, joyflags, custom_screen_state, turn).await.expect("send input");

                                        let (input_pairs, left) = battle.consume_and_peek_local().await;
                                        let mut fastforwarder = fastforwarder.lock();
                                        let (committed_state, dirty_state, last_input) = fastforwarder.fastforward(
                                            battle.committed_state().as_ref().expect("committed state"),
                                            battle.local_player_index(),
                                            &input_pairs,
                                            battle.last_committed_remote_input(),
                                            &left,
                                            battle.replay_writer()
                                        ).expect("fastforward");
                                        battle.set_committed_state(committed_state);
                                        let last_joyflags = last_input.remote.joyflags;
                                        battle.set_last_input(last_input);

                                        let tps = EXPECTED_FPS as i32 + (remote_tick as i32 - local_tick as i32 - battle.local_delay() as i32) - (last_committed_remote_input.remote_tick as i32 - last_committed_remote_input.local_tick as i32 - battle.remote_delay() as i32);
                                        core.gba_mut().sync_mut().expect("sync").set_fps_target(tps as f32);

                                        let new_in_battle_time = bn6.in_battle_time(core);
                                        if new_in_battle_time != in_battle_time {
                                            panic!("fastforwarder moved battle time: expected {}, got {}", in_battle_time, new_in_battle_time);
                                        }

                                        core.load_state(&dirty_state).expect("load dirty state");
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
                                    let battle = if let Some(battle) = battle_state.battle.as_mut()
                                    {
                                        battle
                                    } else {
                                        return;
                                    };

                                    if !battle.is_accepting_input() {
                                        battle.start_accepting_input();
                                        log::info!("battle is now accepting input");
                                        return;
                                    }

                                    let ip = battle.take_last_input().expect("last input");

                                    bn6.set_player_input_state(
                                        core,
                                        battle.local_player_index() as u32,
                                        ip.local.joyflags as u16,
                                        ip.local.custom_screen_state as u8,
                                    );
                                    if !ip.local.turn.is_empty() {
                                        bn6.set_player_marshaled_battle_state(
                                            core,
                                            battle.local_player_index() as u32,
                                            ip.local.turn.as_slice(),
                                        );
                                    }
                                    bn6.set_player_input_state(
                                        core,
                                        battle.remote_player_index() as u32,
                                        ip.remote.joyflags as u16,
                                        ip.remote.custom_screen_state as u8,
                                    );
                                    if !ip.remote.turn.is_empty() {
                                        bn6.set_player_marshaled_battle_state(
                                            core,
                                            battle.remote_player_index() as u32,
                                            ip.remote.turn.as_slice(),
                                        );
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
                            Box::new(move |core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };


                                    let battle_state = &mut m.lock_battle_state().await;
                                    battle_state.battle.as_mut().expect("attempted to get battle p2 information while no battle was active!");
                                    match core.as_ref().gba().cpu().gpr(0) {
                                        1 => { battle_state.won_last_battle = true; },
                                        2 => { battle_state.won_last_battle = false; },
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
                            Box::new(move |_core| {
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
                            Box::new(move |_core| {
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
                            Box::new(move |core| {
                                log::warn!("unhandled call to commMenu_handleLinkCableInput at 0x{:0x}: uh oh!", core.as_ref().gba().cpu().gpr(15)-4);
                            }),
                        )
                    },
                    {
                        let match_state = match_state.clone();
                        let bn6 = bn6.clone();
                        let handle = handle.clone();
                        let gui_state = gui_state.clone();
                        let config = config.clone();
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
                                            panic!("match was aborted without being started?")
                                        }
                                        MatchState::NoMatch => {
                                            let gui_state = gui_state.upgrade().expect("upgrade");
                                            gui_state.open_link_code_dialog();
                                            match &*gui_state.lock_connect_request_state() {
                                                gui::DialogState::Pending(_) => {
                                                    return;
                                                }
                                                gui::DialogState::Ok(s) => {
                                                    let config = config.lock();
                                                    let m = battle::Match::new(
                                                        s.code.to_string(),
                                                        bn6.match_type(core),
                                                        core.as_ref().game_title(),
                                                        core.as_ref().crc32(),
                                                        s.input_delay,
                                                        battle::Settings {
                                                            matchmaking_connect_addr: config
                                                                .matchmaking
                                                                .connect_addr
                                                                .to_string(),
                                                            make_webrtc_config: {
                                                                let webrtc = config.webrtc.clone();
                                                                Box::new(move || {
                                                                    webrtc.make_webrtc_config()
                                                                })
                                                            },
                                                        },
                                                    );
                                                    *match_state = MatchState::Match(m);
                                                    match &*match_state {
                                                        MatchState::Match(m) => m.start(handle2),
                                                        _ => unreachable!(),
                                                    }
                                                }
                                                gui::DialogState::Cancelled => {
                                                    bn6.drop_matchmaking_from_comm_menu(core, 0);
                                                }
                                                gui::DialogState::Closed => {
                                                    unreachable!();
                                                }
                                            }
                                            gui_state.close_link_code_dialog();
                                        }
                                        MatchState::Match(m) => match m.poll_for_ready().await {
                                            battle::NegotiationStatus::NotReady => {}
                                            battle::NegotiationStatus::Ready => {
                                                bn6.start_battle_from_comm_menu(core);
                                                log::info!("match started");
                                            }
                                            battle::NegotiationStatus::MatchTypeMismatch
                                            | battle::NegotiationStatus::GameMismatch => {
                                                const WRONG_MODE: u32 = 0x25;
                                                bn6.drop_matchmaking_from_comm_menu(
                                                    core, WRONG_MODE,
                                                );
                                                *match_state = MatchState::NoMatch;
                                            }
                                            battle::NegotiationStatus::Failed(e) => {
                                                log::error!("negotiation failed: {}", e);
                                                const CONNECTION_ERROR: u32 = 0x24;
                                                bn6.drop_matchmaking_from_comm_menu(
                                                    core,
                                                    CONNECTION_ERROR,
                                                );
                                                *match_state = MatchState::NoMatch;
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
                            Box::new(move |core| {
                                handle.block_on(async {
                                    let match_state = match_state.lock().await;
                                    let m = if let MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };

                                    let mut rng = m.rng().await.expect("rng");
                                    bn6.set_link_battle_settings_and_background(
                                        core,
                                        bn6::random_battle_settings_and_background(
                                            &mut *rng,
                                            (m.match_type() & 0xff) as u8,
                                        ),
                                    );
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
                            Box::new(move |_core| {
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

        {
            let core = main_core.clone();
            let vbuf = vbuf.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            thread.set_frame_callback(Some(Box::new(move || {
                // TODO: This sometimes causes segfaults when the game gets unloaded.
                let core = core.lock();
                let vbuf = match vbuf.upgrade() {
                    Some(vbuf) => vbuf,
                    None => {
                        return;
                    }
                };
                let mut vbuf = vbuf.lock();
                vbuf.copy_from_slice(core.video_buffer().unwrap());
                for i in (0..vbuf.len()).step_by(4) {
                    vbuf[i + 3] = 0xff;
                }
                if let Some(emu_tps_counter) = emu_tps_counter.upgrade() {
                    let mut emu_tps_counter = emu_tps_counter.lock();
                    emu_tps_counter.mark();
                }
            })));
        }

        Ok(GameState {
            main_core,
            match_state,
            _trapper: trapper,
            _thread: thread,
            _stream: stream,
        })
    }
}

impl Game {
    pub fn new(config: config::Config) -> Result<Game, anyhow::Error> {
        let audio_device = cpal::default_host()
            .default_output_device()
            .ok_or(anyhow::format_err!("could not open audio device"))?;
        log::info!(
            "supported audio output configs: {:?}",
            audio_device.supported_output_configs()?.collect::<Vec<_>>()
        );

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;

        {
            let bind_addr = config.matchmaking.bind_addr.to_string();
            rt.spawn(async {
                let bind_addr2 = bind_addr.clone();
                if let Err(e) = (move || async {
                    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
                    log::info!("bound local matchmaking server on {}", listener.local_addr()?);
                    let mut server = tango_matchmaking::server::Server::new(listener);
                    server.run().await;
                    Result::<(), anyhow::Error>::Ok(())
                })()
                .await
                {
                    log::info!("failed to bind local matchmaking server to {}, direct connect will not be available: {}", bind_addr2, e)
                }
            });
        }

        let handle = rt.handle().clone();

        let event_loop = Some(winit::event_loop::EventLoop::new());

        let current_input =
            std::rc::Rc::new(std::cell::RefCell::new(current_input::CurrentInput::new()));

        let vbuf = Arc::new(Mutex::new(vec![
            0u8;
            (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4)
                as usize
        ]));

        let window = {
            let size = winit::dpi::LogicalSize::new(
                mgba::gba::SCREEN_WIDTH * 3,
                mgba::gba::SCREEN_HEIGHT * 3,
            );
            winit::window::WindowBuilder::new()
                .with_title("tango")
                .with_inner_size(size)
                .with_min_inner_size(size)
                .build(event_loop.as_ref().expect("event loop"))?
        };

        let config = Arc::new(Mutex::new(config));

        let fps_counter = Arc::new(Mutex::new(tps::Counter::new(30)));
        let emu_tps_counter = Arc::new(Mutex::new(tps::Counter::new(10)));

        let (pixels, gui) = {
            let config = config.clone();
            let window_size = window.inner_size();
            let surface_texture =
                pixels::SurfaceTexture::new(window_size.width, window_size.height, &window);
            let pixels = pixels::PixelsBuilder::new(
                mgba::gba::SCREEN_WIDTH,
                mgba::gba::SCREEN_HEIGHT,
                surface_texture,
            )
            .request_adapter_options(pixels::wgpu::RequestAdapterOptions {
                power_preference: pixels::wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .build()?;
            let gui = gui::Gui::new(
                config,
                current_input.clone(),
                window_size.width,
                window_size.height,
                window.scale_factor() as f32,
                &pixels,
            );
            (pixels, gui)
        };

        let gui_state = gui.state();

        let game_state = Arc::new(Mutex::new(None::<GameState>));

        {
            let game_state = Arc::downgrade(&game_state);
            let fps_counter = fps_counter.clone();
            let emu_tps_counter = emu_tps_counter.clone();
            let handle = handle.clone();
            gui_state.set_debug_stats_getter(Some(Box::new(move || {
                handle.block_on(async {
                    let game_state = if let Some(game_state) = game_state.upgrade() {
                        game_state
                    } else {
                        return None;
                    };
                    let game_state = game_state.lock();
                    let game_state = if let Some(game_state) = &*game_state {
                        game_state
                    } else {
                        return None;
                    };

                    let core = game_state.main_core.lock();
                    let emu_tps_counter = emu_tps_counter.lock();
                    let fps_counter = fps_counter.lock();
                    let match_state = game_state.match_state.lock().await;
                    Some(gui::DebugStats {
                        fps: 1.0 / fps_counter.mean_duration().as_secs_f32(),
                        emu_tps: 1.0 / emu_tps_counter.mean_duration().as_secs_f32(),
                        target_tps: core.as_ref().gba().sync().unwrap().fps_target(),
                        match_state: match &*match_state {
                            MatchState::NoMatch => "none",
                            MatchState::Aborted => "aborted",
                            MatchState::Match(_) => "active",
                        },
                        battle_debug_stats: match &*match_state {
                            MatchState::NoMatch => None,
                            MatchState::Aborted => None,
                            MatchState::Match(m) => {
                                let battle_state = m.lock_battle_state().await;
                                match &battle_state.battle {
                                    Some(battle) => Some(gui::BattleDebugStats {
                                        local_player_index: battle.local_player_index(),
                                        local_qlen: battle.local_queue_length().await,
                                        remote_qlen: battle.remote_queue_length().await,
                                        local_delay: battle.local_delay(),
                                        remote_delay: battle.remote_delay(),
                                    }),
                                    None => None,
                                }
                            }
                        },
                    })
                })
            })));
        };

        Ok(Game {
            rt,
            audio_device,
            config,
            fps_counter,
            current_input,
            event_loop,
            window,
            pixels,
            vbuf,
            gui,
            game_state,
            emu_tps_counter,
        })
    }

    pub fn load(&self, rom_filename: &std::path::Path) -> anyhow::Result<()> {
        let save_filename = rom_filename.with_extension("sav");

        *self.game_state.lock() = {
            let handle = self.rt.handle().clone();
            Some(GameState::new(
                rom_filename,
                &save_filename,
                handle,
                &self.audio_device,
                self.config.clone(),
                Arc::downgrade(&self.gui.state()),
                Arc::downgrade(&self.vbuf),
                Arc::downgrade(&self.emu_tps_counter),
            )?)
        };
        Ok(())
    }

    pub fn run(mut self: Self) {
        let mut rom_list: Vec<gui::ROMInfo> = std::fs::read_dir("roms")
            .expect("roms")
            .flat_map(|dirent| {
                let dirent = dirent.expect("dirent");
                let mut core = mgba::core::Core::new_gba("tango").expect("new_gba");
                let vf =
                    match mgba::vfile::VFile::open(&dirent.path(), mgba::vfile::flags::O_RDONLY) {
                        Ok(vf) => vf,
                        Err(e) => {
                            log::warn!(
                                "failed to open {} for probing: {}",
                                dirent.path().display(),
                                e
                            );
                            return vec![];
                        }
                    };
                if let Err(e) = core.as_mut().load_rom(vf) {
                    log::warn!(
                        "failed to load {} for probing: {}",
                        dirent.path().display(),
                        e
                    );
                    return vec![];
                }

                let title = core.as_ref().game_title();
                if bn6::BN6::new(&title).is_none() {
                    return vec![];
                }

                vec![gui::ROMInfo {
                    path: dirent
                        .path()
                        .strip_prefix("roms")
                        .expect("strip prefix")
                        .to_owned(),
                    title,
                }]
            })
            .collect();
        rom_list.sort_unstable_by(|x, y| x.path.cmp(&y.path));

        // Probe for ROMs.
        {
            let gui_state = self.gui.state();
            gui_state.set_rom_list(rom_list.clone());
            gui_state.open_rom_select_dialog();
        }

        let handle = self.rt.handle().clone();

        let mut gui_handled = false;

        let current_input = self.current_input.clone();

        self.event_loop
            .take()
            .expect("event loop")
            .run(move |event, _, control_flow| {
                *control_flow = winit::event_loop::ControlFlow::Poll;

                match event {
                    winit::event::Event::WindowEvent {
                        event: ref window_event,
                        ..
                    } => {
                        match window_event {
                            winit::event::WindowEvent::CloseRequested => {
                                *control_flow = winit::event_loop::ControlFlow::Exit;
                            }
                            winit::event::WindowEvent::Resized(size) => {
                                self.pixels.resize_surface(size.width, size.height);
                                self.gui.resize(size.width, size.height);
                            }
                            _ => {}
                        };
                        {
                            let mut current_input = current_input.borrow_mut();
                            current_input.handle_event(&window_event);
                        }
                        gui_handled = self.gui.handle_event(&window_event);
                    }
                    winit::event::Event::MainEventsCleared => {
                        {
                            let current_input = current_input.borrow();

                            if !gui_handled {
                                if let Some(game_state) = &*self.game_state.lock() {
                                    let mut core = game_state.main_core.lock();
                                    let config = self.config.lock();

                                    let mut keys = 0u32;
                                    if current_input.key_held[config.keymapping.left as usize] {
                                        keys |= mgba::input::keys::LEFT;
                                    }
                                    if current_input.key_held[config.keymapping.right as usize] {
                                        keys |= mgba::input::keys::RIGHT;
                                    }
                                    if current_input.key_held[config.keymapping.up as usize] {
                                        keys |= mgba::input::keys::UP;
                                    }
                                    if current_input.key_held[config.keymapping.down as usize] {
                                        keys |= mgba::input::keys::DOWN;
                                    }
                                    if current_input.key_held[config.keymapping.a as usize] {
                                        keys |= mgba::input::keys::A;
                                    }
                                    if current_input.key_held[config.keymapping.b as usize] {
                                        keys |= mgba::input::keys::B;
                                    }
                                    if current_input.key_held[config.keymapping.l as usize] {
                                        keys |= mgba::input::keys::L;
                                    }
                                    if current_input.key_held[config.keymapping.r as usize] {
                                        keys |= mgba::input::keys::R;
                                    }
                                    if current_input.key_held[config.keymapping.start as usize] {
                                        keys |= mgba::input::keys::START;
                                    }
                                    if current_input.key_held[config.keymapping.select as usize] {
                                        keys |= mgba::input::keys::SELECT;
                                    }

                                    handle.block_on(async {
                                        match &*game_state.match_state.lock().await {
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
                                }
                                gui_handled = false;
                            }

                            {
                                let gui_state = self.gui.state();

                                let selected_rom = {
                                    let mut selected_rom = None;
                                    let rom_select_state = gui_state.lock_rom_select_state();
                                    match &*rom_select_state {
                                        gui::DialogState::Pending(_) => {}
                                        gui::DialogState::Ok(None) => {
                                            unreachable!();
                                        }
                                        gui::DialogState::Ok(Some(index)) => {
                                            selected_rom = Some(&rom_list[*index]);
                                        }
                                        gui::DialogState::Cancelled => {
                                            unreachable!();
                                        }
                                        gui::DialogState::Closed => {}
                                    }
                                    selected_rom
                                };

                                if let Some(selected_rom) = selected_rom {
                                    log::info!("loading rom: {:?}", selected_rom);
                                    self.load(&selected_rom.path).expect("load rom");
                                    gui_state.close_rom_select_dialog();
                                }
                            }

                            if current_input.key_actions.iter().any(|action| {
                                if let current_input::KeyAction::Pressed(
                                    winit::event::VirtualKeyCode::Escape,
                                ) = action
                                {
                                    true
                                } else {
                                    false
                                }
                            }) {
                                self.gui.state().toggle_menu();
                            }
                        }

                        let vbuf = self.vbuf.lock().clone();
                        self.pixels.get_frame().copy_from_slice(&vbuf);

                        self.gui.prepare(&self.window);
                        self.pixels
                            .render_with(|encoder, render_target, context| {
                                context.scaling_renderer.render(encoder, render_target);
                                self.gui.render(encoder, render_target, context)?;
                                Ok(())
                            })
                            .expect("render pixels");
                        self.fps_counter.lock().mark();

                        {
                            let mut current_input = current_input.borrow_mut();
                            current_input.step();
                        }
                    }
                    _ => {}
                }
            });
    }
}
