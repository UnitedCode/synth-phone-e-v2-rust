pub struct CircularBuffer<T, const N: usize> {
    buffer: [T; N],
    read_index: usize,
    write_index: usize,
    hop_pointer: usize,
    hop_size: usize,
    default_value: T,
}

impl<T, const N: usize> CircularBuffer<T, N>
where
    T: Copy + core::ops::AddAssign + core::fmt::Debug,
{
    pub fn new(default_value: T, hop_size: Option<usize>) -> CircularBuffer<T, N> {
        let hop_size = hop_size.unwrap_or(0);

        CircularBuffer {
            buffer: [default_value; N],
            read_index: 0,
            write_index: 0,
            hop_size,
            hop_pointer: 0,
            default_value,
        }
    }

    fn increment_index(&mut self, index: usize) -> usize {
        (index + 1) % N
    }

    pub fn read(&mut self) -> T {
        let current_index = self.read_index;
        self.read_index = self.increment_index(self.read_index);

        self.buffer[current_index]
    }

    pub fn write(&mut self, value: T) -> () {
        self.buffer[self.write_index] = value;

        //if we are at the max buffer size, circle back to 0
        self.write_index = self.increment_index(self.write_index);
    }

    pub fn read_and_reset(&mut self) -> T {
        // Check that read isn't past hop pointer
        let value = self.buffer[self.read_index];
        self.buffer[self.read_index] = self.default_value;

        self.read_index = self.increment_index(self.read_index);

        value
    }

    pub fn add_value(&mut self, value: T) {
        self.buffer[self.write_index] += value;
        self.write_index = self.increment_index(self.write_index);
    }

    pub fn next_hop(&mut self) {
      let hop_index = (self.hop_pointer + self.hop_size) % self.buffer.len();
      self.hop_pointer = hop_index;
      self.write_index = hop_index;

    }

    pub fn push_read_back(&mut self, window_size: usize) {
        let push_back = ((self.read_index as isize - window_size as isize + self.buffer.len() as isize) % self.buffer.len() as isize) as usize;
        self.read_index = push_back;
    }
}

// TODO add test