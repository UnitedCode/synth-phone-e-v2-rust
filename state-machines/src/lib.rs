#![cfg_attr(not(test), no_std)]

/// The top-level states:

// New state machine structure only
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProcessingProfile {
    Autotune,
    Vocode,
    Dry,
}

/// The top-level states
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AppState {
    Splash,
    Processing(ProcessingProfile),
    EffectsProfile(ProcessingProfile),
    Menu(MenuState, ProcessingProfile),
}

/// Menu state with selection/editing profiles
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuState {
    Selecting(usize), // Index of the current menu item
    Editing(usize),   // Index of the item being edited
}

/// Menu items available for adjustment
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuItem {
    BitRate1,
    BitRate2,
    SampleRate1,
    SampleRate2,
    FormantMale,
    FormantFemale,
    AutotuneSpeed,
    Magnitude,
    PadMatrix,
}

/// Array of all menu items for iteration
pub const MENU_ITEMS: [MenuItem; 9] = [
    MenuItem::BitRate1,
    MenuItem::BitRate2,
    MenuItem::SampleRate1,
    MenuItem::SampleRate2,
    MenuItem::FormantMale,
    MenuItem::FormantFemale,
    MenuItem::AutotuneSpeed,
    MenuItem::Magnitude,
    MenuItem::PadMatrix,
];

/// Storage for all adjustable menu values
#[derive(Debug, Clone, Copy)]
pub struct MenuValues {
    pub bit_rate_1: i32,
    pub bit_rate_2: i32,
    pub sample_rate_1: i32,
    pub sample_rate_2: i32,
    pub formant_male: i32,
    pub formant_female: i32,
    pub autotune_speed: i32,
    pub magnitude: i32,
    pub pad_matrix: i32,
}

impl Default for MenuValues {
    fn default() -> Self {
        Self {
            bit_rate_1: 32,
            bit_rate_2: 10,
            sample_rate_1: 5,
            sample_rate_2: 16,
            formant_male: 5,
            formant_female: 5,
            autotune_speed: 5,
            magnitude: 5,
            pad_matrix: 0,
        }
    }
}

use log::{info, warn};

impl MenuValues {
    /// Get a value for a specific menu item
    pub fn get(&self, item: MenuItem) -> i32 {
        match item {
            MenuItem::BitRate1 => self.bit_rate_1,
            MenuItem::BitRate2 => self.bit_rate_2,
            MenuItem::SampleRate1 => self.sample_rate_1,
            MenuItem::SampleRate2 => self.sample_rate_2,
            MenuItem::FormantMale => self.formant_male,
            MenuItem::FormantFemale => self.formant_female,
            MenuItem::AutotuneSpeed => self.autotune_speed,
            MenuItem::Magnitude => self.magnitude,
            MenuItem::PadMatrix => self.pad_matrix,
        }
    }

    /// Set a value for a specific menu item (with appropriate clamping)
    pub fn set(&mut self, item: MenuItem, value: i32) {
        match item {
            MenuItem::BitRate1 => self.bit_rate_1 = value.clamp(4, 32),
            MenuItem::BitRate2 => self.bit_rate_2 = value.clamp(4, 32),
            MenuItem::SampleRate1 => self.sample_rate_1 = value.clamp(1, 32),
            MenuItem::SampleRate2 => self.sample_rate_2 = value.clamp(1, 32),
            MenuItem::FormantMale => self.formant_male = value.clamp(1, 10),
            MenuItem::FormantFemale => self.formant_female = value.clamp(1, 10),
            MenuItem::AutotuneSpeed => self.autotune_speed = value.clamp(1, 10),
            MenuItem::Magnitude => self.magnitude = value.clamp(1, 10),
            MenuItem::PadMatrix => self.pad_matrix = value.clamp(0, 1),
        }
    }

    /// Get a user-friendly name for a menu item
    pub fn get_item_name(item: MenuItem) -> &'static str {
        match item {
            MenuItem::BitRate1 => "Bit Rate 1",
            MenuItem::BitRate2 => "Bit Rate 2",
            MenuItem::SampleRate1 => "Sample Rate 1",
            MenuItem::SampleRate2 => "Sample Rate 2",
            MenuItem::FormantMale => "Formant Male",
            MenuItem::FormantFemale => "Formant Female",
            MenuItem::AutotuneSpeed => "Autotune Speed",
            MenuItem::Magnitude => "Magnitude",
            MenuItem::PadMatrix => "Pad Matrix",
        }
    }
}

