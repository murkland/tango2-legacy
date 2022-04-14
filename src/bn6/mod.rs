use crate::{battle, config, fastforwarder, gui, hooks, input, loaded};

mod munger;
mod offsets;

#[derive(Clone)]
pub struct BN6 {
    offsets: offsets::Offsets,
    munger: munger::Munger,
}

impl BN6 {
    pub fn new(title: &str) -> Option<BN6> {
        let offsets = match offsets::offsets(title) {
            Some(o) => o.clone(),
            None => return None,
        };
        Some(BN6 {
            offsets: offsets.clone(),
            munger: munger::Munger { offsets },
        })
    }
}

fn random_battle_settings_and_background(rng: &mut impl rand::Rng, match_type: u8) -> u16 {
    const BATTLE_BACKGROUNDS: &[u16] = &[
        0x00, 0x01, 0x01, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x11, 0x13, 0x13,
    ];

    let lo = match match_type {
        0 => rng.gen_range(0..0x44u16),
        1 => rng.gen_range(0..0x60u16),
        2 => rng.gen_range(0..0x44u16) + 0x60u16,
        _ => 0u16,
    };

    let hi = BATTLE_BACKGROUNDS[rng.gen_range(0..BATTLE_BACKGROUNDS.len())];

    hi << 0x8 | lo
}

