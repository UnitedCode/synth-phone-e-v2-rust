#![cfg_attr(not(test), no_std)]

/// The top-level states:

// New state machine structure only
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProcessingState {
    Autotune,
    Vocode,
    Dry,
}

/// The top-level states
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AppState {
    Splash,
    Processing(ProcessingState),
    EffectsProfile(ProcessingState),
    Menu(MenuState, ProcessingState),
}

/// Menu state with selection/editing modes
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuState {
    Selecting(usize), // Index of the current menu item
    Editing(usize),   // Index of the item being edited
}

/// Menu items available for adjustment
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Crush1,
    Crush2,
    FormantMale,
    FormantFemale,
    AutotuneSpeed,
    Magnitude,
    PadMatrix,
}

/// Array of all menu items for iteration
pub const MENU_ITEMS: [MenuItem; 7] = [
    MenuItem::Crush1,
    MenuItem::Crush2,
    MenuItem::FormantMale,
    MenuItem::FormantFemale,
    MenuItem::AutotuneSpeed,
    MenuItem::Magnitude,
    MenuItem::PadMatrix,
];

/// Storage for all adjustable menu values
#[derive(Debug, Clone, Copy)]
pub struct MenuValues {
    pub crush1: i32,         // 1-10
    pub crush2: i32,         // 1-10
    pub formant_male: i32,   // 1-10
    pub formant_female: i32, // 1-10
    pub autotune_speed: i32, // 1-10
    pub magnitude: i32,      // 1-10
    pub pad_matrix: i32,     // 0 or 1
}

impl Default for MenuValues {
    fn default() -> Self {
        Self {
            crush1: 5,
            crush2: 5,
            formant_male: 5,
            formant_female: 5,
            autotune_speed: 5,
            magnitude: 5,
            pad_matrix: 0,
        }
    }
}

