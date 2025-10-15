/// The top-level states:

// New state machine structure only
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProcessingProfile {
    PitchControl,
    Vocode,
    Dry,
    Harmony,
    Phone,
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
// Macro to generate MenuItem enum and related implementations
macro_rules! menu_items {
    ($(
        $variant:ident => {
            field: $field:ident,
            name: $name:expr,
            min: $min:expr,
            max: $max:expr $(,)?
        }
    ),* $(,)?) => {
        // Generate the MenuItem enum
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum MenuItem {
            $($variant,)*
        }

        // Generate the MENU_ITEMS array
        pub const MENU_ITEMS: [MenuItem; menu_items!(@count $($variant)*)] = [
            $(MenuItem::$variant,)*
        ];

        impl MenuValues {
            /// Get a value for a specific menu item
            pub fn get(&self, item: MenuItem) -> i8 {
                match item {
                    $(MenuItem::$variant => self.$field,)*
                }
            }

            /// Set a value for a specific menu item (with appropriate clamping)
            pub fn set(&mut self, item: MenuItem, value: i8) {
                match item {
                    $(MenuItem::$variant => self.$field = value.clamp($min, $max),)*
                }
            }

            /// Get a user-friendly name for a menu item
            pub fn get_item_name(item: MenuItem) -> &'static str {
                match item {
                    $(MenuItem::$variant => $name,)*
                }
            }
        }
    };

    // Helper to count variants
    (@count) => { 0 };
    (@count $head:ident $($tail:ident)*) => { 1 + menu_items!(@count $($tail)*) };
}

// Define all menu items with their metadata in one place
menu_items! {
    BitRate1 => { field: bit_rate_soft, name: "Bit Rate 1", min: 4, max: 32 },
    BitRate2 => { field: bit_rate_harsh, name: "Bit Rate 2", min: 4, max: 32 },
    SampleRate1 => { field: sample_reduction_soft, name: "Sample Rate 1", min: 1, max: 32 },
    SampleRate2 => { field: sample_reduction_harsh, name: "Sample Rate 2", min: 1, max: 32 },
    FormantMale => { field: formant_male, name: "Formant Male", min: 1, max: 10 },
    FormantFemale => { field: formant_female, name: "Formant Female", min: 1, max: 10 },
    AutotuneSpeed => { field: autotune_speed, name: "Autotune Speed", min: 1, max: 10 },
    Magnitude => { field: magnitude, name: "Magnitude", min: 1, max: 10 },
    PadMatrix => { field: pad_matrix, name: "Pad Matrix", min: 0, max: 1 },
}

/// Storage for all adjustable menu values
#[derive(Debug, Clone, Copy)]
pub struct MenuValues {
    pub bit_rate_soft: i8,
    pub bit_rate_harsh: i8,
    pub sample_reduction_soft: i8,
    pub sample_reduction_harsh: i8,
    pub formant_male: i8,
    pub formant_female: i8,
    pub autotune_speed: i8,
    pub magnitude: i8,
    pub pad_matrix: i8,
}

impl Default for MenuValues {
    fn default() -> Self {
        Self {
            bit_rate_soft: 32,
            bit_rate_harsh: 10,
            sample_reduction_soft: 5,
            sample_reduction_harsh: 16,
            formant_male: 5,
            formant_female: 5,
            autotune_speed: 5,
            magnitude: 5,
            pad_matrix: 0,
        }
    }
}

use log::info;

