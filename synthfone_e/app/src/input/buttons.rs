use libdaisy::{
    gpio::*,
    prelude::{Input, Output, PushPull},
};
use log::info;
use state_machines::AppState;

pub fn scan_button_matrix(
    col_1: &mut Daisy21<Output<PushPull>>,
    col_2: &mut Daisy20<Output<PushPull>>,
    col_3: &mut Daisy19<Output<PushPull>>,
    row_1: &Daisy15<Input>,
    row_2: &Daisy16<Input>,
    row_3: &Daisy17<Input>,
    row_4: &Daisy18<Input>,
) -> [[bool; 3]; 4] {
    let mut state = [[false; 3]; 4];

    // Helper closure to read rows
    let read_rows = |r1: &Daisy15<Input>,
                     r2: &Daisy16<Input>,
                     r3: &Daisy17<Input>,
                     r4: &Daisy18<Input>|
     -> [bool; 4] { [r1.is_low(), r2.is_low(), r3.is_low(), r4.is_low()] };

    // Scan each column with proper delays
    // Column 1
    col_1.set_low();
    col_2.set_high();
    col_3.set_high();
    cortex_m::asm::delay(10000); // Increased delay for better settling
    let col1_rows = read_rows(row_1, row_2, row_3, row_4);

    // Column 2
    col_1.set_high();
    col_2.set_low();
    col_3.set_high();
    cortex_m::asm::delay(10000);
    let col2_rows = read_rows(row_1, row_2, row_3, row_4);

    // Column 3
    col_1.set_high();
    col_2.set_high();
    col_3.set_low();
    cortex_m::asm::delay(10000);
    let col3_rows = read_rows(row_1, row_2, row_3, row_4);

    // Fill state array
    for (r, pressed) in col1_rows.iter().enumerate() {
        state[r][0] = *pressed;
    }
    for (r, pressed) in col2_rows.iter().enumerate() {
        state[r][1] = *pressed;
    }
    for (r, pressed) in col3_rows.iter().enumerate() {
        state[r][2] = *pressed;
    }

    state
}

pub fn handle_button_press(
    row: usize,
    col: usize,
    current_state: AppState,
) -> state_machines::AppEvent {
    // Temporarily use direct mapping to see what's actually happening
    let actual_col = 2 - col; // This reverses: 0→2, 1→1, 2→0
    let key_num = row * 3 + actual_col + 1;

    info!(
        "Button pressed - Row: {}, Col: {}, Key: {}",
        row, col, key_num
    );

    match current_state {
        // In Processing state - buttons are notes or key changes
        AppState::Processing(_) => state_machines::AppEvent::KeypadPress(key_num),

        // In Effects state - buttons control effects
        AppState::EffectsProfile(_) => {
            info!("In Processing state, sending KeypadPress({})", key_num);
            state_machines::AppEvent::KeypadPress(key_num)
        }

        // In Menu state - buttons go back to processing
        AppState::Menu(_, _) => {
            state_machines::AppEvent::EncoderDoublePress // Exit menu on any button press
        }

        // In Splash state - any button exits splash
        AppState::Splash => state_machines::AppEvent::SplashComplete,
    }
}

pub fn handle_button_release(
    row: usize,
    col: usize,
    current_state: AppState,
) -> state_machines::AppEvent {
    // Use the same mapping as press
    let actual_col = 2 - col;
    let key_num = row * 3 + actual_col + 1;

    match current_state {
        AppState::Processing(_) | AppState::EffectsProfile(_) => {
            state_machines::AppEvent::KeypadRelease(key_num)
        }
        _ => state_machines::AppEvent::NoOp,
    }
}
