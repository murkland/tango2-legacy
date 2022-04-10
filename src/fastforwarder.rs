use super::bn6;
use super::input;
use super::mgba;

struct State {
    local_player_index: u8,
    input_pairs: std::collections::VecDeque<[input::Input; 2]>,
    commit_time: u32,
    committed_state: Option<mgba::state::State>,
    dirty_time: u32,
    dirty_state: Option<mgba::state::State>,
    result: anyhow::Result<()>,
}

pub struct Fastforwarder {
    core: mgba::core::Core,
    _trapper: mgba::trapper::Trapper,
    bn6: bn6::BN6,
    state: std::rc::Rc<std::cell::RefCell<Option<State>>>,
}

impl Fastforwarder {
    pub fn new(rom_path: &std::path::Path, bn6: bn6::BN6) -> anyhow::Result<Self> {
        let mut core = {
            let core = mgba::core::Core::new_gba("tango")?;
            let rom_vf = mgba::vfile::VFile::open(rom_path, mgba::vfile::flags::O_RDONLY)?;
            core.as_mut().load_rom(rom_vf)?;
            core
        };

        let state =
            std::rc::Rc::<std::cell::RefCell<Option<State>>>::new(std::cell::RefCell::new(None));

        let trapper = {
            mgba::trapper::Trapper::new(
                &mut core,
                vec![
                    {
                        let bn6 = bn6::BN6::clone(&bn6);
                        let state = std::rc::Rc::clone(&state);
                        (
                            bn6.offsets.rom.main_read_joyflags,
                            Box::new(move |mut core| {
                                let in_battle_time = bn6.in_battle_time(core);
                                let mut state = state.borrow_mut();

                                if in_battle_time == state.as_ref().unwrap().commit_time {
                                    state.as_mut().unwrap().committed_state =
                                        Some(core.save_state().unwrap());
                                }

                                if in_battle_time == state.as_ref().unwrap().dirty_time {
                                    state.as_mut().unwrap().dirty_state =
                                        Some(core.save_state().unwrap());
                                }

                                if state.as_ref().unwrap().input_pairs.is_empty() {
                                    return;
                                }

                                let ip = state.as_mut().unwrap().input_pairs.pop_front().unwrap();
                                if ip[0].local_tick != ip[1].local_tick {
                                    state.as_mut().unwrap().result = Err(anyhow::anyhow!(
                                        "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                        in_battle_time,
                                        ip[0].local_tick,
                                        ip[1].local_tick
                                    ));
                                    return;
                                }

                                if ip[0].local_tick != in_battle_time {
                                    state.as_mut().unwrap().result = Err(anyhow::anyhow!(
                                        "input tick != in battle tick: {} != {}",
                                        ip[0].local_tick,
                                        in_battle_time,
                                    ));
                                    return;
                                }

                                core.gba_mut().cpu_mut().set_gpr(
                                    4,
                                    ip[state.as_ref().unwrap().local_player_index as usize].joyflags
                                        as i32,
                                );
                            }),
                        )
                    },
                    {
                        let bn6 = bn6::BN6::clone(&bn6);
                        let state = std::rc::Rc::clone(&state);
                        (
                            bn6.offsets.rom.battle_init_call_battle_copy_input_data,
                            Box::new(move |mut core| {
                                let in_battle_time = bn6.in_battle_time(core);
                                let mut state = state.borrow_mut();

                                let commit_time = state.as_ref().unwrap().commit_time;

                                if state.as_ref().unwrap().input_pairs.is_empty() {
                                    return;
                                }

                                core.gba_mut().cpu_mut().set_gpr(0, 0);
                                let r15 = core.as_ref().gba().cpu().gpr(15) as u32;
                                core.gba_mut().cpu_mut().set_pc(r15 + 4);

                                let ip = state.as_mut().unwrap().input_pairs.pop_front().unwrap();
                                if ip[0].local_tick != ip[1].local_tick {
                                    state.as_mut().unwrap().result = Err(anyhow::anyhow!(
                                        "p1 tick != p2 tick (in battle tick = {}): {} != {}",
                                        in_battle_time,
                                        ip[0].local_tick,
                                        ip[1].local_tick
                                    ));
                                    return;
                                }

                                if ip[0].local_tick != in_battle_time {
                                    state.as_mut().unwrap().result = Err(anyhow::anyhow!(
                                        "input tick != in battle tick: {} != {}",
                                        ip[0].local_tick,
                                        in_battle_time,
                                    ));
                                    return;
                                }

                                bn6.set_player_input_state(
                                    core,
                                    0,
                                    ip[0].joyflags,
                                    ip[0].custom_screen_state,
                                );
                                if let Some(turn) = ip[0].turn {
                                    bn6.set_player_marshaled_battle_state(core, 0, &turn);
                                    if in_battle_time < commit_time {
                                        log::info!("p1 turn committed at tick {}", in_battle_time);
                                    }
                                }

                                bn6.set_player_input_state(
                                    core,
                                    1,
                                    ip[1].joyflags,
                                    ip[1].custom_screen_state,
                                );
                                if let Some(turn) = ip[1].turn {
                                    bn6.set_player_marshaled_battle_state(core, 1, &turn);
                                    if in_battle_time < commit_time {
                                        log::info!("p2 turn committed at tick {}", in_battle_time);
                                    }
                                }

                                // TODO: replay writer
                            }),
                        )
                    },
                    {
                        let bn6 = bn6::BN6::clone(&bn6);
                        let state = std::rc::Rc::clone(&state);
                        (
                            bn6.offsets.rom.battle_is_p2_tst,
                            Box::new(move |mut core| {
                                let state = state.borrow();
                                core.gba_mut()
                                    .cpu_mut()
                                    .set_gpr(0, state.as_ref().unwrap().local_player_index as i32);
                            }),
                        )
                    },
                    {
                        let bn6 = bn6::BN6::clone(&bn6);
                        let state = std::rc::Rc::clone(&state);
                        (
                            bn6.offsets.rom.link_is_p2_ret,
                            Box::new(move |mut core| {
                                let state = state.borrow();
                                core.gba_mut()
                                    .cpu_mut()
                                    .set_gpr(0, state.as_ref().unwrap().local_player_index as i32);
                            }),
                        )
                    },
                    {
                        let bn6 = bn6::BN6::clone(&bn6);
                        (
                            bn6.offsets.rom.get_copy_data_input_state_ret,
                            Box::new(move |mut core| {
                                core.gba_mut().cpu_mut().set_gpr(0, 2);
                            }),
                        )
                    },
                ],
            )
        };

        core.as_mut().reset();

        Ok(Fastforwarder {
            core,
            _trapper: trapper,
            bn6,
            state,
        })
    }