/// Events that can be triggered by hardware inputs
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AppEvent {
    // General events
    SplashComplete,     // Splash screen animation finished
    EncoderPress,       // Single press of the encoder
    EncoderDoublePress, // Double press of the encoder
    EncoderRotate(i32), // Rotation of the encoder (positive or negative)

    // Button presses in different profiles
    KeypadPress(usize),   // Press of a keypad button (1-12)
    KeypadRelease(usize), // Release of a keypad button (1-12)

    // Processing profile selection
    CycleProcessingProfile, // Cycle to next processing profile
    SetProcessingProfile(ProcessingProfile), // Set a specific processing profile

    // Effects parameters
    SetOctave(i32),   // Set octave (low=-1, normal=0, high=1)
    SetBitCrush(i32), // Set bit crush (1=crush1, 0=none, 2=crush2)
    SetFormant(i32),  // Set formant (-1=male, 0=none, 1=female)
    KeyChange(i32),   // Change musical key up or down

    NoOp,
}

impl AppState {
    /// Create a new state machine starting at the splash screen
    pub fn new() -> Self {
        AppState::Splash
    }

    /// Handle a state transition based on an event
    pub fn transition(self, event: AppEvent) -> Self {
        match (self, event) {
            // From Splash screen
            (AppState::Splash, AppEvent::SplashComplete) => {
                AppState::Processing(ProcessingProfile::Autotune)
            }

            // Encoder press to toggle between Processing and Effects
            (AppState::Processing(profile), AppEvent::EncoderPress) => {
                AppState::EffectsProfile(profile)
            }
            (AppState::EffectsProfile(profile), AppEvent::EncoderPress) => {
                AppState::Processing(profile)
            }

            // Double press to enter menu from anywhere
            (AppState::Processing(profile), AppEvent::EncoderDoublePress) => {
                AppState::Menu(MenuState::Selecting(0), profile)
            }
            (AppState::EffectsProfile(profile), AppEvent::EncoderDoublePress) => {
                AppState::Menu(MenuState::Selecting(0), profile)
            }

            // Double press to exit menu
            (AppState::Menu(_, profile), AppEvent::EncoderDoublePress) => {
                AppState::Processing(profile)
            }

            // Handle profile changes in Effects
            (AppState::EffectsProfile(_), AppEvent::CycleProcessingProfile) => {
                // Implemented separately in cycle_profile function
                self
            }
            (AppState::EffectsProfile(_), AppEvent::SetProcessingProfile(new_profile)) => {
                AppState::Processing(new_profile)
            }

            // Stay in current state for all other events
            _ => self,
        }
    }

    /// Helper function to cycle through processing profiles
    pub fn cycle_profile(&self) -> Self {
        match self {
            AppState::EffectsProfile(ProcessingProfile::Autotune) => {
                AppState::EffectsProfile(ProcessingProfile::Vocode)
            }
            AppState::EffectsProfile(ProcessingProfile::Vocode) => {
                AppState::EffectsProfile(ProcessingProfile::Dry)
            }
            AppState::EffectsProfile(ProcessingProfile::Dry) => {
                AppState::EffectsProfile(ProcessingProfile::Autotune)
            }
            AppState::Processing(ProcessingProfile::Autotune) => {
                AppState::Processing(ProcessingProfile::Vocode)
            }
            AppState::Processing(ProcessingProfile::Vocode) => {
                AppState::Processing(ProcessingProfile::Dry)
            }
            AppState::Processing(ProcessingProfile::Dry) => {
                AppState::Processing(ProcessingProfile::Autotune)
            }
            // For any other state, don't change
            _ => *self,
        }
    }
}

/// Events for navigating the menu
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuNavEvent {
    Next,
    Prev,
    Select,
}