impl MenuValues {
    /// Get a value for a specific menu item
    pub fn get(&self, item: MenuItem) -> i32 {
        match item {
            MenuItem::Crush1 => self.crush1,
            MenuItem::Crush2 => self.crush2,
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
            MenuItem::Crush1 => self.crush1 = value.clamp(1, 10),
            MenuItem::Crush2 => self.crush2 = value.clamp(1, 10),
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
            MenuItem::Crush1 => "Crush 1",
            MenuItem::Crush2 => "Crush 2",
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
    SplashComplete,       // Splash screen animation finished
    EncoderPress,         // Single press of the encoder
    EncoderDoublePress,   // Double press of the encoder
    EncoderRotate(i32),   // Rotation of the encoder (positive or negative)
    
    // Button presses in different modes
    KeypadPress(usize),   // Press of a keypad button (0-11)
    
    // Processing mode selection
    CycleProcessingMode,  // Cycle to next processing mode
    SetProcessingMode(ProcessingMode), // Set a specific processing mode
    
    // Effects parameters
    SetOctave(i32),       // Set octave (low=-1, normal=0, high=1)
    SetBitCrush(i32),     // Set bit crush mode (1=crush1, 0=none, 2=crush2)
    SetFormant(i32),      // Set formant mode (-1=male, 0=none, 1=female)
    KeyChange(i32),       // Change musical key up or down
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
                AppState::Processing(ProcessingMode::Autotune)
            }
            
            // Encoder press to toggle between Processing and Effects
            (AppState::Processing(mode), AppEvent::EncoderPress) => {
                AppState::Effects(mode)
            }
            (AppState::Effects(mode), AppEvent::EncoderPress) => {
                AppState::Processing(mode)
            }
            
            // Double press to enter menu from anywhere
            (AppState::Processing(mode), AppEvent::EncoderDoublePress) => {
                AppState::Menu(MenuState::Selecting(0), mode)
            }
            (AppState::Effects(mode), AppEvent::EncoderDoublePress) => {
                AppState::Menu(MenuState::Selecting(0), mode)
            }
            
            // Double press to exit menu
            (AppState::Menu(_, mode), AppEvent::EncoderDoublePress) => {
                AppState::Processing(mode)
            }
            
            // Handle mode changes in Effects
            (AppState::Effects(_), AppEvent::CycleProcessingMode) => {
                // Implemented separately in cycle_mode function
                self
            }
            (AppState::Effects(_), AppEvent::SetProcessingMode(new_mode)) => {
                AppState::Processing(new_mode)
            }
            
            // Stay in current state for all other events
            _ => self,
        }
    }
    
    /// Helper function to cycle through processing modes
    pub fn cycle_mode(&self) -> Self {
        match self {
            AppState::Effects(ProcessingMode::Autotune) => {
                AppState::Effects(ProcessingMode::Vocode)
            }
            AppState::Effects(ProcessingMode::Vocode) => {
                AppState::Effects(ProcessingMode::Dry)
            }
            AppState::Effects(ProcessingMode::Dry) => {
                AppState::Effects(ProcessingMode::Autotune)
            }
            // If not in Effects state, don't change
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
    pub fn handle_event(
        &self,
        event: MenuNavEvent,
        values: &mut MenuValues,
    ) -> MenuState {
        match (self, event) {
            // Navigation in selection mode
            (MenuState::Selecting(idx), MenuNavEvent::Next) => {
                MenuState::Selecting((idx + 1) % MENU_ITEMS.len())
            }
            (MenuState::Selecting(idx), MenuNavEvent::Prev) => {
                MenuState::Selecting((idx + MENU_ITEMS.len() - 1) % MENU_ITEMS.len())
            }
            (MenuState::Selecting(idx), MenuNavEvent::Select) => {
                MenuState::Editing(*idx)
            }
            
            // Value adjustment in editing mode
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
            (MenuState::Editing(idx), MenuNavEvent::Select) => {
                MenuState::Selecting(*idx)
            }
        }
    }
}

/// Main state machine for the telephone effects box
pub struct TelephoneEffectsBox {
    state: AppState,
    values: MenuValues,
    current_key: i32, // Musical key (0=C, 1=C#, etc.)
    current_octave: i32, // Current octave (-1=low, 0=normal, 1=high)
    current_crush: i32, // Current bit crush (1=crush1, 0=none, 2=crush2)
    current_formant: i32, // Current formant (-1=male, 0=none, 1=female)
}

impl TelephoneEffectsBox {
    /// Create a new instance of the state machine
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            values: MenuValues::default(),
            current_key: 0, // Start in C
            current_octave: 0, // Start at normal octave
            current_crush: 0, // Start with no crush
            current_formant: 0, // Start with no formant
        }
    }
    
    /// Get the current state
    pub fn state(&self) -> AppState {
        self.state
    }
    
    /// Handle incoming events
    pub fn handle_event(&mut self, event: AppEvent) {
        // Handle special events for specific states
        match (&self.state, event) {
            // Handle keypad presses in Processing mode (for notes)
            (AppState::Processing(_), AppEvent::KeypadPress(key)) => {
                if key < 9 {
                    // First 9 buttons are notes
                    self.play_note(key);
                } else if key == 9 {
                    // Lower key
                    self.current_key = (self.current_key + 11) % 12;
                } else if key == 11 {
                    // Raise key
                    self.current_key = (self.current_key + 1) % 12;
                }
                // Key 10 does nothing in Processing mode
            },
            
            // Handle keypad presses in Effects mode
            (AppState::Effects(_), AppEvent::KeypadPress(key)) => {
                match key {
                    // Row 1: Octave controls
                    0 => self.current_octave = -1, // Low
                    1 => self.current_octave = 0,  // Normal
                    2 => self.current_octave = 1,  // High
                    
                    // Row 2: Bit crush controls
                    3 => self.current_crush = 1,   // Crush 1
                    4 => self.current_crush = 0,   // No crush
                    5 => self.current_crush = 2,   // Crush 2
                    
                    // Row 3: Formant controls
                    6 => self.current_formant = -1, // Male
                    7 => self.current_formant = 0,  // None
                    8 => self.current_formant = 1,  // Female
                    
                    // Row 4: Key and mode controls
                    9 => self.current_key = (self.current_key + 11) % 12, // Key down
                    10 => self.state = self.cycle_mode(), // Cycle mode
                    11 => self.current_key = (self.current_key + 1) % 12, // Key up
                    
                    _ => (), // Invalid key
                }
            },
            
            // Handle encoder rotation in Menu mode
            (AppState::Menu(menu_state, _), AppEvent::EncoderRotate(delta)) => {
                // Convert rotation to next/prev events
                let nav_event = if delta > 0 {
                    MenuNavEvent::Next
                } else {
                    MenuNavEvent::Prev
                };
                
                // Update menu state
                let new_menu_state = menu_state.handle_event(nav_event, &mut self.values);
                if let AppState::Menu(_, mode) = self.state {
                    self.state = AppState::Menu(new_menu_state, mode);
                }
                
                return; // Skip the standard transition
            },
            
            // Handle encoder press in Menu mode
            (AppState::Menu(menu_state, _), AppEvent::EncoderPress) => {
                // Toggle between selecting and editing
                let new_menu_state = menu_state.handle_event(MenuNavEvent::Select, &mut self.values);
                if let AppState::Menu(_, mode) = self.state {
                    self.state = AppState::Menu(new_menu_state, mode);
                }
                
                return; // Skip the standard transition
            },
            
            _ => (), // Other events handled by standard transition
        }
        
        // Standard state transition
        self.state = self.state.transition(event);
    }
    
    /// Play a note based on the current key and octave
    fn play_note(&self, key_index: usize) {
        // Convert keypad position to a note in the current key and octave
        // Implementation depends on your audio system
        // This is just a placeholder
        let _note = self.current_key + (key_index as i32) + (self.current_octave * 12);
        // Play the note with current effects
    }
    
    /// Get a snapshot of all current values
    pub fn get_values(&self) -> MenuValues {
        self.values
    }
    
    /// Get processing-related parameters
    pub fn get_processing_params(&self) -> (i32, i32, i32, i32) {
        (self.current_key, self.current_octave, self.current_crush, self.current_formant)
    }
}