    pub fn fastforward(
        &mut self,
        state: &mgba::state::State,
        local_player_index: u8,
        commit_pairs: &[[input::Input; 2]],
        last_committed_remote_input: input::Input,
        local_player_inputs_left: &[input::Input],
    ) -> anyhow::Result<(mgba::state::State, mgba::state::State, [input::Input; 2])> {
        self.core.as_mut().load_state(state)?;
        let start_in_battle_time = self.bn6.in_battle_time(self.core.as_mut());
        let commit_time = start_in_battle_time + commit_pairs.len() as u32;

        let input_pairs = commit_pairs
            .iter()
            .cloned()
            .chain(local_player_inputs_left.iter().cloned().map(|inp| {
                let mut ip = [inp.clone(), inp.clone()];
                let predicted = &mut ip[1 - local_player_index as usize];
                predicted.joyflags = 0;
                if last_committed_remote_input.joyflags & mgba::input::keys::A as u16 != 0 {
                    predicted.joyflags |= mgba::input::keys::A as u16;
                }
                if last_committed_remote_input.joyflags & mgba::input::keys::B as u16 != 0 {
                    predicted.joyflags |= mgba::input::keys::B as u16;
                }
                predicted.custom_screen_state = last_committed_remote_input.custom_screen_state;
                predicted.turn = None;
                ip
            }))
            .collect::<std::collections::VecDeque<[input::Input; 2]>>();

        let dirty_time = start_in_battle_time + input_pairs.len() as u32 - 1;

        self.core
            .as_mut()
            .gba_mut()
            .cpu_mut()
            .set_pc(self.bn6.offsets.rom.main_read_joyflags);

        *self.state.borrow_mut() = Some(State {
            local_player_index,
            input_pairs,
            commit_time,
            committed_state: None,
            dirty_time,
            dirty_state: None,
            result: Ok(()),
        });

        while self
            .state
            .borrow()
            .as_ref()
            .unwrap()
            .committed_state
            .is_none()
            && self.state.borrow().as_ref().unwrap().dirty_state.is_none()
        {
            self.state.borrow_mut().as_mut().unwrap().result = Ok(());
            self.core.as_mut().run_frame();
            if let Err(_) = self.state.borrow().as_ref().unwrap().result {
                let state = self.state.take().unwrap();
                return Err(state.result.unwrap_err());
            }
        }

        let state = self.state.take().unwrap();
        Ok((
            state.committed_state.unwrap(),
            state.dirty_state.unwrap(),
            state.input_pairs.back().unwrap().clone(),
        ))
    }
}
