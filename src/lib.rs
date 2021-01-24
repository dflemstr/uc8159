#![cfg_attr(not(feature = "std"), no_std)]
use core::convert;
use core::marker;
use core::mem;
use core::slice;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    White = 1,
    Green = 2,
    Blue = 3,
    Red = 4,
    Yellow = 5,
    Orange = 6,
    Clean = 7,
}

#[derive(Clone, Debug)]
pub struct Palette([[u8; 3]; 7]);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Config {
    pub border_color: Color,
}

// Currently hard-coded behavior for Pimoroni Inky Impression
const WIDTH: usize = 600;
const HEIGHT: usize = 448;

const SPI_CHUNK_SIZE: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
enum Command {
    PSR = 0x00,
    PWR = 0x01,
    POF = 0x02,
    PFS = 0x03,
    PON = 0x04,
    // BTST = 0x06,
    // DSLP = 0x07,
    DTM1 = 0x10,
    // DSP = 0x11,
    DRF = 0x12,
    // IPC = 0x13,
    PLL = 0x30,
    // TSC = 0x40,
    TSE = 0x41,
    // TSW = 0x42,
    // TSR = 0x43,
    CDI = 0x50,
    // LPD = 0x51,
    TCON = 0x60,
    TRES = 0x61,
    DAM = 0x65,
    // REV = 0x70,
    // FLG = 0x71,
    // AMV = 0x80,
    // VV = 0x81,
    // VDCS = 0x82,
    PWS = 0xE3,
    // TSSET = 0xE5,
}

#[derive(Debug)]
pub struct Display<SPI, TIMER, RESET, BUSY, DC, ERR = convert::Infallible>
where
    SPI: embedded_hal::blocking::spi::Write<u8>,
    TIMER: embedded_hal::blocking::delay::DelayMs<u16>,
    RESET: embedded_hal::digital::v2::OutputPin,
    BUSY: embedded_hal::digital::v2::InputPin,
    DC: embedded_hal::digital::v2::OutputPin,
    ERR: From<SPI::Error> + From<RESET::Error> + From<BUSY::Error> + From<DC::Error>,
{
    spi: SPI,
    delay: TIMER,
    reset: RESET,
    busy: BUSY,
    dc: DC,
    config: Config,
    buffer: [u8; WIDTH / 2 * HEIGHT],
    phantom: marker::PhantomData<ERR>,
}

