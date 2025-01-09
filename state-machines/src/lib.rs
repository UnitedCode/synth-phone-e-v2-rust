#![cfg_attr(not(test), no_std)]

/// The top-level states:
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuState {
    Volume,
    Key,
    SubMenu,
    Octave,
}

/// The sub-menu states:
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SubMenuState {
    None,
    DryWet,
    Speed,
    Effect,
}

/// The effect states:
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EffectState {
    PassThrough,
    Autotune,
    Vocoder,
}

/// The **public** "snapshot" struct. 
/// This is how external code sees the complete state, 
/// *without* knowing about the nested machines inside.
#[derive(Debug)]
pub struct AllStatesSnapshot {
    pub menu_state: MenuState,
    pub volume: i32,
    pub key: i32,
    pub octave: i32,
    pub note: i32,

    pub sub_menu_state: SubMenuState,
    pub dry_wet: i32,
    pub speed: i32,

    pub effect_state: EffectState,
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
    pub note: i32,

    // Nested machine is private:
    sub_menu: SubMenuStateMachine,
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
            sub_menu: SubMenuStateMachine::new(),
        }
    }

    /// The single snapshot method that collects *all* necessary info
    /// from top-level, sub-menu, and effect machines.
    pub fn snapshot(&self) -> AllStatesSnapshot {
        AllStatesSnapshot {
            menu_state: self.current_state,
            volume: self.volume,
            key: self.key,
            octave: self.octave,
            note: self.note,

            sub_menu_state: self.sub_menu.current_state,
            dry_wet: self.sub_menu.dry_wet,
            speed: self.sub_menu.speed,

            effect_state: self.sub_menu.effect_machine.current_effect,
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
                    self.key = clamp_value(self.key, delta, 0, 24);
                }
                MenuState::Octave => {
                    self.octave = clamp_value(self.octave, delta, 0, 8);
                }
                MenuState::SubMenu => {
                    // Forward to the sub-menu's event handler
                    self.sub_menu.handle_submenu_event(SubMenuEvent::Adjust(delta));
                }
            },
            MenuEvent::Select => {
                match self.current_state {
                    MenuState::SubMenu => {
                        self.sub_menu.handle_submenu_event(SubMenuEvent::Select);
                    }
                    _ => {
                        // Possibly confirm or do nothing in other states
                    }
                }
            }
            MenuEvent::Return => {
                // Maybe if the user is in the sub-menu, we return to Volume?
                if self.current_state == MenuState::SubMenu {
                    self.sub_menu.handle_submenu_event(SubMenuEvent::Return);
                    self.current_state = MenuState::Volume;
                }
            }
            MenuEvent::SetNote(note) => {
                self.note = note;
            },
            MenuEvent::NoOp => (),
        }
    }
}

/// ============  2. Private Sub-Menu Machine  ============
/// This is *not* exposed outside the module.
/// It's only known to `MenuStateMachine`.
///
/// We define a minimal set of events for the sub-menu.
#[derive(Debug)]
enum SubMenuEvent {
    Adjust(i32),
    Select,
    Return,
}

/// Private sub-menu machine
struct SubMenuStateMachine {
    current_state: SubMenuState,
    dry_wet: i32,
    speed: i32,

    // Nested effect machine:
    effect_machine: EffectStateMachine,
}

impl SubMenuStateMachine {
    fn new() -> Self {
        Self {
            current_state: SubMenuState::None,
            dry_wet: 0,
            speed: 0,
            effect_machine: EffectStateMachine::new(),
        }
    }

    fn handle_submenu_event(&mut self, event: SubMenuEvent) {
        match event {
            SubMenuEvent::Adjust(delta) => {
                match self.current_state {
                    SubMenuState::None => {
                        // no-op
                    }
                    SubMenuState::DryWet => {
                        self.dry_wet = clamp_value(self.dry_wet, delta, 0, 100);
                    }
                    SubMenuState::Speed => {
                        self.speed = clamp_value(self.speed, delta, 0, 100);
                    }
                    SubMenuState::Effect => {
                        // Forward effect adjustments (cycle next/previous, etc.)
                        if delta > 0 {
                            self.effect_machine.handle_effect_event(EffectEvent::CycleNext);
                        } else if delta < 0 {
                            self.effect_machine.handle_effect_event(EffectEvent::CyclePrevious);
                        }
                    }
                }
            }
            SubMenuEvent::Select => {
                // Possibly finalize or switch states. 
                // For demonstration, let's cycle states:
                self.current_state = match self.current_state {
                    SubMenuState::None => SubMenuState::DryWet,
                    SubMenuState::DryWet => SubMenuState::Speed,
                    SubMenuState::Speed => SubMenuState::Effect,
                    SubMenuState::Effect => SubMenuState::None,
                };
            }
            SubMenuEvent::Return => {
                // Return to "None" (or keep current)
                self.current_state = SubMenuState::None;
            }
        }
    }
}