impl hooks::Hooks for BN6 {
    fn install_main_hooks(
        &self,
        config: std::sync::Arc<parking_lot::Mutex<config::Config>>,
        core: mgba::core::CoreMutRef,
        handle: tokio::runtime::Handle,
        match_state: std::sync::Arc<tokio::sync::Mutex<loaded::MatchState>>,
        joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
        gui_state: std::sync::Weak<gui::State>,
        mut fastforwarder: fastforwarder::Fastforwarder,
    ) -> mgba::trapper::Trapper {
        mgba::trapper::Trapper::new(
            core,
            vec![
                {
                    let match_state = match_state.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_init_call_battle_copy_input_data,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                            let match_state = match_state.lock().await;
                            let m = if let loaded::MatchState::Match(m) = &*match_state {
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
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_init_marshal_ret,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let mut match_state = match_state.lock().await;
                                'abort: loop {
                                    let m = if let loaded::MatchState::Match(m) = &*match_state {
                                        m
                                    } else {
                                        return;
                                    };

                                    let mut battle_state = m.lock_battle_state().await;
                                    let battle_number = battle_state.number;
                                    let battle = battle_state.battle.as_mut().expect("attempted to get p2 battle information while no battle was active!");

                                    let replay_writer = battle.replay_writer().upgrade().expect("upgrade");
                                    let mut replay_writer = replay_writer.lock();

                                    let local_init = munger.local_marshaled_battle_state(core);
                                    m.send_init(battle_number, battle.local_delay(), &local_init).await.expect("send init");
                                    log::info!("sent local init");
                                    munger.set_player_marshaled_battle_state(core, battle.local_player_index() as u32, local_init.as_slice());

                                    let remote_init = match m.receive_remote_init().await {
                                        Some(remote_init) => remote_init,
                                        None => {
                                            core.gba_mut().sync_mut().expect("sync").set_fps_target(loaded::EXPECTED_FPS as f32);
                                            break 'abort;
                                        }
                                    };
                                    log::info!("received remote init: {:?}", remote_init);
                                    munger.set_player_marshaled_battle_state(core, battle.remote_player_index() as u32, remote_init.marshaled.as_slice());
                                    battle.set_remote_delay(remote_init.input_delay);

                                    let (p1_init, p2_init) = if battle.local_player_index() == 0 {
                                        (local_init.as_slice(), remote_init.marshaled.as_slice())
                                    } else {
                                        (remote_init.marshaled.as_slice(), local_init.as_slice())
                                    };

                                    replay_writer.write_inits(p1_init, p2_init).expect("write init");

                                    return;
                                }
                                *match_state = loaded::MatchState::Aborted;
                            });
                        }),
                    )
                },
                {
                    let match_state = match_state.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_turn_marshal_ret,
                        Box::new(move |core| {
                            handle.block_on(async {
                                let match_state = match_state.lock().await;
                                let m = if let loaded::MatchState::Match(m) = &*match_state {
                                    m
                                } else {
                                    return;
                                };

                                let mut battle_state = m.lock_battle_state().await;
                                let battle = battle_state.battle.as_mut().expect("attempted to get p2 battle information while no battle was active!");

                                log::info!("turn data marshaled on {}", munger.in_battle_time(core));

                                let local_turn = munger.local_marshaled_battle_state(core);
                                battle.add_local_pending_turn(local_turn);
                            });
                        }),
                    )
                },
                {
                    let match_state = match_state.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.main_read_joyflags,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                    let mut match_state = match_state.lock().await;
                                    'abort: loop {
                                        let m = if let loaded::MatchState::Match(m) = &*match_state {
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

                                        let replay_writer = battle.replay_writer().upgrade().expect("upgrade");
                                        let mut replay_writer = replay_writer.lock();

                                        let in_battle_time = munger.in_battle_time(core);
                                        if battle.committed_state().is_none() {
                                            for i in 0..battle.local_delay() {
                                                battle
                                                    .add_local_input(
                                                        input::Input {
                                                            local_tick: in_battle_time + i,
                                                            remote_tick: 0,
                                                            joyflags: 0,
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
                                                            joyflags: 0,
                                                            custom_screen_state: 0,
                                                            turn: vec![],
                                                        },
                                                    )
                                                    .await;
                                            }
                                            let committed_state = core.save_state().expect("save committed state");

                                            replay_writer.write_state(&committed_state).expect("write state");

                                            battle.set_committed_state(committed_state);

                                            log::info!("battle state committed");
                                        }

                                        let joyflags: u16 = joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16;
                                        let local_tick = in_battle_time + battle.local_delay();
                                        let last_committed_remote_input =
                                            battle.last_committed_remote_input();
                                        let remote_tick = last_committed_remote_input.local_tick;
                                        let custom_screen_state = munger.local_custom_screen_state(core);
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
                                            core.gba_mut().sync_mut().expect("sync").set_fps_target(loaded::EXPECTED_FPS as f32);
                                            break 'abort;
                                        }

                                        m.send_input(battle_number, local_tick, remote_tick, joyflags, custom_screen_state, turn).await.expect("send input");

                                        let (input_pairs, left) = battle.consume_and_peek_local().await;

                                        for ip in &input_pairs {
                                            replay_writer.write_input(battle.local_player_index(), &ip,).expect("write input");
                                        }

                                        let (committed_state, dirty_state, last_input) = fastforwarder.fastforward(
                                            battle.committed_state().as_ref().expect("committed state"),
                                            battle.local_player_index(),
                                            &input_pairs,
                                            battle.last_committed_remote_input(),
                                            &left,
                                        ).expect("fastforward");
                                        battle.set_committed_state(committed_state);
                                        let last_joyflags = last_input.remote.joyflags;
                                        battle.set_last_input(last_input);

                                        let tps = loaded::EXPECTED_FPS as i32 + (remote_tick as i32 - local_tick as i32 - battle.local_delay() as i32) - (last_committed_remote_input.remote_tick as i32 - last_committed_remote_input.local_tick as i32 - battle.remote_delay() as i32);
                                        core.gba_mut().sync_mut().expect("sync").set_fps_target(tps as f32);

                                        let new_in_battle_time = munger.in_battle_time(core);
                                        if new_in_battle_time != in_battle_time {
                                            panic!("fastforwarder moved battle time: expected {}, got {}", in_battle_time, new_in_battle_time);
                                        }

                                        core.load_state(&dirty_state).expect("load dirty state");
                                        core.gba_mut().cpu_mut().set_gpr(4, last_joyflags as i32);
                                        return;
                                    }
                                    *match_state = loaded::MatchState::Aborted;
                                });
                        }),
                    )
                },
                {
                    let match_state = match_state.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_update_call_battle_copy_input_data,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = match_state.lock().await;
                                let m = if let loaded::MatchState::Match(m) = &*match_state {
                                    m
                                } else {
                                    return;
                                };

                                core.gba_mut().cpu_mut().set_gpr(0, 0);
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                let battle_state = &mut m.lock_battle_state().await;
                                let battle = if let Some(battle) = battle_state.battle.as_mut() {
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

                                munger.set_player_input_state(
                                    core,
                                    battle.local_player_index() as u32,
                                    ip.local.joyflags as u16,
                                    ip.local.custom_screen_state as u8,
                                );
                                if !ip.local.turn.is_empty() {
                                    munger.set_player_marshaled_battle_state(
                                        core,
                                        battle.local_player_index() as u32,
                                        ip.local.turn.as_slice(),
                                    );
                                }
                                munger.set_player_input_state(
                                    core,
                                    battle.remote_player_index() as u32,
                                    ip.remote.joyflags as u16,
                                    ip.remote.custom_screen_state as u8,
                                );
                                if !ip.remote.turn.is_empty() {
                                    munger.set_player_marshaled_battle_state(
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
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_run_unpaused_step_cmp_retval,
                        Box::new(move |core| {
                            handle.block_on(async {
                            let match_state = match_state.lock().await;
                            let m = if let loaded::MatchState::Match(m) = &*match_state {
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
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_start_ret,
                        Box::new(move |_core| {
                            handle.block_on(async {
                                let match_state = match_state.lock().await;
                                let m = if let loaded::MatchState::Match(m) = &*match_state {
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
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_ending_ret,
                        Box::new(move |_core| {
                            handle.block_on(async {
                                let match_state = match_state.lock().await;
                                let m = if let loaded::MatchState::Match(m) = &*match_state {
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
                        self.offsets.rom.battle_is_p2_tst,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                            let match_state = match_state.lock().await;
                            let m = if let loaded::MatchState::Match(m) = &*match_state {
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
                        self.offsets.rom.link_is_p2_ret,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                            let match_state = match_state.lock().await;
                            let m = if let loaded::MatchState::Match(m) = &*match_state {
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
                        self.offsets.rom.get_copy_data_input_state_ret,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let mut r0 = core.as_ref().gba().cpu().gpr(0);
                                if r0 != 2 {
                                    log::warn!("expected r0 to be 2 but got {}", r0);
                                }

                                let match_state = match_state.lock().await;
                                if let loaded::MatchState::Aborted = *match_state {
                                    r0 = 4;
                                }

                                core.gba_mut().cpu_mut().set_gpr(0, r0);
                            });
                        }),
                    )
                },
                {
                    (
                        self.offsets.rom.comm_menu_handle_link_cable_input_entry,
                        Box::new(move |core| {
                            log::warn!("unhandled call to commMenu_handleLinkCableInput at 0x{:0x}: uh oh!", core.as_ref().gba().cpu().gpr(15)-4);
                        }),
                    )
                },
                {
                    let match_state = match_state.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    let gui_state = gui_state.clone();
                    let config = config;
                    (
                        self.offsets
                            .rom
                            .comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input,
                        Box::new(move |mut core| {
                            let handle2 = handle.clone();
                            handle.block_on(async {
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                let mut match_state = match_state.lock().await;
                                match &*match_state {
                                    loaded::MatchState::Aborted => {
                                        panic!("match was aborted without being started?")
                                    }
                                    loaded::MatchState::NoMatch => {
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
                                                    munger.match_type(core),
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
                                                *match_state = loaded::MatchState::Match(m);
                                                match &*match_state {
                                                    loaded::MatchState::Match(m) => {
                                                        m.start(handle2)
                                                    }
                                                    _ => unreachable!(),
                                                }
                                            }
                                            gui::DialogState::Cancelled => {
                                                munger.drop_matchmaking_from_comm_menu(core, 0);
                                            }
                                            gui::DialogState::Closed => {
                                                unreachable!();
                                            }
                                        }
                                        gui_state.close_link_code_dialog();
                                    }
                                    loaded::MatchState::Match(m) => {
                                        match m.poll_for_ready().await {
                                            battle::NegotiationStatus::NotReady => {}
                                            battle::NegotiationStatus::Ready => {
                                                munger.start_battle_from_comm_menu(core);
                                                log::info!("match started");
                                            }
                                            battle::NegotiationStatus::MatchTypeMismatch
                                            | battle::NegotiationStatus::GameMismatch => {
                                                const WRONG_MODE: u32 = 0x25;
                                                munger.drop_matchmaking_from_comm_menu(
                                                    core, WRONG_MODE,
                                                );
                                                *match_state = loaded::MatchState::NoMatch;
                                            }
                                            battle::NegotiationStatus::Failed(e) => {
                                                log::error!("negotiation failed: {}", e);
                                                const CONNECTION_ERROR: u32 = 0x24;
                                                munger.drop_matchmaking_from_comm_menu(
                                                    core,
                                                    CONNECTION_ERROR,
                                                );
                                                *match_state = loaded::MatchState::NoMatch;
                                            }
                                        }
                                    }
                                };
                            });
                        }),
                    )
                },
                {
                    let match_state = match_state.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.comm_menu_init_battle_entry,
                        Box::new(move |core| {
                            handle.block_on(async {
                                let match_state = match_state.lock().await;
                                let m = if let loaded::MatchState::Match(m) = &*match_state {
                                    m
                                } else {
                                    return;
                                };

                                let mut rng = m.rng().await.expect("rng");
                                munger.set_link_battle_settings_and_background(
                                    core,
                                    random_battle_settings_and_background(
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
                        self.offsets.rom.comm_menu_wait_for_friend_ret_cancel,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                log::info!("match canceled by user");
                                let mut match_state = match_state.lock().await;
                                *match_state = loaded::MatchState::NoMatch;
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
                        self.offsets.rom.comm_menu_end_battle_entry,
                        Box::new(move |_core| {
                            handle.block_on(async {
                                let mut match_state = match_state.lock().await;
                                *match_state = loaded::MatchState::NoMatch;
                                log::info!("match ended");
                            });
                        }),
                    )
                },
                {
                    (
                        self.offsets
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
    }

    fn install_fastforwarder_hooks(
        &self,
        core: mgba::core::CoreMutRef,
        ff_state: Box<dyn hooks::FastforwarderState>,
    ) -> mgba::trapper::Trapper {
        mgba::trapper::Trapper::new(
            core,
            vec![
                {
                    let munger = self.munger.clone();
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.main_read_joyflags,
                        Box::new(move |mut core| {
                            let in_battle_time = munger.in_battle_time(core);

                            if in_battle_time == ff_state.commit_time() {
                                ff_state.set_committed_state(
                                    core.save_state().expect("save committed state"),
                                );
                            }

                            if in_battle_time == ff_state.dirty_time() {
                                ff_state
                                    .set_dirty_state(core.save_state().expect("save dirty state"));
                            }

                            let ip = match ff_state.peek_input_pair() {
                                Some(ip) => ip,
                                None => {
                                    return;
                                }
                            };

                            if ip.local.local_tick != ip.remote.local_tick {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                    in_battle_time,
                                    ip.local.local_tick,
                                    ip.remote.local_tick
                                ));
                                return;
                            }

                            if ip.local.local_tick != in_battle_time {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "input tick != in battle tick: {} != {}",
                                    ip.local.local_tick,
                                    in_battle_time,
                                ));
                                return;
                            }

                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(4, ip.local.joyflags as i32);
                        }),
                    )
                },
                {
                    let munger = self.munger.clone();
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.battle_update_call_battle_copy_input_data,
                        Box::new(move |mut core| {
                            let in_battle_time = munger.in_battle_time(core);

                            let ip = match ff_state.pop_input_pair() {
                                Some(ip) => ip,
                                None => {
                                    return;
                                }
                            };

                            core.gba_mut().cpu_mut().set_gpr(0, 0);
                            let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                            core.gba_mut().cpu_mut().set_pc(r15 + 4);

                            if ip.local.local_tick != ip.remote.local_tick {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                    in_battle_time,
                                    ip.local.local_tick,
                                    ip.local.local_tick
                                ));
                                return;
                            }

                            if ip.local.local_tick != in_battle_time {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "input tick != in battle tick: {} != {}",
                                    ip.local.local_tick,
                                    in_battle_time,
                                ));
                                return;
                            }

                            let local_player_index = ff_state.local_player_index();
                            let remote_player_index = 1 - local_player_index;

                            munger.set_player_input_state(
                                core,
                                local_player_index as u32,
                                ip.local.joyflags,
                                ip.local.custom_screen_state,
                            );
                            if !ip.local.turn.is_empty() {
                                munger.set_player_marshaled_battle_state(
                                    core,
                                    local_player_index as u32,
                                    ip.local.turn.as_slice(),
                                );
                            }

                            munger.set_player_input_state(
                                core,
                                remote_player_index as u32,
                                ip.remote.joyflags,
                                ip.remote.custom_screen_state,
                            );
                            if !ip.remote.turn.is_empty() {
                                munger.set_player_marshaled_battle_state(
                                    core,
                                    remote_player_index as u32,
                                    ip.remote.turn.as_slice(),
                                );
                            }
                        }),
                    )
                },
                {
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.battle_is_p2_tst,
                        Box::new(move |mut core| {
                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, ff_state.local_player_index() as i32);
                        }),
                    )
                },
                {
                    let ff_state = ff_state.clone();
                    (
                        self.offsets.rom.link_is_p2_ret,
                        Box::new(move |mut core| {
                            core.gba_mut()
                                .cpu_mut()
                                .set_gpr(0, ff_state.local_player_index() as i32);
                        }),
                    )
                },
                {
                    (
                        self.offsets
                            .rom
                            .comm_menu_in_battle_call_comm_menu_handle_link_cable_input,
                        Box::new(move |mut core| {
                            let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                            core.gba_mut().cpu_mut().set_pc(r15 + 4);
                        }),
                    )
                },
                {
                    (
                        self.offsets.rom.get_copy_data_input_state_ret,
                        Box::new(move |mut core| {
                            core.gba_mut().cpu_mut().set_gpr(0, 2);
                        }),
                    )
                },
            ],
        )
    }

    fn prepare_for_fastforward(&self, mut core: mgba::core::CoreMutRef) {
        core.gba_mut()
            .cpu_mut()
            .set_pc(self.offsets.rom.main_read_joyflags);
    }

    fn in_battle_time(&self, core: mgba::core::CoreMutRef) -> u32 {
        self.munger.in_battle_time(core)
    }
}