impl<SPI, DELAY, RESET, BUSY, DC, ERR> Display<SPI, DELAY, RESET, BUSY, DC, ERR>
where
    SPI: embedded_hal::blocking::spi::Write<u8>,
    DELAY: embedded_hal::blocking::delay::DelayMs<u16>,
    RESET: embedded_hal::digital::v2::OutputPin,
    BUSY: embedded_hal::digital::v2::InputPin,
    DC: embedded_hal::digital::v2::OutputPin,
    ERR: From<SPI::Error> + From<RESET::Error> + From<BUSY::Error> + From<DC::Error>,
{
    pub fn new(spi: SPI, delay: DELAY, reset: RESET, busy: BUSY, dc: DC, config: Config) -> Self {
        let phantom = marker::PhantomData;
        let buffer = [0; WIDTH / 2 * HEIGHT];

        Self {
            spi,
            delay,
            reset,
            busy,
            dc,
            config,
            buffer,
            phantom,
        }
    }

    pub fn width(&self) -> usize {
        WIDTH
    }

    pub fn height(&self) -> usize {
        HEIGHT
    }

    pub fn fill(&mut self, color: Color) {
        self.buffer = [((color as u8) << 4) | color as u8; WIDTH / 2 * HEIGHT];
    }

    pub fn copy_from(&mut self, color: &[Color]) {
        for (idx, cell) in color.chunks(2).enumerate() {
            self.buffer[idx] = ((cell[0] as u8) << 4) | cell[1] as u8;
        }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, color: Color) {
        let cell = &mut self.buffer[y * WIDTH / 2 + x / 2];
        if (x & 1) == 0 {
            *cell = (*cell & 0b00001111) | ((color as u8) << 4);
        } else {
            *cell = (*cell & 0b11110000) | color as u8;
        }
    }

    pub fn show(&mut self) -> Result<(), ERR> {
        self.setup()?;

        let ptr = &self.buffer as *const _ as *const u8;
        let len = mem::size_of_val(&self.buffer);
        let data = unsafe { slice::from_raw_parts(ptr, len) };

        Self::send_command(&mut self.spi, &mut self.dc, Command::DTM1, data)?;
        self.busy_wait()?;

        Self::send_command(&mut self.spi, &mut self.dc, Command::PON, &[])?;
        self.busy_wait()?;

        Self::send_command(&mut self.spi, &mut self.dc, Command::DRF, &[])?;
        self.busy_wait()?;

        Self::send_command(&mut self.spi, &mut self.dc, Command::POF, &[])?;
        self.busy_wait()?;
        Ok(())
    }

    fn setup(&mut self) -> Result<(), ERR> {
        self.reset.set_low()?;
        self.delay.delay_ms(100);
        self.reset.set_high()?;
        self.delay.delay_ms(100);

        self.busy_wait()?;

        let width_bytes = (WIDTH as u16).to_be_bytes();
        let height_bytes = (HEIGHT as u16).to_be_bytes();
        Self::send_command(
            &mut self.spi,
            &mut self.dc,
            Command::TRES,
            &[
                width_bytes[0],
                width_bytes[1],
                height_bytes[0],
                height_bytes[1],
            ],
        )?;

        // Panel Setting
        // 0b11000000 = Resolution select, 0b00 = 640x480, our panel is 0b11 = 600x448
        // 0b00100000 = LUT selection, 0 = ext flash, 1 = registers, we use ext flash
        // 0b00010000 = Ignore
        // 0b00001000 = Gate scan direction, 0 = down, 1 = up (default)
        // 0b00000100 = Source shift direction, 0 = left, 1 = right (default)
        // 0b00000010 = DC-DC converter, 0 = off, 1 = on
        // 0b00000001 = Soft reset, 0 = Reset, 1 = Normal (Default)
        Self::send_command(
            &mut self.spi,
            &mut self.dc,
            Command::PSR,
            &[
                0b11101111, // See above for more magic numbers
                0x08,       // display_colours == UC8159_7C
            ],
        )?;

        Self::send_command(
            &mut self.spi,
            &mut self.dc,
            Command::PWR,
            &[
                (0x06 << 3) |  // ??? - not documented in UC8159 datasheet
                    (0x01 << 2) |  // SOURCE_INTERNAL_DC_DC
                    (0x01 << 1) |  // GATE_INTERNAL_DC_DC
                    (0x01), // LV_SOURCE_INTERNAL_DC_DC
                0x00, // VGx_20V
                0x23, // UC8159_7C
                0x23, // UC8159_7C
            ],
        )?;

        // Set the PLL clock frequency to 50Hz
        // 0b11000000 = Ignore
        // 0b00111000 = M
        // 0b00000111 = N
        // PLL = 2MHz * (M / N)
        // PLL = 2MHz * (7 / 4)
        // PLL = 2,800,000 ???
        Self::send_command(&mut self.spi, &mut self.dc, Command::PLL, &[0x3C])?;

        Self::send_command(&mut self.spi, &mut self.dc, Command::TSE, &[0x00])?;

        // VCOM and Data Interval setting
        // 0b11100000 = Vborder control (0b001 = LUTB voltage)
        // 0b00010000 = Data polarity
        // 0b00001111 = Vcom and data interval (0b0111 = 10, default)
        Self::send_command(
            &mut self.spi,
            &mut self.dc,
            Command::CDI,
            &[((self.config.border_color as u8) << 5) | 0x17],
        )?;

        // Gate/Source non-overlap period
        // 0b11110000 = Source to Gate (0b0010 = 12nS, default)
        // 0b00001111 = Gate to Source
        Self::send_command(&mut self.spi, &mut self.dc, Command::TCON, &[0x22])?;

        // Disable external flash
        Self::send_command(&mut self.spi, &mut self.dc, Command::DAM, &[0b00000000])?;

        // UC8159_7C
        Self::send_command(&mut self.spi, &mut self.dc, Command::PWS, &[0xAA])?;

        // Power off sequence
        // 0b00110000 = power off sequence of VDH and VDL, 0b00 = 1 frame (default)
        // All other bits ignored?
        Self::send_command(
            &mut self.spi,
            &mut self.dc,
            Command::PFS,
            &[0b00000000], // PFS_1_FRAME
        )?;
        Ok(())
    }

    fn busy_wait(&mut self) -> Result<(), ERR> {
        while self.busy.is_low()? {
            self.delay.delay_ms(10);
        }
        Ok(())
    }

    fn send_command(spi: &mut SPI, dc: &mut DC, command: Command, data: &[u8]) -> Result<(), ERR> {
        dc.set_low()?;
        spi.write(&[command as u8])?;
        if !data.is_empty() {
            dc.set_high()?;
            for chunk in data.chunks(SPI_CHUNK_SIZE) {
                spi.write(chunk)?;
            }
        }
        Ok(())
    }
}

