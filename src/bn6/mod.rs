use crate::{facade, fastforwarder, hooks};

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
            Some(o) => o,
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
        core: mgba::core::CoreMutRef,
        handle: tokio::runtime::Handle,
        mut facade: facade::Facade,
    ) -> mgba::trapper::Trapper {
        mgba::trapper::Trapper::new(
            core,
            vec![
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_init_call_battle_copy_input_data,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                core.gba_mut().cpu_mut().set_gpr(0, 0);
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_init_marshal_ret,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let mut match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                'abort: loop {
                                    let mut battle_state = match_state.lock_battle_state().await;

                                    let local_init = munger.local_marshaled_battle_state(core);
                                    battle_state.send_init(&local_init).await;
                                    munger.set_player_marshaled_battle_state(
                                        core,
                                        battle_state.local_player_index() as u32,
                                        local_init.as_slice(),
                                    );

                                    let remote_init = match battle_state.receive_init().await {
                                        Some(remote_init) => remote_init,
                                        None => {
                                            break 'abort;
                                        }
                                    };
                                    munger.set_player_marshaled_battle_state(
                                        core,
                                        battle_state.remote_player_index() as u32,
                                        remote_init.as_slice(),
                                    );
                                    return;
                                }
                                match_state.abort(core);
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_turn_marshal_ret,
                        Box::new(move |core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                let mut battle_state = match_state.lock_battle_state().await;

                                log::info!("turn data marshaled on {}", munger.current_tick(core));
                                let local_turn = munger.local_marshaled_battle_state(core);
                                battle_state.add_local_pending_turn(local_turn);
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.main_read_joyflags,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let mut match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                'abort: loop {
                                    let mut battle_state = match_state.lock_battle_state().await;
                                    if !battle_state.is_active() {
                                        return;
                                    }

                                    if !battle_state.is_accepting_input() {
                                        return;
                                    }

                                    let current_tick = munger.current_tick(core);
                                    if !battle_state.has_committed_state() {
                                        battle_state.set_committed_state(core.save_state().expect("save state"));
                                        battle_state.fill_input_delay(current_tick).await;
                                        log::info!("battle state committed");
                                    }

                                    let turn = battle_state.take_local_pending_turn();

                                    const TIMEOUT: std::time::Duration =
                                        std::time::Duration::from_secs(5);
                                    let (committed_state, dirty_state, last_input) = if let Ok((committed_state, dirty_state, last_input)) = tokio::time::timeout(
                                        TIMEOUT,
                                        battle_state.add_local_input_and_fastforward(
                                                current_tick,
                                                facade.joyflags() as u16,
                                                munger.local_custom_screen_state(core),
                                                turn.clone()
                                        ),
                                    )
                                    .await
                                    {
                                        (committed_state, dirty_state, last_input)
                                    } else {
                                        log::error!("could not queue local input within {:?}, dropping connection", TIMEOUT);
                                        break 'abort;
                                    };

                                    battle_state.set_committed_state(committed_state);
                                    let last_joyflags = last_input.local.joyflags;
                                    battle_state.set_last_input(last_input, core);

                                    core.load_state(&dirty_state).expect("load dirty state");
                                    core.gba_mut().cpu_mut().set_gpr(4, last_joyflags as i32);
                                    return;
                                }
                                match_state.abort(core);
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_update_call_battle_copy_input_data,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                core.gba_mut().cpu_mut().set_gpr(0, 0);
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                let mut battle_state = match_state.lock_battle_state().await;
                                if !battle_state.is_active() {
                                    return;
                                }

                                if !battle_state.is_accepting_input() {
                                    battle_state.mark_accepting_input();
                                    log::info!("battle is now accepting input");
                                    return;
                                }

                                let ip = battle_state.take_last_input().expect("last input");

                                munger.set_player_input_state(
                                    core,
                                    battle_state.local_player_index() as u32,
                                    ip.local.joyflags as u16,
                                    ip.local.custom_screen_state as u8,
                                );
                                if !ip.local.turn.is_empty() {
                                    munger.set_player_marshaled_battle_state(
                                        core,
                                        battle_state.local_player_index() as u32,
                                        ip.local.turn.as_slice(),
                                    );
                                }
                                munger.set_player_input_state(
                                    core,
                                    battle_state.remote_player_index() as u32,
                                    ip.remote.joyflags as u16,
                                    ip.remote.custom_screen_state as u8,
                                );
                                if !ip.remote.turn.is_empty() {
                                    munger.set_player_marshaled_battle_state(
                                        core,
                                        battle_state.remote_player_index() as u32,
                                        ip.remote.turn.as_slice(),
                                    );
                                }
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_run_unpaused_step_cmp_retval,
                        Box::new(move |core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                let mut battle_state = match_state.lock_battle_state().await;
                                if !battle_state.is_active() {
                                    return;
                                }

                                match core.as_ref().gba().cpu().gpr(0) {
                                    1 => {
                                        battle_state.set_won_last_battle(true);
                                    }
                                    2 => {
                                        battle_state.set_won_last_battle(false);
                                    }
                                    _ => {}
                                }
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_start_ret,
                        Box::new(move |_core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                match_state.start_battle().await;
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_ending_ret,
                        Box::new(move |core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                match_state.end_battle(core).await;
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.battle_is_p2_tst,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                let battle_state = match_state.lock_battle_state().await;
                                core.gba_mut()
                                    .cpu_mut()
                                    .set_gpr(0, battle_state.local_player_index() as i32);
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.link_is_p2_ret,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                let battle_state = match_state.lock_battle_state().await;
                                core.gba_mut()
                                    .cpu_mut()
                                    .set_gpr(0, battle_state.local_player_index() as i32);
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.get_copy_data_input_state_ret,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() && !match_state.is_aborted() {
                                    return;
                                }

                                let mut r0 = core.as_ref().gba().cpu().gpr(0);
                                if r0 != 2 {
                                    log::warn!("expected r0 to be 2 but got {}", r0);
                                }

                                if match_state.is_aborted() {
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
                    let mut facade = facade.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets
                            .rom
                            .comm_menu_wait_for_friend_call_comm_menu_handle_link_cable_input,
                        Box::new(move |mut core| {
                            let handle2 = handle.clone();
                            handle.block_on(async {
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                let match_state = facade.match_state();
                                let mut match_state = match_state.lock().await;

                                if match_state.is_aborted() {
                                    panic!("match was aborted without being started?")
                                }

                                if !match_state.is_active() {
                                    match facade.request_connect() {
                                        facade::ConnectRequestStatus::InputComplete(s) => {
                                            let match_type = munger.match_type(core);
                                            match_state.start(core, handle2, match_type, s);
                                        }
                                        facade::ConnectRequestStatus::None => {
                                            munger.drop_matchmaking_from_comm_menu(core);
                                        }
                                        facade::ConnectRequestStatus::Pending => {}
                                    }
                                    return;
                                }

                                match match_state.poll_for_ready().await {
                                    facade::MatchReadyStatus::NotReady => {}
                                    facade::MatchReadyStatus::Ready => {
                                        munger.start_battle_from_comm_menu(core);
                                        log::info!("match started");
                                    }
                                    facade::MatchReadyStatus::Failed => {
                                        munger.drop_matchmaking_from_comm_menu(core);
                                        match_state.end();
                                    }
                                }
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let munger = self.munger.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.comm_menu_init_battle_entry,
                        Box::new(move |core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let match_state = match_state.lock().await;
                                if !match_state.is_active() {
                                    return;
                                }

                                let mut rng = match_state.lock_rng().await;
                                munger.set_link_battle_settings_and_background(
                                    core,
                                    random_battle_settings_and_background(
                                        &mut *rng,
                                        (match_state.match_type() & 0xff) as u8,
                                    ),
                                );
                            });
                        }),
                    )
                },
                {
                    let mut facade = facade.clone();
                    let handle = handle.clone();
                    (
                        self.offsets.rom.comm_menu_wait_for_friend_ret_cancel,
                        Box::new(move |mut core| {
                            handle.block_on(async {
                                log::info!("match canceled by user");
                                let match_state = facade.match_state();
                                let mut match_state = match_state.lock().await;
                                match_state.end();
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);
                            });
                        }),
                    )
                },
                {
                    let handle = handle;
                    (
                        self.offsets.rom.comm_menu_end_battle_entry,
                        Box::new(move |_core| {
                            handle.block_on(async {
                                let match_state = facade.match_state();
                                let mut match_state = match_state.lock().await;
                                match_state.end();
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
        ff_state: fastforwarder::State,
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
                            let current_tick = munger.current_tick(core);

                            if current_tick == ff_state.commit_time() {
                                ff_state.set_committed_state(
                                    core.save_state().expect("save committed state"),
                                );
                            }

                            if current_tick == ff_state.dirty_time() {
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
                                    current_tick,
                                    ip.local.local_tick,
                                    ip.remote.local_tick
                                ));
                                return;
                            }

                            if ip.local.local_tick != current_tick {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "input tick != in battle tick: {} != {}",
                                    ip.local.local_tick,
                                    current_tick,
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
                            let current_tick = munger.current_tick(core);

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
                                    current_tick,
                                    ip.local.local_tick,
                                    ip.local.local_tick
                                ));
                                return;
                            }

                            if ip.local.local_tick != current_tick {
                                ff_state.set_anyhow_error(anyhow::anyhow!(
                                    "input tick != in battle tick: {} != {}",
                                    ip.local.local_tick,
                                    current_tick,
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

    fn current_tick(&self, core: mgba::core::CoreMutRef) -> u32 {
        self.munger.current_tick(core)
    }
}
