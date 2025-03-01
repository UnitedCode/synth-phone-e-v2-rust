#![cfg_attr(not(test), no_std)]

/// The top-level states:
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuState {
    Volume,
    Key,
    SubMenu,
    Octave,
    None,
    // Sub menu states
    DryWet,
    Speed,
    Effect,
    Magnitude,
    Crush,
}
/// The effect states:
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EffectState {
    PassThrough,
    Autotune,
    Vocoder,
}

/// The top-level events that external code can trigger:
#[derive(Debug)]
pub enum MenuEvent {
    GoToVolume,
    GoToKey,
    GoToSubMenu,
    GoToOctave,
    Adjust(i32),
    SetNote(i32),
    Select,
    Return,
    NoOp
}

/// The top-level state machine.
/// Notice that `SubMenuStateMachine` and `EffectStateMachine` are *not* exposed.
pub struct MenuStateMachine {
    current_state: MenuState,
    volume: i32,
    key: i32,
    octave: i32,
    note: i32,

    // Nested machine is private:
    sub_menu: i32,
    sub_menu_selected: bool,

    pub dry_wet: i32,
    pub speed: i32,
    pub effect: i32,
    pub magnitude: i32,
    pub crush: i32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MenuStateMachineSnapshot {
    pub current_state: MenuState,
    pub volume: i32,
    pub key: i32,
    pub octave: i32,
    pub note: i32,

    // Nested machine is private:
    pub sub_menu: i32,
    pub sub_menu_selected: bool,
    pub dry_wet: i32,
    pub speed: i32,
    pub effect: i32,
    pub magnitude: i32,
    pub crush: i32,
}

pub const MENU_ITEMS_LENGTH: usize = 5;

pub struct SubMenuContext {
    pub previous_item: (&'static str, i32),
    pub current_item: (&'static str, i32),
    pub next_item: (&'static str, i32),
}

impl MenuStateMachine {
    /// Constructor
    pub fn new() -> Self {
        Self {
            current_state: MenuState::Volume,
            note: 0,
            volume: 50,
            key: 0,
            octave: 4,
            sub_menu: 0,
            sub_menu_selected: false,
        
            dry_wet: 0,
            speed: 31,
            effect: 0,
            magnitude: 05,
            crush: 31,
        }
    }

    pub fn current(&self) -> SubMenuContext {
        let menu_order: [(&str, i32); MENU_ITEMS_LENGTH] = [
            get_menu_item_details(MenuState::DryWet, self),
            get_menu_item_details(MenuState::Speed, self),
            get_menu_item_details(MenuState::Effect, self),
            get_menu_item_details(MenuState::Magnitude, self),
            get_menu_item_details(MenuState::Crush, self)
            ];
        
        let total_items = menu_order.len();
        let current = self.sub_menu as usize;
        let previous = if current == 0 {
            total_items - 1
        } else {
            current - 1
        };
        
        let next = (current + 1) % total_items;
        SubMenuContext {
            previous_item: menu_order[previous],
            current_item: menu_order[current],
            next_item: menu_order[next],
        }
    }

    /// The single snapshot method that collects *all* necessary info
    /// from top-level, sub-menu, and effect machines.
    pub fn snapshot(&self) -> MenuStateMachineSnapshot {
        MenuStateMachineSnapshot {
            current_state: self.current_state,
            volume: self.volume,
            key: self.key,
            octave: self.octave,
            note: self.note,

            sub_menu: self.sub_menu,
            sub_menu_selected: self.sub_menu_selected,
            dry_wet: self.dry_wet,
            speed: self.speed,
            effect: self.effect,
            magnitude: self.magnitude,
            crush: self.crush,
       
        }
    }

    /// Public event handler. This is how external code drives the machine.
    pub fn handle_event(&mut self, event: MenuEvent) {
        match event {
            MenuEvent::GoToVolume => {
                self.current_state = MenuState::Volume;
            }
            MenuEvent::GoToKey => {
                self.current_state = MenuState::Key;
            }
            MenuEvent::GoToSubMenu => {
                self.current_state = MenuState::SubMenu;
            }
            MenuEvent::GoToOctave => {
                self.current_state = MenuState::Octave;
            }
            MenuEvent::Adjust(delta) => match self.current_state {
                MenuState::Volume => {
                    self.volume = clamp_value(self.volume, delta, 1, 100);
                }
                MenuState::Key => {
                    self.key = wrap_value(self.key, delta, 0, 24);
                }
                MenuState::Octave => {
                    self.octave = clamp_value(self.octave, delta, 0, 8);
                }
                MenuState::SubMenu => {
                    self.sub_menu = wrap_value(self.sub_menu, delta, 0, (MENU_ITEMS_LENGTH -1) as i32);

                }
                MenuState::None => (),
                MenuState::DryWet => {
                    self.dry_wet = clamp_value(self.dry_wet, delta, 0, 100)
                },
                MenuState::Speed => {
                    self.speed = clamp_value(self.speed, delta, 6, 31)
                },
                MenuState::Effect => {
                    self.effect = clamp_value(self.effect, delta, 0, 100)
                },
                MenuState::Magnitude => {
                    self.magnitude = clamp_value(self.magnitude, delta, 1, 1000)
                },
                MenuState::Crush => {
                    self.crush = clamp_value(self.crush, delta, 4, 32)
                },
            },
            MenuEvent::SetNote(note) => {
                self.note = note;
            },
            MenuEvent::NoOp => (),
            MenuEvent::Select => {
                match self.current_state {
                    MenuState::SubMenu => {
                        match self.sub_menu {
                            0 => self.current_state = MenuState::DryWet,
                            1 => self.current_state = MenuState::Speed,
                            2 => self.current_state = MenuState::Effect,
                            3 => self.current_state = MenuState::Magnitude,
                            4 => self.current_state = MenuState::Crush,
                            _ => self.current_state = MenuState::DryWet,
                            
                        }
                    },
                    MenuState::Volume | 
                    MenuState::Key |
                    MenuState::Octave |
                    MenuState::None => {}
                    MenuState::DryWet |
                    MenuState::Speed |
                    MenuState::Magnitude | 
                    MenuState::Crush |                   
                    MenuState::Effect => {
                        self.current_state = MenuState::SubMenu
                    },
                }
            },
            MenuEvent::Return => {
                self.handle_event(MenuEvent::GoToVolume);
            },
        }
    }
}


/// A small helper function to clamp an i32.
fn clamp_value(current: i32, delta: i32, min: i32, max: i32) -> i32 {
    (current + delta).clamp(min, max)
}
fn wrap_value(current: i32, delta: i32, min: i32, max: i32) -> i32 {
    let range = max - min + 1;
    ((current + delta - min) % range + range) % range + min
}
pub fn get_menu_item_details(menu_state:MenuState, msm: &MenuStateMachine) -> (&'static str, i32){
    match menu_state {
        MenuState::Volume => ("Vol", msm.volume),
        MenuState::Key => ("Key", msm.key),
        MenuState::SubMenu => ("Sub", msm.sub_menu),
        MenuState::Octave => ("Oct", msm.octave),
        MenuState::None => ("None", 0),
        MenuState::DryWet => ("Dry/Wet", msm.dry_wet),
        MenuState::Speed => ("Speed", msm.speed),
        MenuState::Effect => ("Effect", msm.effect),
        MenuState::Magnitude => ("Magnitude", msm.magnitude),
        MenuState::Crush => ("Crush", msm.crush),
    }
}

/// ============  4. Example Usage or Tests  ============

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_snapshot() {
    //     let mut fsm = MenuStateMachine::new();
    //     // Initially, let's see the snapshot
    //     let snap1 = fsm.snapshot();
    //     assert_eq!(snap1.menu_state, MenuState::Volume);
    //     assert_eq!(snap1.volume, 50);
    //     assert_eq!(snap1.sub_menu_state, SubMenuState::None);

    //     // Move to SubMenu
    //     fsm.handle_event(MenuEvent::GoToSubMenu);
    //     let snap2 = fsm.snapshot();
    //     assert_eq!(snap2.menu_state, MenuState::SubMenu);
    //     assert_eq!(snap2.sub_menu_state, SubMenuState::None);

    //     // Adjust => forwarded to SubMenu (SubMenuState::None => no change)
    //     fsm.handle_event(MenuEvent::Adjust(10));
    //     let snap3 = fsm.snapshot();
    //     assert_eq!(snap3.dry_wet, 0);  // No effect, because we're in SubMenuState::None

    //     // Select => cycles sub-menu state from None => DryWet
    //     fsm.handle_event(MenuEvent::Select);
    //     let snap4 = fsm.snapshot();
    //     assert_eq!(snap4.sub_menu_state, SubMenuState::DryWet);

    //     // Adjust now affects 'dry_wet'
    //     fsm.handle_event(MenuEvent::Adjust(40));
    //     let snap5 = fsm.snapshot();
    //     assert_eq!(snap5.dry_wet, 40);

    //     // Another Select => cycles DryWet => Speed
    //     fsm.handle_event(MenuEvent::Select);
    //     let snap6 = fsm.snapshot();
    //     assert_eq!(snap6.sub_menu_state, SubMenuState::Speed);

    //     // Adjust => now affects 'speed'
    //     fsm.handle_event(MenuEvent::Adjust(15));
    //     let snap7 = fsm.snapshot();
    //     assert_eq!(snap7.speed, 15);

    //     // Another Select => cycles Speed => Effect
    //     fsm.handle_event(MenuEvent::Select);
    //     let snap8 = fsm.snapshot();
    //     assert_eq!(snap8.sub_menu_state, SubMenuState::Effect);
    //     assert_eq!(snap8.effect_state, EffectState::PassThrough);

    //     // Adjust => cycles effect forward
    //     fsm.handle_event(MenuEvent::Adjust(1));
    //     let snap9 = fsm.snapshot();
    //     assert_eq!(snap9.effect_state, EffectState::Autotune);

    //     // Return => sub menu returns to 'None', top-level returns to 'Volume'
    //     fsm.handle_event(MenuEvent::Return);
    //     let snap10 = fsm.snapshot();
    //     assert_eq!(snap10.menu_state, MenuState::Volume);
    //     assert_eq!(snap10.sub_menu_state, SubMenuState::None);
    // }

    #[test]
    fn test_crush_adjustment() {
        let mut fsm = MenuStateMachine::new();
        // Initially, the crush value is set to 32 by the constructor.
        assert_eq!(fsm.crush, 32);

        // Force the state machine into the SubMenu.
        fsm.handle_event(MenuEvent::GoToSubMenu);
        // Manually set sub_menu so that a Select event will choose Crush.
        // (According to our match, a sub_menu value of 5 will map to MenuState::Crush.)
        fsm.sub_menu = 5;
        fsm.handle_event(MenuEvent::Select);
        assert_eq!(fsm.current_state, MenuState::Crush);

        // Now test adjusting the crush value.
        // The crush value should be clamped between 4 and 32.
        // Try decreasing by 10. From 32, it should become 22.
        fsm.handle_event(MenuEvent::Adjust(-10));
        assert_eq!(fsm.crush, 22);

        // Try decreasing by another 20.
        // 22 - 20 = 2, but since the minimum is 4, it should clamp to 4.
        fsm.handle_event(MenuEvent::Adjust(-20));
        assert_eq!(fsm.crush, 4);

        // Try increasing by 100.
        // 4 + 100 = 104, but maximum allowed is 32 so it should clamp to 32.
        fsm.handle_event(MenuEvent::Adjust(100));
        assert_eq!(fsm.crush, 32);
    }

}