impl MenuState {
    /// Handle menu navigation and editing
    pub fn handle_event(&self, event: MenuNavEvent, values: &mut MenuValues) -> MenuState {
        match (self, event) {
            // Navigation in selection profile
            (MenuState::Selecting(idx), MenuNavEvent::Next) => {
                MenuState::Selecting((idx + 1) % MENU_ITEMS.len())
            }
            (MenuState::Selecting(idx), MenuNavEvent::Prev) => {
                MenuState::Selecting((idx + MENU_ITEMS.len() - 1) % MENU_ITEMS.len())
            }
            (MenuState::Selecting(idx), MenuNavEvent::Select) => MenuState::Editing(*idx),

            // Value adjustment in editing profile
            (MenuState::Editing(idx), MenuNavEvent::Next) => {
                let item = MENU_ITEMS[*idx];
                let val = values.get(item);
                values.set(item, val + 1);
                MenuState::Editing(*idx)
            }
            (MenuState::Editing(idx), MenuNavEvent::Prev) => {
                let item = MENU_ITEMS[*idx];
                let val = values.get(item);
                values.set(item, val - 1);
                MenuState::Editing(*idx)
            }
            (MenuState::Editing(idx), MenuNavEvent::Select) => MenuState::Selecting(*idx),
        }
    }
}

/// Main state machine for the telephone effects box
pub struct AppStateMachine {
    state: AppState,
    values: MenuValues,
    current_key: i32,
    current_octave: i32,
    current_bitcrush: i32,
    current_formant: i32, 
    sample_rate: i32,
    bit_rate: i32,
    pub volume: i32,
    pub note: i32,
    pub key_down_pressed: bool,
    pub process_cycle_pressed: bool,
    pub key_up_pressed: bool,
}

// Add a snapshot struct to hold all the state information
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AppStateMachineSnapshot {
    pub current_state: AppState,
    pub key: i32,
    pub octave: i32,
    pub note: i32,
    pub volume: i32,
    pub crush: i32,
    pub sample_rate: i32,
    pub bit_rate: i32,
    pub formant: i32,
    pub autotune_speed: i32,
    pub magnitude: i32,
    pub pad_matrix: i32,
    pub key_down_pressed: bool,
    pub process_cycle_pressed: bool,
    pub key_up_pressed: bool,
}

