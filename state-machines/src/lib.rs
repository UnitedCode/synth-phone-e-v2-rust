#![cfg_attr(not(test), no_std)]

#[derive(Debug, PartialEq)]
enum MenuState {
    Volume = 0,
    Key = 1,
    SubMenu = 2,
    Octave = 3,
    PitchBend = 4,
}

#[derive(Debug)]
enum MenuEvent {
    GoToKey,
    GoToMenuSelect,
    GoToOctave,
    ReturnToVolume,
    GotToPitchBend,
    Adjust(i32),
}

pub struct MenuStateMachine {
    pub current_state: MenuState,
    pub volume: i32,
    pub key: i32,
    pub menu_option: i32,
    pub octave: i32,
    pub pitch_bend: i32,
}

impl MenuStateMachine {
    pub fn new() -> Self {
        MenuStateMachine {
            current_state: MenuState::Volume,
            volume: 50,
            key: 0,
            menu_option: 0,
            octave: 4,
            pitch_bend: 0,
        }
    }

    pub fn handle_event(&mut self, event: MenuEvent) {
        match event {
            MenuEvent::GoToKey => {
                if self.current_state == MenuState::Volume {
                    self.current_state = MenuState::Key
                }
            }
            MenuEvent::GoToMenuSelect => {
                if self.current_state == MenuState::Volume {
                    self.current_state = MenuState::SubMenu
                }
            }
            MenuEvent::GoToOctave => {
                if self.current_state == MenuState::Volume {
                    self.current_state = MenuState::Octave
                }
            }
            MenuEvent::ReturnToVolume => self.current_state = MenuState::Volume,
            MenuEvent::GotToPitchBend => self.current_state = MenuState::PitchBend,
            MenuEvent::Adjust(value) => match self.current_state {
                MenuState::Volume => self.volume = (self.volume + value).clamp(0, 100),
                MenuState::Key => self.key = (self.key + value).clamp(0, 12),
                MenuState::SubMenu => self.menu_option = (self.menu_option + value).clamp(0, 5),
                MenuState::Octave => self.octave = (self.octave + value).clamp(0, 8),
                MenuState::PitchBend => self.pitch_bend = (self.pitch_bend + value).clamp(-12, 12),
            },
        }
    }
}

#[cfg(test)]
mod detect_fun_freq_tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let fsm = MenuStateMachine::new();
        assert_eq!(fsm.current_state, MenuState::Volume);
        assert_eq!(fsm.volume, 50);
        assert_eq!(fsm.key, 0);
        assert_eq!(fsm.menu_option, 0);
        assert_eq!(fsm.octave, 4);
    }

    #[test]
    fn test_transition_to_key() {
        let mut fsm: MenuStateMachine = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::GoToKey);
        assert_eq!(fsm.current_state, MenuState::Key);
    }

    #[test]
    fn test_transition_to_menu_select() {
        let mut fsm = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::GoToMenuSelect);
        assert_eq!(fsm.current_state, MenuState::SubMenu);
    }

    #[test]
    fn test_transition_to_octave() {
        let mut fsm = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::GoToOctave);
        assert_eq!(fsm.current_state, MenuState::Octave);
    }

    #[test]
    fn test_return_to_volume() {
        let mut fsm = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::GoToKey);
        fsm.handle_event(MenuEvent::ReturnToVolume);
        assert_eq!(fsm.current_state, MenuState::Volume);
    }

    #[test]
    fn test_adjust_volume() {
        let mut fsm = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::Adjust(20));
        assert_eq!(fsm.volume, 70);
        fsm.handle_event(MenuEvent::Adjust(-100));
        assert_eq!(fsm.volume, 0);
    }

    #[test]
    fn test_adjust_key() {
        let mut fsm = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::GoToKey);
        fsm.handle_event(MenuEvent::Adjust(5));
        assert_eq!(fsm.key, 5);
        fsm.handle_event(MenuEvent::Adjust(-20));
        assert_eq!(fsm.key, 0);
    }

    #[test]
    fn test_adjust_menu_option() {
        let mut fsm = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::GoToMenuSelect);
        fsm.handle_event(MenuEvent::Adjust(3));
        assert_eq!(fsm.menu_option, 3);
        fsm.handle_event(MenuEvent::Adjust(10));
        assert_eq!(fsm.menu_option, 5); // Clamped to max 5
    }

    #[test]
    fn test_adjust_octave() {
        let mut fsm = MenuStateMachine::new();
        fsm.handle_event(MenuEvent::GoToOctave);
        fsm.handle_event(MenuEvent::Adjust(2));
        assert_eq!(fsm.octave, 6);
        fsm.handle_event(MenuEvent::Adjust(-7));
        assert_eq!(fsm.octave, 0); // Clamped to min 0
    }
}