impl Color {
    pub fn all() -> [Self; 8] {
        [
            Color::Black,
            Color::White,
            Color::Green,
            Color::Blue,
            Color::Red,
            Color::Yellow,
            Color::Orange,
            Color::Clean,
        ]
    }

    pub fn all_significant() -> [Self; 7] {
        [
            Color::Black,
            Color::White,
            Color::Green,
            Color::Blue,
            Color::Red,
            Color::Yellow,
            Color::Orange,
        ]
    }

    pub fn palette(saturation: f32) -> Palette {
        let all_significant = Self::all_significant();
        let mut colors = [[0; 3]; 7];
        for (idx, color) in all_significant.iter().copied().enumerate() {
            let [rs, gs, bs] = color.as_rgb_saturated();
            let [rd, gd, bd] = color.as_rgb_desaturated();
            let r_corr = (rs as f32 * saturation + rd as f32 * (1.0 - saturation)) as u8;
            let g_corr = (gs as f32 * saturation + gd as f32 * (1.0 - saturation)) as u8;
            let b_corr = (bs as f32 * saturation + bd as f32 * (1.0 - saturation)) as u8;

            colors[idx] = [r_corr, g_corr, b_corr];
        }
        Palette(colors)
    }

    fn as_rgb_desaturated(self) -> [u8; 3] {
        match self {
            Color::Black => [0, 0, 0],
            Color::White => [255, 255, 255],
            Color::Green => [0, 255, 0],
            Color::Blue => [0, 0, 255],
            Color::Red => [255, 0, 0],
            Color::Yellow => [255, 255, 0],
            Color::Orange => [255, 140, 0],
            Color::Clean => [255, 255, 255],
        }
    }

    fn as_rgb_saturated(self) -> [u8; 3] {
        match self {
            Color::Black => [57, 48, 57],
            Color::White => [255, 255, 255],
            Color::Green => [58, 91, 70],
            Color::Blue => [61, 59, 94],
            Color::Red => [156, 72, 75],
            Color::Yellow => [208, 190, 71],
            Color::Orange => [77, 106, 73],
            Color::Clean => [255, 255, 255],
        }
    }
}

impl Palette {
    pub fn closest_color(&self, r: u8, g: u8, b: u8) -> Color {
        let idx = self
            .0
            .iter()
            .enumerate()
            .min_by_key(|(_, &[pr, pg, pb])| {
                let dr = if pr > r { pr - r } else { r - pr } as u32;
                let dg = if pg > g { pg - g } else { g - pg } as u32;
                let db = if pb > b { pb - b } else { b - pb } as u32;
                dr * dr + dg * dg + db * db
            })
            .unwrap()
            .0;
        Color::all()[idx]
    }
}