// Add a MenuContext struct for menu display
pub struct MenuContext {
    pub previous_item: (&'static str, i32),
    pub current_item: (&'static str, i32),
    pub next_item: (&'static str, i32),
}

impl AppStateMachine {
    /// Create a new instance of the state machine
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            values: MenuValues::default(),
            current_key: 0,
            current_octave: 2,
            current_bitcrush: 0,
            current_formant: 0,
            sample_rate: 32,
            bit_rate: 32,
            volume: 0,
            note: 0,
            key_down_pressed: false,
            process_cycle_pressed: false,
            key_up_pressed: false,
        }
    }

    /// Get the current state
    pub fn state(&self) -> AppState {
        self.state
    }

    // Add a snapshot method to provide current state info
    pub fn snapshot(&self) -> AppStateMachineSnapshot {
        AppStateMachineSnapshot {
            current_state: self.state,
            key: self.current_key,
            octave: self.current_octave,
            note: self.note,
            volume: self.volume,
            crush: self.current_bitcrush,
            sample_rate: self.sample_rate,
            bit_rate: self.bit_rate,
            formant: self.current_formant,
            autotune_speed: self.values.autotune_speed,
            magnitude: self.values.magnitude,
            pad_matrix: self.values.pad_matrix,
            key_down_pressed: self.key_down_pressed,
            process_cycle_pressed: self.process_cycle_pressed,
            key_up_pressed: self.key_up_pressed,
        }
    }

    /// Handle incoming events
    pub fn handle_event(&mut self, event: AppEvent) {
        // Handle special events for specific states
        match (&self.state, event) {
            //adjust volume
            (AppState::Processing(_), AppEvent::EncoderRotate(delta))
            | (AppState::EffectsProfile(_), AppEvent::EncoderRotate(delta)) => {
                self.volume = clamp_value(self.volume, delta, 0, 10);
            }

            // Handle keypad presses in Processing profile (for notes)
            (AppState::Processing(_), AppEvent::KeypadPress(key)) => {
                match key {
                    1..=9 => {
                        // First 9 buttons are notes
                        self.play_note(key);
                        self.note = key as i32;
                    }
                    10 => self.current_key = (self.current_key + 23) % 24,
                    11 => self.state = self.state.cycle_profile(),
                    12 => self.current_key = (self.current_key + 1) % 24,
                    _ => {}
                }
            }

            // Handle keypad presses in Effects profile
            (AppState::EffectsProfile(_), AppEvent::KeypadPress(key)) => {
                match key {
                    // Row 1: Octave controls
                    1 => self.current_octave = 1, // Low
                    2 => self.current_octave = 2, // Normal
                    3 => self.current_octave = 4, // High

                    // Row 2: Bit crush controls
                    4 => {
                        self.current_bitcrush = 1;
                        self.bit_rate = self.values.bit_rate_1;
                        self.sample_rate = self.values.sample_rate_1;
                    },
                    5 => {
                        self.current_bitcrush = 0;
                        self.bit_rate = 32;
                        self.sample_rate = 1;
                    },
                    6 => {
                        self.current_bitcrush = 2;
                        self.bit_rate = self.values.bit_rate_2;
                        self.sample_rate = self.values.sample_rate_2;
                    },

                    // Row 3: Formant controls
                    7 => self.current_formant = 1, // Male
                    8 => self.current_formant = 0, // None
                    9 => self.current_formant = 2, // Female

                    // Row 4: Key and profile controls (keys 10-12)
                    10 => {
                        self.key_down_pressed = true;
                        self.current_key = (self.current_key + 23) % 24; // Key down
                    }
                    11 => {
                        self.process_cycle_pressed = true;
                        self.state = self.state.cycle_profile(); // Cycle profile
                    }
                    12 => {
                        self.key_up_pressed = true;
                        self.current_key = (self.current_key + 1) % 24; // Key up
                    }

                    _ => {
                        self.key_down_pressed = false;
                        self.process_cycle_pressed = false;
                        self.key_up_pressed = false;
                    } // Key Released
                }
            }

            (AppState::EffectsProfile(_), AppEvent::KeypadRelease(key)) => match key {
                10 => self.key_down_pressed = false,
                11 => self.process_cycle_pressed = false,
                12 => self.key_up_pressed = false,
                _ => {}
            },

            (AppState::Processing(_), AppEvent::KeypadRelease(key)) => match key {
                _ => self.play_note(0),
            },

            // Handle encoder rotation in Menu profile
            (AppState::Menu(menu_state, _), AppEvent::EncoderRotate(delta)) => {
                // Convert rotation to next/prev events
                let nav_event = if delta > 0 {
                    MenuNavEvent::Next
                } else {
                    MenuNavEvent::Prev
                };

                // Update menu state
                let new_menu_state = menu_state.handle_event(nav_event, &mut self.values);
                if let AppState::Menu(_, profile) = self.state {
                    self.state = AppState::Menu(new_menu_state, profile);
                }

                return; // Skip the standard transition
            }

            // Handle encoder press in Menu profile
            (AppState::Menu(menu_state, _), AppEvent::EncoderPress) => {
                // Toggle between selecting and editing
                let new_menu_state =
                    menu_state.handle_event(MenuNavEvent::Select, &mut self.values);
                if let AppState::Menu(_, profile) = self.state {
                    self.state = AppState::Menu(new_menu_state, profile);
                }

                return; // Skip the standard transition
            }

            _ => (), // Other events handled by standard transition
        }

        // Standard state transition
        self.state = self.state.transition(event);
    }

    // Add a current method to get menu context
    pub fn current(&self) -> MenuContext {
        let idx = self.active_menu_index().unwrap_or(0);
        let menu_items = [
            ("BitRate1", self.values.bit_rate_1),
            ("BitRate2", self.values.bit_rate_2),
            ("SampleRate1", self.values.sample_rate_1),
            ("SampleRate2", self.values.sample_rate_2),
            ("FormantMale", self.values.formant_male),
            ("FormantFemale", self.values.formant_female),
            ("Speed", self.values.autotune_speed),
            ("Magnitude", self.values.magnitude),
            ("PadMatrix", self.values.pad_matrix),
        ];

        let total = menu_items.len();

        MenuContext {
            previous_item: menu_items[(idx + total - 1) % total],
            current_item: menu_items[idx],
            next_item: menu_items[(idx + 1) % total],
        }
    }

    fn active_menu_index(&self) -> Option<usize> {
        if let AppState::Menu(ref menu_state, _) = self.state {
            Some(match menu_state {
                MenuState::Selecting(i) | MenuState::Editing(i) => *i,
            })
        } else {
            None
        }
    }

    /// Play a note based on the current key and octave
    fn play_note(&mut self, key_index: usize) {
        info!("the note before {}", key_index);
        // Convert keypad position to a note in the current key and octave
        // Implementation depends on your audio system
        // This is just a placeholder
        self.note = key_index as i32; //self.current_key + (key_index as i32) + (self.current_octave * 12);
                                      // Play the note with current effects
        info!("the note after {}", self.note);
    }

    /// Get a snapshot of all current values
    pub fn get_values(&self) -> MenuValues {
        self.values
    }

    // Get processing-related parameters
    // pub fn get_processing_params(&self) -> (i32, i32, i32, i32) {
    //     (
    //         self.current_key,
    //         self.current_octave,
    //         self.current_crush,
    //         self.current_formant,
    //     )
    // }
}

/// A small helper function to clamp an i32.
fn clamp_value(current: i32, delta: i32, min: i32, max: i32) -> i32 {
    (current + delta).clamp(min, max)
}
fn wrap_value(current: i32, delta: i32, min: i32, max: i32) -> i32 {
    let range = max - min + 1;
    ((current + delta - min) % range + range) % range + min
}

/// Unit tests for the state machine
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splash_to_processing() {
        let mut app = AppStateMachine::new();
        assert!(matches!(app.state(), AppState::Splash));

        app.handle_event(AppEvent::SplashComplete);
        assert!(matches!(
            app.state(),
            AppState::Processing(ProcessingProfile::Autotune)
        ));
    }