/// Events that can be triggered by hardware inputs
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AppEvent {
    // General events
    SplashComplete,     // Splash screen animation finished
    EncoderPress,       // Single press of the encoder
    EncoderDoublePress, // Double press of the encoder
    EncoderRotate(i8),  // Rotation of the encoder (positive or negative)
    HangupPress,        // Hangup button press

    // Button presses in different profiles
    KeypadPress(usize),   // Press of a keypad button (1-12)
    KeypadRelease(usize), // Release of a keypad button (1-12)

    // Processing profile selection
    CycleProcessingProfile, // Cycle to next processing profile
    SetProcessingProfile(ProcessingProfile), // Set a specific processing profile

    // Effects parameters
    SetOctave(i8),   // Set octave (low=-1, normal=0, high=1)
    SetBitCrush(i8), // Set bit crush (1=crush1, 0=none, 2=crush2)
    SetFormant(i8),  // Set formant (-1=male, 0=none, 1=female)
    KeyChange(i8),   // Change musical key up or down

    NoOp,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
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
                AppState::EffectsProfile(ProcessingProfile::PitchControl)
            }
            (AppState::Splash, AppEvent::EncoderPress) => {
                AppState::EffectsProfile(ProcessingProfile::PitchControl)
            }

            // Encoder press to toggle between Processing and Effects
            (AppState::EffectsProfile(profile), AppEvent::EncoderPress) => {
                AppState::Processing(profile)
            }
            (AppState::Processing(profile), AppEvent::EncoderPress) => {
                AppState::EffectsProfile(profile)
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
            AppState::EffectsProfile(ProcessingProfile::PitchControl) => {
                AppState::EffectsProfile(ProcessingProfile::Vocode)
            }
            AppState::EffectsProfile(ProcessingProfile::Vocode) => {
                AppState::EffectsProfile(ProcessingProfile::Dry)
            }
            AppState::EffectsProfile(ProcessingProfile::Dry) => {
                AppState::EffectsProfile(ProcessingProfile::Harmony)
            }
            AppState::EffectsProfile(ProcessingProfile::Harmony) => {
                AppState::EffectsProfile(ProcessingProfile::Phone)
            }
            AppState::EffectsProfile(ProcessingProfile::Phone) => {
                AppState::EffectsProfile(ProcessingProfile::PitchControl)
            }
            AppState::Processing(ProcessingProfile::PitchControl) => {
                AppState::Processing(ProcessingProfile::Vocode)
            }
            AppState::Processing(ProcessingProfile::Vocode) => {
                AppState::Processing(ProcessingProfile::Dry)
            }
            AppState::Processing(ProcessingProfile::Dry) => {
                AppState::Processing(ProcessingProfile::Harmony)
            }
            AppState::Processing(ProcessingProfile::Harmony) => {
                AppState::Processing(ProcessingProfile::Phone)
            }
            AppState::Processing(ProcessingProfile::Phone) => {
                AppState::Processing(ProcessingProfile::PitchControl)
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
    current_key: i8,
    current_octave: i8,
    current_bitcrush: i8,
    current_formant: i8,
    current_waveform: i8,
    sample_reduction: i8,
    bit_rate: i8,
    pub volume: i8,
    pub note: i8,
    pub key_down_pressed: bool,
    pub process_cycle_pressed: bool,
    pub key_up_pressed: bool,
}

// Add a snapshot struct to hold all the state information
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AppStateMachineSnapshot {
    pub current_state: AppState,
    pub key: i8,
    pub octave: i8,
    pub note: i8,
    pub volume: i8,
    pub crush: i8,
    pub sample_reduction: i8,
    pub bit_rate: i8,
    pub formant: i8,
    pub autotune_speed: i8,
    pub magnitude: i8,
    pub pad_matrix: i8,
    pub key_down_pressed: bool,
    pub process_cycle_pressed: bool,
    pub key_up_pressed: bool,
    pub waveform: i8,
}

// Add a MenuContext struct for menu display
pub struct MenuContext {
    pub previous_item: (&'static str, i8),
    pub current_item: (&'static str, i8),
    pub next_item: (&'static str, i8),
}

impl Default for AppStateMachine {
    fn default() -> Self {
        Self::new()
    }
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
            current_waveform: 0,
            sample_reduction: 1,
            bit_rate: 32,
            volume: 10,
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
            sample_reduction: self.sample_reduction,
            bit_rate: self.bit_rate,
            formant: self.current_formant,
            autotune_speed: self.values.autotune_speed,
            magnitude: self.values.magnitude,
            pad_matrix: self.values.pad_matrix,
            key_down_pressed: self.key_down_pressed,
            process_cycle_pressed: self.process_cycle_pressed,
            key_up_pressed: self.key_up_pressed,
            waveform: self.current_waveform,
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
                self.play_note(key);
                self.note = key as i8;
            }

            // Handle keypad presses in Effects profile
            (AppState::EffectsProfile(profile), AppEvent::KeypadPress(key)) => {
                match key {
                    // Row 1: Octave controls
                    1 => self.current_octave = 1, // Low
                    2 => self.current_octave = 2, // Normal
                    3 => self.current_octave = 4, // High

                    // Row 2: Bit crush controls
                    4 => {
                        self.current_bitcrush = 1;
                        self.bit_rate = self.values.bit_rate_soft;
                        self.sample_reduction = self.values.sample_reduction_soft;
                    }
                    5 => {
                        self.current_bitcrush = 0;
                        self.bit_rate = 32;
                        self.sample_reduction = 1;
                    }
                    6 => {
                        self.current_bitcrush = 2;
                        self.bit_rate = self.values.bit_rate_harsh;
                        self.sample_reduction = self.values.sample_reduction_harsh;
                    }

                    // Row 3: Formant controls or waveform
                    7..=9 => {
                        match profile {
                            ProcessingProfile::PitchControl | ProcessingProfile::Harmony => {
                                // map 7/8/9 -> male/none/female (or whatever mapping you want)
                                self.current_formant = match key {
                                    7 => 1, // male
                                    8 => 0, // none
                                    9 => 2, // female
                                    _ => 0,
                                };
                            }
                            ProcessingProfile::Vocode
                            | ProcessingProfile::Dry
                            | ProcessingProfile::Phone => {
                                // map 7/8/9 -> waveforms 0/1/2 (example)
                                self.current_waveform = match key {
                                    7 => 0, // triangle
                                    8 => 1, // square
                                    9 => 2, // saw
                                    _ => 0,
                                }
                            }
                        }
                    }

                    //TODO: add an if phone profile and have tone vs drum options

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

            (AppState::Processing(profile), AppEvent::KeypadRelease(_key)) => {
                self.play_note(0); // stop tone for those modes
            }

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
            ("BitRate1", self.values.bit_rate_soft),
            ("BitRate2", self.values.bit_rate_harsh),
            ("SampleRate1", self.values.sample_reduction_soft),
            ("SampleRate2", self.values.sample_reduction_harsh),
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
        self.note = key_index as i8; //self.current_key + (key_index as i8) + (self.current_octave * 12);
                                     // Play the note with current effects
        info!("the note after {}", self.note);
    }

    /// Get a snapshot of all current values
    pub fn get_values(&self) -> MenuValues {
        self.values
    }

    // Get processing-related parameters
    // pub fn get_processing_params(&self) -> (i8, i8, i8, i8) {
    //     (
    //         self.current_key,
    //         self.current_octave,
    //         self.current_crush,
    //         self.current_formant,
    //     )
    // }
}

/// A small helper function to clamp an i8.
fn clamp_value(current: i8, delta: i8, min: i8, max: i8) -> i8 {
    (current + delta).clamp(min, max)
}

#[allow(dead_code)]
fn wrap_value(current: i8, delta: i8, min: i8, max: i8) -> i8 {
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