/// ============  3. Private Effect Machine  ============
/// Also not visible outside this module.
#[derive(Debug)]
enum EffectEvent {
    CycleNext,
    CyclePrevious,
}

struct EffectStateMachine {
    current_effect: EffectState,
}

impl EffectStateMachine {
    fn new() -> Self {
        Self {
            current_effect: EffectState::PassThrough,
        }
    }

    fn handle_effect_event(&mut self, event: EffectEvent) {
        match event {
            EffectEvent::CycleNext => {
                self.current_effect = match self.current_effect {
                    EffectState::PassThrough => EffectState::Autotune,
                    EffectState::Autotune => EffectState::Vocoder,
                    EffectState::Vocoder => EffectState::PassThrough,
                };
            }
            EffectEvent::CyclePrevious => {
                self.current_effect = match self.current_effect {
                    EffectState::PassThrough => EffectState::Vocoder,
                    EffectState::Autotune => EffectState::PassThrough,
                    EffectState::Vocoder => EffectState::Autotune,
                };
            }
        }
    }
}

/// A small helper function to clamp an i32.
fn clamp_value(current: i32, delta: i32, min: i32, max: i32) -> i32 {
    (current + delta).clamp(min, max)
}

/// ============  4. Example Usage or Tests  ============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot() {
        let mut fsm = MenuStateMachine::new();
        // Initially, let's see the snapshot
        let snap1 = fsm.snapshot();
        assert_eq!(snap1.menu_state, MenuState::Volume);
        assert_eq!(snap1.volume, 50);
        assert_eq!(snap1.sub_menu_state, SubMenuState::None);

        // Move to SubMenu
        fsm.handle_event(MenuEvent::GoToSubMenu);
        let snap2 = fsm.snapshot();
        assert_eq!(snap2.menu_state, MenuState::SubMenu);
        assert_eq!(snap2.sub_menu_state, SubMenuState::None);

        // Adjust => forwarded to SubMenu (SubMenuState::None => no change)
        fsm.handle_event(MenuEvent::Adjust(10));
        let snap3 = fsm.snapshot();
        assert_eq!(snap3.dry_wet, 0);  // No effect, because we're in SubMenuState::None

        // Select => cycles sub-menu state from None => DryWet
        fsm.handle_event(MenuEvent::Select);
        let snap4 = fsm.snapshot();
        assert_eq!(snap4.sub_menu_state, SubMenuState::DryWet);

        // Adjust now affects 'dry_wet'
        fsm.handle_event(MenuEvent::Adjust(40));
        let snap5 = fsm.snapshot();
        assert_eq!(snap5.dry_wet, 40);

        // Another Select => cycles DryWet => Speed
        fsm.handle_event(MenuEvent::Select);
        let snap6 = fsm.snapshot();
        assert_eq!(snap6.sub_menu_state, SubMenuState::Speed);

        // Adjust => now affects 'speed'
        fsm.handle_event(MenuEvent::Adjust(15));
        let snap7 = fsm.snapshot();
        assert_eq!(snap7.speed, 15);

        // Another Select => cycles Speed => Effect
        fsm.handle_event(MenuEvent::Select);
        let snap8 = fsm.snapshot();
        assert_eq!(snap8.sub_menu_state, SubMenuState::Effect);
        assert_eq!(snap8.effect_state, EffectState::PassThrough);

        // Adjust => cycles effect forward
        fsm.handle_event(MenuEvent::Adjust(1));
        let snap9 = fsm.snapshot();
        assert_eq!(snap9.effect_state, EffectState::Autotune);

        // Return => sub menu returns to 'None', top-level returns to 'Volume'
        fsm.handle_event(MenuEvent::Return);
        let snap10 = fsm.snapshot();
        assert_eq!(snap10.menu_state, MenuState::Volume);
        assert_eq!(snap10.sub_menu_state, SubMenuState::None);
    }
}
