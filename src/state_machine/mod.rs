/// The top-level states:

// New state machine structure only
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProcessingProfile {
    PitchControl,
    Vocode,
    Dry,
    Harmony,
    Percussion,
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

            /// Get the (min, max) range for a menu item
            pub fn range(item: MenuItem) -> (i8, i8) {
                match item {
                    $(MenuItem::$variant => ($min, $max),)*
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
    PitchLow     => { field: pitch_low,               name: "Pitch Low",     min: -12, max: 0 },
    PitchHigh    => { field: pitch_high,              name: "Pitch High",    min: 0, max: 12 },
    BitRate1     => { field: bit_rate_soft,           name: "Bit Rate 1",    min: 4, max: 32 },
    BitRate2     => { field: bit_rate_harsh,          name: "Bit Rate 2",    min: 4, max: 32 },
    SampleRate1  => { field: sample_reduction_soft,   name: "Sample Rate 1", min: 1, max: 32 },
    SampleRate2  => { field: sample_reduction_harsh,  name: "Sample Rate 2", min: 1, max: 32 },
    FormantMale  => { field: formant_male,            name: "Formant Male",  min: 1, max: 10 },
    FormantFemale => { field: formant_female,         name: "Formant Female", min: 1, max: 10 },
    AutotuneSpeed => { field: autotune_speed,         name: "Autotune Speed", min: 1, max: 10 },
    Magnitude    => { field: magnitude,               name: "Magnitude",     min: 1, max: 10 },
    PadMatrix    => { field: pad_matrix,              name: "Pad Matrix",    min: 0, max: 1  },
}

/// Storage for all adjustable menu values
#[derive(Debug, Clone, Copy)]
pub struct MenuValues {
    /// Semitone offset of the low pitch preset (-12..0, 0 = no shift)
    pub pitch_low: i8,
    /// Semitone offset of the high pitch preset (0..12, 0 = no shift)
    pub pitch_high: i8,
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
            pitch_low: -12,
            pitch_high: 12,
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
                AppState::EffectsProfile(ProcessingProfile::Percussion)
            }
            AppState::EffectsProfile(ProcessingProfile::Percussion) => {
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
                AppState::Processing(ProcessingProfile::Percussion)
            }
            AppState::Processing(ProcessingProfile::Percussion) => {
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
    /// 0 = low, 1 = normal, 2 = high — actual value derived from menu in snapshot()
    current_octave_preset: i8,
    current_bitcrush: i8,
    current_formant: i8,
    current_waveform: i8,
    current_percussion: i8,
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
    /// Active pitch offset in semitones (0 = no shift), from the selected preset
    pub pitch_semitones: i8,
    /// 0=low, 1=normal, 2=high — for display sprite selection only
    pub octave_preset: i8,
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
    pub percussion: i8,
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
            current_octave_preset: 1, // normal
            current_bitcrush: 0,
            current_formant: 0,
            current_waveform: 0,
            current_percussion: 1,
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
        let pitch_semitones = match self.current_octave_preset {
            0 => self.values.pitch_low,
            2 => self.values.pitch_high,
            _ => 0,
        };
        let (sample_reduction, bit_rate) = match self.current_bitcrush {
            1 => (self.values.sample_reduction_soft, self.values.bit_rate_soft),
            2 => (
                self.values.sample_reduction_harsh,
                self.values.bit_rate_harsh,
            ),
            _ => (1, 32),
        };
        AppStateMachineSnapshot {
            current_state: self.state,
            key: self.current_key,
            pitch_semitones,
            octave_preset: self.current_octave_preset,
            note: self.note,
            volume: self.volume,
            crush: self.current_bitcrush,
            sample_reduction,
            bit_rate,
            formant: self.current_formant,
            autotune_speed: self.values.autotune_speed,
            magnitude: self.values.magnitude,
            pad_matrix: self.values.pad_matrix,
            key_down_pressed: self.key_down_pressed,
            process_cycle_pressed: self.process_cycle_pressed,
            key_up_pressed: self.key_up_pressed,
            waveform: self.current_waveform,
            percussion: self.current_percussion,
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
                    // Row 1: Octave
                    1 => self.current_octave_preset = 0,
                    2 => self.current_octave_preset = 1,
                    3 => self.current_octave_preset = 2,

                    // Row 2: Bit crush
                    4 => self.current_bitcrush = 1,
                    5 => self.current_bitcrush = 0,
                    6 => self.current_bitcrush = 2,

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
                            ProcessingProfile::Vocode | ProcessingProfile::Dry => {
                                // map 7/8/9 -> waveforms 0/1/2 (example)
                                self.current_waveform = match key {
                                    7 => 0, // triangle
                                    8 => 1, // square
                                    9 => 2, // saw
                                    _ => 0,
                                }
                            }
                            ProcessingProfile::Percussion => {
                                // map 7/8/9 -> percussion types 1/2/3 (example)
                                self.current_percussion = match key {
                                    7 => 0, // dial tones
                                    8 => 1, // ringer
                                    9 => 2, // drums
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
            ("Pitch Low", self.values.pitch_low),
            ("Pitch High", self.values.pitch_high),
            ("Bit Rate 1", self.values.bit_rate_soft),
            ("Bit Rate 2", self.values.bit_rate_harsh),
            ("Sample Rate 1", self.values.sample_reduction_soft),
            ("Sample Rate 2", self.values.sample_reduction_harsh),
            ("Formant Male", self.values.formant_male),
            ("Formant Female", self.values.formant_female),
            ("Speed", self.values.autotune_speed),
            ("Magnitude", self.values.magnitude),
            ("Pad Matrix", self.values.pad_matrix),
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

    /// Sets output volume from an incoming MIDI CC7 value (0-127), scaled to the
    /// internal 0-10 range shared with the encoder-driven volume control.
    pub fn set_volume_from_midi(&mut self, value: u8) {
        self.volume = ((value as i16 * 10) / 127) as i8;
    }

    /// Sets waveform from an incoming MIDI Program Change value, wrapped onto
    /// the three waveforms (0=Triangle, 1=Square, 2=Saw). Drum-channel program
    /// changes select drum kits, not waveforms, so those are ignored.
    pub fn set_waveform_from_midi(&mut self, channel: u8, program: u8) {
        if channel == 9 {
            return;
        }
        self.current_waveform = (program % 3) as i8;
    }

    /// Sets one of the fine-tune menu values (pitch presets, bit rate,
    /// sample rate, formant ratios, magnitude) from an incoming MIDI CC on
    /// 34-42, scaling the 0-127 CC value proportionally into that item's
    /// range.
    pub fn set_menu_value_from_cc(&mut self, controller: u8, value: u8) {
        let item = match controller {
            34 => MenuItem::PitchLow,
            35 => MenuItem::PitchHigh,
            36 => MenuItem::BitRate1,
            37 => MenuItem::BitRate2,
            38 => MenuItem::SampleRate1,
            39 => MenuItem::SampleRate2,
            40 => MenuItem::FormantMale,
            41 => MenuItem::FormantFemale,
            42 => MenuItem::Magnitude,
            _ => return,
        };
        let (min, max) = MenuValues::range(item);
        let span = (max - min) as i16;
        let scaled = min as i16 + (value as i16 * span) / 127;
        self.values.set(item, scaled as i8);
    }

    /// Cycles the musical key up one step (wrapping) from an incoming MIDI
    /// CC43. Only the press half (value >= 64) of a momentary button cycles,
    /// so the release (value 0) doesn't step the key a second time.
    pub fn cycle_key_from_midi(&mut self, value: u8) {
        if value >= 64 {
            self.current_key = (self.current_key + 1) % 24;
        }
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

/// Maps the 12-key phone keypad to GM1 drum note numbers.
/// Layout prioritises kick/snare/hat in the top row for ergonomic live playing.
///
/// Physical layout (col order is reversed in scan; key_num = row*3 + (2-col) + 1):
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

    fn effects_app() -> AppStateMachine {
        let mut app = AppStateMachine::new();
        app.handle_event(AppEvent::SplashComplete);
        app
    }

    #[test]
    fn test_splash_to_effects_profile() {
        let mut app = AppStateMachine::new();
        assert!(matches!(app.state(), AppState::Splash));
        app.handle_event(AppEvent::SplashComplete);
        assert!(matches!(
            app.state(),
            AppState::EffectsProfile(ProcessingProfile::PitchControl)
        ));
    }

    #[test]
    fn test_toggle_effects() {
        let mut app = effects_app();

        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::Processing(ProcessingProfile::PitchControl)
        ));

        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::EffectsProfile(ProcessingProfile::PitchControl)
        ));
    }

    #[test]
    fn test_menu_enter_and_exit() {
        let mut app = effects_app();

        app.handle_event(AppEvent::EncoderDoublePress);
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Selecting(0), _)
        ));

        app.handle_event(AppEvent::EncoderRotate(1));
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Selecting(1), _)
        ));

        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Editing(1), _)
        ));

        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(
            app.state(),
            AppState::Menu(MenuState::Selecting(1), _)
        ));

        app.handle_event(AppEvent::EncoderDoublePress);
        assert!(matches!(app.state(), AppState::Processing(_)));
    }

    #[test]
    fn test_pitch_low_default() {
        let app = AppStateMachine::new();
        assert_eq!(app.get_values().pitch_low, -12);
    }

    #[test]
    fn test_pitch_high_default() {
        let app = AppStateMachine::new();
        assert_eq!(app.get_values().pitch_high, 12);
    }

    #[test]
    fn test_button1_uses_pitch_low_value() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(1));
        assert_eq!(app.snapshot().pitch_semitones, -12);
    }

    #[test]
    fn test_button2_always_no_pitch_shift() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(2));
        assert_eq!(app.snapshot().pitch_semitones, 0);
    }

    #[test]
    fn test_button3_uses_pitch_high_value() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(3));
        assert_eq!(app.snapshot().pitch_semitones, 12);
    }

    #[test]
    fn test_octave_preset_index_low() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(1));
        assert_eq!(app.snapshot().octave_preset, 0);
    }

    #[test]
    fn test_octave_preset_index_normal() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(2));
        assert_eq!(app.snapshot().octave_preset, 1);
    }

    #[test]
    fn test_octave_preset_index_high() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(3));
        assert_eq!(app.snapshot().octave_preset, 2);
    }

    #[test]
    fn test_pitch_low_live_update() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(1));
        assert_eq!(app.snapshot().pitch_semitones, -12);

        // PitchLow is index 0 — edit without re-pressing the button
        app.handle_event(AppEvent::EncoderDoublePress);
        app.handle_event(AppEvent::EncoderPress);
        app.handle_event(AppEvent::EncoderRotate(1)); // -12 → -11
        app.handle_event(AppEvent::EncoderDoublePress);

        assert_eq!(app.snapshot().pitch_semitones, -11);
    }

    #[test]
    fn test_pitch_high_live_update() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(3));
        assert_eq!(app.snapshot().pitch_semitones, 12);

        // PitchHigh is index 1
        app.handle_event(AppEvent::EncoderDoublePress);
        app.handle_event(AppEvent::EncoderRotate(1));
        app.handle_event(AppEvent::EncoderPress);
        app.handle_event(AppEvent::EncoderRotate(-1)); // 12 → 11
        app.handle_event(AppEvent::EncoderDoublePress);

        assert_eq!(app.snapshot().pitch_semitones, 11);

        // Clamped at the top of the range
        app.handle_event(AppEvent::EncoderDoublePress);
        app.handle_event(AppEvent::EncoderRotate(1));
        app.handle_event(AppEvent::EncoderPress);
        app.handle_event(AppEvent::EncoderRotate(1)); // 11 → 12
        app.handle_event(AppEvent::EncoderRotate(1)); // clamped at 12
        app.handle_event(AppEvent::EncoderDoublePress);

        assert_eq!(app.snapshot().pitch_semitones, 12);
    }

    #[test]
    fn test_pitch_cc_scaling() {
        let mut app = AppStateMachine::new();
        // CC 34 spans the full PitchLow range: -12..0
        app.set_menu_value_from_cc(34, 0);
        assert_eq!(app.get_values().pitch_low, -12);
        app.set_menu_value_from_cc(34, 127);
        assert_eq!(app.get_values().pitch_low, 0);
        app.set_menu_value_from_cc(34, 64);
        assert_eq!(app.get_values().pitch_low, -6);

        // CC 35 spans the full PitchHigh range: 0..12
        app.set_menu_value_from_cc(35, 0);
        assert_eq!(app.get_values().pitch_high, 0);
        app.set_menu_value_from_cc(35, 127);
        assert_eq!(app.get_values().pitch_high, 12);
    }

    #[test]
    fn test_crush1_live_update_bit_rate() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(4));
        assert_eq!(app.snapshot().bit_rate, 32);

        // BitRate1 is index 2 (after PitchLow, PitchHigh)
        app.handle_event(AppEvent::EncoderDoublePress);
        app.handle_event(AppEvent::EncoderRotate(1));
        app.handle_event(AppEvent::EncoderRotate(1));
        app.handle_event(AppEvent::EncoderPress);
        app.handle_event(AppEvent::EncoderRotate(-1)); // 32 → 31
        app.handle_event(AppEvent::EncoderDoublePress);

        assert_eq!(app.snapshot().bit_rate, 31);
        assert_eq!(app.snapshot().crush, 1);
    }

    #[test]
    fn test_crush2_live_update_sample_reduction() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(6));
        assert_eq!(app.snapshot().sample_reduction, 16);

        // SampleRate2 is index 5
        app.handle_event(AppEvent::EncoderDoublePress);
        for _ in 0..5 {
            app.handle_event(AppEvent::EncoderRotate(1));
        }
        app.handle_event(AppEvent::EncoderPress);
        app.handle_event(AppEvent::EncoderRotate(-1)); // 16 → 15
        app.handle_event(AppEvent::EncoderDoublePress);

        assert_eq!(app.snapshot().sample_reduction, 15);
        assert_eq!(app.snapshot().crush, 2);
    }

    #[test]
    fn test_crush_none_always_hardcoded() {
        let mut app = effects_app();
        app.handle_event(AppEvent::KeypadPress(5));
        assert_eq!(app.snapshot().bit_rate, 32);
        assert_eq!(app.snapshot().sample_reduction, 1);
        assert_eq!(app.snapshot().crush, 0);
    }

    #[test]
    fn test_formant_male_default() {
        let app = AppStateMachine::new();
        assert_eq!(app.get_values().formant_male, 5);
    }

    #[test]
    fn test_formant_female_default() {
        let app = AppStateMachine::new();
        assert_eq!(app.get_values().formant_female, 5);
    }

    #[test]
    fn test_formant_male_can_be_changed_via_menu() {
        let mut app = effects_app();

        // FormantMale is at index 6
        app.handle_event(AppEvent::EncoderDoublePress);
        for _ in 0..6 {
            app.handle_event(AppEvent::EncoderRotate(1));
        }
        app.handle_event(AppEvent::EncoderPress);
        app.handle_event(AppEvent::EncoderRotate(1)); // 5 → 6
        app.handle_event(AppEvent::EncoderDoublePress);

        assert_eq!(app.get_values().formant_male, 6);
    }

    #[test]
    fn test_formant_female_can_be_changed_via_menu() {
        let mut app = effects_app();

        // FormantFemale is at index 7
        app.handle_event(AppEvent::EncoderDoublePress);
        for _ in 0..7 {
            app.handle_event(AppEvent::EncoderRotate(1));
        }
        app.handle_event(AppEvent::EncoderPress);
        app.handle_event(AppEvent::EncoderRotate(1)); // 5 → 6
        app.handle_event(AppEvent::EncoderDoublePress);

        assert_eq!(app.get_values().formant_female, 6);
    }
}
