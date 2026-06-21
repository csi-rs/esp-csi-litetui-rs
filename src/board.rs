//! M5Stack CoreS3 SE board specifics.
//!
//! This hardware shares one SPI bus between the ILI9342C LCD and the microSD
//! slot, and routes some control lines in ways that the generic esp-hal GPIO
//! driver can't express cleanly. The register-level workarounds below are
//! deliberate and load-bearing on this board — do not "simplify" them without
//! hardware to test against.
//!
//! Pin map (CoreS3 SE):
//! | Function | GPIO |
//! |----------|------|
//! | I2C SDA  | 12   |
//! | I2C SCL  | 11   |
//! | SPI SCK  | 36   |
//! | SPI MOSI | 37   |
//! | SPI MISO | 35   |
//! | LCD CS   | 3    |
//! | SD  CS   | 4    |
//!
//! I2C devices: AXP2101 PMIC @ 0x34, AW9523 IO-expander @ 0x58,
//! FT6336 capacitive touch @ 0x38.

use core::convert::Infallible;

use embedded_hal::digital::{ErrorType as DigitalErrorType, OutputPin};
use embedded_hal::i2c::I2c;
use embedded_hal::spi::{ErrorType as SpiErrorType, Operation, SpiDevice};
use embedded_sdmmc::{TimeSource, Timestamp};

/// AXP2101 power-management IC.
const ADDR_PMIC: u8 = 0x34;
/// AW9523 IO expander (controls LCD reset / backlight enable lines).
const ADDR_IO_EXPANDER: u8 = 0x58;
/// FT6336 capacitive-touch controller.
const ADDR_TOUCH: u8 = 0x38;

/// LCD data/command line on GPIO35.
///
/// On the CoreS3 SE the DC line cannot be driven through the normal GPIO
/// matrix while the SD card shares the bus, so it is toggled by writing the
/// GPIO output set/clear registers directly (base `0x6000_4000`, bit 3).
pub struct LcdDcPin;

impl DigitalErrorType for LcdDcPin {
    type Error = Infallible;
}

impl OutputPin for LcdDcPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        unsafe {
            // GPIO_OUT_W1TC + enable
            core::ptr::write_volatile((0x6000_4000 + 0x0018) as *mut u32, 1 << 3);
            core::ptr::write_volatile((0x6000_4000 + 0x0030) as *mut u32, 1 << 3);
        }
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        unsafe {
            // GPIO_OUT_W1TS + enable
            core::ptr::write_volatile((0x6000_4000 + 0x0014) as *mut u32, 1 << 3);
            core::ptr::write_volatile((0x6000_4000 + 0x0030) as *mut u32, 1 << 3);
        }
        Ok(())
    }
}

/// SPI device wrapper for the SD card on the shared bus.
///
/// Re-asserts the GPIO direction for the shared MISO/DC line (bit 3) before
/// each transaction so a preceding LCD transfer can't leave it mis-routed.
pub struct SdSpiDevice<T> {
    pub inner: T,
}

impl<T: SpiErrorType> SpiErrorType for SdSpiDevice<T> {
    type Error = T::Error;
}

impl<T: SpiDevice> SpiDevice for SdSpiDevice<T> {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        unsafe {
            // GPIO_ENABLE_W1TS, bit 3
            core::ptr::write_volatile((0x6000_4000 + 0x0034) as *mut u32, 1 << 3);
        }
        self.inner.transaction(operations)
    }
}

/// Fixed-date [`TimeSource`] for FAT timestamps.
///
/// The board has no battery-backed RTC, so file timestamps are stamped with a
/// fixed build-era date. This is intentional — CSI records carry their own
/// monotonic device timestamp; the FAT mtime is not used for analysis.
pub struct BuildClock;

impl TimeSource for BuildClock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56, // 2026
            zero_indexed_month: 5, // June
            zero_indexed_day: 20,
            hours: 12,
            minutes: 0,
            seconds: 0,
        }
    }
}

/// Bring up display power and release the LCD reset via the PMIC + IO expander.
///
/// Best-effort: every transfer is ignored on error so a missing/edge-case
/// peripheral can't wedge boot. Mirrors the known-good CoreS3 SE sequence.
pub fn init_power_management<I: I2c>(i2c: &mut I) {
    // AXP2101: enable the LCD/backlight rails and set output levels.
    let _ = i2c.write(ADDR_PMIC, &[0x16, 0x07]);
    let _ = i2c.write(ADDR_PMIC, &[0x90, 0xBF]);
    let _ = i2c.write(ADDR_PMIC, &[0x93, 0x1C]);
    let _ = i2c.write(ADDR_PMIC, &[0x95, 0x1C]);
    let _ = i2c.write(ADDR_PMIC, &[0x99, 0x1C]);

    // AW9523: toggle the LCD reset line and drive the backlight-enable pin.
    let mut reg = [0u8; 1];
    let _ = i2c.write_read(ADDR_IO_EXPANDER, &[0x05], &mut reg);
    let _ = i2c.write(ADDR_IO_EXPANDER, &[0x05, reg[0] & !0x02]);
    let _ = i2c.write(ADDR_IO_EXPANDER, &[0x03, reg[0] | 0x02]);

    let mut cfg_p0 = [0u8; 1];
    let _ = i2c.write_read(ADDR_IO_EXPANDER, &[0x04], &mut cfg_p0);
    let _ = i2c.write(ADDR_IO_EXPANDER, &[0x04, cfg_p0[0] & !0x01]);

    let mut out_p0 = [0u8; 1];
    let _ = i2c.write_read(ADDR_IO_EXPANDER, &[0x02], &mut out_p0);
    let _ = i2c.write(ADDR_IO_EXPANDER, &[0x02, out_p0[0] | 0x01]);
}

/// Poll the capacitive-touch controller. Returns `Some((x, y))` in raw
/// 0..320 / 0..240 panel coordinates when a finger is down, else `None`.
pub fn read_touch<I: I2c>(i2c: &mut I) -> Option<(u16, u16)> {
    let mut td = [0u8; 5];
    if i2c.write_read(ADDR_TOUCH, &[0x02], &mut td).is_ok() {
        let touches = td[0] & 0x0F;
        if touches > 0 {
            let x = (((td[1] & 0x0F) as u16) << 8) | td[2] as u16;
            let y = (((td[3] & 0x0F) as u16) << 8) | td[4] as u16;
            return Some((x, y));
        }
    }
    None
}
