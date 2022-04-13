use crate::{battle, bn6, config, fastforwarder, gui, input, tps};
use cpal::traits::StreamTrait;
use parking_lot::Mutex;
use std::sync::Arc;

const EXPECTED_FPS: u32 = 60;

pub enum MatchState {
    NoMatch,
    Aborted,
    Match(battle::Match),
}

pub struct Loaded {
    core: Arc<Mutex<mgba::core::Core>>,
    match_state: Arc<tokio::sync::Mutex<MatchState>>,
    joyflags: Arc<std::sync::atomic::AtomicU32>,
    _trapper: mgba::trapper::Trapper,
    _thread: mgba::thread::Thread,
    _stream: cpal::Stream,
}

impl Loaded {
    pub fn new(
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

        let core = Arc::new(Mutex::new({
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
            let core = core.clone();
            let core = core.lock();
            bn6::BN6::new(&core.as_ref().game_title()).unwrap()
        };

        let match_state = Arc::new(tokio::sync::Mutex::new(MatchState::NoMatch));

        let mut thread = {
            let core = core.clone();
            mgba::thread::Thread::new(core)
        };
        thread.start();

        let stream = {
            let core = core.clone();
            mgba::audio::open_stream(core, audio_device)?
        };
        stream.play()?;

        {
            let core = core.clone();
            let mut core = core.lock();
            core.as_mut()
                .gba_mut()
                .sync_mut()
                .as_mut()
                .expect("sync")
                .set_fps_target(60.0);
        }

        let joyflags = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let trapper = {
            // TODO: Should these be weak?
            let core = core.clone();
            let mut core = core.lock();
            let joyflags = joyflags.clone();
            let bn6 = bn6;
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

                                        let remote_init = match m.receive_remote_init().await {
                                            Some(remote_init) => remote_init,
                                            None => {
                                                core.gba_mut().sync_mut().expect("sync").set_fps_target(EXPECTED_FPS as f32);
                                                break 'abort;
                                            }
                                        };
                                        log::info!("received remote init: {:?}", remote_init);
                                        bn6.set_player_marshaled_battle_state(core, battle.remote_player_index() as u32, remote_init.marshaled.as_slice());
                                        battle.set_remote_delay(remote_init.input_delay);

                                        let (p1_init, p2_init) = if battle.local_player_index() == 0 {
                                            (local_init.as_slice(), remote_init.marshaled.as_slice())
                                        } else {
                                            (remote_init.marshaled.as_slice(), local_init.as_slice())
                                        };

                                        replay_writer.write_inits(p1_init, p2_init).expect("write init");

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
                                        if battle.committed_state().is_none() {
                                            for i in 0..battle.local_delay() {
                                                battle
                                                    .add_local_input(
                                                        input::Input {
                                                            local_tick: in_battle_time + i,
                                                            remote_tick: 0,
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
                                                            remote_tick: 0,
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

                                        let joyflags: u16 = joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16 | 0xfc00;
                                        let local_tick = in_battle_time + battle.local_delay();
                                        let last_committed_remote_input =
                                            battle.last_committed_remote_input();
                                        let remote_tick = last_committed_remote_input.local_tick;
                                        let custom_screen_state = bn6.local_custom_screen_state(core);
                                        let turn = battle.take_local_pending_turn();

                                        const TIMEOUT: std::time::Duration =
                                            std::time::Duration::from_secs(5);
                                        if (tokio::time::timeout(
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
                                        .await).is_err()
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
                        let gui_state = gui_state;
                        let config = config;
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
                        let handle = handle;
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
            let core = core.clone();
            let vbuf = vbuf;
            let emu_tps_counter = emu_tps_counter;
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

        Ok(Loaded {
            core,
            match_state,
            joyflags,
            _trapper: trapper,
            _thread: thread,
            _stream: stream,
        })
    }

    pub fn lock_core(&self) -> parking_lot::MutexGuard<mgba::core::Core> {
        self.core.lock()
    }

    pub async fn lock_match_state<'a>(&'a self) -> tokio::sync::MutexGuard<'a, MatchState> {
        self.match_state.lock().await
    }

    pub fn set_joyflags(&self, joyflags: u32) {
        self.joyflags
            .store(joyflags, std::sync::atomic::Ordering::Relaxed)
    }
}