/// Unit tests for the state machine
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_splash_to_processing() {
        let mut app = TelephoneEffectsBox::new();
        assert!(matches!(app.state(), AppState::Splash));
        
        app.handle_event(AppEvent::SplashComplete);
        assert!(matches!(app.state(), AppState::Processing(ProcessingMode::Autotune)));
    }
    
    #[test]
    fn test_toggle_effects() {
        let mut app = TelephoneEffectsBox::new();
        app.handle_event(AppEvent::SplashComplete);
        
        // Toggle to Effects
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(app.state(), AppState::Effects(ProcessingMode::Autotune)));
        
        // Toggle back to Processing
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(app.state(), AppState::Processing(ProcessingMode::Autotune)));
    }
    
    #[test]
    fn test_menu_navigation() {
        let mut app = TelephoneEffectsBox::new();
        app.handle_event(AppEvent::SplashComplete);
        
        // Enter menu
        app.handle_event(AppEvent::EncoderDoublePress);
        assert!(matches!(app.state(), AppState::Menu(MenuState::Selecting(0), _)));
        
        // Rotate encoder to next item
        app.handle_event(AppEvent::EncoderRotate(1));
        assert!(matches!(app.state(), AppState::Menu(MenuState::Selecting(1), _)));
        
        // Enter edit mode
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(app.state(), AppState::Menu(MenuState::Editing(1), _)));
        
        // Change value
        app.handle_event(AppEvent::EncoderRotate(1));
        let values = app.get_values();
        assert_eq!(values.crush2, 6); // Default was 5, now 6
        
        // Exit edit mode
        app.handle_event(AppEvent::EncoderPress);
        assert!(matches!(app.state(), AppState::Menu(MenuState::Selecting(1), _)));
        
        // Exit menu
        app.handle_event(AppEvent::EncoderDoublePress);
        assert!(matches!(app.state(), AppState::Processing(ProcessingMode::Autotune)));
    }
    
    #[test]
    fn test_effects_controls() {
        let mut app = TelephoneEffectsBox::new();
        app.handle_event(AppEvent::SplashComplete);
        app.handle_event(AppEvent::EncoderPress); // Enter Effects
        
        // Test octave controls
        app.handle_event(AppEvent::KeypadPress(0)); // Low octave
        let (_, octave, _, _) = app.get_processing_params();
        assert_eq!(octave, -1);
        
        // Test bit crush controls
        app.handle_event(AppEvent::KeypadPress(5)); // Crush 2
        let (_, _, crush, _) = app.get_processing_params();
        assert_eq!(crush, 2);
        
        // Test formant controls
        app.handle_event(AppEvent::KeypadPress(6)); // Male formant
        let (_, _, _, formant) = app.get_processing_params();
        assert_eq!(formant, -1);
        
        // Test cycle mode
        app.handle_event(AppEvent::KeypadPress(10)); // Cycle mode
        assert!(matches!(app.state(), AppState::Effects(ProcessingMode::Vocode)));
        
        app.handle_event(AppEvent::KeypadPress(10)); // Cycle mode again
        assert!(matches!(app.state(), AppState::Effects(ProcessingMode::Dry)));
        
        app.handle_event(AppEvent::KeypadPress(10)); // Cycle mode again
        assert!(matches!(app.state(), AppState::Effects(ProcessingMode::Autotune)));
    }
}