    #[test]
    fn test_toggle_effects() {
        let mut app = AppStateMachine::new();
        app.handle_event(AppEvent::SplashComplete);

        // Toggle to Effects
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::EffectsProfile(ProcessingProfile::Autotune)
        ));

        // Toggle back to Processing
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::Processing(ProcessingProfile::Autotune)
        ));
    }

    #[test]
    fn test_menu_navigation() {
        let mut app = AppStateMachine::new();
        app.handle_event(AppEvent::SplashComplete);

        // Enter menu
        app.handle_event(AppEvent::EncoderDoublePress);
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Selecting(0), _)
        ));

        // Rotate encoder to next item
        app.handle_event(AppEvent::EncoderRotate(1));
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Selecting(1), _)
        ));

        // Enter edit profile
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Editing(1), _)
        ));

        // Change value
        app.handle_event(AppEvent::EncoderRotate(1));
        let values = app.get_values();
        assert_eq!(values.crush2, 6); // Default was 5, now 6

        // Exit edit profile
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Selecting(1), _)
        ));

        // Exit menu
        app.handle_event(AppEvent::EncoderDoublePress);
        assert!(matches!(
            app.state(),
            AppState::Processing(ProcessingProfile::Autotune)
        ));
    }

    // #[test]
    // fn test_effects_controls() {
    //     let mut app = AppStateMachine::new();
    //     app.handle_event(AppEvent::SplashComplete);
    //     app.handle_event(AppEvent::EncoderPress); // Enter Effects

    //     // Test octave controls
    //     app.handle_event(AppEvent::KeypadPress(0)); // Low octave
    //     let (_, octave, _, _) = app.get_processing_params();
    //     assert_eq!(octave, -1);

    //     // Test bit crush controls
    //     app.handle_event(AppEvent::KeypadPress(5)); // Crush 2
    //     let (_, _, crush, _) = app.get_processing_params();
    //     assert_eq!(crush, 2);

    //     // Test formant controls
    //     app.handle_event(AppEvent::KeypadPress(6)); // Male formant
    //     let (_, _, _, formant) = app.get_processing_params();
    //     assert_eq!(formant, -1);

    //     // Test cycle profile
    //     app.handle_event(AppEvent::KeypadPress(10)); // Cycle profile
    //     assert!(matches!(
    //         app.state(),
    //         AppState::EffectsProfile(ProcessingProfile::Vocode)
    //     ));

    //     app.handle_event(AppEvent::KeypadPress(10)); // Cycle profile again
    //     assert!(matches!(
    //         app.state(),
    //         AppState::EffectsProfile(ProcessingProfile::Dry)
    //     ));

    //     app.handle_event(AppEvent::KeypadPress(10)); // Cycle profile again
    //     assert!(matches!(
    //         app.state(),
    //         AppState::EffectsProfile(ProcessingProfile::Autotune)
    //     ));
    // }
}
