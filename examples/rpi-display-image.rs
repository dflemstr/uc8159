use rppal::gpio;
use rppal::hal;
use rppal::spi;
use std::env;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("unknown")]
    Unknown,
    #[error("SPI")]
    Spi(#[from] rppal::spi::Error),
}

fn main() -> anyhow::Result<()> {
    let spi = spi::Spi::new(
        spi::Bus::Spi0,
        spi::SlaveSelect::Ss0,
        3_000_000,
        spi::Mode::Mode0,
    )?;
    let gpio = gpio::Gpio::new()?;
    let delay = hal::Delay::new();
    let reset = gpio.get(27)?.into_output();
    let busy = gpio.get(17)?.into_input();
    let dc = gpio.get(22)?.into_output();

    let mut display = uc8159::Display::<_, _, _, _, _, Error>::new(
        spi,
        delay,
        reset,
        busy,
        dc,
        uc8159::Config {
            border_color: uc8159::Color::White,
        },
    );

    display.fill(uc8159::Color::White);
    let image_path = env::args_os().nth(1).ok_or_else(|| {
        anyhow::anyhow!("Expected one command line arg with a path to an image file")
    })?;
    let image = image::io::Reader::open(image_path)?
        .decode()?
        .resize(
            display.width() as u32,
            display.height() as u32,
            image::imageops::FilterType::Nearest,
        )
        .to_rgb8();
    let palette = uc8159::Palette::new(1.0);
    eprintln!("palette: {:?}", palette);

    let padding_x = 0.max((display.width() - image.width() as usize) / 2);
    let padding_y = 0.max((display.height() - image.height() as usize) / 2);
    for (x, y, &image::Rgb([r, g, b])) in image.enumerate_pixels() {
        display.set_pixel(
            padding_x + x as usize,
            padding_y + y as usize,
            palette.closest_color(r, g, b),
        );
    }

    display.show()?;

    Ok(())
}

impl From<()> for Error {
    fn from(_: ()) -> Self {
        Error::Unknown
    }
}
