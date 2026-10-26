//! Driver for active-low heating relays (LOW = ON, HIGH = OFF).

use esp_idf_svc::hal::gpio::{Output, OutputPin, PinDriver};
use esp_idf_svc::sys::{esp, gpio_set_level, EspError};
use log::info;

pub struct Relay<'d> {
    name: &'static str,
    pin: PinDriver<'d, Output>,
}

impl<'d> Relay<'d> {
    /// Takes ownership of `pin` and configures it as an output with the relay OFF.
    ///
    /// The GPIO output latch is set HIGH (OFF) *before* the output driver is
    /// enabled, so the relay never sees a LOW (ON) pulse. (`PinDriver::output()`
    /// alone would briefly drive the pin LOW, because the latch is 0 after reset.)
    pub fn new(name: &'static str, pin: impl OutputPin + 'd) -> Result<Self, EspError> {
        // SAFETY: we exclusively own `pin`; this only writes its output latch,
        // and the output driver is not enabled yet.
        esp!(unsafe { gpio_set_level(pin.pin().into(), 1) })?;
        let pin = PinDriver::output(pin)?;

        info!("Relay '{name}' initialised on GPIO{} (OFF)", pin.pin());
        Ok(Self { name, pin })
    }

    pub fn on(&mut self) -> Result<(), EspError> {
        let was_on = self.is_on();
        self.pin.set_low()?;
        if !was_on {
            info!("Relay '{}' switched ON", self.name);
        }
        Ok(())
    }

    pub fn off(&mut self) -> Result<(), EspError> {
        let was_on = self.is_on();
        self.pin.set_high()?;
        if was_on {
            info!("Relay '{}' switched OFF", self.name);
        }
        Ok(())
    }

    pub fn set(&mut self, active: bool) -> Result<(), EspError> {
        if active {
            self.on()
        } else {
            self.off()
        }
    }

    pub fn is_on(&self) -> bool {
        self.pin.is_set_low()
    }
}
